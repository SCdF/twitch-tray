import QtQuick
import QtTest
import ui

Item {
    width: 400
    height: 400

    CompactRepresentation {
        id: compact
        liveCount: 0
    }

    TestCase {
        name: "CompactRepresentationTests"
        when: windowShown

        function init() {
            compact.liveCount = 0
        }

        function test_no_badge_when_zero_live_streams() {
            compact.liveCount = 0
            wait(10)
            var badge = findChild(compact, "badge")
            verify(badge, "badge element should exist")
            verify(!badge.visible, "badge should be hidden when no live streams")
        }

        function test_badge_shows_correct_live_count() {
            compact.liveCount = 7
            wait(10)
            var badge = findChild(compact, "badge")
            verify(badge, "badge element should exist")
            verify(badge.visible, "badge should be visible when streams are live")
            var badgeText = findChild(compact, "badgeText")
            verify(badgeText, "badgeText should exist")
            compare(badgeText.text, "7")
        }

        function test_icon_always_visible() {
            var icon = findChild(compact, "twitchIcon")
            verify(icon, "twitchIcon should exist")
            verify(icon.visible, "icon should always be visible")
        }
    }
}
