import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts

ColumnLayout {
    id: root

    property string categoryId: ""
    property string name: ""
    property string boxArtUrl: ""
    property string totalViewersFormatted: ""
    property var streams: []
    property bool expanded: false

    signal streamClicked(string userLogin)

    spacing: 0

    Controls.ItemDelegate {
        id: headerDelegate
        objectName: "categoryHeader"
        Layout.fillWidth: true
        onClicked: root.expanded = !root.expanded

        contentItem: RowLayout {
            spacing: 8

            Item {
                id: iconContainer
                objectName: "iconContainer"
                width: 40
                height: 40

                Rectangle {
                    id: iconInner
                    objectName: "iconInner"
                    width: 30
                    height: 40
                    anchors.centerIn: parent
                    color: "transparent"

                    Image {
                        id: boxArtImage
                        objectName: "boxArtImage"
                        anchors.fill: parent
                        source: root.boxArtUrl
                        fillMode: Image.PreserveAspectCrop
                        smooth: true
                        mipmap: true
                        visible: root.boxArtUrl !== ""
                    }

                    Rectangle {
                        id: iconPlaceholder
                        objectName: "iconPlaceholder"
                        anchors.fill: parent
                        color: root.palette.mid
                        visible: root.boxArtUrl === ""

                        Controls.Label {
                            anchors.centerIn: parent
                            text: root.name.charAt(0)
                            font.bold: true
                        }
                    }
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0

                Controls.Label {
                    id: nameLabel
                    objectName: "nameLabel"
                    text: root.name
                    font.bold: true
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }

                Controls.Label {
                    id: viewerCountLabel
                    objectName: "viewerCountLabel"
                    text: root.totalViewersFormatted
                    opacity: 0.5
                }
            }

            Controls.Label {
                id: chevron
                objectName: "chevron"
                text: root.expanded ? "\u2303" : "\u2304"
                font.pixelSize: 16
                opacity: 0.5
            }
        }
    }

    Column {
        id: streamList
        objectName: "streamList"
        Layout.fillWidth: true
        Layout.leftMargin: 16
        visible: root.expanded

        Repeater {
            model: root.streams
            delegate: Controls.ItemDelegate {
                id: streamDelegate
                width: streamList.width
                hoverEnabled: true
                onClicked: root.streamClicked(modelData.user_login)

                contentItem: RowLayout {
                    spacing: 8

                    StreamerAvatar {
                        profileImageUrl: modelData.profile_image_url || ""
                        displayName: modelData.user_name
                        isFavourite: false
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0

                        RowLayout {
                            Layout.fillWidth: true

                            Controls.Label {
                                text: modelData.user_name
                                font.bold: true
                                elide: Text.ElideRight
                            }

                            Item { Layout.fillWidth: true }

                            Controls.Label {
                                text: modelData.viewer_count_formatted
                                opacity: 0.7
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            visible: (modelData.title || "") !== "" || (modelData.duration_formatted || "") !== ""

                            ScrollingLabel {
                                Layout.fillWidth: true
                                visible: (modelData.title || "") !== ""
                                text: modelData.title || ""
                                scrollEnabled: streamDelegate.hovered
                                fadeColor: root.palette.window
                            }

                            Controls.Label {
                                text: modelData.duration_formatted || ""
                                opacity: 0.5
                                visible: (modelData.duration_formatted || "") !== ""
                            }
                        }
                    }
                }
            }
        }
    }
}
