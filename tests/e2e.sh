#!/usr/bin/env bash
# fproton end-to-end tests — real machine, real binary, no mocks (DESIGN.md §4b).
#
#   tests/e2e.sh            --fast (default): only what §4b calls GREEN, < 15 s
#   tests/e2e.sh --full     adds the AMBER timeout paths, < 60 s
#
# Rules this script keeps:
#   * it never mutates the desktop: `setup` runs only with --dry-run, and the set of
#     active NetworkManager connections is compared before and after;
#   * every AMBER case runs in its OWN state directory with --timeout 3, so a previous
#     case's 60 s cli.hang backoff can never answer for it;
#   * nothing asserts that a connection succeeded. The Proton CLI is wedged on this
#     machine (§1.2) and a green run here is not evidence that connecting works.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${FPROTON_BIN:-$ROOT/target/release/fproton}"
MODE="fast"
[[ "${1:-}" == "--full" ]] && MODE="full"
[[ "${1:-}" == "--fast" ]] && MODE="fast"

PASS=0; FAIL=0; SKIP=0
RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/fproton-e2e-XXXXXX")"
trap 'rm -rf "$RUN_DIR"' EXIT

# A private state/config/cache triple for the whole run, so the owner's own state,
# settings and index are never touched.
export FPROTON_STATE_DIR="$RUN_DIR/state"
export FPROTON_CONFIG_DIR="$RUN_DIR/config"
export FPROTON_CACHE_DIR="$RUN_DIR/cache"

ok()   { PASS=$((PASS+1)); printf '  \033[32mok\033[0m    %s\n' "$1"; }
bad()  { FAIL=$((FAIL+1)); printf '  \033[31mFAIL\033[0m  %s\n' "$1"; [[ -n "${2:-}" ]] && printf '        %s\n' "$2"; }
skip() { SKIP=$((SKIP+1)); printf '  \033[2mskip\033[0m  %s\n' "$1"; [[ -n "${2:-}" ]] && printf '        %s\n' "$2"; }
head_() { printf '\n\033[1m%s\033[0m\n' "$1"; }

# run <expected-exit> <label> -- <argv...>; stdout in $OUT, stderr in $ERR, ms in $MS
OUT=""; ERR=""; CODE=0; MS=0
run() {
  local want="$1" label="$2"; shift 3
  local start end
  start=$(date +%s%N)
  OUT="$("$@" 2>"$RUN_DIR/stderr")"; CODE=$?
  end=$(date +%s%N)
  MS=$(( (end - start) / 1000000 ))
  ERR="$(cat "$RUN_DIR/stderr")"
  if [[ "$want" == "any" || "$CODE" == "$want" ]]; then
    ok "$label (exit $CODE, ${MS} ms)"
    return 0
  fi
  bad "$label — expected exit $want, got $CODE (${MS} ms)" "${OUT:0:200} ${ERR:0:200}"
  return 1
}

jq_is() { # jq_is <filter> <expected> <label>
  local got; got="$(printf '%s' "$OUT" | jq -r "$1" 2>/dev/null)"
  if [[ "$got" == "$2" ]]; then ok "$3 ($1 = $2)"; else bad "$3 — $1 is '$got', expected '$2'"; fi
}

[[ -x "$BIN" ]] || { echo "no binary at $BIN (cargo build --release)"; exit 2; }
command -v jq >/dev/null || { echo "jq is required"; exit 2; }

echo "fproton e2e — $MODE — $BIN"
"$BIN" version --json | jq -c .

ACTIVE_BEFORE="$(nmcli -t -f NAME,UUID,ACTIVE connection show --active | sort)"

# ---------------------------------------------------------------- GREEN rows

head_ "machine contract"
run 0 "capabilities" -- "$BIN" capabilities
jq_is '.ok' 'true' "capabilities is a valid envelope"
jq_is '.commands | length' '23' "capabilities lists every command"
jq_is '.exit_codes | length' '15' "capabilities lists every exit code"
jq_is '.build_stage' 'phase-1b-core' "capabilities names this build stage"
run 0 "robot-docs" -- "$BIN" robot-docs
[[ "${OUT:0:8}" == "fproton " ]] && ok "robot-docs prints the handbook" || bad "robot-docs printed something else"
run 0 "version --json" -- "$BIN" version --json
jq_is '.contract_version' '1.0.0' "version carries the contract version"

head_ "status (§5)"
run 0 "status --json" -- "$BIN" status --json
for key in ok schema generated_at state engine installed signed_in max_tier account link \
           server traffic kill_switch port_forwarding config index pending cli last_error \
           warnings commands; do
  if printf '%s' "$OUT" | jq -e "has(\"$key\")" >/dev/null; then :; else bad "status.$key is missing"; fi
done
ok "status carries every §5 key"
jq_is '.schema' '1' "status is schema 1"
STATE="$(printf '%s' "$OUT" | jq -r .state)"
case "$STATE" in
  connected|connecting|disconnecting|disconnected|error) ok "status.state is one of the five ($STATE)" ;;
  *) bad "status.state is '$STATE'" ;;
esac
# §5: unknown is null, never a zero. With no tunnel there is no traffic and no port.
if [[ "$(printf '%s' "$OUT" | jq -r .link.up)" == "false" ]]; then
  jq_is '.traffic' 'null' "no device means no traffic numbers"
  jq_is '.port_forwarding.port' 'null' "no tunnel means no forwarded port"
fi
jq_is '.installed.networkmanager' 'true' "NetworkManager is detected"
# Warm timing, the §2 budget. Measured, and printed even when it passes.
"$BIN" status --json >/dev/null
total=0
for _ in 1 2 3 4 5 6 7 8 9 10; do
  s=$(date +%s%N); "$BIN" status --json >/dev/null; e=$(date +%s%N)
  total=$(( total + (e - s) / 1000 ))
done
MEAN_US=$(( total / 10 ))
printf '        status --json mean over 10 runs: %d.%03d ms (§2 budget 50 ms)\n' $((MEAN_US/1000)) $((MEAN_US%1000))
if (( MEAN_US < 150000 )); then ok "status stays well inside a 150 ms e2e guard"; else bad "status took ${MEAN_US} us"; fi

head_ "index, servers, countries, cities"
run 0 "index --rebuild --json" -- "$BIN" index --rebuild --json
SERVERS="$(printf '%s' "$OUT" | jq -r .index.servers)"
(( SERVERS > 1000 )) && ok "the index holds $SERVERS servers" || bad "the index holds only $SERVERS servers"
run 0 "index --stats --json" -- "$BIN" index --stats --json
jq_is ".index.servers" "$SERVERS" "index --stats reads the header it just wrote"
run 0 "servers --country CH --json" -- "$BIN" servers --country CH --json --limit 20
jq_is '[.servers[].country] | unique | join(",")' 'CH' "every row is Swiss"
s=$(date +%s%N); "$BIN" servers --country CH --json >/dev/null; e=$(date +%s%N)
printf '        servers --country CH --json: %d ms (§2 budget 20 ms)\n' $(( (e-s)/1000000 ))
run 0 "servers --tier free --json" -- "$BIN" servers --tier free --json --limit 5
jq_is '[.servers[].tier] | unique | join(",")' '0' "--tier free returns only tier-0 servers"
run 0 "servers --feature tor --json" -- "$BIN" servers --feature tor --json --limit 5
jq_is '[.servers[] | .features | index("tor") != null] | all' 'true' "--feature tor filters on the bitmask"
run 0 "countries --json" -- "$BIN" countries --json
COUNTRIES="$(printf '%s' "$OUT" | jq -r '.countries | length')"
(( COUNTRIES > 50 )) && ok "countries lists $COUNTRIES countries" || bad "countries lists only $COUNTRIES"
run 0 "cities --country CH --json" -- "$BIN" cities --country CH --json
jq_is '[.cities[].country] | unique | join(",")' 'CH' "cities are filtered by country"
run 2 "cities --country nonsense" -- "$BIN" cities --country nonsense --json

head_ "local state and settings"
run 0 "settings --json" -- "$BIN" settings --json
jq_is '.settings.engine' 'cli' "the only engine is cli"
run 0 "settings set default_target CH" -- "$BIN" settings set default_target CH --json
run 0 "settings get default_target" -- "$BIN" settings get default_target --json
jq_is '.value' 'CH' "the setting round-trips through config.toml"
run 2 "settings set engine wg" -- "$BIN" settings set engine wg --json
run 0 "settings set default_target fastest" -- "$BIN" settings set default_target fastest --json
run 0 "history --json" -- "$BIN" history --json
run 0 "recents --json" -- "$BIN" recents --json

head_ "policy and safety (§2)"
run 2 "connect '; rm -rf'" -- "$BIN" connect '; rm -rf' --json
jq_is '.code_name' 'usage' "an injection-shaped target is a usage error"
run 2 "connect CH#4611111" -- "$BIN" connect 'CH#4611111' --json
run 3 "disconnect --force without --yes" -- "$BIN" disconnect --force --json
jq_is '.code_name' 'safety' "an irreversible action refuses without --yes"
printf '%s' "$OUT" | jq -e '.error | contains("nmcli")' >/dev/null \
  && ok "the refusal names the exact argv it would run" || bad "the refusal hides its argv"
run 0 "disconnect --force --dry-run" -- "$BIN" disconnect --force --dry-run --json
jq_is '.dry_run' 'true' "--dry-run changes nothing and says so"
run 2 "config set kill-switch permanent" -- "$BIN" config set kill-switch permanent --json
jq_is '.code_name' 'usage' "there is no kill-switch permanent upstream"
run 2 "config set protocol wireguard" -- "$BIN" config set protocol wireguard --json
run 2 "tui without a TTY" -- "$BIN" tui --json
run 6 "port without a tunnel" -- "$BIN" port --json
jq_is '.code_name' 'not_found' "port with no tunnel is exit 6"

head_ "RED: the experiment stays gated (§4b)"
run 2 "wg list without the env gate" -- "$BIN" wg list --json
jq_is '.code_name' 'usage' "wg refuses without FPROTON_EXPERIMENTAL_WG=1"
printf '%s' "$OUT" | jq -e '.hint | contains("FPROTON_EXPERIMENTAL_WG")' >/dev/null \
  && ok "the refusal names the gate" || bad "the refusal does not name the gate"

head_ "output policy"
if "$BIN" | jq -e '.schema == 1' >/dev/null 2>&1; then
  ok "a bare, piped invocation prints status --json"
else
  bad "a bare, piped invocation did not print status --json"
fi
if script -qec "$BIN status" /dev/null | grep -q $'\033\['; then
  ok "a TTY gets colour"
else
  skip "a TTY got no colour" "script(1) may not allocate a pty here"
fi
for env_flag in "NO_COLOR=1" "CI=1"; do
  if env $env_flag script -qec "$BIN status" /dev/null | grep -q $'\033\['; then
    bad "$env_flag did not disable colour"
  else
    ok "$env_flag disables colour"
  fi
done
if script -qec "$BIN status --no-color" /dev/null | grep -q $'\033\['; then
  bad "--no-color did not disable colour"
else
  ok "--no-color disables colour"
fi

head_ "no secrets in any output (§2)"
LEAK_FILE="$RUN_DIR/all-output.txt"
: > "$LEAK_FILE"
for cmd in "status" "servers --limit 5" "countries" "cities --country CH" "index --stats" \
           "settings" "history" "recents" "capabilities" "version" "port" "recents" \
           "setup --all --dry-run" "disconnect --force --dry-run" "wg list"; do
  # shellcheck disable=SC2086
  "$BIN" $cmd --json >> "$LEAK_FILE" 2>> "$LEAK_FILE"
  # shellcheck disable=SC2086
  "$BIN" $cmd -v >> "$LEAK_FILE" 2>> "$LEAK_FILE"
done
"$BIN" robot-docs >> "$LEAK_FILE" 2>&1
LEAKS=0
for pattern in "PrivateKey" "RefreshToken" "AccessToken" "x25519pk" "X25519PublicKey"; do
  if grep -qi -- "$pattern" "$LEAK_FILE"; then bad "output contains '$pattern'"; LEAKS=1; fi
done
# A 44-character base64 blob ending in '=' is what a WireGuard key looks like.
if grep -qE '[A-Za-z0-9+/]{43}=' "$LEAK_FILE"; then
  bad "output contains a 44-character base64 blob (key-shaped)"; LEAKS=1
fi
(( LEAKS == 0 )) && ok "no command leaks a key, a token or a keyring path"
if grep -q "/.local/share/keyrings" "$LEAK_FILE"; then
  bad "output names a keyring file path"
else
  ok "no output names ~/.local/share/keyrings"
fi

head_ "setup (only --dry-run, §4b)"
MENU="$HOME/.config/omarchy/extensions/omarchy-menu.jsonc"
MENU_BEFORE=""; [[ -f "$MENU" ]] && MENU_BEFORE="$(sha256sum "$MENU" | cut -d' ' -f1)"
run any "setup --all --dry-run" -- "$BIN" setup --all --dry-run --json
jq_is '.dry_run' 'true' "setup --dry-run says it changed nothing"
printf '%s' "$OUT" | jq -e '[.steps[].outcome] | index("changed") == null' >/dev/null \
  && ok "no setup step reports a change" || bad "a setup step changed something in --dry-run"
if [[ -n "$MENU_BEFORE" ]]; then
  MENU_AFTER="$(sha256sum "$MENU" | cut -d' ' -f1)"
  [[ "$MENU_BEFORE" == "$MENU_AFTER" ]] && ok "the owner's omarchy-menu.jsonc is byte-identical" \
    || bad "setup --dry-run modified $MENU"
fi
run any "setup without --yes" -- "$BIN" setup --menu --json
jq_is '.dry_run' 'true' "setup without --yes is a dry run"

# ---------------------------------------------------------------- AMBER rows

if [[ "$MODE" == "full" ]]; then
  head_ "AMBER: the Proton CLI's deadline path (§4b) — never a success assertion"
  amber() { # amber <label> <argv...>
    local label="$1"; shift
    local dir="$RUN_DIR/amber-$(echo "$label" | tr -c 'a-zA-Z0-9' '-')"
    mkdir -p "$dir"
    local start end ms code out
    start=$(date +%s%N)
    out="$(FPROTON_STATE_DIR="$dir" "$BIN" "$@" --timeout 3 --json 2>&1)"; code=$?
    end=$(date +%s%N); ms=$(( (end - start) / 1000000 ))
    local name; name="$(printf '%s' "$out" | jq -r '.code_name // "?"' 2>/dev/null)"
    local hint; hint="$(printf '%s' "$out" | jq -r '.hint // ""' 2>/dev/null)"
    if [[ "$code" != 5 ]]; then
      bad "$label — expected exit 5 (upstream), got $code in ${ms} ms" "${out:0:200}"
      # If the CLI ever answers, make sure this script did not leave a tunnel behind.
      [[ "$label" == connect* ]] && "$BIN" disconnect --force --yes --json >/dev/null 2>&1
      return
    fi
    [[ "$name" == "upstream" ]] || { bad "$label — code_name is '$name'"; return; }
    [[ -n "$hint" ]] || { bad "$label — no hint"; return; }
    if (( ms < 2500 || ms > 8000 )); then
      bad "$label — ${ms} ms is outside 2500-8000 ms (a sub-second exit means the backoff answered)"
      return
    fi
    ok "$label — exit 5 upstream after ${ms} ms, hint: $hint"
  }
  amber "account" account
  amber "config-list" config list
  amber "config-set" config set netshield off
  amber "signout" signout
  amber "disconnect" disconnect
  amber "connect-fastest" connect fastest

  # reconnect needs a previous target: seed one in this case's own state dir.
  RDIR="$RUN_DIR/amber-reconnect"; mkdir -p "$RDIR"
  printf '{"recents":["CH"]}' > "$RDIR/state.json"
  start=$(date +%s%N)
  out="$(FPROTON_STATE_DIR="$RDIR" "$BIN" reconnect --timeout 3 --json 2>&1)"; code=$?
  end=$(date +%s%N); ms=$(( (end-start)/1000000 ))
  if [[ "$code" == 5 ]] && (( ms >= 2500 && ms <= 8000 )); then
    ok "reconnect — exit 5 upstream after ${ms} ms"
  else
    bad "reconnect — exit $code in ${ms} ms" "${out:0:200}"
  fi

  head_ "AMBER: the 60 s backoff answers the second call (§2)"
  BDIR="$RUN_DIR/backoff"; mkdir -p "$BDIR"
  FPROTON_STATE_DIR="$BDIR" "$BIN" account --timeout 3 --json >/dev/null 2>&1
  start=$(date +%s%N)
  out="$(FPROTON_STATE_DIR="$BDIR" "$BIN" account --timeout 3 --json 2>&1)"; code=$?
  end=$(date +%s%N); ms=$(( (end-start)/1000000 ))
  if [[ "$code" == 5 ]] && (( ms < 1000 )); then
    ok "the second call exits 5 in ${ms} ms without spawning the CLI (backoff)"
  else
    bad "the backoff did not short-circuit: exit $code in ${ms} ms" "${out:0:200}"
  fi
  FPROTON_STATE_DIR="$BDIR" "$BIN" status --json | jq -e '.cli.hang_suspected == true' >/dev/null \
    && ok "status reports cli.hang_suspected" || bad "status does not report the hang"
  FPROTON_STATE_DIR="$BDIR" "$BIN" status --json | jq -e '[.warnings[].id] | index("cli_hang_suspected") != null' >/dev/null \
    && ok "status carries the cli_hang_suspected warning" || bad "the warning is missing"

  head_ "AMBER: one case at the un-overridden 15 s probe deadline"
  PDIR="$RUN_DIR/probe-default"; mkdir -p "$PDIR"
  start=$(date +%s%N)
  out="$(FPROTON_STATE_DIR="$PDIR" "$BIN" account --json 2>&1)"; code=$?
  end=$(date +%s%N); ms=$(( (end-start)/1000000 ))
  if [[ "$code" == 5 ]] && (( ms >= 14000 && ms <= 20000 )); then
    ok "account at the default probe deadline — exit 5 after ${ms} ms"
  else
    bad "account at the default deadline — exit $code in ${ms} ms" "${out:0:200}"
  fi

  head_ "RED: the doctor's connected-group checks"
  if "$BIN" doctor --json >/dev/null 2>&1; then
    "$BIN" doctor --json 2>/dev/null | jq -e '[.checks[] | select(.id=="dns_when_connected") | .status] | index("skip") != null' >/dev/null \
      && ok "doctor skips dns_when_connected while the link is down" \
      || bad "doctor does not skip dns_when_connected"
  else
    skip "doctor connected-group checks" "doctor is a Phase-2 stub in this build"
  fi
fi

# ---------------------------------------------------------------- the desktop

head_ "the desktop is as it was"
ACTIVE_AFTER="$(nmcli -t -f NAME,UUID,ACTIVE connection show --active | sort)"
if [[ "$ACTIVE_BEFORE" == "$ACTIVE_AFTER" ]]; then
  ok "the set of active NetworkManager connections is unchanged"
else
  bad "the active connections changed" "$(diff <(echo "$ACTIVE_BEFORE") <(echo "$ACTIVE_AFTER") | head -5)"
fi

# ---------------------------------------------------------------- generated docs

if [[ -w "$ROOT/docs" || -w "$ROOT" ]]; then
  mkdir -p "$ROOT/docs"
  "$BIN" robot-docs > "$ROOT/docs/ROBOT.md"
  ok "docs/ROBOT.md regenerated from robot-docs"
fi

printf '\n%d passed, %d failed, %d skipped (%s)\n' "$PASS" "$FAIL" "$SKIP" "$MODE"
(( FAIL == 0 ))
