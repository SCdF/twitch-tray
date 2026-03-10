import QtQuick
import QtTest
import ui

Item {
    width: 400
    height: 600

    property var liveState: ({
        "authenticated": true,
        "login_state": { "type": "Idle" },
        "live": {
            "visible": [
                {
                    "user_login": "streamer1",
                    "user_name": "Streamer One",
                    "game_name": "Overwatch 2",
                    "title": "Competitive ranked grind",
                    "profile_image_url": "https://example.com/avatar1.jpg",
                    "viewer_count_formatted": "1.2k",
                    "duration_formatted": "2h 15m",
                    "is_favourite": true
                },
                {
                    "user_login": "streamer2",
                    "user_name": "Streamer Two",
                    "game_name": "Minecraft",
                    "title": "Building a castle",
                    "profile_image_url": "",
                    "viewer_count_formatted": "500",
                    "duration_formatted": "45m",
                    "is_favourite": false
                }
            ],
            "overflow": []
        },
        "categories": [],
        "schedule": {
            "lookahead_hours": 24,
            "loaded": true,
            "visible": [
                {
                    "broadcaster_login": "streamer3",
                    "broadcaster_name": "Streamer Three",
                    "start_time_formatted": "Today 8:00 PM",
                    "is_inferred": false,
                    "is_favourite": false
                }
            ],
            "overflow": []
        }
    })

    FullRepresentation {
        id: rep
        anchors.fill: parent
        daemonRunning: false
        plasmoidState: liveState
    }

    SignalSpy { id: loginSpy; target: rep; signalName: "loginRequested" }
    SignalSpy { id: logoutSpy; target: rep; signalName: "logoutRequested" }
    SignalSpy { id: settingsSpy; target: rep; signalName: "settingsRequested" }
    SignalSpy { id: streamSpy; target: rep; signalName: "openStream" }

    TestCase {
        name: "FullRepresentationTests"
        when: windowShown

        function init() {
            rep.daemonRunning = false
            rep.plasmoidState = liveState
            loginSpy.clear()
            logoutSpy.clear()
            settingsSpy.clear()
            streamSpy.clear()
        }

        function test_component_loads() {
            var col = findChild(rep, "fullColumn")
            verify(col, "fullColumn should exist — FullRepresentation loaded successfully")
        }

        function test_daemon_not_running_shown() {
            rep.daemonRunning = false
            wait(10)
            var label = findChild(rep, "daemonNotRunning")
            verify(label, "daemonNotRunning label should exist")
            verify(label.visible, "should be visible when daemon not running")
        }

        function test_daemon_not_running_hides_authenticated() {
            rep.daemonRunning = false
            wait(10)
            var auth = findChild(rep, "authenticatedContent")
            verify(auth, "authenticatedContent should exist")
            verify(!auth.visible, "authenticated content hidden when daemon not running")
        }

        function test_login_view_shown_when_unauthenticated() {
            rep.daemonRunning = true
            rep.plasmoidState = {
                "authenticated": false,
                "login_state": { "type": "Idle" },
                "live": { "visible": [], "overflow": [] },
                "categories": [],
                "schedule": { "lookahead_hours": 24, "loaded": true, "visible": [], "overflow": [] }
            }
            wait(10)
            var login = findChild(rep, "loginView")
            verify(login, "loginView should exist")
            verify(login.visible, "login view shown when unauthenticated")
        }

        function test_authenticated_shows_live_streams() {
            rep.daemonRunning = true
            wait(10)
            var header = findChild(rep, "liveHeader")
            verify(header, "liveHeader should exist")
            var heading = findChild(header, "heading")
            compare(heading.text, "Following Live (2)")
        }

        function test_authenticated_shows_schedule() {
            rep.daemonRunning = true
            wait(10)
            var header = findChild(rep, "scheduleHeader")
            verify(header, "scheduleHeader should exist")
            var heading = findChild(header, "heading")
            compare(heading.text, "Scheduled (Next 24h)")
        }

        function test_authenticated_shows_action_buttons() {
            rep.daemonRunning = true
            wait(10)
            var settings = findChild(rep, "settingsButton")
            var logout = findChild(rep, "logoutButton")
            verify(settings, "settings button should exist")
            verify(logout, "logout button should exist")
        }

        function test_logout_signal() {
            rep.daemonRunning = true
            wait(50)
            var btn = findChild(rep, "logoutButton")
            verify(btn, "logout button should exist")
            verify(btn.visible, "logout button should be visible")
            mouseClick(btn)
            compare(logoutSpy.count, 1)
        }

        function test_settings_signal() {
            rep.daemonRunning = true
            wait(50)
            var btn = findChild(rep, "settingsButton")
            verify(btn, "settings button should exist")
            verify(btn.visible, "settings button should be visible")
            mouseClick(btn)
            compare(settingsSpy.count, 1)
        }

        function test_full_column_has_nonzero_width() {
            rep.daemonRunning = true
            wait(10)
            var col = findChild(rep, "fullColumn")
            verify(col.width > 0, "fullColumn width should be > 0, was: " + col.width)
        }

        function test_categories_header_hidden_when_empty() {
            rep.daemonRunning = true
            wait(10)
            var header = findChild(rep, "categoriesHeader")
            verify(header, "categoriesHeader should exist")
            verify(!header.visible, "categories header hidden when no categories")
        }

        function test_categories_header_shown_when_present() {
            rep.daemonRunning = true
            rep.plasmoidState = {
                "authenticated": true,
                "login_state": { "type": "Idle" },
                "live": { "visible": [], "overflow": [] },
                "categories": [
                    {
                        "id": "27471",
                        "name": "Minecraft",
                        "box_art_url": "https://example.com/mc-144x192.jpg",
                        "total_viewers_formatted": "45k",
                        "streams": [
                            { "user_login": "s1", "user_name": "S1", "title": "Building", "profile_image_url": "", "viewer_count_formatted": "10k", "duration_formatted": "1h 30m" }
                        ]
                    }
                ],
                "schedule": { "lookahead_hours": 24, "loaded": true, "visible": [], "overflow": [] }
            }
            wait(10)
            var header = findChild(rep, "categoriesHeader")
            verify(header, "categoriesHeader should exist")
            verify(header.visible, "categories header shown when categories present")
            var heading = findChild(header, "heading")
            compare(heading.text, "Categories")
        }
    }
}
