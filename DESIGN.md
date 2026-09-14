# fproton — design brief (source of truth for every agent working on this repo)

Proton VPN for Omarchy, rebuilt: a Rust CLI + engine (`fproton`), a FrankenTUI app,
a `doctor`, and a thin Omarchy bar widget (`oliver.protonvpn`). It replaces the
third-party `io.github.grichard99.omaproton-vpn` Quickshell plugin, which is 4.7k
lines of QML/Python and is currently broken on this machine.

Repo: `~/Projects/odcpw/fproton` (GitHub `odcpw/fproton`, pushed later by the owner).
Owner: Oliver (`oliver` on this box, Omarchy 4.0.3, Arch, Hyprland, Quickshell).
Sibling project to copy conventions from: `~/Projects/odcpw/fbittorrent`
(Rust CLI + ftui TUI + `plugin/oliver.torrents` QML widget). Read its README,
`src/main.rs`, `src/tui/{mod,theme}.rs` and `plugin/oliver.torrents/*` first.

## 1. What is broken today and why (verified 2026-09-14)

1. The old plugin fails to load: `Service.qml:820:12: Expected token ','` →
   `Panel.qml:449:3: Type Service unavailable` (journalctl --user, omarchy-shell).
   A prior session hand-patched `Service.qml` (uncommitted, +125/-17) trying to add
   watchdogs for problem 2 and left a syntax error.
2. The official CLI (`/usr/bin/protonvpn`, package `proton-vpn-cli 1.0.3`, Python)
   hangs on **every** command that touches the session: `status`, `info`,
   `config list` all exceed 45 s with no output. `protonvpn --help` is instant.
   Cause: gnome-keyring-daemon crashed (SIGABRT 16:18 today) and was repaired by hand;
   `~/.local/share/keyrings/default` now names `Default_keyring`, but on D-Bus the
   Secret Service exposes collections `Default` (unlocked) and `session` only —
   `/org/freedesktop/secrets/collection/Default_5fkeyring` "does not exist". The CLI's
   keyring lookup triggers a gcr-prompter prompt that never maps on Hyprland, so the
   process blocks forever (journal shows gcr-prompter "couldn't find the callback").
   The owner will fix the keyring after a reboot. **fproton must never depend on the
   CLI answering promptly**: hard timeouts everywhere, and `status` must not call it.
3. Facts about the CLI's on-disk footprint (all readable without the CLI):
   - Server cache: `~/.cache/Proton/VPN/serverlist.json` (24 MB, 18 221 logical
     servers, 149 countries, 195 cities). Top-level keys: `LogicalServers`,
     `MaxTier`, `LastModifiedTime`, `ExpirationTime`, `LoadsExpirationTime`.
     Logical keys: `Name` ("CH#461"), `EntryCountry`, `ExitCountry`, `Domain`, `Tier`
     (0 free, 2 plus), `Features` (bitmask: 1 secure-core, 2 tor, 4 p2p, 8 streaming,
     16 ipv6 — 8/16 verify against Proton docs, keep the raw int too), `City`,
     `Score` (lower = better), `Location{Lat,Long}`, `Status`, `Load` (0-100),
     `Servers[]` physical: `EntryIP`, `ExitIP`, `Domain`, `X25519PublicKey`, `Status`.
   - `~/.cache/Proton/VPN/clientconfig.json`: `DefaultPorts.WireGuard.UDP` = [443,88,1224,51820,500,4500], feature flags.
   - `~/.cache/Proton/VPN/connection/connection_persistence.json`: last connection
     (server_name, domain, server_ip, protocol, backend, x25519pk).
   - `~/.cache/Proton/VPN/logs/vpn-cli.log`: CLI log (state changes, "Waiting for agent status").
   - NetworkManager: the CLI creates a WireGuard profile `ProtonVPN <Name>` on
     interface `proton0` (ipv4 10.2.0.2/32, dns 10.2.0.1, fwmark, peer allowed-ips
     0.0.0.0/0;::/0 endpoint <EntryIP>:443) and a `dummy` profile
     `pvpn-killswitch-ipv6` on `ipv6leakintrf0` for the kill switch. Both exist now,
     both inactive. `nmcli -t -f NAME,TYPE,DEVICE,ACTIVE connection show` is ~10 ms.
   - Daemon: `proton.VPN.service` (system, `python3 -m proton.vpn.daemon`) — split tunneling.
   - Tunnel throughput: `/sys/class/net/proton0/statistics/{rx,tx}_bytes`.
   - Port forwarding: NAT-PMP (RFC 6886) to 10.2.0.1:5351 inside the tunnel, 60 s
     lifetime, renew every 45 s (see old `port.py`; reimplement in Rust, std UDP only).
   - Sign-in is interactive (`protonvpn signin <user>` prompts password + TOTP).
     The old plugin opened it with `omarchy-launch-floating-terminal-with-presentation`.
4. Toolchain: cargo 1.98, rustc 1.98. Crates fetched: ftui 0.7.0 (+ ftui-extras),
   fsqlite 0.4.1 (needs `asupersync = "0.5"` `RuntimeBuilder` + `runtime.block_on`),
   sources under `~/.cargo/registry/src/*/`. fbittorrent used ftui 0.6 — same shape.
5. Omarchy plugin API: `/usr/share/omarchy/shell/plugins/README.md`,
   `/usr/share/omarchy/shell/plugins/bar/README.md`, the Tailscale panel
   (`/usr/share/omarchy/shell/plugins/panels/tailscale/{Panel,Service,TailscaleIcon}.qml`,
   `Model.js`, `manifest.json`) is THE reference for chrome, keyboard model and
   Process usage. `qs.Commons` (Color, Style, Util) and `qs.Ui` (Panel and friends)
   are importable. Validate with `omarchy-plugin-validate <dir>`. User plugins live in
   `~/.config/omarchy/plugins/<id>/` (symlink OK for dev; hot-reloads on save).
   Menu entries: `~/.config/omarchy/extensions/omarchy-menu.jsonc` (see fbittorrent's
   `torrents.*` entries). Skill docs: `~/.claude/skills/omarchy/plugins.md`.
6. Reference copy of the old plugin (MIT, by grichard99): `~/.config/omarchy/plugins/io.github.grichard99.omaproton-vpn/`.
   Port ideas and `WorldMap.qml` + `World.js` (world outline) with attribution in
   the file header; do not copy its Service.qml architecture.

## 2. Non-negotiables

- **Never hang.** Every external process runs in its own process group with a
  deadline (SIGTERM, then SIGKILL 2 s later). Defaults: probes 15 s, `connect` 90 s,
  `disconnect` 25 s, `signin` none (interactive). `status` never spawns `protonvpn`.
- **argv only.** No shell strings, no `sh -c`, no string-built commands. Targets and
  user input are validated (`^[A-Z]{2}(#\d+)?$` etc.) before reaching any argv.
- **No secrets.** fproton never stores passwords, tokens or WireGuard private keys.
  WG configs are handed to NetworkManager (`nmcli connection import`) and deleted
  from any temp location afterwards. Logs redact keys.
- **`--json` everywhere**, one JSON object per invocation, `{"ok":false,"code":N,
  "code_name":"…","error":"…","hint":"<runnable command>"}` on failure with the same
  exit code. Non-TTY stdout defaults to JSON for `status`. `capabilities` and
  `robot-docs` exist (agent-ergonomics skill).
- **Fast.** `fproton status --json` < 50 ms warm. Index queries < 20 ms.
- **Thin plugin.** QML only renders and calls `fproton … --json`. Target < 1 400 lines
  of QML total; no Python; no business logic in QML beyond view state.
- **Honest.** README and `--help` claim only what was verified on this machine. The
  CLI is hung on this box, so CLI-backed actions (`connect` via engine `cli`,
  `config set`, `account`) can only be exercised for timeout/error paths right now.
  Say so in DECISIONS.md and README "Status" section. No mocks presented as tests.
- **Don't reformat or delete files you don't own** (see §8). Never `cargo clean`.
  Append to `DECISIONS.md` (never rewrite) for every non-obvious choice.

## 3. Repo layout (Cargo workspace, resolver 2)

```
Cargo.toml                    workspace (deps pinned here)
crates/fproton-core/          library: engines, state, index, natpmp, nm, cli driver
  src/lib.rs                  pub mod …; pub use the main types
  src/nm.rs                   nmcli parsing + link state + sysfs traffic (+ tests)
  src/cli.rs                  protonvpn driver: spawn w/ deadline, parse status/info/config/countries (+ tests, fixtures)
  src/wg.rs                   WireGuard engine: parse Proton .conf, nmcli import/up/down, endpoint swap (+ tests)
  src/engine.rs               trait Engine { connect, disconnect, name } + Target enum + selection
  src/index.rs                ServerIndex: build from serverlist.json → fsqlite; query API (+ tests on tests/fixtures/serverlist-small.json)
  src/natpmp.rs               NAT-PMP request/response codec + query with backoff (+ tests)
  src/state.rs                XDG state: pending action, last probe results, history, recents, cli-hang backoff
  src/settings.rs             ~/.config/fproton/config.toml
  src/status.rs               derive Status (the JSON contract) from nm + state + index
  src/proc.rs                 run_with_deadline(argv, timeout) → Output{code, stdout, stderr, timed_out}
  src/error.rs                Error → exit code + code_name + hint
crates/fproton/               binary
  src/main.rs                 clap; dispatch; JSON/TTY output; exit codes
  src/output.rs               human tables vs JSON
  src/doctor/mod.rs           doctor subcommand (Phase 2 owner: doctor agent)
  src/tui/mod.rs, theme.rs …  FrankenTUI app (Phase 2 owner: TUI agent)
  src/setup.rs                setup: plugin symlink, menu entries, user unit, old-plugin disable
plugin/oliver.protonvpn/      manifest.json, Panel.qml, Service.qml, ProtonIcon.qml, Model.js, WorldMap.qml, World.js, README.md
scripts/install.sh, scripts/uninstall.sh
tests/e2e.sh                  real-machine, no mocks, must finish < 15 s
tests/plugin-smoke.sh         omarchy-plugin-validate + manifest/jq checks
tests/fixtures/               nmcli outputs, serverlist-small.json, protonvpn status/info/config outputs, wg conf
docs/ROBOT.md                 what `robot-docs` prints (agent handbook)
DESIGN.md (this) · DECISIONS.md · CHANGELOG.md · README.md · LICENSE (MIT)
.github/workflows/ci.yml
```

## 4. CLI contract

Global: `--json`, `--timeout <s>` (override external deadlines), `-v`, `--no-color`.
Bare `fproton` on a TTY → `tui`; otherwise `status --json`.

| Command | Does | Backend |
|---|---|---|
| `status [--fast]` | full Status JSON (§5). Never calls protonvpn. | nm + state + index |
| `connect [TARGET] [--engine cli\|wg] [--wait] [--feature F] [--city C] [--country CC]` | TARGET: `fastest` (default) · `CC` · `CC#N` · `random` · feature name `p2p\|secure-core\|tor`. Records `pending` in state first (optimistic UI), runs engine, records history. | engine |
| `disconnect [--force]` | engine disconnect; `--force` (and automatically after a CLI timeout) tears down `proton0` + `pvpn-killswitch-ipv6` via nmcli. | engine / nm |
| `reconnect` · `change` | reconnect same target · next server in same country by score | engine |
| `servers [--country CC] [--city C] [--feature F] [--tier free\|plus] [--search TEXT] [--sort load\|score\|name] [--limit N]` | from index | index |
| `countries` · `cities [--country CC]` | with server counts, min load, lat/lon, has_free | index |
| `index [--rebuild] [--stats]` | (re)build fsqlite index from serverlist.json; auto-rebuilds when the JSON mtime changes | index |
| `config list\|get K\|set K V` | Proton settings via the CLI with deadline; allowed keys/values whitelisted: `kill-switch {off,standard,permanent}`, `netshield {off,malware-only,malware-ads-trackers}` (map to CLI's actual vocabulary — verify with `protonvpn config --help`), `port-forwarding {on,off}`, `protocol {wireguard,openvpn-udp,openvpn-tcp}` | cli |
| `settings [get\|set K V]` | fproton's own config.toml: `engine`, `timeouts.*`, `default_target`, `notify` | local |
| `port [--keepalive]` | NAT-PMP query; `--keepalive` loops every 45 s until tunnel goes down | natpmp |
| `history [--limit N]` · `recents` | connection history from state | state |
| `account` | `protonvpn info` with deadline; caches result for `status.signed_in` | cli |
| `signin [USER]` · `signout` | interactive in the current terminal (no deadline for signin); from the plugin: launched in a floating terminal | cli |
| `wg import FILE [--name N]` · `wg list` · `wg remove N` | manage imported Proton WireGuard configs in NetworkManager | wg |
| `watch [--interval S]` | NDJSON stream of Status on change (TUI/agents) | status |
| `doctor [--fix] [--dry-run] [--allow-keyring]` · `doctor undo RUN` · `doctor capabilities` · `doctor health` | §7 | doctor |
| `capabilities` · `robot-docs` | machine-readable contract; agent handbook | — |
| `tui [--screen status\|servers\|history\|doctor]` | FrankenTUI | tui |
| `setup [--plugin] [--menu] [--unit] [--all] [--disable-old-plugin] [--yes]` | install integration pieces idempotently | setup |
| `version` | | |

Exit codes: 0 ok · 1 generic · 2 usage · 3 protonvpn CLI not installed · 4 not signed in ·
5 external command timed out (CLI hang) · 6 engine/connect failed · 7 unknown target/server ·
8 wait timeout · 9 index missing/corrupt (hint: `fproton index --rebuild`) · 10 NetworkManager error ·
11 doctor found failures (doctor only) · 130 interrupted.

Human output: aligned tables, colours only on TTY. Every error ends with a `hint:` line
holding a runnable command.

## 5. Status JSON (schema 1) — the plugin/TUI/agent contract

```json
{
  "ok": true, "schema": 1, "generated_at": "2026-09-14T21:00:00+02:00",
  "state": "disconnected",            // disconnected|connecting|connected|disconnecting|error|unknown
  "engine": "cli",                    // configured engine: cli|wg
  "installed": {"cli": true, "daemon": true, "gtk_app": false, "wireguard": true},
  "signed_in": null,                  // true|false|null (null = never probed; status never probes)
  "account": {"user": "…", "plan": "…", "probed_at": "…"} ,   // or null
  "link": {"up": false, "device": "proton0", "nm_connection": "ProtonVPN CH#461",
           "since": null, "uptime_sec": 0, "endpoint": "146.70.226.194:443"},
  "server": {"name": "CH#461", "country": "CH", "country_name": "Switzerland", "city": "Zurich",
             "lat": 47.37, "lon": 8.54, "load": 31, "score": 1.2, "tier": 2,
             "features": ["p2p"], "features_raw": 4, "entry_ip": "…", "exit_ip": "…"},   // or null
  "traffic": {"rx_bytes": 0, "tx_bytes": 0, "rx_bps": 0, "tx_bps": 0, "sampled_at": "…"},
  "kill_switch": {"configured": "standard", "active": false},   // configured from cached config, may be null
  "port_forwarding": {"enabled": null, "port": null},
  "config": {"netshield": "malware-only", "cached_at": "…"},    // last `config list`, or {}
  "pending": {"action": "connect", "target": "CH", "started_at": "…", "engine": "cli"},  // or null
  "cli": {"reachable": null, "last_ok_at": null, "hang_suspected": false, "backoff_until": null},
  "last_error": null,
  "warnings": ["cli_hang_suspected"],  // stable ids; each also has human text in `warning_text`
  "warning_text": {"cli_hang_suspected": "protonvpn timed out 3 min ago; run fproton doctor"},
  "next": ["fproton connect", "fproton doctor"]    // agent hints, runnable
}
```

`state` derivation: link up → `connected` (or `disconnecting` if pending disconnect);
link down + pending connect younger than its deadline → `connecting`; link down → `disconnected`;
a failed pending action → `error` until the next successful action or `status --ack`… keep it
simple: `error` is shown while `last_error` is set and the link is down; any new action clears it.

## 6. Plugin `oliver.protonvpn` (Quickshell, Omarchy 4)

- `manifest.json`: schemaVersion 1, id `oliver.protonvpn`, kinds `["bar-widget"]`,
  entryPoints.barWidget `Panel.qml`, category Network, defaultSection right,
  defaults + schema: `refreshIntervalSec` 3 (open), `idleRefreshIntervalSec` 8,
  `binary` "fproton", `notifications` on/off, `showMap` on/off, `tuiCommand`
  "omarchy-launch-or-focus-tui --app-id=fproton fproton tui".
- `Service.qml` (≤ 450 lines): one `Process` polling `[bin, "status", "--json"]`
  (interval by open/idle); a serialized action queue (`connect`, `disconnect --force`
  fallback, `config set`, `account`); lazy `countries --json` / `cities --country CC --json`;
  `doctor --json` on demand; `notify-send` on state transitions when enabled; sign-in
  via `Quickshell.execDetached(["omarchy-launch-floating-terminal-with-presentation", "fproton signin"])`.
  Optimistic UI comes for free from `status.pending`.
- `Panel.qml` (≤ 800 lines): same components/idioms as the Tailscale panel: hero with
  the Proton mark + power switch + state phrase, detail rows (server, city, load,
  uptime, rx/tx), quick-connect row (Fastest · P2P · Secure Core · Tor · Random),
  Recents, Countries list with `/` filter → cities → connect, Protections section
  (Kill Switch / NetShield / Port forwarding switches + forwarded port with copy),
  optional WorldMap (ported), footer: "Open TUI", "Doctor", "Sign in/out".
  Arrow keys/Esc/Enter navigation like Tailscale. Shows `warnings` as a yellow row
  with the doctor hint. Never blocks: a hung action shows "Taking long… force disconnect".
- `ProtonIcon.qml`: bar icon; dim when disconnected, `bar.foreground` when connected,
  pulsing opacity while connecting, `bar.urgent` tint on `error`/warnings.
  Left click = panel, right click = toggle connect/disconnect, middle = refresh.
- `Model.js`: pure helpers (format bytes/rates/uptime, flag emoji from CC, feature labels).
- Must pass `omarchy-plugin-validate plugin/oliver.protonvpn`. Test load by symlinking
  into `~/.config/omarchy/plugins/oliver.protonvpn` and checking
  `journalctl --user -b --no-pager | grep -E "oliver.protonvpn"` shows no "failed".
  The old plugin id `io.github.grichard99.omaproton-vpn` is currently in
  `~/.config/omarchy/shell.json` right section; `setup --disable-old-plugin` swaps it
  (`omarchy plugin disable …` then `omarchy plugin enable oliver.protonvpn right`).

## 7. Doctor (world-class-doctor skill, scaled to this project)

`fproton doctor` runs checks, prints a table (or JSON: `{ok, run_id, checks:[{id, severity,
status: pass|warn|fail|skip, detail, fixable, fix_cmd}], summary}`), exit 11 on failures.
`--fix` applies fixable ones through ONE `mutate()` chokepoint that backs up any file it
changes to `~/.local/state/fproton/doctor/runs/<run-id>/backups/` and writes an inverse;
`doctor undo <run-id>` reverts. `--dry-run` prints planned mutations. Checks (minimum):
`cli_installed`, `cli_help_fast` (`protonvpn --help` < 3 s), `cli_session_probe`
(`protonvpn config list` with 8 s deadline → pass / `hang` / not-signed-in), `daemon_active`,
`gtk_app_conflict`, `networkmanager_running`, `wireguard_available`, `secret_service_on_bus`,
`keyring_default_alias` (the exact condition in §1.2: alias file vs D-Bus collections; on
mismatch report the fix `--allow-keyring` would apply: back up `~/.local/share/keyrings/default`
and point it at an existing unlocked collection — never touch `.keyring` files),
`keyring_default_locked`, `stale_proton_profiles` (inactive `ProtonVPN *` profiles → fix: delete),
`killswitch_orphan` (`pvpn-killswitch-ipv6` active while no tunnel → fix: down),
`serverlist_cache` (exists, size, expired?), `index_fresh`, `state_dir`, `plugin_linked`,
`plugin_enabled`, `old_plugin_enabled` (fix: disable), `menu_entries`, `binary_on_path`,
`dns_when_connected` (resolvectl shows 10.2.0.1 on proton0 when up), `ipv6_leak_guard`.
Fixtures + a regression test per fixer. `doctor capabilities --json` lists checks and
fix safety class. `doctor health` = exit-code-only quick check for scripts.

## 8. Agents, ownership and hand-offs

Phase 1 — **core** (one agent): everything in `crates/fproton-core`, `crates/fproton/src/{main,output,setup}.rs`,
`tests/e2e.sh`, `tests/fixtures/*`. Must leave `crates/fproton/src/tui/mod.rs` and
`crates/fproton/src/doctor/mod.rs` as compiling stubs with the fixed signatures
`pub fn run(args: TuiArgs, ctx: &fproton_core::Context) -> anyhow::Result<i32>` /
`pub fn run(args: DoctorArgs, ctx: &Context) -> anyhow::Result<i32>` where the `*Args`
clap structs live in those modules, so Phase 2 agents never touch `main.rs`.
Exit gate: `cargo build --release`, `cargo test`, `cargo clippy -- -D warnings`,
`tests/e2e.sh` green, `fproton status --json` real output pasted in DECISIONS.md.

Phase 2 (parallel, disjoint files):
- **tui**: `crates/fproton/src/tui/**` (+ Cargo deps only if unavoidable).
- **plugin**: `plugin/oliver.protonvpn/**`, `tests/plugin-smoke.sh`.
- **doctor**: `crates/fproton/src/doctor/**`, may add `crates/fproton-core/src/probe.rs` (new file only).
- **docs**: `README.md`, `CHANGELOG.md`, `docs/ROBOT.md` content (the core agent's
  `robot-docs` reads `include_str!("../../../docs/ROBOT.md")`), `scripts/*.sh`,
  `.github/workflows/ci.yml`, `plugin/oliver.protonvpn/README.md` (coordinate: plugin agent owns QML; docs agent owns that README).

Phase 3 — **integrate**: build the whole tree, fix cross-module breakage, ergonomics pass, run all tests, commit.
Phase 4 — reviewers (read-only, findings JSON) → verification → **fixer** → **final verifier**
(installs locally with `scripts/install.sh --yes`, checks the bar loads, runs e2e, reports honestly).

Commits: each agent commits only its own files, message `<area>: <what>` + trailer
`Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`. Rebase-free: everyone works
on `main` in the same checkout; run `git add <own paths>` only.

## 9. Skills to read (paths; read the SKILL.md, follow what applies, skip ceremony)

- CLI/agents: `~/.claude/skills/agent-ergonomics-and-intuitiveness-maximization-for-cli-tools/references/QUICKREF.md`,
  `~/.claude/skills/world-class-doctor-mode-for-cli-tools/assets/skill-card.md` (+ KERNEL.md if needed)
- TUI: `~/.claude/skills/frankentui/SKILL.md` + `references/TUI_POLISH_PLAYBOOK.md`, `~/.claude/skills/tui-glamorous/SKILL.md`
- Omarchy: `~/.claude/skills/omarchy/plugins.md`, `~/.claude/skills/omarchy/theming.md`
- Docs: `~/.claude/skills/readme-writing/SKILL.md`, `~/.claude/skills/de-slopify/SKILL.md`,
  `~/.claude/skills/installer-workmanship/SKILL.md`, `~/.claude/skills/changelog-md-workmanship/SKILL.md`, `~/.claude/skills/gh-actions/SKILL.md`
- Quality: `~/.claude/skills/multi-pass-bug-hunting/SKILL.md`, `~/.claude/skills/ubs/SKILL.md` (`ubs` binary is installed),
  `~/.claude/skills/mock-code-finder/SKILL.md`, `~/.claude/skills/just-say-no-to-process-porn-and-ceremony/SKILL.md`,
  `~/.claude/skills/reality-check-for-project/SKILL.md`, `~/.claude/skills/testing-real-service-e2e-no-mocks/SKILL.md`
- Franken: `~/.claude/skills/frankensqlite-mega-skill/SKILL.md` (downstream embedding lane only)
