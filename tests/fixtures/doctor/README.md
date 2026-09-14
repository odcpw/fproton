# `tests/fixtures/doctor/` — captured inputs for the doctor's parsing detectors

DESIGN.md §7 requires a captured-input fixture for the nine detectors that parse
structured text, and a table test for the one-line predicates. Every directory here is

```
<check-id>[__<variant>]/
  input/            one file per probe, named after the probe it feeds
  expect.json       {check, status, reason, fixable, detail_contains[]}
```

`crates/fproton/src/doctor/probes.rs::from_fixture_dir` reads `input/` into the same
`Probes` fields the live capture fills and parses them with the same functions
(`fproton_core::secrets::parse_*`, `nm::parse_connections`, `serverlist_facts_from_bytes`,
`toml` → `Settings` → `settings::validate`), so a fixture exercises the real parse path
rather than a re-implementation of it. The test is
`doctor::checks::tests::every_fixture_directory_produces_the_verdict_it_declares`.

`input/meta.json` carries what a text file cannot: exit codes (`codes`), deadline kills
(`timed_out`), the `now_unix` a time comparison is made against, and the small number of
derived facts the `killswitch_orphan` preconditions read from fproton's own state.

## Provenance

Captured live on this machine (host `t14`, 2026-09-14/15), read-only commands only:

| File | Command |
|---|---|
| `*/input/nmcli-connections.txt` | `nmcli -t -f NAME,UUID,TYPE,DEVICE,ACTIVE connection show` |
| `secret_service_default_alias/input/busctl-readalias-default.txt` | `busctl --user --timeout=1 call org.freedesktop.secrets /org/freedesktop/secrets org.freedesktop.Secret.Service ReadAlias s default` |
| `secret_service_default_alias/input/busctl-alias-property.txt` | the same `get-property … Label Locked` on the returned path — it **fails**, which is the bug (`meta.json` records exit 1) |
| `secret_service_default_alias/input/busctl-collections.txt` | `get-property … Secret.Service Collections` |
| `secret_service_default_alias/input/busctl-collection-<name>-{property,searchitems}.txt` | `get-property`/`SearchItems` per collection; a file is **absent** where the call failed, which is how `exported: false` is expressed |
| `secret_service_default_alias/input/keyrings-default` | `cat ~/.local/share/keyrings/default` — 16 bytes naming a keyring *basename*, the single §2 exception; no `*.keyring` file is ever read |
| `proton_session_item/input/busctl-searchitems-service-proton.txt` | `SearchItems a{ss} 1 service Proton` |

Reused from the Phase-0 fixtures, unchanged except where noted:

- `cli_session_probe/input/protonvpn-config-list.txt` — `tests/fixtures/protonvpn-config-list.txt`
  with its `#` provenance header stripped (the CLI prints no such header). That file is
  *derived from the proton-vpn-cli 1.0.3 source*, not captured from a run: the CLI hangs on
  this machine (§1.2), so the only `config list` output that exists today is the derived one.
  The `fail:hang` branch this machine actually produces is a `timed_out` flag, not a parse,
  and is covered by the table test.
- `serverlist_{present,expired}/input/serverlist.json` — `tests/fixtures/serverlist-small.json`
  (38 real logical servers). 38 is below the 1 000-logical floor `serverlist_present`
  enforces, so that fixture legitimately expects `fail:too_small` — the branch a truncated
  cache hits. The live 18 221-server file passes; see the gate transcript in DECISIONS.md.

Hand-edited from the live capture above, for a branch that cannot be produced today
without changing the desktop. Each is a one-field edit of real output:

- `killswitch_orphan__orphan/input/nmcli-connections.txt` — the real listing with
  `pvpn-killswitch-ipv6`'s `DEVICE`/`ACTIVE` columns set to `ipv6leakintrf0`/`yes`.
  Producing this for real means bringing the IPv6 leak guard up, which the doctor must
  never do to a user's machine as a test.
- `stale_proton_profiles__accumulated/input/nmcli-connections.txt` — the real listing plus
  three further `ProtonVPN <name>` WireGuard profiles with synthetic UUIDs. The CLI creates
  one profile per server over time; this machine has only ever had one.
- `secret_service_default_alias__healthy/input/` — the same real per-collection captures,
  with `ReadAlias` answering `/org/freedesktop/secrets/collection/Default` (the collection
  that really does hold the Proton item) instead of the phantom. This is the state the
  Class C fixer is supposed to produce.
- `settings_valid{,__invalid}/input/config.toml` — hand-written; fproton's own config file
  has no capture to take (this machine has none, so every setting is at its default).
