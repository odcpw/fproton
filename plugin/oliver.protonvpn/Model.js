.pragma library

// Pure formatters and validators for the Proton VPN widget. No I/O, no state:
// everything here takes the last `fproton … --json` document and returns a
// string, a number or a boolean. The validators are the QML half of DESIGN §2
// "argv only": a value that does not match the grammar fproton accepts is
// dropped by the caller instead of being put in an argv.

function parseJson(raw) {
  try {
    var d = JSON.parse(String(raw))
    return d && typeof d === "object" ? d : null
  } catch (e) {
    return null
  }
}

// A Status document, or a marker object the panel renders as "not answering".
function parseStatus(raw) {
  var d = parseJson(raw)
  if (!d || d.ok !== true || typeof d.state !== "string") return { ok: false, state: "disconnected" }
  return d
}

function bytes(n) {
  n = Math.max(0, Number(n) || 0)
  var u = ["B", "KiB", "MiB", "GiB", "TiB"]
  var i = 0
  while (n >= 1024 && i < u.length - 1) { n /= 1024; i++ }
  return (i === 0 ? n.toFixed(0) : n.toFixed(1)) + " " + u[i]
}

function rate(n) { return bytes(n) + "/s" }

function uptime(sec) {
  sec = Math.max(0, Math.floor(Number(sec) || 0))
  var d = Math.floor(sec / 86400), h = Math.floor((sec % 86400) / 3600)
  var m = Math.floor((sec % 3600) / 60), s = sec % 60
  if (d > 0) return d + "d " + h + "h"
  if (h > 0) return h + "h " + (m < 10 ? "0" : "") + m + "m"
  if (m > 0) return m + "m " + (s < 10 ? "0" : "") + s + "s"
  return s + "s"
}

// Regional-indicator pair. Proton ships two codes ISO 3166-1 does not assign:
// UK (the flag lives at GB) and XK (Kosovo, which has no emoji flag at all).
function flag(cc) {
  var c = String(cc || "").toUpperCase()
  if (c === "UK") c = "GB"
  if (c === "XK" || !/^[A-Z]{2}$/.test(c)) return ""
  return String.fromCharCode(0xd83c, 0xdde6 + c.charCodeAt(0) - 65) +
         String.fromCharCode(0xd83c, 0xdde6 + c.charCodeAt(1) - 65)
}

function featureLabels(features) {
  if (!Array.isArray(features) || features.length === 0) return ""
  var names = { "secure-core": "Secure Core", "tor": "Tor", "p2p": "P2P", "streaming": "Streaming", "ipv6": "IPv6" }
  var out = []
  for (var i = 0; i < features.length; i++) {
    var f = String(features[i])
    out.push(names[f] || f)
  }
  return out.join(" · ")
}

function statePhrase(status) {
  if (!status || status.ok !== true) return "fproton is not answering"
  switch (String(status.state)) {
    case "connected": return "Connected"
    case "connecting": return "Connecting…"
    case "disconnecting": return "Disconnecting…"
    case "error": return "Error"
    default: return "Not connected"
  }
}

function serverTitle(status) {
  var s = status ? status.server : null
  if (s && s.name) return String(s.name)
  return "Proton VPN"
}

// "CH#461 · Zurich, Switzerland · load 63%" — whatever of it is known.
function serverLine(status) {
  var s = status ? status.server : null
  if (!s) return "No server yet"
  var parts = []
  var place = []
  if (s.city) place.push(String(s.city))
  if (s.country_name) place.push(String(s.country_name))
  else if (s.country) place.push(String(s.country))
  if (place.length > 0) parts.push(place.join(", "))
  if (typeof s.load === "number") parts.push("load " + s.load + "%")
  var feats = featureLabels(s.features)
  if (feats !== "") parts.push(feats)
  return parts.length > 0 ? parts.join(" · ") : "No server yet"
}

function trafficLine(status) {
  var t = status ? status.traffic : null
  if (!t) return "traffic unavailable"
  var down = t.rx_bps === null || t.rx_bps === undefined ? bytes(t.rx_bytes) : rate(t.rx_bps)
  var up = t.tx_bps === null || t.tx_bps === undefined ? bytes(t.tx_bytes) : rate(t.tx_bps)
  return "↓ " + down + "   ↑ " + up
}

function countryLabel(row) {
  if (!row) return ""
  var name = row.country_name ? String(row.country_name) : String(row.country || "")
  var f = flag(row.country)
  return f === "" ? name : f + "  " + name
}

function countryMeta(row) {
  if (!row) return ""
  var parts = [row.servers + (Number(row.servers) === 1 ? " server" : " servers")]
  if (typeof row.min_load === "number") parts.push("from " + row.min_load + "%")
  if (row.has_free === true) parts.push("free")
  return parts.join(" · ")
}

// A recents entry is one of the exact strings `fproton connect` accepts.
function targetLabel(target) {
  var t = String(target || "")
  if (t === "fastest") return "Fastest"
  if (t === "p2p") return "P2P"
  if (t === "secure-core") return "Secure Core"
  if (t === "tor") return "Tor"
  if (t === "random") return "Random"
  if (t.indexOf("city:") === 0) return t.substring(5)
  return t
}

function isCountry(value) { return /^[A-Z]{2}$/.test(String(value || "")) }

function isServer(value) {
  var v = String(value || "")
  return v.length <= 12 && /^[A-Z]{2}(-(?:[A-Z]{2}|FREE))?(#[0-9]{1,5})?(-TOR)?$/.test(v)
}

function isCity(value) {
  var v = String(value || "")
  if (v.length < 1 || v.length > 40) return false
  try {
    return new RegExp("^[\\p{L} .'-]+$", "u").test(v)
  } catch (e) {
    // Older JS engines have no Unicode property escapes; fall back to a
    // conservative class that still rejects every shell and argv metacharacter.
    return /^[A-Za-z\u00c0-\u024f\u0370-\u1fff\u3040-\ud7ff .'-]+$/.test(v)
  }
}

// The whole grammar of DESIGN §4 `connect`, for values the panel did not make
// up itself (recents come back from a state file).
function isTarget(value) {
  var t = String(value || "")
  if (["fastest", "p2p", "secure-core", "tor", "random"].indexOf(t) >= 0) return true
  if (t.indexOf("city:") === 0) return isCity(t.substring(5))
  return isCountry(t) || isServer(t)
}

// A command line typed into the widget's settings, split into argv. Every token
// must be inert: no spaces to re-split, no shell metacharacters, nothing that
// would matter if a launcher script passed it through `eval`.
function safeArgv(command) {
  var parts = String(command || "").trim().split(/\s+/)
  if (parts.length === 0 || parts[0] === "") return []
  for (var i = 0; i < parts.length; i++) {
    if (!/^[A-Za-z0-9._:@/=+-]+$/.test(parts[i])) return []
  }
  return parts
}
