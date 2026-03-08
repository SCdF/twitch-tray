import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.plasma5support as P5Support
import org.kde.kirigami as Kirigami

PlasmoidItem {
    id: root

    Plasmoid.status: {
        if (!daemonRunning || !state.authenticated)
            return PlasmaCore.Types.PassiveStatus
        if (state.live.visible.length + state.live.overflow.length > 0)
            return PlasmaCore.Types.ActiveStatus
        return PlasmaCore.Types.PassiveStatus
    }

    property bool daemonRunning: false
    property var state: ({
        "authenticated": false,
        "login_state": { "type": "Idle" },
        "live": { "visible": [], "overflow": [] },
        "categories": [],
        "schedule": { "lookahead_hours": 24, "loaded": true, "visible": [], "overflow": [] }
    })

    // D-Bus communication via qdbus6 through the executable DataEngine
    P5Support.DataSource {
        id: dbusExec
        engine: "executable"
        connectedSources: []

        onNewData: (sourceName, data) => {
            disconnectSource(sourceName)
            if (sourceName.indexOf("State") >= 0) {
                var out = data["stdout"].trim()
                if (out.length > 0) {
                    try {
                        root.state = JSON.parse(out)
                        root.daemonRunning = true
                    } catch (e) {
                        console.warn("TwitchTray: failed to parse state:", e)
                        root.daemonRunning = false
                    }
                } else {
                    root.daemonRunning = false
                }
            }
        }
    }

    function pollState() {
        dbusExec.connectSource(
            "qdbus6 info.sdufresne.TwitchTray1 /info/sdufresne/TwitchTray" +
            " info.sdufresne.TwitchTray1.State"
        )
    }

    function dbusCall(method) {
        dbusExec.connectSource(
            "qdbus6 info.sdufresne.TwitchTray1 /info/sdufresne/TwitchTray" +
            " info.sdufresne.TwitchTray1." + method
        )
    }

    function dbusCallArgs(method, args) {
        dbusExec.connectSource(
            "qdbus6 info.sdufresne.TwitchTray1 /info/sdufresne/TwitchTray" +
            " info.sdufresne.TwitchTray1." + method + " " + args
        )
    }

    Timer {
        interval: 1000
        running: true
        repeat: true
        onTriggered: root.pollState()
    }

    compactRepresentation: CompactRepresentation {
        daemonRunning: root.daemonRunning
        authenticated: root.state.authenticated
    }

    fullRepresentation: FullRepresentation {
        daemonRunning: root.daemonRunning
        plasmoidState: root.state
        onLoginRequested: root.dbusCall("Login")
        onCancelLoginRequested: root.dbusCall("CancelLogin")
        onCopyCodeRequested: (code) => Qt.copyToClipboard(code)
        onLogoutRequested: root.dbusCall("Logout")
        onSettingsRequested: root.dbusCall("OpenSettings")
        onOpenStream: (login) =>
            root.dbusCallArgs("OpenStream", "'" + login + "'")
    }
}
