//! status ctx: the fold — one pure, table-tested function decides every shown status.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/fold.rs
//! Deps:    discovery (NativeStatus)
//! Tested:  inline `#[cfg(test)]` priority table incl. stall boundary
//!
//! Key responsibilities:
//! - Fold (native status × pending question × transcript age) → `Status`, first match wins.
//! - `Stalled?` covers the invisible permission-prompt class (dossier risk row 1).
//! - Bucket sort order: attention-needing states first.
//!
//! Design constraints:
//! - Pure — no clocks (ages are inputs). Every fold change needs a table row (pre-mortem #3).
//! - Unknown native statuses surface as `Unknown` (drift signal), never silently mapped.

use crate::discovery::NativeStatus;

/// Busy with no transcript append for this long → `Stalled?`.
pub const STALL_AFTER_SECS: u64 = 300;

/// The shown status vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// A pending AskUserQuestion — the maintainer's answer is blocking the session.
    NeedsAnswer,
    /// Native `waiting` — blocked on input the transcript can't show (permission prompt etc.).
    Waiting,
    /// Busy but the transcript stopped growing — possibly a prompt fleetops cannot see.
    Stalled,
    /// A native status string this build doesn't know — parser drift.
    Unknown,
    /// Claude is processing and the transcript is moving.
    Working,
    /// Waiting at the prompt.
    Idle,
    /// User dropped to shell mode.
    Shell,
}

/// The fold. First match wins (spec 004 table).
pub const fn status(
    native: &NativeStatus,
    pending_question: bool,
    secs_since_append: Option<u64>,
) -> Status {
    if pending_question {
        return Status::NeedsAnswer;
    }
    match native {
        NativeStatus::Busy => match secs_since_append {
            Some(age) if age > STALL_AFTER_SECS => Status::Stalled,
            _ => Status::Working,
        },
        NativeStatus::Idle => Status::Idle,
        NativeStatus::Shell => Status::Shell,
        NativeStatus::Waiting => Status::Waiting,
        NativeStatus::Other(_) => Status::Unknown,
    }
}

/// Sort bucket: lower = higher on the board.
pub const fn sort_key(status: Status) -> u8 {
    match status {
        Status::NeedsAnswer => 0,
        Status::Waiting => 1,
        Status::Stalled => 2,
        Status::Unknown => 3,
        Status::Working => 4,
        Status::Idle => 5,
        Status::Shell => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_priority_table() {
        let other = NativeStatus::Other("pondering".to_string());
        #[rustfmt::skip]
        let cases: &[(&NativeStatus, bool, Option<u64>, Status)] = &[
            // pending question beats everything, even stalled-busy and unknown
            (&NativeStatus::Busy,  true,  Some(9_999), Status::NeedsAnswer),
            (&NativeStatus::Idle,  true,  None,        Status::NeedsAnswer),
            (&other,               true,  None,        Status::NeedsAnswer),
            // stall boundary: strictly greater than STALL_AFTER_SECS
            (&NativeStatus::Busy,  false, Some(STALL_AFTER_SECS - 1), Status::Working),
            (&NativeStatus::Busy,  false, Some(STALL_AFTER_SECS),     Status::Working),
            (&NativeStatus::Busy,  false, Some(STALL_AFTER_SECS + 1), Status::Stalled),
            // busy with no transcript yet = working (young session), not stalled
            (&NativeStatus::Busy,  false, None,        Status::Working),
            (&NativeStatus::Idle,  false, Some(9_999), Status::Idle),
            (&NativeStatus::Shell, false, None,        Status::Shell),
            // native `waiting` (found live 2026-07-10, disproves dossier A6): input-blocked,
            // regardless of transcript age — never downgraded to Idle or Stalled
            (&NativeStatus::Waiting, false, Some(9_999), Status::Waiting),
            (&NativeStatus::Waiting, false, None,        Status::Waiting),
            (&other,               false, Some(1),     Status::Unknown),
        ];
        for (native, pending, age, want) in cases {
            assert_eq!(
                status(native, *pending, *age),
                *want,
                "{native:?} pending={pending} age={age:?}"
            );
        }
    }

    #[test]
    fn sort_buckets_put_attention_first() {
        let order = [
            Status::NeedsAnswer,
            Status::Waiting,
            Status::Stalled,
            Status::Unknown,
            Status::Working,
            Status::Idle,
            Status::Shell,
        ];
        assert!(order.windows(2).all(|w| sort_key(w[0]) < sort_key(w[1])));
    }
}
