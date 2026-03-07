use std::sync::{Arc, Mutex};

use futures_util::TryStreamExt;
use tokio::net::UnixStream;
use tokio::sync::{mpsc, watch};
use zbus::zvariant::OwnedValue;
use zbus::{connection, message, names::BusName, MessageStream};

use twitch_backend::{handle::RawDisplayData, AuthCommand};
use twitch_kde::{
    dbus_service::{DbusService, WindowRequest, OBJECT_PATH},
    dto::{LiveSectionDto, LoginStateDto, PlasmoidState, ScheduleSectionDto},
};

fn default_state() -> PlasmoidState {
    PlasmoidState {
        authenticated: false,
        login_state: LoginStateDto::Idle,
        live: LiveSectionDto {
            visible: vec![],
            overflow: vec![],
        },
        categories: vec![],
        schedule: ScheduleSectionDto {
            lookahead_hours: 24,
            loaded: false,
            visible: vec![],
            overflow: vec![],
        },
    }
}

fn make_service(state: PlasmoidState) -> (DbusService, Arc<Mutex<PlasmoidState>>) {
    let (auth_tx, _auth_rx) = mpsc::unbounded_channel::<AuthCommand>();
    let (window_tx, _window_rx) = mpsc::channel::<WindowRequest>(4);
    let (cancel_tx, _cancel_rx) = mpsc::channel::<()>(1);
    let state_arc = Arc::new(Mutex::new(state));
    let service = DbusService {
        state: Arc::clone(&state_arc),
        auth_cmd_tx: auth_tx,
        window_tx,
        open_url: Arc::new(|_| {}),
        cancel_login_tx: cancel_tx,
    };
    (service, state_arc)
}

async fn make_peer_conns(service: DbusService) -> (zbus::Connection, zbus::Connection) {
    let guid = zbus::Guid::generate();
    let (p0, p1) = UnixStream::pair().unwrap();

    let (server, client) = futures_util::try_join!(
        connection::Builder::unix_stream(p0)
            .server(guid) // pass by value — Guid<'static> works directly
            .unwrap()
            .p2p()
            .serve_at(OBJECT_PATH, service)
            .unwrap()
            .build(),
        connection::Builder::unix_stream(p1).p2p().build(),
    )
    .unwrap();

    (server, client)
}

#[tokio::test]
async fn state_property_reflects_initial_display_data() {
    let (service, _) = make_service(default_state());
    let expected_json = serde_json::to_string(&default_state()).unwrap();

    let (_server, client) = make_peer_conns(service).await;

    // Read the State property via org.freedesktop.DBus.Properties.Get
    let reply = client
        .call_method(
            None::<BusName<'_>>,
            OBJECT_PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.twitch.TwitchTray1", "State"),
        )
        .await
        .unwrap();

    // Properties.Get returns a Variant; unwrap it to the inner string
    let variant: OwnedValue = reply.body().deserialize().unwrap();
    let state_json = variant
        .downcast_ref::<zbus::zvariant::Str>()
        .expect("State property should be a string variant")
        .as_str()
        .to_string();

    assert_eq!(state_json, expected_json);
}

#[tokio::test]
async fn login_method_reachable_over_dbus() {
    let (auth_tx, mut auth_rx) = mpsc::unbounded_channel::<AuthCommand>();
    let (window_tx, _) = mpsc::channel::<WindowRequest>(4);
    let (cancel_tx, _) = mpsc::channel::<()>(1);

    let service = DbusService {
        state: Arc::new(Mutex::new(default_state())),
        auth_cmd_tx: auth_tx,
        window_tx,
        open_url: Arc::new(|_| {}),
        cancel_login_tx: cancel_tx,
    };

    let (_server, client) = make_peer_conns(service).await;

    client
        .call_method(
            None::<BusName<'_>>,
            OBJECT_PATH,
            Some("org.twitch.TwitchTray1"),
            "Login",
            &(),
        )
        .await
        .unwrap();

    assert!(matches!(auth_rx.try_recv().unwrap(), AuthCommand::Login));
}

#[tokio::test]
async fn state_changed_signal_emitted_when_display_rx_updates() {
    let (display_tx, display_rx) = watch::channel(RawDisplayData::default());
    let (login_tx, login_rx) = watch::channel::<Option<twitch_backend::LoginProgress>>(None);

    let (service, state_arc) = make_service(default_state());
    let (server, client) = make_peer_conns(service).await;

    // Get signal context from the registered interface
    let iface_ref = server
        .object_server()
        .interface::<_, DbusService>(OBJECT_PATH)
        .await
        .unwrap();
    let ctxt = iface_ref.signal_context().to_owned();

    twitch_kde::dbus_service::spawn_state_watcher(state_arc, display_rx, login_rx, ctxt);

    // Subscribe to all messages on the client before triggering the update
    let mut stream = MessageStream::from(&client);

    // Trigger a state change by sending to the watch channel
    display_tx.send(RawDisplayData::default()).unwrap();
    let _ = login_tx; // keep sender alive

    // Wait for a StateChanged signal (with timeout to avoid hanging)
    let received = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Ok(Some(msg)) = stream.try_next().await {
            let hdr = msg.header();
            if hdr.message_type() == message::Type::Signal
                && hdr
                    .member()
                    .map(|m| m.as_str() == "StateChanged")
                    .unwrap_or(false)
            {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);

    assert!(
        received,
        "StateChanged signal was not received within timeout"
    );
}
