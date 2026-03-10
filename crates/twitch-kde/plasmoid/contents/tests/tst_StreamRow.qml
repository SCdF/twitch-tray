import QtQuick
import QtTest
import ui

Item {
    width: 400
    height: 400

    StreamRow {
        id: row
        login: "testuser"
        displayName: "Test User"
        subtitle: "Gaming"
        title: "Playing something fun"
        profileImageUrl: ""
        topRightText: "1.2k"
        bottomRightText: "2h 15m"
        bottomRightItalic: false
        isFavourite: false
    }

    SignalSpy {
        id: clickSpy
        target: row
        signalName: "clicked_"
    }

    TestCase {
        name: "StreamRowTests"
        when: windowShown

        function init() {
            row.login = "testuser"
            row.displayName = "Test User"
            row.subtitle = "Gaming"
            row.title = "Playing something fun"
            row.profileImageUrl = ""
            row.topRightText = "1.2k"
            row.bottomRightText = "2h 15m"
            row.bottomRightItalic = false
            row.isFavourite = false
            clickSpy.clear()
        }

        function test_name_displayed() {
            var label = findChild(row, "nameLabel")
            verify(label, "nameLabel should exist")
            compare(label.text, "Test User")
        }

        function test_subtitle_with_separator() {
            var label = findChild(row, "subtitleLabel")
            verify(label, "subtitleLabel should exist")
            compare(label.text, "\u00B7 Gaming")
        }

        function test_subtitle_hidden_when_empty() {
            row.subtitle = ""
            wait(10)
            var label = findChild(row, "subtitleLabel")
            compare(label.text, "")
        }

        function test_top_right_text_displayed() {
            var label = findChild(row, "topRightLabel")
            verify(label, "topRightLabel should exist")
            compare(label.text, "1.2k")
        }

        function test_title_displayed() {
            var label = findChild(row, "titleLabel")
            verify(label, "titleLabel should exist")
            compare(label.text, "Playing something fun")
            verify(label.visible, "title should be visible")
        }

        function test_title_hidden_when_empty() {
            row.title = ""
            row.bottomRightText = ""
            wait(10)
            var label = findChild(row, "titleLabel")
            verify(!label.visible, "title should be hidden when empty")
        }

        function test_bottom_right_text_displayed() {
            var label = findChild(row, "bottomRightLabel")
            verify(label, "bottomRightLabel should exist")
            compare(label.text, "2h 15m")
            verify(label.visible, "bottom right should be visible when set")
        }

        function test_bottom_right_hidden_when_empty() {
            row.bottomRightText = ""
            wait(10)
            var label = findChild(row, "bottomRightLabel")
            verify(!label.visible, "bottom right should be hidden when empty")
        }

        function test_bottom_right_italic() {
            row.bottomRightItalic = true
            wait(10)
            var label = findChild(row, "bottomRightLabel")
            verify(label.font.italic, "should be italic when bottomRightItalic is true")
        }

        function test_bottom_right_not_italic_by_default() {
            var label = findChild(row, "bottomRightLabel")
            verify(!label.font.italic, "should not be italic by default")
        }

        function test_avatar_exists() {
            var avatar = findChild(row, "avatarContainer")
            verify(avatar, "avatarContainer should exist")
            compare(avatar.width, 40)
            compare(avatar.height, 40)
        }

        function test_favourite_border() {
            row.isFavourite = true
            wait(10)
            var avatar = findChild(row, "avatarContainer")
            compare(avatar.border.width, 2)
        }

        function test_not_favourite_no_border() {
            var avatar = findChild(row, "avatarContainer")
            compare(avatar.border.width, 0)
        }

        function test_click_emits_login() {
            mouseClick(row)
            compare(clickSpy.count, 1)
            compare(clickSpy.signalArguments[0][0], "testuser")
        }

        function test_second_row_visible_when_only_bottom_right_set() {
            row.title = ""
            row.bottomRightText = "(inferred)"
            wait(10)
            var label = findChild(row, "bottomRightLabel")
            verify(label.visible, "bottom right should be visible even without title")
        }
    }
}
