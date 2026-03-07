import QtQuick

Item {
    id: root

    property int liveCount: 0

    implicitWidth: 24
    implicitHeight: 24

    Text {
        id: twitchIcon
        objectName: "twitchIcon"
        anchors.centerIn: parent
        text: "\uD83D\uDCFA"
        font.pixelSize: 16
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
