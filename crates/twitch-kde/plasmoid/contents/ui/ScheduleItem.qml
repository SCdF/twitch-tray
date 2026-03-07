import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts

Controls.ItemDelegate {
    id: root

    property string broadcasterLogin: ""
    property string broadcasterName: ""
    property string startTimeFormatted: ""
    property bool isInferred: false
    property bool isFavourite: false

    signal scheduleClicked(string broadcasterLogin)

    onClicked: root.scheduleClicked(root.broadcasterLogin)

    contentItem: RowLayout {
        id: row
        spacing: 4

        Controls.Label {
            id: favouriteStar
            objectName: "favouriteStar"
            text: "\u2605"
            visible: root.isFavourite
        }

        Controls.Label {
            id: inferredIndicator
            objectName: "inferredIndicator"
            text: "\u2728"
            visible: root.isInferred
        }

        Controls.Label {
            id: broadcasterNameLabel
            objectName: "broadcasterNameLabel"
            text: root.broadcasterName
            font.bold: true
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
}
