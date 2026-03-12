import QtQuick
import QtQuick.Controls as Controls
import Qt5Compat.GraphicalEffects

Rectangle {
    id: root

    property string profileImageUrl: ""
    property string displayName: ""
    property bool isFavourite: false
    property bool isHot: false

    width: 40
    height: 40
    radius: width / 2
    color: "transparent"
    border.width: (root.isHot || root.isFavourite) ? 2 : 0
    border.color: root.isHot ? "transparent" : (root.isFavourite ? root.palette.highlight : "transparent")

    // Animated swirling gradient ring for hot streams
    Rectangle {
        id: hotRingSource
        objectName: "hotRingSource"
        anchors.fill: parent
        visible: false
        radius: width / 2
        color: "transparent"
        border.width: 2
        border.color: "white"
    }

    ConicalGradient {
        id: hotRing
        objectName: "hotRing"
        anchors.fill: parent
        visible: root.isHot
        angle: hotRotation.angle
        source: hotRingSource
        gradient: Gradient {
            GradientStop { position: 0.0; color: "#FF4500" }
            GradientStop { position: 0.25; color: "#FFD700" }
            GradientStop { position: 0.5; color: "#FF6347" }
            GradientStop { position: 0.75; color: "#FFD700" }
            GradientStop { position: 1.0; color: "#FF4500" }
        }
    }

    QtObject {
        id: hotRotation
        property real angle: 0
    }

    NumberAnimation {
        target: hotRotation
        property: "angle"
        from: 0
        to: 360
        duration: 2000
        loops: Animation.Infinite
        running: root.isHot
    }

    Image {
        id: avatarImage
        objectName: "avatarImage"
        anchors.fill: parent
        anchors.margins: (root.isHot || root.isFavourite) ? 2 : 0
        source: root.profileImageUrl
        fillMode: Image.PreserveAspectCrop
        smooth: true
        mipmap: true
        visible: false
    }

    OpacityMask {
        objectName: "maskedAvatar"
        anchors.fill: avatarImage
        source: avatarImage
        maskSource: Rectangle {
            width: avatarImage.width
            height: avatarImage.height
            radius: width / 2
        }
        visible: root.profileImageUrl !== ""
    }

    Rectangle {
        id: avatarPlaceholder
        objectName: "avatarPlaceholder"
        anchors.fill: parent
        anchors.margins: (root.isHot || root.isFavourite) ? 2 : 0
        radius: width / 2
        color: root.palette.mid
        visible: root.profileImageUrl === ""

        Controls.Label {
            anchors.centerIn: parent
            text: root.displayName.charAt(0)
            font.bold: true
        }
    }
}
