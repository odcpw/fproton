# fproton — design brief v2 (source of truth for every agent working on this repo)

Proton VPN for Omarchy, rebuilt: a Rust CLI + engine (`fproton`), a FrankenTUI app, a
`doctor`, and a thin Omarchy bar widget (`oliver.protonvpn`). It replaces the third-party
`io.github.grichard99.omaproton-vpn` Quickshell plugin (4.7k lines of QML/Python), which
broke on this machine and is now disabled.

v2 incorporates a seven-lens review (planning, agent-ergonomics, doctor, FrankenTUI, Omarchy
plugin, reality/security, franken embedding) run 2026-09-14. Every "verified" fact below was
re-checked on this machine by a reviewer; where v1 was wrong it is corrected here.

Repo: `~/Projects/odcpw/fproton` (GitHub `odcpw/fproton`, pushed later by the owner).
Owner: Oliver (`oliver` on host `t14`, Omarchy 4.0.3, Arch, Hyprland, Quickshell).
Sibling project whose conventions we copy: `~/Projects/odcpw/fbittorrent` (Rust CLI +
ftui TUI + `plugin/oliver.torrents` QML widget). Read its README, `src/main.rs`,
`src/agent.rs`, `src/tui/{mod,theme}.rs` and `plugin/oliver.torrents/*` before writing code.

## 0. Scope of v0.1 (decided; do not widen)

IN: `cli` engine (official `protonvpn` CLI, hardened), `status` without the CLI, server
index snapshot, `connect/disconnect/reconnect`, `servers/countries/cities`, `config` via CLI,
`port` one-shot NAT-PMP, `history/recents`, `signin/signout/account`, `doctor` (detect + a
small set of safe fixes), `tui` (Status · Servers · History), the bar widget, `setup`,
`scripts/install.sh`, `tests/e2e.sh`, `tests/plugin-smoke.sh`, README.

OUT (v0.2 or later, do not build): WorldMap in the panel, cities drill-down in the panel,
desktop notifications wired on (setting exists, default off), `port --keepalive` loop,
`watch`/NDJSON stream, `wg import`, a doctor screen in the TUI, TUI mouse, editing Proton
settings from the TUI, CI workflow and CHANGELOG (added at first push), fsqlite.

EXPERIMENT (env-gated, built last, never load-bearing): `wg up|swap|list` — see §4 `wg`.

## 1. Facts about this machine (verified 2026-09-14)

1. **Old plugin.** Failed to load on 2026-09-12 with `Service.qml:820:12: Expected token ','`
   → `Panel.qml:449:3: Type Service unavailable` after an uncommitted hand patch. It is now
   `disabled` (`omarchy plugin list`) and absent from `~/.config/omarchy/shell.json`; it only
   remains on disk at `~/.config/omarchy/plugins/io.github.grichard99.omaproton-vpn/`.
2. **The CLI hangs.** `/usr/bin/protonvpn` (proton-vpn-cli 1.0.3, Python) answers `--help`
   in 0.4 s but every session-touching command (`status`, `info`, `config list`, `connect`)
   blocks forever. Cause: gnome-keyring-daemon crashed (SIGABRT 16:18) and was hand-repaired;
   `~/.local/share/keyrings/default` names `Default_keyring`, whose file
   `Default_keyring.keyring` is a 100-byte header-only stub with zero items, while the real
   Proton credentials (`Password for 'proton-sso-accounts' on 'Proton'`) live in
   `Default.keyring` (collection `Default`, unlocked). On D-Bus,
   `Secret.Service.ReadAlias("default")` returns `/org/freedesktop/secrets/collection/Default_5fkeyring`,
   which is listed in `Collections` but has **no exported object** (any property GET fails
   with "Object does not exist at path"). python-keyring's SecretService backend resolves the
   default collection on every call (even backend validation), so each session command blocks
   on a gcr-prompter prompt that never maps on Hyprland (journal: "couldn't find the callback",
   "10 second inactivity timeout"). The owner will repair the keyring after a reboot.
   **fproton must never depend on the CLI answering**: process-group deadlines everywhere,
   and `status` never spawns `protonvpn`.
3. **Proton's on-disk footprint** (readable without the CLI):
   - `~/.cache/Proton/VPN/serverlist.json`: 24 432 460 bytes, 18 221 `LogicalServers`, 149
     countries, 195 (country, city) pairs. Top-level: `LogicalServers`, `Code`,
     `ResponseMetadata`, `MaxTier` (the signed-in account's entitlement: 2 = Plus here, 0 =
     free), `LastModifiedTime`, `ExpirationTime`, `LoadsExpirationTime` (Unix floats; the
     loads expire within hours, the list within ~2 h of writing; only the official CLI
     refreshes this file — **fproton never contacts Proton's API in v0.1**).
     Logical keys: `Name` ("CH#461", "IS-DE#1", "US-CA#1022", "NL-FREE#15", "US-CO#21-TOR"),
     `EntryCountry`, `ExitCountry`, `Domain`, `Tier` (only 0 or 2 occur), `Features`
     bitmask (1 secure-core, 2 tor, 4 p2p, 8 streaming, 16 ipv6; 28 = p2p|streaming|ipv6 is
     the common value; keep the raw int), `City`, `Score` (lower is better), `Load` 0-100,
     `Location{Lat,Long}`, `Status`, `ID`, `HostCountry`, `Region`, `Servers[]` physical:
     `EntryIP`, `ExitIP`, `Domain`, `X25519PublicKey`, `Status`.
     Real values for CH#461 (also in `tests/fixtures/serverlist-small.json`): Load 63,
     Score 1.8623468195, Features 28, Tier 2, Zurich, 47.37/8.54, entry/exit 146.70.226.194.
   - `~/.cache/Proton/VPN/clientconfig.json`: `DefaultPorts.WireGuard.UDP` =
     [443, 88, 1224, 51820, 500, 4500]; feature flags. **No user settings are stored on disk
     anywhere** (`~/.config/Proton/VPN/` is empty): kill-switch/netshield/port-forwarding/ipv6
     values are obtainable only from `protonvpn config list`.
   - `~/.cache/Proton/VPN/connection/connection_persistence.json`: last connection
     (`server_name`, `domain`, `server_ip`, `protocol`, `backend`, `x25519pk`).
   - `~/.cache/Proton/VPN/logs/vpn-cli.log`: the CLI's local agent negotiates NetShield,
     port forwarding, moderate NAT etc. over a TLS control channel *after* the tunnel is up
     (`AgentFeatures { netshield_level, port_forwarding, … }`). A raw WireGuard tunnel gets
     none of those, and no kill switch.
   - NetworkManager (nmcli 1.58.1): the CLI creates WireGuard profile `ProtonVPN <Name>` on
     `proton0` (ipv4 manual 10.2.0.2/32 dns 10.2.0.1 dns-priority -1500; ipv6 manual
     2a07:b944::2:2/128 dns 2a07:b944::2:1; fwmark 0xea13b2c; `wireguard.peers <pubkey>
     allowed-ips=0.0.0.0/0;::/0 endpoint=<EntryIP>:443`; `wireguard.private-key-flags 0` =
     the private key is stored in the root-only keyfile under
     `/etc/NetworkManager/system-connections/`, unreadable to the user, and *is not
     regenerable without the CLI*) and a `dummy` profile `pvpn-killswitch-ipv6` on
     `ipv6leakintrf0`. Both exist, both inactive. `tailscale0` (tun) is ACTIVE and will
     contest the default route/DNS with a Proton tunnel.
   - Privilege boundary: `nmcli general permissions` → `settings.modify.own yes`,
     `settings.modify.system yes`, `network-control yes`. Every nmcli call fproton makes
     (show/add/clone/modify/up/down) works without sudo or a polkit prompt. fproton never
     invokes sudo/pkexec/systemd-run; a polkit denial is exit 4 with the permission name.
     `nmcli -t -f NAME,TYPE,DEVICE,ACTIVE connection show` takes 11-25 ms.
   - Daemon `proton.VPN.service` active (split tunneling). Kernel `wireguard` module loaded.
   - Tunnel throughput: `/sys/class/net/proton0/statistics/{rx,tx}_bytes` — exists only
     while the tunnel is up.
   - Port forwarding: NAT-PMP (RFC 6886) to 10.2.0.1:5351 inside the tunnel, 60 s lifetime;
     Proton's own help says an external script must maintain the lease. Old `port.py` is the
     62-line reference; reimplement in Rust with std UDP only.
   - Session certificate in the keyring is `Mode: session` with a ~7-day expiry
     (2026-09-21) refreshed only via Proton's API → any reuse of the existing WireGuard key
     outside the CLI inherits that expiry.
   - Sign-in is interactive: `protonvpn signin USERNAME` (USERNAME is a **required**
     positional) prompts for password + TOTP.
4. **Toolchain.** cargo/rustc 1.98 stable. ftui 0.7.0 + ftui-extras 0.7.0 fetched and known
   to compile; fbittorrent's ftui 0.6 code is API-compatible (Model/Cmd/Subscription/App
   builder and every widget it uses exist unchanged in 0.7; additions only). **fsqlite and
   asupersync require nightly (`#![feature]`, E0554) — banned.** `postcard` + `toml` are in
   the manifests.
5. **Omarchy plugin host (Quickshell).** Docs: `/usr/share/omarchy/shell/plugins/README.md`,
   `/usr/share/omarchy/shell/plugins/bar/README.md`, `~/.claude/skills/omarchy/plugins.md`.
   Reference panel: `/usr/share/omarchy/shell/plugins/panels/tailscale/{Panel,Service,TailscaleIcon}.qml`,
   `Model.js`, `manifest.json`. `qs.Ui` (Panel, KeyboardPanel, PanelKeyCatcher, PanelHero,
   PanelSectionHeader, PanelSeparator, PanelToolTip, PanelActionButton, BarIconButton,
   CursorSurface, ToggleSwitch, TextField, Button, …) and `qs.Commons` (Color, Style, Util,
   Border) are importable by third-party plugins. Facts that bite:
   - Manifest `barWidget.defaults`/`schema` are **never merged into `settings`** — enabling
     writes `{"id":"oliver.protonvpn"}` into shell.json and the widget receives only inline
     keys. Every setting needs an in-code fallback.
   - A **symlinked plugin dir is discovered and loaded but does not hot-reload** (the registry
     watches with `inotifywait -r`, which does not descend symlinks). After each QML edit run
     `omarchy-shell shell rescanPlugins`.
   - `omarchy-plugin-validate` refuses any symlink (including a top-level symlinked dir):
     always validate the repo directory `plugin/oliver.protonvpn`, never the installed link.
   - `bar` is a `PluginBarApi` facade: `barForeground` (use for bar-drawn content),
     `foreground`, `urgent`, `fontFamily`, `barSize`, `requestPopout/releasePopout`,
     `showTooltip/hideTooltip`, `run(cmd)` (= `bash -lc <string>` — **never use**).
   - Shell-string entry points: `omarchy-launch-floating-terminal-with-presentation` joins
     `$*` and runs it under `bash -c`; `omarchy-launch-or-focus` ends in `eval exec`. Only
     constant literals may be passed to them. `omarchy-launch-or-focus-tui [--app-id=ID] cmd
     args…` is argv-shaped. `omarchy-notification-send [-u low] [-g glyph] headline [body]`
     is the house notifier.
   - Placement: `omarchy plugin enable oliver.protonvpn` then `omarchy bar put
     oliver.protonvpn --before omarchy.tailscale` (what fbittorrent's README uses);
     `omarchy plugin disable <id>` is idempotent.
   - The shell's PATH includes `~/.local/bin` but **not** `~/.cargo/bin`: the binary must be
     installed to `~/.local/bin/fproton`.
   - Every panel plugin on this box logs a benign `WARN scene: QML IpcHandler … Handler was
     registered but will not be used` line at reload; log-based smoke tests must whitelist it.
   - Journal identifier is `omarchy-shell`; real failures look like
     `WARN qml: Plugin widget <id> failed: file:///…:L:C: <message>`.
6. **Reference material.** Old plugin at `~/.config/omarchy/plugins/io.github.grichard99.omaproton-vpn/`
   (MIT, grichard99): port `ProtonIcon.qml` (Simple Icons Proton mark, CC0) with attribution;
   `WorldMap.qml`/`World.js` are saved under `docs/reference/` for v0.2. Do not copy its
   Service.qml architecture.

## 2. Non-negotiables

- **Never hang.** Every external process runs in its own process group with a deadline
  (SIGTERM, then SIGKILL 2 s later), stdin/stdout/stderr all `Stdio::piped()` (never inherit).
  Defaults: probes 15 s, `connect` 90 s, `disconnect` 25 s, `config set` 20 s, nmcli 10 s,
  busctl 1 s; `signin` has no deadline (interactive, only from a terminal). `status` never
  spawns `protonvpn`. After a deadline kill on `protonvpn`, state records `cli.hang` with a
  60 s backoff during which CLI-backed commands exit 5 immediately with the doctor hint
  (`--force-cli` overrides).
- **argv only.** No shell strings, no `sh -c`, no string-built commands, ever, in Rust or
  QML. User input is validated before reaching argv. Server name regex (verified against all
  18 221 names): `^[A-Z]{2}(-(?:[A-Z]{2}|FREE))?(#[0-9]{1,5})?(-TOR)?$`, max 12 chars.
  Country: `^[A-Z]{2}$`. City: `^[\p{L} .'-]{1,40}$`. A test asserts the server regex
  matches every `Name` in `tests/fixtures/serverlist-small.json` and rejects `; rm -rf`,
  `--`, `CH#461 foo`.
- **No secrets — enforced by path, not by pattern.** fproton never reads, copies, backs up,
  hashes, logs, or includes in any output the body of `~/.local/share/keyrings/**`,
  `/etc/NetworkManager/system-connections/**`, or the output of `nmcli --show-secrets`
  (never invoked). The login keyring on this box is stored unencrypted; a regex is not a
  defence. fproton stores no passwords, tokens or WireGuard keys. A unit test greps the full
  `--json` and `-v` output of every command for `PrivateKey`, `RefreshToken`, `AccessToken`
  and 44-char base64 ending in `=` and fails on any hit.
- **Core is silent.** No function in `fproton-core` writes to stdout/stderr. All output is
  produced by the binary (`output.rs`). Phase 3 gate: `grep -rE 'println!|eprintln!|print!|dbg!' crates/fproton-core/src` is empty.
- **JSON contract.** `--json` on every command, one JSON object per invocation. **Non-TTY
  stdout implies `--json` for every command**; `--json` on a TTY forces it. Colour off when
  not a TTY, with `--no-color`, or when `NO_COLOR`/`CI` is set. stdout is data; progress and
  diagnostics go to stderr. Errors: `{"ok":false,"schema":1,"code":N,"code_name":"…",
  "error":"…","hint":"<runnable command>"}` with the same exit code. Every human-mode error
  ends with a `hint:` line. `capabilities` (JSON, same keys as `fbittorrent capabilities`)
  and `robot-docs [SECTION]` (text) are always machine output.
- **Irreversible ops need `--yes` and offer `--dry-run`**: `disconnect --force`, `wg swap`,
  `setup --disable-old-plugin`, `doctor --fix`. Without `--yes` on a TTY they print the exact
  argv/diff and exit 3 `safety`; `disconnect --force` is implied without prompt after a
  deadline kill (leaving `proton0` up is the unsafe state).
- **The doctor never deletes.** No fixer removes a file, a NetworkManager profile, or a
  keyring entry; anything delete-shaped is reported with a `fix_cmd` for the human.
- **Fast.** `fproton status --json` < 50 ms warm; `servers --country CH` < 20 ms. Measured
  with `hyperfine` at the Phase 1 gate and pasted into DECISIONS.md (a miss is recorded with
  the real number, not hidden).
- **Thin plugin.** QML renders and calls `fproton … --json`; no Python, no nmcli, no
  business logic. Budgets in §6.
- **The TUI never blocks its event loop** (§7a): short reads in `Cmd::task`, long actions as
  a cancellable `Subscription`.
- **Honest.** README and `--help` claim only what the §4b matrix marks GREEN or AMBER, and
  say so. No mocks presented as tests; fixtures are captured real output.
- **Ownership.** Touch only the files §8 assigns you. Never `cargo clean`. Never edit any
  `Cargo.toml` after Phase 1 (ask the orchestrator). Append to `DECISIONS.md` (never
  rewrite) for every non-obvious choice.

## 3. Repo layout (Cargo workspace, resolver 2)

```
Cargo.toml                    workspace; deps pinned here
crates/fproton-core/          library (silent, no I/O to the terminal)
  src/lib.rs                  pub mod …; pub use Context, Status, Error, CancelToken …
  src/context.rs              Context (§8): plain data, Clone + Send + Sync + 'static
  src/paths.rs                XDG paths; FPROTON_STATE_DIR / FPROTON_CONFIG_DIR / FPROTON_CACHE_DIR overrides for tests
  src/proc.rs                 run_with_deadline(argv, timeout, cancel) → Output{code, stdout, stderr, timed_out}; process groups; piped stdio
  src/nm.rs                   nmcli parsing (fixtures), link state, sysfs traffic
  src/cli.rs                  protonvpn driver: argv mapping, deadline, text parsers for status/info/config list/countries (fixtures)
  src/engine.rs               Engine trait, Target parsing/validation, cli engine, tier gating
  src/wg.rs                   EXPERIMENT (env-gated): up / swap / list via nmcli clone+modify; no key handling
  src/index.rs                ServerIndex: serverlist.json → postcard snapshot at $XDG_CACHE_HOME/fproton/index.bin (rebuilt when the JSON mtime changes); queries
  src/natpmp.rs               NAT-PMP codec + one-shot query with RFC backoff (fixtures)
  src/state.rs                XDG state: pending action, last probe results, cli hang/backoff, history, recents, cached config, signed_in cache
  src/settings.rs             ~/.config/fproton/config.toml (engine, timeouts, default_target, notify) + the config-key whitelist table
  src/status.rs               derive Status (§5) from nm + state + index + secret-service probe
  src/secrets.rs              read-only Secret Service probes via busctl (ReadAlias, Collections, SearchItems) — never reads keyring files
  src/agent.rs                pub const ROBOT_GUIDE: &str (agent handbook) + capabilities_json() — same shape as fbittorrent/src/agent.rs
  src/error.rs                Error → exit code + code_name + hint
crates/fproton/               binary
  src/main.rs                 clap; dispatch; builds Context; exit codes
  src/output.rs               human tables vs JSON; colour policy
  src/setup.rs                setup: binary check, plugin symlink, enable+place, menu entries, old-plugin disable
  src/doctor/mod.rs           DoctorArgs (+ DoctorCmd enum) and run() — Phase 2 doctor agent
  src/doctor/{checks,probes,mutate,report}.rs
  src/tui/{mod,theme,screens,subs}.rs   TuiArgs and run() — Phase 2 TUI agent
plugin/oliver.protonvpn/      manifest.json, Panel.qml, Service.qml, ProtonIcon.qml, Model.js, README.md
scripts/install.sh · scripts/uninstall.sh · scripts/check.sh (the local gate: fmt, clippy, test, e2e, plugin-smoke)
tests/e2e.sh                  real machine, no mocks, < 15 s, fills the §4b matrix
tests/plugin-smoke.sh         §6.9
tests/fixtures/               nmcli-*.txt, serverlist-small.json, clientconfig.json, protonvpn-*.txt (captured --help and, when available, real outputs), doctor/<check>/{input/,expect.json}
docs/ROBOT.md                 generated by `fproton robot-docs > docs/ROBOT.md` in tests/e2e.sh; never hand-edited
docs/reference/               WorldMap.qml, World.js, LICENSE.omaproton-vpn (v0.2 material)
DESIGN.md (this) · DECISIONS.md · README.md · LICENSE (MIT)
```

## 4. CLI contract

Global flags: `--json`, `--timeout <s>` (override external deadlines), `-v`, `--no-color`,
`--yes`, `--dry-run` (where listed). Bare `fproton` on a TTY → `tui`; otherwise `status --json`.

| Command | Does | Backend |
|---|---|---|
| `status` | full Status JSON (§5). Never calls protonvpn. | nm + state + index + secrets |
| `connect [TARGET] [--engine cli\|wg] [--wait]` | TARGET is exactly ONE of: `fastest` (default) · `CC` · `CC#N` (server, upstream positional `SERVER_NAME`) · `city:"New York"` · `p2p` · `secure-core` · `tor` · `random`. No `--country/--city/--feature` flags. cli mapping: `CC`→`--country CC`, `city:X`→`--city X`, `CC#N`→positional, `p2p`→`--p2p`, `secure-core`→`--securecore` (one word), `tor`→`--tor`, `random`→`--random`, `fastest`→no arg (verify each against `protonvpn connect --help`). Target selection filters `Tier <= status.max_tier`; above it → exit 6 with hint `fproton servers --tier free`. Records `pending` in state first (optimistic UI), runs the engine under its deadline, records history. Unknown target → 6, several matches → 7. | engine |
| `disconnect [--force] [--dry-run]` | engine disconnect; `--force` tears down `proton0` (`nmcli device disconnect proton0` / `connection down`) and `pvpn-killswitch-ipv6` via nmcli, prints its argv, needs `--yes` on a TTY unless implied by a deadline kill. Idempotent no-op when nothing is up (asserted by e2e). | engine / nm |
| `reconnect [--next]` | reconnect the last target; `--next` picks the next server in the same country by score | engine |
| `servers [--country CC] [--city C] [--feature p2p\|secure-core\|tor\|streaming] [--tier free\|plus] [--search TEXT] [--sort load\|score\|name] [--limit N]` | from the index; rows carry `Load`/`Score` plus `index.built_from_mtime` so consumers can grey stale loads. Note in `--help`/robot-docs: upstream `protonvpn servers` only prints a URL; this is strictly better. | index |
| `countries` · `cities [--country CC]` | counts, min load, lat/lon, has_free | index |
| `index [--rebuild] [--stats]` | (re)build the snapshot; auto-rebuild when serverlist.json mtime changes. An expired serverlist is **served, not refused**: `status.warnings` gains `serverlist_stale`; nothing exits non-zero for staleness. | index |
| `config list` · `config get K` · `config set K V` | Proton settings via the CLI under a deadline. Upstream has **no `get`** and **no `--json`**: `list` runs `protonvpn config list`, parses the table (fixture), caches it in state; `get` answers from the cache (exit 5 if empty and the CLI is hung). `set` runs `protonvpn config set <K> <V>` (K is an upstream subcommand). Whitelist, verified with `protonvpn config set <K> --help` on 1.0.3: `netshield {off,malware-only,malware-ads-trackers}` · `kill-switch {off,standard}` · `port-forwarding {off,on}` · `ipv6 {off,on}` · `vpn-accelerator {off,on}` · `moderate-nat {off,on}`. Parse all eight keys in `list` (also `custom-dns`, `anonymous-crash-reports`) but do not expose `set` for those two in v0.1. **There is no `protocol` setting and no `kill-switch permanent`**; reject at parse time with exit 2. The whitelist lives in one table in `settings.rs`; a test asserts every pair appears in the captured `--help` fixtures. | cli |
| `settings [get K\|set K V]` | fproton's own config.toml: `engine` (`cli`), `default_target`, `timeouts.*`, `notify` | local |
| `port` | one-shot NAT-PMP query → `{port, expires_at}`; exit 6-class error when no tunnel | natpmp |
| `history [--limit N]` · `recents` | from state | state |
| `account` | `protonvpn info` under a deadline; parses user/plan (fixture); caches for `status.account` | cli |
| `signin [USER]` · `signout` | `signin`: if USER omitted, prompt for it on stdin (the upstream positional is required), validate `^[A-Za-z0-9._+@-]{1,64}$`, then exec `protonvpn signin <user>` in the current terminal with no deadline. `signout` under a deadline. | cli |
| `wg up [NAME]` · `wg swap NAME TARGET --yes` · `wg list` | **EXPERIMENT, env-gated (`FPROTON_EXPERIMENTAL_WG=1`), built last, ≤ 200 lines.** `up` activates an existing `ProtonVPN *` NM profile (the CLI's own, which holds the key) — a CLI-free reconnect; `swap` clones it (`nmcli connection clone`) and retargets the clone's `wireguard.peers` at another server from the index (pubkey + `EntryIP:443`) — the key is copied by NetworkManager, never read; `list` shows profiles. Every use prints `experimental: unverified on this machine`. Kill criterion (owner runs after the keyring fix): `wg up` control connect works; `wg swap` to another server makes `curl -s ifconfig.co` show that server's exit IP within 15 s; if not, or if it stops working after 2026-09-21, delete `wg.rs`. Nothing in §5-§7 or any gate depends on it. | nm |
| `doctor [--fix] [--dry-run] [--allow-keyring] [--only GROUP] [--yes]` · `doctor undo RUN\|latest` · `doctor capabilities` | §7 | doctor |
| `capabilities` · `robot-docs [SECTION]` | machine contract / handbook (`ROBOT_GUIDE` in `agent.rs`) | — |
| `tui` | FrankenTUI (§7a) | tui |
| `setup [--plugin] [--menu] [--disable-old-plugin] [--all] [--yes] [--dry-run]` | idempotent integration: check `~/.local/bin/fproton` on PATH, symlink `plugin/oliver.protonvpn` → `~/.config/omarchy/plugins/oliver.protonvpn`, `omarchy plugin enable` + `omarchy bar put … --before omarchy.tailscale` (skip if present), merge `protonvpn.*` entries into `~/.config/omarchy/extensions/omarchy-menu.jsonc` (backup to `~/.local/state/fproton/backups/`, JSONC-safe merge, never clobber other keys), `--disable-old-plugin` runs `omarchy plugin disable io.github.grichard99.omaproton-vpn` only if `omarchy plugin list` says enabled | setup |
| `version` | | |

### 4a. Exit codes (0-8 identical to fbittorrent; 9+ fproton-specific; printed by `capabilities` and `robot-docs`)

| Code | `code_name` | Meaning | Typical hint |
|---|---|---|---|
| 0 | `ok` | success | — |
| 1 | `failure` | unexpected (bug, I/O) | `fproton doctor` |
| 2 | `usage` | bad arguments, unknown config key/value | `fproton <cmd> --help` |
| 3 | `safety` | refused without `--yes` | `… --yes` |
| 4 | `backend` | protonvpn not installed, NetworkManager down, polkit denial | `omarchy pkg add proton-vpn-cli` |
| 5 | `upstream` | protonvpn/nmcli errored **or was killed at its deadline** (the CLI hang lands here; sets `cli.hang_suspected`) | `fproton doctor` |
| 6 | `not_found` | unknown target/server/country/city; no tunnel for `port` | `fproton countries` |
| 7 | `ambiguous` | target matched several servers | `fproton servers --search <t>` |
| 8 | `timeout` | `--wait` ran out; nothing broken | `fproton status --json` |
| 9 | `not_signed_in` | no Proton session | `fproton signin` |
| 10 | `index` | index missing/corrupt | `fproton index --rebuild` |
| 11 | `doctor` | doctor found failures | `fproton doctor --fix` |
| 12 | `doctor_partial` | a fixer failed / partial fix | `fproton doctor` |
| 13 | `doctor_refused` | unsafe precondition or undo integrity failure | see detail |
| 130 | `interrupted` | SIGINT | — |

### 4b. Verification matrix (Phase 1 pastes the filled table into DECISIONS.md; README "Status" mirrors it)

Columns: `command | class | how verified today | evidence (exact command + first line of real output) | claimed in README?`

- **GREEN — end-to-end verifiable on this machine now:** `status`, `servers`, `countries`,
  `cities`, `index`, `settings`, `history`, `recents`, `capabilities`, `robot-docs`,
  `version`, `setup` (with `--dry-run` and for real), `tui`, `doctor` (every check except
  those marked skip), `disconnect --force` (must stay a no-op while both profiles are inactive
  and must not delete anything), `port` (error path: no tunnel → 6).
- **AMBER — only the timeout/error path is verifiable:** `connect --engine cli`,
  `config list|get|set`, `account`, `signout`. Tests assert exit 5, `code_name` `upstream`,
  a `hint:` line, and wall-clock under deadline + 2 s; they must NOT assert success.
  `signin` is exercised only for its argument validation.
- **RED — cannot be verified at all today; must not be claimed working anywhere:**
  `connect` success, `wg *`, `port` success, doctor `dns_when_connected`,
  `tunnel_route_ok`. A README bullet or `--help` line describing a RED item as working is a
  Phase 3 blocker. `tests/e2e.sh` exercises every GREEN row and every AMBER timeout path and
  fails if any RED command is reachable without its env gate.

## 5. Status JSON (schema 1) — the contract for the plugin, the TUI and agents

Shape rules: every key is always present; unknown is `null` (never an absent key, never `{}`,
never a zero standing in for unknown); timestamps RFC 3339 with offset; durations are
integers named `*_sec`; every literal in the example below matches
`tests/fixtures/serverlist-small.json` (a test asserts it).

```json
{
  "ok": true, "schema": 1, "generated_at": "2026-09-14T22:30:00+02:00",
  "state": "disconnected",
  "engine": "cli",
  "installed": {"cli": true, "daemon": true, "gtk_app": false, "wireguard": true, "networkmanager": true},
  "signed_in": true,
  "max_tier": 2,
  "account": null,
  "link": {"up": false, "device": "proton0", "nm_connection": "ProtonVPN CH#461", "since": null, "uptime_sec": 0, "endpoint": "146.70.226.194:443"},
  "server": {"name": "CH#461", "country": "CH", "country_name": "Switzerland", "city": "Zurich", "lat": 47.37, "lon": 8.54, "load": 63, "score": 1.8623468195, "tier": 2, "features": ["p2p", "streaming", "ipv6"], "features_raw": 28, "entry_ip": "146.70.226.194", "exit_ip": "146.70.226.194"},
  "traffic": null,
  "kill_switch": {"active": false, "supported": true},
  "port_forwarding": {"port": null, "expires_at": null},
  "config": null,
  "index": {"servers": 18221, "built_from_mtime": "2026-09-14T20:26:43+02:00", "serverlist_expires_at": "2026-09-14T23:45:35+02:00", "loads_expired": true},
  "pending": null,
  "cli": {"last_ok_at": null, "hang_suspected": false, "backoff_until": null},
  "last_error": null,
  "warnings": [{"id": "serverlist_stale", "severity": "info", "text": "server loads last refreshed 2 h ago", "hint": "protonvpn connect"}],
  "commands": ["fproton connect", "fproton doctor"]
}
```

- `state` is a total function of three inputs. A `pending` action is *live* while
  `now < started_at + deadline`; an expired `connect` is dropped into `last_error`; any new
  action clears `last_error`. There is no `unknown`.

  | link up | live pending | last_error | state |
  |---|---|---|---|
  | yes | `disconnect` | any | `disconnecting` |
  | yes | none or `connect` | any | `connected` |
  | no | `connect` | any | `connecting` |
  | no | none | set | `error` |
  | no | none | null | `disconnected` |

- `server`: from the active (or last, when `link.up` is false — then `link.nm_connection`
  names it) `ProtonVPN <Name>` profile joined to the index; `null` if neither exists.
- `signed_in`: from `secrets.rs` — `busctl --user --timeout=1 call org.freedesktop.secrets
  /org/freedesktop/secrets org.freedesktop.Secret.Service SearchItems a{ss} 1 service Proton`
  (read-only, ~6 ms, no unlock, never spawns protonvpn); `true` if any item path is returned,
  `false` if none, `null` if busctl fails/times out. Cached in state for 60 s.
- `max_tier`: `serverlist.json` top-level `MaxTier` (0 free, 2 plus); `null` if no cache.
- `config`: single carrier of cached Proton settings from the last successful `config list`
  (upstream key spellings verbatim, plus `cached_at`), or `null` if never probed. On this
  machine it stays `null` until the keyring is fixed; then `warnings` carries
  `protections_unknown`. `kill_switch.active` is always derivable (nmcli: is
  `pvpn-killswitch-ipv6` active). `supported` is `false` whenever `engine != "cli"`.
- `traffic`: `null` whenever `/sys/class/net/<device>` is absent; when present
  `{rx_bytes, tx_bytes, rx_bps, tx_bps, sampled_at}` with `*_bps` `null` until a second
  sample exists (previous sample kept in state).
- `warnings`: array of `{id, severity: info|warn|error, text, hint}`; ids stable and listed in
  `capabilities`: `cli_hang_suspected`, `serverlist_stale`, `serverlist_missing`,
  `protections_unknown`, `conflicting_tunnel` (e.g. tailscale0 active), `killswitch_orphan`,
  `stale_pending`, `not_signed_in`, `secret_service_broken`.
- `commands`: runnable next steps (fbittorrent's envelope name).

## 6. Plugin `oliver.protonvpn` (Quickshell bar widget, Omarchy 4)

### 6.1 Files and budgets

| File | Budget | Contents |
|---|---|---|
| `manifest.json` | ~60 lines | schemaVersion 1, id `oliver.protonvpn`, name "Proton VPN", version, author, license, description, `kinds:["bar-widget"]`, `entryPoints.barWidget:"Panel.qml"`, `barWidget{displayName, description, category:"Network", allowMultiple:false, defaultSection:"right", defaults{}, schema[]}` (defaults/schema feed only the Setup > Plugins form) |
| `Service.qml` | ≤ 500 | state model (6.4), poll, action queue, notifications (default off) |
| `Panel.qml` | ≤ 850 | bar button + KeyboardPanel content (6.6) |
| `ProtonIcon.qml` | ≤ 90 | ported from the old plugin + `crossed`/`warning` (6.7) |
| `Model.js` | ≤ 180 | pure formatters: bytes, rate, uptime, flag emoji from CC, feature labels, `parseStatus(text)` |
| `README.md` | — | docs agent |

Total QML in v0.1 < 1 500 lines (Panel + Service + ProtonIcon). Calibration: the Tailscale
panel is 1 273 + 637 lines for a smaller feature set; do not treat 850 as a hard gate if the
alternative is a cramped panel — record the real count in DECISIONS.md.

### 6.2 Imports and host contract

`import qs.Commons` and `import qs.Ui`. `Panel.qml` is `qs.Ui.Panel` with
`moduleName: "oliver.protonvpn"`, `ipcTarget: "oliver.protonvpn"`, `manageIpc: false` and its
own `IpcHandler` (`toggle`, `connect`, `disconnect`, `refresh`) as a convenience only. The host
injects `bar`, `moduleName`, `settings`. Use `bar.barForeground` for bar-drawn content,
`bar.foreground/urgent/fontFamily/barSize` elsewhere. Never `bar.run()`; always
`Quickshell.execDetached(argv)`.

### 6.3 Settings (trap)

Manifest `defaults` are never injected. Read every setting through
`setting(name, fallback)` / `intSetting(name, fallback, min, max)` (copy from
`oliver.torrents/Service.qml`) with fallbacks duplicating the manifest verbatim: `binary`
`"fproton"`, `refreshIntervalSec` 3 (1-60, panel open), `idleRefreshIntervalSec` 8 (2-600),
`notifications` `"off"`, `defaultTarget` `"fastest"`, `tuiCommand`
`"omarchy-launch-or-focus-tui --app-id=fproton fproton tui"`. `tests/plugin-smoke.sh` asserts
every manifest default key has an in-code fallback.

### 6.4 `Service.qml` — one source of truth: the last Status JSON

```qml
property var status: ({ ok: false, state: "disconnected" })
readonly property bool alive:     status.ok === true
readonly property string state:   String(status.state || "disconnected")
readonly property var link:       status.link || null
readonly property var server:     status.server || null
readonly property var traffic:    status.traffic || null
readonly property var config:     status.config || null          // null ⇒ protections unknown
readonly property var pending:    status.pending || null
readonly property var warnings:   Array.isArray(status.warnings) ? status.warnings : []
readonly property string lastError: String(status.last_error || "")
property string actionStatus: ""   // transient label, cleared 2.2 s after an action exits
property string localError: ""     // stderr of the last failed action
property int _desired: -1          // optimism bridge: -1 follow, 0 want down, 1 want up
readonly property bool on: _desired === -1 ? (state === "connected" || state === "connecting") : _desired === 1
readonly property bool busy: actionProc.running || pending !== null
```

`_desired` is set on click and cleared when a Status arrives with non-null `pending`, or
whose `state` agrees, or after 5 s, whichever is first (fproton writes `pending` itself, so
this bridges at most one poll interval).

Polling: one `Process { command: [bin, "status", "--json"] }` with `StdioCollector`s; a
`Timer` (`opened ? refreshMs : idleRefreshMs`, `triggeredOnStart`) that **skips the tick while
the process is running**; a `pollWatchdog` `Timer { interval: 8000; repeat: false }` armed on
the launch that needs watching and never restarted while armed (Tailscale `Service.qml:176`
documents why). Non-zero exit / unparsable stdout ⇒ `status = {ok:false, state:"disconnected"}`,
`localError` from stderr, last good `server` row kept for display.

Binary discovery on first load: `Process { command: ["which", bin] }`; failure renders
"fproton is not installed or not on PATH (~/.local/bin)" and pauses polling until the next open.

Actions: serialized FIFO (`_queue`, `pump()`), one `actionProc`, each followed by a 600 ms
`delayedRefresh`. Argv table (values validated first):

| UI action | argv |
|---|---|
| Connect default | `[bin, "connect", defaultTarget, "--json"]` |
| Connect country | `[bin, "connect", CC, "--json"]`, CC ~ `^[A-Z]{2}$` |
| Connect server (recents) | `[bin, "connect", NAME, "--json"]`, NAME ~ server regex (§2) |
| Quick-connect | `[bin, "connect", F, "--json"]`, F ∈ {`p2p`, `secure-core`, `tor`, `random`} |
| Disconnect | `[bin, "disconnect", "--json"]` |
| Force disconnect | `[bin, "disconnect", "--force", "--yes", "--json"]` |
| Reconnect / Next | `[bin, "reconnect", "--json"]` / `[bin, "reconnect", "--next", "--json"]` |
| Kill switch | `[bin, "config", "set", "kill-switch", V, "--json"]`, V ∈ {off, standard} |
| NetShield | `[bin, "config", "set", "netshield", V, "--json"]`, V ∈ {off, malware-only, malware-ads-trackers} |
| Port forwarding | `[bin, "config", "set", "port-forwarding", V, "--json"]`, V ∈ {off, on} |
| Countries (lazy, first open) | `[bin, "countries", "--json"]` |
| Refresh config (explicit click only) | `[bin, "config", "list", "--json"]` |
| Copy forwarded port | `Quickshell.execDetached(["wl-copy", "--", String(port)])` |
| Open TUI | `Quickshell.execDetached(["omarchy-launch-or-focus-tui", "--app-id=fproton", "fproton", "tui"])` |
| Doctor | `Quickshell.execDetached(["omarchy-launch-or-focus-tui", "--app-id=fproton-doctor", "fproton", "doctor"])` |
| Sign in | `Quickshell.execDetached(["omarchy-launch-floating-terminal-with-presentation", "fproton signin"])` — **constant literal only** (that script joins `$*` under `bash -c`); the panel has no username field |
| Sign out | `[bin, "signout", "--json"]` |

Notifications (`notifications === "on"` only), fired on a `state` transition between
`connected`/`disconnected`/`error`, never on a poll:
`Quickshell.execDetached(["omarchy-notification-send", "-u", "low", "-g", "󰦝", "Proton VPN", text])`.

### 6.5 Bar icon states (`ProtonIcon` inside a `BarIconButton`)

| Condition (first match wins) | Colour | Modifier |
|---|---|---|
| `!alive` | `Qt.darker(bar.barForeground, 1.55)` | crossed + warning badge |
| `state === "error"` or `localError !== ""` | `bar.urgent` | warning badge |
| any warning with severity `warn`/`error` | `bar.barForeground` | warning badge |
| `connecting` / `disconnecting` | `bar.barForeground` | opacity 0.45↔1.0, 900 ms, InOutQuad |
| `on` | `bar.barForeground` | — |
| otherwise | `Qt.darker(bar.barForeground, 1.55)` | crossed |

Mouse: left ⇒ toggle panel; right ⇒ `on ? disconnect() : connect(defaultTarget)`; middle ⇒ refresh.

### 6.6 `Panel.qml` — layout and keyboard

`KeyboardPanel { anchorItem: button; owner: root; bar: root.bar; open: root.opened; focusTarget: keyCatcher; contentWidth: panel.fittedContentWidth(Style.space(380)); contentHeight: panel.fittedContentHeight(column.implicitHeight, Style.space(560)) }`
wrapping a `PanelKeyCatcher` wrapping a `Flickable`.

Sections in cursor order: `header` (PanelHero: Proton mark, server name or "Proton VPN",
state phrase, `ToggleSwitch` bound to `on`/`busy`) → detail rows (country/city, load, uptime,
rx/tx or "traffic unavailable"; not cursor targets) → `warning` (visible when any warning:
`bar.urgent` text + hint, Enter runs the hint's action when it is `fproton doctor`, else
copies it) → `quick` (Fastest · P2P · Secure Core · Tor · Random) → `recents` →
`countries` (with `/` filter; Enter = connect to that country's fastest) → `protections`
(rendered only when `config !== null && kill_switch.supported`: Kill Switch, NetShield,
Port forwarding rows as toggles/cyclers + forwarded port with copy; otherwise one row
"Protections unknown — Proton CLI not responding" + "Refresh"/"Run doctor") → `footer`
(Open TUI · Doctor · Sign in / Sign out; sign-in shown when `signed_in !== true`).
A hung action (pending older than its deadline) shows a `bar.urgent` row "Taking long… force
disconnect"; activating it runs the force-disconnect argv.

Keyboard — `PanelKeyCatcher` owns these; do not re-implement:

| Key | Effect |
|---|---|
| `Down`/`j`, `Up`/`k` | move cursor within a section, then to the next/previous visible section |
| `Right`/`l`, `Left`/`h` | within `quick`, `protections` (cycle value), `footer` |
| `Enter`/`Space` | activate |
| `Esc` | filter open ⇒ close filter; else close panel |
| `Tab`/`Shift-Tab` | `switchPanel(±1)` |
| textKey `/` | open the country filter, `forceActiveFocus()` the TextField |
| textKey `c` `d` `f` `r` `t` | connect default · disconnect · force disconnect · refresh · open TUI |

`h j k l x` are consumed by the catcher and are not shortcuts. While the filter has focus:
`PanelKeyCatcher { blocked: root.countryFilterOpen }`.

### 6.7 `ProtonIcon.qml`
Port the old plugin's `ProtonIcon.qml` (52 lines, MIT; path = Simple Icons Proton VPN mark,
CC0) verbatim with attribution in the header, then add `crossed` and `warning` properties
modelled on `TailscaleIcon.qml`. QtQuick.Shapes + PathSvg; no image asset.

### 6.8 Install and dev loop
Dev: `ln -sfn ~/Projects/odcpw/fproton/plugin/oliver.protonvpn ~/.config/omarchy/plugins/oliver.protonvpn`,
`omarchy-shell shell rescanPlugins`, `omarchy plugin enable oliver.protonvpn`,
`omarchy bar put oliver.protonvpn --before omarchy.tailscale`. **After every QML edit run
`omarchy-shell shell rescanPlugins`** (symlinks do not hot-reload). Validate the repo dir,
never the link. Shipping install for others: `omarchy plugin add <git url> --enable --yes`.

### 6.9 `tests/plugin-smoke.sh` (the gate; exits non-zero; < 10 s)
Validates the repo dir with `omarchy-plugin-validate`; `jq -e` checks id/kinds/entryPoint/
defaultSection; required files exist; `manageIpc: false` present; no `execDetached(["bash"`
/ `bar.run(` anywhere; every manifest default has an in-code fallback; then
`omarchy-shell shell rescanPlugins`, sleep 2, and `journalctl --user -t omarchy-shell --since
<start>` filtered to `oliver.protonvpn` minus the benign `Local plugin changed` /
`Handler was registered but will not be used` lines must contain none of
`failed:|Expected token|unavailable|is not a type|ReferenceError|TypeError`.

## 7. Doctor

`fproton doctor` runs detectors, prints a table (or JSON `{ok, run_id, checks:[{id, group,
severity, status: pass|warn|fail|skip, detail, fixable, class, fix_cmd}], summary}`), exit 11
on failures, 0 otherwise. `--only GROUP` limits to a group (`cli`, `keyring`, `network`,
`cache`, `install`, `connected`). `doctor capabilities --json` lists checks, groups, classes.
Health for scripts is `fproton doctor --json >/dev/null; echo $?` (no separate subcommand).

**Detectors are pure functions of injected input**: `fn detect(&Probes) -> Verdict`, where
`Probes` holds already-captured bytes (nmcli output, busctl output, file contents,
`systemctl is-active` strings, sysfs reads, `protonvpn --help` timing). Only `doctor/probes.rs`
(using `fproton_core::proc`/`secrets`) spawns processes or talks to D-Bus, and every probe is
written verbatim to `runs/<run-id>/probes/<name>.txt`. Every detector has a captured-input
fixture in `tests/fixtures/doctor/<check-id>/{input/,expect.json}` asserting the verdict —
this is what makes checks that are green on this machine testable.

**`--fix` goes through ONE `mutate(Action)` chokepoint.** `Action` is an enum:
`WriteFile{path, bytes}` (verbatim backup to `runs/<id>/backups/<rel>` + sha256 before/after),
`CreateSymlink{link, target}` (inverse: remove link), `RunArgv{argv, inverse_argv}`,
`DbusSetAlias{name, from, to}` (inverse: set back to `from`). Every variant appends one line to
`runs/<id>/actions.jsonl` (append-only, fsync) with `{seq, class, action, before, after,
inverse, started_at, ok}`. `doctor undo <run-id|latest>` replays that file in reverse and
refuses (exit 13) if a `WriteFile` backup is missing or its hash mismatches. `--dry-run` prints
planned mutations and changes nothing (verified by `git status` + a state-dir checksum).
Run artifacts: `~/.local/state/fproton/doctor/runs/<UTC ISO8601 basic>__<run-id>/` with
`report.json`, `actions.jsonl`, `probes/`, `backups/`; `…/doctor/latest` symlink updated by
symlink-to-temp + rename. `doctor` without `--fix` writes only there.

Safety classes:

| Class | Runs under | Reverts by | Checks |
|---|---|---|---|
| A — fproton-owned state | `--fix` | file backup | `state_dir`, `stale_pending_action`, `index_fresh`, `settings_valid` |
| B — fproton-created integration | `--fix` | recorded inverse argv/symlink | `plugin_linked`, `plugin_enabled`, `menu_entries`, `old_plugin_enabled`, `killswitch_orphan` (gated) |
| C — foreign state, reversible | `--fix --allow-keyring --yes` | recorded inverse D-Bus call + file; best-effort, says so | `secret_service_default_alias` |
| D — never auto-fixed | never | — | `stale_proton_profiles`, anything touching `*.keyring`, `Unlock`, restarting gnome-keyring-daemon, deleting any NM profile |

Class A fixers get a round-trip test (`corrupt` → `doctor --fix` → assert → `doctor undo
latest` → compare) with `FPROTON_STATE_DIR`/`FPROTON_CONFIG_DIR` in a temp dir. B/C fixers
record their inverse and are exercised by hand once with the transcript in DECISIONS.md.

Checks (group · class · detection command · verdict rule):

- `cli_installed` (cli, D): `which protonvpn` + `pacman -Q proton-vpn-cli`.
- `cli_help_fast` (cli, D): `protonvpn --help` under 3 s (it is 0.4 s here).
- `cli_session_probe` (cli, D): `protonvpn config list` with an 8 s deadline → `pass` /
  `fail:hang` (deadline kill; detail names the keyring cause) / `fail:not_signed_in`.
- `daemon_active` (cli, D): `systemctl is-active proton.VPN.service`.
- `networkmanager_running` (network, D), `wireguard_available` (network, D: `lsmod`/`ls /sys/module/wireguard`).
- `secret_service_on_bus` (keyring, D): `busctl --user list | grep org.freedesktop.secrets`.
- `secret_service_default_alias` (keyring, C): `ReadAlias("default")` then GET `Label` on the
  returned path; `fail:phantom_collection` if "Object does not exist", `fail:locked` if
  `Locked=true`, else pass. Detail names the alias target, the live collections and which
  collection holds a `service=Proton` item (`SearchItems`). Fix (C): capture
  `old = ReadAlias("default")`; target = the single unlocked collection whose `SearchItems
  a{ss} 1 service Proton` returns an item (refuse, exit 13, if zero or several);
  `mutate(DbusSetAlias{"default", old, target})`; re-run the detector and report `fixed` only
  if the new alias resolves; also `mutate(WriteFile ~/.local/share/keyrings/default)` so it
  survives login. Prints "this switches which keyring the CLI reads; you may need to run
  `protonvpn signin` again" before applying. Never opens a `*.keyring` file, never `Unlock`,
  never restarts the daemon.
- `proton_session_item` (keyring, D): `SearchItems a{ss} 1 service Proton` → pass if an item
  exists (feeds `status.signed_in`), else `fail:not_signed_in`.
- `conflicting_tunnel` (network, D): any active `tun`/`wireguard` device other than
  `proton0` (tailscale0 today) → `warn`, hint `sudo tailscale down` / `tailscale set --exit-node=`.
- `stale_proton_profiles` (network, D, report-only): inactive `ProtonVPN *` profiles not
  named in `connection_persistence.json` → `warn` at ≥ 3; `fix_cmd` prints
  `nmcli connection delete '<id>'` for the human. Never automated (the profile holds the key).
- `killswitch_orphan` (network, B gated): `pvpn-killswitch-ipv6` active while `proton0` is
  down. `config.kill-switch` cached `standard`/`off` → `warn`, fixable (`nmcli connection
  down pvpn-killswitch-ipv6`, inverse `up`); `null` (config never cached) → `warn` report-only.
- `serverlist_present` (cache, D: exists, parses, ≥ 1 000 logicals), `serverlist_expired`
  (cache, D: `ExpirationTime` past → `warn`), `serverlist_loads_stale` (cache, D → `info`).
- `index_fresh` (cache, A: snapshot missing or older than the JSON → fix: rebuild).
- `state_dir` (install, A), `stale_pending_action` (install, A: pending older than its
  deadline → fix: clear), `settings_valid` (install, A).
- `binary_on_path` (install, D: `~/.local/bin/fproton` resolves; note `~/.cargo/bin` is not
  on the shell's PATH).
- `plugin_linked` (install, B: `~/.config/omarchy/plugins/oliver.protonvpn` exists and resolves
  into this repo; validate the RESOLVED path), `plugin_enabled` (install, B: id in shell.json
  via `omarchy plugin list`; fix: `omarchy plugin enable` + `bar put`), `menu_entries`
  (install, B), `old_plugin_enabled` (install, B: fix `omarchy plugin disable …`).
- `dns_when_connected` (connected, D) and `tunnel_route_ok` (connected, D): `skip:link_down`
  with a reason whenever `proton0` is absent; when up, `resolvectl dns proton0` shows
  10.2.0.1 and `ip route get 1.1.1.1` goes via proton0.

## 7a. TUI (`fproton tui`) — one agent, one pass, ≤ 1 200 lines across `src/tui/{mod,theme,screens,subs}.rs`

**Runtime.** ftui 0.7 Elm loop: one `struct Tui` implementing `ftui::Model`;
`App::fullscreen(tui).run()`; no mouse in v0.1. Owns a `Context` clone; calls fproton-core
in process (never spawns `fproton`). Reference code: `~/Projects/odcpw/fbittorrent/src/tui/{mod,theme}.rs`
(ftui 0.6, API unchanged) and `~/.cargo/registry/src/*/ftui-0.7.0/examples/`,
`ftui-runtime-0.7.0/src/{program.rs,subscription.rs,process_subscription.rs}`.

**Chrome (every screen).** Row 0: tab strip `Status · Servers · History` (`widgets::Tabs`).
Rows 1..h-2: the screen. Row h-1: `widgets::StatusLine` = `[state badge] [server or —]
[uptime] [↓rate ↑rate] │ [spinner + action label when busy] │ ? help`. Badge colour:
green connected, yellow connecting/disconnecting, dim disconnected, red error.

| Screen | Data source | Shows |
|---|---|---|
| Status (default) | `core::status(&ctx)` every 1 s via `Every`, run in `Cmd::task`, dropped if one is in flight | Hero panel: state phrase, server Name + country + city, load `MiniBar`, tier/feature chips, endpoint, uptime. Detail: rx/tx totals + rates with a 60-sample `widgets::Sparkline` of rx_bps (or "traffic unavailable"). Protections (read-only): `config.*`, `kill_switch.active` or "unknown — CLI not responding". Warnings: every entry with text + hint. Recents row. |
| Servers | `index::servers(&ctx, &q)` on each filter keystroke (debounced 80 ms); `index::countries` once | `widgets::virtualized::VirtualizedList` (18 221 rows; not `List`). Columns: Name · Country · City · Load bar · Score · Tier · features. Filter `TextInput` at top; `f`/`t`/`o` cycle feature/tier/sort chips; header `N/18221 shown`. |
| History | `state::history(&ctx, 200)` at init and after each completed action | `widgets::Table`: when · server · country · engine · result · duration. |

**Keymap** (also the exact content of the `?` overlay; one `&[(&str,&str)]` table so help and
behaviour cannot drift): `q`/`Ctrl+C` quit (refuses with a status-line note while an action is
in flight; second press within 2 s cancels and quits) · `?` help · `Tab`/`Shift+Tab`/`1 2 3`
screens · `r` refresh · `Esc` close overlay, else cancel the in-flight action · Status: `c`
connect default target, `d`/`D` disconnect / force, `R`/`n` reconnect / next, `S` open a
sign-in terminal (detached `omarchy-launch-floating-terminal-with-presentation fproton signin`)
· Servers: `/` filter, `↑↓ PgUp PgDn Home End`, `Enter` connect, `f t o` cycle · History:
`Enter` reconnect to that target.

**Long actions are cancellable Subscriptions, never `Cmd::task`** (`Cmd` has no cancel):

```rust
struct ActionSub { id: SubId, ctx: Context, action: Action }
impl Subscription<Msg> for ActionSub {
    fn id(&self) -> SubId { self.id }
    fn run(&self, tx: Sender<Msg>, stop: StopSignal) {
        let cancel = CancelToken::new(); let c2 = cancel.clone();
        std::thread::spawn(move || { loop { if stop.wait_timeout(Duration::from_millis(200)) { c2.cancel(); return } } });
        let r = match &self.action {
            Action::Connect(t) => engine::connect(&self.ctx, t, &cancel).map(|_| ()),
            Action::Disconnect(f) => engine::disconnect(&self.ctx, *f, &cancel),
            Action::Rebuild => index::rebuild(&self.ctx, &cancel).map(|_| ()),
        };
        let _ = tx.send(Msg::ActionDone(r.map_err(|e| e.to_string())));
    }
}
```
The model holds `action: Option<(SubId, Action, Instant)>`; `subscriptions()` returns the
`ActionSub` only while it is `Some`. `Esc`/quit sets it to `None` → StopSignal → CancelToken
→ core SIGTERM/SIGKILLs the child group and returns `Interrupted`; a cancelled connect is
followed by `disconnect --force`. The 1 s poll keeps running, showing `status.pending` and
`elapsed / deadline`.

**Empty, error and hang states — each must render, none may be blank:** index error →
"No server index. Press i to build it (fproton index --rebuild)" with `i` running Rebuild;
empty filter → "No servers match…"; `cli.hang_suspected`/backoff → persistent yellow row and
`c/R/n` refused with the same text; connect in flight → hero shows target, spinner,
`12s / 90s`, "Esc to cancel"; cancelled/timed out → red status line with `last_error`, History
gains the failed entry; `state == error` → red hero border, `last_error`, `commands` listed;
`traffic == null` → "traffic unavailable"; `signed_in == false` → hero replaced by "Not signed
in. Press S to open a sign-in terminal."

**Theme.** Copy `fbittorrent/src/tui/theme.rs` verbatim (reads Omarchy's current theme
colours, neutral fallback) and add `fn state(&self, s: &str) -> Style`. No literal colours
outside theme.rs. Re-read when the theme file's mtime changes (checked on the 1 s poll).

**Resize.** `view()` returns immediately on a zero area. ≥ 100 cols → two-column Status;
60-99 → single column, no sparkline; < 60 cols or < 16 rows → status line + hero only,
tabs collapse to `1·2·3`; below 30x8 render one line "fproton — terminal too small (need
30x8)". One unit test per tier renders into a `Buffer` and asserts no panic + badge present.

**Exit code.** `Ok(0)` on clean quit, `Ok(6)` if the last action failed, `Ok(130)` on Ctrl+C.

## 8. Phases, ownership, exit gates

### Phase 1 — core (one agent)
Owns: `Cargo.toml` (all three; pins the FINAL dependency set, then frozen),
`crates/fproton-core/**`, `crates/fproton/src/{main,output,setup}.rs`, compiling stubs
`crates/fproton/src/tui/mod.rs` and `crates/fproton/src/doctor/mod.rs`, `tests/e2e.sh`,
`tests/fixtures/**` (captures `protonvpn <sub> --help` for every subcommand and `config set
<K> --help` for every key as fixtures), `scripts/check.sh`, `docs/ROBOT.md` (generated).

Frozen at the gate (Phase 2 may only call these; may extend `TuiArgs`/`DoctorArgs` but not
change the `run` signatures or touch `main.rs`):

```rust
// fproton-core
#[derive(Clone)] pub struct Context { pub settings: Settings, pub paths: Paths, pub timeouts: Timeouts, pub json: bool, pub verbose: u8 }  // plain data, Send + Sync + 'static, no open handles
pub fn status(ctx: &Context) -> anyhow::Result<Status>;                       // §5 struct (serde Serialize); < 50 ms; never spawns protonvpn
pub mod index { pub struct Query { pub country: Option<String>, pub city: Option<String>, pub feature: Option<Feature>, pub tier: Option<Tier>, pub search: Option<String>, pub sort: Sort, pub limit: Option<usize> }
                pub fn servers(ctx: &Context, q: &Query) -> anyhow::Result<Vec<ServerRow>>;   // < 20 ms
                pub fn countries(ctx: &Context) -> anyhow::Result<Vec<CountryRow>>;
                pub fn cities(ctx: &Context, country: &str) -> anyhow::Result<Vec<CityRow>>;
                pub fn rebuild(ctx: &Context, cancel: &CancelToken) -> anyhow::Result<IndexStats>; }
pub mod state  { pub fn history(ctx: &Context, limit: usize) -> anyhow::Result<Vec<HistoryEntry>>; pub fn recents(ctx: &Context) -> anyhow::Result<Vec<Target>>; }
pub mod engine { /// polls `cancel` at least every 250 ms; on cancel SIGTERM/SIGKILLs the child group and returns Err(Error::Interrupted); never touches stdout/stderr
                 pub fn connect(ctx: &Context, target: &Target, cancel: &CancelToken) -> Result<Connected, Error>;
                 pub fn disconnect(ctx: &Context, force: bool, cancel: &CancelToken) -> Result<(), Error>; }
pub mod proc    { pub fn run_with_deadline(argv: &[&str], timeout: Duration, cancel: Option<&CancelToken>) -> Result<Output, Error>; }
pub mod secrets { pub fn read_alias_default() -> Result<AliasProbe, Error>; pub fn collections() -> …; pub fn search_proton_items() -> …; }  // busctl, read-only
pub struct CancelToken(Arc<AtomicBool>); impl CancelToken { pub fn new() -> Self; pub fn cancel(&self); pub fn is_cancelled(&self) -> bool; }
pub enum Error { … } impl Error { pub fn exit_code(&self) -> i32; pub fn code_name(&self) -> &'static str; pub fn hint(&self) -> String; }
// fproton (bin) stubs
pub fn tui::run(args: TuiArgs, ctx: Context) -> anyhow::Result<i32>;        // owned Context
pub fn doctor::run(args: DoctorArgs, ctx: Context) -> anyhow::Result<i32>;  // DoctorArgs carries #[command(subcommand)] sub: Option<DoctorCmd> with Undo{run}, Capabilities
```

Exit gate (all pasted into DECISIONS.md with real output): `cargo build --release`,
`cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`,
`tests/e2e.sh` green, `fproton status --json | jq .` real output, `hyperfine -w3 'fproton
status --json'` mean < 50 ms and `'fproton servers --country CH --json'` < 20 ms (misses
recorded, not hidden), `fproton robot-docs | head -3` prints real text, §4b matrix filled.

### Phase 2 — parallel, disjoint files (four agents)
- **tui**: `crates/fproton/src/tui/**`. Gate: clippy clean; `fproton tui` opens and quits
  cleanly on a real pty (use `script -qc` or a tmux pane to drive it); every screen renders
  with the index present and with the index missing; resize-tier unit tests; no
  `println!`; screenshot/text capture noted in DECISIONS.md.
- **plugin**: `plugin/oliver.protonvpn/**` except README, `tests/plugin-smoke.sh`. Gate:
  `omarchy-plugin-validate plugin/oliver.protonvpn` clean, `tests/plugin-smoke.sh` green with
  the plugin symlinked and enabled on this machine, real line counts recorded.
- **doctor**: `crates/fproton/src/doctor/**`, `tests/fixtures/doctor/**`. Gate: `fproton doctor
  --json | jq` lists every §7 check with a real verdict on this machine (the keyring check
  must report `fail:phantom_collection` today), `doctor --fix --dry-run` changes nothing,
  round-trip tests for Class A, `doctor capabilities --json` valid.
- **docs**: `README.md`, `plugin/oliver.protonvpn/README.md`, `scripts/install.sh`,
  `scripts/uninstall.sh`, menu-entry JSON template used by `setup --menu` (agree the path with
  the core agent: `crates/fproton/assets/menu.jsonc` written by Phase 1 as a stub). README
  follows readme-writing + de-slopify, has a "Status" section mirroring the §4b matrix, and
  claims nothing RED. Gate: `scripts/install.sh --dry-run` works; a reviewer can find every
  README claim in code or the matrix.

### Phase 3 — integrate (one agent)
Build the whole tree; fix cross-module breakage; agent-ergonomics pass (`--help` quality,
hints, `capabilities`, `robot-docs` accuracy — lite mode, no audit workspace); run
`scripts/check.sh` (fmt, clippy, test, e2e, plugin-smoke) green; the "core is silent" grep;
the secrets grep test; commit.

### Phase 4 — one review pass, one fix pass, one verifier
Three reviewers in parallel, read-only, returning findings inline (bugs+security;
honesty/README-vs-code-vs-matrix + mock/stub finder + `ubs`; plugin+TUI UX against §6/§7a).
One fixer confirms each finding in code before changing anything and applies the confirmed
ones. One verifier runs `scripts/install.sh --yes` on this machine, confirms the bar widget
loads (`tests/plugin-smoke.sh`), runs `scripts/check.sh`, re-fills the §4b matrix from real
output, and reports honestly. Nothing else.

Commits: each agent commits only its own paths (`git add <paths>`), message `<area>: <what>`,
trailer `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`; everyone works on
`main` in the same checkout.

## 9. Skills to read (paths; apply substance, skip ceremony — no beads, NTM, sibling workspaces)

- CLI/agents: `~/.claude/skills/agent-ergonomics-and-intuitiveness-maximization-for-cli-tools/references/QUICKREF.md` and `references/CHEAT-SHEET.md`
- Doctor: `~/.claude/skills/world-class-doctor-mode-for-cli-tools/assets/skill-card.md`, `references/methodology/KERNEL.md` (axioms 1-3, 7, 13-15), `COOKBOOK.md` patterns 4 and 8
- TUI: `~/.claude/skills/frankentui/references/TUI_POLISH_PLAYBOOK.md` §1 and §3 only (its file:line references point at a repo not on this machine). Do NOT read `tui-glamorous` (Go/Charmbracelet).
- Omarchy: `~/.claude/skills/omarchy/plugins.md`, `theming.md`
- Docs: `~/.claude/skills/readme-writing/SKILL.md`, `~/.claude/skills/de-slopify/SKILL.md`, `~/.claude/skills/installer-workmanship/SKILL.md`
- Quality: `~/.claude/skills/multi-pass-bug-hunting/SKILL.md`, `~/.claude/skills/ubs/SKILL.md` (`ubs` installed), `~/.claude/skills/mock-code-finder/SKILL.md`, `~/.claude/skills/just-say-no-to-process-porn-and-ceremony/SKILL.md`, `~/.claude/skills/reality-check-for-project/SKILL.md`, `~/.claude/skills/testing-real-service-e2e-no-mocks/SKILL.md`
