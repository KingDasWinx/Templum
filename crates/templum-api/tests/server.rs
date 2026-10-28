//! The server over real HTTP, in plain text on loopback (docs/specs/api.md).

mod common;

use common::*;
use serde_json::{Value, json};
use std::net::TcpStream;
use std::time::Duration;
use templum_api::store::{Listen, Scope, Settings, Tls, hash};
use templum_api::{Options, start};

fn read_fake() -> (Fake, String) {
    let (text, token) = token(Scope::Read, None);
    (fake(vec![token], |_| {}), text)
}

fn control_fake() -> (Fake, String) {
    let (text, token) = token(Scope::Control, None);
    (fake(vec![token], |_| {}), text)
}

#[test]
fn a_request_without_a_token_is_refused() {
    let (text, token) = token(Scope::Read, None);
    let fake = fake(vec![token], |_| {});
    let answer = fake.call(req("GET", "/v1/state"));
    assert_eq!((answer.status, answer.code()), (401, "auth"));
    assert_eq!(answer.header("www-authenticate"), Some("Bearer"));
    for (name, value) in [
        ("x-content-type-options", "nosniff"),
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        ("x-frame-options", "DENY"),
        (
            "content-security-policy",
            "default-src 'none'; frame-ancestors 'none'",
        ),
        ("cross-origin-resource-policy", "same-origin"),
    ] {
        assert_eq!(answer.header(name), Some(value), "{name}");
    }
    assert!(
        answer.header("strict-transport-security").is_none(),
        "plain HTTP has no HSTS"
    );
    // A token in a URL, or any other scheme, is no token.
    for request in [
        req("GET", &format!("/v1/state?token={text}")),
        req("GET", &format!("/v1/state?access_token={text}")),
        req("GET", "/v1/state").header("Authorization", &format!("Basic {text}")),
        req("GET", "/v1/state").header("Authorization", &text),
        req("GET", "/v1/state").header("Authorization", "Bearer "),
    ] {
        let again = fake.call(request);
        assert_eq!((again.status, again.code()), (401, "auth"));
        assert_eq!(again.json, answer.json, "the same body whatever the reason");
    }
    assert!(fake.asked().is_empty(), "nothing reached the UI thread");
}

#[test]
fn a_bearer_token_reads_the_state() {
    let (fake, text) = read_fake();
    let answer = fake.call(req("GET", "/v1/state").token(&text));
    assert_eq!(answer.status, 200);
    assert_eq!(answer.header("content-type"), Some("application/json"));
    assert_eq!(answer.json["blocks"].as_array().unwrap().len(), 2);
    assert_eq!(answer.json["app"]["api"], 1);
    let block = fake.call(req("GET", "/v1/blocks/b7k2m").token(&text));
    assert_eq!(
        (block.status, block.json["id"].as_str()),
        (200, Some("b7k2m"))
    );
    let missing = fake.call(req("GET", "/v1/blocks/zzz").token(&text));
    assert_eq!((missing.status, missing.code()), (404, "not_found"));
    assert_eq!(
        missing.message(),
        "There's no block zzz: GET /v1/blocks lists them."
    );
    // The state is asked of the UI thread, as the client "api".
    assert!(
        fake.asked()
            .iter()
            .all(|s| s.client == "api" && s.method == templum_api::STATE)
    );
    for (path, key) in [
        ("/v1/workspaces", "id"),
        ("/v1/requests", "id"),
        ("/v1/boards", "id"),
        ("/v1/automations", "id"),
        ("/v1/agents", "id"),
    ] {
        let list = fake.call(req("GET", path).token(&text));
        assert_eq!(list.status, 200, "{path}");
        assert!(
            list.json
                .as_array()
                .is_some_and(|l| !l.is_empty() && l[0].get(key).is_some()),
            "{path}: {}",
            list.text
        );
    }
    assert_eq!(
        fake.call(req("GET", "/v1/agents").token(&text))
            .json
            .as_array()
            .unwrap()
            .len(),
        1,
        "only the blocks with an agent"
    );
    let filtered = fake.call(req("GET", "/v1/blocks?workspace=w9").token(&text));
    assert_eq!(filtered.json, json!([]));
    assert_eq!(
        fake.call(req("GET", "/v1/cards/40").token(&text)).json["title"],
        "Login"
    );
    assert_eq!(
        fake.call(req("GET", "/v1/boards/3").token(&text)).json["name"],
        "Sprint"
    );
    assert_eq!(
        fake.call(req("GET", "/v1/requests/r12").token(&text))
            .status,
        200
    );
    assert_eq!(
        fake.call(req("GET", "/v1/workspaces/w3").token(&text))
            .status,
        200
    );
    assert_eq!(
        fake.call(req("GET", "/v1/automations/1").token(&text))
            .status,
        200
    );
    assert_eq!(
        fake.call(req("GET", "/v1/automations/9").token(&text))
            .message(),
        "There's no automation 9: GET /v1/automations lists them."
    );
    assert_eq!(fake.call(req("GET", "/v1/usage").token(&text)).status, 200);
    assert_eq!(
        fake.call(req("GET", "/v1/cards/x").token(&text)).status,
        404
    );
}

#[test]
fn a_limited_token_reads_only_its_workspaces() {
    let (text, token) = token(Scope::Read, Some(vec!["w9"]));
    let fake = fake(vec![token], |_| {});
    let state = fake.call(req("GET", "/v1/state").token(&text));
    assert_eq!(state.status, 200);
    assert_eq!(state.json["blocks"], json!([]));
    assert_eq!(state.json["usage"], Value::Null);
    let block = fake.call(req("GET", "/v1/blocks/b7k2m").token(&text));
    assert_eq!(
        block.status, 404,
        "the server filters too, whatever the UI thread sent"
    );
}

#[test]
fn a_tool_runs_through_the_ui_channel() {
    let (text, token) = token(Scope::Control, None);
    let id = token.id.to_string();
    let fake = fake(vec![token], |_| {});
    let answer = fake.call(
        req("POST", "/v1/tools/create_card")
            .token(&text)
            .json(&json!({"board": "S", "title": "x"})),
    );
    assert_eq!(
        (answer.status, answer.json.clone()),
        (200, json!({"text": "did create_card"}))
    );
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].params,
        json!({"name": "create_card", "arguments": {"board": "S", "title": "x"}})
    );
    assert_eq!(
        (calls[0].client.as_str(), calls[0].terminal.as_str()),
        ("api", id.as_str())
    );
}

#[test]
fn an_alias_names_its_tool_and_the_path_wins() {
    let (fake, text) = control_fake();
    let moved = fake.call(
        req("POST", "/v1/cards/40/move")
            .token(&text)
            .json(&json!({"column": "Done", "card": 99})),
    );
    assert_eq!(moved.status, 200, "{}", moved.text);
    let read =
        fake.call(req("GET", "/v1/terminals/b7k2m/text?lines=50&last_command=true").token(&text));
    assert_eq!(read.status, 200, "{}", read.text);
    let calls = fake.calls();
    assert_eq!(
        calls[0].params,
        json!({"name": "move_card", "arguments": {"card": 40, "column": "Done"}})
    );
    assert_eq!(
        calls[1].params,
        json!({"name": "read_terminal", "arguments": {"block": "b7k2m", "lines": 50, "last_command": true}})
    );
    let wired = fake.call(req("DELETE", "/v1/wires/b7k2m/k9x1d").token(&text));
    assert_eq!(wired.status, 200);
    assert_eq!(
        fake.calls()[2].params,
        json!({"name": "disconnect", "arguments": {"a": "b7k2m", "b": "k9x1d"}})
    );
}

#[test]
fn a_refusal_is_422_and_a_bad_argument_400() {
    let (fake, text) = control_fake();
    let refused = fake.call(req("DELETE", "/v1/blocks/b7k2m").token(&text));
    assert_eq!(
        (refused.status, refused.code(), refused.message()),
        (422, "refused", "No.")
    );
    let before = fake.calls().len();
    let invalid = fake.call(
        req("POST", "/v1/cards/40/move")
            .token(&text)
            .json(&json!({})),
    );
    assert_eq!((invalid.status, invalid.code()), (400, "invalid"));
    assert!(
        invalid.message().contains("column"),
        "{}",
        invalid.message()
    );
    let mistyped = fake.call(req("GET", "/v1/terminals/b/text?lines=many").token(&text));
    assert_eq!(
        (mistyped.status, mistyped.message()),
        (400, "\"lines\" is a whole number")
    );
    let bad_json = fake.call(
        req("POST", "/v1/show")
            .token(&text)
            .raw("application/json", "{oops"),
    );
    assert_eq!((bad_json.status, bad_json.code()), (400, "invalid"));
    let not_object = fake.call(
        req("POST", "/v1/show")
            .token(&text)
            .raw("application/json", "[1]"),
    );
    assert_eq!(not_object.status, 400);
    assert_eq!(
        fake.calls().len(),
        before,
        "none of them reached the UI thread"
    );
}

#[test]
fn a_tool_beyond_the_tokens_scope_is_403() {
    let (text, token) = token(Scope::Read, None);
    let fake = fake(vec![token], |_| {});
    let denied = fake.call(
        req("POST", "/v1/tools/create_card")
            .token(&text)
            .json(&json!({"board": "S", "title": "x"})),
    );
    assert_eq!(
        (denied.status, denied.code(), denied.message()),
        (403, "scope", "create_card needs a Control token.")
    );
    // Read tokens read: a tool that only reads is theirs.
    let read = fake.call(
        req("POST", "/v1/tools/overview")
            .token(&text)
            .json(&json!({})),
    );
    assert_eq!(read.status, 200);
    let (control, token) = self::token(Scope::Control, None);
    fake.server.set_tokens(vec![token]);
    let typing = fake.call(
        req("POST", "/v1/terminals/b7k2m/text")
            .token(&control)
            .json(&json!({"text": "ls"})),
    );
    assert_eq!(
        (typing.status, typing.message()),
        (403, "send_to_terminal needs a Full token.")
    );
    let automation = fake.call(
        req("POST", "/v1/automations")
            .token(&control)
            .json(&json!({"do": {"command": {"text": "ls"}}})),
    );
    assert_eq!(
        (automation.status, automation.message()),
        (403, "save_automation needs a Full token.")
    );
    assert_eq!(fake.calls().len(), 1, "only the read reached the UI thread");
}

#[test]
fn a_post_must_be_json_and_small() {
    let (fake, text) = control_fake();
    let plain = fake.call(
        req("POST", "/v1/show")
            .token(&text)
            .raw("text/plain", "{\"workspace\":\"w3\"}"),
    );
    assert_eq!((plain.status, plain.code()), (415, "json"));
    let form = fake.call(
        req("POST", "/v1/show")
            .token(&text)
            .raw("application/x-www-form-urlencoded", "workspace=w3"),
    );
    assert_eq!(form.status, 415);
    // A button pressed has no body and says no type; a body says what it is.
    let button = fake.call(req("POST", "/v1/tickets").token(&text));
    assert_eq!(button.status, 200, "{}", button.text);
    let untyped = fake.call(
        req("POST", "/v1/show")
            .token(&text)
            .untyped("{\"workspace\":\"w3\"}"),
    );
    assert_eq!((untyped.status, untyped.code()), (415, "json"));
    let charset = fake.call(
        req("POST", "/v1/show")
            .token(&text)
            .raw("application/json; charset=utf-8", "{\"workspace\":\"w3\"}"),
    );
    assert_eq!(charset.status, 200);
    let big = format!("{{\"text\":\"{}\"}}", "x".repeat(1024 * 1024));
    let large = fake.call(
        req("POST", "/v1/show")
            .token(&text)
            .raw("application/json", &big),
    );
    assert_eq!((large.status, large.code()), (413, "too_large"));
    let calls = fake.calls().len();
    assert_eq!(calls, 1, "only the good one reached the UI thread");
}

#[test]
fn cors_is_off_unless_the_origin_is_listed() {
    let (text, token) = token(Scope::Read, None);
    let off = fake(vec![token.clone()], |_| {});
    let answer = off.call(
        req("GET", "/v1/state")
            .token(&text)
            .header("Origin", "https://evil.example"),
    );
    assert!(answer.header("access-control-allow-origin").is_none());
    let pre = off.call(req("OPTIONS", "/v1/state").header("Origin", "https://evil.example"));
    assert_eq!(pre.status, 204);
    assert!(pre.header("access-control-allow-origin").is_none());
    let listed = fake(vec![token], |options| {
        options.origins = vec!["https://dash.example".into()]
    });
    let answer = listed.call(
        req("GET", "/v1/state")
            .token(&text)
            .header("Origin", "https://dash.example"),
    );
    assert_eq!(
        answer.header("access-control-allow-origin"),
        Some("https://dash.example")
    );
    assert_eq!(answer.header("vary"), Some("Origin"));
    let pre = listed
        .call(req("OPTIONS", "/v1/tools/create_card").header("Origin", "https://dash.example"));
    assert_eq!(pre.status, 204, "no token needed");
    assert_eq!(
        pre.header("access-control-allow-origin"),
        Some("https://dash.example")
    );
    let allowed = pre.header("access-control-allow-headers").unwrap();
    assert!(
        allowed.contains("Authorization")
            && allowed.contains("Last-Event-ID")
            && allowed.contains("Content-Type")
    );
    assert!(
        pre.header("access-control-allow-methods")
            .unwrap()
            .contains("PATCH")
    );
    assert_eq!(pre.header("access-control-max-age"), Some("600"));
    let evil = listed.call(
        req("GET", "/v1/state")
            .token(&text)
            .header("Origin", "https://dash.example.evil"),
    );
    assert!(evil.header("access-control-allow-origin").is_none());
    assert_eq!(
        evil.status, 200,
        "the call is still served: CORS only keeps a page from reading it"
    );
    assert!(listed.asked().len() <= 2);
}

#[test]
fn failed_logins_slow_one_address_but_never_a_good_token() {
    let (text, token) = token(Scope::Read, None);
    let fake = fake(vec![token], |_| {});
    // The proxy's entry is the last of X-Forwarded-For; a header a client can send is no address.
    let from = |ip: &str, token: &str| {
        req("GET", "/v1/state")
            .token(token)
            .header("X-Forwarded-For", &format!("9.9.9.9, {ip}"))
    };
    for i in 0..10 {
        let wrong = fake.call(from("203.0.113.7", &format!("tpl_wrong{i}")));
        assert_eq!(wrong.status, 401, "{i}");
        assert_eq!(
            wrong.header("connection"),
            Some("close"),
            "a failure holds no connection"
        );
    }
    let slowed = fake.call(from("203.0.113.7", "tpl_wrong"));
    assert_eq!((slowed.status, slowed.code()), (429, "slow_down"));
    assert_eq!(slowed.header("connection"), Some("close"));
    assert!(
        slowed
            .header("retry-after")
            .unwrap()
            .parse::<u32>()
            .unwrap()
            >= 1
    );
    // The right token passes from anywhere, that address included: forging it can't lock the person out.
    assert_eq!(fake.call(from("203.0.113.7", &text)).status, 200);
    assert_eq!(fake.call(from("198.51.100.2", &text)).status, 200);
    assert_eq!(
        fake.call(from("198.51.100.2", "tpl_wrong")).status,
        401,
        "another address is not slowed"
    );
    // Cloudflare's header says nothing: ten failures under ten names are ten from one address.
    for i in 0..10 {
        let ip = format!("203.0.113.{}", 100 + i);
        let attempt = req("GET", "/v1/state")
            .token("tpl_wrong")
            .header("CF-Connecting-IP", &ip);
        fake.call(attempt);
    }
    let attempt = req("GET", "/v1/state")
        .token("tpl_wrong")
        .header("CF-Connecting-IP", "198.18.0.1");
    assert_eq!(fake.call(attempt).status, 429);
}

#[test]
fn asking_with_no_credentials_is_not_a_failed_attempt() {
    let (text, token) = token(Scope::Read, None);
    let fake = fake(vec![token], |_| {});
    for _ in 0..30 {
        let answer = fake.call(req("GET", "/v1/state"));
        assert_eq!(
            (answer.status, answer.header("connection")),
            (401, Some("close"))
        );
    }
    assert_eq!(
        fake.call(req("GET", "/v1/state").token("tpl_wrong")).status,
        401,
        "the first failure"
    );
    assert_eq!(fake.call(req("GET", "/v1/state").token(&text)).status, 200);
}

#[test]
fn a_revoked_or_expired_token_is_refused_at_once() {
    let (text, token) = token(Scope::Read, None);
    let fake = fake(vec![token.clone()], |_| {});
    assert_eq!(fake.call(req("GET", "/v1/state").token(&text)).status, 200);
    fake.server.set_tokens(vec![]);
    assert_eq!(fake.call(req("GET", "/v1/state").token(&text)).status, 401);
    let mut expiring = token;
    expiring.expires = Some(1);
    fake.server.set_tokens(vec![expiring]);
    assert_eq!(
        fake.call(req("GET", "/v1/state").token(&text)).status,
        401,
        "long expired"
    );
}

#[test]
fn unknown_routes_and_tools_are_404() {
    let (fake, text) = control_fake();
    for request in [
        req("GET", "/v1/nope"),
        req("GET", "/v2/state"),
        req("DELETE", "/v1/state"),
        req("POST", "/v1/tools/nope").json(&json!({})),
    ] {
        let answer = fake.call(request.token(&text));
        assert_eq!((answer.status, answer.code()), (404, "not_found"));
    }
    // Without a token the same paths are 401: a route's existence isn't told to strangers.
    assert_eq!(fake.call(req("GET", "/v1/nope")).status, 401);
    assert!(fake.calls().is_empty());
}

#[test]
fn the_registry_and_its_document_are_served() {
    let (fake, text) = read_fake();
    let tools = fake.call(req("GET", "/v1/tools").token(&text));
    assert_eq!(tools.status, 200);
    let list = tools.json.as_array().unwrap();
    let find = |name: &str| {
        list.iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name}"))
    };
    assert_eq!(find("overview")["scope"], "read");
    assert_eq!(find("send_keys")["scope"], "full");
    assert_eq!(find("answer_request")["scope"], "control or full");
    assert!(find("move_block")["inputSchema"]["properties"]["x"].is_object());
    let document = fake.call(req("GET", "/v1/openapi.json").token(&text));
    assert_eq!(
        (document.status, document.json["openapi"].as_str()),
        (200, Some("3.1.0"))
    );
    assert_eq!(fake.call(req("GET", "/v1/tools")).status, 401);
    assert_eq!(fake.call(req("GET", "/v1/openapi.json")).status, 401);
}

/// A connection kept open, which has shown `token`: its answer read, the socket left.
fn connected(port: u16, token: &str) -> TcpStream {
    use std::io::{Read, Write};
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "GET /v1/nope HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\n\r\n"
    )
    .unwrap();
    let mut answer = Vec::new();
    let mut byte = [0u8; 1];
    // The head, then as many bytes as it says: the connection stays for the next request.
    while !answer.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        answer.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&answer).to_lowercase();
    assert!(head.starts_with("http/1.1 404"), "{head}");
    let length: usize = head
        .split("content-length: ")
        .nth(1)
        .unwrap()
        .split("\r\n")
        .next()
        .unwrap()
        .parse()
        .unwrap();
    stream.read_exact(&mut vec![0u8; length]).unwrap();
    stream
}

/// A body of the wrong type is read before it is refused: answered as its head came, the client
/// still writing it met a reset (Windows CI), and the connection closed under the next request.
#[test]
fn a_body_of_the_wrong_type_is_read_before_its_answer_and_the_connection_stays() {
    use std::io::{Read, Write};
    let (fake, text) = control_fake();
    let mut stream = connected(fake.server.addr().port(), &text);
    let body = "workspace=w3";
    write!(
        stream,
        "POST /v1/show HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {text}\r\n\
         Content-Type: text/plain\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    assert!(
        stream.read(&mut [0u8; 1]).is_err(),
        "no answer before the body"
    );
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "{body}GET /v1/nope HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {text}\r\n\r\n"
    )
    .unwrap();
    let mut answers = Vec::new();
    let mut chunk = [0u8; 4096];
    while !String::from_utf8_lossy(&answers).contains("HTTP/1.1 404") {
        let n = stream.read(&mut chunk).unwrap();
        assert!(n > 0, "closed: {}", String::from_utf8_lossy(&answers));
        answers.extend_from_slice(&chunk[..n]);
    }
    assert!(String::from_utf8_lossy(&answers).starts_with("HTTP/1.1 415"));
}

#[test]
fn the_oldest_stranger_makes_room_and_a_full_table_of_tokens_is_busy() {
    let (text, token) = token(Scope::Read, None);
    let fake = fake(vec![token], |_| {});
    let port = fake.server.addr().port();
    // 64 connections that have shown nothing: a new request evicts the oldest, not the person.
    let mut silent: Vec<TcpStream> = (0..64)
        .map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap())
        .collect();
    // They are accepted in order, so the 65th request finds the table full.
    assert_eq!(
        fake.call(req("GET", "/v1/state").token(&text)).status,
        200,
        "served, not busy"
    );
    silent[0]
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    use std::io::Read;
    assert!(
        matches!(silent[0].read(&mut [0u8; 1]), Ok(0) | Err(_)),
        "the oldest stranger was let go"
    );
    assert!(
        silent[63]
            .set_read_timeout(Some(Duration::from_millis(50)))
            .is_ok()
            && silent[63].read(&mut [0u8; 1]).is_err(),
        "the newest was not (it just waits)"
    );
    silent.clear();
}

#[test]
fn a_token_holds_sixteen_connections_and_sixty_four_authenticated_ones_fill_the_table() {
    let tokens: Vec<_> = (0..4).map(|_| token(Scope::Read, None)).collect();
    let fake = fake(tokens.iter().map(|(_, t)| t.clone()).collect(), |_| {});
    let port = fake.server.addr().port();
    let mut held: Vec<TcpStream> = Vec::new();
    for (text, _) in &tokens {
        held.extend((0..16).map(|_| connected(port, text)));
    }
    assert_eq!(fake.server.client_count(), 64);
    // A 65th has nowhere to sit: none of the 64 is a stranger.
    let busy = fake.call(req("GET", "/v1/state").token(&tokens[0].0));
    assert_eq!((busy.status, busy.code()), (503, "busy"));
    // Free one of the first token's: the next connection of its own is its 16th again.
    held.remove(0);
    wait_for("room", || fake.server.client_count() == 63);
    let first = connected(port, &tokens[0].0);
    held.push(first);
    wait_for("full again", || fake.server.client_count() == 64);
    held.clear();
    wait_for("all gone", || fake.server.client_count() == 0);
    // One token, 17 connections.
    let mut mine: Vec<TcpStream> = (0..16).map(|_| connected(port, &tokens[1].0)).collect();
    let refused = fake.call(req("GET", "/v1/state").token(&tokens[1].0));
    assert_eq!(
        (refused.status, refused.code()),
        (429, "slow_down"),
        "{}",
        refused.text
    );
    assert_eq!(
        fake.call(req("GET", "/v1/state").token(&tokens[2].0))
            .status,
        200,
        "another token is fine"
    );
    mine.clear();
}

#[test]
fn a_call_whose_client_left_is_closed() {
    let (fake, text) = control_fake();
    let agent = {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(1)))
            .build();
        ureq::Agent::from(config)
    };
    let request = req("POST", "/v1/agents/b7k2m/task")
        .token(&text)
        .json(&json!({"task": "t", "wait": true}));
    let gave_up = agent.run(request.build(&fake.base));
    assert!(gave_up.is_err(), "the fake never answers a waiting task");
    wait_for("the close", || {
        !fake.notifications(templum_mcp::bus::CLOSED).is_empty()
    });
    let call = fake.calls().pop().unwrap();
    let closed = fake.notifications(templum_mcp::bus::CLOSED).pop().unwrap();
    assert_eq!(closed.conn, call.conn, "it names the call that was left");
    assert_eq!(closed.terminal, call.terminal);
}

#[test]
fn a_revoked_tokens_waiting_call_is_closed() {
    let (fake, text) = control_fake();
    let request = req("POST", "/v1/agents/b7k2m/task")
        .token(&text)
        .json(&json!({"task": "t", "wait": true}))
        .build(&fake.base);
    let client = std::thread::spawn(move || {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(20)))
            .build();
        ureq::Agent::from(config).run(request).is_err()
    });
    wait_for("the call to reach the UI", || !fake.calls().is_empty());
    fake.server.set_tokens(vec![]);
    wait_for("the close", || {
        !fake.notifications(templum_mcp::bus::CLOSED).is_empty()
    });
    assert!(client.join().unwrap(), "the revoked call got no answer");
}

#[test]
fn clients_uses_and_disconnect() {
    let (text, token) = token(Scope::Read, None);
    let id = token.id;
    let fake = fake(vec![token], |_| {});
    assert!(fake.server.clients().is_empty() && fake.server.uses().is_empty());
    // One agent, so the connection is kept for the next call.
    let agent = http();
    let first = fake.call(req("GET", "/v1/state").token(&text).agent(agent.clone()));
    assert_eq!(first.status, 200);
    let clients = fake.server.clients();
    assert_eq!(clients.len(), 1);
    assert_eq!(
        (
            clients[0].token,
            clients[0].name.as_str(),
            clients[0].addr.as_str()
        ),
        (id, "phone", "127.0.0.1")
    );
    assert!(!clients[0].streaming);
    let uses = fake.server.uses();
    assert_eq!(uses.len(), 1);
    assert_eq!((uses[0].0, uses[0].1.addr.as_str()), (id, "127.0.0.1"));
    assert!(uses[0].1.at > 1_700_000_000);
    assert!(fake.server.uses().is_empty(), "forgotten once told");
    fake.server.disconnect(clients[0].id);
    wait_for("the client to go", || fake.server.clients().is_empty());
    // The token still works.
    assert_eq!(fake.call(req("GET", "/v1/state").token(&text)).status, 200);
}

#[test]
fn the_audit_log_names_calls_but_no_secrets() {
    let (text, token) = token(Scope::Control, None);
    let fake = fake(vec![token], |_| {});
    let wrong = "tpl_thisIsNotAValidTokenButItIsASecretLookingOne";
    assert_eq!(fake.call(req("GET", "/v1/state").token(wrong)).status, 401);
    let body = json!({"board": "S", "title": "a secret title"});
    assert_eq!(
        fake.call(
            req("POST", "/v1/tools/create_card")
                .token(&text)
                .json(&body)
        )
        .status,
        200
    );
    assert_eq!(
        fake.call(req("GET", "/v1/blocks/b7k2m?x=1").token(&text))
            .status,
        200
    );
    let read = || logs(fake.dir.path());
    // Written by a thread of its own: it comes.
    wait_for("the last call in the log", || {
        read().contains("/v1/blocks/b7k2m")
    });
    let log = read();
    for wanted in [
        "\"connect\"",
        "\"auth_failed\"",
        "\"create_card\"",
        "/v1/blocks/b7k2m",
        "\"token\":\"phone\"",
        "\"status\":401",
    ] {
        assert!(log.contains(wanted), "{wanted} in\n{log}");
    }
    for secret in [wrong, &text, &text[4..], "a secret title", &hash(&text)] {
        assert!(!log.contains(secret), "the log holds a secret");
    }
    for line in log.lines() {
        serde_json::from_str::<Value>(line).unwrap();
    }
}

#[test]
fn strangers_make_one_audit_line_not_a_flood() {
    let (text, token) = token(Scope::Control, None);
    let fake = fake(vec![token], |_| {});
    let log = || logs(fake.dir.path());
    assert_eq!(
        fake.call(
            req("POST", "/v1/tools/create_card")
                .token(&text)
                .json(&json!({"board": "S", "title": "x"}))
        )
        .status,
        200
    );
    for _ in 0..40 {
        assert_eq!(fake.call(req("GET", "/v1/state")).status, 401);
    }
    assert_eq!(
        fake.call(
            req("POST", "/v1/tools/create_card")
                .token(&text)
                .json(&json!({"board": "S", "title": "y"}))
        )
        .status,
        200
    );
    wait_for("the calls in the log", || {
        log().matches("create_card").count() == 2
    });
    let log = log();
    assert_eq!(
        log.matches("auth_failed").count(),
        1,
        "one line for forty strangers' requests:\n{log}"
    );
    assert_eq!(
        log.matches("\"connect\"").count(),
        1,
        "nor one per connection:\n{log}"
    );
}

#[test]
fn clients_coming_and_going_are_told_to_the_ui() {
    let (text, token) = token(Scope::Read, None);
    let fake = fake(vec![token], |_| {});
    assert_eq!(fake.server.client_count(), 0);
    let agent = http();
    assert_eq!(
        fake.call(req("GET", "/v1/state").token(&text).agent(agent.clone()))
            .status,
        200
    );
    assert_eq!(fake.server.client_count(), 1);
    wait_for("told it came", || {
        fake.notifications(templum_api::CLIENTS).len() == 1
    });
    let id = fake.server.clients()[0].id;
    fake.server.disconnect(id);
    wait_for("told it went", || {
        fake.notifications(templum_api::CLIENTS).len() == 2
    });
    assert_eq!(fake.server.client_count(), 0);
}

#[test]
fn plain_http_off_loopback_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let settings = |listen, tls| Settings {
        listen,
        tls,
        port: 0,
        ..Default::default()
    };
    for listen in [
        Listen::All,
        Listen::Address("192.168.1.5".into()),
        Listen::Address("::".into()),
    ] {
        assert_eq!(
            Options::from(&settings(listen, Tls::Off), dir.path()).err(),
            Some("Off this computer, the API needs HTTPS.".to_string())
        );
    }
    assert!(
        Options::from(
            &settings(Listen::Address("127.0.0.1".into()), Tls::Off),
            dir.path()
        )
        .is_ok()
    );
    assert!(
        Options::from(
            &settings(Listen::Address("::1".into()), Tls::Off),
            dir.path()
        )
        .is_ok()
    );
    assert!(
        Options::from(
            &settings(Listen::Address("somewhere".into()), Tls::Off),
            dir.path()
        )
        .is_err()
    );
    // The same check stands in start(), for options made by hand.
    let mut options = Options::from(&settings(Listen::Loopback, Tls::Off), dir.path()).unwrap();
    options.addr = "0.0.0.0:0".parse().unwrap();
    let (tx, _rx) = futures::channel::mpsc::unbounded();
    assert_eq!(
        start(options, vec![], tx).err(),
        Some("Off this computer, the API needs HTTPS.".to_string())
    );
}

#[test]
fn a_port_in_use_says_so() {
    let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = held.local_addr().unwrap().port();
    let dir = tempfile::tempdir().unwrap();
    let options = Options::from(
        &Settings {
            port,
            ..Default::default()
        },
        dir.path(),
    )
    .unwrap();
    let (tx, _rx) = futures::channel::mpsc::unbounded();
    assert_eq!(
        start(options, vec![], tx).err(),
        Some(format!("Port {port} is in use: choose another port."))
    );
}

#[test]
fn dropping_the_server_closes_its_listener() {
    let (fake, text) = read_fake();
    let base = fake.base.clone();
    assert_eq!(req("GET", "/v1/state").token(&text).send(&base).status, 200);
    let port = fake.server.addr().port();
    drop(fake);
    wait_for("the port to close", || {
        TcpStream::connect(("127.0.0.1", port)).is_err()
    });
}

fn later() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600
}

#[test]
fn me_says_who_the_token_is_without_asking_the_ui() {
    let (text, mut phone) = token(Scope::Read, Some(vec!["w3"]));
    let end = later();
    phone.expires = Some(end);
    let fake = fake(vec![phone], |options| {
        options.computer = "winx-desktop".into()
    });
    let me = fake.call(req("GET", "/v1/me").token(&text));
    assert_eq!(me.status, 200);
    assert_eq!(
        me.json,
        json!({"name": "phone", "scope": "read", "workspaces": ["w3"], "expires": end,
            "share": null, "computer": "winx-desktop", "version": env!("CARGO_PKG_VERSION"),
            "api": 1})
    );
    assert!(
        fake.asked().iter().all(|s| s.method != templum_api::STATE),
        "the UI thread wasn't asked: {:?}",
        fake.asked()
    );
    // A share is its token's `s<id>`; a token for every workspace has no list.
    let (text, mut shared) = token(Scope::Full, None);
    shared.id = 4;
    shared.share = Some(templum_api::store::Share {
        workspace: "w3".into(),
        way: templum_api::store::Way::Public,
        address: "https://t.example.com".into(),
    });
    let fake = fake_with_one(shared);
    let me = fake.call(req("GET", "/v1/me").token(&text));
    assert_eq!(
        (me.json["share"].as_str(), me.json["workspaces"].is_null()),
        (Some("s4"), true)
    );
}

fn fake_with_one(token: templum_api::Token) -> Fake {
    fake(vec![token], |_| {})
}

#[test]
fn me_says_a_deleted_workspace_as_none() {
    let (text, own) = token(Scope::Full, Some(vec!["w3"]));
    let fake = fake(vec![own.clone()], |_| {});
    let mut gone = own;
    gone.workspaces = Some(vec![]);
    fake.server.set_tokens(vec![gone]);
    let me = fake.call(req("GET", "/v1/me").token(&text));
    assert_eq!(me.status, 200);
    assert_eq!(me.json["workspaces"], json!([]));
}

#[test]
fn shares_are_a_resource() {
    let (all, mut everyone) = token(Scope::Read, None);
    let (limited, mut one) = token(Scope::Read, Some(vec!["w3"]));
    (everyone.id, one.id) = (9001, 2);
    let fake = fake(vec![everyone, one], |_| {});
    let ids = |text: &str| -> Vec<String> {
        let list = fake.call(req("GET", "/v1/shares").token(text));
        assert_eq!(list.status, 200, "{}", list.text);
        list.json
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(ids(&all), ["s1", "s2"]);
    assert_eq!(ids(&limited), ["s2"]);
    let doc = fake.call(req("GET", "/v1/openapi.json").token(&all));
    for path in ["/v1/me", "/v1/shares"] {
        assert_eq!(
            doc.json["paths"][path]["get"]["x-templum-scope"], "read",
            "{path}"
        );
    }
}

#[test]
fn a_share_that_revokes_itself_gets_its_answer() {
    let (text, mut token) = token(Scope::Full, Some(vec!["w3"]));
    token.share = Some(templum_api::store::Share {
        workspace: "w3".into(),
        way: templum_api::store::Way::Public,
        address: "https://t.example.com".into(),
    });
    let fake = fake(vec![token], |_| {});
    // One agent: the next call rides the connection the share was revoked on.
    let agent = http();
    let gone = fake.call(
        req("DELETE", "/v1/shares/s4")
            .token(&text)
            .agent(agent.clone()),
    );
    assert_eq!(
        (gone.status, &gone.json["text"]),
        (200, &json!("Revoked s4."))
    );
    let next = fake.call(req("GET", "/v1/me").token(&text).agent(agent));
    assert_eq!(next.status, 401);
}

#[test]
fn a_tools_code_reaches_the_answer() {
    let (text, token) = token(Scope::Full, None);
    let fake = fake(vec![token], |_| {});
    let body = json!({"name": "Another computer", "scope": "full", "way": "public",
        "address": "https://t.example.com"});
    let answer = fake.call(
        req("POST", "/v1/workspaces/w3/shares")
            .token(&text)
            .json(&body),
    );
    assert_eq!(answer.status, 200, "{}", answer.text);
    assert_eq!(
        answer.json,
        json!({"text": "Shared API as s4.", "code": "tplc1_x"})
    );
    let call = &fake.calls()[0];
    assert_eq!(call.params["arguments"]["workspace"], "w3");
    // The audit log says which tool was called, never the code that came back.
    wait_for("the call is logged", || {
        logs(fake.dir.path()).contains("share_workspace")
    });
    assert!(!logs(fake.dir.path()).contains("tplc1_x"));
    // A Control token can't share.
    let (weak, token) = token_of(Scope::Control);
    fake.server.set_tokens(vec![token]);
    let refused = fake.call(
        req("POST", "/v1/workspaces/w3/shares")
            .token(&weak)
            .json(&body),
    );
    assert_eq!((refused.status, refused.code()), (403, "scope"));
}

fn token_of(scope: Scope) -> (String, templum_api::Token) {
    token(scope, None)
}
