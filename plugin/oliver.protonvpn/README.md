# oliver.protonvpn

The Proton VPN bar widget for Omarchy 4, and the front end of
[fproton](../../README.md). It draws the Proton mark in the bar and opens a
keyboard-driven panel; everything it knows comes from `fproton status --json`, and everything
it does is an `fproton` command run as argv.

The QML holds no business logic: no Python, no nmcli, no parsing of Proton's cache, no shell
strings. When the panel needs a fact that Status does not carry (countries, recents) it asks
for it with another `fproton … --json` call.

## Install

The widget comes with fproton:

```bash
fproton setup --plugin        # symlink, enable, place before omarchy.tailscale
```

By hand:

```bash
ln -sfn "$PWD/plugin/oliver.protonvpn" ~/.config/omarchy/plugins/oliver.protonvpn
omarchy plugin enable oliver.protonvpn
omarchy bar move oliver.protonvpn --before omarchy.tailscale
```

`fproton` must be on the shell's PATH, which includes `~/.local/bin` and not `~/.cargo/bin`.
The panel says so, and stops polling, when it cannot find the binary.

## Developing

A symlinked plugin directory is discovered and loaded, but it does not hot-reload: the
registry watches with `inotifywait -r`, which does not descend symlinks. **Nor does a
rescan help** — measured on this machine by appending a deliberate syntax error to
`Panel.qml`: `rescanPlugins` logged nothing, the widget kept running the old code and
`tests/plugin-smoke.sh` still passed. The registry's reload is guarded by
`Qt.clearComponentCache` and does not re-read the QML of an already-loaded symlinked
plugin. So after every QML edit:

```bash
omarchy restart shell                        # the dev loop: this is what re-reads the QML
journalctl --user -t omarchy-shell -f        # real failures read: Plugin widget <id> failed: …
```

`omarchy-shell shell rescanPlugins` is the **cold-load** path — it picks up a plugin
directory that was not there before. It will not show you an edit to one already loaded.

`omarchy-plugin-validate` refuses any symlink, so validate the repository directory and
never the installed link:

```bash
omarchy-plugin-validate plugin/oliver.protonvpn
tests/plugin-smoke.sh                        # validate, manifest checks, rescan, read the journal
```

One benign line appears on every reload and the smoke test whitelists it:
`QML IpcHandler … Handler was registered but will not be used`.

## Files

| File | Contents |
|---|---|
| `manifest.json` | id, kinds, `entryPoints.barWidget`, and the Setup > Plugins form (`barWidget.defaults` / `schema`) |
| `Service.qml` | the state model: one poll process, the action queue, notifications |
| `Panel.qml` | the bar button and the panel content, including the keyboard state machine |
| `ProtonIcon.qml` | the Proton mark, drawn from the Simple Icons path, with `crossed` and `warning` |
| `Model.js` | pure formatters: bytes, rates, uptime, flag emoji, feature labels |

## Settings

Omarchy never merges a manifest's `barWidget.defaults` into the plugin's `settings`:
enabling the widget writes `{"id":"oliver.protonvpn"}` into `shell.json`, and the widget
receives only the keys someone typed in the Setup form. Every setting therefore has an
in-code fallback that repeats the manifest value, and the smoke test asserts that each
manifest default has one.

| Setting | Default | Meaning |
|---|---|---|
| `binary` | `fproton` | resolved with `which` on first load |
| `refreshIntervalSec` | 3 (1-60) | poll interval while the panel is open |
| `idleRefreshIntervalSec` | 8 (2-600) | poll interval while it is closed |
| `notifications` | `off` | `on` sends one notification per state transition, never per poll |
| `defaultTarget` | `fastest` | what the toggle and the right-click connect to |
| `tuiCommand` | `omarchy-launch-or-focus-tui --app-id=fproton fproton tui` | how the footer opens the TUI |

## Using it

Bar icon: left click toggles the panel, right click connects or disconnects, middle click
refreshes. The mark is crossed out while disconnected, pulses while connecting or
disconnecting, turns urgent on an error, and carries a warning badge when `fproton` cannot be
reached or Status carries a warning.

In the panel: `j`/`k` (or the arrows) move, `h`/`l` move within a row, Enter or Space
activates, `/` filters the country list, Tab moves to the next bar panel, and Esc closes the
filter, then the panel. Shortcuts: `c` connect, `d` disconnect, `f` force disconnect,
`r` refresh, `t` open the TUI.

The protections rows (kill switch, NetShield, port forwarding) appear only when Proton's
settings have actually been read. While the Proton CLI is not answering, Status carries
`config: null`, and the panel says "Protections unknown" with a Refresh and a Run doctor
button rather than showing a switch whose position it is guessing. A connect that runs past
its deadline shows a "Taking long" row that offers the forced disconnect.

## Safety

- `bar.run()` is never used: it is `bash -lc <string>`. Every action is
  `Quickshell.execDetached([...])` with an argv array.
- `omarchy-launch-or-focus-tui`, `omarchy-launch-or-focus` and
  `omarchy-launch-floating-terminal-with-presentation` all end up feeding a string to `eval`
  or `bash -c`, so only constant literals are passed to them: never a server name, a target,
  or anything derived from Status. `omarchy-notification-send` is genuinely argv-shaped and
  is the only one that carries computed text.
- Country codes, server names, city names and recents are validated against the same grammar
  fproton uses before they are put in an argv; anything that does not match is dropped
  rather than sent.
- The doctor opens under app-id `fprotondoctor`, not `fproton-doctor`:
  `omarchy-launch-or-focus` matches `\b<pattern>\b` against the window class and title, so a
  hyphenated id would also match the TUI window.

## Credits

`ProtonIcon.qml` is ported from grichard99's omaproton-vpn plugin (MIT), which drew the mark
from the Simple Icons Proton VPN path (CC0). The panel's chrome and its keyboard handling
follow Omarchy's first-party Tailscale plugin.
