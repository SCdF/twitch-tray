import QtQuick
import QtTest
import ui

Item {
    width: 400
    height: 400

    StreamItem {
        id: item
        userLogin: "streamer1"
        userName: "Streamer One"
        gameName: "Overwatch 2"
        viewerCountFormatted: "1.2k"
        durationFormatted: "2h 15m"
        isFavourite: false
    }

    SignalSpy {
        id: clickSpy
        target: item
        signalName: "clicked"
    }

    TestCase {
        name: "StreamItemTests"
        when: windowShown

        function init() {
            item.isFavourite = false
            clickSpy.clear()
        }

        function test_user_name_displayed() {
            var label = findChild(item, "userNameLabel")
            verify(label, "userNameLabel should exist")
            compare(label.text, "Streamer One")
        }

        function test_subtitle_contains_game_and_viewers() {
            var subtitle = findChild(item, "subtitleLabel")
            verify(subtitle, "subtitleLabel should exist")
            verify(subtitle.text.indexOf("Overwatch 2") >= 0, "should contain game name")
            verify(subtitle.text.indexOf("1.2k") >= 0, "should contain viewer count")
            verify(subtitle.text.indexOf("2h 15m") >= 0, "should contain duration")
        }

        function test_star_visible_when_favourite() {
            item.isFavourite = true
            wait(10)
            var star = findChild(item, "favouriteStar")
            verify(star, "favouriteStar should exist")
            verify(star.visible, "star should be visible when favourite")
        }

        function test_star_hidden_when_not_favourite() {
            item.isFavourite = false
            wait(10)
            var star = findChild(item, "favouriteStar")
            verify(star, "favouriteStar should exist")
            verify(!star.visible, "star should be hidden when not favourite")
        }

        function test_click_emits_user_login() {
            var clickArea = findChild(item, "clickArea")
            verify(clickArea, "clickArea should exist")
            mouseClick(clickArea)
            compare(clickSpy.count, 1)
            compare(clickSpy.signalArguments[0][0], "streamer1")
        }
    }
}
