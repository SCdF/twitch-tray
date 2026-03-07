import QtQuick
import QtTest
import org.kde.kirigami as Kirigami
import ui

Item {
    width: 400
    height: 400

    SectionHeader {
        id: header
        text: "Following Live (5)"
    }

    TestCase {
        name: "SectionHeaderTests"
        when: windowShown

        function test_heading_text_rendered() {
            var heading = findChild(header, "heading")
            verify(heading, "heading element should exist")
            compare(heading.text, "Following Live (5)")
        }

        function test_separator_present() {
            var separator = findChild(header, "separator")
            verify(separator, "separator element should exist")
        }
    }
}
