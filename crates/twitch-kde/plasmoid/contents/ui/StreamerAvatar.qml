import QtQuick
import QtQuick.Controls as Controls

Rectangle {
    id: root

    property string profileImageUrl: ""
    property string displayName: ""
    property bool isFavourite: false

    width: 40
    height: 40
    color: "transparent"
    border.width: root.isFavourite ? 2 : 0
    border.color: root.isFavourite ? root.palette.highlight : "transparent"

    Image {
        id: avatarImage
        objectName: "avatarImage"
        anchors.fill: parent
        anchors.margins: root.isFavourite ? 2 : 0
        source: root.profileImageUrl
        fillMode: Image.PreserveAspectCrop
        visible: root.profileImageUrl !== ""
    }

    Rectangle {
        id: avatarPlaceholder
        objectName: "avatarPlaceholder"
        anchors.fill: parent
        anchors.margins: root.isFavourite ? 2 : 0
        color: root.palette.mid
        visible: root.profileImageUrl === ""

        Controls.Label {
            anchors.centerIn: parent
            text: root.displayName.charAt(0)
            font.bold: true
        }
    }
}
