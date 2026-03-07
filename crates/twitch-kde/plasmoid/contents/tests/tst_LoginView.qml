import QtQuick
import QtTest
import ui

Item {
    width: 400
    height: 400

    LoginView {
        id: loginView
        loginState: "Idle"
        userCode: ""
        verificationUri: ""
    }

    SignalSpy {
        id: loginSpy
        target: loginView
        signalName: "loginRequested"
    }

    SignalSpy {
        id: cancelSpy
        target: loginView
        signalName: "cancelLoginRequested"
    }

    SignalSpy {
        id: copySpy
        target: loginView
        signalName: "copyCodeRequested"
    }

    TestCase {
        name: "LoginViewTests"
        when: windowShown

        function init() {
            loginView.loginState = "Idle"
            loginView.userCode = ""
            loginView.verificationUri = ""
            loginSpy.clear()
            cancelSpy.clear()
            copySpy.clear()
        }

        function test_login_button_shown_when_idle() {
            loginView.loginState = "Idle"
            wait(10)
            var loginBtn = findChild(loginView, "loginButton")
            verify(loginBtn, "loginButton should exist")
            verify(loginBtn.visible, "login button should be visible when idle")
        }

        function test_code_and_copy_shown_when_pending() {
            loginView.loginState = "PendingCode"
            loginView.userCode = "ABCD-1234"
            loginView.verificationUri = "https://twitch.tv/activate"
            wait(10)
            var codeLabel = findChild(loginView, "codeLabel")
            verify(codeLabel, "codeLabel should exist")
            compare(codeLabel.text, "ABCD-1234")

            var copyBtn = findChild(loginView, "copyButton")
            verify(copyBtn, "copyButton should exist")
            verify(copyBtn.visible, "copy button should be visible")
        }

        function test_copy_button_emits_code() {
            loginView.loginState = "PendingCode"
            loginView.userCode = "ABCD-1234"
            wait(10)
            var copyBtn = findChild(loginView, "copyButton")
            mouseClick(copyBtn)
            compare(copySpy.count, 1)
            compare(copySpy.signalArguments[0][0], "ABCD-1234")
        }

        function test_busy_indicator_shown_when_awaiting() {
            loginView.loginState = "AwaitingConfirmation"
            wait(10)
            var busy = findChild(loginView, "busyIndicator")
            verify(busy, "busyIndicator should exist")
            verify(busy.visible, "busy indicator should be visible")
        }

        function test_cancel_button_emits_when_pending() {
            loginView.loginState = "PendingCode"
            loginView.userCode = "ABCD-1234"
            wait(10)
            var cancelBtn = findChild(loginView, "cancelButton")
            verify(cancelBtn, "cancelButton should exist")
            mouseClick(cancelBtn)
            compare(cancelSpy.count, 1)
        }

        function test_cancel_button_emits_when_awaiting() {
            loginView.loginState = "AwaitingConfirmation"
            wait(10)
            var cancelBtn = findChild(loginView, "cancelButton")
            verify(cancelBtn, "cancelButton should exist")
            mouseClick(cancelBtn)
            compare(cancelSpy.count, 1)
        }

        function test_login_button_emits_login_requested() {
            loginView.loginState = "Idle"
            wait(10)
            var loginBtn = findChild(loginView, "loginButton")
            mouseClick(loginBtn)
            compare(loginSpy.count, 1)
        }
    }
}
