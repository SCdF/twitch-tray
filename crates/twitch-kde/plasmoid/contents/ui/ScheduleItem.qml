import QtQuick

StreamRow {
    id: root

    property string broadcasterLogin: ""
    property string broadcasterName: ""
    property string startTimeFormatted: ""
    // title: inherited
    property string category: ""
    // profileImageUrl: inherited
    property bool isInferred: false
    // isFavourite: inherited

    signal scheduleClicked(string broadcasterLogin)

    login: root.broadcasterLogin
    displayName: root.broadcasterName
    subtitle: root.category
    topRightText: root.startTimeFormatted
    bottomRightText: root.isInferred ? qsTr("(inferred)") : ""
    bottomRightItalic: true

    onClicked_: (login) => root.scheduleClicked(login)
}
