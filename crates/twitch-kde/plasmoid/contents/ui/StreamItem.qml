import QtQuick
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

        Text {
            id: favouriteStar
            objectName: "favouriteStar"
            text: "\u2605"
            visible: root.isFavourite
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0

            Text {
                id: userNameLabel
                objectName: "userNameLabel"
                text: root.userName
                font.bold: true
                Layout.fillWidth: true
            }

            Text {
                id: subtitleLabel
                objectName: "subtitleLabel"
                text: root.gameName + " \u00B7 " + root.viewerCountFormatted + " \u00B7 " + root.durationFormatted
                opacity: 0.7
                Layout.fillWidth: true
            }
        }
    }
}
