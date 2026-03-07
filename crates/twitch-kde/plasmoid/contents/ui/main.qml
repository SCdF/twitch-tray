import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.kirigami as Kirigami
import ui

PlasmoidItem {
    id: root

    property var state: ({
        "authenticated": false,
        "login_state": { "type": "Idle" },
        "live": { "visible": [], "overflow": [] },
        "categories": [],
        "schedule": { "lookahead_hours": 24, "loaded": true, "visible": [], "overflow": [] }
    })

    // D-Bus communication via qdbus6 through the executable DataEngine
    PlasmaCore.DataSource {
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
                    } catch (e) {
                        console.warn("TwitchTray: failed to parse state:", e)
                    }
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
        liveCount: root.state.live.visible.length + root.state.live.overflow.length
    }

    fullRepresentation: Controls.ScrollView {
        Layout.preferredWidth: 320
        Layout.preferredHeight: Math.min(fullColumn.implicitHeight + 16, 600)

        Column {
            id: fullColumn
            width: 320
            padding: 8
            spacing: 4

            // --- Unauthenticated ---
            LoginView {
                visible: !root.state.authenticated
                width: parent.width - 16
                loginState: root.state.login_state.type
                userCode: root.state.login_state.user_code || ""
                verificationUri: root.state.login_state.verification_uri || ""
                onLoginRequested: root.dbusCall("Login")
                onCancelLoginRequested: root.dbusCall("CancelLogin")
                onCopyCodeRequested: (code) => Qt.copyToClipboard(code)
            }

            // --- Authenticated ---
            Column {
                visible: root.state.authenticated
                width: parent.width - 16
                spacing: 2

                // Live section
                SectionHeader {
                    width: parent.width
                    text: "Following Live (" +
                          (root.state.live.visible.length + root.state.live.overflow.length) + ")"
                }

                Repeater {
                    model: root.state.live.visible
                    delegate: StreamItem {
                        width: parent.width
                        userLogin: modelData.user_login
                        userName: modelData.user_name
                        gameName: modelData.game_name
                        viewerCountFormatted: modelData.viewer_count_formatted
                        durationFormatted: modelData.duration_formatted
                        isFavourite: modelData.is_favourite
                        onClicked: (login) =>
                            root.dbusCallArgs("OpenStream", "'" + login + "'")
                    }
                }

                ExpandableSection {
                    visible: root.state.live.overflow.length > 0
                    width: parent.width
                    heading: "More"
                    count: root.state.live.overflow.length

                    Repeater {
                        model: root.state.live.overflow
                        delegate: StreamItem {
                            width: parent.width
                            userLogin: modelData.user_login
                            userName: modelData.user_name
                            gameName: modelData.game_name
                            viewerCountFormatted: modelData.viewer_count_formatted
                            durationFormatted: modelData.duration_formatted
                            isFavourite: modelData.is_favourite
                            onClicked: (login) =>
                                root.dbusCallArgs("OpenStream", "'" + login + "'")
                        }
                    }
                }

                // Category sections (one collapsible section per category)
                Repeater {
                    model: root.state.categories
                    delegate: ExpandableSection {
                        width: parent.width
                        heading: modelData.name + " \u00B7 " + modelData.total_viewers_formatted
                        count: modelData.streams.length

                        Repeater {
                            model: modelData.streams
                            delegate: Item {
                                width: parent.width
                                implicitHeight: catRow.implicitHeight + 8

                                MouseArea {
                                    anchors.fill: parent
                                    onClicked: root.dbusCallArgs(
                                        "OpenStream", "'" + modelData.user_login + "'"
                                    )
                                }

                                RowLayout {
                                    id: catRow
                                    anchors.fill: parent
                                    anchors.margins: 4

                                    Text {
                                        text: modelData.user_name
                                        font.bold: true
                                        Layout.fillWidth: true
                                    }

                                    Text {
                                        text: modelData.viewer_count_formatted
                                        opacity: 0.7
                                    }
                                }
                            }
                        }
                    }
                }

                // Schedule section
                SectionHeader {
                    width: parent.width
                    text: "Scheduled (Next " + root.state.schedule.lookahead_hours + "h)"
                }

                Controls.BusyIndicator {
                    visible: !root.state.schedule.loaded
                    running: !root.state.schedule.loaded
                    anchors.horizontalCenter: parent.horizontalCenter
                }

                Repeater {
                    model: root.state.schedule.visible
                    delegate: ScheduleItem {
                        width: parent.width
                        broadcasterLogin: modelData.broadcaster_login
                        broadcasterName: modelData.broadcaster_name
                        startTimeFormatted: modelData.start_time_formatted
                        isInferred: modelData.is_inferred
                        isFavourite: modelData.is_favourite
                        onClicked: (login) => root.dbusCallArgs(
                            "OpenStreamerSettings",
                            "'" + login + "' '" + modelData.broadcaster_name + "'"
                        )
                    }
                }

                ExpandableSection {
                    visible: root.state.schedule.overflow.length > 0
                    width: parent.width
                    heading: "More"
                    count: root.state.schedule.overflow.length

                    Repeater {
                        model: root.state.schedule.overflow
                        delegate: ScheduleItem {
                            width: parent.width
                            broadcasterLogin: modelData.broadcaster_login
                            broadcasterName: modelData.broadcaster_name
                            startTimeFormatted: modelData.start_time_formatted
                            isInferred: modelData.is_inferred
                            isFavourite: modelData.is_favourite
                            onClicked: (login) => root.dbusCallArgs(
                                "OpenStreamerSettings",
                                "'" + login + "' '" + modelData.broadcaster_name + "'"
                            )
                        }
                    }
                }

                // Bottom actions
                Kirigami.Separator {
                    width: parent.width
                }

                RowLayout {
                    width: parent.width

                    Controls.Button {
                        text: "Settings"
                        onClicked: root.dbusCall("OpenSettings")
                    }

                    Item { Layout.fillWidth: true }

                    Controls.Button {
                        text: "Logout"
                        onClicked: root.dbusCall("Logout")
                    }
                }
            }
        }
    }
}
