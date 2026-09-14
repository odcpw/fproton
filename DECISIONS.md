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
