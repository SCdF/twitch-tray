import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

RowLayout {
    property alias text: heading.text

    spacing: Kirigami.Units.smallSpacing

    Kirigami.Heading {
        id: heading
        objectName: "heading"
        level: 5
        font.bold: true
    }

    Kirigami.Separator {
        objectName: "separator"
        Layout.fillWidth: true
        Layout.alignment: Qt.AlignVCenter
    }
}
