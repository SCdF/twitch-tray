import QtQuick
import QtQuick.Layouts

Item {
    id: root

    property string broadcasterLogin: ""
    property string broadcasterName: ""
    property string startTimeFormatted: ""
    property bool isInferred: false
    property bool isFavourite: false

    signal clicked(string broadcasterLogin)

    implicitHeight: row.implicitHeight + 8
    implicitWidth: row.implicitWidth

    MouseArea {
        id: clickArea
        objectName: "clickArea"
        anchors.fill: parent
        onClicked: root.clicked(root.broadcasterLogin)
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

        Text {
            id: inferredIndicator
            objectName: "inferredIndicator"
            text: "\u2728"
            visible: root.isInferred
        }

        Text {
            id: broadcasterNameLabel
            objectName: "broadcasterNameLabel"
            text: root.broadcasterName
            font.bold: true
        }

        Text {
            id: startTimeLabel
            objectName: "startTimeLabel"
            text: root.startTimeFormatted
            opacity: 0.7
        }
    }
}
