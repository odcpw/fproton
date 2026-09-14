#!/usr/bin/env bash
# Smoke test for the Omarchy bar widget (DESIGN §6.9). Runs against this machine:
# it validates the repository copy of the plugin, checks the manifest and the
# source for the traps §6 names, then asks the running shell to reload its
# plugins and reads the journal for a load failure.
#
# Exits non-zero on the first failure. Under ~10 s.
#
#   tests/plugin-smoke.sh            # full run (needs a running omarchy-shell)
#   tests/plugin-smoke.sh --static   # skip the reload + journal half

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLUGIN="$REPO/plugin/oliver.protonvpn"
ID="oliver.protonvpn"
STATIC_ONLY=0
[[ ${1:-} == "--static" ]] && STATIC_ONLY=1

PASS=0
FAIL=0

ok()   { printf '  ok    %s\n' "$1"; PASS=$((PASS + 1)); }
bad()  { printf '  FAIL  %s\n' "$1"; FAIL=$((FAIL + 1)); }
die()  { printf 'plugin-smoke: %s\n' "$1" >&2; exit 2; }

command -v jq >/dev/null || die "jq is required"
[[ -d $PLUGIN ]] || die "plugin directory not found: $PLUGIN"

echo "plugin-smoke: $PLUGIN"

# --- 1. the validator the shell itself mirrors -------------------------------
# It refuses symlinks, so this must be the repository directory and never the
# link installed under ~/.config/omarchy/plugins.
if command -v omarchy-plugin-validate >/dev/null; then
  if out=$(omarchy-plugin-validate "$PLUGIN" 2>&1); then
    ok "omarchy-plugin-validate"
  else
    bad "omarchy-plugin-validate: $out"
  fi
else
  bad "omarchy-plugin-validate not on PATH"
fi

# --- 2. manifest ---------------------------------------------------------------
M="$PLUGIN/manifest.json"
check_jq() { # <description> <filter>
  if jq -e "$2" "$M" >/dev/null 2>&1; then ok "$1"; else bad "$1"; fi
}
check_jq "manifest id is $ID"                 ".id == \"$ID\""
check_jq "kinds contains bar-widget"          '(.kinds | index("bar-widget")) != null'
check_jq "entryPoints.barWidget = Panel.qml"  '.entryPoints.barWidget == "Panel.qml"'
check_jq "defaultSection = right"             '.barWidget.defaultSection == "right"'
check_jq "allowMultiple = false"              '.barWidget.allowMultiple == false'

# --- 3. files -------------------------------------------------------------------
for f in manifest.json Panel.qml Service.qml ProtonIcon.qml Model.js README.md; do
  if [[ -f $PLUGIN/$f ]]; then ok "$f exists"; else bad "$f is missing"; fi
done

# --- 4. QML syntax -----------------------------------------------------------------
# `omarchy-shell shell rescanPlugins` does NOT re-read the QML of an already
# loaded symlinked plugin on this machine (verified 2026-09-15 by breaking
# Panel.qml on purpose: the journal stayed clean until `omarchy restart shell`).
# So a syntax check here is what actually catches a broken edit; the journal and
# IPC checks below only catch a plugin that fails on a cold load.
QMLLINT="$(command -v qmllint 2>/dev/null || true)"
[[ -n $QMLLINT ]] || QMLLINT=/usr/lib/qt6/bin/qmllint
if [[ -x $QMLLINT ]]; then
  # Only syntax: qmllint cannot resolve qs.Ui / qs.Commons outside the shell's
  # import path, and those import warnings are expected noise.
  syntax=$("$QMLLINT" "$PLUGIN"/*.qml "$PLUGIN"/*.js 2>&1 | grep -E 'Syntax error|\[syntax\]' || true)
  if [[ -z $syntax ]]; then ok "qmllint: no syntax errors"; else bad "qmllint: $syntax"; fi
else
  bad "qmllint not found (looked on PATH and in /usr/lib/qt6/bin)"
fi

# --- 5. the §6 traps -------------------------------------------------------------
if grep -q 'manageIpc: false' "$PLUGIN/Panel.qml"; then
  ok "Panel.qml sets manageIpc: false"
else
  bad "Panel.qml must set manageIpc: false (it declares its own IpcHandler)"
fi

if grep -n 'execDetached(\["bash' "$PLUGIN"/*.qml "$PLUGIN"/*.js >/dev/null 2>&1; then
  bad "execDetached([\"bash\" …]) is a shell string in argv clothing"
else
  ok "no execDetached([\"bash\" …])"
fi

if grep -n 'bar\.run(' "$PLUGIN"/*.qml "$PLUGIN"/*.js >/dev/null 2>&1; then
  bad "bar.run() is bash -lc <string>; use Quickshell.execDetached(argv)"
else
  ok "no bar.run()"
fi

# Manifest defaults are never merged into `settings` (DESIGN §1.5), so every
# default key must be read through setting()/intSetting() with the same value.
missing_fallback=0
while IFS= read -r key; do
  if grep -qE "(setting|intSetting)\(\"$key\"" "$PLUGIN"/*.qml; then continue; fi
  bad "manifest default '$key' has no in-code fallback"
  missing_fallback=1
done < <(jq -r '.barWidget.defaults | keys[]' "$M")
[[ $missing_fallback -eq 0 ]] && ok "every manifest default has an in-code fallback"

# --- 6. reload the shell and read the journal -------------------------------------
if [[ $STATIC_ONLY -eq 1 ]]; then
  echo "plugin-smoke: --static, skipping the reload half"
elif ! command -v omarchy-shell >/dev/null; then
  bad "omarchy-shell not on PATH"
elif ! omarchy-shell shell ping >/dev/null 2>&1; then
  bad "omarchy-shell is not answering (is the shell running?)"
else
  SINCE=$(date '+%Y-%m-%d %H:%M:%S')
  if omarchy-shell shell rescanPlugins >/dev/null 2>&1; then ok "rescanPlugins"; else bad "rescanPlugins failed"; fi
  sleep 2
  # Two lines are benign and appear on every reload: the registry's own reload
  # notice, and the IpcHandler warning every panel plugin on this box logs.
  noise='Local plugin changed|Handler was registered but will not be used'
  bads='failed:|Expected token|unavailable|is not a type|ReferenceError|TypeError'
  lines=$(journalctl --user -t omarchy-shell --since "$SINCE" --no-pager 2>/dev/null \
            | grep -F "$ID" | grep -Ev "$noise" | grep -E "$bads" || true)
  if [[ -z $lines ]]; then
    ok "journal carries no load failure for $ID"
  else
    bad "journal: $lines"
  fi

  # The widget answers its own IPC only when Panel.qml actually instantiated.
  if cursor=$(omarchy-shell "$ID" cursor 2>/dev/null) && [[ $cursor == \{* ]]; then
    ok "widget is loaded (cursor: $cursor)"
  else
    bad "widget did not answer IPC — it is not on the bar, or it failed to load"
  fi
fi

echo
printf 'plugin-smoke: %d passed, %d failed\n' "$PASS" "$FAIL"
[[ $FAIL -eq 0 ]]
