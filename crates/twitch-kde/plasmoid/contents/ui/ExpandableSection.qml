import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts

ColumnLayout {
    id: root

    property var items: []
    property bool expanded: false
    default property alias content: contentColumn.children

    signal avatarClicked(int index)

    spacing: 0

    // Collapsed: avatar row inside an ItemDelegate (same padding/height as StreamItem)
    Controls.ItemDelegate {
        id: avatarDelegate
        objectName: "avatarDelegate"
        Layout.fillWidth: true
        visible: !root.expanded && root.items.length > 0
        onClicked: root.expanded = true

        contentItem: RowLayout {
            spacing: 8

            Item {
                id: avatarClip
                objectName: "avatarClip"
                Layout.fillWidth: true
                implicitHeight: avatarList.height
                clip: true

                property real overflow: Math.max(0, avatarList.implicitWidth - width)

                Row {
                    id: avatarList
                    objectName: "avatarList"
                    spacing: 4

                    Repeater {
                        model: root.items
                        delegate: MouseArea {
                            width: 40
                            height: 40
                            cursorShape: Qt.PointingHandCursor
                            onClicked: (mouse) => {
                                mouse.accepted = true
                                root.avatarClicked(index)
                            }

                            StreamerAvatar {
                                anchors.fill: parent
                                profileImageUrl: modelData.profile_image_url || ""
                                displayName: modelData.user_name || modelData.broadcaster_name || ""
                                isFavourite: modelData.is_favourite || false
                            }
                        }
                    }
                }

                Rectangle {
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.bottom: parent.bottom
                    width: 24
                    visible: avatarClip.overflow > 0
                    gradient: Gradient {
                        orientation: Gradient.Horizontal
                        GradientStop { position: 0.0; color: "transparent" }
                        GradientStop { position: 1.0; color: avatarDelegate.palette.window }
                    }
                }
            }

            Controls.Label {
                text: "\u2304"
                font.pixelSize: 16
                opacity: 0.5
            }
        }
    }

    // Expanded: collapse row (same ItemDelegate wrapper)
    Controls.ItemDelegate {
        id: collapseDelegate
        objectName: "collapseDelegate"
        Layout.fillWidth: true
        visible: root.expanded
        onClicked: root.expanded = false

        contentItem: RowLayout {
            Item { Layout.fillWidth: true }

            Controls.Label {
                text: "\u2303"
                font.pixelSize: 16
                opacity: 0.5
            }
        }
    }

    // Expanded content
    Column {
        id: contentColumn
        objectName: "contentColumn"
        Layout.fillWidth: true
        visible: root.expanded
    }
}
