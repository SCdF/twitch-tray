import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts

Controls.ItemDelegate {
    id: root

    property string broadcasterLogin: ""
    property string broadcasterName: ""
    property string startTimeFormatted: ""
    property string title: ""
    property string category: ""
    property string profileImageUrl: ""
    property bool isInferred: false
    property bool isFavourite: false

    signal scheduleClicked(string broadcasterLogin)

    onClicked: root.scheduleClicked(root.broadcasterLogin)

    hoverEnabled: true

    contentItem: RowLayout {
        id: row
        spacing: 8

        StreamerAvatar {
            id: avatarContainer
            objectName: "avatarContainer"
            profileImageUrl: root.profileImageUrl
            displayName: root.broadcasterName
            isFavourite: root.isFavourite
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0

            RowLayout {
                Layout.fillWidth: true

                Controls.Label {
                    id: broadcasterNameLabel
                    objectName: "broadcasterNameLabel"
                    text: root.broadcasterName
                    font.bold: true
                    elide: Text.ElideRight
                }

                Controls.Label {
                    id: categoryLabel
                    objectName: "categoryLabel"
                    text: root.category ? "\u00B7 " + root.category : ""
                    opacity: 0.7
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }

                Controls.Label {
                    id: startTimeLabel
                    objectName: "startTimeLabel"
                    text: root.startTimeFormatted
                    opacity: 0.7
                    horizontalAlignment: Text.AlignRight
                }
            }

            RowLayout {
                Layout.fillWidth: true

                ScrollingLabel {
                    id: titleLabel
                    objectName: "titleLabel"
                    Layout.fillWidth: true
                    text: root.title || " "
                    textOpacity: root.title !== "" ? 0.5 : 0
                    scrollEnabled: root.hovered && root.title !== ""
                    fadeColor: root.palette.window
                }

                Controls.Label {
                    id: inferredIndicator
                    objectName: "inferredIndicator"
                    text: qsTr("(inferred)")
                    font.italic: true
                    opacity: 0.5
                    visible: root.isInferred
                    horizontalAlignment: Text.AlignRight
                }
            }
        }
    }
}
