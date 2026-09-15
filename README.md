# fproton

Proton VPN for Omarchy. A Rust CLI and engine, a FrankenTUI app, a `doctor`, and a thin
Quickshell bar widget (`oliver.protonvpn`). Proton's own CLI stays underneath as the thing
that negotiates the tunnel; fproton is what you use, and it keeps answering when that CLI
wedges.

It replaces `io.github.grichard99.omaproton-vpn`, a 4 700-line QML/Python bar widget that
stopped loading here in September 2026 and is now disabled.

## Why

Two things went wrong.

**The old plugin was too much QML.** Timeouts, hang handling, cache parsing and NAT-PMP port
forwarding lived in QML and Python inside a Quickshell plugin, where none of it could be
tested. A single syntax error in `Service.qml` took the whole widget out of the bar.

**The Proton CLI hangs.** `protonvpn --help` answers in about 0.4 s, but on this machine every
session-touching command (`status`, `info`, `config list`, `connect`) blocks forever. The
cause is the login keyring, not Proton: gnome-keyring-daemon crashed and was hand-repaired,
and the Secret Service `default` alias now points at
`/org/freedesktop/secrets/collection/Default_5fkeyring`, a collection that is listed in
`Collections` but has no exported object, so every property read on it fails with "Object
does not exist at path". python-keyring resolves the default collection on every call, so
each session command waits on a gcr prompt that never maps under Hyprland. The real Proton
credentials are fine; they live in the `Default` collection, which is unlocked.

So fproton is built to work while its backend is wedged:

- `fproton status` never spawns `protonvpn`. It reads NetworkManager, Proton's own cache
  files, `/sys/class/net`, and one read-only Secret Service probe (11 ms here).
- Every external process runs in its own process group under a deadline (SIGTERM, then
  SIGKILL two seconds later), with all three stdio pipes closed off. A deadline kill is
  exit 5 with a runnable hint, never a hung terminal.
- After a deadline kill, CLI-backed commands fail fast for 60 s instead of queueing up
  behind another doomed call.
- argv only. No `sh -c`, no shell strings, in Rust or in QML. Targets are validated against
  the real server-name grammar before they reach a command line.
- No secrets, enforced by path: fproton never reads, copies, hashes or logs anything under
  `~/.local/share/keyrings/`, `/etc/NetworkManager/system-connections/`, or
  `nmcli --show-secrets` (never invoked). The one exception is the doctor's keyring fixer,
  which rewrites the 16-byte `keyrings/default` pointer file, whose content is a keyring
  basename, never key material.

## Install

```bash
scripts/install.sh --dry-run          # print the plan
scripts/install.sh --yes              # build, install to ~/.local/bin, wire up Omarchy
```

That is `cargo build --release`, an atomic install of `~/.local/bin/fproton`, and
`fproton setup --all --yes`. Run it again after a `git pull`; every step is idempotent.
`scripts/uninstall.sh` reverses it (`--menu` also strips the menu entries, `--purge` also
deletes fproton's own config, state and cache).

By hand, or to see what `setup` does:

```bash
cargo build --release && install -m0755 target/release/fproton ~/.local/bin/
ln -sfn "$PWD/plugin/oliver.protonvpn" ~/.config/omarchy/plugins/oliver.protonvpn
omarchy plugin enable oliver.protonvpn
omarchy bar move oliver.protonvpn --before omarchy.tailscale
fproton setup --menu                  # adds the protonvpn.* entries to the Omarchy menu
```

The Omarchy shell's PATH includes `~/.local/bin` but not `~/.cargo/bin`, so install there.
Menu entries land in `~/.config/omarchy/extensions/omarchy-menu.jsonc` under `protonvpn.*`;
`setup --menu` inserts whole lines before the file's final `}`, changing at most the
trailing comma of the entry that was previously last; comments and every other line are
untouched, and `--dry-run` shows that one rewrite as a paired `-`/`+`.

## CLI

Every command takes `--json` and prints one JSON object; a non-TTY stdout implies `--json`.
Errors are `{"ok":false,"schema":1,"code":N,"code_name":"…","error":"…","hint":"<runnable
command>"}` with the same exit code, and every human-mode error ends in a `hint:` line.

```
fproton                       TUI on a terminal; `status --json` when stdout is not a TTY
fproton status                the whole Status document (schema 1); never spawns protonvpn
fproton connect [TARGET]      TARGET: fastest | CC | CC#N | city:NAME | p2p | secure-core | tor | random
fproton disconnect [--force]  --force tears down proton0 and the IPv6 leak guard via nmcli (needs --yes)
fproton reconnect [--next]    last target again; --next takes the next server in that country by score
fproton servers               --country --city --feature --tier --search --sort --limit, from the local index
fproton countries             counts, lowest load, has_free
fproton cities [--country CC]
fproton index [--rebuild] [--stats]      the postcard snapshot of Proton's serverlist.json
fproton config list|get|set   Proton's own settings, through the Proton CLI, under a deadline
fproton settings [get|set]    fproton's config.toml
fproton port                  one-shot NAT-PMP request through the tunnel
fproton history [--limit N] · fproton recents
fproton account · fproton signin [USER] · fproton signout
fproton doctor [--fix] [--allow-keyring] [--only GROUP] [--dry-run] [--save]
fproton doctor undo latest · fproton doctor capabilities
fproton setup [--plugin] [--menu] [--disable-old-plugin] [--all]
fproton tui [--servers|--history]
fproton capabilities · fproton robot-docs · fproton version
fproton wg up|swap|list       EXPERIMENT, off unless FPROTON_EXPERIMENTAL_WG=1
```

Global flags: `--json`, `--timeout <s>` (clamps every external deadline), `-v`,
`--no-color`, `--yes`, `--dry-run`.

Exit codes: 0 ok · 1 failure · 2 usage · 3 safety (needs `--yes`) · 4 backend · 5 upstream
(the Proton CLI errored or was killed at its deadline) · 6 not found · 7 ambiguous ·
8 timeout · 9 not signed in · 10 index · 11 doctor found failures · 12 a fixer failed ·
13 doctor refused · 130 interrupted.

Upstream `protonvpn servers` only prints a URL. fproton instead reads
Proton's own 24 MB `serverlist.json` from `~/.cache/Proton/VPN/`, keeps a postcard snapshot
in `~/.cache/fproton/index.bin`, and filters locally. The snapshot carries the source file's
mtime and length, so a stale one is rebuilt rather than trusted; `status` reports staleness
and never rebuilds.

### For agents

```
fproton robot-docs                        # the handbook, once
fproton capabilities --json               # commands, exit codes, warning ids, file paths
fproton status --json | jq -r .state      # connected | connecting | disconnecting | disconnected | error
```

`status` is a total function of link state, a live pending action and the last error, so
there is no `unknown`. Every key is always present; unknown is `null`, never an absent key
and never a zero standing in for unknown.

## Status

Verified on the owner's machine on 2026-09-15, after the keyring repair (a reboot) and a
sign-in. Until then the Proton CLI hung on every session command, which is why the project
tracks every command in one of three classes. The evidence for each row is the
"first connect after the keyring repair" entry at the end of `DECISIONS.md`.

| Class | Meaning | Commands |
|---|---|---|
| GREEN | verified end to end here | `status`, `servers`, `countries`, `cities`, `index`, `settings`, `history`, `recents`, `capabilities`, `robot-docs`, `version`, `setup`, `tui`, `doctor` (26/26 pass while connected, including `dns_when_connected` and `tunnel_route_ok`), `signin`, `account`, `config list\|get\|set`, `connect` (CH#507 in 1.8 s), `disconnect` (0.9 s), `disconnect --force`, `reconnect` (1.6 s), `port` error path (exit 5 with the hint to turn port forwarding on) |
| AMBER | exercised only on its error or timeout path | `signout` (never run; it would end the session), the 60 s CLI-hang backoff and the deadline kills, which the e2e suite still asserts under `--timeout 3` |
| RED | not verified | `port` success (port forwarding is off in the owner's config), every `wg` subcommand (env-gated experiment, see `DESIGN.md` §4 for its kill criterion) |

`fproton capabilities` carries `build_stage` and `build_note` for the build you actually
have. Before the repair, the e2e suite proved the other half of the design: a wedged CLI
yields exit 5 and a runnable hint within the deadline instead of a hang.

## The widget

`plugin/oliver.protonvpn` is a Quickshell bar widget for Omarchy 4. It renders the Proton
mark in the bar, coloured by state, crossed out when disconnected and badged when something
is wrong, and opens a keyboard-navigable panel: server and country, load, uptime, rx/tx,
warnings, quick connect (Fastest, P2P, Secure Core, Tor, Random), recents, a filterable
country list, the Proton protections (kill switch, NetShield, port forwarding) when the CLI
has answered, and a footer with the TUI, the doctor and sign in or out.

The QML holds no business logic. It polls `fproton status --json` and runs argv-validated
`fproton` commands; no Python, no nmcli, no shell strings. See
`plugin/oliver.protonvpn/README.md`.

## The TUI

`fproton tui` is a FrankenTUI app with three screens: Status, Servers and History. It calls
the same Rust library in process, so it never shells out to `fproton`. Long actions run as
cancellable subscriptions: `Esc` cancels a connect, which SIGTERMs the child process group
and follows with a forced disconnect rather than leaving a half-built tunnel. `q` refuses
while an action is in flight and quits on a second press. The Servers screen lists every
logical server in Proton's list (18 221 today) through a virtualized list, with a filter and
feature, tier and sort chips. Colours come from the active Omarchy theme and reload when the
theme file changes.

## The doctor

`fproton doctor` runs detectors over captured input and prints a table (or JSON with
`--json`): one row per check with a group, a severity, a status of pass, warn, fail or skip,
a detail line and, where one exists, a `fix_cmd` you can run yourself. Exit 11 means it found
failures, so `fproton doctor --json >/dev/null; echo $?` is the health check for scripts.

`--fix` applies only the findings whose repair is reversible, and every mutation goes
through one chokepoint that appends its inverse to `runs/<id>/actions.jsonl` with a verbatim
backup and a SHA-256 of any file it writes. `fproton doctor undo latest` replays that log in
reverse, and refuses with exit 13 if a backup is missing or its hash no longer matches. The
doctor never deletes anything: no file, no NetworkManager profile, no keyring entry. Findings
that need a deletion print the command for you instead.

The keyring check is the one that matters here. `secret_service_default_alias` calls
`ReadAlias("default")` and then reads `Label` and `Locked` on the path it returns. When that
read fails with "Object does not exist at path", the alias points at a phantom collection and
the check reports `fail:phantom_collection`, naming the alias target, the live collections,
and which collection actually holds a `service=Proton` item. That is today's state on this
machine, and it is why the Proton CLI hangs.

Repairing it touches state fproton does not own, so it is gated twice:

```bash
fproton doctor --fix --allow-keyring --yes
```

It re-points the Secret Service `default` alias at the single unlocked collection that holds
a Proton item, by `SetAlias`, and writes that collection's basename into
`~/.local/share/keyrings/default` so the alias survives a login. It refuses (exit 13) if zero
or several collections qualify. It never opens a `*.keyring` file, never calls `Unlock`,
never restarts gnome-keyring-daemon, and it warns first that switching keyrings may mean
running `protonvpn signin` again. Undo clears the alias rather than restoring it, because
the previous target does not exist; `doctor undo` says so and exits 0.

## Layout

```
crates/fproton-core/     the library: status, index, engine, nm, cli, natpmp, secrets, state, proc
crates/fproton/          the binary: clap surface, output, setup, doctor/, tui/
plugin/oliver.protonvpn/ the Quickshell bar widget
scripts/install.sh · scripts/uninstall.sh · scripts/check.sh
tests/e2e.sh · tests/plugin-smoke.sh · tests/fixtures/
docs/ROBOT.md            generated by `fproton robot-docs`, never hand-edited
DESIGN.md                the contract every agent works from
DECISIONS.md             append-only log of non-obvious choices, with real command output
```

Core is silent: no function in `fproton-core` writes to stdout or stderr, and a grep enforces
it. All output is produced by the binary.

## Config

`~/.config/fproton/config.toml`

```toml
engine = "cli"                 # the only engine in v0.1
default_target = "fastest"
notify = false

[timeouts]
probe_sec = 15
connect_sec = 90
disconnect_sec = 25
config_set_sec = 20
nmcli_sec = 10
busctl_sec = 1
```

State lives in `~/.local/state/fproton/` (history, recents, pending action, caches, doctor
runs and backups), the server index in `~/.cache/fproton/index.bin`. Proton's own settings
are not stored on disk by Proton at all, so `fproton config list` is the only way to read
them, and fproton caches the result.

## Limitations

- v0.1 never contacts Proton's API. The server list is whatever the official CLI last
  refreshed into `~/.cache/Proton/VPN/serverlist.json`, and loads expire within hours. An
  expired list is served with a warning, not refused.
- `wg up|swap|list` is an unverified experiment behind `FPROTON_EXPERIMENTAL_WG=1`. A raw
  WireGuard tunnel gets no NetShield, no port forwarding and no kill switch (the CLI's local
  agent negotiates those after the tunnel is up), and the existing profile's key is bound to
  a session certificate that expires. It will be deleted rather than fixed if it fails its
  kill criterion.
- No WorldMap in the panel, no cities drill-down, no notifications wired on by default, no
  `port --keepalive` loop, no `watch` stream, no `wg import`. Those are v0.2 material.
- Port forwarding is a one-shot NAT-PMP request with a 60 s lease; keeping it alive is the
  caller's job in v0.1.
- `tailscale0` is active on this machine. That is not a conflict: without an exit node
  Tailscale routes only the tailnet, so it coexists with a Proton tunnel. The doctor warns
  only when another tunnel owns a default route or a Tailscale exit node is set, and it
  notes that Proton's kill switch blocks direct tailnet peer traffic while connected.

## Credits

The Proton mark is ported from grichard99's
[omaproton-vpn](https://github.com/grichard99/omaproton-vpn) plugin (MIT), which drew it from
the [Simple Icons](https://simpleicons.org/) Proton VPN path (CC0) so it takes the theme
foreground instead of the brand colours. `WorldMap.qml` and `World.js` from the same plugin
are kept under `docs/reference/` with its licence as the starting point for the v0.2 map.
The panel's chrome follows Omarchy's own Tailscale plugin.

*About Contributions:* Please don't take this the wrong way, but I do not accept outside
contributions for any of my projects. I simply don't have the mental bandwidth to review
anything, and it's my name on the thing, so I'm responsible for any problems it causes; thus,
the risk-reward is highly asymmetric from my perspective. I'd also have to worry about other
"stakeholders," which seems unwise for tools I mostly make for myself for free. Feel free to
submit issues, and even PRs if you want to illustrate a proposed fix, but know I won't merge
them directly. Instead, I'll have Claude or Codex review submissions via `gh` and
independently decide whether and how to address them. Bug reports in particular are welcome.
Sorry if this offends, but I want to avoid wasted time and hurt feelings. I understand this
isn't in sync with the prevailing open-source ethos that seeks community contributions, but
it's the only way I can move at this velocity and keep my sanity.

MIT. Proton VPN is a trademark of Proton AG; this is an unofficial client for their CLI.
