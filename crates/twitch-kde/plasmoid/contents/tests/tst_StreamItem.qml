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
        signalName: "streamClicked"
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

        function test_game_name_displayed() {
            var label = findChild(item, "gameNameLabel")
            verify(label, "gameNameLabel should exist")
            compare(label.text, "Overwatch 2")
        }

        function test_viewer_count_displayed() {
            var label = findChild(item, "viewerCountLabel")
            verify(label, "viewerCountLabel should exist")
            compare(label.text, "1.2k")
        }

        function test_duration_displayed() {
            var label = findChild(item, "durationLabel")
            verify(label, "durationLabel should exist")
            compare(label.text, "2h 15m")
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
            mouseClick(item)
            compare(clickSpy.count, 1)
            compare(clickSpy.signalArguments[0][0], "streamer1")
        }
    }
}
