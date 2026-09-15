# fproton — notes for agents working in this repo

Proton VPN for Omarchy: Rust CLI + engine (`crates/fproton-core`, `crates/fproton`), a
FrankenTUI app (`fproton tui`), a `doctor`, and a thin Quickshell bar widget
(`plugin/oliver.protonvpn`). Owner: Oliver (GitHub org `odcpw`).

## Read first
- `DESIGN.md` — the contract (v3). §2 non-negotiables, §4 CLI table + exit codes, §4b the
  GREEN/AMBER/RED verification matrix, §5 Status JSON, §6 plugin, §7 doctor, §7a TUI.
- `DECISIONS.md` — append-only log with real gate transcripts. Read the newest entries
  before assuming anything about what works.
- `README.md` "Status" section — what is verified on this machine and what is not.

## Rules that are easy to break
- Never hang: every external process goes through `fproton_core::proc::run_with_deadline`
  (process group, piped stdio, deadline). `status` never spawns `protonvpn`.
- argv only — no shell strings anywhere (Rust or QML). `omarchy-launch-*` helpers and
  `bar.run()` are shell strings: pass constant literals only.
- No secrets: never read `~/.local/share/keyrings/*.keyring`, NM keyfiles, or run
  `nmcli --show-secrets`. The only sanctioned keyring read is the 16-byte pointer file.
- Core is silent (no print macros in `fproton-core`). The doctor never deletes anything.
- `--json` on every command; non-TTY stdout is JSON; errors carry a runnable `hint`.
- Claim nothing RED in README/`--help`/robot-docs. The keyring fault (DESIGN §1.2) was
  repaired by a reboot on 2026-09-15; the CLI answers and connects now. Keep the deadlines
  anyway — that hang is the failure mode this tool exists for.
- Append to `DECISIONS.md`; never rewrite earlier entries.

## Build, test, install
```bash
scripts/check.sh --full        # fmt, clippy -D warnings, tests, e2e --full, plugin-smoke
scripts/install.sh --yes       # cargo build --release, ~/.local/bin/fproton, fproton setup --all
tests/e2e.sh --fast            # GREEN rows only, < 15 s
```
The bar widget is a symlink into `plugin/oliver.protonvpn`; QML edits need
`omarchy restart shell` (rescanPlugins does not reload an already-loaded plugin). Validate the
repo directory (`omarchy-plugin-validate plugin/oliver.protonvpn`), never the installed link.

## Layout
`crates/fproton-core/src/{proc,nm,route,cli,engine,index,natpmp,state,settings,secrets,status,agent,wg}.rs`
· `crates/fproton/src/{main,output,setup}.rs`, `src/doctor/`, `src/tui/` · `plugin/oliver.protonvpn/`
· `tests/{e2e.sh,plugin-smoke.sh,fixtures/}` · `scripts/{check,install,uninstall}.sh` · `docs/ROBOT.md` (generated).

## State (2026-09-15)
Signed in as `odc`; `connect CH` / `disconnect` / `reconnect` verified live (CH#507, exit IP
in Switzerland, DNS and default route via `proton0`, doctor 26/26). Still unverified: `port`
success (needs `fproton config set port-forwarding on` and a P2P server) and the `wg`
experiment (DESIGN §4 kill criterion; certificate expiry 2026-09-21). Tailscale coexists
(no exit node); the kill switch is off in the owner's Proton config.
