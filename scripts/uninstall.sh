#!/usr/bin/env bash
# fproton uninstaller: reverse scripts/install.sh.
#
#   scripts/uninstall.sh --dry-run      # print the plan, change nothing
#   scripts/uninstall.sh --yes          # do it without asking
#   scripts/uninstall.sh --yes --menu   # also strip the fproton lines from the Omarchy menu
#   scripts/uninstall.sh --yes --purge  # also delete fproton's config, state and cache
#
# It removes only what the installer created. Proton's own CLI, its cache, your
# NetworkManager profiles and the keyring are never touched.

set -euo pipefail
umask 022

REPO="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${FPROTON_PREFIX:-$HOME/.local/bin}"
BIN="fproton"
PLUGIN_ID="oliver.protonvpn"
PLUGIN_LINK="${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/plugins/$PLUGIN_ID"
MENU_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/extensions/omarchy-menu.jsonc"
BACKUP_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/fproton/backups"

ASSUME_YES=0
DRY_RUN=0
DO_MENU=0
DO_PURGE=0
QUIET=0
USE_COLOR=1

E_FAIL=1
E_USAGE=2
E_SAFETY=3

usage() {
  cat <<'EOF'
fproton uninstaller

Usage: scripts/uninstall.sh [OPTIONS]

Options:
  --yes            Do not ask for confirmation
  --dry-run        Print every step and change nothing
  --menu           Also remove fproton's entries from ~/.config/omarchy/extensions/omarchy-menu.jsonc
                   (verbatim backup first; restored if the result no longer parses)
  --purge          Also delete ~/.config/fproton, ~/.local/state/fproton, ~/.cache/fproton
  --prefix DIR     Where the binary was installed (default: ~/.local/bin)
  --quiet          Only warnings and errors
  --no-color       Never emit colour
  -h, --help       This text

Removes, in order:
  1. the bar widget      omarchy plugin disable oliver.protonvpn
  2. the plugin symlink  ~/.config/omarchy/plugins/oliver.protonvpn  (only when it is a symlink)
  3. the binary          PREFIX/fproton
  4. --menu, --purge     only when asked

Never removed: the Proton CLI, ~/.cache/Proton, NetworkManager profiles, anything in the keyring.
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --yes | -y) ASSUME_YES=1 ;;
    --dry-run | -n) DRY_RUN=1 ;;
    --menu) DO_MENU=1 ;;
    --purge) DO_PURGE=1 ;;
    --prefix)
      [ $# -ge 2 ] || {
        echo "uninstall.sh: --prefix needs a directory" >&2
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
      echo "uninstall.sh: unknown argument '$1'" >&2
      echo "hint: scripts/uninstall.sh --help" >&2
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

run() {
  if [ "$DRY_RUN" -eq 1 ]; then
    plan "$*"
    return 0
  fi
  info "$*"
  "$@"
}

DEST="$PREFIX/$BIN"

if [ "$DRY_RUN" -eq 0 ] && [ "$ASSUME_YES" -eq 0 ]; then
  if [ -t 0 ] && [ -t 1 ]; then
    printf '%s\n' "This will disable the $PLUGIN_ID widget, remove its symlink, and delete $DEST."
    if [ "$DO_MENU" -eq 1 ]; then printf '%s\n' "It will also strip fproton's lines from $MENU_FILE (backed up first)."; fi
    if [ "$DO_PURGE" -eq 1 ]; then printf '%s\n' "It will also delete fproton's config, state and cache directories."; fi
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
    err "uninstall.sh needs --yes when stdin is not a terminal"
    echo "hint: scripts/uninstall.sh --yes   # or --dry-run to see the plan" >&2
    exit "$E_SAFETY"
  fi
fi

# ------------------------------------------------------------ 1. bar widget --

if command -v omarchy >/dev/null 2>&1; then
  plugin_state="$(omarchy plugin list 2>/dev/null | awk -v id="$PLUGIN_ID" '$1 == id { print $2 }' || true)"
  if [ "$plugin_state" = "enabled" ]; then
    run omarchy plugin disable "$PLUGIN_ID"
  elif [ -n "$plugin_state" ]; then
    skip "$PLUGIN_ID is already $plugin_state"
  else
    skip "$PLUGIN_ID is not registered with omarchy"
  fi
else
  skip "omarchy is not on PATH; cannot disable the widget"
fi

# -------------------------------------------------------- 2. plugin symlink --

if [ -L "$PLUGIN_LINK" ]; then
  target="$(readlink -f -- "$PLUGIN_LINK" || true)"
  run rm -f -- "$PLUGIN_LINK"
  if [ -n "$target" ]; then skip "that link pointed at $target (left alone)"; fi
elif [ -d "$PLUGIN_LINK" ]; then
  warn "$PLUGIN_LINK is a real directory, not the installer's symlink; leaving it alone"
  echo "hint: rm -rf '$PLUGIN_LINK'   # only if you are sure it is yours" >&2
else
  skip "no plugin link at $PLUGIN_LINK"
fi

# ---------------------------------------------------------------- 3. binary --

if [ -f "$DEST" ]; then
  run rm -f -- "$DEST"
else
  skip "no binary at $DEST"
fi

# ------------------------------------------------------------ 4. menu lines --

# The menu file is the owner's: full of comments and their own entries. Remove
# only whole lines whose key is "protonvpn…" plus a lone "// fproton" marker,
# take a verbatim backup first, and put the backup back if the result stops
# parsing (a stripped entry can leave a trailing comma behind).
menu_strip() {
  local src="$1" out="$2"
  awk '
    /^[[:space:]]*\/\/[[:space:]]*fproton[[:space:]]*$/ { next }
    /^[[:space:]]*"protonvpn(\.[a-z-]+)?"[[:space:]]*:/ { next }
    { print }
  ' "$src" >"$out"
}

# Dropping the last entries of the object leaves the entry above them with a
# trailing comma, which JSONC does not allow either. Remove exactly one comma,
# on the last real line before the file's closing brace.
menu_fix_trailing_comma() {
  local src="$1" out="$2"
  awk '
    { line[NR] = $0 }
    END {
      close_at = 0
      for (i = NR; i >= 1; i--) if (line[i] ~ /^[[:space:]]*}[[:space:]]*$/) { close_at = i; break }
      if (close_at > 1) {
        for (i = close_at - 1; i >= 1; i--) {
          if (line[i] ~ /^[[:space:]]*$/) continue
          if (line[i] ~ /^[[:space:]]*\/\//) continue
          sub(/,[[:space:]]*$/, "", line[i])
          break
        }
      }
      for (i = 1; i <= NR; i++) print line[i]
    }
  ' "$src" >"$out"
}

menu_parses() {
  python3 - "$1" <<'PY'
import json, re, sys
src = open(sys.argv[1], encoding="utf-8").read()
# strip // line comments outside strings, then parse
out, i, n, in_str, esc = [], 0, len(src), False, False
while i < n:
    c = src[i]
    if in_str:
        out.append(c)
        if esc: esc = False
        elif c == "\\": esc = True
        elif c == '"': in_str = False
        i += 1
        continue
    if c == '"':
        in_str = True; out.append(c); i += 1; continue
    if c == "/" and i + 1 < n and src[i+1] == "/":
        while i < n and src[i] != "\n": i += 1
        continue
    out.append(c); i += 1
try:
    json.loads("".join(out))
except Exception as e:
    print(e, file=sys.stderr); sys.exit(1)
PY
}

if [ "$DO_MENU" -eq 1 ]; then
  if [ ! -f "$MENU_FILE" ]; then
    skip "no menu file at $MENU_FILE"
  elif ! grep -qE '^[[:space:]]*"protonvpn(\.[a-z-]+)?"[[:space:]]*:' "$MENU_FILE"; then
    skip "no fproton entries in $MENU_FILE"
  elif ! command -v python3 >/dev/null 2>&1; then
    warn "python3 is missing; not editing $MENU_FILE without being able to check the result"
  else
    scratch="$(mktemp)"
    menu_strip "$MENU_FILE" "$scratch"
    if ! menu_parses "$scratch" 2>/dev/null; then
      fixed="$(mktemp)"
      menu_fix_trailing_comma "$scratch" "$fixed"
      if menu_parses "$fixed" 2>/dev/null; then
        mv -f -- "$fixed" "$scratch"
      else
        rm -f -- "$fixed"
      fi
    fi
    if [ "$DRY_RUN" -eq 1 ]; then
      plan "rewrite $MENU_FILE:"
      diff -u --label "$MENU_FILE" --label "$MENU_FILE (after)" -- "$MENU_FILE" "$scratch" || true
      rm -f -- "$scratch"
    else
      stamp="$(date -u +%Y%m%dT%H%M%SZ)"
      mkdir -p -- "$BACKUP_DIR"
      cp -p -- "$MENU_FILE" "$BACKUP_DIR/omarchy-menu.jsonc.$stamp"
      cat -- "$scratch" >"$MENU_FILE"
      rm -f -- "$scratch"
      if menu_parses "$MENU_FILE"; then
        ok "removed fproton's entries from $MENU_FILE (backup: $BACKUP_DIR/omarchy-menu.jsonc.$stamp)"
      else
        cp -p -- "$BACKUP_DIR/omarchy-menu.jsonc.$stamp" "$MENU_FILE"
        err "removing those lines left invalid JSON; restored $MENU_FILE from the backup"
        echo "hint: edit $MENU_FILE by hand and delete the \"protonvpn…\" lines" >&2
        exit "$E_FAIL"
      fi
    fi
  fi
else
  if [ -f "$MENU_FILE" ] && grep -qE '^[[:space:]]*"protonvpn(\.[a-z-]+)?"[[:space:]]*:' "$MENU_FILE"; then
    skip "menu entries kept in $MENU_FILE (pass --menu to remove them)"
  fi
fi

# -------------------------------------------------------------- 5. own data --

if [ "$DO_PURGE" -eq 1 ]; then
  for d in \
    "${XDG_CONFIG_HOME:-$HOME/.config}/fproton" \
    "${XDG_STATE_HOME:-$HOME/.local/state}/fproton" \
    "${XDG_CACHE_HOME:-$HOME/.cache}/fproton"; do
    if [ -d "$d" ]; then
      run rm -rf -- "$d"
    else
      skip "no $d"
    fi
  done
else
  skip "config, state and cache kept (pass --purge to delete them)"
fi

if [ "$QUIET" -eq 0 ]; then
  echo
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '%s\n' "${C_DIM}dry run: nothing above was executed.${C_OFF}"
  else
    printf '%s\n' "${C_GRN}fproton removed${C_OFF}"
    printf '%s\n' "  kept     the Proton CLI, ~/.cache/Proton, NetworkManager profiles, the keyring"
    printf '%s\n' "  reinstall $REPO/scripts/install.sh"
  fi
fi
