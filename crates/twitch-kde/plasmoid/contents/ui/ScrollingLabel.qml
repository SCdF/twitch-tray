import QtQuick
import QtQuick.Controls as Controls

Item {
    id: root

    property string text: ""
    property real textOpacity: 0.5
    property bool scrollEnabled: false
    property color fadeColor: palette.window

    implicitHeight: label.implicitHeight
    clip: true

    property real overflow: Math.max(0, label.implicitWidth - width)

    onScrollEnabledChanged: {
        if (scrollEnabled && overflow > 0) {
            scrollAnim.start()
        } else {
            scrollAnim.stop()
            label.x = 0
        }
    }

    Controls.Label {
        id: label
        objectName: "scrollingLabel"
        text: root.text
        opacity: root.textOpacity
        width: Math.max(implicitWidth, root.width)

        SequentialAnimation on x {
            id: scrollAnim
            running: false
            loops: Animation.Infinite

            PauseAnimation { duration: 2000 }
            NumberAnimation {
                to: -root.overflow
                duration: root.overflow * 25
                easing.type: Easing.Linear
            }
            PauseAnimation { duration: 3000 }
            NumberAnimation {
                to: 0
                duration: 0
            }
        }
    }

    Rectangle {
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: 16
        visible: root.overflow > 0 && label.x >= 0
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0.0; color: "transparent" }
            GradientStop { position: 1.0; color: root.fadeColor }
        }
    }
}
