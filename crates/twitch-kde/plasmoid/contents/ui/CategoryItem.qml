import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts

ColumnLayout {
    id: root

    property string categoryId: ""
    property string name: ""
    property string boxArtUrl: ""
    property string totalViewersFormatted: ""
    property string streamCountFormatted: ""
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

            Rectangle {
                id: iconContainer
                objectName: "iconContainer"
                width: 30
                height: 40
                color: "transparent"

                Image {
                    id: boxArtImage
                    objectName: "boxArtImage"
                    anchors.fill: parent
                    source: root.boxArtUrl
                    fillMode: Image.PreserveAspectCrop
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

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0

                RowLayout {
                    Layout.fillWidth: true

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
                        opacity: 0.7
                    }
                }

                RowLayout {
                    Layout.fillWidth: true

                    Item { Layout.fillWidth: true }

                    Controls.Label {
                        id: streamCountLabel
                        objectName: "streamCountLabel"
                        text: root.streamCountFormatted
                        opacity: 0.5
                    }
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
                width: streamList.width
                onClicked: root.streamClicked(modelData.user_login)

                contentItem: RowLayout {
                    spacing: 4

                    Controls.Label {
                        text: modelData.user_name
                        font.bold: true
                        Layout.fillWidth: true
                    }

                    Controls.Label {
                        text: modelData.viewer_count_formatted
                        opacity: 0.7
                    }
                }
            }
        }
    }
}
