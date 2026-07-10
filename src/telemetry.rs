//! telemetry ctx: transcript tail — tokens, context %, ai-title, pending question.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/telemetry.rs
//! Deps:    serde_json; std::fs (called via spawn_blocking by the sensor)
//! Tested:  inline `#[cfg(test)]` — synthetic JSONL lines matching live-verified shapes
//!
//! Key responsibilities:
//! - Locate a session's transcript: `<projects>/<slug(cwd)>/<sessionId>.jsonl`.
//! - Parse the tail (last 256 KiB): last assistant `usage` → context tokens (statusline recipe:
//!   input + cache_read + cache_creation vs 200k), last `ai-title`, unresolved `AskUserQuestion`.
//! - Cache per-session facts keyed by `(size, mtime)` — unchanged file = no re-read.
//!
//! Design constraints:
//! - Tolerant line parsing: unknown types and garbage lines are skipped, never an error
//!   (format is an undocumented internal, 9 CC releases/month).
//! - Numbers are approximate, never a bill (usage is a mid-stream snapshot, #27361).
//! - Never log or store message text — only titles, counts, and flags leave this module.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

/// Context window assumed for ctx% (the statusline default).
pub const CONTEXT_WINDOW: u64 = 200_000;
/// The large window: sessions on 1M-context models routinely exceed 200k tokens (seen live:
/// 408k → a nonsense "204%"). Over-200k usage implies the large window (statusline recipe).
const LARGE_CONTEXT_WINDOW: u64 = 1_000_000;
/// How much of the file tail is parsed.
const TAIL_BYTES: u64 = 256 * 1024;

/// Facts read from a transcript tail.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TailFacts {
    /// Context tokens of the last assistant line (input + cache_read + cache_creation).
    pub context_tokens: Option<u64>,
    /// Last `ai-title` in the tail — the semantic name.
    pub ai_title: Option<String>,
    /// An `AskUserQuestion` tool_use with no matching tool_result at EOF.
    pub pending_question: bool,
}

/// Per-session telemetry: tail facts + transcript activity age.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Telemetry {
    /// Facts from the tail; `None` = transcript missing.
    pub facts: Option<TailFacts>,
    /// Seconds since the transcript last grew; `None` = transcript missing.
    pub secs_since_append: Option<u64>,
}

/// Claude Code's project-dir slug: every char outside `[A-Za-z0-9-]` becomes `-`.
pub fn project_slug(cwd: &str) -> String {
    cwd.replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "-")
}

/// Path of a session's transcript.
pub fn transcript_path(projects_dir: &Path, cwd: &str, session_id: &str) -> PathBuf {
    projects_dir
        .join(project_slug(cwd))
        .join(format!("{session_id}.jsonl"))
}

/// ctx% used: context tokens as a percentage of the inferred window — 200k by default, 1M once
/// usage exceeds 200k (a session can't legitimately sit over its own window; it would compact).
/// Saturating: a hostile/corrupt usage value must never panic the draw (debug overflow checks).
pub const fn ctx_used_pct(context_tokens: u64) -> u64 {
    let window = if context_tokens > CONTEXT_WINDOW {
        LARGE_CONTEXT_WINDOW
    } else {
        CONTEXT_WINDOW
    };
    context_tokens.saturating_mul(100) / window
}

/// Compact token count: `999`, `118k`, `1.2M`.
pub fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        let tenths = tokens / 100_000;
        format!("{}.{}M", tenths / 10, tenths % 10)
    } else if tokens >= 1_000 {
        format!("{}k", tokens / 1_000)
    } else {
        tokens.to_string()
    }
}

/// Parse transcript-tail bytes. `skip_first_line` drops the leading partial line of a mid-file read.
pub fn parse_tail(bytes: &[u8], skip_first_line: bool) -> TailFacts {
    let mut facts = TailFacts::default();
    let mut pending_asks: Vec<String> = Vec::new();
    let lines = bytes
        .split(|&b| b == b'\n')
        .skip(usize::from(skip_first_line));
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Ok(value): Result<Value, _> = serde_json::from_slice(line) else {
            continue; // garbage / truncated line — skip, never fail
        };
        match value.get("type").and_then(Value::as_str) {
            Some("ai-title") => {
                if let Some(t) = value.get("aiTitle").and_then(Value::as_str) {
                    facts.ai_title = Some(t.to_string());
                }
            }
            Some("assistant") => {
                let message = value.get("message");
                if let Some(usage) = message.and_then(|m| m.get("usage")) {
                    // Saturating fold: usage fields are untrusted input; an absurd value must
                    // saturate, never panic (debug overflow checks) — tolerant-parser invariant.
                    let sum = [
                        "input_tokens",
                        "cache_read_input_tokens",
                        "cache_creation_input_tokens",
                    ]
                    .iter()
                    .filter_map(|k| usage.get(k).and_then(Value::as_u64))
                    .fold(0u64, u64::saturating_add);
                    if sum > 0 {
                        facts.context_tokens = Some(sum);
                    }
                }
                for block in content_blocks(message) {
                    if block.get("type").and_then(Value::as_str) == Some("tool_use")
                        && block.get("name").and_then(Value::as_str) == Some("AskUserQuestion")
                    {
                        if let Some(id) = block.get("id").and_then(Value::as_str) {
                            pending_asks.push(id.to_string());
                        }
                    }
                }
            }
            Some("user") => {
                for block in content_blocks(value.get("message")) {
                    if let Some(id) = block.get("tool_use_id").and_then(Value::as_str) {
                        pending_asks.retain(|a| a != id);
                    }
                }
            }
            _ => {} // unknown line types: skip (tolerant by design)
        }
    }
    facts.pending_question = !pending_asks.is_empty();
    facts
}

fn content_blocks(message: Option<&Value>) -> impl Iterator<Item = &Value> {
    message
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .map(|a| a.iter())
        .into_iter()
        .flatten()
}

/// `(size, mtime)`-keyed cache so unchanged transcripts are not re-read every poll.
#[derive(Debug, Default)]
pub struct TailCache {
    entries: HashMap<String, ((u64, SystemTime), TailFacts)>,
}

impl TailCache {
    /// Read a session's telemetry, reusing cached facts when the file is unchanged.
    /// Blocking fs work — call inside `spawn_blocking`.
    pub fn read(&mut self, projects_dir: &Path, cwd: &str, session_id: &str) -> Telemetry {
        let path = transcript_path(projects_dir, cwd, session_id);
        let Ok(meta) = std::fs::metadata(&path) else {
            self.entries.remove(session_id);
            return Telemetry::default();
        };
        let Ok(mtime) = meta.modified() else {
            return Telemetry::default();
        };
        let secs_since_append = SystemTime::now()
            .duration_since(mtime)
            .map(|d| d.as_secs())
            .ok(); // mtime in the future (clock skew) → age unknown
        let stamp = (meta.len(), mtime);
        let facts = match self.entries.get(session_id) {
            Some((cached_stamp, cached)) if *cached_stamp == stamp => cached.clone(),
            _ => match read_tail_facts(&path, meta.len()) {
                Some(facts) => {
                    self.entries
                        .insert(session_id.to_string(), (stamp, facts.clone()));
                    facts
                }
                // Read failed after metadata succeeded (raced away / unreadable). Caching a
                // default here would serve pending_question=false under this stamp FOREVER for
                // a session blocked on a question (blocked = no appends = stamp never moves).
                // Don't cache; the next poll retries.
                None => {
                    return Telemetry {
                        facts: None,
                        secs_since_append,
                    }
                }
            },
        };
        Telemetry {
            facts: Some(facts),
            secs_since_append,
        }
    }

    /// Drop cache entries for sessions no longer live.
    pub fn retain(&mut self, live_ids: &[&str]) {
        self.entries.retain(|id, _| live_ids.contains(&id.as_str()));
    }
}

fn read_tail_facts(path: &Path, len: u64) -> Option<TailFacts> {
    let mut file = std::fs::File::open(path).ok()?;
    let offset = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = Vec::with_capacity(usize::try_from(len - offset).ok()?);
    file.read_to_end(&mut bytes).ok()?;
    Some(parse_tail(&bytes, offset > 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_table() {
        let cases = [
            ("/tui/fleetops", "-tui-fleetops"),
            (
                "/home/user/work/demo_skills",
                "-home-rob-acme-DEMO-skills",
            ),
            ("/home/user/.claude", "-home-rob--claude"),
            ("/", "-"),
        ];
        for (cwd, want) in cases {
            assert_eq!(project_slug(cwd), want, "cwd {cwd:?}");
        }
    }

    fn assistant_usage(input: u64, cache_read: u64, cache_create: u64) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"usage":{{"input_tokens":{input},"output_tokens":5,"cache_read_input_tokens":{cache_read},"cache_creation_input_tokens":{cache_create}}},"content":[]}}}}"#
        )
    }

    fn ask(id: &str) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"{id}","name":"AskUserQuestion","input":{{}}}}]}}}}"#
        )
    }

    fn answer(id: &str) -> String {
        format!(
            r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"{id}","content":"ok"}}]}}}}"#
        )
    }

    #[test]
    fn last_assistant_usage_wins_and_sums_the_recipe() {
        let jsonl = [
            assistant_usage(10, 100, 1),
            assistant_usage(2, 117_585, 2_298),
        ]
        .join("\n");
        let facts = parse_tail(jsonl.as_bytes(), false);
        assert_eq!(facts.context_tokens, Some(2 + 117_585 + 2_298));
    }

    #[test]
    fn last_ai_title_wins() {
        let jsonl = concat!(
            r#"{"type":"ai-title","aiTitle":"Old title","sessionId":"s"}"#,
            "\n",
            r#"{"type":"ai-title","aiTitle":"Resume FleetOps conversation","sessionId":"s"}"#,
        );
        let facts = parse_tail(jsonl.as_bytes(), false);
        assert_eq!(
            facts.ai_title.as_deref(),
            Some("Resume FleetOps conversation")
        );
    }

    #[test]
    fn unanswered_question_is_pending_answered_is_not() {
        let pending = [ask("t1")].join("\n");
        assert!(parse_tail(pending.as_bytes(), false).pending_question);

        let answered = [ask("t1"), answer("t1")].join("\n");
        assert!(!parse_tail(answered.as_bytes(), false).pending_question);

        let other_result_does_not_resolve = [ask("t1"), answer("t2")].join("\n");
        assert!(parse_tail(other_result_does_not_resolve.as_bytes(), false).pending_question);
    }

    #[test]
    fn partial_first_line_garbage_and_unknown_types_are_skipped() {
        let jsonl = [
            r#"okens":123}}}"#.to_string(), // partial line from mid-file seek
            "not json at all".to_string(),
            r#"{"type":"last-prompt","weird":true}"#.to_string(),
            assistant_usage(1, 2, 3),
        ]
        .join("\n");
        let facts = parse_tail(jsonl.as_bytes(), true);
        assert_eq!(facts.context_tokens, Some(6));
    }

    #[test]
    fn empty_tail_yields_defaults() {
        assert_eq!(parse_tail(b"", false), TailFacts::default());
    }

    #[test]
    fn zero_usage_line_never_clobbers_an_earlier_real_count() {
        // A later assistant line whose usage sums to 0 (only output_tokens, or explicit zeros)
        // must keep the earlier real context count — and alone it proves nothing.
        let zero = r#"{"type":"assistant","message":{"usage":{"output_tokens":42},"content":[]}}"#;
        let jsonl = [assistant_usage(5, 5, 5), zero.to_string()].join("\n");
        assert_eq!(
            parse_tail(jsonl.as_bytes(), false).context_tokens,
            Some(15),
            "earlier real count survives a zero-usage line"
        );
        assert_eq!(parse_tail(zero.as_bytes(), false).context_tokens, None);
    }

    #[test]
    fn format_tokens_table() {
        let cases = [
            (0, "0"),
            (999, "999"),
            (1_000, "1k"),
            (117_585, "117k"),
            (1_234_567, "1.2M"),
        ];
        for (n, want) in cases {
            assert_eq!(format_tokens(n), want, "n={n}");
        }
    }

    #[test]
    fn ctx_pct_recipe() {
        assert_eq!(ctx_used_pct(100_000), 50);
        assert_eq!(ctx_used_pct(0), 0);
        assert_eq!(
            ctx_used_pct(200_000),
            100,
            "at the 200k boundary, still the small window"
        );
        // Over 200k → the 1M window (seen live: 408k must read 40%, not 204%).
        assert_eq!(ctx_used_pct(408_000), 40);
        // saturates, never panics (untrusted input can carry absurd values)
        assert_eq!(ctx_used_pct(u64::MAX), u64::MAX / 1_000_000);
    }

    #[test]
    fn hostile_usage_values_saturate_never_panic() {
        let jsonl = format!(
            r#"{{"type":"assistant","message":{{"usage":{{"input_tokens":{max},"cache_read_input_tokens":{max},"cache_creation_input_tokens":1}},"content":[]}}}}"#,
            max = u64::MAX
        );
        let facts = parse_tail(jsonl.as_bytes(), false);
        assert_eq!(facts.context_tokens, Some(u64::MAX));
    }

    #[test]
    fn cache_rereads_only_when_stamp_changes_and_handles_missing_file() {
        let tmp = std::env::temp_dir().join(format!("fleet-tel-{}", std::process::id()));
        let project = tmp.join(project_slug("/w"));
        std::fs::create_dir_all(&project).unwrap();
        let path = project.join("sid.jsonl");
        std::fs::write(&path, assistant_usage(1, 2, 3)).unwrap();

        let mut cache = TailCache::default();
        let first = cache.read(&tmp, "/w", "sid");
        assert_eq!(first.facts.as_ref().unwrap().context_tokens, Some(6));
        assert!(first.secs_since_append.is_some());

        // Unchanged file → cached (same facts back).
        let second = cache.read(&tmp, "/w", "sid");
        assert_eq!(second.facts, first.facts);

        // Grown file → re-read.
        let mut content = std::fs::read_to_string(&path).unwrap();
        content.push('\n');
        content.push_str(&assistant_usage(10, 20, 30));
        std::fs::write(&path, content).unwrap();
        let third = cache.read(&tmp, "/w", "sid");
        assert_eq!(third.facts.as_ref().unwrap().context_tokens, Some(60));

        // Missing transcript → default telemetry.
        let missing = cache.read(&tmp, "/w", "other-sid");
        assert_eq!(missing, Telemetry::default());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn failed_read_is_not_cached_as_an_empty_transcript() {
        let tmp = std::env::temp_dir().join(format!("fleet-tel-badread-{}", std::process::id()));
        let project = tmp.join(project_slug("/w"));
        // A DIRECTORY where the transcript should be: metadata succeeds, reading fails.
        std::fs::create_dir_all(project.join("sid.jsonl")).unwrap();

        let mut cache = TailCache::default();
        let t = cache.read(&tmp, "/w", "sid");
        assert_eq!(
            t.facts, None,
            "a failed read must not masquerade as an empty transcript"
        );

        // Once readable, the next poll picks up the real facts (nothing poisoned the cache).
        std::fs::remove_dir_all(project.join("sid.jsonl")).unwrap();
        std::fs::write(project.join("sid.jsonl"), assistant_usage(1, 2, 3)).unwrap();
        let healed = cache.read(&tmp, "/w", "sid");
        assert_eq!(healed.facts.as_ref().unwrap().context_tokens, Some(6));

        std::fs::remove_dir_all(&tmp).ok();
    }
}
