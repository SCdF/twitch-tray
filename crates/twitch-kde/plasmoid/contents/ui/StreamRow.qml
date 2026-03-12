import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts

Controls.ItemDelegate {
    id: root

    property string login: ""
    property string displayName: ""
    property string subtitle: ""
    property string title: ""
    property string profileImageUrl: ""
    property string topRightText: ""
    property string bottomRightText: ""
    property bool bottomRightItalic: false
    property bool isFavourite: false
    property bool isHot: false

    signal clicked_(string login)

    onClicked: root.clicked_(root.login)

    hoverEnabled: true

    contentItem: RowLayout {
        spacing: 8

        StreamerAvatar {
            objectName: "avatarContainer"
            profileImageUrl: root.profileImageUrl
            displayName: root.displayName
            isFavourite: root.isFavourite
            isHot: root.isHot
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0

            RowLayout {
                Layout.fillWidth: true

                Controls.Label {
                    objectName: "nameLabel"
                    text: root.displayName
                    font.bold: true
                    elide: Text.ElideRight
                }

                Controls.Label {
                    objectName: "subtitleLabel"
                    text: root.subtitle ? "\u00B7 " + root.subtitle : ""
                    opacity: 0.7
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }

                Controls.Label {
                    objectName: "topRightLabel"
                    text: root.topRightText
                    opacity: 0.7
                }
            }

            RowLayout {
                Layout.fillWidth: true
                visible: root.title !== "" || root.bottomRightText !== ""

                ScrollingLabel {
                    objectName: "titleLabel"
                    Layout.fillWidth: true
                    visible: root.title !== ""
                    text: root.title
                    scrollEnabled: root.hovered
                    fadeColor: root.palette.window
                }

                Controls.Label {
                    objectName: "bottomRightLabel"
                    text: root.bottomRightText
                    font.italic: root.bottomRightItalic
                    opacity: 0.5
                    visible: root.bottomRightText !== ""
                }
            }
        }
    }
}
