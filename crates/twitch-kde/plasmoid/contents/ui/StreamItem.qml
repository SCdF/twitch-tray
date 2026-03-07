import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts

Item {
    id: root

    property string userLogin: ""
    property string userName: ""
    property string gameName: ""
    property string viewerCountFormatted: ""
    property string durationFormatted: ""
    property bool isFavourite: false

    signal clicked(string userLogin)

    implicitHeight: row.implicitHeight + 8
    implicitWidth: row.implicitWidth

    MouseArea {
        id: clickArea
        objectName: "clickArea"
        anchors.fill: parent
        onClicked: root.clicked(root.userLogin)
    }

    RowLayout {
        id: row
        anchors.fill: parent
        anchors.margins: 4
        spacing: 4

        Controls.Label {
            id: favouriteStar
            objectName: "favouriteStar"
            text: "\u2605"
            visible: root.isFavourite
        }

        GridLayout {
            columns: 2
            rowSpacing: 0
            columnSpacing: 4
            Layout.fillWidth: true

            Controls.Label {
                id: userNameLabel
                objectName: "userNameLabel"
                text: root.userName
                font.bold: true
                Layout.fillWidth: true
            }

            Controls.Label {
                id: viewerCountLabel
                objectName: "viewerCountLabel"
                text: root.viewerCountFormatted
                opacity: 0.7
                horizontalAlignment: Text.AlignRight
            }

            Controls.Label {
                id: gameNameLabel
                objectName: "gameNameLabel"
                text: root.gameName
                opacity: 0.7
                Layout.fillWidth: true
            }

            Controls.Label {
                id: durationLabel
                objectName: "durationLabel"
                text: root.durationFormatted
                opacity: 0.7
                horizontalAlignment: Text.AlignRight
            }
        }
    }
}
