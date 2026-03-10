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
        title: "Competitive ranked grind"
        profileImageUrl: ""
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
            item.profileImageUrl = ""
            item.title = "Competitive ranked grind"
            clickSpy.clear()
        }

        function test_user_name_displayed() {
            var label = findChild(item, "nameLabel")
            verify(label, "nameLabel should exist")
            compare(label.text, "Streamer One")
        }

        function test_game_name_beside_user_name() {
            var label = findChild(item, "subtitleLabel")
            verify(label, "subtitleLabel should exist")
            compare(label.text, "\u00B7 Overwatch 2")
        }

        function test_title_displayed_below_name() {
            var clip = findChild(item, "titleLabel")
            verify(clip, "titleLabel should exist")
            compare(clip.text, "Competitive ranked grind")
            verify(clip.visible, "title should be visible when set")
        }

        function test_title_hidden_when_empty() {
            item.title = ""
            wait(10)
            var clip = findChild(item, "titleLabel")
            verify(clip, "titleLabel should exist")
            verify(!clip.visible, "title should be hidden when empty")
        }

        function test_viewer_count_displayed() {
            var label = findChild(item, "topRightLabel")
            verify(label, "topRightLabel should exist")
            compare(label.text, "1.2k")
        }

        function test_duration_displayed() {
            var label = findChild(item, "bottomRightLabel")
            verify(label, "bottomRightLabel should exist")
            compare(label.text, "2h 15m")
        }

        function test_avatar_container_exists() {
            var container = findChild(item, "avatarContainer")
            verify(container, "avatarContainer should exist")
            compare(container.width, 40)
            compare(container.height, 40)
        }

        function test_avatar_placeholder_shown_when_no_url() {
            item.profileImageUrl = ""
            wait(10)
            var placeholder = findChild(item, "avatarPlaceholder")
            verify(placeholder, "avatarPlaceholder should exist")
            verify(placeholder.visible, "placeholder should be visible when no URL")

            var image = findChild(item, "avatarImage")
            verify(image, "avatarImage should exist")
            verify(!image.visible, "image should be hidden when no URL")
        }

        function test_avatar_image_shown_when_url_set() {
            item.profileImageUrl = "https://example.com/avatar.jpg"
            wait(10)
            var masked = findChild(item, "maskedAvatar")
            verify(masked, "maskedAvatar should exist")
            verify(masked.visible, "masked avatar should be visible when URL set")

            var placeholder = findChild(item, "avatarPlaceholder")
            verify(!placeholder.visible, "placeholder should be hidden when URL set")
        }

        function test_favourite_shows_border_on_avatar() {
            item.isFavourite = true
            wait(10)
            var container = findChild(item, "avatarContainer")
            verify(container, "avatarContainer should exist")
            compare(container.border.width, 2, "favourite should have 2px border")
        }

        function test_not_favourite_no_border() {
            item.isFavourite = false
            wait(10)
            var container = findChild(item, "avatarContainer")
            verify(container, "avatarContainer should exist")
            compare(container.border.width, 0, "non-favourite should have no border")
        }

        function test_click_emits_user_login() {
            mouseClick(item)
            compare(clickSpy.count, 1)
            compare(clickSpy.signalArguments[0][0], "streamer1")
        }
    }
}
