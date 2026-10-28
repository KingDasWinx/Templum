//! The mirror's client against a home: following its state, reconnecting, and why it stops
//! (docs/specs/templum-to-templum.md). A fake home, over loopback; no keyring, no sleeps.

mod common;

use common::*;
use std::io::{Read, Write};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use templum_api::Caller;
use templum_api::client::{
    Bad, Command, Kept, KeySource, Keys, Limits, Remote, SERVICE_DEV, Secrets, Status, Update, Why,
};
use templum_api::code::{Key, Trust};
use templum_api::registry::mouse_value;
use templum_api::store::{Listen, Scope, Settings, Tls};
use templum_api::tls;
use templum_terminal::{Button, MouseInput, MouseKind, Terminal};

/// The secrets of `remote` for the share `text`: bound to where the remote goes.
fn secrets_for(remote: &Remote, text: &str) -> Secrets {
    Secrets {
        token: Key::new(text.into()),
        access_secret: None,
        proxy_auth: None,
        url: remote.url.clone(),
        trust: remote.trust.clone(),
        trust_added: remote.trust_added,
        proxy: remote.proxy.clone(),
    }
}

fn live(update: &Update) -> bool {
    matches!(update, Update::Status(Status::Live))
}

fn cut(why: Why) -> impl FnMut(&Update) -> bool {
    move |update| matches!(update, Update::Status(Status::Cut(w)) if *w == why)
}

fn reconnecting(attempt: u32) -> impl FnMut(&Update) -> bool {
    move |update| matches!(update, Update::Status(Status::Reconnecting { attempt: a, .. }) if *a == attempt)
}

#[test]
fn a_mirror_follows_home_live() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token.clone()], |_| {});
    let (mirror, wakes) = mirror(Keys::without_keyring(None));
    mirror.connect("r1", remote_for(&fake, &text));

    let seen = until(&mirror, "r1", "live", live);
    assert!(
        matches!(seen[0], Update::Status(Status::Connecting)),
        "{seen:?}"
    );
    let me = seen
        .iter()
        .find_map(|u| match u {
            Update::Me(me) => Some(me.clone()),
            _ => None,
        })
        .expect("who the share is");
    assert_eq!(me.workspaces, Some(vec!["w3".to_string()]));
    assert!(me.share.is_some());

    let mut first = None;
    wait_for("the snapshot", || {
        first = mirror.state("r1");
        first.is_some()
    });
    assert_eq!(first.unwrap(), sample().visible(&Caller::of(&token)));
    assert_eq!(mirror.state("r1"), None, "taken: none until a newer one");

    let mut moved = sample();
    Arc::make_mut(&mut moved.blocks[0]).rect.x = 55.0;
    fake.server.publish(moved);
    wait_for("the delta", || {
        mirror
            .state("r1")
            .is_some_and(|s| s.blocks.iter().any(|b| b.id == "b7k2m" && b.rect.x == 55.0))
    });
    assert!(wakes.load(Ordering::SeqCst) > 2);
}

#[test]
fn the_key_is_saved_before_connecting() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let dir = tempfile::tempdir().unwrap();
    let store = keyring_core::mock::Store::new().unwrap();
    let keys = Keys::with_store(store.clone(), SERVICE_DEV, Some(dir.path()));
    let (first, _) = mirror(keys.clone());
    let mut remote = remote_for(&fake, "unused");
    let secrets = secrets_for(&remote, &text);
    remote.key = KeySource::Save {
        entry: "remote-ab12cd34".into(),
        secrets: secrets.clone(),
        file: false,
        from: None,
    };
    first.connect("r1", remote.clone());
    let seen = until(&first, "r1", "live", live);
    let kept = seen
        .iter()
        .position(|u| matches!(u, Update::Kept(Kept::Keyring)));
    let alive = seen.iter().position(live);
    assert!(kept.is_some() && kept < alive, "saved, then live: {seen:?}");
    assert_eq!(keys.load("remote-ab12cd34", false), Ok(secrets));
    drop(first);

    let (second, _) = mirror(Keys::with_store(store, SERVICE_DEV, Some(dir.path())));
    remote.key = KeySource::Kept {
        entry: "remote-ab12cd34".into(),
        file: false,
    };
    second.connect("r1", remote.clone());
    until(&second, "r1", "live from the kept key", live);

    remote.key = KeySource::Kept {
        entry: "remote-nobody".into(),
        file: false,
    };
    second.connect("r2", remote);
    until(&second, "r2", "the keyring's refusal", cut(Why::Keyring));
}

#[test]
fn a_silent_stream_is_dead_and_the_remote_comes_back() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |o| o.heartbeat = Duration::from_secs(60));
    let limits = Limits {
        silence: Duration::from_millis(300),
        me_every: Duration::from_millis(200),
        ..Default::default()
    };
    let (mirror, _) = mirror_with(Keys::without_keyring(None), limits);
    mirror.connect("r1", remote_for(&fake, &text));
    until(&mirror, "r1", "live", live);
    let seen = until(&mirror, "r1", "reconnecting", reconnecting(1));
    assert!(
        seen.iter().any(|u| matches!(u, Update::Latency(_))),
        "calls went on: {seen:?}"
    );
    until(&mirror, "r1", "live again", live);
}

/// A relay to `fake`, and a mirror live on it (as `r1`) through the relay.
fn live_through(fake: &Fake, text: &str, limits: Limits) -> (Mirror, Relay) {
    let relay = Relay::to(fake.server.addr().port());
    let (mirror, _) = mirror_with(Keys::without_keyring(None), limits);
    mirror.connect("r1", remote_at(&relay.url(), text));
    until(&mirror, "r1", "live", live);
    (mirror, relay)
}

#[test]
fn home_gone_then_back() {
    let (text, token) = share_token(Scope::Full, "w3");
    let first = fake(vec![token.clone()], |_| {});
    let (mirror, relay) = live_through(&first, &text, Limits::default());

    // Home goes: its connections end and nothing answers at the address.
    relay.point(0);
    relay.cut();
    drop(first);
    let seen = until(&mirror, "r1", "the first retry", reconnecting(1));
    let next = seen.iter().find_map(|u| match u {
        Update::Status(Status::Reconnecting { next, live_at, .. }) => Some((*next, *live_at)),
        _ => None,
    });
    let (next, live_at) = next.unwrap();
    assert!(
        next >= Duration::from_millis(800) && next <= Duration::from_millis(1200),
        "{next:?}"
    );
    assert!(live_at.is_some(), "it says when it was last live");
    until(&mirror, "r1", "the second retry", reconnecting(2));

    // Home is back, on a port of its own behind the same address.
    let again = fake(vec![token], |_| {});
    relay.point(again.server.addr().port());
    mirror.send("r1", Command::TryNow);
    until(&mirror, "r1", "live at once", live);

    let log = relay.log();
    let events: Vec<&str> = log.split("get /v1/events").skip(1).collect();
    assert!(events.len() >= 2, "{log}");
    let head = |request: &str| request.split("\r\n\r\n").next().unwrap().to_string();
    assert!(
        !head(events[0]).contains("last-event-id"),
        "the first stream starts from the snapshot"
    );
    assert!(
        head(events.last().unwrap()).contains("last-event-id: "),
        "a reconnect says where it was: {}",
        head(events.last().unwrap())
    );
}

#[test]
fn failures_start_again_after_a_long_enough_live() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let relay = Relay::to(fake.server.addr().port());
    let limits = Limits {
        live_resets: Duration::from_millis(300),
        ..Default::default()
    };
    let (mirror, _) = mirror_with(Keys::without_keyring(None), limits);
    relay.point(0);
    mirror.connect("r1", remote_at(&relay.url(), &text));
    until(&mirror, "r1", "the first failure", reconnecting(1));
    mirror.send("r1", Command::TryNow);
    until(&mirror, "r1", "the second failure", reconnecting(2));

    relay.point(fake.server.addr().port());
    mirror.send("r1", Command::TryNow);
    until(&mirror, "r1", "live", live);
    std::thread::sleep(Duration::from_millis(450));
    relay.point(0);
    relay.cut();
    // A long live made the count start over: not the third.
    let seen = until(&mirror, "r1", "the first failure again", reconnecting(1));
    assert!(
        !seen
            .iter()
            .any(|u| matches!(u, Update::Status(Status::Reconnecting { attempt: 3, .. }))),
        "{seen:?}"
    );
}

#[test]
fn a_deleted_workspace_and_a_second_mirror_stop_for_good() {
    let (text, mut token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token.clone()], |_| {});
    let (mirror, _) = mirror(Keys::without_keyring(None));
    mirror.connect("r1", remote_for(&fake, &text));
    until(&mirror, "r1", "live", live);

    // Part 1 takes the id out of every token's list when its workspace is deleted.
    token.workspaces = Some(Vec::new());
    fake.server.set_tokens(vec![token.clone()]);
    let seen = until(&mirror, "r1", "deleted", cut(Why::Deleted));
    assert!(
        !seen
            .iter()
            .any(|u| matches!(u, Update::Status(Status::Reconnecting { .. }))),
        "{seen:?}"
    );
    wait_for("nothing open", || fake.server.streams() == 0);

    // Two computers, one code: the second is live and the first stops.
    token.workspaces = Some(vec!["w3".into()]);
    fake.server.set_tokens(vec![token]);
    let first = a_live_mirror(&fake, &text);
    let (second, _) = common::mirror(Keys::without_keyring(None));
    second.connect("r1", remote_for(&fake, &text));
    until(&second, "r1", "the second is live", live);
    let seen = until(&first, "r1", "the first is replaced", cut(Why::Replaced));
    assert!(
        !seen
            .iter()
            .any(|u| matches!(u, Update::Status(Status::Reconnecting { .. }))),
        "{seen:?}"
    );
}

/// A mirror that is already live on `text`.
fn a_live_mirror(fake: &Fake, text: &str) -> templum_api::client::Mirror {
    let (first, _) = mirror(Keys::without_keyring(None));
    first.connect("r1", remote_for(fake, text));
    until(&first, "r1", "the first is live", live);
    first
}

#[test]
fn a_changed_pin_and_a_tunnel_that_isnt_templum_are_refused() {
    let (text, token) = share_token(Scope::Full, "w3");
    let own = Settings {
        tls: Tls::Own,
        listen: Listen::Loopback,
        port: 0,
        ..Default::default()
    };
    let fake = fake_with(own, vec![token], |_| {});
    let certificate = tls::own(fake.dir.path()).unwrap();
    let pin = tls::pin(&certificate.chain[0]);
    assert!(fake.base.starts_with("https://"));

    let (mirror, _) = mirror(Keys::without_keyring(None));
    let mut remote = remote_for(&fake, &text);
    remote.trust = Trust::Pinned(pin);
    rebind(&mut remote);
    mirror.connect("pinned", remote.clone());
    until(&mirror, "pinned", "live over a pinned TLS", live);
    remote.trust = Trust::Pinned("00".repeat(32));
    rebind(&mut remote);
    mirror.connect("moved", remote.clone());
    let seen = until(&mirror, "moved", "a changed pin", cut(Why::PinChanged));
    assert!(!seen.iter().any(live));

    // A public certificate is the web's: Templum's own is not valid there.
    remote.trust = Trust::Public;
    rebind(&mut remote);
    mirror.connect("public", remote);
    // Where the platform's verifier names the reason (Linux's does) it is read; elsewhere it
    // may call it "other", and Mozilla's list reads it then: either way not a bare "other"
    // unless the platform refused for no reason webpki finds.
    until(&mirror, "public", "an unknown authority", |u| {
        #[cfg(target_os = "linux")]
        let wanted = matches!(u, Update::Status(Status::Cut(Why::Certificate(Bad::Alone))));
        #[cfg(not(target_os = "linux"))]
        let wanted = matches!(
            u,
            Update::Status(Status::Cut(Why::Certificate(Bad::Alone | Bad::Other)))
        );
        wanted
    });

    // Something that answers 404 text/html is not a Templum.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut head = [0u8; 2048];
                let _ = stream.read(&mut head);
                let _ = stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Type: text/html\r\nContent-Length: 6\r\n\r\n<html>",
                );
            });
        }
    });
    let mut remote = remote_for(&fake, &text);
    remote.url = format!("http://127.0.0.1:{port}");
    rebind(&mut remote);
    mirror.connect("tunnel", remote);
    until(
        &mirror,
        "tunnel",
        "a tunnel that is not Templum",
        cut(Why::NotTemplum {
            status: 404,
            kind: "text/html".into(),
        }),
    );
}

// -- Terminals, typing, moves, answers, Forget --

use futures::executor::block_on;
use serde_json::{Value, json};
use templum_api::Notice;
use templum_api::client::{About, Call, Mirror, Refusal};
use templum_api::model::Rect;

fn call(tool: &str, args: Value, slow: bool) -> Call {
    Call {
        tool: tool.into(),
        args,
        slow,
    }
}

/// A call that comes back means the mirror has dealt with everything sent before it.
fn settled(mirror: &Mirror) {
    block_on(mirror.call("r1", call("show", json!({"workspace": "w3"}), false))).unwrap();
}

fn refusal(sentence: &str) -> Refusal {
    Refusal::of(sentence)
}

/// A mirror live on a share of w3 on `fake`, as `r1`.
fn live_on(fake: &Fake, text: &str) -> Mirror {
    let (mirror, _) = common::mirror(Keys::without_keyring(None));
    mirror.connect("r1", remote_for(fake, text));
    until(&mirror, "r1", "live", live);
    mirror
}

/// What the fake was asked to run as `tool`, in order: each call's arguments.
fn calls_of(fake: &Fake, tool: &str) -> Vec<Value> {
    fake.calls()
        .into_iter()
        .filter(|call| call.params["name"] == tool)
        .map(|call| call.params["arguments"].clone())
        .collect()
}

/// Every key the fake was sent for `terminal_input`, call after call.
fn keys_sent(fake: &Fake) -> Vec<String> {
    calls_of(fake, "terminal_input")
        .iter()
        .flat_map(|args| args["keys"].as_array().cloned().unwrap_or_default())
        .map(|key| key.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_terminal_streams_into_its_viewer() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let home = viewer(20, 4);
    lend(&fake, "b7k2m", "w3", &home);
    let mirror = live_on(&fake, &text);
    let shown = viewer(20, 4);
    mirror.send(
        "r1",
        Command::Terminals(vec![("b7k2m".into(), shown.feeder())]),
    );

    home.feed(b"hello");
    wait_for("hello in the mirror", || {
        shown.shared().lines(1) == ["hello"]
    });
    home.feed(b"\x1b]2;vim\x07");
    wait_for("the title", || {
        shown
            .shared()
            .screen()
            .is_some_and(|s| s.title.as_deref() == Some("vim"))
    });

    mirror.send("r1", Command::Terminals(vec![]));
    wait_for("the stream closed", || fake.server.terminal_streams() == 0);
}

#[test]
fn at_most_eleven_terminals_stream() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let homes: Vec<_> = (0..14).map(|_| viewer(10, 2)).collect();
    let shown: Vec<_> = (0..14).map(|_| viewer(10, 2)).collect();
    let mut set = Vec::new();
    for (n, (home, mirrored)) in homes.iter().zip(&shown).enumerate() {
        let block = format!("t{n:02}");
        lend(&fake, &block, "w3", home);
        set.push((block, mirrored.feeder()));
    }
    let mirror = live_on(&fake, &text);
    mirror.send("r1", Command::Terminals(set));
    wait_for("eleven streams", || fake.server.terminal_streams() == 11);
    settled(&mirror);
    assert_eq!(fake.server.terminal_streams(), 11);
}

#[test]
fn a_terminal_home_refused_is_asked_again_when_the_set_changes_not_when_reordered() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let (home, late, gone) = (viewer(10, 2), viewer(10, 2), viewer(10, 2));
    lend(&fake, "b7k2m", "w3", &home);
    let mirror = live_on(&fake, &text);
    let (a, b) = (viewer(10, 2), viewer(10, 2));
    let asked_for = |block: &str| {
        fake.seen()
            .iter()
            .filter(|s| s.method == templum_api::TERMINAL && s.params["block"] == block)
            .count()
    };
    let round_trip = || settled(&mirror);
    let set = |blocks: &[(&str, &templum_terminal::Terminal)]| {
        Command::Terminals(
            blocks
                .iter()
                .map(|(block, term)| (block.to_string(), term.feeder()))
                .collect(),
        )
    };
    mirror.send("r1", set(&[("gone", &a), ("b7k2m", &b)]));
    wait_for("the refusal", || {
        asked_for("gone") == 1 && fake.server.terminal_streams() == 1
    });
    round_trip();
    // It exists now, but the same terminals in another order are not a new look.
    lend(&fake, "gone", "w3", &gone);
    mirror.send("r1", set(&[("b7k2m", &b), ("gone", &a)]));
    round_trip();
    round_trip();
    assert_eq!(asked_for("gone"), 1, "not asked again for a reorder");
    assert_eq!(fake.server.terminal_streams(), 1);
    // A terminal more is.
    lend(&fake, "late", "w3", &late);
    mirror.send("r1", set(&[("b7k2m", &b), ("gone", &a), ("late", &late)]));
    wait_for("all three", || fake.server.terminal_streams() == 3);
}

/// Key names Templum knows, in two cycles of different lengths so that a reordering shows.
fn first_kind(n: usize) -> String {
    [
        "b", "c", "enter", "tab", "up", "space", "ctrl+a", "f5", "alt+left", "D", "é",
    ][n % 11]
        .into()
}

fn second_kind(n: usize) -> String {
    [
        "x",
        "y",
        "z",
        "down",
        "home",
        "end",
        "page_up",
        "shift+tab",
        "7",
        "ç",
        "ctrl+c",
        "left",
        "right",
    ][n % 13]
        .into()
}

#[test]
fn keys_wait_their_turn_and_overflow_is_dropped() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let mirror = live_on(&fake, &text);
    fake.hold_calls.store(true, Ordering::SeqCst);
    let send = |keys: Vec<String>| {
        mirror.send(
            "r1",
            Command::Keys {
                block: "b7k2m".into(),
                keys,
            },
        )
    };
    send(vec!["a".into()]);
    wait_for("the first call", || keys_sent(&fake).len() == 1);
    // One call is in flight: the rest wait, 1024 at most.
    send((0..300).map(first_kind).collect());
    send((0..800).map(second_kind).collect());
    let seen = until(
        &mirror,
        "r1",
        "the overflow",
        |u| matches!(u, Update::Dropped(b) if b == "b7k2m"),
    );
    assert!(seen.iter().any(|u| matches!(u, Update::Dropped(_))));
    assert_eq!(
        keys_sent(&fake),
        ["a"],
        "nothing else went while the call was in flight"
    );

    fake.release_calls();
    wait_for("the waiting keys", || keys_sent(&fake).len() == 1 + 1024);
    let mut wanted = vec!["a".to_string()];
    wanted.extend((0..300).map(first_kind));
    wanted.extend((0..724).map(second_kind));
    assert_eq!(
        keys_sent(&fake),
        wanted,
        "in order, the new ones past 1024 dropped"
    );
    let calls = calls_of(&fake, "terminal_input");
    assert!(
        calls
            .iter()
            .all(|c| c["keys"].as_array().unwrap().len() <= 256),
        "256 a call at most"
    );
    assert!(calls.iter().all(|c| c["block"] == "b7k2m"));
}

#[test]
fn a_paste_goes_after_the_keys_typed_before_it() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let mirror = live_on(&fake, &text);
    fake.hold_calls.store(true, Ordering::SeqCst);
    let block = || "b7k2m".to_string();
    mirror.send(
        "r1",
        Command::Keys {
            block: block(),
            keys: vec!["a".into()],
        },
    );
    wait_for("the first call", || keys_sent(&fake).len() == 1);
    mirror.send(
        "r1",
        Command::Keys {
            block: block(),
            keys: vec!["b".into()],
        },
    );
    mirror.send(
        "r1",
        Command::Paste {
            block: block(),
            text: "pasted".into(),
        },
    );
    mirror.send(
        "r1",
        Command::Keys {
            block: block(),
            keys: vec!["c".into()],
        },
    );
    mirror.send(
        "r1",
        Command::Paste {
            block: block(),
            text: "x".repeat(300 * 1024),
        },
    );
    until(
        &mirror,
        "r1",
        "the big paste refused",
        |u| matches!(u, Update::Refused { about: About::Keys(_), sentence } if sentence.starts_with("Paste at most")),
    );
    fake.release_calls();
    wait_for("all four calls", || {
        calls_of(&fake, "terminal_input").len() == 4
    });
    let calls = calls_of(&fake, "terminal_input");
    assert_eq!(calls[1], json!({"block": "b7k2m", "keys": ["b"]}));
    assert_eq!(calls[2], json!({"block": "b7k2m", "text": "pasted"}));
    assert_eq!(calls[3], json!({"block": "b7k2m", "keys": ["c"]}));
}

#[test]
fn moves_and_calls_reach_home() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let mirror = live_on(&fake, &text);
    mirror.send(
        "r1",
        Command::Move {
            block: "b7k2m".into(),
            rect: Rect {
                x: 10.0,
                y: 20.0,
                w: 300.0,
                h: 200.0,
            },
        },
    );
    let answered = block_on(mirror.call(
        "r1",
        call(
            "answer_request",
            json!({"request": "r12", "answers": [["Postgres"]]}),
            false,
        ),
    ));
    assert_eq!(answered.unwrap()["text"], "did answer_request");
    settled(&mirror);
    wait_for("the three calls", || fake.calls().len() >= 3);
    assert_eq!(
        calls_of(&fake, "move_block"),
        [json!({"block": "b7k2m", "x": 10.0, "y": 20.0, "w": 300.0, "h": 200.0})]
    );
    assert_eq!(
        calls_of(&fake, "answer_request"),
        [json!({"request": "r12", "answers": [["Postgres"]]})]
    );
    assert_eq!(calls_of(&fake, "show"), [json!({"workspace": "w3"})]);

    mirror.send(
        "r1",
        Command::Move {
            block: "locked".into(),
            rect: Rect {
                x: 1.0,
                y: 2.0,
                w: 300.0,
                h: 200.0,
            },
        },
    );
    until(
        &mirror,
        "r1",
        "home's refusal",
        |u| matches!(u, Update::Refused { about: About::Move(b), sentence } if b == "locked" && sentence == "No."),
    );
}

#[test]
fn a_call_is_answered_with_home_s_json() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let mirror = live_on(&fake, &text);
    let answered = block_on(mirror.call("r1", call("show", json!({"workspace": "w3"}), false)));
    assert_eq!(answered.unwrap()["text"], "did show");
    assert_eq!(calls_of(&fake, "show"), [json!({"workspace": "w3"})]);
}

#[test]
fn a_refused_call_says_home_s_words() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let mirror = live_on(&fake, &text);
    let answered =
        block_on(mirror.call("r1", call("close_block", json!({"block": "b7k2m"}), false)));
    assert_eq!(answered, Err(refusal("No.")));
}

#[test]
fn past_sixty_four_waiting_calls_are_refused_at_once() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |options| {
        options.computer = "winx-desktop".into()
    });
    let mirror = live_on(&fake, &text);
    fake.hold_tool("show");
    let first = mirror.call("r1", call("show", json!({"workspace": "w3"}), false));
    wait_for("the first call at home", || fake.held() == 1);
    let waiting: Vec<_> = (0..64)
        .map(|_| mirror.call("r1", call("show", json!({"workspace": "w3"}), false)))
        .collect();
    assert_eq!(
        block_on(mirror.call("r1", call("show", json!({"workspace": "w3"}), false))),
        Err(refusal("winx-desktop is slow to answer: try again."))
    );
    fake.release_tool();
    assert!(block_on(first).is_ok());
    for answered in waiting {
        assert!(block_on(answered).is_ok());
    }
}

#[test]
fn a_block_s_waiting_moves_keep_only_the_newest() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let mirror = live_on(&fake, &text);
    fake.hold_tool("show");
    let shown = mirror.call("r1", call("show", json!({"workspace": "w3"}), false));
    wait_for("the show at home", || fake.held() == 1);
    for x in [1.0, 2.0, 3.0] {
        mirror.send(
            "r1",
            Command::Move {
                block: "b7k2m".into(),
                rect: Rect {
                    x,
                    y: 5.0,
                    w: 300.0,
                    h: 200.0,
                },
            },
        );
    }
    fake.release_tool();
    assert!(block_on(shown).is_ok());
    wait_for("the move", || !calls_of(&fake, "move_block").is_empty());
    settled(&mirror);
    assert_eq!(
        calls_of(&fake, "move_block"),
        [json!({"block": "b7k2m", "x": 3.0, "y": 5.0, "w": 300.0, "h": 200.0})]
    );
}

#[test]
fn a_quick_call_is_answered_while_a_slow_one_runs() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let mirror = live_on(&fake, &text);
    fake.hold_tool("show");
    let slow = mirror.call("r1", call("show", json!({"workspace": "w3"}), true));
    wait_for("the slow call at home", || fake.held() == 1);
    let quick = block_on(mirror.call(
        "r1",
        call("move_block", json!({"block": "b7k2m", "x": 1.0}), false),
    ));
    assert!(quick.is_ok(), "{quick:?}");
    assert_eq!(fake.held(), 1);
    fake.release_tool();
    assert!(block_on(slow).is_ok());
}

#[test]
fn calls_waiting_when_the_attempt_drops_are_refused_once() {
    let (text, token) = share_token(Scope::Full, "w3");
    // Home's name is the computer's own unless set: set, it is the same on every machine.
    let fake = fake(vec![token], |options| options.computer = "winx".into());
    let mirror = live_on(&fake, &text);
    fake.hold_tool("show");
    let calls: Vec<_> = (0..3)
        .map(|_| mirror.call("r1", call("show", json!({"workspace": "w3"}), false)))
        .collect();
    wait_for("the first call at home", || fake.held() == 1);
    mirror.send("r1", Command::Disconnect);
    for (n, answered) in calls.into_iter().enumerate() {
        // The first was in flight: home may have done it. The others never left.
        let expected = if n == 0 {
            Refusal::maybe_done("winx may have done it: the connection dropped before it answered.")
        } else {
            refusal("Couldn't reach the other computer.")
        };
        assert_eq!(block_on(answered), Err(expected));
    }
    mirror.send("r1", Command::TryNow);
    until(&mirror, "r1", "live again", live);
    fake.release_tool();
    settled(&mirror);
    assert_eq!(
        calls_of(&fake, "show").len(),
        2,
        "the barrier's, and the one that was in flight"
    );
}

#[test]
fn home_s_tools_come_at_each_connect() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let (mirror, _) = common::mirror(Keys::without_keyring(None));
    mirror.connect("r1", remote_for(&fake, &text));
    let is_home_s = |u: &Update| {
        matches!(u, Update::Tools(t)
            if t.has("move_block") && t.takes("move_block", "x") && !t.has("nope"))
    };
    until(&mirror, "r1", "home's tools", is_home_s);
    mirror.send("r1", Command::Disconnect);
    until(&mirror, "r1", "disconnected", |u| {
        matches!(u, Update::Status(Status::Disconnected))
    });
    mirror.send("r1", Command::TryNow);
    until(&mirror, "r1", "the tools again", is_home_s);
}

#[test]
fn a_notice_reaches_the_ui_as_its_first_line() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let mirror = live_on(&fake, &text);
    fake.server.notice(Notice {
        text: format!("{}\nsecond", "é".repeat(400)),
        block: Some("b7k2m".into()),
        workspace: Some("w3".into()),
        automation: None,
    });
    until(&mirror, "r1", "the notice", |u| {
        matches!(u, Update::Notice(n)
            if n.text == "é".repeat(300) && n.block.as_deref() == Some("b7k2m"))
    });
}

#[test]
fn a_command_while_not_live_is_refused_or_dropped() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let mirror = live_on(&fake, &text);
    mirror.send("r1", Command::Disconnect);
    until(&mirror, "r1", "disconnected", |u| {
        matches!(u, Update::Status(Status::Disconnected))
    });
    mirror.send(
        "r1",
        Command::Keys {
            block: "b7k2m".into(),
            keys: vec!["a".into()],
        },
    );
    assert_eq!(
        block_on(mirror.call("r1", call("show", json!({"workspace": "w3"}), false))),
        Err(refusal("Not connected."))
    );
    assert!(
        keys_sent(&fake).is_empty(),
        "keys are never queued for later"
    );
    wait_for("nothing open", || {
        fake.server.streams() == 0 && fake.server.client_count() == 0
    });
    // Disconnected is not the end: Try now connects it again.
    mirror.send("r1", Command::TryNow);
    until(&mirror, "r1", "live again", live);
}

#[test]
fn the_keyring_is_probed_off_the_ui_thread() {
    for (keys, answers) in [
        (
            Keys::with_store(keyring_core::mock::Store::new().unwrap(), SERVICE_DEV, None),
            true,
        ),
        (Keys::without_keyring(None), false),
    ] {
        let (mirror, _) = common::mirror(keys);
        mirror.probe();
        until(
            &mirror,
            "",
            "the answer",
            |u| matches!(u, Update::Keyring(a) if *a == answers),
        );
    }
}

#[test]
fn a_revoked_share_cuts_the_mirror_for_good() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let (mirror, _) = common::mirror(Keys::without_keyring(None));
    mirror.connect("r1", remote_for(&fake, &text));
    let mut all = until(&mirror, "r1", "live", live);
    fake.hold_calls.store(true, Ordering::SeqCst);
    let block = || "b7k2m".to_string();
    mirror.send(
        "r1",
        Command::Keys {
            block: block(),
            keys: vec!["a".into()],
        },
    );
    wait_for("a call in flight", || keys_sent(&fake).len() == 1);
    mirror.send(
        "r1",
        Command::Keys {
            block: block(),
            keys: vec!["b".into(), "c".into()],
        },
    );

    fake.server.set_tokens(Vec::new());
    all.extend(until(&mirror, "r1", "revoked", cut(Why::Revoked)));
    wait_for("nothing open", || {
        fake.server.streams() == 0 && fake.server.client_count() == 0
    });
    all.extend(mirror.updates().into_iter().map(|(_, u)| u));
    assert!(
        !all.iter()
            .any(|u| matches!(u, Update::Status(Status::Reconnecting { .. }))),
        "a revoke is not a reconnect: {all:?}"
    );
    fake.release_calls();
    assert_eq!(
        keys_sent(&fake),
        ["a"],
        "the waiting keys were dropped, not sent late"
    );
}

#[test]
fn forget_revokes_then_deletes_the_key() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let dir = tempfile::tempdir().unwrap();
    let keys = Keys::with_store(
        keyring_core::mock::Store::new().unwrap(),
        SERVICE_DEV,
        Some(dir.path()),
    );
    let (mirror, _) = common::mirror(keys.clone());
    let mut remote = remote_for(&fake, "unused");
    remote.key = KeySource::Save {
        entry: "remote-ab12cd34".into(),
        secrets: secrets_for(&remote, &text),
        file: false,
        from: None,
    };
    mirror.connect("r1", remote);
    until(&mirror, "r1", "live", live);
    assert!(keys.load("remote-ab12cd34", false).is_ok());

    mirror.forget("r1", true);
    wait_for("the key is gone", || {
        keys.load("remote-ab12cd34", false).is_err()
    });
    assert_eq!(
        calls_of(&fake, "revoke_share"),
        [json!({})],
        "revoked before the key went"
    );
    assert!(
        mirror.updates().iter().all(|(id, _)| id != "r1"),
        "no update follows a forget"
    );
}

/// A forget with revoke ends the attempt's streams before the revoke call is answered: a home
/// that is slow to answer it must not go on being read.
#[test]
fn forget_with_revoke_drops_the_streams_before_the_revoke_is_answered() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    fake.hold_revoke
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let (mirror, _) = common::mirror(Keys::with_store(
        keyring_core::mock::Store::new().unwrap(),
        SERVICE_DEV,
        None,
    ));
    let mut remote = remote_for(&fake, &text);
    remote.key = KeySource::Memory(secrets_for(&remote, &text));
    mirror.connect("r1", remote);
    until(&mirror, "r1", "live", live);
    assert_eq!(fake.server.streams(), 1);

    mirror.forget("r1", true);
    wait_for("the revoke reached home", || {
        calls_of(&fake, "revoke_share").len() == 1
    });
    wait_for("the stream is closed while the revoke waits", || {
        fake.server.streams() == 0
    });
}

#[test]
fn nothing_shows_the_key() {
    const TEXT: &str = "tpl_SECRETSECRETSECRET";
    let (_, mut token) = share_token(Scope::Full, "w3");
    token.hash = templum_api::store::hash(TEXT);
    let fake = fake(vec![token], |_| {});
    let mut remote = remote_for(&fake, "unused");
    let secrets = Secrets {
        access_secret: Some("acc-secret".into()),
        proxy_auth: Some("user:proxy-password".into()),
        ..secrets_for(&remote, TEXT)
    };
    remote.access_id = Some("access-id".into());
    remote.key = KeySource::Save {
        entry: "remote-ab12cd34".into(),
        secrets: secrets.clone(),
        file: false,
        from: None,
    };
    let keys = Keys::with_store(keyring_core::mock::Store::new().unwrap(), SERVICE_DEV, None);
    let (mirror, _) = common::mirror(keys);
    mirror.connect("r1", remote.clone());
    let mut shown: Vec<String> = Vec::new();
    let mut all = until(&mirror, "r1", "live", live);
    fake.server.set_tokens(Vec::new());
    all.extend(until(&mirror, "r1", "revoked", cut(Why::Revoked)));
    shown.extend(all.iter().map(|u| format!("{u:?} {u:#?}")));
    shown.push(format!(
        "{remote:?} {:?} {secrets:?} {:#?}",
        remote.key, remote.key
    ));

    // Behind a proxy that wants a login: the proxy sees the password, nothing prints it.
    let (port, heads) = proxy(407);
    let mut via = remote_at("https://127.0.0.1:9", TEXT);
    via.trust = Trust::Pinned("00".repeat(32));
    via.proxy = Some(format!("http://127.0.0.1:{port}"));
    via.access_id = Some("access-id".into());
    via.key = KeySource::Memory(Secrets {
        access_secret: Some("acc-secret".into()),
        proxy_auth: Some("user:proxy-password".into()),
        ..secrets_for(&via, TEXT)
    });
    mirror.connect("via", via.clone());
    let refused = until(&mirror, "via", "the proxy's refusal", |u| {
        matches!(
            u,
            Update::Status(Status::Cut(Why::Proxy { status: 407, .. }))
        )
    });
    assert!(heads.lock().unwrap()[0].contains("Proxy-Authorization: Basic "));
    shown.extend(refused.iter().map(|u| format!("{u:?} {u:#?}")));
    shown.push(format!("{via:?} {:#?}", via.key));
    for why in [
        Why::Expired,
        Why::Revoked,
        Why::Deleted,
        Why::Replaced,
        Why::PinChanged,
        Why::Certificate(Bad::Alone),
        Why::AddedAuthority,
        Why::Access,
        Why::NotTemplum {
            status: 404,
            kind: "text/html".into(),
        },
        Why::TooOld,
        Why::Proxy {
            proxy: "proxy.corp:8080".into(),
            status: 407,
        },
        Why::Keyring,
    ] {
        shown.push(format!("{why:?} {:?}", Status::Cut(why.clone())));
    }
    let shown = shown.join("\n");
    for secret in ["SECRETSECRET", "acc-secret", "proxy-password"] {
        assert!(!shown.contains(secret), "{secret} shows in:\n{shown}");
    }
    assert!(shown.contains("tpl_…"), "the token shows as tpl_…");
    for (at, _) in shown.match_indices("tpl_") {
        assert!(
            shown[at + 4..].starts_with('…'),
            "a tpl_ not followed by …: {}",
            &shown[at..at + 12]
        );
    }
}

// -- Where the key may go --

/// A proxy that answers every `CONNECT` with `status` and closes; the heads it was sent.
fn proxy(status: u16) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let heads: Arc<Mutex<Vec<String>>> = Arc::default();
    let kept = heads.clone();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let kept = kept.clone();
            std::thread::spawn(move || {
                let (mut head, mut byte) = (Vec::new(), [0u8; 1]);
                while !head.ends_with(b"\r\n\r\n") && stream.read_exact(&mut byte).is_ok() {
                    head.push(byte[0]);
                }
                kept.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&head).into_owned());
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} Nope\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            });
        }
    });
    (port, heads)
}

/// A listener nobody should connect to; how many did.
fn watched() -> (String, Arc<std::sync::atomic::AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = count.clone();
    std::thread::spawn(move || {
        for _ in listener.incoming().flatten() {
            counted.fetch_add(1, Ordering::SeqCst);
        }
    });
    (url, count)
}

#[test]
fn a_key_is_never_sent_where_the_index_now_says() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let keys = Keys::with_store(keyring_core::mock::Store::new().unwrap(), SERVICE_DEV, None);
    let (mirror, _) = common::mirror(keys.clone());
    let mut remote = remote_for(&fake, &text);
    remote.key = KeySource::Save {
        entry: "remote-ab12cd34".into(),
        secrets: secrets_for(&remote, &text),
        file: false,
        from: None,
    };
    mirror.connect("kept", remote.clone());
    until(&mirror, "kept", "live from its own address", live);
    remote.key = KeySource::Kept {
        entry: "remote-ab12cd34".into(),
        file: false,
    };

    // Someone rewrote the index: another address, another pin, an accepted authority.
    let (elsewhere, accepted) = watched();
    let mut moved = remote.clone();
    moved.url = elsewhere;
    let mut repinned = remote.clone();
    repinned.trust = Trust::Pinned("00".repeat(32));
    let mut added = remote.clone();
    added.trust_added = true;
    let mut proxied = remote.clone();
    proxied.proxy = Some("http://127.0.0.1:9".into());
    for (name, changed) in [
        ("moved", moved),
        ("repinned", repinned),
        ("added", added),
        ("proxied", proxied),
    ] {
        mirror.connect(name, changed);
        let seen = until(&mirror, name, name, cut(Why::Keyring));
        assert!(
            seen.iter().any(|u| matches!(
                u,
                Update::Refused { sentence, .. } if sentence.contains("changed outside Templum")
            )),
            "{name} says why: {seen:?}"
        );
    }
    // The address, the way in and the proxy as they were are still fine.
    mirror.connect("same", remote);
    until(&mirror, "same", "live as before", live);
    assert_eq!(accepted.load(Ordering::SeqCst), 0, "nobody was called");
}

#[test]
fn plain_http_and_proxies_stay_on_this_computer() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let (mirror, _) = common::mirror(Keys::without_keyring(None));
    let (listener, accepted) = watched();
    let refused = |mirror: &Mirror, id: &str, remote: Remote| {
        mirror.connect(id, remote);
        until(mirror, id, id, |u| {
            matches!(
                u,
                Update::Status(Status::Cut(Why::NotTemplum { status: 0, .. }))
            )
        });
    };

    // TLS to this computer by the "loopback" way: nobody checks it, so the key doesn't go.
    let mut tls = remote_at(&listener.replace("http://", "https://"), &text);
    tls.trust = Trust::Loopback;
    rebind(&mut tls);
    refused(&mirror, "tls-loopback", tls);
    // Plain HTTP off this computer, whatever the way in.
    for (id, trust) in [
        ("http-loopback", Trust::Loopback),
        ("http-pinned", Trust::Pinned("00".repeat(32))),
        ("http-public", Trust::Public),
    ] {
        let mut remote = remote_at("http://example.com", &text);
        remote.trust = trust;
        rebind(&mut remote);
        refused(&mirror, id, remote);
    }
    assert_eq!(accepted.load(Ordering::SeqCst), 0, "nothing was dialed");

    // This computer is never reached through a proxy, whatever is set.
    let (proxy_port, heads) = proxy(503);
    let mut remote = remote_for(&fake, &text);
    remote.proxy = Some(format!("http://127.0.0.1:{proxy_port}"));
    rebind(&mut remote);
    mirror.connect("direct", remote);
    until(&mirror, "direct", "live without the proxy", live);
    assert!(heads.lock().unwrap().is_empty(), "the proxy was not asked");
}

#[test]
fn a_proxy_that_cant_reach_home_is_retried_and_one_that_refuses_is_not() {
    let (text, _) = share_token(Scope::Full, "w3");
    let (mirror, _) = common::mirror(Keys::without_keyring(None));
    // A pinned share behind a proxy: the proxy is asked before anyone shakes hands.
    let through = |port: u16| {
        let mut remote = remote_at("https://127.0.0.1:9", &text);
        remote.trust = Trust::Pinned("00".repeat(32));
        remote.proxy = Some(format!("http://127.0.0.1:{port}"));
        let KeySource::Memory(secrets) = &mut remote.key else {
            unreachable!()
        };
        secrets.proxy_auth = Some("u:p".into());
        rebind(&mut remote);
        remote
    };

    for status in [502, 503, 504] {
        let (port, heads) = proxy(status);
        let id = format!("down-{status}");
        mirror.connect(&id, through(port));
        let seen = until(&mirror, &id, &id, reconnecting(1));
        assert!(
            !seen
                .iter()
                .any(|u| matches!(u, Update::Status(Status::Cut(_)))),
            "{status} is home being down, not a refusal: {seen:?}"
        );
        let heads = heads.lock().unwrap();
        assert!(
            heads[0].starts_with("CONNECT 127.0.0.1:9 HTTP/1.1\r\n"),
            "{heads:?}"
        );
        assert!(
            heads[0].contains("Proxy-Authorization: Basic dTpw\r\n"),
            "the login in the keyring goes to the proxy: {heads:?}"
        );
    }
    let (port, _) = proxy(407);
    mirror.connect("refused", through(port));
    until(
        &mirror,
        "refused",
        "a proxy that wants another login",
        cut(Why::Proxy {
            proxy: format!("127.0.0.1:{port}"),
            status: 407,
        }),
    );
}

// -- Typing --

#[test]
fn typing_fast_goes_in_few_calls_and_loses_no_key() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let mirror = live_on(&fake, &text);
    // Home is slow to answer, as it is when keys come faster than its bucket refills: the first
    // call is in flight while the person types on, one key at a time. Held and released rather
    // than timed, so a loaded runner can't change the count.
    fake.hold_calls.store(true, Ordering::SeqCst);
    let keys: Vec<String> = (0..200).map(first_kind).collect();
    for key in &keys {
        mirror.send(
            "r1",
            Command::Keys {
                block: "b7k2m".into(),
                keys: vec![key.clone()],
            },
        );
    }
    // Every command has been dealt with by the time the barrier is answered.
    barrier(&mirror);
    fake.release_calls();
    wait_for("every key", || keys_sent(&fake).len() == keys.len());
    assert_eq!(keys_sent(&fake), keys, "in order, none lost");
    // A call a key would be 200: the waiting keys go together.
    let calls = calls_of(&fake, "terminal_input").len();
    assert!(calls < 10, "{calls} calls");
}

#[test]
fn typing_while_not_live_is_refused_once_a_block_and_overflow_says_so_once() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let mirror = live_on(&fake, &text);
    let send = |block: &str, keys: Vec<String>| {
        mirror.send(
            "r1",
            Command::Keys {
                block: block.into(),
                keys,
            },
        )
    };
    let typed_refusals = |seen: &[Update], block: &str| {
        seen.iter()
            .filter(|u| {
                matches!(u, Update::Refused { about: About::Keys(b), sentence }
                if b == block && sentence == "Not connected: what you type isn't sent.")
            })
            .count()
    };

    mirror.send("r1", Command::Disconnect);
    until(&mirror, "r1", "disconnected", |u| {
        matches!(u, Update::Status(Status::Disconnected))
    });
    for _ in 0..3 {
        send("b7k2m", vec!["a".into()]);
    }
    mirror.send(
        "r1",
        Command::Paste {
            block: "b7k2m".into(),
            text: "pasted".into(),
        },
    );
    send("other", vec!["a".into()]);
    assert_eq!(
        block_on(mirror.call("r1", call("show", json!({"workspace": "w3"}), false))),
        Err(refusal("Not connected."))
    );
    let seen: Vec<Update> = mirror.updates().into_iter().map(|(_, u)| u).collect();
    assert_eq!(typed_refusals(&seen, "b7k2m"), 1, "{seen:?}");
    assert_eq!(typed_refusals(&seen, "other"), 1, "{seen:?}");
    assert!(keys_sent(&fake).is_empty());

    // Live again, and gone again: told again.
    mirror.send("r1", Command::TryNow);
    until(&mirror, "r1", "live again", live);
    mirror.send("r1", Command::Disconnect);
    until(&mirror, "r1", "disconnected again", |u| {
        matches!(u, Update::Status(Status::Disconnected))
    });
    send("b7k2m", vec!["a".into()]);
    assert_eq!(
        block_on(mirror.call("r1", call("show", json!({"workspace": "w3"}), false))),
        Err(refusal("Not connected."))
    );
    let seen: Vec<Update> = mirror.updates().into_iter().map(|(_, u)| u).collect();
    assert_eq!(typed_refusals(&seen, "b7k2m"), 1, "{seen:?}");

    // A full queue says `Dropped` once, not for each command past it.
    mirror.send("r1", Command::TryNow);
    until(&mirror, "r1", "live for the overflow", live);
    fake.hold_calls.store(true, Ordering::SeqCst);
    send("b7k2m", vec!["a".into()]);
    wait_for("the first call", || keys_sent(&fake).len() == 1);
    send("b7k2m", (0..1024).map(first_kind).collect());
    for _ in 0..5 {
        send("b7k2m", vec!["z".into(), "y".into()]);
    }
    let seen = barrier(&mirror);
    let dropped = |seen: &[Update]| {
        seen.iter()
            .filter(|u| matches!(u, Update::Dropped(b) if b == "b7k2m"))
            .count()
    };
    assert_eq!(dropped(&seen), 1, "{seen:?}");

    // Drained, it says it again the next time it overflows.
    fake.release_calls();
    wait_for("the queue drained", || keys_sent(&fake).len() == 1 + 1024);
    fake.hold_calls.store(true, Ordering::SeqCst);
    send("b7k2m", vec!["a".into()]);
    wait_for("a call in flight", || {
        keys_sent(&fake).len() == 1 + 1024 + 1
    });
    send("b7k2m", (0..1024).map(first_kind).collect());
    send("b7k2m", vec!["z".into()]);
    let seen = barrier(&mirror);
    assert_eq!(dropped(&seen), 1, "{seen:?}");
    fake.release_calls();
}

#[test]
fn a_queue_drained_in_part_says_dropped_again_when_it_overflows_again() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let mirror = live_on(&fake, &text);
    let send = |keys: Vec<String>| {
        mirror.send(
            "r1",
            Command::Keys {
                block: "b7k2m".into(),
                keys,
            },
        )
    };
    let dropped = |seen: &[Update]| {
        seen.iter()
            .filter(|u| matches!(u, Update::Dropped(b) if b == "b7k2m"))
            .count()
    };
    fake.hold_calls.store(true, Ordering::SeqCst);
    send(vec!["a".into()]);
    wait_for("the first call", || keys_sent(&fake).len() == 1);
    send((0..1024).map(first_kind).collect());
    send(vec!["z".into()]);
    assert_eq!(dropped(&barrier(&mirror)), 1, "the first overflow");

    // One call answered: 256 keys go as the next call, which stays held, and 768 wait.
    fake.release_one_call();
    wait_for("the next call", || keys_sent(&fake).len() == 1 + 256);
    send((0..300).map(second_kind).collect());
    assert_eq!(
        dropped(&barrier(&mirror)),
        1,
        "room for 256 only: the queue overflowed again, and says so again"
    );
    fake.release_calls();
}

/// Everything a remote said until it answered a paste too big: what was sent before it has
/// been dealt with by then.
fn barrier(mirror: &Mirror) -> Vec<Update> {
    mirror.send(
        "r1",
        Command::Paste {
            block: "b7k2m".into(),
            text: "x".repeat(300 * 1024),
        },
    );
    until(
        mirror,
        "r1",
        "the barrier",
        |u| matches!(u, Update::Refused { sentence, .. } if sentence.starts_with("Paste at most")),
    )
}

fn mouse(kind: MouseKind, button: Option<Button>, row: usize, col: usize) -> MouseInput {
    MouseInput {
        button,
        kind,
        row,
        col,
        ctrl: false,
        alt: false,
        shift: false,
    }
}

#[test]
fn mouse_events_ride_the_typing_queue_in_order_and_waiting_moves_merge() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let mirror = live_on(&fake, &text);
    let send = |command| mirror.send("r1", command);
    let keys = |k: &str| Command::Keys {
        block: "b7k2m".into(),
        keys: vec![k.into()],
    };
    let event = |event| Command::Mouse {
        block: "b7k2m".into(),
        event,
    };
    let (left, inputs) = (Some(Button::Left), || calls_of(&fake, "terminal_input"));
    fake.hold_calls.store(true, Ordering::SeqCst);
    send(keys("a"));
    wait_for("the first call", || inputs().len() == 1);
    let (down, up) = (
        mouse(MouseKind::Press, left, 1, 1),
        mouse(MouseKind::Release, left, 1, 4),
    );
    send(event(down));
    for col in 2..=4 {
        send(event(mouse(MouseKind::Move, left, 1, col)));
    }
    send(event(up));
    send(keys("b"));
    barrier(&mirror);
    fake.release_calls();
    wait_for("the rest", || inputs().len() == 3);
    let calls = inputs();
    let moved = mouse(MouseKind::Move, left, 1, 4);
    assert_eq!(
        calls[1]["mouse"],
        json!([mouse_value(&down), mouse_value(&moved), mouse_value(&up)]),
        "in order, the waiting moves merged into the newest"
    );
    assert_eq!(
        calls[2]["keys"],
        json!(["b"]),
        "the key typed after them goes after them"
    );
    // Seventy steps of the wheel: 64 a call at most.
    fake.hold_calls.store(true, Ordering::SeqCst);
    send(keys("c"));
    wait_for("the held call", || inputs().len() == 4);
    for _ in 0..70 {
        send(event(mouse(MouseKind::Press, Some(Button::WheelUp), 0, 0)));
    }
    barrier(&mirror);
    fake.release_calls();
    wait_for("the wheel", || inputs().len() == 6);
    let sizes: Vec<usize> = inputs()[4..]
        .iter()
        .map(|c| c["mouse"].as_array().unwrap().len())
        .collect();
    assert_eq!(sizes, [64, 6]);
}

/// A home terminal lent as `block`, its output `lines` lines `{prefix} 1.` …, and the viewer it
/// streams to.
fn streamed_with_lines(
    fake: &Fake,
    block: &str,
    prefix: &str,
    lines: usize,
) -> (Terminal, Terminal) {
    let home = viewer(20, 4);
    lend(fake, block, "w3", &home);
    let out: Vec<String> = (1..=lines).map(|n| format!("{prefix} {n}.")).collect();
    home.feed(out.join("\r\n").as_bytes());
    (home, viewer(20, 4))
}

fn history_of(mirror: &Mirror, block: &str) {
    until(
        mirror,
        "r1",
        "the history",
        |u| matches!(u, Update::History(b) if b == block),
    );
}

#[test]
fn home_s_history_fills_the_viewer_once_until_a_frame_comes() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let (home, shown) = streamed_with_lines(&fake, "b7k2m", "row", 30);
    let mirror = live_on(&fake, &text);
    mirror.send(
        "r1",
        Command::Terminals(vec![("b7k2m".into(), shown.feeder())]),
    );
    wait_for("the live screen", || shown.shared().lines(1) == ["row 30."]);
    let back = || {
        mirror.send(
            "r1",
            Command::History {
                block: "b7k2m".into(),
            },
        )
    };
    back();
    history_of(&mirror, "b7k2m");
    let all = shown.shared().lines(100);
    assert_eq!(all.len(), 30, "{all:?}");
    assert_eq!((all[0].as_str(), all[29].as_str()), ("row 1.", "row 30."));
    assert_eq!(
        calls_of(&fake, "read_terminal"),
        [json!({"block": "b7k2m", "lines": 2000})]
    );
    back();
    settled(&mirror);
    assert_eq!(
        calls_of(&fake, "read_terminal").len(),
        1,
        "nothing changed: not read again"
    );
    home.feed(b"\r\nrow 31.");
    wait_for("the new line", || shown.shared().lines(1) == ["row 31."]);
    back();
    wait_for("read again after a frame", || {
        calls_of(&fake, "read_terminal").len() == 2
    });
}

#[test]
fn one_terminal_s_history_at_a_time_and_it_goes_with_its_stream() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let (_a_home, a) = streamed_with_lines(&fake, "aaaaa", "a", 10);
    let (_b_home, b) = streamed_with_lines(&fake, "bbbbb", "b", 10);
    let mirror = live_on(&fake, &text);
    mirror.send(
        "r1",
        Command::Terminals(vec![
            ("aaaaa".into(), a.feeder()),
            ("bbbbb".into(), b.feeder()),
        ]),
    );
    wait_for("both live", || {
        a.shared().lines(1) == ["a 10."] && b.shared().lines(1) == ["b 10."]
    });
    let back = |block: &str| {
        mirror.send(
            "r1",
            Command::History {
                block: block.into(),
            },
        )
    };
    // An answer for a terminal no longer asked for fills nothing (the held one answers "did
    // read_terminal").
    fake.hold_tool("read_terminal");
    back("aaaaa");
    wait_for("a's read held", || fake.held() == 1);
    back("bbbbb");
    fake.release_tool();
    history_of(&mirror, "bbbbb");
    assert_eq!(
        a.shared().lines(100).len(),
        4,
        "{:?}",
        a.shared().lines(100)
    );
    assert_eq!(b.shared().lines(100).len(), 10);
    // Scrolling back in a drops b's.
    back("aaaaa");
    history_of(&mirror, "aaaaa");
    assert_eq!(
        (a.shared().lines(100).len(), b.shared().lines(100).len()),
        (10, 4)
    );
    // a's stream ends: its history goes.
    mirror.send("r1", Command::Terminals(vec![("bbbbb".into(), b.feeder())]));
    wait_for("a's history dropped", || a.shared().lines(100).len() == 4);
}

#[test]
fn a_frame_fed_while_the_history_is_read_makes_the_next_ask_read_again() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let (home, shown) = streamed_with_lines(&fake, "b7k2m", "row", 30);
    let mirror = live_on(&fake, &text);
    mirror.send(
        "r1",
        Command::Terminals(vec![("b7k2m".into(), shown.feeder())]),
    );
    wait_for("the live screen", || shown.shared().lines(1) == ["row 30."]);
    let back = || {
        mirror.send(
            "r1",
            Command::History {
                block: "b7k2m".into(),
            },
        )
    };
    // The answer is made at home before the frame: it is only as new as the ask.
    fake.hold_tool("read_terminal");
    back();
    wait_for("the read held", || fake.held() == 1);
    home.feed(b"\r\nrow 31.");
    wait_for("the new line", || shown.shared().lines(1) == ["row 31."]);
    back();
    // Nothing shows that the second ask was heard, and the one connection is busy with the first.
    std::thread::sleep(Duration::from_millis(200));
    fake.release_tool();
    history_of(&mirror, "b7k2m");
    back();
    wait_for("read again", || calls_of(&fake, "read_terminal").len() == 2);
}

#[test]
fn a_history_that_could_not_be_filled_stays_held_and_goes_with_its_stream() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let (_home, shown) = streamed_with_lines(&fake, "b7k2m", "row", 30);
    let mirror = live_on(&fake, &text);
    mirror.send(
        "r1",
        Command::Terminals(vec![("b7k2m".into(), shown.feeder())]),
    );
    wait_for("the live screen", || shown.shared().lines(1) == ["row 30."]);
    let back = || {
        mirror.send(
            "r1",
            Command::History {
                block: "b7k2m".into(),
            },
        )
    };
    back();
    history_of(&mirror, "b7k2m");
    assert_eq!(shown.shared().lines(100).len(), 30);
    // Home's program went full screen between the ask and the answer: nothing is filled.
    shown.feed(b"\x1b[?1049h");
    fake.hold_tool("read_terminal");
    back();
    wait_for("the read held", || fake.held() == 1);
    fake.release_tool();
    settled(&mirror);
    shown.feed(b"\x1b[?1049l");
    mirror.send("r1", Command::Terminals(vec![]));
    wait_for("the history dropped", || {
        shown.shared().lines(100).len() == 4
    });
}
