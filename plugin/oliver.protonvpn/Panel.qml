import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Bar button plus keyboard-driven panel for Proton VPN. Everything shown here
// comes from `fproton status --json` (plus `countries` and `recents`), and every
// action is an argv array; the QML holds no VPN logic of its own (DESIGN §6).
Panel {
  id: root
  moduleName: "oliver.protonvpn"
  ipcTarget: "oliver.protonvpn"
  manageIpc: false

  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family

  // ---- bar icon state (DESIGN §6.5, first match wins) ----------------------
  readonly property bool anyWarning: {
    for (var i = 0; i < svc.warnings.length; i++) {
      var s = String(svc.warnings[i].severity || "")
      if (s === "warn" || s === "error") return true
    }
    return false
  }
  readonly property int iconCase: !svc.alive ? 0
    : (svc.state === "error" || svc.localError !== "") ? 1
    : anyWarning ? 2
    : (svc.state === "connecting" || svc.state === "disconnecting") ? 3
    : svc.on ? 4 : 5
  readonly property color barIconColor: iconCase === 1 ? urgent
    : (iconCase === 0 || iconCase === 5) ? Qt.darker(barForeground, 1.55) : barForeground
  readonly property bool iconCrossed: iconCase === 0 || iconCase === 5
  readonly property bool iconWarning: iconCase <= 2
  readonly property bool iconPulse: iconCase === 3

  // ---- panel state ---------------------------------------------------------
  property bool cursorActive: false
  property string focusSection: "header"
  property int selectedIndex: 0
  property bool countryFilterOpen: false
  property string countryQuery: ""

  readonly property var quickTargets: ["fastest", "p2p", "secure-core", "tor", "random"]
  readonly property bool protectionsKnown: svc.config !== null && svc.killSwitch !== null && svc.killSwitch.supported === true
  readonly property bool portKnown: svc.portForwarding !== null && svc.portForwarding.port !== null && svc.portForwarding.port !== undefined
  // DESIGN §6.6 asks for this row when `elapsed_sec >= deadline_sec`, but §5 has
  // `status` retire a pending the moment it passes its deadline, so that exact
  // condition is only ever true for the instant between the two — the row would
  // never be seen. It also fires here once a live action has been waiting 15 s,
  // which is the case the row exists for: a connect with 75 s still to run and
  // no other way out.
  readonly property int hungAfterSec: 15
  readonly property bool hungAction: svc.pending !== null
    && Number(svc.pending.elapsed_sec) >= Math.min(Number(svc.pending.deadline_sec), hungAfterSec)
  readonly property bool signInVisible: svc.signedIn !== true

  // 149 countries is more than a panel should draw, so the list is alphabetical
  // (where a name can be predicted) and capped; `/` narrows it.
  //
  // Four while the filter is closed, eight once the user is actually looking for a
  // country: on first open the eight rows pushed Protections and the whole footer
  // (Open TUI · Doctor · Sign in/Sign out) below the fold of a panel capped at
  // Style.space(560), with the QQC2 default scrollbar — transparent until you
  // scroll — giving no hint that anything was down there.
  readonly property int countryLimit: countryFilterOpen ? 8 : 4
  readonly property var countriesSorted: {
    var list = []
    for (var i = 0; i < svc.countries.length; i++) {
      var r = svc.countries[i]
      if (r && Model.isCountry(String(r.country))) list.push(r)
    }
    list.sort(function(a, b) {
      var an = String(a.country_name || a.country), bn = String(b.country_name || b.country)
      return an < bn ? -1 : (an > bn ? 1 : 0)
    })
    return list
  }
  readonly property var countryMatches: {
    var q = String(countryQuery).trim().toLowerCase()
    if (q === "") return countriesSorted
    var out = []
    for (var i = 0; i < countriesSorted.length; i++) {
      var r = countriesSorted[i]
      var hay = (String(r.country) + " " + String(r.country_name || "")).toLowerCase()
      if (hay.indexOf(q) >= 0) out.push(r)
    }
    return out
  }
  readonly property var countryRows: countryMatches.slice(0, countryLimit)

  // ---- cursor --------------------------------------------------------------
  readonly property var navSections: {
    var s = ["header"]
    if (hungAction) s.push("hung")
    if (svc.warnings.length > 0) s.push("warnings")
    s.push("quick")
    if (svc.recents.length > 0) s.push("recents")
    if (countryRows.length > 0) s.push("countries")
    s.push("protections")
    s.push("footer")
    return s
  }

  function sectionCount(name) {
    if (name === "warnings") return svc.warnings.length
    if (name === "quick") return quickTargets.length
    if (name === "recents") return svc.recents.length
    if (name === "countries") return countryRows.length
    if (name === "protections") return protectionsKnown ? (portKnown ? 4 : 3) : 2
    if (name === "footer") return 3
    return 1
  }
  function isHorizontal(name) {
    return name === "quick" || name === "footer" || (name === "protections" && !protectionsKnown)
  }

  function ensureCursor() {
    if (navSections.indexOf(focusSection) < 0) { focusSection = "header"; selectedIndex = 0 }
    var n = sectionCount(focusSection)
    if (selectedIndex >= n) selectedIndex = Math.max(0, n - 1)
    if (selectedIndex < 0) selectedIndex = 0
  }

  function gotoSection(position, direction) {
    if (position < 0 || position >= navSections.length) return
    focusSection = navSections[position]
    var n = sectionCount(focusSection)
    selectedIndex = direction > 0 || isHorizontal(focusSection) ? 0 : Math.max(0, n - 1)
    scrollCursorIntoView()
  }

  function moveCursor(dx, dy) {
    cursorActive = true
    ensureCursor()
    var here = navSections.indexOf(focusSection)
    if (here < 0) return
    if (dy !== 0) {
      if (!isHorizontal(focusSection)) {
        var next = selectedIndex + dy
        if (next >= 0 && next < sectionCount(focusSection)) {
          selectedIndex = next
          scrollCursorIntoView()
          return
        }
      }
      gotoSection(here + dy, dy)
      return
    }
    if (dx !== 0) {
      if (isHorizontal(focusSection)) {
        selectedIndex = Math.max(0, Math.min(sectionCount(focusSection) - 1, selectedIndex + dx))
        scrollCursorIntoView()
      } else if (focusSection === "protections" && protectionsKnown) {
        cycleProtection(selectedIndex, dx)
      }
    }
  }

  function setCursor(section, index) {
    cursorActive = true
    focusSection = section
    selectedIndex = index
    ensureCursor()
  }

  function activateCursor() {
    ensureCursor()
    if (focusSection === "header") svc.toggle()
    else if (focusSection === "hung") svc.forceDisconnect()
    else if (focusSection === "warnings") activateWarning(selectedIndex)
    else if (focusSection === "quick") svc.connectTo(quickTargets[selectedIndex])
    else if (focusSection === "recents") svc.connectTo(svc.recents[selectedIndex])
    else if (focusSection === "countries") connectCountry(countryRows[selectedIndex])
    else if (focusSection === "protections") activateProtection(selectedIndex)
    else if (focusSection === "footer") activateFooter(selectedIndex)
  }

  function connectCountry(row) {
    if (!row) return
    svc.connectTo(String(row.country))
  }

  function activateWarning(index) {
    var w = svc.warnings[index]
    if (!w) return
    var hint = String(w.hint || "")
    if (hint === "") return
    if (hint.indexOf("fproton doctor") === 0) { svc.openDoctor(); root.close(); return }
    Quickshell.execDetached(["wl-copy", "--", hint])
    svc.flash("Copied: " + hint)
  }

  readonly property var netshieldValues: ["off", "malware-only", "malware-ads-trackers"]

  function protectionValue(key) {
    var c = svc.config
    if (!c) return ""
    var v = c[key]
    return v === null || v === undefined ? "" : String(v)
  }

  function cycleProtection(index, direction) {
    if (index === 0) toggleProtection("kill-switch", ["off", "standard"], direction)
    else if (index === 1) toggleProtection("netshield", netshieldValues, direction)
    else if (index === 2) toggleProtection("port-forwarding", ["off", "on"], direction)
  }

  function toggleProtection(key, values, direction) {
    var current = values.indexOf(protectionValue(key))
    if (current < 0) return                       // a value we cannot map ("Upgrade to enable")
    var step = direction === 0 ? 1 : direction
    var next = (current + step + values.length) % values.length
    svc.setConfig(key, values[next])
  }

  function activateProtection(index) {
    if (!protectionsKnown) {
      if (index === 0) svc.refreshConfig()
      else { svc.openDoctor(); root.close() }
      return
    }
    if (index === 3) svc.copyPort()
    else cycleProtection(index, 0)
  }

  function activateFooter(index) {
    if (index === 0) { svc.openTui(); root.close() }
    else if (index === 1) { svc.openDoctor(); root.close() }
    else if (signInVisible) { svc.signIn(); root.close() }
    else svc.signOut()
  }

  function openCountryFilter() {
    countryFilterOpen = true
    cursorActive = true
    focusSection = "countries"
    selectedIndex = 0
    Qt.callLater(function() { if (countryFilter) countryFilter.forceActiveFocus() })
  }

  function closeCountryFilter() {
    countryFilterOpen = false
    countryQuery = ""
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  function moveCountryCursor(delta) {
    if (countryRows.length === 0) return
    cursorActive = true
    focusSection = "countries"
    selectedIndex = Math.max(0, Math.min(countryRows.length - 1, selectedIndex + delta))
    scrollCursorIntoView()
  }

  // ---- scrolling -----------------------------------------------------------
  function sectionColumn(name) {
    if (name === "warnings") return warningColumn
    if (name === "recents") return recentColumn
    if (name === "countries") return countryColumn
    return null
  }
  function sectionAnchor(name) {
    if (name === "header") return heroBlock
    if (name === "hung") return hungRow
    if (name === "quick") return quickBlock
    if (name === "protections") return protectionBlock
    if (name === "footer") return footerBlock
    return null
  }
  function scrollCursorIntoView() {
    var col = sectionColumn(focusSection)
    var item = col && selectedIndex >= 0 && selectedIndex < col.children.length
      ? col.children[selectedIndex] : sectionAnchor(focusSection)
    scrollItemIntoView(item)
  }
  function scrollItemIntoView(item) {
    if (!panelFlick || !item) return
    Qt.callLater(function() {
      if (!item) return
      var margin = Style.space(6)
      var point = item.mapToItem(panelFlick.contentItem, 0, 0)
      var top = point.y
      var bottom = top + item.height
      var viewTop = panelFlick.contentY
      var viewBottom = viewTop + panelFlick.height
      var maxY = Math.max(0, panelFlick.contentHeight - panelFlick.height)
      if (top < viewTop + margin) panelFlick.contentY = Math.max(0, top - margin)
      else if (bottom > viewBottom - margin) panelFlick.contentY = Math.min(maxY, bottom + margin - panelFlick.height)
    })
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  onOpenedChanged: {
    svc.opened = opened
    if (!opened) { countryFilterOpen = false; countryQuery = ""; return }
    cursorActive = false
    focusSection = "header"
    selectedIndex = 0
    if (panelFlick) panelFlick.contentY = 0
    svc.refresh()
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }
  onNavSectionsChanged: ensureCursor()
  onCountryRowsChanged: ensureCursor()

  Service {
    id: svc
    settings: root.settings
  }

  IpcHandler {
    target: root.ipcTarget
    function open(): void { root.open() }
    function close(): void { root.close() }
    function show(): void { root.open() }
    function hide(): void { root.close() }
    function toggle(): void { root.toggle() }
    function refresh(): string { svc.refresh(); return "ok" }
    function connect(): string { svc.connectDefault(); return "ok" }
    function disconnect(): string { svc.disconnect(); return "ok" }
    function reconnect(): string { svc.reconnect(false); return "ok" }
    function nextServer(): string { svc.reconnect(true); return "ok" }
    // Test hook: where the keyboard cursor is, without a screenshot.
    function cursor(): string {
      return JSON.stringify({ section: root.focusSection, index: root.selectedIndex,
                              active: root.cursorActive, opened: root.opened,
                              sections: root.navSections, filter: root.countryFilterOpen,
                              alive: svc.alive, binaryMissing: svc.binaryMissing, state: svc.state })
    }
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    tooltipText: Model.statePhrase(svc.status) + (svc.server && svc.server.name ? " · " + svc.server.name : "")
    iconComponent: Component {
      Item {
        ProtonIcon {
          id: barIcon
          property real pulse: 1.0
          anchors.centerIn: parent
          iconSize: Style.space(12)
          color: root.barIconColor
          badgeColor: root.urgent
          crossed: root.iconCrossed
          warning: root.iconWarning
          opacity: root.iconPulse ? pulse : 1.0

          SequentialAnimation on pulse {
            running: root.iconPulse
            loops: Animation.Infinite
            NumberAnimation { from: 1.0; to: 0.45; duration: 900; easing.type: Easing.InOutQuad }
            NumberAnimation { from: 0.45; to: 1.0; duration: 900; easing.type: Easing.InOutQuad }
          }
        }
      }
    }
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.RightButton) svc.toggle()
      else if (buttonCode === Qt.MiddleButton) svc.refresh()
      else root.toggle()
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(380))
    contentHeight: panel.fittedContentHeight(column.implicitHeight, Style.space(560))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      // While the filter is open every key belongs to the TextField, which then
      // has to handle Escape itself: closeRequested does not fire when blocked.
      blocked: root.countryFilterOpen
      onMoveRequested: function(dx, dy) {
        if (!root.cursorActive) { root.cursorActive = true; return }
        root.moveCursor(dx, dy)
      }
      onActivateRequested: if (root.cursorActive) root.activateCursor()
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }
      onTextKey: function(t) {
        var k = String(t).toLowerCase()
        if (k === "/") root.openCountryFilter()
        else if (k === "c") svc.connectDefault()
        else if (k === "d") svc.disconnect()
        else if (k === "f") svc.forceDisconnect()
        else if (k === "r") { svc.refresh(); svc.loadLists() }
        else if (k === "t") { svc.openTui(); root.close() }
      }

      Flickable {
        id: panelFlick
        anchors.fill: parent
        contentWidth: width
        contentHeight: column.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick
        interactive: contentHeight > height
        // AsNeeded is the QQC2 default: transparent until the user scrolls, which is
        // no affordance at all for content they do not know exists. Shown whenever
        // there is something below the fold (the same idiom as Ui/…/audio/Panel.qml).
        ScrollBar.vertical: ScrollBar {
          policy: panelFlick.contentHeight > panelFlick.height
                  ? ScrollBar.AlwaysOn : ScrollBar.AlwaysOff
        }

        Column {
          id: column
          width: panelFlick.width
          spacing: Style.space(10)

          // ---- header ------------------------------------------------------
          Item {
            id: heroBlock
            width: parent.width
            implicitHeight: hero.implicitHeight
            // The hero's trailingControl resolves `root` to PanelHero, so panel
            // state is reached through this wrapper instead.
            readonly property bool ringVisible: root.cursorActive && root.focusSection === "header"
            function focusHero() { root.setCursor("header", 0) }

            PanelHero {
              id: hero
              width: parent.width
              title: Model.serverTitle(svc.status)
              meta: Model.statePhrase(svc.status)
              detail: svc.server && svc.server.country ? Model.flag(svc.server.country) + " " + String(svc.server.country) : ""
              foreground: root.foreground
              fontFamily: root.fontFamily
              iconOpacity: svc.on ? 1.0 : 0.5
              iconComponent: Component {
                ProtonIcon {
                  iconSize: Style.font.display
                  color: svc.on ? root.foreground : root.dim
                  badgeColor: root.urgent
                  crossed: !svc.on && svc.alive
                  warning: !svc.alive || svc.state === "error"
                }
              }
              trailingControl: Component {
                ToggleSwitch {
                  id: powerSwitch
                  checked: svc.on
                  busy: svc.busy
                  hasCursor: heroBlock.ringVisible
                  foreground: hero.foreground
                  onHovered: function(on) { if (on) heroBlock.focusHero() }
                  onToggled: svc.toggle()

                  PanelToolTip {
                    visible: powerSwitch.containsMouse
                    text: svc.on ? "Disconnect (d)" : "Connect to " + Model.targetLabel(svc.defaultTarget) + " (c)"
                    fontFamily: hero.fontFamily
                  }
                }
              }
            }
          }

          // ---- detail rows (not cursor targets) ----------------------------
          Column {
            width: parent.width
            spacing: Style.space(3)
            visible: svc.alive && !svc.binaryMissing

            DetailRow {
              label: "Location"
              value: svc.server ? Model.countryLabel(svc.server) + (svc.server.city ? " · " + svc.server.city : "") : "no server yet"
            }
            DetailRow {
              label: "Server"
              value: svc.server && typeof svc.server.load === "number"
                ? "load " + svc.server.load + "%" + (Model.featureLabels(svc.server.features) !== "" ? " · " + Model.featureLabels(svc.server.features) : "")
                : "—"
            }
            DetailRow {
              label: "Uptime"
              value: svc.link && svc.link.up === true ? Model.uptime(svc.link.uptime_sec) : "—"
            }
            DetailRow { label: "Traffic"; value: Model.trafficLine(svc.status) }
          }

          Text {
            textFormat: Text.PlainText
            visible: text !== ""
            width: parent.width
            // Whatever the panel most recently learned: the action it is running,
            // the error that action returned, or the error Status still carries.
            text: svc.actionStatus !== "" ? svc.actionStatus : (svc.localError !== "" ? svc.localError : svc.lastError)
            color: svc.actionStatus === "" && (svc.localError !== "" || svc.lastError !== "") ? root.urgent : root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.bodySmall
            wrapMode: Text.WordWrap
            maximumLineCount: 3
            elide: Text.ElideRight
          }

          Text {
            textFormat: Text.PlainText
            visible: svc.binaryMissing
            width: parent.width
            text: "fproton is not installed or not on PATH (~/.local/bin)."
            color: root.urgent
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            wrapMode: Text.WordWrap
          }

          // ---- a connect that ran past its deadline -------------------------
          NavRow {
            id: hungRow
            section: "hung"
            idx: 0
            visible: root.hungAction
            width: parent.width

            Text {
              textFormat: Text.PlainText
              anchors.left: parent.left
              anchors.right: parent.right
              anchors.verticalCenter: parent.verticalCenter
              anchors.leftMargin: Style.space(10)
              anchors.rightMargin: Style.space(10)
              text: "Taking long… force disconnect"
              color: root.urgent
              font.family: root.fontFamily
              font.pixelSize: Style.font.body
              elide: Text.ElideRight
            }
          }

          // ---- warnings -----------------------------------------------------
          Column {
            width: parent.width
            spacing: Style.space(6)
            visible: svc.warnings.length > 0

            PanelSeparator { foreground: root.foreground }

            Column {
              id: warningColumn
              width: parent.width
              spacing: Style.space(4)

              Repeater {
                model: svc.warnings
                WarnRow {
                  required property var modelData
                  required property int index
                  section: "warnings"
                  idx: index
                  width: warningColumn.width
                  message: String(modelData.text || "")
                  hint: String(modelData.hint || "")
                  severity: String(modelData.severity || "warn")
                }
              }
            }
          }

          // ---- quick connect -------------------------------------------------
          Column {
            id: quickBlock
            width: parent.width
            spacing: Style.space(6)

            PanelSeparator { foreground: root.foreground }
            PanelSectionHeader { text: "QUICK CONNECT"; foreground: root.foreground; fontFamily: root.fontFamily }

            Flow {
              width: parent.width
              spacing: Style.space(6)

              Repeater {
                model: root.quickTargets
                Chip {
                  required property var modelData
                  required property int index
                  section: "quick"
                  idx: index
                  label: Model.targetLabel(String(modelData))
                }
              }
            }
          }

          // ---- recents --------------------------------------------------------
          Column {
            width: parent.width
            spacing: Style.space(6)
            visible: svc.recents.length > 0

            PanelSeparator { foreground: root.foreground }
            PanelSectionHeader { text: "RECENT"; foreground: root.foreground; fontFamily: root.fontFamily }

            Column {
              id: recentColumn
              width: parent.width
              spacing: Style.space(4)

              Repeater {
                model: svc.recents
                ListRow {
                  required property var modelData
                  required property int index
                  section: "recents"
                  idx: index
                  width: recentColumn.width
                  title: Model.targetLabel(String(modelData))
                  meta: "connect"
                }
              }
            }
          }

          // ---- countries ------------------------------------------------------
          Column {
            width: parent.width
            spacing: Style.space(6)

            PanelSeparator { foreground: root.foreground }

            Row {
              width: parent.width
              spacing: Style.space(6)

              PanelSectionHeader {
                text: "COUNTRIES"
                foreground: root.foreground
                fontFamily: root.fontFamily
              }
              Text {
                textFormat: Text.PlainText
                text: root.countryFilterOpen ? "Esc closes the filter" : "/ to filter"
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.caption
              }
            }

            TextField {
              id: countryFilter
              visible: root.countryFilterOpen
              width: parent.width
              foreground: root.foreground
              placeholderText: "Filter countries"
              text: root.countryQuery
              onTextChanged: {
                root.countryQuery = text
                root.selectedIndex = 0
              }
              onAccepted: root.connectCountry(root.countryRows[root.selectedIndex])
              Keys.onPressed: function(event) {
                if (event.key === Qt.Key_Down) { root.moveCountryCursor(1); event.accepted = true }
                else if (event.key === Qt.Key_Up) { root.moveCountryCursor(-1); event.accepted = true }
                else if (event.key === Qt.Key_Escape) { root.closeCountryFilter(); event.accepted = true }
              }
            }

            Text {
              textFormat: Text.PlainText
              visible: root.countryRows.length === 0
              width: parent.width
              text: svc.countries.length === 0 ? "No country list yet — press r to refresh." : "No country matches that filter."
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.bodySmall
              horizontalAlignment: Text.AlignHCenter
            }

            Column {
              id: countryColumn
              width: parent.width
              spacing: Style.space(4)

              Repeater {
                model: root.countryRows
                ListRow {
                  required property var modelData
                  required property int index
                  section: "countries"
                  idx: index
                  width: countryColumn.width
                  title: Model.countryLabel(modelData)
                  meta: Model.countryMeta(modelData)
                }
              }
            }

            Text {
              textFormat: Text.PlainText
              visible: root.countryMatches.length > root.countryRows.length
              width: parent.width
              text: (root.countryMatches.length - root.countryRows.length) + " more — press / to filter"
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              horizontalAlignment: Text.AlignHCenter
            }
          }

          // ---- protections ----------------------------------------------------
          Column {
            id: protectionBlock
            width: parent.width
            spacing: Style.space(6)

            PanelSeparator { foreground: root.foreground }
            PanelSectionHeader { text: "PROTECTIONS"; foreground: root.foreground; fontFamily: root.fontFamily }

            Column {
              width: parent.width
              spacing: Style.space(4)
              visible: root.protectionsKnown

              ListRow {
                section: "protections"; idx: 0; width: parent.width
                title: "Kill switch"; meta: root.protectionValue("kill-switch")
              }
              ListRow {
                section: "protections"; idx: 1; width: parent.width
                title: "NetShield"; meta: root.protectionValue("netshield")
              }
              ListRow {
                section: "protections"; idx: 2; width: parent.width
                title: "Port forwarding"; meta: root.protectionValue("port-forwarding")
              }
              ListRow {
                visible: root.portKnown
                section: "protections"; idx: 3; width: parent.width
                title: "Forwarded port " + (root.portKnown ? String(svc.portForwarding.port) : "")
                meta: "copy"
              }
            }

            Column {
              width: parent.width
              spacing: Style.space(6)
              visible: !root.protectionsKnown

              Text {
                textFormat: Text.PlainText
                width: parent.width
                text: "Protections unknown — Proton CLI not responding"
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.bodySmall
                wrapMode: Text.WordWrap
              }

              Row {
                width: parent.width
                spacing: Style.space(6)
                Chip { section: "protections"; idx: 0; label: "Refresh" }
                Chip { section: "protections"; idx: 1; label: "Run doctor" }
              }
            }
          }

          // ---- footer ----------------------------------------------------------
          Column {
            id: footerBlock
            width: parent.width
            spacing: Style.space(6)

            PanelSeparator { foreground: root.foreground }

            Row {
              width: parent.width
              spacing: Style.space(6)
              Chip { section: "footer"; idx: 0; label: "Open TUI" }
              Chip { section: "footer"; idx: 1; label: "Doctor" }
              Chip { section: "footer"; idx: 2; label: root.signInVisible ? "Sign in" : "Sign out" }
            }
          }
        }
      }
    }
  }

  // ---- reusable pieces -------------------------------------------------------
  component DetailRow: Row {
    property string label: ""
    property string value: ""
    width: parent ? parent.width : 0
    spacing: Style.space(8)

    Text {
      textFormat: Text.PlainText
      width: Style.space(70)
      text: label
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.bodySmall
    }
    Text {
      textFormat: Text.PlainText
      width: parent.width - Style.space(78)
      text: value
      color: root.foreground
      font.family: root.fontFamily
      font.pixelSize: Style.font.bodySmall
      elide: Text.ElideRight
    }
  }

  component NavRow: CursorSurface {
    id: navRow
    property string section: ""
    property int idx: 0

    hasCursor: root.cursorActive && root.focusSection === section && root.selectedIndex === idx
    foreground: root.foreground
    implicitHeight: Style.spacing.popupRowHeight

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onEntered: root.setCursor(navRow.section, navRow.idx)
      onClicked: { root.setCursor(navRow.section, navRow.idx); root.activateCursor() }
    }
  }

  component ListRow: NavRow {
    id: listRow
    property string title: ""
    property string meta: ""

    implicitHeight: Math.max(Style.spacing.popupRowHeight, rowText.implicitHeight + Style.spacing.rowPaddingX)

    RowLayout {
      id: rowText
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(10)
      spacing: Style.space(8)

      Text {
        textFormat: Text.PlainText
        Layout.fillWidth: true
        text: listRow.title
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        elide: Text.ElideRight
      }
      Text {
        textFormat: Text.PlainText
        text: listRow.meta
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
        elide: Text.ElideRight
      }
    }
  }

  component WarnRow: NavRow {
    id: warnRow
    property string message: ""
    property string hint: ""
    property string severity: "warn"

    implicitHeight: warnText.implicitHeight + Style.spacing.rowPaddingX

    Column {
      id: warnText
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(10)
      spacing: Style.space(2)

      Text {
        textFormat: Text.PlainText
        width: parent.width
        text: warnRow.message
        color: warnRow.severity === "info" ? root.foreground : root.urgent
        font.family: root.fontFamily
        font.pixelSize: Style.font.bodySmall
        wrapMode: Text.WordWrap
      }
      Text {
        textFormat: Text.PlainText
        visible: warnRow.hint !== ""
        width: parent.width
        text: warnRow.hint
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
        elide: Text.ElideRight
      }
    }
  }

  component Chip: NavRow {
    id: chip
    property string label: ""

    implicitWidth: chipText.implicitWidth + Style.space(20)
    implicitHeight: Style.spacing.controlHeight
    bordered: true

    Text {
      id: chipText
      anchors.centerIn: parent
      textFormat: Text.PlainText
      text: chip.label
      color: root.foreground
      font.family: root.fontFamily
      font.pixelSize: Style.font.bodySmall
    }
  }
}
