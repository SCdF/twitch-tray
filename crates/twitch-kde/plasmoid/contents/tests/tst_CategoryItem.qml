import QtQuick
import QtTest
import ui

Item {
    width: 400
    height: 400

    CategoryItem {
        id: item
        width: parent.width
        categoryId: ""
        name: "Minecraft"
        boxArtUrl: ""
        totalViewersFormatted: "45k"
        streams: [
            { "user_login": "streamer1", "user_name": "Streamer One", "title": "Building a castle", "profile_image_url": "", "viewer_count_formatted": "10k", "duration_formatted": "2h 15m", "is_favourite": false },
            { "user_login": "streamer2", "user_name": "Streamer Two", "title": "Survival mode", "profile_image_url": "", "viewer_count_formatted": "5k", "duration_formatted": "45m", "is_favourite": false }
        ]
    }

    SignalSpy {
        id: clickSpy
        target: item
        signalName: "streamClicked"
    }

    TestCase {
        name: "CategoryItemTests"
        when: windowShown

        function init() {
            item.expanded = false
            item.categoryId = ""
            item.name = "Minecraft"
            item.boxArtUrl = ""
            clickSpy.clear()
        }

        function test_name_displayed() {
            var label = findChild(item, "nameLabel")
            verify(label, "nameLabel should exist")
            compare(label.text, "Minecraft")
        }

        function test_viewer_count_displayed() {
            var label = findChild(item, "viewerCountLabel")
            verify(label, "viewerCountLabel should exist")
            compare(label.text, "45k")
        }

        function test_icon_container_matches_avatar_size() {
            var container = findChild(item, "iconContainer")
            verify(container, "iconContainer should exist")
            compare(container.width, 40, "container width matches avatar size")
            compare(container.height, 40, "container height matches avatar size")

            var inner = findChild(item, "iconInner")
            verify(inner, "iconInner should exist")
            compare(inner.width, 30, "inner has box art width")
            compare(inner.height, 40, "inner has box art height")
        }

        function test_placeholder_shown_when_no_url() {
            item.boxArtUrl = ""
            wait(10)
            var placeholder = findChild(item, "iconPlaceholder")
            verify(placeholder, "iconPlaceholder should exist")
            verify(placeholder.visible, "placeholder visible when no URL")

            var image = findChild(item, "boxArtImage")
            verify(image, "boxArtImage should exist")
            verify(!image.visible, "image hidden when no URL")
        }

        function test_image_shown_when_url_set() {
            item.boxArtUrl = "https://example.com/boxart.jpg"
            wait(10)
            var image = findChild(item, "boxArtImage")
            verify(image.visible, "image visible when URL set")

            var placeholder = findChild(item, "iconPlaceholder")
            verify(!placeholder.visible, "placeholder hidden when URL set")
        }

        function test_box_art_url_bound_from_property() {
            item.boxArtUrl = "https://example.com/boxart-144x192.jpg"
            wait(10)
            var image = findChild(item, "boxArtImage")
            compare(image.source.toString(), "https://example.com/boxart-144x192.jpg")
        }

        function test_collapsed_by_default() {
            var list = findChild(item, "streamList")
            verify(list, "streamList should exist")
            verify(!list.visible, "stream list hidden when collapsed")
        }

        function test_expand_shows_streams() {
            item.expanded = true
            wait(10)
            var list = findChild(item, "streamList")
            verify(list.visible, "stream list visible when expanded")
        }

        function test_chevron_changes_on_expand() {
            var chevron = findChild(item, "chevron")
            verify(chevron, "chevron should exist")
            var collapsedText = chevron.text
            item.expanded = true
            wait(10)
            verify(chevron.text !== collapsedText, "chevron should change when expanded")
        }

        function test_click_header_toggles_expand() {
            var header = findChild(item, "categoryHeader")
            verify(header, "categoryHeader should exist")
            verify(!item.expanded, "should start collapsed")
            mouseClick(header)
            wait(10)
            verify(item.expanded, "should be expanded after click")
            mouseClick(header)
            wait(10)
            verify(!item.expanded, "should be collapsed after second click")
        }
    }
}
