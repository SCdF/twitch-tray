import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts

Controls.ItemDelegate {
    id: root

    property string userLogin: ""
    property string userName: ""
    property string gameName: ""
    property string viewerCountFormatted: ""
    property string durationFormatted: ""
    property bool isFavourite: false

    signal streamClicked(string userLogin)

    onClicked: root.streamClicked(root.userLogin)

    contentItem: RowLayout {
        id: row
        spacing: 4

        Controls.Label {
            id: favouriteStar
            objectName: "favouriteStar"
            text: "\u2605"
            visible: root.isFavourite
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
                    Layout.fillWidth: true
                    elide: Text.ElideRight
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
                    id: gameNameLabel
                    objectName: "gameNameLabel"
                    text: root.gameName
                    opacity: 0.7
                    Layout.fillWidth: true
                    elide: Text.ElideRight
                }

                Controls.Label {
                    id: durationLabel
                    objectName: "durationLabel"
                    text: root.durationFormatted
                    opacity: 0.7
                }
            }
        }
    }
}
