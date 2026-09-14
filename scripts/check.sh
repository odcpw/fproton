#!/usr/bin/env bash
# The local gate (DESIGN.md §3): fmt, clippy, test, e2e, plugin-smoke.
#
#   scripts/check.sh            e2e in --fast mode (default)
#   scripts/check.sh --full     e2e in --full mode (the Phase-1b gate)
#   scripts/check.sh --quick    skip e2e and plugin-smoke (fmt + clippy + unit tests)
#
# Every step prints its real output. A step that cannot run on this machine is
# reported as skipped with the reason, never silently passed.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 2

MODE="fast"
case "${1:-}" in
  --full) MODE="full" ;;
  --quick) MODE="quick" ;;
  --fast|"") MODE="fast" ;;
  *) echo "usage: scripts/check.sh [--fast|--full|--quick]"; exit 2 ;;
esac

FAILED=()
step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
note() { printf '    %s\n' "$*"; }

step "cargo fmt --check"
if cargo fmt --check; then note "no diff"; else FAILED+=("cargo fmt --check"); fi

step "cargo clippy --all-targets -- -D warnings"
if cargo clippy --all-targets -- -D warnings 2>&1 | tail -3; then :; fi
if ! cargo clippy --all-targets -- -D warnings >/dev/null 2>&1; then FAILED+=("cargo clippy"); fi

step "cargo test"
if ! cargo test 2>&1 | grep -E "^test result|^error|panicked"; then :; fi
if ! cargo test >/dev/null 2>&1; then FAILED+=("cargo test"); fi

step "core is silent (§2)"
HITS="$(grep -rnE 'println!|eprintln!|print!|dbg!' crates/fproton-core/src || true)"
if [[ -z "$HITS" ]]; then
  note "no print macro anywhere in crates/fproton-core/src"
else
  echo "$HITS"
  FAILED+=("core is silent")
fi

step "no shell strings in argv (§2)"
SHELLISH="$(grep -rnE '\b(sh|bash)\b\s*,\s*"-c"|Command::new\("(sh|bash)"\)' crates/*/src || true)"
if [[ -z "$SHELLISH" ]]; then
  note "no sh -c, no bash -c, anywhere in the Rust sources"
else
  echo "$SHELLISH"
  FAILED+=("no shell strings")
fi

if [[ "$MODE" != "quick" ]]; then
  step "cargo build --release"
  if ! cargo build --release 2>&1 | tail -2; then FAILED+=("cargo build --release"); fi

  step "tests/e2e.sh --$MODE"
  if [[ -x tests/e2e.sh ]]; then
    if ! tests/e2e.sh "--$MODE"; then FAILED+=("tests/e2e.sh --$MODE"); fi
  else
    note "skipped: tests/e2e.sh is not executable"
  fi

  step "tests/plugin-smoke.sh"
  if [[ -x tests/plugin-smoke.sh ]]; then
    if ! tests/plugin-smoke.sh; then FAILED+=("tests/plugin-smoke.sh"); fi
  else
    note "skipped: tests/plugin-smoke.sh does not exist yet (Phase 2, the plugin agent)"
  fi
fi

printf '\n'
if (( ${#FAILED[@]} == 0 )); then
  printf '\033[32mcheck.sh: everything green (%s)\033[0m\n' "$MODE"
  exit 0
fi
printf '\033[31mcheck.sh: %d step(s) failed\033[0m\n' "${#FAILED[@]}"
printf '  - %s\n' "${FAILED[@]}"
exit 1
