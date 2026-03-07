import QtQuick
import QtTest
import ui

Item {
    width: 400
    height: 400

    CompactRepresentation {
        id: compact
        daemonRunning: false
        authenticated: false
    }

    TestCase {
        name: "CompactRepresentationTests"
        when: windowShown

        function init() {
            compact.daemonRunning = false
            compact.authenticated = false
        }

        function test_icon_always_visible() {
            var icon = findChild(compact, "twitchIcon")
            verify(icon, "twitchIcon should exist")
            verify(icon.visible, "icon should always be visible")
        }

        function test_icon_dimmed_when_daemon_not_running() {
            compact.daemonRunning = false
            wait(10)
            var icon = findChild(compact, "twitchIcon")
            compare(icon.opacity, 0.4)
        }

        function test_icon_dimmed_when_not_authenticated() {
            compact.daemonRunning = true
            compact.authenticated = false
            wait(10)
            var icon = findChild(compact, "twitchIcon")
            compare(icon.opacity, 0.4)
        }

        function test_icon_full_opacity_when_authenticated() {
            compact.daemonRunning = true
            compact.authenticated = true
            wait(10)
            var icon = findChild(compact, "twitchIcon")
            compare(icon.opacity, 1.0)
        }
    }
}
