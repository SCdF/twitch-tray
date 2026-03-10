import QtQuick

StreamRow {
    id: root

    property string userLogin: ""
    property string userName: ""
    property string gameName: ""
    // title: inherited
    // profileImageUrl: inherited
    property string viewerCountFormatted: ""
    property string durationFormatted: ""
    // isFavourite: inherited

    signal streamClicked(string userLogin)

    login: root.userLogin
    displayName: root.userName
    subtitle: root.gameName
    topRightText: root.viewerCountFormatted
    bottomRightText: root.durationFormatted

    onClicked_: (login) => root.streamClicked(login)
}
