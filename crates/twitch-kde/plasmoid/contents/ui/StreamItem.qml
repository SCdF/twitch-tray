import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts

Controls.ItemDelegate {
    id: root

    property string userLogin: ""
    property string userName: ""
    property string gameName: ""
    property string title: ""
    property string profileImageUrl: ""
    property string viewerCountFormatted: ""
    property string durationFormatted: ""
    property bool isFavourite: false

    signal streamClicked(string userLogin)

    onClicked: root.streamClicked(root.userLogin)

    contentItem: RowLayout {
        id: row
        spacing: 8

        Rectangle {
            id: avatarContainer
            objectName: "avatarContainer"
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
                    text: root.userName.charAt(0)
                    font.bold: true
                }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0

            RowLayout {
                Layout.fillWidth: true

                Controls.Label {
                    id: userNameLabel
                    objectName: "userNameLabel"
                    text: root.userName
                    font.bold: true
                    elide: Text.ElideRight
                }

                Controls.Label {
                    id: gameNameLabel
                    objectName: "gameNameLabel"
                    text: root.gameName ? "\u00B7 " + root.gameName : ""
                    opacity: 0.7
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }

                Controls.Label {
                    id: viewerCountLabel
                    objectName: "viewerCountLabel"
                    text: root.viewerCountFormatted
                    opacity: 0.7
                }
            }

            RowLayout {
                Layout.fillWidth: true

                Controls.Label {
                    id: titleLabel
                    objectName: "titleLabel"
                    text: root.title
                    opacity: 0.5
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                    visible: root.title !== ""
                }

                Controls.Label {
                    id: durationLabel
                    objectName: "durationLabel"
                    text: root.durationFormatted
                    opacity: 0.5
                }
            }
        }
    }
}
