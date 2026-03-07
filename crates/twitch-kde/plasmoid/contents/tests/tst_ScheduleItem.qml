import QtQuick
import QtTest
import ui

Item {
    width: 400
    height: 400

    ScheduleItem {
        id: item
        broadcasterLogin: "streamer1"
        broadcasterName: "Streamer One"
        startTimeFormatted: "Today 8:00 PM"
        isInferred: false
        isFavourite: false
    }

    SignalSpy {
        id: clickSpy
        target: item
        signalName: "scheduleClicked"
    }

    TestCase {
        name: "ScheduleItemTests"
        when: windowShown

        function init() {
            item.isInferred = false
            item.isFavourite = false
            clickSpy.clear()
        }

        function test_broadcaster_name_displayed() {
            var label = findChild(item, "broadcasterNameLabel")
            verify(label, "broadcasterNameLabel should exist")
            compare(label.text, "Streamer One")
        }

        function test_start_time_displayed() {
            var label = findChild(item, "startTimeLabel")
            verify(label, "startTimeLabel should exist")
            compare(label.text, "Today 8:00 PM")
        }

        function test_sparkle_visible_when_inferred() {
            item.isInferred = true
            wait(10)
            var sparkle = findChild(item, "inferredIndicator")
            verify(sparkle, "inferredIndicator should exist")
            verify(sparkle.visible, "sparkle should be visible when inferred")
        }

        function test_sparkle_hidden_when_not_inferred() {
            item.isInferred = false
            wait(10)
            var sparkle = findChild(item, "inferredIndicator")
            verify(sparkle, "inferredIndicator should exist")
            verify(!sparkle.visible, "sparkle should be hidden when not inferred")
        }

        function test_star_visible_when_favourite() {
            item.isFavourite = true
            wait(10)
            var star = findChild(item, "favouriteStar")
            verify(star, "favouriteStar should exist")
            verify(star.visible, "star should be visible when favourite")
        }

        function test_click_emits_broadcaster_login() {
            mouseClick(item)
            compare(clickSpy.count, 1)
            compare(clickSpy.signalArguments[0][0], "streamer1")
        }
    }
}
