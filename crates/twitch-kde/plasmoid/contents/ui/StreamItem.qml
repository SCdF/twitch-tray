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
    onHoveredChanged: {
        if (hovered && titleClip.overflow > 0) {
            scrollAnim.start()
        } else {
            scrollAnim.stop()
            titleLabel.x = 0
        }
    }

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
                visible: root.title !== "" || root.durationFormatted !== ""

                Item {
                    id: titleClip
                    objectName: "titleClip"
                    Layout.fillWidth: true
                    implicitHeight: titleLabel.implicitHeight
                    clip: true
                    visible: root.title !== ""

                    property real overflow: Math.max(0, titleLabel.implicitWidth - width)

                    Controls.Label {
                        id: titleLabel
                        objectName: "titleLabel"
                        text: root.title
                        opacity: 0.5
                        width: Math.max(implicitWidth, titleClip.width)

                        SequentialAnimation on x {
                            id: scrollAnim
                            running: false
                            loops: Animation.Infinite

                            PauseAnimation { duration: 2000 }
                            NumberAnimation {
                                to: -titleClip.overflow
                                duration: titleClip.overflow * 25
                                easing.type: Easing.Linear
                            }
                            PauseAnimation { duration: 3000 }
                            NumberAnimation {
                                to: 0
                                duration: 0
                            }
                        }
                    }

                    // Fade-out hint on the right edge when text is clipped
                    Rectangle {
                        id: fadeHint
                        anchors.right: parent.right
                        anchors.top: parent.top
                        anchors.bottom: parent.bottom
                        width: 16
                        visible: titleClip.overflow > 0 && titleLabel.x >= 0
                        gradient: Gradient {
                            orientation: Gradient.Horizontal
                            GradientStop { position: 0.0; color: "transparent" }
                            GradientStop { position: 1.0; color: root.palette.window }
                        }
                    }
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
