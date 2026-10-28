//! The event stream over real HTTP (docs/specs/api.md, Events).

mod common;

use common::*;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use templum_api::Notice;
use templum_api::model::{BlockView, State};
use templum_api::store::Scope;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn setup() -> (Fake, String) {
    let (text, token) = token(Scope::Read, None);
    (
        fake(vec![token], |options| {
            options.heartbeat = Duration::from_millis(100)
        }),
        text,
    )
}

#[test]
fn a_stream_starts_with_a_snapshot() {
    let (fake, text) = setup();
    let stream = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    assert_eq!(stream.status, 200);
    let header = |name: &str| {
        stream
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    assert!(header("content-type").starts_with("text/event-stream"));
    assert_eq!(
        (
            header("cache-control").as_str(),
            header("x-accel-buffering").as_str()
        ),
        ("no-store", "no")
    );
    assert_eq!(header("x-content-type-options"), "nosniff");
    let first = stream.event();
    assert_eq!(first.event, "snapshot");
    let (run, n) = first.id.split_once('-').unwrap();
    assert!(
        run.len() == 8 && run.chars().all(|c| c.is_ascii_hexdigit()) && n.parse::<u64>().is_ok(),
        "{}",
        first.id
    );
    let state: State = serde_json::from_value(first.data).unwrap();
    assert_eq!(state, sample());
    wait_for("the client to stream", || {
        fake.server.clients().iter().any(|c| c.streaming)
    });
    assert_eq!(fake.server.streams(), 1);
}

#[test]
fn a_published_change_is_a_put_or_a_delete() {
    let (fake, text) = setup();
    let stream = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    assert_eq!(stream.event().event, "snapshot");
    let mut moved = sample();
    Arc::make_mut(&mut moved.blocks[0]).rect.x = 50.0;
    fake.server.publish(moved.clone());
    let put = stream.event();
    assert_eq!(
        (
            put.event.as_str(),
            put.data["op"].as_str(),
            put.data["id"].as_str()
        ),
        ("block", Some("put"), Some("b7k2m"))
    );
    assert_eq!(put.data["value"]["rect"]["x"], 50.0);
    assert!(put.id.contains('-'));
    // The same state again says nothing; the next change is the next event.
    fake.server.publish(moved.clone());
    let mut answered = moved;
    answered.requests.clear();
    fake.server.publish(answered);
    let gone = stream.event();
    assert_eq!(
        (gone.event.as_str(), gone.data),
        ("request", json!({"op": "delete", "id": "r12"}))
    );
}

#[test]
fn a_limited_token_hears_only_its_workspaces() {
    let (text, token) = token(Scope::Read, Some(vec!["w3"]));
    let fake = fake(vec![token], |options| {
        options.heartbeat = Duration::from_millis(100)
    });
    let stream = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    assert_eq!(stream.event().event, "snapshot");
    // A new block in w4 is not heard.
    let mut elsewhere = sample();
    let mut x1 = BlockView::clone(&elsewhere.blocks[0]);
    x1.id = "x1".into();
    x1.workspace = "w4".into();
    elsewhere.blocks.push(Arc::new(x1));
    fake.server.publish(elsewhere.clone());
    // b7k2m moves from w3 to w4: a delete (and of what hangs on it); then back: a put.
    let mut moved = elsewhere.clone();
    Arc::make_mut(&mut moved.blocks[0]).workspace = "w4".into();
    fake.server.publish(moved);
    let mut kinds = Vec::new();
    for _ in 0..3 {
        let event = stream.event();
        kinds.push((
            event.event,
            event.data["op"].as_str().unwrap().to_string(),
            event.data["id"].as_str().unwrap().to_string(),
        ));
    }
    assert_eq!(
        kinds,
        [
            (
                "block".to_string(),
                "delete".to_string(),
                "b7k2m".to_string()
            ),
            (
                "wire".to_string(),
                "delete".to_string(),
                "b7k2m-k9x1d".to_string()
            ),
            (
                "request".to_string(),
                "delete".to_string(),
                "r12".to_string()
            ),
        ],
        "nothing for x1, then what left with b7k2m"
    );
    fake.server.publish(elsewhere);
    let back = stream.event();
    assert_eq!(
        (
            back.event.as_str(),
            back.data["op"].as_str(),
            back.data["id"].as_str()
        ),
        ("block", Some("put"), Some("b7k2m"))
    );
    assert_eq!(back.data["value"]["workspace"], "w3");
    // Notices: w4's doesn't reach it, w3's does.
    fake.server.notice(Notice {
        text: "in w4".into(),
        block: None,
        workspace: Some("w4".into()),
        automation: None,
    });
    fake.server.notice(Notice {
        text: "in w3".into(),
        block: Some("b7k2m".into()),
        workspace: Some("w3".into()),
        automation: None,
    });
    loop {
        let event = stream.event();
        if event.event == "notice" {
            assert_eq!(event.data["text"], "in w3");
            break;
        }
    }
}

#[test]
fn a_ticket_opens_one_stream_once() {
    let (text, token) = token(Scope::Read, None);
    let fake = fake(vec![token.clone()], |options| {
        options.ticket_life = Duration::from_millis(400)
    });
    let ticket = |fake: &Fake| fake.call(req("POST", "/v1/tickets").token(&text).json(&json!({})));
    let made = ticket(&fake);
    assert_eq!(made.status, 200);
    let value = made.json["ticket"].as_str().unwrap().to_string();
    assert!(value.starts_with("tkt_") && value.len() == 36, "{value}");
    assert_eq!(made.json["expires_in"], 0, "400 ms is under a second");
    let stream = open(&fake, "/v1/events", Auth::Ticket(&value), None);
    assert_eq!(stream.status, 200);
    assert_eq!(stream.event().event, "snapshot");
    let again = open(&fake, "/v1/events", Auth::Ticket(&value), None);
    assert_eq!(again.status, 401, "a ticket is single use");
    // A ticket opens a stream and nothing else.
    let other = ticket(&fake).json["ticket"].as_str().unwrap().to_string();
    assert_eq!(
        fake.call(req("GET", &format!("/v1/state?ticket={other}")))
            .status,
        401
    );
    assert_eq!(
        fake.call(req("POST", &format!("/v1/tickets?ticket={other}")))
            .status,
        401
    );
    // The state call above didn't spend it, but time does.
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(
        open(&fake, "/v1/events", Auth::Ticket(&other), None).status,
        401,
        "a stale ticket"
    );
    // A ticket needs a token to make it.
    assert_eq!(fake.call(req("POST", "/v1/tickets")).status, 401);
    // The default life is 30 seconds.
    let (_, token) = self::token(Scope::Read, None);
    let normal = self::fake(vec![token], |_| {});
    let (text, token) = self::token(Scope::Read, None);
    normal.server.set_tokens(vec![token]);
    assert_eq!(
        normal
            .call(req("POST", "/v1/tickets").token(&text).json(&json!({})))
            .json["expires_in"],
        30
    );
}

#[test]
fn resuming_gets_a_snapshot_and_the_missed_notices() {
    let (fake, text) = setup();
    let first = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    let x = first.event().id;
    drop(first);
    wait_for("the stream to close", || fake.server.streams() == 0);
    fake.server.notice(Notice {
        text: "t1 ended".into(),
        block: Some("b7k2m".into()),
        workspace: Some("w3".into()),
        automation: None,
    });
    let back = open(&fake, "/v1/events", Auth::Bearer(&text), Some(&x));
    assert_eq!(back.event().event, "snapshot");
    let notice = back.event();
    assert_eq!(
        (notice.event.as_str(), notice.data["text"].as_str()),
        ("notice", Some("t1 ended"))
    );
    // From another run: the snapshot alone, then only heartbeats.
    let strange = open(&fake, "/v1/events", Auth::Bearer(&text), Some("00000000-5"));
    assert_eq!(strange.event().event, "snapshot");
    assert!(
        strange.ping(),
        "the next thing is a heartbeat, not a notice"
    );
    // A notice from before the id isn't sent again.
    let later = open(&fake, "/v1/events", Auth::Bearer(&text), Some(&notice.id));
    assert_eq!(later.event().event, "snapshot");
    assert!(later.ping());
}

#[test]
fn a_heartbeat_comes_while_nothing_changes() {
    let (fake, text) = setup();
    let stream = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    assert_eq!(stream.event().event, "snapshot");
    assert!(stream.ping() && stream.ping());
}

#[test]
fn a_stream_ends_when_its_tokens_workspaces_change() {
    let (text, token) = token(Scope::Read, Some(vec!["w3", "w4"]));
    let fake = fake(vec![token.clone()], |_| {});
    let stream = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    assert_eq!(stream.event().event, "snapshot");
    // The same list again: the stream stays.
    fake.server.set_tokens(vec![token.clone()]);
    assert_eq!(fake.server.streams(), 1);
    let mut fewer = token;
    fewer.workspaces = Some(vec!["w3".into()]);
    fake.server.set_tokens(vec![fewer]);
    assert!(stream.ended(), "the body ends");
    wait_for("no stream", || fake.server.streams() == 0);
    assert_eq!(
        open(&fake, "/v1/events", Auth::Bearer(&text), None)
            .event()
            .event,
        "snapshot"
    );
}

#[test]
fn revoking_cuts_the_stream_and_its_tickets() {
    let (text, token) = token(Scope::Read, None);
    let fake = fake(vec![token.clone()], |options| {
        options.heartbeat = Duration::from_millis(100)
    });
    let stream = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    assert_eq!(stream.event().event, "snapshot");
    let ticket = fake.call(req("POST", "/v1/tickets").token(&text).json(&json!({})));
    let ticket = ticket.json["ticket"].as_str().unwrap().to_string();
    fake.server.set_tokens(vec![]);
    assert!(stream.ended(), "the body ends");
    wait_for("no stream", || fake.server.streams() == 0);
    assert_eq!(
        open(&fake, "/v1/events", Auth::Ticket(&ticket), None).status,
        401
    );
    // An expiry, with no set_tokens: the heartbeat finds it.
    let (text, mut token) = self::token(Scope::Read, None);
    token.expires = Some(now() + 2);
    fake.server.set_tokens(vec![token]);
    let stream = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    assert_eq!(stream.event().event, "snapshot");
    assert!(stream.ended(), "its stream ends at its expiry");
    assert_eq!(fake.call(req("GET", "/v1/state").token(&text)).status, 401);
}

#[test]
fn an_expired_tokens_stream_ends_at_the_next_publish_not_the_next_heartbeat() {
    let (text, mut token) = token(Scope::Read, None);
    token.expires = Some(now() + 2);
    // A heartbeat a minute apart: only the state coming in can find the expiry in time.
    let fake = fake(vec![token], |options| {
        options.heartbeat = Duration::from_secs(60)
    });
    let stream = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    assert_eq!(stream.event().event, "snapshot");
    let ticker = std::thread::scope(|scope| {
        let handle = scope.spawn(|| {
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while fake.server.streams() > 0 && std::time::Instant::now() < deadline {
                fake.server.publish(sample());
                std::thread::sleep(Duration::from_millis(100));
            }
            fake.server.streams()
        });
        assert!(stream.ended(), "its stream ended within the deadline");
        handle.join().unwrap()
    });
    assert_eq!(ticker, 0);
}

#[test]
fn eight_streams_per_token() {
    let (text, token) = token(Scope::Read, None);
    let (other_text, other) = self::token(Scope::Read, None);
    let fake = fake(vec![token, other], |options| {
        options.heartbeat = Duration::from_millis(100)
    });
    let streams: Vec<Stream> = (0..8)
        .map(|_| open(&fake, "/v1/events", Auth::Bearer(&text), None))
        .collect();
    assert!(streams.iter().all(|s| s.status == 200));
    wait_for("8 streams", || fake.server.streams() == 8);
    let ninth = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    assert_eq!(ninth.status, 429);
    assert_eq!(
        open(&fake, "/v1/events", Auth::Bearer(&other_text), None).status,
        200,
        "another token still opens one"
    );
    drop(streams);
    wait_for("room again", || fake.server.streams() <= 1);
    assert_eq!(
        open(&fake, "/v1/events", Auth::Bearer(&text), None).status,
        200
    );
}

#[test]
fn the_first_stream_asks_for_the_tick() {
    let (fake, text) = setup();
    assert!(
        fake.notifications(templum_api::WATCH).is_empty(),
        "nothing before a stream"
    );
    let first = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    first.event();
    wait_for("the watch", || {
        fake.notifications(templum_api::WATCH).len() == 1
    });
    let second = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    second.event();
    assert_eq!(
        fake.notifications(templum_api::WATCH).len(),
        1,
        "one tick serves them all"
    );
    drop((first, second));
    wait_for("no stream", || fake.server.streams() == 0);
    let third = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    third.event();
    wait_for("the watch again", || {
        fake.notifications(templum_api::WATCH).len() == 2
    });
}

#[test]
fn a_stream_with_view_hears_the_camera() {
    let (fake, text) = setup();
    let follower = open(&fake, "/v1/events?view=true", Auth::Bearer(&text), None);
    let plain = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    let snapshot = follower.event();
    assert_eq!(snapshot.event, "snapshot");
    assert_eq!(snapshot.data["view"]["zoom"], 1.0);
    assert_eq!(plain.event().event, "snapshot");
    let mut state = sample();
    state.view.as_mut().unwrap().zoom = 2.0;
    fake.server.publish(state);
    let event = follower.event();
    assert_eq!(event.event, "view");
    assert_eq!(event.data["zoom"], 2.0);
    assert_eq!(event.data["workspace"], "w3");
    assert!(plain.ping(), "the other stream hears no camera");
    let bad = fake.call(req("GET", "/v1/events?view=maybe").token(&text));
    assert_eq!(bad.status, 400);
    assert_eq!(bad.code(), "invalid");
    assert_eq!(bad.message(), "\"view\" is true or false.");
}

#[test]
fn mirrored_says_which_tokens_hold_an_event_stream() {
    let (text, mut token) = token(Scope::Read, None);
    token.id = 41;
    // The heartbeat is what finds the test's reader gone, and closes its connection.
    let fake = fake(vec![token], |options| {
        options.heartbeat = Duration::from_millis(100)
    });
    assert_eq!(fake.server.mirrored(), []);
    let clients = |fake: &Fake| fake.notifications(templum_api::CLIENTS).len();
    let before = clients(&fake);
    let stream = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    assert_eq!(stream.event().event, "snapshot");
    wait_for("the stream is mirrored", || {
        fake.server.mirrored() == [(41, "127.0.0.1".to_string())]
    });
    assert!(
        clients(&fake) > before,
        "the UI thread heard the stream open"
    );
    let opened = clients(&fake);
    drop(stream);
    wait_for("the stream is gone", || fake.server.mirrored().is_empty());
    wait_for("the UI thread hears it end", || clients(&fake) > opened);
}

#[test]
fn a_stream_opening_on_a_known_connection_tells_the_ui() {
    let (text, token) = shared_token(false);
    let fake = fake(vec![token], |options| {
        options.heartbeat = Duration::from_millis(100)
    });
    // The connection showed its token at /v1/me: opening the stream is not what joins it.
    let mut raw = idle(&fake, &text, "one");
    // The join's notification reaches the fake on its own thread: wait for it, so the stream
    // opening is the only thing that can raise the count past `known`.
    wait_for("the UI thread hears the join", || {
        !fake.notifications(templum_api::CLIENTS).is_empty()
    });
    let known = fake.notifications(templum_api::CLIENTS).len();
    write!(
        raw,
        "GET /v1/events HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {text}\r\n{MIRROR}: one\r\n\r\n"
    )
    .unwrap();
    wait_for("the stream is mirrored", || {
        !fake.server.mirrored().is_empty()
    });
    wait_for("the UI thread hears the stream open", || {
        fake.notifications(templum_api::CLIENTS).len() > known
    });
}

#[test]
fn a_stream_still_opening_when_its_token_is_revoked_never_opens() {
    let (text, token) = self::token(Scope::Read, None);
    let fake = fake(vec![token], |options| {
        options.heartbeat = Duration::from_millis(100)
    });
    fake.hold_state
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let mut raw = TcpStream::connect(fake.server.addr()).unwrap();
    raw.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(
        raw,
        "GET /v1/events HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {text}\r\n\r\n"
    )
    .unwrap();
    wait_for("the state to be asked", || !fake.asked().is_empty());
    fake.server.set_tokens(vec![]);
    fake.release_state();
    let mut heard = Vec::new();
    let _ = raw.read_to_end(&mut heard);
    let heard = String::from_utf8_lossy(&heard);
    assert!(!heard.contains("snapshot"), "{heard}");
    assert_eq!(fake.server.streams(), 0);
}

#[test]
fn a_stream_still_opening_when_its_workspaces_change_is_told_to_open_again() {
    let (text, token) = self::token(Scope::Read, Some(vec!["w3"]));
    let fake = fake(vec![token.clone()], |options| {
        options.heartbeat = Duration::from_millis(100)
    });
    fake.hold_state
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let mut raw = TcpStream::connect(fake.server.addr()).unwrap();
    raw.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(
        raw,
        "GET /v1/events HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {text}\r\n\r\n"
    )
    .unwrap();
    wait_for("the state to be asked", || !fake.asked().is_empty());
    // The same token, another list: the person deleted the shared workspace.
    fake.server.set_tokens(vec![templum_api::Token {
        workspaces: Some(vec![]),
        ..token
    }]);
    fake.release_state();
    let mut heard = Vec::new();
    let _ = raw.read_to_end(&mut heard);
    let heard = String::from_utf8_lossy(&heard).to_lowercase();
    assert!(heard.starts_with("http/1.1 429"), "not a 401: {heard}");
    assert!(heard.contains("retry-after"), "{heard}");
    assert!(!heard.contains("snapshot"), "{heard}");
    assert_eq!(fake.server.streams(), 0);
}

fn shared_token(share: bool) -> (String, templum_api::Token) {
    let (text, mut token) = token(Scope::Full, Some(vec!["w3"]));
    if share {
        token.share = Some(templum_api::store::Share {
            workspace: "w3".into(),
            way: templum_api::store::Way::Public,
            address: "https://t.example.com".into(),
        });
    }
    (text, token)
}

const MIRROR: &str = "Templum-Mirror";

#[test]
fn a_new_mirror_session_replaces_the_old_one() {
    let (text, token) = shared_token(true);
    let fake = fake(vec![token], |options| {
        options.heartbeat = Duration::from_millis(100)
    });
    let a = open_with(
        &fake,
        "/v1/events",
        Auth::Bearer(&text),
        None,
        &[(MIRROR, "one")],
    );
    assert_eq!(a.event().event, "snapshot");
    let b = open_with(
        &fake,
        "/v1/events",
        Auth::Bearer(&text),
        None,
        &[(MIRROR, "two")],
    );
    assert_eq!((b.status, b.event().event.as_str()), (200, "snapshot"));
    let replaced = a.event();
    assert_eq!(
        (replaced.event.as_str(), replaced.data),
        ("replaced", json!({}))
    );
    assert!(a.next().is_none(), "and then it ended");
    // B stays: a publish reaches it.
    let mut moved = sample();
    Arc::make_mut(&mut moved.blocks[0]).title = "moved".into();
    fake.server.publish(moved);
    assert_eq!(b.event().event, "block");
    assert_eq!(fake.server.mirrored().len(), 1);
    // A call with the old session is a new call, not a stream: it is served.
    let state = fake.call(req("GET", "/v1/state").token(&text).header(MIRROR, "one"));
    assert_eq!(state.status, 200);
    // The same session opening again replaces nothing.
    let c = open_with(
        &fake,
        "/v1/events",
        Auth::Bearer(&text),
        None,
        &[(MIRROR, "one")],
    );
    assert_eq!(c.event().event, "snapshot");
    let again = b.event();
    assert_eq!(again.event, "replaced", "two was another session than one");
}

#[test]
fn a_settings_token_keeps_its_streams() {
    let (text, token) = shared_token(false);
    let fake = fake(vec![token], |options| {
        options.heartbeat = Duration::from_millis(100)
    });
    let a = open_with(
        &fake,
        "/v1/events",
        Auth::Bearer(&text),
        None,
        &[(MIRROR, "one")],
    );
    assert_eq!(a.event().event, "snapshot");
    let b = open_with(
        &fake,
        "/v1/events",
        Auth::Bearer(&text),
        None,
        &[(MIRROR, "two")],
    );
    assert_eq!(b.event().event, "snapshot");
    let mut moved = sample();
    Arc::make_mut(&mut moved.blocks[0]).title = "moved".into();
    fake.server.publish(moved);
    // Both hear the change, and neither was told anything else.
    assert_eq!(a.event().event, "block");
    assert_eq!(b.event().event, "block");
    assert!(a.ping() && b.ping(), "still open");
}

/// A connection that made one call in `session` and stays open, idle.
fn idle(fake: &Fake, text: &str, session: &str) -> TcpStream {
    let mut stream = TcpStream::connect(fake.server.addr()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "GET /v1/me HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {text}\r\n{MIRROR}: {session}\r\n\r\n"
    )
    .unwrap();
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0u8];
        stream.read_exact(&mut byte).unwrap();
        head.push(byte[0]);
    }
    let head = String::from_utf8(head).unwrap().to_lowercase();
    assert!(head.starts_with("http/1.1 200"), "{head}");
    let length: usize = head
        .lines()
        .find_map(|l| l.strip_prefix("content-length: "))
        .unwrap()
        .parse()
        .unwrap();
    stream.read_exact(&mut vec![0; length]).unwrap();
    stream
}

/// Whether the server has closed `stream`.
fn closed(stream: &mut TcpStream) -> bool {
    match stream.read(&mut [0u8]) {
        Ok(0) => true,
        Ok(_) => false,
        Err(e) => !matches!(
            e.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ),
    }
}

fn mirror_fake(text_token: templum_api::Token) -> Fake {
    fake(vec![text_token], |options| {
        options.heartbeat = Duration::from_millis(100)
    })
}

#[test]
fn a_new_session_aborts_the_old_ones_idle_connections_and_says_so() {
    let (text, token) = shared_token(true);
    let fake = mirror_fake(token);
    let mut old = idle(&fake, &text, "one");
    let b = open_with(
        &fake,
        "/v1/events",
        Auth::Bearer(&text),
        None,
        &[(MIRROR, "two")],
    );
    assert_eq!(b.event().event, "snapshot");
    assert!(
        closed(&mut old),
        "the idle connection of session one is cut"
    );
    wait_for("the audit line", || {
        logs(fake.dir.path()).contains("mirror_replaced")
    });
}

#[test]
fn stale_connections_do_not_keep_a_new_session_out() {
    let (text, token) = shared_token(true);
    let fake = mirror_fake(token);
    // The token's 16, all held by a laptop that slept.
    let mut stale: Vec<TcpStream> = (0..16).map(|_| idle(&fake, &text, "one")).collect();
    let b = open_with(
        &fake,
        "/v1/events",
        Auth::Bearer(&text),
        None,
        &[(MIRROR, "two")],
    );
    assert_eq!((b.status, b.event().event.as_str()), (200, "snapshot"));
    assert!(stale.iter_mut().all(closed));
}

#[test]
fn a_share_token_without_a_mirror_header_replaces_nothing() {
    let (text, token) = shared_token(true);
    let fake = mirror_fake(token);
    let a = open_with(
        &fake,
        "/v1/events",
        Auth::Bearer(&text),
        None,
        &[(MIRROR, "one")],
    );
    assert_eq!(a.event().event, "snapshot");
    let b = open(&fake, "/v1/events", Auth::Bearer(&text), None);
    assert_eq!(b.event().event, "snapshot");
    assert!(a.ping() && b.ping(), "both still open");
    assert!(!logs(fake.dir.path()).contains("mirror_replaced"));
}

#[test]
fn a_new_session_ends_the_old_ones_terminal_streams() {
    let (text, token) = shared_token(true);
    let fake = mirror_fake(token);
    let v = viewer(20, 3);
    lend(&fake, "b7k2m", "w3", &v);
    let a = open_with(
        &fake,
        "/v1/terminals/b7k2m/stream",
        Auth::Bearer(&text),
        None,
        &[(MIRROR, "one")],
    );
    assert_eq!(a.event().event, "frame");
    let b = open_with(
        &fake,
        "/v1/events",
        Auth::Bearer(&text),
        None,
        &[(MIRROR, "two")],
    );
    assert_eq!(b.event().event, "snapshot");
    assert!(
        a.next().is_none(),
        "the terminal stream of session one ended"
    );
}

#[test]
fn a_replaced_stream_whose_client_never_reads_is_cut_after_the_grace() {
    let (text, token) = shared_token(true);
    let fake = fake(vec![token], |options| {
        options.heartbeat = Duration::from_millis(100);
        options.replaced_grace = Duration::from_millis(100);
    });
    // A client that opens a stream and never reads it.
    let mut stuck = TcpStream::connect(fake.server.addr()).unwrap();
    write!(
        stuck,
        "GET /v1/events HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {text}\r\n{MIRROR}: one\r\n\r\n"
    )
    .unwrap();
    wait_for("the stream", || fake.server.streams() == 1);
    // More than any socket buffer holds: the server's write to it blocks.
    for n in 0..200 {
        let mut state = sample();
        Arc::make_mut(&mut state.blocks[0]).title = format!("{}{}", n % 10, "x".repeat(128 * 1024));
        fake.server.publish(state);
        std::thread::sleep(Duration::from_millis(5));
    }
    let b = open_with(
        &fake,
        "/v1/events",
        Auth::Bearer(&text),
        None,
        &[(MIRROR, "two")],
    );
    assert_eq!(b.status, 200);
    wait_for("the stuck connection to be cut", || {
        fake.server.clients().len() == 1
    });
    drop(stuck);
}
