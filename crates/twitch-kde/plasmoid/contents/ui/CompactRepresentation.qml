import QtQuick
import org.kde.kirigami as Kirigami

Item {
    id: root

    property bool daemonRunning: false
    property bool authenticated: false

    implicitWidth: 24
    implicitHeight: 24

    Kirigami.Icon {
        id: twitchIcon
        objectName: "twitchIcon"
        anchors.centerIn: parent
        width: 22
        height: 22
        source: Qt.resolvedUrl("../../icon.png")
        isMask: true
        opacity: (root.daemonRunning && root.authenticated) ? 1.0 : 0.4
    }
}
