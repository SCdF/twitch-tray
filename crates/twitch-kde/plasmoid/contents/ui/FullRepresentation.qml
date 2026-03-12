import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Controls.ScrollView {
    id: scrollView

    property bool daemonRunning: false
    property var plasmoidState: ({
        "authenticated": false,
        "login_state": { "type": "Idle" },
        "live": { "visible": [], "overflow": [] },
        "categories": [],
        "schedule": { "lookahead_hours": 24, "loaded": true, "visible": [], "overflow": [] }
    })

    signal loginRequested()
    signal cancelLoginRequested()
    signal copyCodeRequested(string code)
    signal logoutRequested()
    signal settingsRequested()
    signal openStream(string login)


    Layout.preferredWidth: 320
    Layout.preferredHeight: Math.min(fullColumn.implicitHeight + 16, 600)

    Column {
        id: fullColumn
        objectName: "fullColumn"
        width: scrollView.availableWidth
        padding: 8
        spacing: 4

        // --- Daemon not running ---
        Controls.Label {
            objectName: "daemonNotRunning"
            visible: !scrollView.daemonRunning
            width: parent.width - 16
            text: "Daemon not running"
            horizontalAlignment: Text.AlignHCenter
            opacity: 0.7
        }

        // --- Unauthenticated (daemon running but not logged in) ---
        LoginView {
            objectName: "loginView"
            visible: scrollView.daemonRunning && !scrollView.plasmoidState.authenticated
            width: parent.width - 16
            loginState: scrollView.plasmoidState.login_state.type
            userCode: scrollView.plasmoidState.login_state.user_code || ""
            verificationUri: scrollView.plasmoidState.login_state.verification_uri || ""
            onLoginRequested: scrollView.loginRequested()
            onCancelLoginRequested: scrollView.cancelLoginRequested()
            onCopyCodeRequested: (code) => scrollView.copyCodeRequested(code)
        }

        // --- Authenticated ---
        Column {
            objectName: "authenticatedContent"
            visible: scrollView.daemonRunning && scrollView.plasmoidState.authenticated
            width: parent.width - 16
            spacing: 2

            // Live section
            SectionHeader {
                objectName: "liveHeader"
                width: parent.width
                text: "Following Live (" +
                      (scrollView.plasmoidState.live.visible.length + scrollView.plasmoidState.live.overflow.length) + ")"
            }

            Repeater {
                model: scrollView.plasmoidState.live.visible
                delegate: StreamItem {
                    width: parent.width
                    userLogin: modelData.user_login
                    userName: modelData.user_name
                    gameName: modelData.game_name
                    title: modelData.title || ""
                    profileImageUrl: modelData.profile_image_url || ""
                    viewerCountFormatted: modelData.viewer_count_formatted
                    durationFormatted: modelData.duration_formatted
                    isFavourite: modelData.is_favourite
                    isHot: modelData.is_hot
                    onStreamClicked: (login) => scrollView.openStream(login)
                }
            }

            ExpandableSection {
                visible: scrollView.plasmoidState.live.overflow.length > 0
                width: parent.width
                items: scrollView.plasmoidState.live.overflow
                onAvatarClicked: (index) => scrollView.openStream(
                    scrollView.plasmoidState.live.overflow[index].user_login
                )

                Repeater {
                    model: scrollView.plasmoidState.live.overflow
                    delegate: StreamItem {
                        width: parent.width
                        userLogin: modelData.user_login
                        userName: modelData.user_name
                        gameName: modelData.game_name
                        title: modelData.title || ""
                        profileImageUrl: modelData.profile_image_url || ""
                        viewerCountFormatted: modelData.viewer_count_formatted
                        durationFormatted: modelData.duration_formatted
                        isFavourite: modelData.is_favourite
                        isHot: modelData.is_hot
                        onStreamClicked: (login) => scrollView.openStream(login)
                    }
                }
            }

            // Category sections
            SectionHeader {
                objectName: "categoriesHeader"
                width: parent.width
                text: "Categories"
                visible: scrollView.plasmoidState.categories.length > 0
            }

            Repeater {
                model: scrollView.plasmoidState.categories
                delegate: CategoryItem {
                    width: parent.width
                    categoryId: modelData.id
                    name: modelData.name
                    boxArtUrl: modelData.box_art_url || ""
                    totalViewersFormatted: modelData.total_viewers_formatted
                    streams: modelData.streams
                    onStreamClicked: (login) => scrollView.openStream(login)
                }
            }

            // Schedule section
            SectionHeader {
                objectName: "scheduleHeader"
                width: parent.width
                text: "Scheduled (Next " + scrollView.plasmoidState.schedule.lookahead_hours + "h)"
            }

            Controls.BusyIndicator {
                visible: !scrollView.plasmoidState.schedule.loaded
                running: !scrollView.plasmoidState.schedule.loaded
                anchors.horizontalCenter: parent.horizontalCenter
            }

            Repeater {
                model: scrollView.plasmoidState.schedule.visible
                delegate: ScheduleItem {
                    width: parent.width
                    broadcasterLogin: modelData.broadcaster_login
                    broadcasterName: modelData.broadcaster_name
                    startTimeFormatted: modelData.start_time_formatted
                    title: modelData.title || ""
                    category: modelData.category || ""
                    profileImageUrl: modelData.profile_image_url || ""
                    isInferred: modelData.is_inferred
                    isFavourite: modelData.is_favourite
                    onScheduleClicked: (login) => scrollView.openStream(login)
                }
            }

            ExpandableSection {
                visible: scrollView.plasmoidState.schedule.overflow.length > 0
                width: parent.width
                items: scrollView.plasmoidState.schedule.overflow
                onAvatarClicked: (index) => {
                    var item = scrollView.plasmoidState.schedule.overflow[index]
                    scrollView.openStream(item.broadcaster_login)
                }

                Repeater {
                    model: scrollView.plasmoidState.schedule.overflow
                    delegate: ScheduleItem {
                        width: parent.width
                        broadcasterLogin: modelData.broadcaster_login
                        broadcasterName: modelData.broadcaster_name
                        startTimeFormatted: modelData.start_time_formatted
                        title: modelData.title || ""
                        category: modelData.category || ""
                        profileImageUrl: modelData.profile_image_url || ""
                        isInferred: modelData.is_inferred
                        isFavourite: modelData.is_favourite
                        onScheduleClicked: (login) => scrollView.openStream(login)
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
                    objectName: "settingsButton"
                    text: "Settings"
                    onClicked: scrollView.settingsRequested()
                }

                Item { Layout.fillWidth: true }

                Controls.Button {
                    objectName: "logoutButton"
                    text: "Logout"
                    onClicked: scrollView.logoutRequested()
                }
            }
        }
    }
}
