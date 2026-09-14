import QtQuick
import QtQuick.Shapes
import qs.Commons
import qs.Ui

// The official Proton VPN mark, drawn natively from the Simple Icons path so it
// takes the theme foreground instead of the brand purple/green, the same reason
// the Tailscale widget redraws its logo rather than scaling a tiny SVG.
//
// Ported from ProtonIcon.qml in grichard99's `io.github.grichard99.omaproton-vpn`
// plugin (MIT, Copyright (c) 2026 grichard99); the `crossed` and `warning`
// modifiers below are modelled on Omarchy's own Ui/../tailscale/TailscaleIcon.qml.
// Source path: https://cdn.simpleicons.org/protonvpn (24x24 viewBox, CC0).
Item {
  id: root

  property real iconSize: Style.font.icon
  property color color: Color.foreground
  property color badgeColor: Color.urgent
  property bool crossed: false
  property bool warning: false

  readonly property real viewBox: 24

  width: iconSize
  height: iconSize
  implicitWidth: iconSize
  implicitHeight: iconSize

  Item {
    anchors.centerIn: parent
    width: root.viewBox
    height: root.viewBox
    // Scaled down from the 24px viewBox to the bar's icon size, so the shape
    // geometry stays crisp rather than being rasterized then stretched.
    scale: Math.min(root.width, root.height) / root.viewBox

    Shape {
      anchors.fill: parent
      antialiasing: true
      preferredRendererType: Shape.CurveRenderer

      ShapePath {
        fillColor: root.color
        strokeWidth: 0
        strokeColor: "transparent"
        // SVG's default fill-rule; the mark is two separate subpaths, not a
        // nested hole, so this matches the source rendering.
        fillRule: ShapePath.WindingFill

        PathSvg {
          path: "m10.176 20.058.858-1.28 6.513-9.838c.57-.86.026-2.014-1.005-2.131L.378 4.95l8.373 15.055a.84.84 0 0 0 1.424.052h.001zM23.586 7.14l-9.662 14.61c-1.036 1.567-3.38 1.478-4.293-.162l-.093-.168c.3-.01.594-.086.855-.235a1.85 1.85 0 0 0 .612-.57l.86-1.28 6.516-9.844c.46-.694.525-1.56.173-2.314a2.375 2.375 0 0 0-1.899-1.364L.493 3.956l-.476-.054C-.163 2.392 1.101.95 2.784 1.143l18.991 2.16c1.856.21 2.835 2.289 1.812 3.838z"
        }
      }
    }
  }

  // Disconnected: one stroke through the mark, in the mark's own colour.
  Rectangle {
    visible: root.crossed
    anchors.centerIn: parent
    width: parent.width * 1.22
    height: Math.max(2, parent.height * 0.14)
    radius: height / 2
    color: root.color
    rotation: -45
  }

  // Something needs a human: fproton unreachable, an error, or a Status warning.
  BorderSurface {
    visible: root.warning
    width: Math.max(7, parent.width * 0.42)
    height: width
    radius: width / 2
    color: root.badgeColor
    anchors.right: parent.right
    anchors.bottom: parent.bottom
    borderSpec: Border.flat(Color.popups.background, 1)

    Text {
      anchors.centerIn: parent
      textFormat: Text.PlainText
      text: "!"
      color: Color.background
      font.family: Style.font.family
      font.pixelSize: Math.max(6, parent.height * 0.72)
      font.bold: true
    }
  }
}
