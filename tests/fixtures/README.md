# fproton test fixtures

Real inputs captured from this machine (host `t14`, Omarchy 4.0.3, Arch) on **2026-09-14**,
plus two files *derived* from the installed `proton-vpn-cli` Python source because the CLI
hangs on every session-touching command here (DESIGN.md §1.2).

Two provenance classes, and the difference matters:

- **captured** — the verbatim stdout+stderr of the command named below, written with
  `timeout 8 <cmd> > <file> 2>&1`. Byte-for-byte what the tool printed.
- **derived** — reconstructed offline by reading `/usr/lib/python3.14/site-packages/proton/vpn/cli/`.
  Every derived file starts with the line
  `# derived from proton-vpn-cli 1.0.3 source, not captured from a run`
  followed by further `#` provenance lines. **Consumers must skip leading `#` lines.**
  Replace these with real captures once the keyring is repaired, and delete the header.

Nothing here contains a secret: `nmcli` prints `wireguard.private-key:<hidden>` itself, the
WireGuard *public* keys (peer key, `X25519PublicKey`, `x25519pk`) are public by construction,
and `connection_persistence-redacted.json` is redacted (see below). No `busctl` fixture holds
an item secret — only object paths.

## protonvpn `--help` (captured)

`protonvpn --help` and `protonvpn <sub> --help` never touch the session and answered in
165-220 ms each while the CLI was hung on everything else.

| File | Command |
|---|---|
| `protonvpn-help-top.txt` | `protonvpn --help` |
| `protonvpn-help-signin.txt` | `protonvpn signin --help` |
| `protonvpn-help-signout.txt` | `protonvpn signout --help` |
| `protonvpn-help-info.txt` | `protonvpn info --help` |
| `protonvpn-help-connect.txt` | `protonvpn connect --help` |
| `protonvpn-help-disconnect.txt` | `protonvpn disconnect --help` |
| `protonvpn-help-status.txt` | `protonvpn status --help` |
| `protonvpn-help-servers.txt` | `protonvpn servers --help` |
| `protonvpn-help-countries.txt` | `protonvpn countries --help` |
| `protonvpn-help-countries-list.txt` | `protonvpn countries list --help` |
| `protonvpn-help-cities.txt` | `protonvpn cities --help` |
| `protonvpn-help-cities-list.txt` | `protonvpn cities list --help` |
| `protonvpn-help-config.txt` | `protonvpn config --help` |
| `protonvpn-help-config-list.txt` | `protonvpn config list --help` |
| `protonvpn-help-config-set.txt` | `protonvpn config set --help` |
| `protonvpn-help-config-set-netshield.txt` | `protonvpn config set netshield --help` |
| `protonvpn-help-config-set-kill-switch.txt` | `protonvpn config set kill-switch --help` |
| `protonvpn-help-config-set-port-forwarding.txt` | `protonvpn config set port-forwarding --help` |
| `protonvpn-help-config-set-custom-dns.txt` | `protonvpn config set custom-dns --help` |
| `protonvpn-help-config-set-vpn-accelerator.txt` | `protonvpn config set vpn-accelerator --help` |
| `protonvpn-help-config-set-moderate-nat.txt` | `protonvpn config set moderate-nat --help` |
| `protonvpn-help-config-set-ipv6.txt` | `protonvpn config set ipv6 --help` |
| `protonvpn-help-config-set-anonymous-crash-reports.txt` | `protonvpn config set anonymous-crash-reports --help` |

The eight `config set` keys above are exactly the eight listed by `config set --help`, in that
command's own (alphabetical) order; `config list` prints them in a different order — see below.

## protonvpn output (derived, not captured)

| File | Derived from | Notes |
|---|---|---|
| `protonvpn-info.txt` | `commands/account.py:107` | The whole output of `protonvpn info` is one line, `Account: '<name>'`. No plan, no tier. The name here is the placeholder `user@example.com`. |
| `protonvpn-config-list.txt` | `commands/settings.py:285-331`, `commands/feature_setting_definitions.py:67-190,475-484` | Paid tier (this account is `MaxTier: 2`). Blank line, `Current configuration`, a `tabulate(headers=["Setting","Value"], tablefmt="simple")` table, blank line, two-line footer. Row order is `ALL_FEATURES`: netshield, kill-switch, port-forwarding, custom-dns, vpn-accelerator, moderate-nat, ipv6, anonymous-crash-reports. The *values* are plausible, not this account's real settings — Proton stores no user settings on disk, only `config list` knows them. |
| `protonvpn-config-list-free-tier.txt` | same, plus `core/controller.py:69,257` | The other branch: the five features with `available_on_free_tier=False` render as `Upgrade to enable`, and the footer is the four-line upgrade block. Its `Value` column is 17 chars wide against 22 in the paid file — deliberate, so a parser that hard-codes column offsets fails here. |
| `derive-config-list.py` | — | The renderer that produced both `config-list` files, using the same `python-tabulate 0.10.0` the CLI itself imports. `python3 derive-config-list.py [free]`. |

Parse rule for the table (DESIGN.md §8): split on runs of **≥ 2 spaces**, anchored by the
`---` rule line; never on fixed columns. `custom-dns` legitimately contains a two-space run
inside its own value (`on  [1.1.1.1, 8.8.8.8]`), so split at most once, on the rule-derived
column boundary.

## NetworkManager (captured)

Both Proton profiles were **inactive** at capture time, and `tailscale0` was active — the
state DESIGN.md §4b calls GREEN for `disconnect --force` (a no-op). `tailscale0` being up
is not a conflict on its own: `tests/fixtures/doctor/conflicting_tunnel/input/ip-route-*.json`
(captured `ip -j route show default` / `table 52`, no exit node) make that check pass, and
`conflicting_tunnel__exit_node` adds a `default dev tailscale0` row to table 52 to produce
the warning.

| File | Command | ms |
|---|---|---|
| `nmcli-connections-disconnected.txt` | `nmcli -t -f NAME,TYPE,DEVICE,ACTIVE connection show` | 20 |
| `nmcli-connections-uuid-disconnected.txt` | `nmcli -t -f NAME,UUID,TYPE,DEVICE,ACTIVE connection show` | 20 |
| `nmcli-general-permissions.txt` | `nmcli general permissions` | 101 |
| `nmcli-show-protonvpn-profile.txt` | `nmcli -t connection show 58d6042d-3c68-4ee2-81d6-77bcc608a95c` (`ProtonVPN CH#461`) | — |
| `nmcli-show-killswitch.txt` | `nmcli -t connection show e687d04b-b08b-487f-ba55-c8f2183020b5` (`pvpn-killswitch-ipv6`) | — |

`nmcli-general-permissions.txt` is the evidence for the privilege boundary in DESIGN.md §1.3:
`settings.modify.own`, `settings.modify.system` and `network-control` are all `yes`, so every
nmcli call fproton makes needs no sudo and raises no polkit prompt; `reload`,
`checkpoint-rollback` and the global-DNS/hostname writes are `auth` and fproton never uses them.

`nmcli-show-protonvpn-profile.txt` now includes the two lines the earlier capture dropped —
`wireguard.private-key:<hidden>` and `wireguard.private-key-flags:0`. `<hidden>` is nmcli's own
redaction (the key lives in the root-only keyfile); `private-key-flags:0` is what proves it.

## Secret Service / keyring (captured)

The `busctl` argv below are the exact ones DESIGN.md §7 gives for
`secret_service_default_alias` and `proton_session_item`. Each answered in 3-13 ms and none
raised a gcr prompt. This machine is in the broken state the doctor must detect: the `default`
alias points at a collection that is listed in `Collections` but has **no exported object**.

| File | Command | Result |
|---|---|---|
| `busctl-readalias-default.txt` | `busctl --user --timeout=1 call org.freedesktop.secrets /org/freedesktop/secrets org.freedesktop.Secret.Service ReadAlias s default` | `o "…/collection/Default_5fkeyring"` |
| `busctl-collections.txt` | `busctl --user --timeout=1 get-property org.freedesktop.secrets /org/freedesktop/secrets org.freedesktop.Secret.Service Collections` | `ao 3` — `Default_5fkeyring`, `Default`, `session` |
| `busctl-getprop-label-phantom.txt` | `busctl --user --timeout=1 get-property org.freedesktop.secrets /org/freedesktop/secrets/collection/Default_5fkeyring org.freedesktop.Secret.Collection Label` | **exit 1**, `Object does not exist at path …` — the `fail:phantom_collection` verdict |
| `busctl-getprop-label-locked-default.txt` | `busctl --user --timeout=1 get-property org.freedesktop.secrets /org/freedesktop/secrets/collection/Default org.freedesktop.Secret.Collection Label Locked` | `s "Default"` / `b false` — the healthy, unlocked collection |
| `busctl-searchitems-service-proton.txt` | `busctl --user --timeout=1 call org.freedesktop.secrets /org/freedesktop/secrets org.freedesktop.Secret.Service SearchItems 'a{ss}' 1 service Proton` | `aoao 2 …/Default/1 …/Default/2 0` — two unlocked items, zero locked |
| `busctl-searchitems-collection-default.txt` | same call on `/org/freedesktop/secrets/collection/Default` with interface `org.freedesktop.Secret.Collection` | `ao 2` — the per-collection form the `--fix` path uses to pick its target |
| `busctl-list-secrets.txt` | `busctl --user list --no-pager \| grep -E '^(NAME\|org\.freedesktop\.secrets\|org\.kde\.secretservicecompat)'` | filtered (the full listing is 106 unrelated session services); `org.freedesktop.secrets` is owned by `gnome-keyring-d`, and `org.kde.secretservicecompat` is *activatable* — a second provider that could take the name |

**Trap for the parser:** the phantom-path error uses typographic quotes —
`Object does not exist at path “/org/…”` (U+201C/U+201D), not `"`. Match on the prefix
`Object does not exist at path`, never on an ASCII-quoted path.

## Proton cache and system state

| File | Provenance |
|---|---|
| `serverlist-small.json` | captured — a 38-logical subset of `~/.cache/Proton/VPN/serverlist.json` (18 221 logicals, 24 432 460 bytes), with the real top-level keys intact: `LastModifiedTime` is the HTTP-date **string** that breaks a naive numeric deserialize, `ExpirationTime`/`LoadsExpirationTime` are epoch floats, `MaxTier` is 2. Covers every server-name shape DESIGN.md §1.3 names — `CH#461`, `US-CA#1022`, `NL-FREE#15`, `SE-JP#1`, `IS-DE#1`, `FR#13-TOR`, `US-CO#21-TOR` — and `Features` values 1 (secure-core), 2 (tor) and 28 (p2p\|streaming\|ipv6). |
| `clientconfig.json` | captured — a verbatim copy of `~/.cache/Proton/VPN/clientconfig.json` (byte-identical as JSON; `DefaultPorts.WireGuard.UDP` = 443, 88, 1224, 51820, 500, 4500). |
| `connection_persistence-redacted.json` | captured + redacted — `~/.cache/Proton/VPN/connection/connection_persistence.json` with every key whose name contains `pk` or `key` replaced by `"<redacted>"` (here: `server.x25519pk`). Re-serialized with `json.dumps` default separators, which round-trip the source file byte-for-byte; no trailing newline, same as the original. `connection_id` **is** the NetworkManager UUID of `ProtonVPN CH#461` — match profiles by this, never by name order. |
| `systemctl-is-active.txt` | captured — `systemctl is-active proton.VPN.service NetworkManager` → two lines, both `active`. Exit 0. 7 ms. |
| `lsmod-wireguard.txt` | captured — `lsmod \| grep wireguard`; the `wireguard` module is loaded (refcount 0, no tunnel up). |

## Refreshing

Every captured file can be regenerated by rerunning the command in its row, redirecting
stdout+stderr, under `timeout 8`. Never run a bare `protonvpn` session command without a
timeout on this machine (DESIGN.md §1.2): `--help` is safe, everything else hangs.
