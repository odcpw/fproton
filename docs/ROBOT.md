fproton — agent handbook
========================

Proton VPN for Omarchy. One binary: CLI, doctor, TUI, and the backend of the
`oliver.protonvpn` bar widget.

BUILD STAGE
  Every command is implemented; they differ in how much has been verified.
  Verified end to end on this machine: status, servers, countries, cities, index,
  settings, history, recents, capabilities, robot-docs, version, tui, doctor,
  setup --dry-run, disconnect --force (a no-op while nothing is up) and port's
  no-tunnel error.
  Verified only on their timeout/error path, because the official Proton CLI hangs on
  this machine: connect, reconnect, disconnect, config list|get|set, account, signout.
  Never verified, and claimed nowhere: a successful connect or reconnect, a granted
  forwarded port, every `wg` subcommand, and the doctor's dns_when_connected and
  tunnel_route_ok checks, which skip while proton0 is down.

FIRST CALL
  fproton status --json      Everything about the current state, in one object.
  fproton capabilities       The machine contract: commands, codes, keys, files.

OUTPUT RULES
  * --json is a global flag, and a non-TTY stdout implies it. One object per call.
  * stdout is data. Progress and diagnostics go to stderr.
  * Errors in JSON mode are {"ok":false,"schema":1,"code":N,"code_name":"…",
    "error":"…","hint":"<runnable command>"} on stdout, with N as the exit code.
  * Every human-mode error ends with a `hint:` line naming a command you can run.
  * Colour is off when stdout is not a TTY, with --no-color, or when NO_COLOR or CI is set.

EXIT CODES
  0  ok              8  timeout          (a --wait ran out; nothing is broken)
  1  failure         9  not_signed_in    (run `fproton signin`)
  2  usage          10  index            (run `fproton index --rebuild`)
  3  safety         11  doctor           (the doctor found failures)
  4  backend        12  doctor_partial
  5  upstream       13  doctor_refused
  6  not_found     130  interrupted
  7  ambiguous

  5 is where the official Proton CLI lands when it errors *or when fproton kills it at
  its deadline*. That CLI hangs on this machine (a broken Secret Service default
  collection), so treat exit 5 from connect/config/account as "the CLI did not answer",
  not as "the VPN failed". `fproton doctor` explains it.

CONNECT TARGETS  (exactly one positional; no --country/--city/--feature flags)
  fastest | CC | CC#N | city:NAME | p2p | secure-core | tor | random
  e.g. fproton connect CH · fproton connect CH#461 · fproton connect "city:New York"
  Targets are validated before they reach an argv. Anything else is exit 2.

READING THE STATE
  `status.state` is one of connected, connecting, disconnecting, disconnected, error —
  a total function of the link, a live `pending` action and `last_error`. Every key of
  the document is always present; unknown is null, never an absent key and never 0.
  `status.warnings[]` carries {id, severity, text, hint} with stable ids.
  `status.pending` gives {action, target, started_at, deadline_sec, elapsed_sec, pid}:
  derive progress and "taking long" from those fields alone.

SAFETY
  Irreversible actions (`disconnect --force`, `wg swap`, `setup --disable-old-plugin`,
  `doctor --fix`) refuse without --yes and print the exact argv they would run; all of
  them accept --dry-run. The doctor never deletes anything.

WHAT FPROTON NEVER DOES
  It never reads keyring files, NetworkManager secrets or WireGuard keys; never runs a
  shell (argv only, everywhere); never contacts Proton's API; and never leaves a child
  process running past its deadline.

TYPICAL SESSION
  fproton status --json
  fproton servers --country CH --sort load --limit 10 --json
  fproton connect CH#461 --json
  fproton port --json                 # forwarded port, only while a tunnel is up
  fproton disconnect --json
  fproton doctor --json               # exit 11 when something needs attention
