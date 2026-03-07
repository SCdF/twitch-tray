import QtQuick

QtObject {
    property var calls: []

    property var state: ({
        "authenticated": false,
        "login_state": { "type": "Idle" },
        "live": { "visible": [], "overflow": [] },
        "categories": [],
        "schedule": { "lookahead_hours": 24, "loaded": true, "visible": [], "overflow": [] }
    })

    function login() { calls.push("login") }
    function logout() { calls.push("logout") }
    function openStream(userLogin) { calls.push("openStream:" + userLogin) }
    function openSettings() { calls.push("openSettings") }
    function openStreamerSettings(userLogin, displayName) { calls.push("openStreamerSettings:" + userLogin + ":" + displayName) }
    function cancelLogin() { calls.push("cancelLogin") }

    function reset() {
        calls = []
    }
}
