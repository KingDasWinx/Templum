//! The terminal stream over real HTTP (docs/specs/api.md, Batch 2): the terminals are viewers
//! (fed bytes, no PTY), lent by the fake UI thread as the glue lends the real ones.

mod common;

use common::*;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use templum_api::TerminalRef;
use templum_api::store::Scope;
use templum_terminal::Terminal;

const STREAM: &str = "/v1/terminals/b7k2m/stream";

/// A server with a Read token and a 20x4 viewer lent as `b7k2m` in `w3`.
fn setup() -> (Fake, String, Arc<Terminal>) {
    let (text, token) = token(Scope::Read, None);
    let fake = fake(vec![token], |options| {
        options.heartbeat = Duration::from_millis(100)
    });
    let v = Arc::new(viewer(20, 4));
    lend(&fake, "b7k2m", "w3", &v);
    (fake, text, v)
}

#[test]
fn a_terminal_stream_starts_whole_then_sends_the_rows_that_changed() {
    let (fake, text, v) = setup();
    v.feed(b"hello");
    let stream = open(&fake, STREAM, Auth::Bearer(&text), None);
    assert_eq!(stream.status, 200);
    let content_type = stream.headers.get("content-type").unwrap();
    assert!(
        content_type
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    let first = stream.event();
    assert_eq!(first.event, "frame");
    let frame = &first.data;
    assert_eq!(frame["whole"], true);
    assert_eq!(frame["size"], json!({"cols": 20, "rows": 4}));
    assert_eq!(frame["rows"].as_array().unwrap().len(), 4);
    assert_eq!(
        frame["rows"][0],
        json!({"y": 0, "runs": [{"text": "hello"}]})
    );
    assert_eq!(
        frame["cursor"],
        json!({"row": 0, "col": 5, "shape": "block", "blinking": false})
    );
    v.feed(b"\r\n\x1b[31mred");
    let next = stream.event();
    assert_eq!(next.event, "frame");
    assert_eq!(next.data["whole"], false);
    assert_eq!(
        next.data["rows"],
        json!([{"y": 1, "runs": [{"text": "red", "fg": 1}]}])
    );
    assert_eq!(next.data["cursor"]["row"], 1);
    assert_eq!(next.data["cursor"]["col"], 3);
}

#[test]
fn an_unchanged_terminal_sends_only_pings() {
    let (fake, text, _v) = setup();
    let stream = open(&fake, STREAM, Auth::Bearer(&text), None);
    assert_eq!(stream.event().data["whole"], true);
    assert!(stream.ping(), "a ping, before any other event");
}

#[test]
fn frames_come_at_most_twenty_a_second() {
    let (fake, text, v) = setup();
    let stream = open(&fake, STREAM, Auth::Bearer(&text), None);
    assert_eq!(stream.event().data["whole"], true);
    let feeding = Arc::new(AtomicBool::new(true));
    let feeder = {
        let (v, feeding) = (v.clone(), feeding.clone());
        std::thread::spawn(move || {
            let mut n = 0;
            while feeding.load(Ordering::Relaxed) {
                v.feed(format!("line {n}\r\n").as_bytes());
                n += 1;
                std::thread::sleep(Duration::from_millis(2));
            }
        })
    };
    let started = Instant::now();
    let mut frames = 0;
    while started.elapsed() < Duration::from_secs(1) {
        assert_eq!(stream.event().event, "frame");
        frames += 1;
    }
    let elapsed = started.elapsed().as_millis() as u64;
    feeding.store(false, Ordering::Relaxed);
    feeder.join().unwrap();
    assert!(frames >= 1);
    assert!(
        frames <= elapsed / 50 + 2,
        "{frames} frames in {elapsed} ms"
    );
}

#[test]
fn a_stream_ends_when_its_terminal_leaves() {
    let (limited, limited_token) = token(Scope::Read, Some(vec!["w3"]));
    let (text, token) = token(Scope::Read, None);
    let fake = fake(vec![token, limited_token], |_| {});
    let v = viewer(20, 4);
    lend(&fake, "b7k2m", "w3", &v);
    let stream = open(&fake, STREAM, Auth::Bearer(&text), None);
    assert_eq!(stream.status, 200);
    assert_eq!(stream.event().event, "frame");
    fake.terminals.lock().unwrap().clear();
    fake.server.set_terminals(vec![]);
    assert!(stream.ended(), "the stream ends with its terminal");
    wait_for("the stream to leave", || {
        fake.server.terminal_streams() == 0
    });
    let again = fake.call(req("GET", STREAM).token(&text));
    assert_eq!(again.status, 404);
    assert_eq!(
        again.message(),
        "There's no block b7k2m: GET /v1/blocks lists them."
    );
    // A terminal that moves out of its token's workspaces is as gone.
    lend(&fake, "b7k2m", "w3", &v);
    let stream = open(&fake, STREAM, Auth::Bearer(&limited), None);
    assert_eq!(stream.event().event, "frame");
    fake.server.set_terminals(vec![TerminalRef {
        block: "b7k2m".into(),
        workspace: "w4".into(),
        term: v.shared().clone(),
    }]);
    assert!(stream.ended());
}

#[test]
fn a_limited_token_streams_only_its_workspaces_terminals() {
    let (text, token) = token(Scope::Read, Some(vec!["w3"]));
    let fake = fake(vec![token], |_| {});
    let v = viewer(20, 4);
    lend(&fake, "b7k2m", "w3", &v);
    lend(&fake, "k9x1d", "w4", &v);
    let outside = fake.call(req("GET", "/v1/terminals/k9x1d/stream").token(&text));
    assert_eq!(outside.status, 404);
    assert_eq!(
        outside.message(),
        "There's no block k9x1d: GET /v1/blocks lists them."
    );
    let nowhere = fake.call(req("GET", "/v1/terminals/zzzzz/stream").token(&text));
    assert_eq!(nowhere.status, 404);
    assert_eq!(
        outside.message().replace("k9x1d", "zzzzz"),
        nowhere.message(),
        "the same words for a block outside and a block that isn't"
    );
    assert_eq!(fake.server.terminal_streams(), 0);
}

#[test]
fn a_block_that_isnt_a_terminal_is_refused() {
    let (fake, text, _v) = setup();
    let answer = fake.call(req("GET", "/v1/terminals/n0te/stream").token(&text));
    assert_eq!(answer.status, 422);
    assert_eq!(answer.code(), "refused");
    assert_eq!(answer.message(), "n0te isn't a terminal.");
    assert_eq!(fake.server.terminal_streams(), 0);
}

#[test]
fn revoking_cuts_terminal_streams_and_their_tickets() {
    let (fake, text, _v) = setup();
    let stream = open(&fake, STREAM, Auth::Bearer(&text), None);
    assert_eq!(stream.event().event, "frame");
    let ticket = fake.call(req("POST", "/v1/tickets").token(&text).json(&json!({})));
    let value = ticket.json["ticket"].as_str().unwrap().to_string();
    fake.server.set_tokens(vec![]);
    assert!(stream.ended());
    assert_eq!(open(&fake, STREAM, Auth::Ticket(&value), None).status, 401);
    wait_for("the stream to leave", || {
        fake.server.terminal_streams() == 0
    });
}

#[test]
fn a_ticket_opens_one_terminal_stream_once() {
    let (fake, text, _v) = setup();
    let ticket = |fake: &Fake| {
        let made = fake.call(req("POST", "/v1/tickets").token(&text).json(&json!({})));
        made.json["ticket"].as_str().unwrap().to_string()
    };
    let value = ticket(&fake);
    let stream = open(&fake, STREAM, Auth::Ticket(&value), None);
    assert_eq!(stream.status, 200);
    assert_eq!(stream.event().event, "frame");
    assert_eq!(open(&fake, STREAM, Auth::Ticket(&value), None).status, 401);
    // It opens streams and nothing else, not even the text of the same terminal.
    let other = ticket(&fake);
    let text_route = "/v1/terminals/b7k2m/text";
    assert_eq!(
        open(&fake, text_route, Auth::Ticket(&other), None).status,
        401
    );
}

#[test]
fn terminal_streams_hold_their_tokens_connections() {
    let (other, other_token) = token(Scope::Read, None);
    let (text, token) = token(Scope::Read, None);
    let fake = fake(vec![token, other_token], |_| {});
    let v = viewer(20, 4);
    lend(&fake, "b7k2m", "w3", &v);
    let streams: Vec<Stream> = (0..16)
        .map(|_| open(&fake, STREAM, Auth::Bearer(&text), None))
        .collect();
    assert!(streams.iter().all(|s| s.status == 200));
    let seventeenth = fake.call(req("GET", "/v1/state").token(&text));
    assert_eq!(seventeenth.status, 429);
    assert_eq!(seventeenth.code(), "slow_down");
    assert_eq!(fake.call(req("GET", "/v1/state").token(&other)).status, 200);
}

#[test]
fn the_audit_log_names_the_stream_not_its_ticket() {
    let (fake, text, _v) = setup();
    let ticket = fake.call(req("POST", "/v1/tickets").token(&text).json(&json!({})));
    let value = ticket.json["ticket"].as_str().unwrap().to_string();
    let stream = open(&fake, STREAM, Auth::Ticket(&value), None);
    assert_eq!(stream.event().event, "frame");
    wait_for("the stream in the log", || {
        logs(fake.dir.path()).contains("/v1/terminals/b7k2m/stream")
    });
    let log = logs(fake.dir.path());
    assert!(log.contains("\"status\":200"), "{log}");
    assert!(!log.contains(&value), "the log holds a ticket");
}

#[test]
fn the_document_has_the_stream() {
    let (fake, text, _v) = setup();
    let doc: Value = fake.call(req("GET", "/v1/openapi.json").token(&text)).json;
    let op = &doc["paths"]["/v1/terminals/{block}/stream"]["get"];
    assert_eq!(op["x-templum-scope"], "read");
    assert!(op["description"].as_str().unwrap().contains("event: frame"));
}

#[test]
fn input_is_terminal_input_and_needs_a_full_token() {
    let (control, control_token) = token(Scope::Control, None);
    let (full, full_token) = token(Scope::Full, None);
    let fake = fake(vec![control_token, full_token], |_| {});
    let input = "/v1/terminals/b7k2m/input";
    let answer = fake.call(
        req("POST", input)
            .token(&full)
            .json(&json!({"keys": ["y", "enter"]})),
    );
    assert_eq!(answer.status, 200, "{}", answer.text);
    assert_eq!(answer.json["text"], "did terminal_input");
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].params["name"], "terminal_input");
    assert_eq!(
        calls[0].params["arguments"],
        json!({"block": "b7k2m", "keys": ["y", "enter"]})
    );
    let refused = fake.call(
        req("POST", input)
            .token(&control)
            .json(&json!({"keys": ["y"]})),
    );
    assert_eq!(refused.status, 403);
    assert_eq!(refused.message(), "terminal_input needs a Full token.");
    let unknown = fake.call(
        req("POST", input)
            .token(&full)
            .json(&json!({"keys": ["F13"]})),
    );
    assert_eq!(unknown.status, 400);
    assert_eq!(unknown.code(), "invalid");
    assert!(
        unknown
            .message()
            .starts_with("keys[0] isn't a key Templum knows.")
    );
    let neither = fake.call(req("POST", input).token(&full).json(&json!({})));
    assert_eq!(neither.status, 400);
    assert_eq!(
        neither.message(),
        "terminal_input takes one of text, keys or mouse."
    );
    assert_eq!(
        fake.calls().len(),
        1,
        "the fake saw nothing of the other three"
    );
}

#[test]
fn the_audit_log_never_holds_what_was_typed() {
    let (full, full_token) = token(Scope::Full, None);
    let fake = fake(vec![full_token], |_| {});
    let input = "/v1/terminals/b7k2m/input";
    let text = fake.call(
        req("POST", input)
            .token(&full)
            .json(&json!({"text": "s3cr3t-pass"})),
    );
    let keys = fake.call(
        req("POST", input)
            .token(&full)
            .json(&json!({"keys": ["q", "z"]})),
    );
    assert_eq!((text.status, keys.status), (200, 200));
    wait_for("the first call in the log", || {
        logs(fake.dir.path()).contains("terminal_input")
    });
    let log = logs(fake.dir.path());
    let lines: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("terminal_input"))
        .collect();
    // Typing is one line per token and minute: the second call is counted, to be written with the
    // window's end.
    assert_eq!(lines.len(), 1, "{log}");
    assert!(lines.iter().all(|l| l.contains("\"status\":200")));
    for secret in ["s3cr3t", "\"q\"", "\"z\""] {
        assert!(!log.contains(secret), "{secret} in\n{log}");
    }
}
