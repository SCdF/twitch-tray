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
                            hoverEnabled: true
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

                            Controls.ToolTip {
                                visible: parent.containsMouse
                                delay: 500

                                contentItem: ColumnLayout {
                                    spacing: 2

                                    RowLayout {
                                        Controls.Label {
                                            text: modelData.user_name || modelData.broadcaster_name || ""
                                            font.bold: true
                                        }

                                        Controls.Label {
                                            text: (modelData.game_name || modelData.category)
                                                  ? "\u00B7 " + (modelData.game_name || modelData.category)
                                                  : ""
                                            opacity: 0.7
                                            visible: text !== ""
                                        }

                                    }

                                    Controls.Label {
                                        text: modelData.title || ""
                                        opacity: 0.5
                                        visible: text !== ""
                                        Layout.maximumWidth: 280
                                        wrapMode: Text.WordWrap
                                    }

                                    RowLayout {
                                        visible: (modelData.viewer_count_formatted || "") !== ""
                                                 || (modelData.duration_formatted || "") !== ""
                                                 || (modelData.start_time_formatted || "") !== ""
                                                 || (modelData.is_inferred || false)

                                        Controls.Label {
                                            text: modelData.viewer_count_formatted || ""
                                            opacity: 0.5
                                            visible: text !== ""
                                        }

                                        Controls.Label {
                                            text: "\u00B7"
                                            opacity: 0.5
                                            visible: (modelData.viewer_count_formatted || "") !== ""
                                                     && (modelData.duration_formatted || "") !== ""
                                        }

                                        Controls.Label {
                                            text: modelData.duration_formatted || ""
                                            opacity: 0.5
                                            visible: text !== ""
                                        }

                                        Controls.Label {
                                            text: modelData.start_time_formatted || ""
                                            opacity: 0.5
                                            visible: text !== ""
                                        }

                                        Controls.Label {
                                            text: qsTr("(inferred)")
                                            font.italic: true
                                            opacity: 0.5
                                            visible: modelData.is_inferred || false
                                        }
                                    }
                                }
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
