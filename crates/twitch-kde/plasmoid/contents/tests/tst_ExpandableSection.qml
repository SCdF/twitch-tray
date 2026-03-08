import QtQuick
import QtTest
import ui

Item {
    width: 400
    height: 400

    property var testItems: [
        { "user_login": "alice", "user_name": "Alice", "profile_image_url": "", "is_favourite": false },
        { "user_login": "bob", "user_name": "Bob", "profile_image_url": "", "is_favourite": true },
        { "user_login": "carol", "user_name": "Carol", "profile_image_url": "", "is_favourite": false }
    ]

    ExpandableSection {
        id: section
        width: 400
        items: testItems

        Text {
            id: childContent
            objectName: "childContent"
            text: "hidden content"
        }
    }

    SignalSpy { id: avatarSpy; target: section; signalName: "avatarClicked" }

    TestCase {
        name: "ExpandableSectionTests"
        when: windowShown

        function init() {
            section.expanded = false
            section.items = testItems
            avatarSpy.clear()
        }

        function test_collapsed_by_default() {
            compare(section.expanded, false)
        }

        function test_avatar_row_visible_when_collapsed() {
            var delegate = findChild(section, "avatarDelegate")
            verify(delegate, "avatarDelegate should exist")
            verify(delegate.visible, "avatar row should be visible when collapsed")
        }

        function test_avatar_row_hidden_when_expanded() {
            section.expanded = true
            wait(10)
            var delegate = findChild(section, "avatarDelegate")
            verify(!delegate.visible, "avatar row should be hidden when expanded")
        }

        function test_avatar_count_matches_items() {
            var avatarList = findChild(section, "avatarList")
            verify(avatarList, "avatarList should exist")
            compare(avatarList.children.length - 1, 3, "should have 3 avatars (minus repeater)")
        }

        function test_click_row_expands() {
            var delegate = findChild(section, "avatarDelegate")
            // Click far right of the delegate (not on an avatar)
            mouseClick(delegate, delegate.width - 10, delegate.height / 2)
            compare(section.expanded, true)
        }

        function test_click_collapse_row_collapses() {
            section.expanded = true
            wait(10)
            var delegate = findChild(section, "collapseDelegate")
            verify(delegate, "collapseDelegate should exist")
            mouseClick(delegate)
            compare(section.expanded, false)
        }

        function test_child_hidden_when_collapsed() {
            section.expanded = false
            wait(10)
            var content = findChild(section, "contentColumn")
            verify(!content.visible, "content should be hidden when collapsed")
        }

        function test_child_visible_when_expanded() {
            section.expanded = true
            wait(10)
            var content = findChild(section, "contentColumn")
            verify(content.visible, "content should be visible when expanded")
        }

        function test_avatar_click_emits_signal() {
            var avatarList = findChild(section, "avatarList")
            var firstAvatar = avatarList.children[0]
            verify(firstAvatar, "first avatar should exist")
            mouseClick(firstAvatar)
            compare(avatarSpy.count, 1, "avatarClicked should be emitted once")
            compare(avatarSpy.signalArguments[0][0], 0, "should emit index 0")
        }

        function test_avatar_click_does_not_expand() {
            var avatarList = findChild(section, "avatarList")
            var firstAvatar = avatarList.children[0]
            mouseClick(firstAvatar)
            compare(section.expanded, false, "clicking avatar should not expand")
        }

        function test_hidden_when_no_items() {
            section.items = []
            wait(10)
            var delegate = findChild(section, "avatarDelegate")
            verify(!delegate.visible, "avatar row hidden when no items")
        }
    }
}
