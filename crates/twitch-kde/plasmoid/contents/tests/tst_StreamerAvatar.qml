import QtQuick
import QtTest
import ui

Item {
    width: 400
    height: 400

    StreamerAvatar {
        id: avatar
        profileImageUrl: ""
        displayName: "TestUser"
        isFavourite: false
    }

    TestCase {
        name: "StreamerAvatarTests"
        when: windowShown

        function init() {
            avatar.profileImageUrl = ""
            avatar.displayName = "TestUser"
            avatar.isFavourite = false
        }

        function test_default_size() {
            compare(avatar.width, 40)
            compare(avatar.height, 40)
        }

        function test_placeholder_shown_when_no_url() {
            var placeholder = findChild(avatar, "avatarPlaceholder")
            verify(placeholder, "avatarPlaceholder should exist")
            verify(placeholder.visible, "placeholder should be visible when no URL")

            var image = findChild(avatar, "avatarImage")
            verify(image, "avatarImage should exist")
            verify(!image.visible, "image should be hidden when no URL")
        }

        function test_image_shown_when_url_set() {
            avatar.profileImageUrl = "https://example.com/avatar.jpg"
            wait(10)
            var image = findChild(avatar, "avatarImage")
            verify(image.visible, "image should be visible when URL set")

            var placeholder = findChild(avatar, "avatarPlaceholder")
            verify(!placeholder.visible, "placeholder should be hidden when URL set")
        }

        function test_favourite_border() {
            avatar.isFavourite = true
            wait(10)
            compare(avatar.border.width, 2, "favourite should have 2px border")
        }

        function test_no_border_when_not_favourite() {
            avatar.isFavourite = false
            wait(10)
            compare(avatar.border.width, 0, "non-favourite should have no border")
        }

        function test_favourite_margins_on_image() {
            avatar.isFavourite = true
            avatar.profileImageUrl = "https://example.com/avatar.jpg"
            wait(10)
            var image = findChild(avatar, "avatarImage")
            compare(image.anchors.margins, 2, "image should have 2px margin for favourite border")
        }

        function test_no_margins_when_not_favourite() {
            avatar.isFavourite = false
            avatar.profileImageUrl = "https://example.com/avatar.jpg"
            wait(10)
            var image = findChild(avatar, "avatarImage")
            compare(image.anchors.margins, 0, "image should have no margin when not favourite")
        }
    }
}
