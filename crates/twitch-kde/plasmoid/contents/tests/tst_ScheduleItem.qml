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
        title: "Evening Chill Stream"
        category: "Just Chatting"
        profileImageUrl: ""
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
            item.title = "Evening Chill Stream"
            item.category = "Just Chatting"
            item.profileImageUrl = ""
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

        function test_category_displayed() {
            var label = findChild(item, "categoryLabel")
            verify(label, "categoryLabel should exist")
            compare(label.text, "\u00B7 Just Chatting")
        }

        function test_category_hidden_when_empty() {
            item.category = ""
            wait(10)
            var label = findChild(item, "categoryLabel")
            verify(label, "categoryLabel should exist")
            compare(label.text, "")
        }

        function test_title_displayed() {
            var label = findChild(item, "titleLabel")
            verify(label, "titleLabel should exist")
            compare(label.text, "Evening Chill Stream")
            compare(label.opacity, 0.5, "title should be visible when non-empty")
        }

        function test_title_transparent_when_empty() {
            item.title = ""
            wait(10)
            var label = findChild(item, "titleLabel")
            verify(label, "titleLabel should exist")
            compare(label.opacity, 0, "title should be transparent when empty")
        }

        function test_avatar_container_exists() {
            var avatar = findChild(item, "avatarContainer")
            verify(avatar, "avatarContainer should exist")
        }

        function test_avatar_placeholder_shown_when_no_url() {
            item.profileImageUrl = ""
            wait(10)
            var placeholder = findChild(item, "avatarPlaceholder")
            verify(placeholder, "avatarPlaceholder should exist")
            verify(placeholder.visible, "placeholder should be visible when no URL")
        }

        function test_avatar_image_shown_when_url_set() {
            item.profileImageUrl = "https://example.com/avatar.jpg"
            wait(10)
            var img = findChild(item, "avatarImage")
            verify(img, "avatarImage should exist")
            verify(img.visible, "image should be visible when URL is set")
        }

        function test_favourite_shows_border_on_avatar() {
            item.isFavourite = true
            wait(10)
            var avatar = findChild(item, "avatarContainer")
            verify(avatar, "avatarContainer should exist")
            compare(avatar.border.width, 2)
        }

        function test_not_favourite_no_border() {
            item.isFavourite = false
            wait(10)
            var avatar = findChild(item, "avatarContainer")
            verify(avatar, "avatarContainer should exist")
            compare(avatar.border.width, 0)
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

        function test_click_emits_broadcaster_login() {
            mouseClick(item)
            compare(clickSpy.count, 1)
            compare(clickSpy.signalArguments[0][0], "streamer1")
        }
    }
}
