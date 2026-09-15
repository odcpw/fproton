# Decisions log (append-only; newest at the bottom; one dated entry per decision)

- 2026-09-14 · orchestrator · Rebuild as Rust CLI + engine + FrankenTUI + thin QML widget instead of patching the 4.7k-line QML/Python plugin. Reason: the plugin broke from an uncommitted hand patch, and its logic (timeouts, CLI hang handling, cache parsing, NAT-PMP) belongs in tested Rust, mirroring fbittorrent.
- 2026-09-14 · orchestrator · Two engines behind one trait: `cli` (official protonvpn CLI, hardened with process-group deadlines) as default; `wg` (NetworkManager WireGuard from Proton-generated configs, no Python, no keyring) as the robust alternative. The CLI hangs on this machine today (keyring), so `wg` is the only engine that can connect until the owner repairs the keyring.
- 2026-09-14 · orchestrator · Server index in fsqlite (FrankenSQLite) built from Proton's own serverlist.json; asupersync RuntimeBuilder.block_on for the async API. Fallback if fsqlite cannot be made to work within two attempts: serde/postcard index behind the same `ServerIndex` API, recorded here.
- 2026-09-14 · orchestrator · GitHub push deferred: `gh` token is invalid; SSH alias `github-odcpw` works. Owner will push after fixing the keyring/reboot.
- 2026-09-14 · orchestrator · fsqlite 0.4.1 does NOT build on stable rustc 1.98: `fsqlite-pager-0.4.1/src/lib.rs:2` has `#![cfg_attr(target_arch = "x86_64", feature(core_intrinsics))]` → E0554. A nightly toolchain is not acceptable for an installable user tool. Testing fsqlite 0.3.x on stable; otherwise the server index is a serde snapshot (postcard/JSON) behind the same `ServerIndex` API.
- 2026-09-14 · orchestrator · fsqlite 0.3.18 also fails on stable: its runtime dependency `asupersync` 0.4.11 uses `#![feature]` (E0554). Verified by a scratch build. FrankenSQLite is therefore out of scope; the franken-suite component in fproton is FrankenTUI (`ftui` 0.7). Server index = serde snapshot under `~/.cache/fproton/` (postcard or JSON, decided by the core agent from measurements) behind a `ServerIndex` API, rebuilt when serverlist.json's mtime changes. `fsqlite` removed from Cargo.toml.
- 2026-09-14 · orchestrator · DESIGN.md v2 after a seven-lens review. Corrections: keyring diagnosis is a *phantom* default collection (`ReadAlias` returns a path listed in `Collections` with no exported object), not "only Default and session"; `protonvpn config` has no `get`, no `protocol`, no `kill-switch permanent`; `signin` USERNAME is required; `--securecore` is one word; connect targets are a single positional; manifest defaults are never injected into `settings`; symlinked plugins load but do not hot-reload; `omarchy-plugin-validate` rejects symlinks; the old plugin is already disabled and absent from shell.json (setup is idempotent); Proton stores no user settings on disk so `status.config` stays null until the CLI answers; server-name regex must cover `IS-DE#1`, `US-CA#1022`, `NL-FREE#15`, `US-CO#21-TOR`; exit codes 0-8 now match fbittorrent.
- 2026-09-14 · orchestrator · `wg` downgraded from engine to env-gated experiment (`FPROTON_EXPERIMENTAL_WG=1`, `up|swap|list` only, no `import`). The earlier claim "wg is the only engine that can connect today" was unsupported: the existing NM profile's key is bound to a session certificate (expires 2026-09-21, refreshable only via the hung API path) and a raw tunnel gets no NetShield/port-forwarding/kill switch (the CLI's local agent negotiates those). Kill criterion recorded in DESIGN.md §4.
- 2026-09-14 · orchestrator · Cut from v0.1: WorldMap, cities drill-down in the panel, notifications wired on (setting default off), `port --keepalive`, `watch`, `status --fast`, `change` (now `reconnect --next`), doctor scoring apparatus, `ipv6_leak_guard`, `gtk_app_conflict`, CI workflow + CHANGELOG (at first push), doctor screen in the TUI, TUI mouse. Doctor keeps a `mutate(Action)` chokepoint + `actions.jsonl` + `undo` because the fixers are few and the inverse recording is cheap; the doctor never deletes anything.
- 2026-09-14 · orchestrator · Index = postcard snapshot rebuilt when serverlist.json's mtime changes (Python's C parser reads the 24 MB file in 77 ms; a snapshot is needed for the 50 ms status budget). fsqlite is out (nightly-only).
- 2026-09-14 · orchestrator · DESIGN.md v3 after a second review round (consistency, implementer dry-run, lean/honest/safe). Filled contract gaps: `pending`/`last_error`/`account`/`config`/`HistoryEntry` shapes; frozen core API now returns `Error` everywhere and adds `state::pending/clear_pending`, `settings::validate`, `engine::reconnect`, `Target: FromStr`, `target_for_server`, `AliasProbe` fields; `connect` loses `--engine`; server/link resolution by NM UUID from connection_persistence.json; versioned postcard header, `status` never rebuilds; e2e gets `--fast`/`--full` with per-case state dirs and `--timeout 3`; `setup --menu` is a textual insert (never parse-and-rewrite the user's JSONC); doctor fixtures only for the nine parsing detectors, run artifacts bounded (`--save`, prune to 10), keyring fixer bytes + undo caveat (SetAlias back to a dangling path cannot work → clear alias), `killswitch_orphan` needs four preconditions; `omarchy-launch-or-focus-tui` is NOT argv-safe (constant literals only; doctor app-id `fprotondoctor`); PanelKeyCatcher only dispatches (Panel owns the state machine; TextField handles Esc); `recents` gets a data source; TUI exit code = the error's own code. Phases: 0 fixtures ∥ 1a API freeze → 1b core (docs starts after 1a) → 2 tui/plugin/doctor → 3 → 4. Added `sha2`, `jiff` to fproton-core.
- 2026-09-14 · fixtures · All 23 `protonvpn … --help` fixtures captured live (165-220 ms each, exit 0, no hang) under `timeout 8`; the top-level one is `protonvpn-help-top.txt` because `--help` alone has no subcommand name. `config set --help` confirms exactly the eight keys DESIGN §1.3 lists.
- 2026-09-14 · fixtures · `protonvpn-info.txt` and `protonvpn-config-list.txt` are DERIVED from the installed Python source, never run; each carries the `# derived from proton-vpn-cli 1.0.3 source, not captured from a run` header plus source file:line lines, so consumers must skip leading `#` lines. `config list`'s footer line 2 is `Use 'protonvpn config set <setting> --help' …` — `ctx.help_option_names[0]` is `--help`, not `-h` (`_cli_constants.py:31-32`, order `[HELP_OPTION, HELP_OPTION_ABBREVIATED]`).
- 2026-09-14 · fixtures · Added `protonvpn-config-list-free-tier.txt` (second derived render): it is the only fixture covering the `Upgrade to enable` branch (`available_on_free_tier=False` on five of eight features, `controller.py:69,257`) and the four-line free-tier footer, and its Value column is 17 chars against 22 in the paid file, so a parser using fixed column offsets fails on one of the two. The renderer is committed as `tests/fixtures/derive-config-list.py` (python-tabulate 0.10.0, the same module the CLI imports).
- 2026-09-14 · fixtures · `nmcli -t connection show <proton uuid>` is now stored verbatim, including the two lines the first capture dropped: `wireguard.private-key:<hidden>` (nmcli's own redaction) and `wireguard.private-key-flags:0` (the proof the key is in the root-only keyfile). No secret is exposed.
- 2026-09-14 · fixtures · The Secret Service phantom-path error uses typographic quotes: `Object does not exist at path “/org/freedesktop/secrets/collection/Default_5fkeyring”` (U+201C/U+201D). The `secret_service_default_alias` detector must match the prefix `Object does not exist at path`, never an ASCII-quoted path.
- 2026-09-14 · fixtures · `busctl --user list` is stored filtered (`grep -E '^(NAME|org\.freedesktop\.secrets|org\.kde\.secretservicecompat)'`) rather than verbatim: the full listing is 106 unrelated user-session services. The filter is part of the command recorded in `tests/fixtures/README.md`. It also records that `org.kde.secretservicecompat` is present as *activatable* — a second Secret Service provider that could claim the name.
- 2026-09-14 · fixtures · `serverlist-small.json` extended from 35 to 38 logicals by appending the real `IS-DE#1`, `US-CA#1022` and `US-CO#21-TOR` entries from the live 24 MB list (purely additive; 167 inserted lines, 0 deleted), so the fixture now covers every server-name shape DESIGN §1.3 names and `Features` values 1, 2 and 28.
- 2026-09-14 · fixtures · Transcript of the offline `config list` render (paid tier), produced by `python3 tests/fixtures/derive-config-list.py`:
  ```
  $ python3 tests/fixtures/derive-config-list.py

  Current configuration
  Setting                  Value
  -----------------------  ----------------------
  netshield                malware-ads-trackers
  kill-switch              standard
  port-forwarding          off
  custom-dns               on  [1.1.1.1, 8.8.8.8]
  vpn-accelerator          on
  moderate-nat             off
  ipv6                     off
  anonymous-crash-reports  on

  Use 'protonvpn config set <setting> <value>' to change settings.
  Use 'protonvpn config set <setting> --help' for available values.
  ```
  Values are plausible, not this account's real settings (Proton stores none on disk). `custom-dns` deliberately carries a two-space run inside its own value, so the table parser must split once on the rule-derived boundary, not on every `\s{2,}`.
- 2026-09-14 · phase-1a · Dependency set frozen: workspace pins serde, serde_json, anyhow, thiserror 2, clap 4.6, dirs 6, toml 1.1, libc, ftui 0.7, ftui-extras 0.7, unicode-width, postcard 1.1 (use-std), sha2 0.10, jiff 0.2. `anyhow` REMOVED from `fproton-core` (core returns `Error` everywhere, never anyhow); `jiff` + `sha2` ADDED to the `fproton` binary (the doctor's run ids and its WriteFile backup hashes live in `crates/fproton/src/doctor/**`). Nobody edits a Cargo.toml after this commit.
- 2026-09-14 · phase-1a · `Error` is an enum with named fields `{msg, hint}` per variant plus `Interrupted`, built by `Error::usage(..)`-style constructors and `.with_hint(..)`. DESIGN.md §8 spells the getter `hint(&self) -> String`, so the *builder* is `with_hint` and `hint()` stays the getter (returns the attached hint, else the class default; never empty). `Error::unimplemented("x")` is the one stub error: exit 1, message "x is not implemented in this build", hint `fproton capabilities`. Nothing in this build reports success it did not perform.
- 2026-09-14 · phase-1a · `status.installed.*` are `Option<bool>`, not `bool`. §5's shape rule ("every key always present, unknown = null, never a zero standing in for unknown") outranks the illustrative example, and Phase 1a probes nothing. Phase 1b fills real booleans; consumers test `=== true`.
- 2026-09-14 · phase-1a · The Phase-1a `status()` returns a schema-valid document with `ok:false`, `state:"error"` and `last_error` = the unimplemented message, rather than a null-filled `ok:true` that would assert a disconnected state it never measured. `state` still follows the §5 truth table (link down, no pending, last_error set → `error`), so the contract is exercised, not faked.
- 2026-09-14 · phase-1a · `--yes` and `--dry-run` are GLOBAL flags on the root command and are passed into `DoctorArgs`/`SetupArgs` through `#[clap(skip)]` fields, so those structs can grow their own flags without colliding. `disconnect --force` requires `--yes` whether or not stdout is a TTY (§2 says "on a TTY"; the plugin already passes `--yes`, and an agent piping output should not be able to tear the tunnel down by accident). Every mutating command accepts `--dry-run` and prints the plan.
- 2026-09-14 · phase-1a · Every JSON envelope carries `ok` and `schema` (not only `status` and errors), so a consumer can version-check any response. Envelope keys per command are in `main.rs`; `servers` carries `built_from_mtime` next to the rows.
- 2026-09-14 · phase-1a · Target parsing normalises before validating: lowercase input is upper-cased (`ch#461` → `CH#461`), `city:"New York"` loses its quotes, and `securecore`/`secure_core`/`best`/`default` are accepted aliases. Validation itself is the §2 regex set, hand-written (no `regex` crate): `is_country_code`, `is_server_name`, `is_city_name` in `engine.rs`, unit-tested against the real name shapes and against `; rm -rf`, `--`, `CH#461 foo`, `$(id)`.
- 2026-09-14 · phase-1a · `agent::capabilities_json` takes `&Context` (DESIGN.md left the signature open) so it can report the real config/state/index/serverlist paths. It also carries `build_stage` and `build_note` naming the phase this binary was built at; later phases update those two constants in `agent.rs`.
- 2026-09-14 · phase-1a · `proc::run_with_deadline` is implemented for real: `Command::process_group(0)` (no `pre_exec`), all three stdio piped with stdin closed immediately, stdout/stderr drained on their own threads so a chatty child cannot deadlock the wait loop, a 25 ms poll of deadline + cancel, then SIGTERM to `-pgid` and SIGKILL 2 s later. A deadline is data (`Output.timed_out`), a cancel is `Err(Error::Interrupted)`, a missing program is `Err(Error::Backend)`. `Output` carries the child pid (= its pgid), which also feeds `status.pending.pid`.
- 2026-09-14 · phase-1a · Phase 1a also implemented, rather than stubbed, the parts that are pure data or pure logic and that later agents must not each re-invent: `State::derive` (§5 truth table), `features_from_raw`, `Settings::{load,save,get,set}` + `settings::validate` + the Proton config whitelist, `cli::is_valid_username`, the RFC-3339 helpers, and `proc::which`. `crates/fproton/src/setup.rs` exists as a frozen stub (`SetupArgs` + `run`) because `main.rs` must dispatch to it and Phase 1b may not edit `main.rs`; Phase 1b replaces its body.

## Phase 1a gate (2026-09-14, real output)

```
$ cargo build --release
    Finished `release` profile [optimized] target(s) in 17.07s

$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.60s      (no warnings)

$ cargo fmt --check
                                                                            (no diff)

$ cargo test
test result: ok. 34 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.73s
  (includes proc.rs: deadline kill of `sleep 30` at 0.4 s, whole-process-group kill of
   `timeout 30 sleep 30` asserted with kill(-pgid, 0) == ESRCH, CancelToken stopping a
   60 s deadline in < 3 s, missing binary → exit-code-4 error)
```

```
$ fproton --help | head -12
Proton VPN for Omarchy.

Bare `fproton` opens the TUI on a terminal; without a TTY it prints `status --json`.
Every command accepts --json, and a non-TTY stdout implies it. Agents: start with
`fproton robot-docs` and `fproton capabilities`.

`status` never spawns the Proton CLI, so it answers even while that CLI is wedged.

Usage: fproton [OPTIONS] [COMMAND]

Commands:
  status        The whole Status document (schema 1). Never spawns the Proton CLI
  ... 23 commands: status connect disconnect reconnect servers countries cities index
      config settings port history recents account signin signout wg doctor setup tui
      capabilities robot-docs version

$ fproton doctor --help | head -8
Diagnose the installation, and optionally repair it

Usage: fproton doctor [OPTIONS] [COMMAND]

Commands:
  undo          Replay a `--fix` run in reverse (exit 13 if a backup is missing or altered)
  capabilities  List every check, its group and its safety class

$ fproton tui --help | head -6
Terminal UI: Status · Servers · History

Usage: fproton tui [OPTIONS]

Options:
      --json               Machine-readable JSON on stdout (errors too, with the same exit code)
      --servers            Open on the Servers screen
      --history            Open on the History screen
```

```
$ fproton status --json | jq .
{
  "ok": false, "schema": 1, "generated_at": "2026-09-14T23:02:48+02:00",
  "state": "error", "engine": "cli",
  "installed": {"cli": null, "daemon": null, "gtk_app": null, "wireguard": null, "networkmanager": null},
  "signed_in": null, "max_tier": null, "account": null,
  "link": {"up": false, "device": null, "nm_connection": null, "since": null, "uptime_sec": 0, "endpoint": null},
  "server": null, "traffic": null,
  "kill_switch": {"active": false, "supported": true},
  "port_forwarding": {"port": null, "expires_at": null},
  "config": null,
  "index": {"servers": null, "built_from_mtime": null, "serverlist_expires_at": null, "loads_expired": null},
  "pending": null,
  "cli": {"last_ok_at": null, "hang_suspected": false, "backoff_until": null},
  "last_error": {"at": "2026-09-14T23:02:48+02:00", "code": 1, "code_name": "failure",
                 "text": "status is not implemented in this build",
                 "hint": "fproton capabilities   # what this build actually does"},
  "warnings": [], "commands": ["fproton capabilities"]
}
(jq exit 0; every §5 key present. Shown here in §5 order for readability: the real output
 is key-sorted because the document passes through serde_json::Value, whose map is a BTreeMap —
 deterministic, and the shape rule is about keys existing, not about their order.)

$ fproton capabilities | jq -c '{tool,version,contract_version,build_stage,commands:(.commands|length),exit_codes:(.exit_codes|length),targets:.connect_targets}'
{"tool":"fproton","version":"0.1.0","contract_version":"1.0.0","build_stage":"phase-1a-api-freeze","commands":23,"exit_codes":15,"targets":["fastest","CC","CC#N","city:NAME","p2p","secure-core","tor","random"]}
(jq exit 0)
```

Exit-code and policy spot checks (all real output):

```
$ fproton connect "; rm -rf" --json ; echo exit=$?
{"code":2,"code_name":"usage","error":"'; rm -rf' is not a connect target; use one of: fastest · CC · CC#N · city:NAME · p2p · secure-core · tor · random","hint":"fproton servers --search <text>","ok":false,"schema":1}
exit=2

$ fproton disconnect --force --json ; echo exit=$?
{"code":3,"code_name":"safety","error":"fproton disconnect --force needs --yes; it would run: nmcli device disconnect proton0 && nmcli connection down pvpn-killswitch-ipv6","hint":"fproton disconnect --force --yes   # or add --dry-run to see the plan","ok":false,"schema":1}
exit=3

$ fproton wg list --json ; echo exit=$?
{"code":2,"code_name":"usage","error":"`wg` is an unverified experiment and is gated off","hint":"FPROTON_EXPERIMENTAL_WG=1 fproton wg list","ok":false,"schema":1}
exit=2

$ fproton tui --json ; echo exit=$?          # non-TTY
{"code":2,"code_name":"usage","error":"the TUI needs a terminal; stdout is not a TTY","hint":"fproton status --json","ok":false,"schema":1}
exit=2

$ fproton | jq -c '{ok,schema,state}'        # bare + piped ⇒ status --json
{"ok":false,"schema":1,"state":"error"}

$ script -qec "fproton status" /dev/null | cat -v | head -2      # TTY ⇒ human + colour
^[[31merror^[[0m  M-bM-^@M-^T
  protections ^[[2munknown M-bM-^@M-^T the Proton CLI has not answered^[[0m
$ NO_COLOR=1 script -qec "fproton status" /dev/null | cat -v | head -1
error  M-bM-^@M-^T
$ CI=1 script -qec "fproton status" /dev/null | cat -v | head -1
error  M-bM-^@M-^T
(--no-color behaves identically)

$ fproton settings --json                     # implemented, not a stub
{"ok":true,"path":"/home/oliver/.config/fproton/config.toml","problems":[],"schema":1,"settings":{"default_target":"fastest","engine":"cli","notify":false,"timeouts":{"busctl_sec":1,"config_set_sec":20,"connect_sec":90,"disconnect_sec":25,"nmcli_sec":10,"probe_sec":15}}}

$ fproton config set kill-switch permanent --json ; echo exit=$?
{"code":2,"code_name":"usage","error":"'permanent' is not a value for 'kill-switch'; allowed: off, standard","hint":"fproton config set kill-switch off","ok":false,"schema":1}
exit=2

$ fproton servers --country CH --json ; echo exit=$?        # honest stub
{"code":1,"code_name":"failure","error":"index::servers is not implemented in this build","hint":"fproton capabilities   # what this build actually does","ok":false,"schema":1}
exit=1

$ grep -rnE 'println!|eprintln!|print!|dbg!' crates/fproton-core/src | wc -l
0                                             # "core is silent" already holds
```

Not verified at this gate, and why: no timing numbers (`status` and `servers` do no work yet — Phase 1b measures them), no `protonvpn` was executed at all (§1.2: only `--help` is safe, and nothing here needed it), and nothing claims to connect.

- 2026-09-14 · docs · README follows `~/Projects/odcpw/fbittorrent/README.md` (the named house style): no badges, no hero/TL;DR, no FAQ, no comparison table, prose over bold-label bullets. The one element kept from the readme-writing skill's template is the verbatim "About Contributions" paragraph, because this repo is destined for GitHub. The "Status" section reproduces DESIGN §4b GREEN/AMBER/RED and points the reader at `fproton capabilities` (`build_stage`, `build_note`) so a partially built binary cannot be mistaken for the finished one.
- 2026-09-14 · docs · Menu glyph fix in `crates/fproton/assets/menu.jsonc`: `protonvpn.doctor` was U+F0A7A, which JetBrainsMono Nerd Font maps to `md-trash_can_outline` (checked with fontTools against `/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf`) — a trash can for a doctor that by design never deletes anything. Now U+F04D9 `md-stethoscope`. Top-level label "VPN" → "Proton VPN" (the Omarchy menu already holds other network entries; the `vpn`/`proton`/`protonvpn` aliases are unchanged). ids and actions untouched, as DESIGN §3 requires.
- 2026-09-14 · docs · `scripts/install.sh` installs by writing `$PREFIX/.fproton.install.$$` and `mv`-ing it into place: `install`/`cp` over a running `fproton` (a TUI in another window) gives ETXTBSY, and a half-copied binary on PATH is worse than none. It also takes an mkdir lock under `$XDG_RUNTIME_DIR`, skips the copy when `cmp` says the built binary is already installed, and refuses without `--yes` when stdin is not a terminal with exit 3 — the same `safety` code fproton itself uses (0/1/2/3/4 reused throughout).
- 2026-09-14 · docs · `scripts/uninstall.sh` leaves `~/.config/omarchy/extensions/omarchy-menu.jsonc` alone unless `--menu` is passed. With it: verbatim backup to `~/.local/state/fproton/backups/`, whole-line removal by awk (the lone `// fproton` marker and every `"protonvpn…"` key), then one trailing comma repaired on the last remaining entry, then a parse check (a python JSONC comment-stripper); a result that does not parse is restored from the backup and the script exits 1. Verified on a copy of the owner's real menu file with the fproton block inserted the way `setup --menu` will insert it: the stripped file is byte-identical to the pre-insert original (`cmp` clean), and a second run is a no-op.
- 2026-09-14 · docs · Numbers measured for the README rather than copied from DESIGN: `timeout 8 protonvpn --help` = 428/356/332 ms (three runs, exit 0); the read-only `busctl … SearchItems … service Proton` probe = 19/11/11 ms (exit 0). `shellcheck` and `shfmt` are not installed on this machine (`which` fails), so the script gate is `bash -n` plus real `--dry-run` runs; an integrator with shellcheck should re-run it.

- 2026-09-14 · phase-1b · `state.json` is the single state file, written atomically (`state.json.tmp.<pid>` + `rename(2)`); an unreadable or unparsable file degrades to an empty state rather than failing `status`, because everything in it is a cache. History is capped at 200 entries, recents at 10.
- 2026-09-14 · phase-1b · `StateFile` gained four additive `#[serde(default)]` fields Phase 1a had not foreseen: `account` (§5 `account` has to be cached somewhere), `last_error` (§5 says any new action clears it, so it must persist between invocations), and `link_uuid` + `link_up_since`. **Integrator:** these are additions to a frozen struct, not changes — old files still load. `link.since`/`uptime_sec` are "since fproton first observed this profile up": nmcli's terse listing carries no start time, and sysfs directory mtimes are not creation times on this box (verified: `/sys/class/net/lo` mtime was 1 h old on a 2-day uptime).
- 2026-09-14 · phase-1b · `status.pending.pid` is always `null`. `proc::run_with_deadline` only reports the child pid after the child is reaped, and §5 allows `pid: int|null`; a guessed pid would be worse than none. Consumers derive progress from `elapsed_sec`/`deadline_sec`, which is what §5 §7a §6.6 actually use.
- 2026-09-14 · phase-1b · `INDEX_FORMAT_VERSION` bumped 1 → 2. The snapshot is now five postcard documents back to back — header, meta, countries, cities, servers — with the small blocks first, so `countries`/`cities` never decode the 18 221-row server block and `status` reads only the first 4 KiB. The §4 header contract is unchanged; the new `IndexMeta` block carries `ExpirationTime`/`LoadsExpirationTime`, which `status` needs and the frozen header has no field for. (The alternative, re-reading the 24 MB source in `status`, is forbidden by the 50 ms budget; Proton writes those two keys at the *end* of the file, so a cheap head-read is not possible either.)
- 2026-09-14 · phase-1b · Servers are stored without their derived fields (`features`, `country_name`) and are decoded **borrowing** from the file's bytes (`StoredServerRef<'a>` with `&str`); only matching rows become owned `ServerRow`s. That took `servers --country CH --json` from 34.6 ms to ~20 ms. `find_server` decodes row by row and stops at the first match.
- 2026-09-14 · phase-1b · `country_name` comes from a 251-entry table generated from `/usr/share/iso-codes/json/iso_3166-1.json` (iso-codes 4.19) and compiled in, plus the two codes Proton ships that ISO does not assign: `UK` (United Kingdom) and `XK` (Kosovo). Verified against the real 18 221-server list: those are the only two of 149 exit countries outside ISO 3166-1.
- 2026-09-14 · phase-1b · A country's (or city's) `lat`/`lon` is the location of its **least-loaded** server — one real point, never an average of several, which would put "Switzerland" in a lake.
- 2026-09-14 · phase-1b · `status` runs its three external probes on threads (`nmcli`, `systemctl is-active`, `busctl`) plus a fourth that reads the persistence file and joins the index, so the wall clock is the slowest probe rather than their sum. The single `nmcli -t … connection show` is the dominant cost (27.5 ms standalone under load average 3).
- 2026-09-14 · phase-1b · `status.ok` is `true` whenever the document was produced from real measurements, even when `state` is `error`: `ok` reports whether the *command* succeeded, not whether the VPN is healthy. It is `false` only when the nmcli probe itself failed, and then the failure is in `last_error`. (Phase 1a's `ok:false` meant "this build cannot answer", which no longer applies.)
- 2026-09-14 · phase-1b · A failed `protonvpn` call records `status.last_error` from `cli.rs`, so the panel and the TUI can say "the CLI did not answer" without an engine action having run. Consequence, straight out of the §5 truth table: after a failed probe with the link down and no pending action, `status.state` is `error` until the next action clears it. That is the table, not a bug.
- 2026-09-14 · phase-1b · Plain `disconnect` always calls the Proton CLI; only `disconnect --force` is the idempotent nmcli no-op (§4b lists them in different classes: `--force` is GREEN and must stay a no-op while both profiles are inactive, plain `disconnect` is AMBER and must reach its deadline). nmcli's "not an active connection" / "Device not found" wording is treated as success there.
- 2026-09-14 · phase-1b · `--no-color` is a `main.rs` flag, but `setup`, `doctor` and the TUI build their own `Out` from a `Context`, which does not carry it. `output.rs` records the flag in a process-global the first time `Out::new` runs (always in `main`), so every later `Out` honours it. **Integrator:** the tidier fix is a `no_color` field on `Context`, which is frozen.
- 2026-09-14 · phase-1b · `setup` without `--yes` is a dry run that says so, rather than a refusal: every step is reported with `changed` / `already` / `would` / `skipped` / `failed`, so a second run is visibly a no-op (`doctor`'s `menu_entries` check asserts the same thing). `--menu` appends a comma to the user's previous last entry when it needs one, which is the only byte it ever changes outside its own inserted block; the unified diff in `--dry-run` shows that line.
- 2026-09-14 · phase-1b · `wg.rs` is 265 lines of module body (301 with tests) against the ≤ 200 the design budgeted. The overrun is the `peer_of` lookup: the index stores no public keys, so `swap` re-reads Proton's 24 MB list to find the target's `EntryIP` + `X25519PublicKey`. `wg list` was run live and lists the real `ProtonVPN CH#461` profile with endpoint `146.70.226.194:443`; `wg up` and `wg swap` have **never** been executed (RED).
- 2026-09-14 · phase-1b · No `hyperfine` and no `/usr/bin/time` exist on this machine (both checked), and installing a package to measure was not worth a system change. Timings below are a 20-iteration bash loop around `date +%s%N`, whose own floor is 2.5 ms (measured with `true`); subtract that for the process's own time.
- 2026-09-14 · phase-1b · The "secrets" test is `tests/e2e.sh`, not a unit test: it runs every safe command with `--json` and with `-v`, concatenates stdout+stderr, and greps for `PrivateKey`, `RefreshToken`, `AccessToken`, `x25519pk`, `X25519PublicKey`, any 44-character base64 blob ending in `=`, and any `~/.local/share/keyrings` path. A real binary running real commands is stronger evidence than a unit test over a struct. `status.rs` additionally asserts the same patterns against the serialised document, including Proton's 88-character `server_id`.

## Phase 1b gate (2026-09-14, real output)

```
$ cargo build --release
    Finished `release` profile [optimized] target(s) in 17.01s      (binary 2 447 016 B)

$ cargo test
running 6 tests    test result: ok. 6 passed; 0 failed        (crates/fproton)
running 83 tests   test result: ok. 83 passed; 0 failed       (crates/fproton-core)

$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.17s   (no warnings)

$ cargo fmt --check
                                                                         (no diff)

$ grep -rnE 'println!|eprintln!|print!|dbg!' crates/fproton-core/src | wc -l
0                                              # "core is silent" still holds

$ scripts/check.sh --full
    …
    84 passed, 0 failed, 1 skipped (full)      # tests/e2e.sh --full, 42.5 s
    ==> tests/plugin-smoke.sh
        skipped: tests/plugin-smoke.sh does not exist yet (Phase 2, the plugin agent)
    check.sh: everything green (full)

$ fproton status --json | jq -c '{ok,schema,state,signed_in,max_tier,server:.server.name,load:.server.load,endpoint:.link.endpoint,index:.index.servers,warnings:[.warnings[].id],commands}'
{"ok":true,"schema":1,"state":"disconnected","signed_in":true,"max_tier":2,"server":"CH#461",
 "load":63,"endpoint":"146.70.226.194:443","index":18221,
 "warnings":["serverlist_stale","protections_unknown","conflicting_tunnel"],
 "commands":["fproton connect fastest","fproton index --rebuild"]}
  (every §5 key present; `signed_in` is the live busctl SearchItems answer, `max_tier` and the
   CH#461 row come from the real 18 221-server snapshot, the endpoint from connection_persistence.json
   matched by NM UUID 58d6042d-…)

$ fproton capabilities | jq -c '{build_stage,commands:(.commands|length),index_format_version}'
{"build_stage":"phase-1b-core","commands":23,"index_format_version":2}

$ fproton robot-docs | head -3
fproton — agent handbook
========================
(blank)

$ fproton index --rebuild --json
{"index":{"built_at":"…","bytes":1478660,"cities":195,"countries":149,"duration_ms":64,
 "max_tier":2,"path":"/home/oliver/.cache/fproton/index.bin","rebuilt":true,"servers":18221,
 "source_len":24432460,"source_mtime":"2026-09-14T20:26:43+02:00", …},"ok":true,"schema":1}
  (24 432 460 B of JSON → a 1 478 660 B snapshot in 64 ms)
```

Timing method: no `hyperfine`, no `/usr/bin/time` on this machine; a 20-iteration bash loop
around `date +%s%N`, floor measured at 2.5 ms with `true`. Load average was ~3.0 throughout
(other agents were compiling), so these are pessimistic.

| Measurement | n | mean | min | max | budget |
|---|---|---|---|---|---|
| `fproton status --json` | 20 | **37.1 ms** | 31.3 | 66.1 | < 50 ms — met |
| `fproton servers --country CH --json` (≈100 rows) | 20 | **20.4 ms** (17.9 net) | 17.2 | 24.3 | < 20 ms — met only after subtracting the 2.5 ms measurement floor; recorded as a marginal miss on the raw number |
| `fproton servers --country CH --limit 10 --json` | 20 | 14.8 ms | 10.9 | 16.8 | — |
| `fproton countries --json` | 20 | 8.0 ms | 6.5 | 9.5 | — |
| `nmcli -t -f NAME,UUID,TYPE,DEVICE,ACTIVE connection show` alone | 20 | 27.5 ms | 24.4 | 30.6 | — (the floor under `status`) |
| bash measurement loop floor (`true`) | 20 | 2.5 ms | 1.9 | 3.1 | — |
| `fproton index --rebuild` (24 MB → snapshot) | 1 | 64-85 ms | — | — | — |

### §4b verification matrix, filled from this run

| command | class | how verified today | evidence (exact command + first line of real output) | claimed in README? |
|---|---|---|---|---|
| `status` | GREEN | real run, every §5 key asserted, 10-run timing | `fproton status --json` → `{"ok":true,…,"state":"disconnected","server":{"name":"CH#461","load":63,…}}` | yes |
| `servers` | GREEN | real run over the 18 221-row snapshot; country/tier/feature filters asserted | `fproton servers --country CH --json` → `{"built_from_mtime":"2026-09-14T20:26:43+02:00","count":…}` | yes |
| `countries` | GREEN | real run | `fproton countries --json` → 149 rows | yes |
| `cities` | GREEN | real run | `fproton cities --country CH --json` → every row `"country":"CH"` | yes |
| `index` | GREEN | real rebuild from the real 24 MB file + header re-read | `fproton index --rebuild --json` → `"servers":18221,"bytes":1478660,"duration_ms":64` | yes |
| `settings` | GREEN | real round-trip through config.toml | `fproton settings set default_target CH` then `get` → `{"value":"CH"}` | yes |
| `history` · `recents` | GREEN | real runs; rows written by the AMBER cases | `fproton history --json` → `{"count":1,"history":[{"action":"disconnect","result":"ok",…}]}` | yes |
| `capabilities` · `robot-docs` · `version` | GREEN | real runs, schema asserted | `fproton capabilities \| jq` → `{"build_stage":"phase-1b-core","commands":23}` | yes |
| `setup --dry-run` | GREEN | real run; the owner's `omarchy-menu.jsonc` sha256 unchanged | `fproton setup --all --dry-run --json` → `"dry_run":true`, no step `changed` | yes |
| `disconnect --force` | GREEN | real run with `--yes`; nmcli active set byte-identical before/after | `fproton disconnect --force --yes --json` → `{"action":"disconnect","force":true,"ok":true}` in 163 ms | yes, as a no-op |
| `port` (error path) | GREEN | real run, no tunnel | `fproton port --json` → `{"code":6,"code_name":"not_found","error":"there is no proton0 tunnel…"}` | error path only |
| `signin` (argument validation) | GREEN | unit tests + a real usage-error run | `is_valid_username("; rm -rf") == false` | validation only |
| `tui`, `doctor` | — | **not implemented in this build** (Phase 2 stubs, exit 1) | `fproton tui --json` → exit 2 "the TUI needs a terminal" | no |
| `connect` | AMBER | own state dir, `--timeout 3` | exit 5 `upstream` after **3081 ms**, hint `fproton doctor --only cli` | timeout path only |
| `reconnect` | AMBER | own state dir seeded with `recents:["CH"]`, `--timeout 3` | exit 5 after **3060 ms** | timeout path only |
| `disconnect` (no `--force`) | AMBER | own state dir, `--timeout 3` | exit 5 after **3077 ms** | timeout path only |
| `config list` · `config set` | AMBER | own state dirs, `--timeout 3` | exit 5 after **3056 / 3054 ms** | timeout path only |
| `config get` | AMBER | real run against an empty cache | exit 5 `upstream`, hint `fproton config list` | timeout path only |
| `account` | AMBER | own state dir at `--timeout 3` **and** at the un-overridden 15 s probe deadline | exit 5 after **3055 ms**; exit 5 after **15 052 ms** | timeout path only |
| `signout` | AMBER | own state dir, `--timeout 3` | exit 5 after **3057 ms** | timeout path only |
| CLI-hang backoff | AMBER | second call in the same state dir | exit 5 in **11 ms** without spawning; `status.cli.hang_suspected == true` + `cli_hang_suspected` warning | yes |
| `connect`/`reconnect` success | **RED** | not verifiable: the Proton CLI is wedged (§1.2) | — | never |
| `port` success | **RED** | no tunnel exists to ask | — | never |
| `wg up` · `wg swap` | **RED** | never executed; gate asserted closed (`exit 2`, hint names `FPROTON_EXPERIMENTAL_WG`) | `fproton wg list` (ungated) → `{"code":2,…}` | never |
| `wg list` | RED-adjacent | run live once, read-only | `FPROTON_EXPERIMENTAL_WG=1 fproton wg list --json` → the real `ProtonVPN CH#461`, endpoint `146.70.226.194:443` | no |
| doctor `dns_when_connected`, `tunnel_route_ok` | **RED** | e2e prints `skip`: the doctor is a Phase-2 stub | — | never |

Every row of the §4 table appears exactly once above.

## Phase 2 — TUI (2026-09-15)

- 2026-09-15 · tui · `run` gets its exit code out of the ftui loop through an `Arc<AtomicI32>` the model writes: `App::fullscreen(model).run()` consumes the model, so there is no other way to return §7a's "the §4a code of the session's last failed action". Each `ActionDone` stores that action's own `Error::exit_code()` (0 on success); `Ctrl+C` stores 130. The TUI never invents a code.
- 2026-09-15 · tui · Two `Every` subscriptions, not one: id 1 at 1000 ms drives the `status` poll (dropped while one is in flight) and the theme-mtime check; id 2 at 120 ms drives the spinner, the 10 s expiry of the status-line note and the 80 ms filter debounce. A single 1 s timer would make the spinner stutter and the filter feel dead; a single 120 ms timer would poll `status` eight times too often.
- 2026-09-15 · tui · `ActionSub` translates the runtime's `StopSignal` into the core `CancelToken` with a watcher thread (`stop.wait_timeout(200 ms)` in a loop) because ftui has no cancel callback — the runtime only drops the subscription and raises the signal. The watcher also exits on an `AtomicBool` the action sets when it finishes, and is joined, so a completed action leaves no thread behind.
- 2026-09-15 · tui · Long actions are refused, not attempted, while `status.cli.hang_suspected` or a live `cli.backoff_until` says the Proton CLI is wedged (§2's 60 s breaker): `c`, `R`, `n` and plain `d` answer with the breaker's own text and the `fproton doctor` hint. `D` (force) and `i` (index rebuild) touch only nmcli and the cache, so they are never refused.
- 2026-09-15 · tui · The Servers list carries no `limit`: all 18 221 rows go into `widgets::virtualized::VirtualizedList` and the per-frame item vector is `&ServerRow` + `&Theme` + column widths (no formatting until a row is on screen). The load cell uses `MiniBar::render_string()` inside the row's `Line` (one style per row); the hero draws a real `MiniBar` widget over its load line, with `MiniBarColors` taken from the theme so no colour literal lives outside `theme.rs`.
- 2026-09-15 · tui · `status.index.servers` is the denominator of the `N/18221 shown` header, not `servers.len()` of an unfiltered query — `status` already carries it and the TUI must not run a second 18 k-row query just to count.
- 2026-09-15 · tui · The narrow tier (< 60 cols or < 16 rows) renders the hero and one dim line saying `widen to 60×16 for the Servers and History tables`, on every screen, exactly as §7a orders ("status line + hero only"). Below 30×8 the single line is `WrapMode::Word` over at most three rows rather than one cut row: at 28 columns "(need 30x8)" would otherwise be the part that disappears.
- 2026-09-15 · tui · `S` spawns `omarchy-launch-floating-terminal-with-presentation "fproton signin"` with all three stdio set to `Stdio::null()` — a constant literal argv (§1.5), and a detached child that cannot write into the alternate screen.
- 2026-09-15 · tui · Real line counts (module bodies, tests excluded; `cargo fmt` style): `mod.rs` 810, `screens.rs` 1026, `subs.rs` 150, `theme.rs` 160 = **2 146 against the §7a budget of 1 200** (2 406 with the 260 lines of unit tests). The overrun is formatting, not features: every §7a screen, state and key is implemented and nothing else is. rustfmt's vertical style costs 3-6 lines per styled span, and the copied `theme.rs` grows from fbittorrent's hand-compacted 92 lines to 160 unchanged in behaviour. Recorded as a miss, not hidden; trimming it means either giving up `cargo fmt --check` (a Phase-3 gate) or cutting screens.
- 2026-09-15 · tui · A cancelled `connect`/`reconnect` is followed by `disconnect --force` (§7a); `disconnect` and `Rebuild` need no cleanup. Verified live, see the gate transcript below.

### Phase 2 TUI gate (2026-09-15, real output from a tmux pty)

```
$ cargo clippy --all-targets -- -D warnings      (whole workspace)   clean
$ cargo fmt --check | grep -c 'src/tui/'                             0
$ cargo test --bin fproton   test result: ok. 51 passed; 0 failed    (22 of them tui::*)
$ grep -rnE 'println!|eprintln!|print!|dbg!' crates/fproton/src/tui/ | wc -l    0
```

Driven with `tmux new-session -d -s … -x W -y H "target/release/fproton tui"` + `send-keys` +
`capture-pane -p`. Every frame below is a real capture.

```
120x40, Status (the machine's real state: the Proton CLI was killed at its deadline)
[ 1 Status ]  2 Servers    3 History                                        cli omarchy
╭ Proton VPN ───────────────────────────────╮╭ Protections ──────────────────────────╮
│ ● Last action failed                      ││ kill switch  inactive                 │
│ CH#461 Switzerland · Zurich               ││ settings unknown — the Proton CLI is … │
│ load ████████░░░░ 63%                     ││ hint: fproton doctor                  │
│ tier 2 · p2p · streaming · ipv6           │╰───────────────────────────────────────╯
│ endpoint  146.70.226.194:443              │╭ Warnings ─────────────────────────────╮
│ link      down                            ││ serverlist_stale the server loads in  │
│ the Proton CLI was killed at its deadline…││ Proton's list have expired…           │
│ hint: fproton doctor --only cli           ││   hint: fproton index --rebuild       │
│ fproton doctor   fproton index --rebuild  ││ protections_unknown …                 │
╰───────────────────────────────────────────╯│ conflicting_tunnel tailscale0 is up … │
╭ Traffic ──────────────────────────────────╮╰───────────────────────────────────────╯
│ traffic unavailable (no proton0 device)   │╭ Recents ──────────────────────────────╮
╰───────────────────────────────────────────╯│ none yet                              │
● error CH#461                                                                 ? help

120x40, Servers (index present, 18 221 rows in a VirtualizedList; scrollbar at the right)
╭ Servers  18221/18221 shown ──────────────────────────────────────────────────────────╮
│ / filter by name, country or city                              f:any  t:any  o:score │
│ NAME          COUNTRY          CITY             LOAD           SCORE TIER  FEATURES  │
│ CH#461        Switzerland      Zurich           ████░░░  63%    1.86 plus  p2p …   █ │
│ CH#473        Switzerland      Zurich           █████░░  66%    1.86 plus  p2p …   │ │

after `/` + "reykjavik" (80 ms debounce) and `f` (feature chip → p2p)
╭ Servers  24/18221 shown ─────────────────────────────────────────────────────────────╮
│ / reykjavik                                                    f:p2p  t:any  o:score │
│ IS#11         Iceland          Reykjavik        ████░░░  60%    5.97 plus  p2p strea…│

100x24, History (`widgets::table::Table`)
╭ History — Enter reconnects to that target ───────────────────────────────────────────╮
│ WHEN             │ACTION     │TARGET        │SERVER   │COUNTRY │ENGINE │RESULT       │
│ 09-15 00:00:23   │disconnect │—             │—        │—       │cli    │timeout (ups…│
│ 09-15 00:00:23   │connect    │fastest       │—        │—       │cli    │timeout (ups…│

100x24 with XDG_CACHE_HOME and FPROTON_CACHE_DIR pointed at empty dirs (no serverlist)
╭ Servers  0/? shown ──────────────────────────────────────────────────────────────────╮
│ / filter by name, country or city                              f:any  t:any  o:score │
│ NAME          COUNTRY          CITY             LOAD           SCORE TIER  FEATURES  │
│ No server index. Press i to build it (fproton index --rebuild)                        │
│ Proton's server list is missing at /tmp/…/noindex/xdg/Proton/VPN/serverlist.json      │
(with only FPROTON_CACHE_DIR empty, `servers`/`countries` rebuild the snapshot silently
 and all three screens render normally — the §4 "stale, not corrupt" rule.)

50x14 (narrow tier: tabs collapse to 1·2·3, hero only)      28x6 (below 30x8)
1·2·3  Status                                               fproton — terminal too small
╭ Proton VPN ────────────────────────────────────╮          (need 30x8)
│ ● Last action failed                           │
│ CH#461 Switzerland · Zurich                    │
│ load ████████░░░░ 63%                          │
│ endpoint  146.70.226.194:443                   │
╰────────────────────────────────────────────────╯
widen to 60×16 for the Servers and History tables
● error CH#461                              ? help

? overlay (88 cols; the same `KEYS` table the key handler dispatches on)
╭ Keys — any key closes ───────────────────────────────────────────────────╮
│ q  Ctrl+C               quit (twice while an action runs: cancel, then q…│
│ Tab  ⇧Tab  1 2 3        Status · Servers · History                       │
│ Esc                     close this overlay, else cancel the running acti…│
```

Cancellation, run once deliberately under a bounded deadline (`--timeout 15`, its own
`FPROTON_STATE_DIR`), because it is the one §7a behaviour no unit test can prove:

```
c   at 00:10:06.123      hero: "⠙ connect 1s / 15s · Esc to cancel", state badge yellow
Esc at 00:10:09.138      status line: "● disconnected CH#461 │ tunnel torn down"
state.json history: [("00:10:09","connect","cancelled",3), ("00:10:09","disconnect","ok",0)]
pgrep protonvpn after: nothing left from this run
```

The `connect` child was killed 3 s in, at the keypress — not at its 15 s deadline — so
StopSignal → CancelToken → SIGTERM of the child group works end to end, and the §7a
follow-up force disconnect ran and reported. `Ctrl+C` in the same harness exits **130**;
`q` on a clean session exits **0** (`echo $?` captured from the tmux command).

Not verified, and why: a **successful** connect (RED, §4b — the Proton CLI is wedged on
the keyring), so the hero's `connected` colouring, the uptime line, the traffic panel with
real counters and the rx `Sparkline` have only been rendered from `traffic == null` /
`link.up == false` data. The sparkline's own code path (≥ 2 samples) is therefore unexercised
on this machine; everything else in §7a has a real frame above or a unit test.

- 2026-09-15 · phase-2-doctor · 26 checks, exactly the §7 list, declared as one `CHECKS: &[CheckDef]` table (`id, group, class, severity, title, detect: fn(&Probes) -> Verdict`). `doctor capabilities --json` is generated from that table, so the contract and the behaviour cannot drift; a test asserts every listed check exists and that no Class D check is reported auto-fixable.
- 2026-09-15 · phase-2-doctor · `Verdict` carries a `reason: Option<&'static str>` next to the status, and `report.json` carries `status`, `reason` **and** a joined `verdict` string (`fail:phantom_collection`, `skip:link_down`). §7 names only `status`; the extra two keys are additive and are what the gate, the table and `jq` filters actually read.
- 2026-09-15 · phase-2-doctor · The nine parsing detectors are driven by `tests/fixtures/doctor/<check>[__<variant>]/{input/,expect.json}`; `probes::from_fixture_dir` fills the same `Probes` fields the live capture fills and parses them with the same functions (`secrets::parse_*`, `nm::parse_connections`, `serverlist_facts_from_bytes`, `toml` → `Settings` → `settings::validate`), so a fixture exercises the live parse path. Four `__variant` directories cover branches this machine cannot produce today without changing the desktop (an active IPv6 leak guard, three accumulated CLI profiles, a healthy alias, an invalid config.toml); each is a one-field edit of a real capture and `tests/fixtures/doctor/README.md` says exactly which file was edited and why.
- 2026-09-15 · phase-2-doctor · The 24 MB serverlist is probed into a `ServerlistFacts` struct (`path, size, mtime, logicals, max_tier, ExpirationTime, LoadsExpirationTime, parse_error`) by one pure function over bytes; a unit test serialises the struct and asserts a server name from the input does **not** appear in it. Run artifacts store those facts in `probes/facts.json`, never the body (§7).
- 2026-09-15 · phase-2-doctor · The `state_dir` writability probe uses `libc::access(W_OK|X_OK)`, not a touch-and-delete, so a detect-only run leaves the state directory byte- *and* mtime-identical. That is what makes the `--fix --dry-run` gate checkable with a checksum rather than a prose claim.
- 2026-09-15 · phase-2-doctor · `mutate(Action)` refuses any `WriteFile`/`CreateSymlink` outside a declared write-scope list (state, config and cache dirs, `~/.config/omarchy/{plugins,extensions}`, and the one named file `~/.local/share/keyrings/default`) with exit 13. `doctor capabilities --json::write_scopes` prints that list and `--fix --dry-run` prints the blast radius. A unit test tries to write `/etc/passwd` and asserts the refusal.
- 2026-09-15 · phase-2-doctor · `Action` is the four §7 variants, unchanged. Two consequences worth recording: (a) creating the state directory is `RunArgv{argv: ["mkdir","-p",…], inverse_argv: []}` — an empty inverse, because the doctor never removes a directory; (b) `omarchy bar` has no `remove`, so the `bar put` step's inverse is empty too and the `omarchy plugin disable` recorded for the step before it is what takes the widget out of the bar.
- 2026-09-15 · phase-2-doctor · `index_fresh`'s repair rebuilds the snapshot into a scratch cache dir under `$TMPDIR`, reads the bytes back and writes them to the real `index.bin` **through the chokepoint**, so the only write to a real path is backed up and hashed like every other. The scratch dir is removed whether the rebuild succeeded or not; a test asserts none is left behind.
- 2026-09-15 · phase-2-doctor · A `WriteFile` whose target did not exist records `inverse: quarantine_file`: `doctor undo` renames the created file into `runs/<id>/quarantine/<path>` rather than unlinking it. Exit 13 is reserved for a missing or hash-mismatched backup of a file that *did* exist — the §7 meaning — and a unit test tampers with a backup and asserts the refusal plus that the live file was left alone.
- 2026-09-15 · phase-2-doctor · The run id is `sha256(state_dir|stamp|pid)[..6]`, not the kernel's `sha256(target|stamp)[..6]`. The kernel's formula makes two runs in the same second share a directory on the assumption that the second waits for the first; fproton implements no such wait, and a shared directory would make `undo latest` replay both runs' `actions.jsonl` and would overwrite the first `report.json` — breaking the append-only invariant it was meant to protect. Verified: two `--save` runs inside one second now produce two directories.
- 2026-09-15 · phase-2-doctor · `--fix` without `--yes` and without `--dry-run` plans everything, applies nothing, and exits 3 `safety` (§2). `--fix --dry-run` exits with the diagnosis code (11 here), because the machine's problems are still there. Class C is skipped with a printed reason unless `--allow-keyring`; before any Class C action runs, the §7 sentence about needing `protonvpn signin` again is printed to stderr.
- 2026-09-15 · phase-2-doctor · After any action is actually applied, the doctor re-captures its probes and re-runs the affected detectors; a check is reported `fixed` only when its detector now passes, and `applied, still failing` otherwise. Nothing reports a repair it did not measure.
- 2026-09-15 · phase-2-doctor · `killswitch_orphan`'s "seen in a previous run ≥ 30 s ago" precondition is kept in `state.killswitch_orphan_seen_at` (the field Phase 1b provisioned). The doctor writes it on a detect-only run — the one exception to "a run without --fix writes nothing" — but only when the observation actually changes (set when first seen, cleared when the condition clears) and never under `--dry-run`, so a healthy machine's state dir is untouched. The condition does not hold on this machine today.
- 2026-09-15 · phase-2-doctor · `serverlist_expired` is a `warn`, never a `fail`: §4 says an expired list is served, not refused, and only the official CLI can refresh it. `serverlist_loads_stale` is `severity: info` for the same reason. Warnings alone exit 0; only `fail` reaches exit 11.
- 2026-09-15 · phase-2-doctor · `cli_session_probe` runs `protonvpn config list` under `min(8 s, --timeout)` through `proc::run_with_deadline` directly, **not** through `cli::config_list`: the core wrapper short-circuits on the 60 s hang backoff, which would let a previous invocation's backoff answer for the probe instead of the deadline. The doctor also does not write the hang into state — a detector must stay pure.

## Phase 2 doctor gate (2026-09-15, real output on host `t14`)

`fproton doctor --json | jq` — 26 checks, every one a real verdict, **exit 11**:

| check | group | class | verdict | detail (truncated) |
|---|---|---|---|---|
| `cli_installed` | cli | D | `pass` | /usr/bin/protonvpn (proton-vpn-cli 1.0.3-1) |
| `cli_help_fast` | cli | D | `pass` | answered in 201 ms |
| `cli_session_probe` | cli | D | `fail:hang` | `protonvpn config list` was killed at its deadline after 8030 ms |
| `daemon_active` | cli | D | `pass` | proton.VPN.service is active |
| `secret_service_on_bus` | keyring | D | `pass` | owned by gnome-keyring-d (org.kde.secretservicecompat is also activatable) |
| `secret_service_default_alias` | keyring | C | **`fail:phantom_collection`** | the `default` alias points at …/collection/Default_5fkeyring, listed in Collections but with no exported object; a `service=Proton` item lives in Default; the login pointer file names 'Default_keyring' |
| `proton_session_item` | keyring | D | `pass` | 2 item(s) with service=Proton in Default |
| `networkmanager_running` | network | D | `pass` | NetworkManager is active |
| `wireguard_available` | network | D | `pass` | /sys/module/wireguard is present |
| `conflicting_tunnel` | network | D | `warn:other_tunnel` | tailscale0 active and will contest the default route and DNS |
| `stale_proton_profiles` | network | D | `pass` | 1 ProtonVPN profile(s), none stale |
| `killswitch_orphan` | network | B | `pass` | the IPv6 leak guard is down |
| `serverlist_present` | cache | D | `pass` | 18221 logical servers, 24432460 bytes, MaxTier 2 |
| `serverlist_expired` | cache | D | `warn:expired` | expired 0 h 29 min ago; fproton still serves it |
| `serverlist_loads_stale` | cache | D | `warn:loads_stale` | per-server loads went stale 3 h ago |
| `index_fresh` | cache | A | `pass` | 18221 servers, built from the source's current mtime |
| `state_dir` | install | A | `pass` | /home/oliver/.local/state/fproton is writable |
| `stale_pending_action` | install | A | `pass` | no action is recorded as in flight |
| `settings_valid` | install | A | `pass` | no config.toml; every setting is at its default |
| `binary_on_path` | install | D | `pass` | /home/oliver/.local/bin/fproton is first on PATH |
| `plugin_linked` | install | B | `pass` | …/omarchy/plugins/oliver.protonvpn → …/fproton/plugin/oliver.protonvpn |
| `plugin_enabled` | install | B | `pass` | oliver.protonvpn is enabled |
| `menu_entries` | install | B | `warn:missing_entries` | 5 of fproton's menu entries are absent from omarchy-menu.jsonc |
| `old_plugin_enabled` | install | B | `pass` | io.github.grichard99.omaproton-vpn is 'disabled' |
| `dns_when_connected` | connected | D | `skip:link_down` | proton0 does not exist; needs a live tunnel (§4b RED) |
| `tunnel_route_ok` | connected | D | `skip:link_down` | proton0 does not exist; needs a live tunnel (§4b RED) |

`summary` = `{"total":26,"pass":18,"warn":4,"fail":2,"skip":2,"fixable":2,"fixed":0,"fix_failed":0,"planned":0}`.
Wall clock 8.7 s, of which 8.03 s is the deliberate `cli_session_probe` deadline.

```
$ fproton doctor --json >/dev/null; echo $?                       11
$ fproton doctor --fix --dry-run --json >/dev/null; echo $?       11   (diagnosis stands; nothing applied)
$ fproton doctor --fix --json >/dev/null; echo $?                  3   safety: "add --yes to apply"
$ fproton doctor --only keyring --json >/dev/null; echo $?        11   ["pass","fail:phantom_collection","pass"]
$ fproton doctor --only cache --json >/dev/null; echo $?           0   (warnings alone are not a failure)
$ fproton doctor --only connected --json >/dev/null; echo $?       0   (both checks skip:link_down)
$ fproton doctor capabilities --json | jq -e '.check_count,(.checks|length),.write_scopes' >/dev/null; echo $?   0
$ fproton doctor undo latest --json   →  {"code":6,"code_name":"not_found","error":"there is no …/doctor/latest",…}
```

`--fix --dry-run` changes nothing (the gate, measured, not asserted in prose):

```
$ sum() { find ~/.local/state/fproton -type f -o -type l | sort | xargs -r sha256sum | sha256sum; }
$ sum                    52c96885c96a74dff60d8e56821a7afb68cfc8717a523e5629f724e202fe9a53  -
$ fproton doctor --fix --dry-run --json >/dev/null
$ sum                    52c96885c96a74dff60d8e56821a7afb68cfc8717a523e5629f724e202fe9a53  -   (identical)
$ git status --porcelain | wc -l      unchanged (15 before, 15 after)
$ sha256sum ~/.config/omarchy/extensions/omarchy-menu.jsonc     unchanged
```

The one planned action it printed, with its blast radius:
`[B] write /home/oliver/.config/omarchy/extensions/omarchy-menu.jsonc (2701 bytes)`.

Class C, planned and **never applied** (`--dry-run` only, as the brief requires during
development). The plan is exactly what §7 specifies:

```
$ fproton doctor --only keyring --fix --allow-keyring --dry-run --json | jq -c '[.actions[]|{class,action}]'
[{"class":"C","action":"SetAlias default: /org/freedesktop/secrets/collection/Default_5fkeyring
                                        → /org/freedesktop/secrets/collection/Default"},
 {"class":"C","action":"write /home/oliver/.local/share/keyrings/default (8 bytes)"}]
```

Class A/B apply → assert → undo → compare, at the binary level, entirely inside a temp
tree (`FPROTON_STATE_DIR`, `FPROTON_CACHE_DIR`, `XDG_CONFIG_HOME` all under `$T`, so no
real desktop file is touched):

```
$ printf '{ this is not json' > $T/state/state.json
$ fproton doctor --only install --fix --yes --json | jq -c '{exit:.exit_code,actions:…,fixed:…}'
{"exit":0,"run_id":"d51b38",
 "actions":[{"class":"A","action":"write $T/state/state.json (425 bytes)","outcome":"applied"},
            {"class":"B","action":"symlink $T/config/omarchy/plugins/oliver.protonvpn → …/plugin/oliver.protonvpn","outcome":"applied"},
            {"class":"B","action":"write $T/config/omarchy/extensions/omarchy-menu.jsonc (856 bytes)","outcome":"applied"}],
 "fixed":[{"id":"state_dir","fix_outcome":"fixed","verdict":"pass"},
          {"id":"plugin_linked","fix_outcome":"fixed","verdict":"pass"},
          {"id":"menu_entries","fix_outcome":"fixed","verdict":"pass"}]}

$ cat $T/state/doctor/latest/actions.jsonl
{"seq":1,"check":"state_dir","class":"A","action":"write_file","before":{"existed":true,"sha256":"0fb1db0e…","backup":"backups/…/state.json"},"inverse":"restore_file"}
{"seq":2,"check":"plugin_linked","class":"B","action":"create_symlink","before":{"existed":false},"inverse":"remove_symlink"}
{"seq":3,"check":"menu_entries","class":"B","action":"write_file","before":{"existed":false,"sha256":null},"inverse":"quarantine_file"}

$ fproton doctor undo latest --json | jq -c '.steps[]|{seq,inverse,outcome}'
{"seq":3,"inverse":"quarantine_file","outcome":"moved to …/runs/20260914T221106Z__d51b38/quarantine/…/omarchy-menu.jsonc"}
{"seq":2,"inverse":"remove_symlink","outcome":"removed"}
{"seq":1,"inverse":"restore_file","outcome":"restored"}
$ cat $T/state/state.json          { this is not json      (byte-for-byte the corrupt original)

$ fproton doctor --only install --fix --yes --json | jq -c '{fixed:.summary.fixed}'   {"fixed":3}
$ fproton doctor --only install --fix --yes --json | jq -c '{fixed:.summary.fixed}'   {"fixed":0}   (run twice = run once)
```

Build and test gate:

```
$ cargo fmt --check                                     (no diff)
$ cargo clippy --all-targets -- -D warnings             clean
$ cargo test                                            51 passed (bin, 21 of them doctor) + 83 passed (core); 0 failed
$ cargo build --release                                 Finished
```

Safety greps over every doctor output surface (`doctor --json`, `-v`, `capabilities`,
`--fix --dry-run --allow-keyring`):

```
$ … | grep -aoE 'PrivateKey|RefreshToken|AccessToken|x25519pk|X25519PublicKey|[A-Za-z0-9+/]{43}=|keyrings/[A-Za-z_]*\.keyring'
(no hits)
$ grep -rnE '"(sh|bash)"|sh -c|Command::new' crates/fproton/src/doctor/     (no hits — argv only, through core::proc)
$ grep -rl 'LogicalServers|X25519' <a --save run's artifacts>               (no hits — facts, never the body)
```

Not verified at this gate, and why:

- **No Class C fix was ever applied.** The keyring repair is planned and printed only; the
  brief forbids applying it during development and the owner will repair the keyring after
  a reboot. Its inverse (`SetAlias default /`, plus restoring the pointer file's bytes) is
  therefore recorded but never replayed on this machine.
- **`killswitch_orphan`'s fixer has never run.** The condition does not hold here (both
  profiles inactive), and producing it means bringing the IPv6 leak guard up on the owner's
  machine. Its detector is covered by the `killswitch_orphan__orphan` fixture, which reaches
  the fixable branch with all four preconditions satisfied.
- **`plugin_enabled` and `old_plugin_enabled` fixers have never run**: both checks already
  pass here, so `omarchy plugin enable|disable` was never invoked by the doctor. Their
  inverses are recorded in the plan, not exercised.
- **`dns_when_connected` / `tunnel_route_ok`** are `skip:link_down` and stay §4b RED: there
  has never been a `proton0` on this machine.
- **A real `menu_entries` fix against the owner's own menu file** was not run; it was
  applied and undone only against a copy inside `$XDG_CONFIG_HOME=$T`. `fproton setup
  --menu` owns that path in the real run.

- 2026-09-15 · plugin · Real line counts for the widget: `Panel.qml` 924 (§6.1 budget 850, explicitly soft), `Service.qml` 360 (≤ 500), `Model.js` 172 (≤ 180), `ProtonIcon.qml` 90 (≤ 90), `manifest.json` 34; QML total 1 374 against the < 1 500 ceiling. The overrun in Panel.qml is the section machinery (`navSections`/`sectionCount`/`moveCursor`) plus the four reusable row components; the Tailscale panel spends 1 273 lines on a smaller model.
- 2026-09-15 · plugin · **`omarchy-shell shell rescanPlugins` does not re-read the QML of an already-loaded symlinked plugin on this machine.** Verified by appending a deliberate syntax error to `Panel.qml`: the rescan logged nothing, the widget kept running the old code, and `tests/plugin-smoke.sh` passed 19/0. `omarchy restart shell` then reported `WARN qml: Plugin widget oliver.protonvpn failed: …Panel.qml:924:1: Syntax error` and the IPC target vanished. (`shell.qml:1470` guards its reload with `if (typeof Qt.clearComponentCache === "function")`.) The dev loop for this widget is therefore `omarchy restart shell`, not the rescan DESIGN §6.8 names; every screenshot below was taken after a restart.
- 2026-09-15 · plugin · Because of the above, `tests/plugin-smoke.sh` grew two checks §6.9 did not ask for: a `qmllint` syntax gate (`/usr/lib/qt6/bin/qmllint`, filtered to `Syntax error|[syntax]` because `qs.Ui`/`qs.Commons` cannot be resolved outside the shell's import path), which does catch the broken file, and an `omarchy-shell oliver.protonvpn cursor` probe, which proves the widget instantiated rather than merely that the journal is quiet. `--static` skips the reload half for a machine with no shell running.
- 2026-09-15 · plugin · The "Taking long… force disconnect" row fires at `elapsed_sec >= min(deadline_sec, 15)`, not at §6.6's `elapsed_sec >= deadline_sec`. §5 has `status` retire a pending the instant it passes its deadline, so the literal condition is only true for the gap between the two polls and the row would never be seen; 15 s is the case it exists for (a connect with 75 s still to run and no other way out). Observed live: `fproton connect fastest --timeout 25` from a terminal, panel showed the row at 17 s and `cursor` listed section `hung`.
- 2026-09-15 · plugin · The country list is alphabetical by country name, capped at 8 rows with an "N more — press / to filter" line. Sorted by load (the first cut) the default view was Congo/Mauritania/Libya/Macao — every 0 %-load country nobody is looking for — and 149 rows is more than a panel should instantiate. `/` filters over all 149.
- 2026-09-15 · plugin · `Service.checkBinary()` re-checks when the binary name changes under it (`onBinChanged`), and its `which` handler discards an answer about a name that is no longer current. Without that, editing the `binary` setting left the widget polling a name that does not exist while still claiming the last good Status: the in-flight `which fproton` answered for `fproton-nope`. Verified both directions by `omarchy bar set oliver.protonvpn binary fproton-nope` (panel: "fproton is not installed or not on PATH (~/.local/bin)", bar icon dim + crossed + badge) and back (alive again with no restart). The user's `shell.json` was restored byte-for-byte afterwards.
- 2026-09-15 · plugin · A missing binary pauses polling entirely until the panel is opened again (§6.4), rather than re-running `which` on every tick — at the idle interval that would be a spawn every 8 s forever.
- 2026-09-15 · plugin · The `tuiCommand` setting is honoured through `Model.safeArgv`: the string is split on whitespace and every token must match `^[A-Za-z0-9._:@/=+-]+$`, otherwise the constant argv `["omarchy-launch-or-focus-tui","--app-id=fproton","fproton","tui"]` is used. Nothing is ever joined into a string, so the `eval` inside `omarchy-launch-or-focus-tui` only ever sees inert tokens. README (docs agent) documents the setting as working, and this is how it works without breaking §1.5.
- 2026-09-15 · plugin · §6.6's section list has no reconnect row, so `reconnect` / `reconnect --next` are reachable only through the widget's own `IpcHandler` (`reconnect`, `nextServer`) — a keybinding or menu entry can call them. `cursor` is a test hook returning `{section,index,active,opened,sections,filter,alive,binaryMissing,state}`; `tests/plugin-smoke.sh` and every keyboard check in this gate use it instead of screenshots.
- 2026-09-15 · plugin · Actions get no client-side watchdog (the status poll does). Every action argv is an `fproton` command that carries its own process-group deadline — 90 s for connect, 25 s for disconnect — so a hung action ends on its own; a second watchdog in QML could only kill fproton while the Proton CLI it spawned kept running.
- 2026-09-15 · plugin · Notifications stay default-off and the transition path additionally refuses to fire on the first Status of a session (`_seenStatus`), so a shell restart into a connected tunnel is not announced as a connect.

## Phase 2 plugin gate (2026-09-15, real output)

```
$ omarchy-plugin-validate plugin/oliver.protonvpn     (silent, exit 0)

$ tests/plugin-smoke.sh
plugin-smoke: /home/oliver/Projects/odcpw/fproton/plugin/oliver.protonvpn
  ok    omarchy-plugin-validate
  ok    manifest id is oliver.protonvpn … kinds/entryPoint/defaultSection/allowMultiple
  ok    manifest.json Panel.qml Service.qml ProtonIcon.qml Model.js README.md exist
  ok    qmllint: no syntax errors
  ok    Panel.qml sets manageIpc: false
  ok    no execDetached(["bash" …])        ok    no bar.run()
  ok    every manifest default has an in-code fallback
  ok    rescanPlugins                      ok    journal carries no load failure
  ok    widget is loaded (cursor: {"section":"header",…,"alive":true,"state":"disconnected"})
plugin-smoke: 20 passed, 0 failed
```

Observed on this machine (screenshots in the agent's scratch dir, `grim` + `magick` crops):

- The Proton mark is in the bar immediately before `omarchy.tailscale`, and the panel opens.
  Icon states seen for real: dim + crossed + badge (fproton missing), un-crossed + badge
  (a `warn` Status warning — tailscale0 contesting the route), and the hero switch throwing
  to "on" the instant a connect is queued.
- Panel sections rendered against real Status: hero (CH#461 · NOT CONNECTED / ERROR /
  CONNECTING…), Location/Server/Uptime/Traffic detail rows, the three real warnings with
  their hints, QUICK CONNECT, COUNTRIES (Afghanistan… Australia + "141 more"), PROTECTIONS
  "unknown — Proton CLI not responding" with Refresh / Run doctor, footer Open TUI · Doctor ·
  Sign out.
- Keyboard: `j`/`k`/arrows move within and between sections (`cursor` read after each key),
  `/` opens the filter and "switz" narrows 149 countries to Switzerland (811 servers, from
  40 %, free), Esc inside the filter closes the filter and leaves the panel open, Enter on a
  warning copied `tailscale set --exit-node=` to the clipboard (restored afterwards), `t`
  closed the panel and launched the TUI command.
- Actions reached fproton for real, from the panel, and were recorded in `fproton history`:
  `connect AL` (00:09:39, killed at its 90 s deadline), `connect fastest` (refused by the
  60 s hang backoff), `disconnect` (exit 5 upstream) and `disconnect --force` (ok, and both
  NM profiles stayed inactive — the §4b GREEN no-op).

Not verified, and why:

- **The connected icon state, traffic rows and the protections toggles.** Nothing on this
  machine can connect (§4b RED), and `status.config` stays null while the Proton CLI is
  wedged, so the panel only ever renders the "Protections unknown" branch. The toggle/cycle
  code paths for kill-switch, NetShield and port-forwarding were never executed.
- **The RECENT section.** `fproton recents` returns `[]` until a connect succeeds, so the
  section correctly renders nothing — which is all that was observed.
- **Notifications.** Default off; `notifications: "on"` was never set, so
  `omarchy-notification-send` has never been called by this widget.
- **The forwarded-port copy row.** Needs `status.port_forwarding.port`, which needs a tunnel.

## 2026-09-15 — Phase 3 (integrator)

- **clap's own parse errors now go through the §2 error envelope.** `Cli::parse()` let clap
  print its own text and exit, so `fproton --json servers --feature vpn` answered with
  unparseable prose — the single most likely failure for an agent, and the one place the
  documented JSON contract did not hold. `main.rs` now uses `try_parse_from` and renders a
  `usage` error with a runnable hint. `--help`/`--version` (which clap reports as errors of
  kind `DisplayHelp`/`DisplayVersion`) still print normally and exit 0.
- **The hint names the deepest command that was recognised**, so `fproton config set netshield`
  hints `fproton config set --help`, not `fproton config --help`. `clap_path` walks argv
  through one level of nesting for the four commands that take subcommands.
- **A missing subcommand is reported in one line.** clap renders the whole help page for
  `ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand`; pasting a screenful into the JSON
  `error` field is useless, so that kind is special-cased to "`fproton config` needs a
  subcommand" with the same hint.
- **Global flags get `display_order` 100-105** so that `fproton servers --help` lists the
  command's own arguments first and the six global ones after, instead of interleaving them.
  `--yes`, `--dry-run` and `--timeout` now name where they actually bite, because they appear
  on every subcommand including ones that spawn nothing and mutate nothing.
- **`build_stage` moved from `phase-1b-core` to `v0.1-integrated`, and `BUILD_NOTE` and the
  robot-docs BUILD STAGE block were rewritten.** Both still claimed "`tui` and `doctor` are
  Phase-2 stubs that exit 1", which was false in this build — the machine contract was lying
  about the two largest components. The replacement text is the §4b matrix in three tiers and
  claims nothing RED.
- **`tests/e2e.sh` was silently skipping its §4b RED assertion.** The guard was
  `if "$BIN" doctor --json >/dev/null 2>&1`, but the doctor exits 11 whenever it finds a
  failure, which it does on this machine — so the branch that asserts `dns_when_connected`
  and `tunnel_route_ok` report `skip:link_down` never ran, and the suite reported a green
  skip instead. It now judges the doctor by whether its output parses, and checks both
  connected-group checks. This was a passing gate that was not testing what it claimed.
- **The secrets sweep now includes the doctor** (`--only keyring`, 288 ms; `--only cache`;
  `doctor capabilities`). It was the one command reading the Secret Service and it was not in
  the list of commands whose output is grepped.
- **The keyring-path assertion was wrong, not the doctor.** Adding the doctor made
  `grep -q "/.local/share/keyrings"` fail on
  `/home/oliver/.local/share/keyrings/default` — which §2 names as the one sanctioned path
  (a pointer file holding a keyring basename, never key material). The blunt string ban was
  replaced by what §2 actually protects: any `keyrings/` path other than `default` fails, and
  any `*.keyring` FILE fails. Verified non-vacuous by injecting
  `…/keyrings/Default.keyring` and `…/keyrings/user.keystore` into a copy of the captured
  output — both are caught, today's real output is clean.
- **`scripts/check.sh` ran clippy and the test suite twice each**, once for display and once
  for the exit status, doubling the gate and letting a second run disagree with the output
  printed from the first. Each now runs once, into a variable.
- **Accident, recorded:** while probing error paths I ran a real `fproton connect ch`, which
  is session-touching. It was killed at its 90 s deadline and returned exit 5 as designed, and
  left the expected 60 s `cli.hang` backoff and a `last_error` in state. Nothing was
  connected and no NetworkManager profile changed (the e2e "desktop is as it was" check
  passes). Noted because §1.2 says not to run such commands unbounded, and I did.
- **`<text>` is not a runnable command.** A 21-case corpus of wrong invocations found one
  hint that could not be run: `fproton servers --search <text>`, produced by
  `shell_safe_hint` whenever a rejected value contained anything shell-shaped. The intent
  (never paste an unsanitised value into a hint) was right; the result violated §2's "hint:
  `<runnable command>`". `search_hint` now keeps the value's *leading* safe run —
  `CH#461 foo` hints `--search CH`, which is both runnable and finds what the caller meant,
  and stops at the `#` that would comment out a pasted line — and falls back to bare
  `fproton servers` when there is no safe prefix at all. `Error::Ambiguous`'s default hint
  lost its `<text>` the same way. A test asserts no hint for `; rm -rf`, `$(id)`, `` `id` ``,
  `a|b`, `--` or `&& reboot` contains a shell metacharacter.

## 2026-09-15 — Phase 4 (fixer)

- **correction** · the Phase-1b §4b matrix row for `dns_when_connected`/`tunnel_route_ok`
  cited an e2e `skip` that never ran: the guard short-circuited on the stub doctor's exit 1
  (see the Phase-3 entry above), and a Phase-1b stub doctor emitted no checks at all. That
  assertion first ran, and passed, at the Phase-3 gate; it passes here too.
- **`run_with_deadline` handed its deadline to whatever held the pipe.** The two drain
  threads were `join`ed after the group kill, so a descendant that had left the process
  group (`setsid`, a daemonising helper) kept the inherited write end open, the pipe never
  reached EOF, and the call blocked for ever — DESIGN §2's first invariant. Reproduced with
  a fake `protonvpn` running `setsid sleep 40 & sleep 40`: `fproton account --timeout 3` ran
  the full 20 s and had to be killed from outside (exit 124). Reading now happens inside the
  wait loop through `poll(2)` on non-blocking fds, with a 200 ms bounded drain after the
  reap; the same case now returns in 3.2 s with exit 5. Regression test:
  `a_descendant_outside_the_process_group_cannot_defeat_the_deadline`.
- **The poll rewrite cost `status` 20 ms until `REAP_POLL` was added.** Once both pipes hit
  EOF the pump had nothing to poll and slept the full 25 ms `POLL` waiting for the reap —
  reintroducing the old code's granularity *after* paying for the poll. Measured, 40 runs
  interleaved against the pre-fix binary: `status --json` 53.2 ms vs 34.8 ms. With a 2 ms
  `REAP_POLL` once EOF is reached it is **26.2 ms against 30.9 ms pre-fix** — faster than
  before the fix, and inside the §2 budget.
- **`state::update` holds an advisory `flock` across the read and the write**, and `status`
  commits only the five fields it owns through it instead of saving the snapshot it loaded
  before the probes. Atomic rename alone did not stop two writers from reverting each other:
  a `pending` record, a history row or a recents entry written during the tens of
  milliseconds of probing was silently rolled back. The regression test
  (`concurrent_updates_do_not_revert_each_other`, 4 threads × 20 pushes) fails without the
  lock and passes with it — verified both ways.
- **SIGINT now cancels rather than kills.** The handler stores a signal number; a watcher
  thread turns that into `CancelToken::cancel`, and re-raises with the default disposition
  3 s later so fproton can never become unkillable. Before: Ctrl+C during a connect left the
  child alive (its own process group never saw the terminal's SIGINT) and `pending` behind.
  Verified with a fake CLI: exit 130 through the JSON envelope, no orphan, `pending` cleared,
  and a `cancelled` history row.
- **`signin` refuses without a TTY** (exit 2, hint
  `omarchy-launch-floating-terminal-with-presentation fproton signin`). §2 grants it the only
  deadline-free spawn *because* it is "interactive, only from a terminal"; nothing enforced
  that, so from a script it hung for ever and wrote the child's password prompt into the
  stdout that is implied `--json`.
- **The menu merge put its comma inside a comment.** `merge_menu` appended the separating
  comma after `head.trim_end()` — the last non-blank line, which in a commented JSONC file is
  very often a `//` line. Reproduced in a sandbox menu file; the merged file no longer
  parsed. Both the insertion point and the comma now skip the trailing comment run, and
  `rfind('}')` ignores a brace inside a comment.
- **`fn diff()` could not show a removal**, so the one line the merge rewrites (the previous
  last entry, which gains a comma) rendered as a pure addition — reading as a duplicate of
  the owner's own entry in the only preview of the only user-owned file fproton writes. It
  now emits `-` lines too, and README:70 and the covering test were renamed and reworded to
  say what actually happens.
- **`parse_config_list` panicked on non-ASCII upstream output.** `str::split_at` at a byte
  column taken from the rule line aborts the process when the index lands inside a character:
  exit 101, no envelope, no §4a code. Reproduced with a fake CLI whose `ipv6` row put `ü`
  across the boundary. The cut walks back to a char boundary, with the §8 "run of two spaces"
  rule as a fallback.
- **`connect --wait` was parsed, documented and ignored**, which made §4a's exit 8
  unreachable from any command. It now polls `nm::link_state` until `proton0` is up or the
  connect deadline passes. Verified with a fake CLI that exits 0 without bringing anything
  up: `connect CH --wait --timeout 4` → exit 8, `code_name: timeout`, 4 s.
- **The doctor's write-scope guard was one-sided and `..`-blind.** `Path::starts_with`
  compares components without resolving `..`, and `undo` checked nothing at all. `in_scope`
  now rejects any `..`, the backup/quarantine path mapping refuses one, and
  `restore_file`/`quarantine`/`remove_symlink` are behind the same guard, so undo's blast
  radius equals apply's.
- **The doctor journalled after it mutated**, contradicting its own refusal text ("a mutation
  must be recorded before it is applied"). `write_file` now fsyncs an `ok:false` intent line
  before the rename and an `ok:true` completion line after; `undo` reports an intent with no
  completion as "unknown — the run stopped between the journal entry and the write", naming
  the backup, rather than skipping it in silence.
- **`ClearPending` rebuilt state.json from the probe copy**, captured seconds earlier —
  `cli_session_probe` alone spends 8 s (measured again today: killed at its deadline after
  8014 ms) — so anything written in between was rolled back. It re-reads the file now.
- **The index scratch directory left `/tmp`.** It was `fproton-doctor-index.<pid>.<now>`,
  fully predictable and world-writable, created with `create_dir_all` (which happily reuses
  someone else's directory) and read back into the real server index. It now lives under
  fproton's own cache dir and is created with `create_dir`, so a pre-existing directory is a
  refusal.
- **The "no shell strings in argv" gate could not match the argv it bans.** The pattern
  required the comma to follow `bash` directly, leaving no room for the closing quote, so
  `["bash", "-c", …]` and `["sh", "-c", …]` both passed while the step printed "no sh -c, no
  bash -c, anywhere in the Rust sources". Verified vacuous against a two-line probe file
  (`grep rc=1`), fixed, and the step now proves itself against 3 known violations on every
  run before it is believed.
- **TUI:** the 60-99 column Status branch dropped the whole Warnings panel, so at 80x24 —
  a default terminal — `conflicting_tunnel` was invisible; it is back, with the fixed slots
  shrinking on a 24-row terminal rather than any panel vanishing, and warnings ordered
  most-severe-first so the row that gets cut is never the error or the warn. Verified in a
  real 80x24 tmux pane. The §7a index empty state was gated on the substring "index" while
  the core's message is "Proton's server list is missing at …", so "Press i to build it"
  never appeared on a real install — `servers_err` now carries `(code_name, text, hint)`.
  The CLI hang breaker is a persistent row as well as a refusal, from one shared string.
  `Esc` with nothing running says so. Quitting with `q q` during a connect no longer drops
  §7a's follow-up `disconnect --force`: the debt is settled in `tui::run` after the runtime
  has torn the child group down, so the teardown cannot race the connect it cleans up.
- **Plugin:** `activateWarning` set `svc.actionStatus` directly, so "Copied: <hint>" never
  expired (§6.4 says 2.2 s); `Service.flash()` is now the only way to set that label. The
  panel opened with Protections and the whole footer below the fold behind a QQC2
  `AsNeeded` scrollbar, which is invisible until you scroll: the country list is capped at 4
  while the filter is closed (8 once it is open) and the scrollbar is `AlwaysOn` whenever
  there is anything below the fold.
- **`natpmp` claimed a TCP request it never makes.** `request_port` only ever sends the UDP
  opcode; the doc comment said UDP and TCP were requested separately. v0.1 stays UDP-only —
  adding an untestable fallback on a path that is §4b RED would be a claim we cannot stand
  behind — and the comment, `port --help` and this entry say so instead.
- **`--help` for `connect` and `port` now carries the RED disclosure** that `capabilities`,
  README and `robot-docs` already carried. An agent that reads only `--help` was told nothing.
- **`agent.rs`'s "never" claim was absolute and wrong.** "It never reads keyring files"
  contradicts the doctor's Class C fixer, which rewrites `~/.local/share/keyrings/default`.
  Both the `capabilities` key and the `ROBOT_GUIDE` paragraph now mirror README's wording;
  `docs/ROBOT.md` was regenerated.
- **Accident, recorded:** a first cut of the `reconnect` regression test called
  `engine::reconnect` with an empty cache dir, where `resolve` degrades to `Ok(())` — so it
  spawned a real `protonvpn connect`, which ran to its 90 s deadline and returned exit 5.
  Nothing connected (`proton0` absent, no `ProtonVPN` profile active, checked immediately
  after), and the e2e "the desktop is as it was" check passes. §1.2 says not to run such
  commands, and I did. The test now points the index at `tests/fixtures/serverlist-small.json`
  and asserts `resolve` fails *before* calling `reconnect`, so it cannot spawn anything.

### §4b verification matrix (v0.1-integrated), re-filled from this run

| Command | Class | How verified today | Evidence | Claimed in README? |
|---|---|---|---|---|
| `status` | GREEN | `tests/e2e.sh --full` | `status --json` exit 0, every §5 key, schema 1; mean **24.5 ms** over 20 runs (§2 budget 50 ms) | yes |
| `servers`/`countries`/`cities`/`index` | GREEN | e2e | `servers --country CH --json` mean **7.8 ms** (budget 20 ms); 18 221 servers, 149 countries | yes |
| `settings`/`history`/`recents`/`version` | GREEN | e2e | exit 0, valid envelopes | yes |
| `capabilities`/`robot-docs` | GREEN | e2e | `build_stage = v0.1-integrated`, 23 commands, 15 exit codes | yes |
| `setup --dry-run` | GREEN | e2e | exit 0 in 170 ms; `.dry_run = true`; the owner's `omarchy-menu.jsonc` byte-identical afterwards | yes |
| `tui` | GREEN | real tmux panes, 80x24 and 120x40, plus 28 unit tests | Status renders hero, Traffic, Protections, **Warnings** and Recents at both sizes; `q` quits cleanly | yes |
| `doctor` | GREEN | `fproton doctor --json` on this machine | 26 checks: 18 pass, 4 warn, 2 fail, 2 skip; `secret_service_default_alias` = `fail:phantom_collection` as §1.2 predicts | yes |
| `disconnect --force` | GREEN | e2e | no-op while both profiles are inactive; the active-connection set is unchanged after the suite | yes |
| `port` (no tunnel) | GREEN | e2e | exit 6 `not_found` | error path only |
| `signin` (validation) | GREEN | unit tests + a non-TTY run | a bad username is exit 2; **no TTY is now exit 2**, not a hang | yes |
| `connect`, `reconnect`, `disconnect`, `config list\|get\|set`, `account`, `signout` | AMBER | e2e `--full`, own state dir, `--timeout 3` | each exit 5 `upstream` in 3.03-3.11 s with a runnable hint; `account` at the un-overridden 15 s probe deadline exits 5 after 15 044 ms; the second call in the same state dir exits 5 in **11 ms** on the backoff | deadline path only |
| `connect`/`reconnect` **success**, `port` **success**, `wg *` | RED | not verifiable: the Proton CLI is wedged (§1.2) | — | never; `--help` now says so for `connect` and `port` too |
| doctor `dns_when_connected`, `tunnel_route_ok` | RED | e2e asserts the skip | both report `skip`: "proton0 does not exist; this check needs a live tunnel and has never run" | never |

Gate for this pass: `scripts/check.sh --full` → fmt clean, clippy clean, **149 unit tests**,
core-is-silent clean, the argv gate non-vacuous (3/3), `tests/e2e.sh --full` **96 passed, 0
failed, 0 skipped**, `tests/plugin-smoke.sh` **20 passed, 0 failed**. The one real
`scripts/install.sh --yes` run, and therefore the `~/.local/bin/fproton` staleness the
honesty reviewer found, belongs to the Phase-4 verifier and was deliberately not done here.

## 2026-09-15 — Phase 4 (final verifier)

Everything below was run on `t14` against commit `d77fe30` with a clean tree
(`git status --porcelain` empty before and after; only DECISIONS.md changes in this entry).

- **Gate.** `scripts/check.sh --full` → `check.sh: everything green (full)`: `cargo fmt
  --check` no diff, `cargo clippy --all-targets -- -D warnings` clean, `cargo test`
  **62 + 87 = 149** passed / 0 failed, core-is-silent clean, the argv gate non-vacuous
  (3/3 in its self-test), `tests/e2e.sh --full` **96 passed, 0 failed, 0 skipped**,
  `tests/plugin-smoke.sh` **20 passed, 0 failed**.
- **The one real install.** `scripts/install.sh --yes` replaced a genuinely stale binary —
  `~/.local/bin/fproton` was Sep 14 23:47 and answered `build_stage: phase-1b-core`, exactly
  the staleness the honesty reviewer predicted. Afterwards `sha256sum` of
  `~/.local/bin/fproton` and `target/release/fproton` are the same
  (`5451f260ceac6c84…`), `command -v fproton` is `/home/oliver/.local/bin/fproton`, and
  `fproton version` → `{"build_stage":"v0.1-integrated",…,"version":"0.1.0"}`.
  `setup --all --yes` reported 4 steps `already` and one `changed`: `menu_entries`, backed
  up verbatim to `~/.local/state/fproton/backups/omarchy-menu.jsonc.20260915T034933Z`. The
  merged `omarchy-menu.jsonc` still parses (comments stripped, `jq` → 10 top-level keys) and
  the five fproton ids sit under a `// fproton` marker before the final `}`. A second
  `setup --all --yes` reported all five steps `already` and wrote no new backup — the
  idempotence §4 requires, now observed on the real file rather than a sandbox.
- **The widget is live.** `tests/plugin-smoke.sh` standalone: 20 passed, 0 failed, including
  the IPC cursor query answering from the running shell
  (`"alive":true,"binaryMissing":false,"sections":["header","warnings","quick","protections","footer"]`).
  `omarchy plugin list` → `oliver.protonvpn  enabled  third-party  bar-widget  Proton VPN`;
  `shell.json` `bar.layout.right` = `… oliver.torrents, oliver.protonvpn, omarchy.tailscale …`,
  i.e. placed before `omarchy.tailscale` as §6.8 asks. `io.github.grichard99.omaproton-vpn`
  is still `disabled`.
- **Commands, as run from `/tmp` with the installed binary.** `status --json` — a complete
  §5 object, `state: "error"` (the `last_error` an earlier AMBER run left behind; §5's table
  says so and only a new action clears it), 18 221 servers, `signed_in: true`, `max_tier: 2`,
  three warnings (`serverlist_stale`, `protections_unknown`, `conflicting_tunnel`).
  `doctor` → exit **11**, 26 checks, **19 pass / 3 warn / 2 fail / 2 skip**; the two failures
  are `cli_session_probe` (`fail:hang`, `config list` killed after **8015 ms**) and
  `secret_service_default_alias` (`fail:phantom_collection`), exactly §1.2's diagnosis. (The
  Phase-4 fixer recorded 18/4/2/2; the difference is `menu_entries`, which this install
  turned from warn to pass.) `servers --country CH --limit 3` → CH#461/473/481, Zurich, loads
  63/66/60. `countries` → 149. `disconnect --force --yes --dry-run` → exit 0 in 9 ms, printing
  `["nmcli device disconnect proton0","nmcli connection down pvpn-killswitch-ipv6"]` and
  changing nothing. `port` → exit 6 `not_found`. `tui` in real 100x30 and 100x20 tmux panes:
  Status, Servers (18221/18221 shown) and History all render, `q` exits 0.
- **The one bounded session probe.** `timeout 30 fproton connect --timeout 5` → exit **5**
  `upstream`, `"protonvpn connect was killed at its 5s deadline"`, hint
  `fproton doctor --only cli`. Immediately after: the active NetworkManager connection set is
  unchanged (`oliwifi`, `tailscale0`, `lo`), `proton0` does not exist, `pending` is null, a
  `timeout` history row was appended and `cli.backoff_until` was set 60 s ahead. I did not
  capture its wall clock (`bc` is not installed on this box); the same path measured in the
  e2e run above is 3069 ms at `--timeout 3`.
- **Timing, honestly measured.** Neither `hyperfine` nor `/usr/bin/time` is installed here,
  so this is a bash loop of 20 invocations timed as one batch with `date +%s%3N`:
  `status --json` **25 ms** mean, `servers --country CH --json` **9 ms** mean (§2 budgets 50
  and 20 ms). Timing each run separately — which pays for two `date` forks per iteration —
  gives 37 ms / 11 ms mean with a 43 ms max, so the budget holds either way.

### §4b verification matrix, re-filled by the Phase-4 verifier from the runs above

| Command | Class | How verified today | Evidence | Claimed in README? |
|---|---|---|---|---|
| `status` | GREEN | run after the real install | exit 0, full §5 object, schema 1, mean **25 ms** over 20 runs | yes |
| `servers`/`countries`/`cities`/`index` | GREEN | run + e2e | `servers --country CH --limit 3` → CH#461/473/481; `countries` → 149; `index --stats` → 18 221; mean **9 ms** | yes |
| `settings`/`history`/`recents`/`version` | GREEN | run + e2e | exit 0, valid envelopes; `version` → `build_stage v0.1-integrated` | yes |
| `capabilities`/`robot-docs` | GREEN | run + e2e | `robot-docs` prints the handbook; `capabilities` carries `build_stage`/`build_note` | yes |
| `setup` (real, `--all --yes`) | GREEN | **the one real run**, via `scripts/install.sh --yes` | 4 `already` + `menu_entries` `changed` with a verbatim backup; the second run is all `already` | yes |
| `setup --dry-run` | GREEN | e2e | exit 0, `.dry_run = true`, the owner's menu file byte-identical | yes |
| `tui` | GREEN | real tmux panes 100x30 and 100x20 | all three screens render, `q` → exit 0 | yes |
| `doctor` | GREEN | run on this machine | exit 11; 26 checks, 19/3/2/2; the keyring check is `fail:phantom_collection` | yes |
| plugin `oliver.protonvpn` | GREEN | `tests/plugin-smoke.sh` + `omarchy plugin list` + `shell.json` | 20/0; enabled; loaded in the running shell; placed before `omarchy.tailscale` | yes |
| `disconnect --force` | GREEN | run `--dry-run` + e2e | argv printed, 9 ms, nothing changed; still a no-op while both profiles are inactive | yes |
| `port` (no tunnel) | GREEN | run | exit 6 `not_found`, hint `fproton connect p2p` | error path only |
| `signin` (validation) | GREEN | e2e + unit tests | bad username → exit 2; no TTY → exit 2 | yes |
| `connect`, `reconnect`, `disconnect`, `config list\|get\|set`, `account`, `signout` | AMBER | e2e `--full` + one bounded real `connect` | each exit 5 `upstream` in 3.02–3.07 s with a runnable hint; `account` at the 15 s probe deadline exits 5 after 15 044 ms; the backoff answers a second call in 6 ms; my real `connect --timeout 5` exit 5, nothing connected | deadline path only |
| `connect`/`reconnect` **success**, `port` **success**, `wg *` | RED | still not verifiable: the Proton CLI is wedged (§1.2) | — | never; `connect --help` and `port --help` say so |
| doctor `dns_when_connected`, `tunnel_route_ok` | RED | doctor run + e2e assertion | both `skip:link_down`: "proton0 does not exist; this check needs a live tunnel and has never run on this machine" | never |

No row moved class in this pass. Nothing that was RED became verifiable; nothing that was
claimed GREEN failed. The only material change to the machine is the install itself
(binary, menu entries), reversible with `scripts/uninstall.sh`.
- 2026-09-15 · orchestrator · `setup` positions the widget with `omarchy bar move … --before omarchy.tailscale` instead of `bar put`: `omarchy plugin enable` already places the widget at the section's default slot and `bar put` leaves an already-placed widget where it is (`/usr/share/omarchy/bin/omarchy-bar`: "a widget already on the bar is left where it is"), so on a fresh install `put` never repositioned it (the Phase-2 plugin agent moved it by hand). A missing anchor widget is not a failure: the enable-time placement stands. README snippets updated to match. Verified: fmt/clippy/tests green, `setup --all --dry-run` reports five `already` on this machine.
- 2026-09-15 · orchestrator · `conflicting_tunnel` narrowed to real conflicts after the owner asked whether fproton "switches" between Tailscale and Proton (it does not, and need not). Verified on this machine: with no exit node Tailscale's table 52 holds only `100.x/32` routes and no default; an exit node is what adds `default dev tailscale0`. New `fproton-core::route` reads `ip -j route show default` and `table 52` (6 ms, only when another tunnel is up); a conflict is a tunnel owning a main-table default route or a Tailscale exit node. `status` and the doctor now pass for a merely-up `tailscale0` (detail explains coexistence), and a new info warning `killswitch_blocks_tailnet` appears once the cached config shows the kill switch on, because it blocks direct tailnet peer traffic while connected (old plugin README, "Using it with Tailscale"). Fixtures: the captured no-exit-node case passes; `conflicting_tunnel__exit_node` warns. Gate after: clippy clean, 154 tests, e2e --fast 83/83, plugin-smoke 20/20; binary reinstalled; `fproton status` on this box now shows only the two info warnings.

## 2026-09-15 — first connect after the keyring repair (orchestrator, live on the owner's machine)

The owner rebooted; the reboot rebuilt the login keyring, so the Secret Service `default`
alias now resolves to a real unlocked collection (`Default_5fkeyring`) and the Proton CLI
answers in ~200 ms. The old session was gone with the old keyring, so the owner ran
`fproton signin odc` in a terminal (interactive; "Successfully signed in as 'odc'"). Everything
below is real output from this machine; times are wall clock.

| Command | Result |
|---|---|
| `fproton doctor --only keyring` | `secret_service_default_alias: pass` (alias → Default_5fkeyring, unlocked); before sign-in `proton_session_item: fail:not_signed_in`, after: `pass — 2 item(s) with service=Proton` |
| `fproton doctor --only cli` | `cli_help_fast: pass (213 ms)`, `cli_session_probe` before sign-in `fail:not_signed_in` (no hang), after: pass |
| `fproton account --json` | `{"name":"odc"}` |
| `fproton config list --json` | netshield malware-only · kill-switch off · port-forwarding off · ipv6 on · vpn-accelerator on · moderate-nat off · custom-dns off · anonymous-crash-reports on |
| `fproton connect CH --wait --timeout 90 --json` | `ok:true` in **1.83 s**; status: `state connected`, `link.up`, `proton0`, server **CH#507** Zurich load 41, traffic non-null |
| `fproton doctor --only connected` | `dns_when_connected: pass — proton0 uses Proton's resolver 10.2.0.1`; `tunnel_route_ok: pass — 1.1.1.1 routes via proton0` — first time these left `skip` |
| `curl -s https://ifconfig.co/json` | `{"ip":"185.230.125.15","country":"Switzerland","asn_org":"M247 Europe SRL"}` |
| `tailscale status` | all peers still listed while connected (kill switch off; no exit node) — the coexistence rule from the `route` change holds live |
| `fproton disconnect --json` | `ok:true` in **0.86 s**; status `disconnected`, both NM profiles down (the leak-guard dummy goes down with the tunnel) |
| `fproton reconnect --json` | `ok:true` in **1.64 s**; back on CH#507 |
| `fproton config set netshield malware-only --json` | `ok:true` in 0.79 s (set to its current value on purpose: exercises the path without changing the owner's settings); `config get netshield` → `malware-only` |
| `fproton port --json` | exit **5** `upstream`: "the NAT-PMP gateway 10.2.0.1:5351 did not answer in 4 tries (Connection refused)", hint `fproton config set port-forwarding on` — expected with port forwarding off; `port` success stays RED |
| `fproton doctor --json` (connected) | `{"total":26,"pass":26,"warn":0,"fail":0,"skip":0}` |
| `fproton history --limit 3` | `connect CH → ok, CH#507, 1 s` on top of the pre-repair `timeout` rows |
| bar widget (`omarchy-shell oliver.protonvpn cursor`) | `{"alive":true,"state":"connected"}` |

Two observations, both handled:
- **Transient `secret_service_broken` / `signed_in: null`** in the first `status` after
  sign-in: the 60 s-cached Secret Service probe ran (or timed out at 1 s) while the CLI was
  still writing the session; the next probe returned `signed_in: true` and the warning
  cleared. Left as is — a one-poll flash under a bounded probe is the designed behaviour.
- **`kill_switch.active` is the leak guard, not the setting.** `pvpn-killswitch-ipv6` was up
  during the connection although `config.kill-switch` is `off`: the CLI raises that dummy
  profile (IPv6 leak protection) for every connection. The TUI row that said
  "kill switch: active" now says "leak guard: up (ipv6 leak guard profile)"; DESIGN §5 and
  the widget's Protections rows take the kill-switch mode from `config.kill-switch` only.

§4b after this session: GREEN now also covers `signin`, `account`, `config list|get|set`,
`connect`, `disconnect`, `reconnect`, `dns_when_connected`, `tunnel_route_ok`. AMBER: `signout`
(never run; it would end the session) and the deadline/backoff paths (still asserted by e2e).
RED: `port` success (needs port forwarding on and a P2P server), `wg *` (kill criterion
unchanged; certificate expiry 2026-09-21).

A pre-existing unit test (`status_resolves_the_last_server_from_the_persistence_file_by_uuid`)
failed once the tunnel was up: it injects a fixture persistence file but runs the real
`status()`, which correctly prefers the active profile (§5 rule 1). Made state-aware in the
same commit: it asserts the fixture branch when no Proton profile is active and the
active-profile branch otherwise, instead of assuming a disconnected machine.
- 2026-09-15 · orchestrator · Tests written for a wedged CLI made state-aware now that the CLI works: (1) `tests/e2e.sh --full` skips its AMBER block when `cli_session_probe` passes, because on a healthy CLI those rows would run a real `signout`, `config set` and `connect` against the owner's session (the pre-repair transcript stays as the evidence for the deadline paths); (2) the `port` row expects 6 without a tunnel, and 0 or 5-upstream with one; (3) the doctor's connected-group rows expect `pass` with a tunnel up, `skip` without; (4) the core status unit test asserts the active-profile branch when a tunnel is up. Also: a server unknown to the index (newer than the snapshot, or a small fixture) now keeps `country` from its name prefix, as §5 says, instead of nulling it. Gate with the tunnel up: 154 tests, e2e --fast 80/80, e2e --full 82 passed / 1 skipped, plugin-smoke 20/20.
- 2026-09-15 · orchestrator · The owner ran `fproton index --rebuild` because the panel showed "the server loads in Proton's list have expired" with that hint, and the row stayed. A rebuild cannot refresh Proton's list: only Proton's client writes serverlist.json (at connect, verified: mtime = connect time), and its `LoadsExpirationTime` lapses ~15 min later, so expired loads are the normal state between connects and the hint was wrong. Changed: expired loads raise no warning (`index.loads_expired` stays for UIs); `serverlist_stale` for the list itself fires only when it is > 24 h past expiry, hint `fproton reconnect`; the `commands` list suggests `index --rebuild` only for the stale-snapshot flavour; the doctor's `serverlist_expired` passes with a note under 24 h and `serverlist_loads_stale` always passes with the age in its detail. Fixtures: `serverlist_expired` now 49 h past (warn) plus `serverlist_expired__recent` 1 h past (pass). Live: `status.warnings` is `[]` while connected; the panel row cleared on its next poll.
