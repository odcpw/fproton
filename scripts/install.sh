#!/usr/bin/env bash
# fproton installer: build from this checkout, install the binary, wire up Omarchy.
#
#   scripts/install.sh --dry-run        # print the plan, change nothing
#   scripts/install.sh --yes            # do it without asking
#
# Everything it does is idempotent: running it twice leaves the same machine.
# It never uses sudo, never writes outside $HOME, and never touches the keyring.

set -euo pipefail
umask 022

REPO="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${FPROTON_PREFIX:-$HOME/.local/bin}"
BIN="fproton"
PLUGIN_ID="oliver.protonvpn"

ASSUME_YES=0
DRY_RUN=0
DO_SETUP=1
QUIET=0
USE_COLOR=1
LOCK_DIR=""
TMP_BIN=""

# Exit codes mirror fproton's own (fproton --help): 0 ok, 1 failure, 2 usage,
# 3 safety (refused without --yes), 4 a required tool is missing.
E_FAIL=1
E_USAGE=2
E_SAFETY=3
E_BACKEND=4

usage() {
  cat <<'EOF'
fproton installer

Usage: scripts/install.sh [OPTIONS]

Options:
  --yes            Do not ask for confirmation
  --dry-run        Print every step and change nothing (implies no build)
  --no-setup       Install the binary only; skip `fproton setup --all`
  --prefix DIR     Where to install the binary (default: ~/.local/bin)
  --quiet          Only warnings and errors
  --no-color       Never emit colour (NO_COLOR and a non-TTY stdout are honoured too)
  -h, --help       This text

Steps:
  1. check for cargo and a writable prefix
  2. cargo build --release
  3. install the binary to PREFIX/fproton (atomic rename; safe while fproton runs)
  4. fproton setup --all --yes   (plugin symlink + enable + bar placement + menu entries)

Reverse it with scripts/uninstall.sh.
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --yes | -y) ASSUME_YES=1 ;;
    --dry-run | -n) DRY_RUN=1 ;;
    --no-setup) DO_SETUP=0 ;;
    --prefix)
      [ $# -ge 2 ] || {
        echo "install.sh: --prefix needs a directory" >&2
        exit "$E_USAGE"
      }
      PREFIX="$2"
      shift
      ;;
    --prefix=*) PREFIX="${1#--prefix=}" ;;
    --quiet | -q) QUIET=1 ;;
    --no-color) USE_COLOR=0 ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "install.sh: unknown argument '$1'" >&2
      echo "hint: scripts/install.sh --help" >&2
      exit "$E_USAGE"
      ;;
  esac
  shift
done

if [ ! -t 1 ] || [ -n "${NO_COLOR:-}" ] || [ -n "${CI:-}" ]; then USE_COLOR=0; fi
if [ "$USE_COLOR" -eq 1 ]; then
  C_DIM=$'\033[2m' C_RED=$'\033[31m' C_GRN=$'\033[32m' C_YEL=$'\033[33m' C_BLU=$'\033[34m' C_OFF=$'\033[0m'
else
  C_DIM='' C_RED='' C_GRN='' C_YEL='' C_BLU='' C_OFF=''
fi

info() { [ "$QUIET" -eq 1 ] || printf '%s\n' "${C_BLU}->${C_OFF} $*"; }
ok() { [ "$QUIET" -eq 1 ] || printf '%s\n' "${C_GRN}ok${C_OFF}   $*"; }
skip() { [ "$QUIET" -eq 1 ] || printf '%s\n' "${C_DIM}--   $*${C_OFF}"; }
warn() { printf '%s\n' "${C_YEL}warn${C_OFF} $*" >&2; }
err() { printf '%s\n' "${C_RED}error${C_OFF} $*" >&2; }
plan() { [ "$QUIET" -eq 1 ] || printf '%s\n' "${C_DIM}would${C_OFF} $*"; }

# Show the exact argv of anything that changes the machine, then run it (or not).
run() {
  if [ "$DRY_RUN" -eq 1 ]; then
    plan "$*"
    return 0
  fi
  info "$*"
  "$@"
}

cleanup() {
  [ -n "$TMP_BIN" ] && [ -e "$TMP_BIN" ] && rm -f -- "$TMP_BIN"
  [ -n "$LOCK_DIR" ] && [ -d "$LOCK_DIR" ] && rmdir -- "$LOCK_DIR" 2>/dev/null
  return 0
}
trap cleanup EXIT

# ---------------------------------------------------------------- preflight --

[ -f "$REPO/Cargo.toml" ] && [ -d "$REPO/crates/fproton" ] || {
  err "$REPO does not look like the fproton checkout (no Cargo.toml + crates/fproton)"
  exit "$E_FAIL"
}

if ! command -v cargo >/dev/null 2>&1; then
  err "cargo is not on PATH; fproton is built from source"
  echo "hint: omarchy pkg add rustup && rustup default stable" >&2
  exit "$E_BACKEND"
fi

if [ "$DO_SETUP" -eq 1 ] && ! command -v omarchy >/dev/null 2>&1; then
  warn "omarchy is not on PATH; the desktop integration step will report what it cannot do"
fi

DEST="$PREFIX/$BIN"
if [ -e "$DEST" ] && [ ! -f "$DEST" ]; then
  err "$DEST exists and is not a regular file; refusing to replace it"
  exit "$E_FAIL"
fi
if [ "$DRY_RUN" -eq 0 ]; then
  mkdir -p -- "$PREFIX" || {
    err "cannot create $PREFIX"
    exit "$E_FAIL"
  }
  [ -w "$PREFIX" ] || {
    err "$PREFIX is not writable"
    exit "$E_FAIL"
  }
fi

case ":$PATH:" in
  *":$PREFIX:"*) : ;;
  *) warn "$PREFIX is not on your PATH. The Omarchy shell's PATH includes ~/.local/bin but not ~/.cargo/bin, so install there." ;;
esac

OLD_VERSION=""
if [ -x "$DEST" ]; then OLD_VERSION="$("$DEST" --version 2>/dev/null || true)"; fi

# ------------------------------------------------------------------- consent --

if [ "$DRY_RUN" -eq 0 ] && [ "$ASSUME_YES" -eq 0 ]; then
  if [ -t 0 ] && [ -t 1 ]; then
    printf '%s\n' "This will build fproton and install it to $DEST${OLD_VERSION:+ (replacing $OLD_VERSION)}."
    if [ "$DO_SETUP" -eq 1 ]; then
      printf '%s\n' "Then: $BIN setup --all --yes  (plugin link, bar widget, menu entries)"
    fi
    printf '%s' "Proceed? [y/N] "
    read -r reply || reply=""
    case "$reply" in
      y | Y | yes | YES) : ;;
      *)
        err "cancelled"
        exit "$E_SAFETY"
        ;;
    esac
  else
    err "install.sh needs --yes when stdin is not a terminal"
    echo "hint: scripts/install.sh --yes   # or --dry-run to see the plan" >&2
    exit "$E_SAFETY"
  fi
fi

# ---------------------------------------------------------------------- lock --

if [ "$DRY_RUN" -eq 0 ]; then
  LOCK_DIR="${XDG_RUNTIME_DIR:-/tmp}/fproton-install.lock"
  if ! mkdir -- "$LOCK_DIR" 2>/dev/null; then
    err "another install.sh holds $LOCK_DIR"
    echo "hint: rmdir '$LOCK_DIR'   # if no installer is running" >&2
    LOCK_DIR=""
    exit "$E_FAIL"
  fi
fi

# --------------------------------------------------------------------- build --

if [ "$DRY_RUN" -eq 1 ]; then
  plan "cargo build --release   (in $REPO)"
else
  info "cargo build --release"
  (cd -- "$REPO" && cargo build --release) || {
    err "the build failed; nothing was installed"
    exit "$E_FAIL"
  }
fi

BUILT="$REPO/target/release/$BIN"
if [ "$DRY_RUN" -eq 0 ] && [ ! -x "$BUILT" ]; then
  err "$BUILT is missing after a successful build"
  exit "$E_FAIL"
fi

# ------------------------------------------------------------------- install --

if [ "$DRY_RUN" -eq 1 ]; then
  plan "install -m 0755 $BUILT $DEST"
elif [ -f "$DEST" ] && cmp -s -- "$BUILT" "$DEST"; then
  skip "$DEST is already this exact binary"
else
  # Write beside the target and rename: replacing a running binary in place
  # gives ETXTBSY, and a half-copied fproton on PATH is worse than none.
  TMP_BIN="$PREFIX/.$BIN.install.$$"
  install -m 0755 -- "$BUILT" "$TMP_BIN"
  mv -f -- "$TMP_BIN" "$DEST"
  TMP_BIN=""
  ok "installed $DEST"
fi

NEW_VERSION=""
if [ "$DRY_RUN" -eq 0 ]; then
  NEW_VERSION="$("$DEST" --version 2>/dev/null || true)"
  [ -n "$NEW_VERSION" ] || {
    err "$DEST does not run"
    exit "$E_FAIL"
  }
fi

# --------------------------------------------------------------------- setup --

SETUP_RC=0
if [ "$DO_SETUP" -eq 0 ]; then
  skip "desktop integration (--no-setup)"
elif [ "$DRY_RUN" -eq 1 ]; then
  # Ask an fproton that already exists (this checkout's build, else the installed
  # one) for the real plan; fall back to naming the command when there is none.
  PROBE=""
  if [ -x "$BUILT" ]; then
    PROBE="$BUILT"
  elif [ -x "$DEST" ]; then
    PROBE="$DEST"
  fi
  if [ -n "$PROBE" ]; then
    info "$PROBE setup --all --dry-run"
    "$PROBE" setup --all --dry-run || SETUP_RC=$?
    if [ "$SETUP_RC" -ne 0 ]; then
      warn "that fproton exited $SETUP_RC for the setup plan (see above); the real run may differ"
      SETUP_RC=0
    fi
  else
    plan "$BIN setup --all --yes   (plugin link, enable, bar placement, menu entries)"
  fi
else
  info "$BIN setup --all --yes"
  "$DEST" setup --all --yes || SETUP_RC=$?
fi

# ------------------------------------------------------------------- summary --

if [ "$QUIET" -eq 0 ]; then
  echo
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '%s\n' "${C_DIM}dry run: nothing above was executed.${C_OFF}"
  else
    printf '%s\n' "${C_GRN}fproton installed${C_OFF}  ${NEW_VERSION:-}"
    printf '%s\n' "  binary   $DEST"
    printf '%s\n' "  next     fproton doctor      # what is healthy on this machine"
    printf '%s\n' "           fproton tui         # Status · Servers · History"
    printf '%s\n' "  undo     $REPO/scripts/uninstall.sh"
  fi
fi

if [ "$SETUP_RC" -ne 0 ]; then
  err "fproton setup --all exited $SETUP_RC; the binary is installed, the desktop integration is not complete"
  echo "hint: fproton setup --all --dry-run   # what it wanted to do" >&2
  exit "$SETUP_RC"
fi
