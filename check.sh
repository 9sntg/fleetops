#!/usr/bin/env bash
# check.sh — the gate. Must be green before any wave is "done".
# fmt (no drift) + clippy (all/pedantic/nursery/cargo, -D warnings) + rustdoc + tests.
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck disable=SC1090
. "$HOME/.cargo/env" 2>/dev/null || true

echo "▶ cargo fmt --check"
cargo fmt --check
echo "▶ cargo clippy --all-targets --all-features -- -D warnings"
cargo clippy --all-targets --all-features -- -D warnings
echo "▶ cargo doc --no-deps (rustdoc -D warnings)"
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --quiet
echo "▶ cargo test"
cargo test
echo "✓ check green"
