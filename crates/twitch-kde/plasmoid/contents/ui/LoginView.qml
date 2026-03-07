import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts

ColumnLayout {
    id: root

    property string loginState: "Idle"
    property string userCode: ""
    property string verificationUri: ""

    signal loginRequested()
    signal cancelLoginRequested()
    signal copyCodeRequested(string code)

    spacing: 8

    // Idle state: Login button
    Controls.Button {
        id: loginButton
        objectName: "loginButton"
        text: "Login to Twitch"
        visible: root.loginState === "Idle"
        Layout.alignment: Qt.AlignHCenter
        onClicked: root.loginRequested()
    }

    // PendingCode state: URI, code, copy
    ColumnLayout {
        visible: root.loginState === "PendingCode"
        spacing: 4
        Layout.fillWidth: true

        Controls.Label {
            objectName: "uriLabel"
            text: "Visit: " + root.verificationUri
            Layout.alignment: Qt.AlignHCenter
        }

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: 8

            Controls.Label {
                text: "Enter code:"
            }

            Controls.Label {
                id: codeLabel
                objectName: "codeLabel"
                text: root.userCode
                font.bold: true
                font.pixelSize: 16
            }

            Controls.Button {
                id: copyButton
                objectName: "copyButton"
                text: "Copy"
                onClicked: root.copyCodeRequested(root.userCode)
            }
        }

        Controls.Label {
            text: "Browser opened \u00B7 Waiting..."
            opacity: 0.7
            Layout.alignment: Qt.AlignHCenter
        }
    }

    // AwaitingConfirmation state: busy indicator
    ColumnLayout {
        visible: root.loginState === "AwaitingConfirmation"
        spacing: 4
        Layout.fillWidth: true

        Controls.BusyIndicator {
            id: busyIndicator
            objectName: "busyIndicator"
            running: root.loginState === "AwaitingConfirmation"
            Layout.alignment: Qt.AlignHCenter
        }

        Controls.Label {
            text: "Waiting for confirmation..."
            Layout.alignment: Qt.AlignHCenter
        }
    }

    // Cancel button shared between PendingCode and AwaitingConfirmation
    Controls.Button {
        objectName: "cancelButton"
        text: "Cancel"
        visible: root.loginState !== "Idle"
        Layout.alignment: Qt.AlignHCenter
        onClicked: root.cancelLoginRequested()
    }
}
