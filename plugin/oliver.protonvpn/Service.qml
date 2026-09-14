import QtQuick
import Quickshell
import Quickshell.Io
import "Model.js" as Model

// The widget's whole state model: one `fproton status --json` poll, one
// serialized action queue, and three lazy list calls. Nothing is computed about
// the VPN here — Status is the single source of truth, and every action is an
// argv array handed to fproton. No shell strings, ever (DESIGN §2, §6.4).
Item {
  id: root

  property var settings: ({})
  property bool opened: false

  property var status: ({ ok: false, state: "disconnected" })
  readonly property bool alive: status.ok === true
  readonly property string state: String(status.state || "disconnected")
  readonly property var link: status.link || null
  readonly property var server: status.server || null
  readonly property var traffic: status.traffic || null
  readonly property var config: status.config || null          // null ⇒ protections unknown
  readonly property var pending: status.pending || null
  readonly property var warnings: Array.isArray(status.warnings) ? status.warnings : []
  readonly property string lastError: String((status.last_error && status.last_error.text) || "")
  readonly property var killSwitch: status.kill_switch || null
  readonly property var portForwarding: status.port_forwarding || null
  // true / false / null (unknown): the panel tests `!== true`.
  readonly property var signedIn: status.signed_in === undefined ? null : status.signed_in

  property string actionStatus: ""   // transient label, cleared 2.2 s after an action exits
  property string localError: ""     // stderr (or JSON error) of the last failed action
  property int _desired: -1          // optimism bridge: -1 follow, 0 want down, 1 want up
  readonly property bool on: _desired === -1 ? (state === "connected" || state === "connecting") : _desired === 1
  readonly property bool busy: actionProc.running || pending !== null

  // Lists Status does not carry, fetched lazily on first open.
  property var countries: []
  property var recents: []
  property bool listsRequested: false

  property bool binaryChecked: false
  property bool binaryMissing: false

  readonly property string bin: String(setting("binary", "fproton"))
  readonly property int refreshMs: 1000 * intSetting("refreshIntervalSec", 3, 1, 60)
  readonly property int idleRefreshMs: 1000 * intSetting("idleRefreshIntervalSec", 8, 2, 600)
  readonly property string defaultTarget: {
    var t = String(setting("defaultTarget", "fastest"))
    return Model.isTarget(t) ? t : "fastest"
  }
  readonly property bool notifications: String(setting("notifications", "off")) === "on"
  readonly property string tuiCommand: String(setting("tuiCommand", "omarchy-launch-or-focus-tui --app-id=fproton fproton tui"))

  // Manifest `barWidget.defaults` are never merged into `settings` (DESIGN §1.5):
  // enabling the widget writes only `{"id":"oliver.protonvpn"}`, so every default
  // above repeats its manifest value verbatim.
  function setting(name, fallback) {
    var v = settings ? settings[name] : undefined
    return v === undefined || v === null ? fallback : v
  }
  function intSetting(name, fallback, min, max) {
    var n = parseInt(String(setting(name, fallback)), 10)
    if (!isFinite(n)) n = fallback
    return Math.max(min, Math.min(max, n))
  }

  function refresh() {
    // A missing binary pauses polling until the panel is opened again (§6.4);
    // re-running `which` on every tick would be a spawn per tick forever.
    if (binaryMissing) return
    if (!binaryChecked) { checkBinary(); return }
    if (statusProc.running) return
    statusProc.running = true
    if (!pollWatchdog.running) pollWatchdog.start()
  }

  function checkBinary() {
    if (whichProc.running) return
    whichProc.command = ["which", root.bin]
    whichProc.running = true
  }

  function loadLists() {
    listsRequested = true
    if (!countriesProc.running) countriesProc.running = true
    loadRecents()
  }
  function loadRecents() {
    if (!recentsProc.running) recentsProc.running = true
  }

  // ---- actions -------------------------------------------------------------
  // One at a time, in order, each followed by a 600 ms refresh. `label` is the
  // transient line under the hero; an empty label shows nothing.
  property var _queue: []
  function runAction(args, label) {
    if (binaryMissing) return
    _queue.push({ args: args, label: String(label || "") })
    pump()
  }
  function pump() {
    if (actionProc.running || _queue.length === 0) return
    var next = _queue.shift()
    actionStatus = next.label
    actionStatusTimer.stop()
    localError = ""
    actionProc.command = [root.bin].concat(next.args)
    actionProc.running = true
  }

  function connectTo(target) {
    var t = String(target || "")
    if (!Model.isTarget(t)) return           // never put an unvalidated value in an argv
    _desired = 1
    desiredTimer.restart()
    runAction(["connect", t, "--json"], "Connecting to " + Model.targetLabel(t) + "…")
  }
  function connectDefault() { connectTo(root.defaultTarget) }
  function disconnect() {
    _desired = 0
    desiredTimer.restart()
    runAction(["disconnect", "--json"], "Disconnecting…")
  }
  function forceDisconnect() {
    _desired = 0
    desiredTimer.restart()
    runAction(["disconnect", "--force", "--yes", "--json"], "Forcing the tunnel down…")
  }
  function reconnect(next) {
    _desired = 1
    desiredTimer.restart()
    var args = next === true ? ["reconnect", "--next", "--json"] : ["reconnect", "--json"]
    runAction(args, next === true ? "Switching server…" : "Reconnecting…")
  }
  function toggle() { on ? disconnect() : connectDefault() }

  function setConfig(key, value) {
    var allowed = {
      "kill-switch": ["off", "standard"],
      "netshield": ["off", "malware-only", "malware-ads-trackers"],
      "port-forwarding": ["off", "on"]
    }
    var values = allowed[String(key)]
    if (!values || values.indexOf(String(value)) < 0) return
    runAction(["config", "set", String(key), String(value), "--json"], "Setting " + key + " to " + value + "…")
  }
  function refreshConfig() { runAction(["config", "list", "--json"], "Reading Proton's settings…") }
  function signOut() { runAction(["signout", "--json"], "Signing out…") }

  // ---- detached launches ---------------------------------------------------
  // Only constant literals reach omarchy-launch-*: those scripts end in `eval`
  // or `bash -c` on a joined string (DESIGN §1.5). `wl-copy` and
  // omarchy-notification-send are genuinely argv-shaped.
  function copyPort() {
    var p = portForwarding && portForwarding.port !== null ? portForwarding.port : null
    if (p === null || p === undefined) return
    var n = parseInt(String(p), 10)
    if (!isFinite(n) || n < 1 || n > 65535) return
    Quickshell.execDetached(["wl-copy", "--", String(n)])
    actionStatus = "Copied port " + n
    actionStatusTimer.restart()
  }
  function openTui() {
    var argv = Model.safeArgv(root.tuiCommand)
    if (argv.length === 0) argv = ["omarchy-launch-or-focus-tui", "--app-id=fproton", "fproton", "tui"]
    Quickshell.execDetached(argv)
  }
  function openDoctor() {
    Quickshell.execDetached(["omarchy-launch-or-focus-tui", "--app-id=fprotondoctor", "fproton", "doctor"])
  }
  function signIn() {
    Quickshell.execDetached(["omarchy-launch-floating-terminal-with-presentation", "fproton signin"])
  }

  // ---- status --------------------------------------------------------------
  property string _lastNotifiedState: ""
  property bool _seenStatus: false
  function applyStatus(text, exitCode, stderrText) {
    var parsed = Model.parseStatus(text)
    var previous = root.state
    if (parsed.ok === true) {
      root.status = parsed
      root.localError = ""
    } else {
      // Keep nothing but the shape: a panel that cannot reach fproton must not
      // go on showing a connected tunnel. The last server row is preserved for
      // display so the panel does not blank out mid-poll.
      root.status = { ok: false, state: "disconnected", server: root.server, link: root.link }
      var msg = String(stderrText || "").trim()
      root.localError = msg !== "" ? msg.split("\n")[0] : "fproton status failed (exit " + exitCode + ")"
    }
    if (root._desired !== -1) {
      var agrees = (root._desired === 1) === (root.state === "connected" || root.state === "connecting")
      if (root.pending !== null || agrees) { root._desired = -1; desiredTimer.stop() }
    }
    if (root._seenStatus && root.state !== previous) notifyTransition(previous, root.state)
    root._seenStatus = true
  }

  function notifyTransition(from, to) {
    if (!notifications) { _lastNotifiedState = to; return }
    var interesting = ["connected", "disconnected", "error"]
    if (interesting.indexOf(to) < 0) return
    if (_lastNotifiedState === to) return
    _lastNotifiedState = to
    var text = to === "connected"
      ? "Connected to " + Model.serverTitle(root.status)
      : (to === "error" ? (root.lastError !== "" ? root.lastError : "Proton VPN error") : "Disconnected")
    Quickshell.execDetached(["omarchy-notification-send", "-u", "low", "-g", "󰦝", "Proton VPN", text])
  }

  Process {
    id: whichProc
    running: false
    command: []
    stdout: StdioCollector {}
    stderr: StdioCollector {}
    onExited: function(exitCode) {
      // The name may have changed while this check was in flight (shell.json
      // hot-reloads); an answer about the old name says nothing about the new.
      if (whichProc.command.length > 1 && String(whichProc.command[1]) !== root.bin) {
        root.checkBinary()
        return
      }
      root.binaryChecked = true
      root.binaryMissing = exitCode !== 0
      if (!root.binaryMissing) root.refresh()
      else root.status = { ok: false, state: "disconnected" }
    }
  }

  Process {
    id: statusProc
    running: false
    command: [root.bin, "status", "--json"]
    stdout: StdioCollector { id: statusOut; waitForEnd: true }
    stderr: StdioCollector { id: statusErr; waitForEnd: true }
    onExited: function(exitCode) {
      pollWatchdog.stop()
      root.applyStatus(String(statusOut.text || ""), exitCode, String(statusErr.text || ""))
    }
  }

  Process {
    id: actionProc
    running: false
    command: []
    stdout: StdioCollector { id: actionOut; waitForEnd: true }
    stderr: StdioCollector { id: actionErr; waitForEnd: true }
    onExited: function(exitCode) {
      if (exitCode !== 0) {
        root._desired = -1
        var doc = Model.parseJson(String(actionOut.text || ""))
        var msg = doc && doc.error ? String(doc.error) : String(actionErr.text || "").trim().split("\n")[0]
        if (doc && doc.hint) msg += " — try: " + doc.hint
        root.localError = msg !== "" ? msg : "fproton exited " + exitCode
        root.actionStatus = root.localError
      } else {
        root.localError = ""
        root.actionStatus = ""
      }
      actionStatusTimer.restart()
      delayedRefresh.restart()
      root.pump()
    }
  }

  Process {
    id: countriesProc
    running: false
    command: [root.bin, "countries", "--json"]
    stdout: StdioCollector { id: countriesOut; waitForEnd: true }
    stderr: StdioCollector {}
    onExited: function(exitCode) {
      var doc = exitCode === 0 ? Model.parseJson(String(countriesOut.text || "")) : null
      root.countries = doc && Array.isArray(doc.countries) ? doc.countries : []
    }
  }

  Process {
    id: recentsProc
    running: false
    command: [root.bin, "recents", "--json"]
    stdout: StdioCollector { id: recentsOut; waitForEnd: true }
    stderr: StdioCollector {}
    onExited: function(exitCode) {
      var doc = exitCode === 0 ? Model.parseJson(String(recentsOut.text || "")) : null
      var list = doc && Array.isArray(doc.recents) ? doc.recents : []
      var keep = []
      for (var i = 0; i < list.length; i++) {
        var t = String(list[i])
        if (Model.isTarget(t)) keep.push(t)   // anything else is dropped, not sent
      }
      root.recents = keep
    }
  }

  Timer {
    id: pollTimer
    interval: root.opened ? root.refreshMs : root.idleRefreshMs
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refresh()
  }

  Timer {
    // Every tick is skipped while the poll is still running, so a status call
    // that never exits would stop the panel refreshing for good. Armed on the
    // launch that needs watching and never restarted while armed (the Tailscale
    // service documents why at Service.qml:176).
    id: pollWatchdog
    interval: 8000
    repeat: false
    onTriggered: if (statusProc.running) statusProc.running = false
  }

  Timer {
    id: delayedRefresh
    interval: 600
    repeat: false
    onTriggered: { root.refresh(); root.loadRecents() }
  }

  Timer {
    id: actionStatusTimer
    interval: 2200
    repeat: false
    onTriggered: root.actionStatus = ""
  }

  Timer {
    // Last resort for the optimism bridge: fproton writes `pending` itself, so a
    // click normally reconciles within one poll.
    id: desiredTimer
    interval: 5000
    repeat: false
    onTriggered: root._desired = -1
  }

  // The binary name can change under us: shell.json hot-reloads, so someone
  // editing the setting in Setup > Plugins must not leave the panel polling a
  // name that no longer exists (or still showing the last answer of one).
  onBinChanged: {
    binaryChecked = false
    binaryMissing = false
    checkBinary()
  }

  onOpenedChanged: {
    if (!opened) return
    if (binaryMissing) checkBinary()
    else {
      refresh()
      if (!listsRequested) loadLists()
      else loadRecents()
    }
  }
}
