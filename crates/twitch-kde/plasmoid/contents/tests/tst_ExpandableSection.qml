import QtQuick
import QtTest
import ui

Item {
    width: 400
    height: 400

    ExpandableSection {
        id: section
        heading: "More"
        count: 3

        Text {
            id: childContent
            objectName: "childContent"
            text: "hidden content"
        }
    }

    TestCase {
        name: "ExpandableSectionTests"
        when: windowShown

        function init() {
            section.expanded = false
        }

        function test_collapsed_by_default() {
            compare(section.expanded, false)
        }

        function test_click_header_expands() {
            var headerArea = findChild(section, "headerArea")
            verify(headerArea, "headerArea should exist")
            mouseClick(headerArea)
            compare(section.expanded, true)
        }

        function test_click_again_collapses() {
            section.expanded = true
            var headerArea = findChild(section, "headerArea")
            mouseClick(headerArea)
            compare(section.expanded, false)
        }

        function test_heading_text_includes_count() {
            var headerLabel = findChild(section, "headerLabel")
            verify(headerLabel, "headerLabel should exist")
            verify(headerLabel.text.indexOf("More") >= 0, "should contain heading text")
            verify(headerLabel.text.indexOf("3") >= 0, "should contain count")
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
    }
}
