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
        totalViewersFormatted: "45k"
        streamCountFormatted: "12 live"
        streams: [
            { "user_login": "streamer1", "user_name": "Streamer One", "viewer_count_formatted": "10k" },
            { "user_login": "streamer2", "user_name": "Streamer Two", "viewer_count_formatted": "5k" }
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

        function test_stream_count_displayed() {
            var label = findChild(item, "streamCountLabel")
            verify(label, "streamCountLabel should exist")
            compare(label.text, "12 live")
        }

        function test_icon_container_exists() {
            var container = findChild(item, "iconContainer")
            verify(container, "iconContainer should exist")
            compare(container.width, 40)
            compare(container.height, 40)
        }

        function test_placeholder_shown_when_no_id() {
            item.categoryId = ""
            wait(10)
            var placeholder = findChild(item, "iconPlaceholder")
            verify(placeholder, "iconPlaceholder should exist")
            verify(placeholder.visible, "placeholder visible when no ID")

            var image = findChild(item, "boxArtImage")
            verify(image, "boxArtImage should exist")
            verify(!image.visible, "image hidden when no ID")
        }

        function test_image_shown_when_id_set() {
            item.categoryId = "27471"
            wait(10)
            var image = findChild(item, "boxArtImage")
            verify(image.visible, "image visible when ID set")

            var placeholder = findChild(item, "iconPlaceholder")
            verify(!placeholder.visible, "placeholder hidden when ID set")
        }

        function test_box_art_url_constructed_from_id() {
            item.categoryId = "509658"
            wait(10)
            verify(item.boxArtUrl.indexOf("https://static-cdn.jtvnw.net/ttv-boxart/509658-") === 0,
                   "URL should start with CDN base + category ID, got: " + item.boxArtUrl)
            verify(item.boxArtUrl.indexOf(".jpg") > 0, "URL should end with .jpg")
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
