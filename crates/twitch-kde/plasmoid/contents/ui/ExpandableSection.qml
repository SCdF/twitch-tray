import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: root

    property string heading: ""
    property int count: 0
    property bool expanded: false
    default property alias content: contentColumn.children

    spacing: 0

    MouseArea {
        id: headerArea
        objectName: "headerArea"
        Layout.fillWidth: true
        implicitHeight: headerRow.implicitHeight + 8

        onClicked: root.expanded = !root.expanded

        RowLayout {
            id: headerRow
            anchors.fill: parent
            anchors.leftMargin: 4

            Text {
                id: chevron
                text: root.expanded ? "\u25BC" : "\u25B6"
                font.pixelSize: 10
            }

            Text {
                id: headerLabel
                objectName: "headerLabel"
                text: root.heading + " (" + root.count + ")"
                font.bold: true
                Layout.fillWidth: true
            }
        }
    }

    Column {
        id: contentColumn
        objectName: "contentColumn"
        Layout.fillWidth: true
        visible: root.expanded
    }
}
