import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    property alias text: heading.text

    spacing: 2

    Kirigami.Heading {
        id: heading
        objectName: "heading"
        level: 5
        font.bold: true
        Layout.fillWidth: true
    }

    Kirigami.Separator {
        objectName: "separator"
        Layout.fillWidth: true
    }
}
