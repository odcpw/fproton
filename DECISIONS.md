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
