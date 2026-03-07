import QtQuick
import org.kde.kirigami as Kirigami

Item {
    id: root

    property int liveCount: 0

    implicitWidth: 24
    implicitHeight: 24

    Kirigami.Icon {
        id: twitchIcon
        objectName: "twitchIcon"
        anchors.centerIn: parent
        width: 22
        height: 22
        source: Qt.resolvedUrl("../../icon.png")
        isMask: false
    }

    Rectangle {
        id: badge
        objectName: "badge"
        visible: root.liveCount > 0
        anchors.top: parent.top
        anchors.right: parent.right
        width: Math.max(14, badgeText.implicitWidth + 4)
        height: 14
        radius: 7
        color: "#9147ff"

        Text {
            id: badgeText
            objectName: "badgeText"
            anchors.centerIn: parent
            text: root.liveCount.toString()
            color: "white"
            font.pixelSize: 9
            font.bold: true
        }
    }
}
