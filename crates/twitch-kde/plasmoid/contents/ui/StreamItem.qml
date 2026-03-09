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

    hoverEnabled: true

    contentItem: RowLayout {
        id: row
        spacing: 8

        StreamerAvatar {
            id: avatarContainer
            objectName: "avatarContainer"
            profileImageUrl: root.profileImageUrl
            displayName: root.userName
            isFavourite: root.isFavourite
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
                visible: root.title !== "" || root.durationFormatted !== ""

                ScrollingLabel {
                    id: titleClip
                    objectName: "titleClip"
                    Layout.fillWidth: true
                    visible: root.title !== ""
                    text: root.title
                    scrollEnabled: root.hovered
                    fadeColor: root.palette.window
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
