//! The branch lane: a session's cwd → its git branch, by reading `.git` directly.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/git.rs
//! Deps:    std::fs only (called via spawn_blocking by the sensor)
//! Tested:  inline `#[cfg(test)]` — pure HEAD/gitdir parsers + tempdir repo, worktree, subdir
//!          and non-repo trees (house pattern, see discovery.rs)
//!
//! Key responsibilities:
//! - `branch_of`: walk up from a cwd to the first `.git`, resolve it, and name the branch.
//! - `branch_from_head` / `gitdir_from_file`: the pure parsers.
//!
//! Design constraints:
//! - **Never shell out to `git`.** This runs per session per sweep; a subprocess each would be
//!   one spawn per session every 2 s. Reading `.git/HEAD` is two small file reads.
//! - `.git` is a FILE in a worktree (`gitdir: <path>`), not a dir — and worktrees are the common
//!   case on the maintainer's fleet (4 of 9 live sessions), not an edge case (spec 015).
//! - Read-only, and every failure is `None`: an unreadable/absent/garbage `.git` means "no
//!   branch to show", never an error and never a panic.

use std::path::{Path, PathBuf};

/// How far up from the cwd to look for a repo root. Deep enough for any real checkout, bounded so
/// a pathological path can't walk to `/` forever.
const MAX_ANCESTORS: usize = 64;

/// The branch for `cwd`, or `None` when it isn't inside a git repo.
///
/// Detached HEAD yields the short sha — the honest answer to "what is this session on".
pub fn branch_of(cwd: &Path) -> Option<String> {
    let git_dir = resolve_git_dir(cwd)?;
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    branch_from_head(&head)
}

/// Find the git dir governing `cwd`: walk up to the first `.git`, then resolve a worktree's
/// `.git` FILE through its `gitdir:` pointer.
fn resolve_git_dir(cwd: &Path) -> Option<PathBuf> {
    let dot_git = cwd
        .ancestors()
        .take(MAX_ANCESTORS)
        .map(|dir| dir.join(".git"))
        .find(|p| p.exists())?;
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    // A worktree: `.git` is a file pointing at <main>/.git/worktrees/<name>.
    let contents = std::fs::read_to_string(&dot_git).ok()?;
    gitdir_from_file(&contents).map(PathBuf::from)
}

/// `ref: refs/heads/feat/x` → `feat/x`; a detached HEAD's raw sha → its short form.
/// Anything else (empty, garbage) → `None`.
pub fn branch_from_head(head: &str) -> Option<String> {
    let head = head.trim();
    if head.is_empty() {
        return None;
    }
    if let Some(reference) = head.strip_prefix("ref: ") {
        // Keep the full branch path after refs/heads/ — `feat/x` is one branch, not a namespace.
        return reference
            .strip_prefix("refs/heads/")
            .map(str::to_string)
            .filter(|b| !b.is_empty());
    }
    // Detached HEAD: a raw sha. Show it short, and only if it actually looks like one.
    let is_sha = head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit());
    is_sha.then(|| head.chars().take(7).collect())
}

/// `gitdir: /path/to/.git/worktrees/name` → that path.
pub fn gitdir_from_file(contents: &str) -> Option<String> {
    contents
        .lines()
        .find_map(|l| l.trim().strip_prefix("gitdir:"))
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_from_head_table() {
        let cases: &[(&str, Option<&str>)] = &[
            ("ref: refs/heads/main\n", Some("main")),
            // A slashed branch is ONE branch name — the whole path after refs/heads/ is kept.
            (
                "ref: refs/heads/feat/email-signal-capture\n",
                Some("feat/email-signal-capture"),
            ),
            // Detached HEAD (verified live: `git rev-parse --short` gives 7 chars).
            (
                "d30af8261f5c9b0aa3e4d5f6a7b8c9d0e1f2a3b4\n",
                Some("d30af82"),
            ),
            // A ref we don't render as a branch (tags/remotes aren't checked-out branches).
            ("ref: refs/tags/v1.0\n", None),
            ("ref: refs/heads/\n", None),
            ("", None),
            ("   \n", None),
            ("total garbage", None),
            ("deadbee", Some("deadbee")), // exactly 7 hex chars is a valid short sha
            ("dead", None),               // too short to be a sha
        ];
        for (head, want) in cases {
            assert_eq!(branch_from_head(head).as_deref(), *want, "head={head:?}");
        }
    }

    #[test]
    fn gitdir_from_file_table() {
        assert_eq!(
            gitdir_from_file("gitdir: /repo/.git/worktrees/wt\n").as_deref(),
            Some("/repo/.git/worktrees/wt")
        );
        assert_eq!(
            gitdir_from_file("gitdir:/no/space\n").as_deref(),
            Some("/no/space")
        );
        assert_eq!(gitdir_from_file("gitdir:\n"), None);
        assert_eq!(gitdir_from_file("not a gitdir file"), None);
        assert_eq!(gitdir_from_file(""), None);
    }

    #[test]
    fn branch_of_a_plain_repo_and_a_subdir_below_it() {
        let tmp = std::env::temp_dir().join(format!("fleet-git-{}", std::process::id()));
        let repo = tmp.join("repo");
        let deep = repo.join("src").join("nested");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(repo.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();

        let at_root = branch_of(&repo);
        let below = branch_of(&deep);
        let outside = branch_of(&tmp);
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(at_root.as_deref(), Some("main"));
        assert_eq!(
            below.as_deref(),
            Some("main"),
            "a cwd below the root walks up"
        );
        assert_eq!(outside, None, "no .git above -> no branch, not an error");
    }

    #[test]
    fn branch_of_a_worktree_follows_the_gitdir_file() {
        // THE common case on this fleet: 4 of 9 live sessions run in a worktree, where `.git` is
        // a FILE. Treating it as a dir would yield None for every one of them.
        let tmp = std::env::temp_dir().join(format!("fleet-git-wt-{}", std::process::id()));
        let main_repo = tmp.join("main");
        let wt_admin = main_repo.join(".git").join("worktrees").join("feature");
        let worktree = tmp.join("worktrees").join("feature");
        std::fs::create_dir_all(&wt_admin).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            wt_admin.join("HEAD"),
            "ref: refs/heads/feat/email-signal-capture\n",
        )
        .unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", wt_admin.display()),
        )
        .unwrap();

        let branch = branch_of(&worktree);
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(branch.as_deref(), Some("feat/email-signal-capture"));
    }

    #[test]
    fn a_broken_git_pointer_is_none_never_a_panic() {
        let tmp = std::env::temp_dir().join(format!("fleet-git-bad-{}", std::process::id()));
        let wt = tmp.join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: /nonexistent/elsewhere\n").unwrap();

        let dangling = branch_of(&wt);

        // A `.git` file that isn't a gitdir pointer at all.
        std::fs::write(wt.join(".git"), "corrupted\n").unwrap();
        let garbage = branch_of(&wt);
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(dangling, None, "a gitdir pointing nowhere is not a branch");
        assert_eq!(garbage, None);
    }
}
