//! The OpenAPI 3.1 document (`GET /v1/openapi.json`), generated from the registry: a path for
//! every route and every tool, each with the scope it takes. The schemas of the state and of the
//! errors are written by hand beside the types they describe (`model.rs`); a test walks a state
//! against them.

use crate::registry::{self, Does, ROUTES, Resource};
use serde_json::{Map, Value, json};

const SCHEMAS: &str = "#/components/schemas/";

fn r(name: &str) -> Value {
    json!({ "$ref": format!("{SCHEMAS}{name}") })
}

fn nullable(schema: Value) -> Value {
    json!({ "anyOf": [schema, { "type": "null" }] })
}

fn array(items: Value) -> Value {
    json!({ "type": "array", "items": items })
}

fn kind(name: &str) -> Value {
    json!({ "type": name })
}

/// A scalar or nothing.
fn maybe(name: &str) -> Value {
    json!({ "type": [name, "null"] })
}

/// An object whose every property is always there (a view serializes its nulls).
fn view(properties: &[(&str, Value)]) -> Value {
    let required: Vec<&str> = properties.iter().map(|(name, _)| *name).collect();
    json!({
        "type": "object",
        "properties": properties.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<Map<String, Value>>(),
        "required": required,
    })
}

/// `components.schemas`: the state and its parts, the event's notice, the errors and a tool's
/// answer.
fn schemas() -> Value {
    let (s, i, n, b) = (
        || kind("string"),
        || kind("integer"),
        || kind("number"),
        || kind("boolean"),
    );
    json!({
        "State": view(&[
            ("app", r("App")),
            ("workspaces", array(r("WorkspaceView"))),
            ("blocks", array(r("BlockView"))),
            ("wires", array(r("WireView"))),
            ("requests", array(r("RequestView"))),
            ("boards", array(r("BoardView"))),
            ("cards", array(r("CardView"))),
            ("notes", array(r("NoteView"))),
            ("checklists", array(r("ChecklistView"))),
            ("drawings", array(r("DrawingsView"))),
            ("automations", array(r("AutomationView"))),
            ("shares", array(r("Share"))),
            ("view", nullable(r("View"))),
            ("usage", nullable(r("UsageView"))),
        ]),
        "App": view(&[
            ("version", s()),
            ("api", i()),
            ("shown", maybe("string")),
        ]),
        "WorkspaceView": view(&[
            ("id", s()),
            ("name", s()),
            ("folder", maybe("string")),
            ("color", maybe("string")),
            ("shown", b()),
            ("stopped", b()),
            ("icon", maybe("string")),
            ("opened", maybe("integer")),
        ]),
        "Rect": view(&[("x", n()), ("y", n()), ("w", n()), ("h", n())]),
        "BlockView": view(&[
            ("id", s()),
            ("workspace", s()),
            ("kind", s()),
            ("title", s()),
            ("folder", maybe("string")),
            ("rect", r("Rect")),
            ("z", i()),
            ("opened_by", maybe("string")),
            ("board", maybe("integer")),
            ("agent", nullable(r("AgentView"))),
        ]),
        "AgentView": view(&[
            ("agent", s()),
            ("state", json!({ "type": "string", "enum": ["needs_you", "starting", "waiting_subagents", "working", "done", "idle"] })),
            ("asks", maybe("string")),
            ("subagents", i()),
            ("since", maybe("integer")),
            ("model", maybe("string")),
            ("effort", maybe("string")),
            ("usage", nullable(r("SessionView"))),
        ]),
        "SessionView": view(&[
            ("context_percent", maybe("number")),
            ("tokens", maybe("integer")),
            ("cost_usd", maybe("number")),
        ]),
        "WireView": view(&[("a", s()), ("b", s()), ("label", maybe("string"))]),
        "RequestView": view(&[
            ("id", s()),
            ("kind", json!({ "type": "string", "enum": ["question", "plan", "connect", "remove_worktree", "merge_worktree", "approval"] })),
            ("block", maybe("string")),
            ("workspace", maybe("string")),
            ("since", i()),
            ("summary", s()),
            ("why", r("Why")),
            ("questions", array(r("QuestionView"))),
            ("plan", nullable(r("PlanView"))),
            ("buttons", json!({ "type": ["array", "null"], "items": s() })),
            ("stopped", b()),
            ("scope", json!({ "type": "string", "enum": ["read", "control", "full"] })),
            ("tool", maybe("string")),
            ("said", maybe("string")),
            ("details", maybe("string")),
            ("facts", maybe("string")),
            ("reason", maybe("string")),
        ]),
        "Why": view(&[
            ("card", nullable(r("CardRef"))),
            ("task", maybe("string")),
            ("agent", maybe("string")),
        ]),
        "CardRef": view(&[("id", i()), ("title", s())]),
        "QuestionView": view(&[
            ("question", s()),
            ("header", maybe("string")),
            ("multi", b()),
            ("options", array(r("OptionView"))),
        ]),
        "OptionView": view(&[
            ("label", s()),
            ("description", maybe("string")),
            ("preview", maybe("string")),
        ]),
        "PlanView": view(&[("title", maybe("string")), ("text", s())]),
        "BoardView": view(&[("id", i()), ("name", s()), ("columns", array(r("ColumnView")))]),
        "ColumnView": view(&[("id", i()), ("title", s()), ("cards", array(i()))]),
        "CardView": view(&[
            ("id", i()),
            ("board", i()),
            ("column", i()),
            ("title", s()),
            ("description", s()),
            ("labels", array(s())),
            ("priority", maybe("string")),
            ("assignee", nullable(r("Assignee"))),
            ("checklist", array(r("ItemView"))),
            ("comments", array(r("CommentView"))),
            ("attachments", array(r("AttachmentView"))),
        ]),
        "NoteView": view(&[
            ("id", i()),
            ("name", s()),
            ("text", s()),
            ("color", maybe("string")),
            ("cut", b()),
            ("rev", i()),
            ("changed", i()),
            ("workspace", maybe("string")),
        ]),
        "ChecklistView": view(&[("id", i()), ("name", s()), ("items", array(r("ItemView")))]),
        "DrawingsView": view(&[("id", s()), ("drawings", array(r("Drawing")))]),
        "NotesShown": view(&[("tabs", array(i())), ("shown", maybe("integer"))]),
        "Assignee": view(&[("block", maybe("string")), ("agent", maybe("string"))]),
        "ItemView": view(&[("id", i()), ("text", s()), ("done", b())]),
        "CommentView": view(&[
            ("number", i()),
            ("by", s()),
            ("text", s()),
            ("at", i()),
            ("edited", b()),
        ]),
        "AttachmentView": view(&[("name", s()), ("size", i())]),
        "AutomationView": view(&[
            ("id", i()),
            ("name", s()),
            ("on", b()),
            ("workspace", maybe("string")),
            ("when", json!({ "description": "As automations.json keeps it: {\"daily\": {\"at\": \"09:00:00\"}}." })),
            ("do", json!({ "description": "As automations.json keeps it: {\"notify\": {\"text\": \"…\"}}." })),
            ("max_runs", maybe("integer")),
            ("cooldown", maybe("integer")),
            ("runs", i()),
            ("last", json!({ "description": "{at, went, about} of its last run, or null." })),
            ("next", maybe("integer")),
            ("sentence", s()),
        ]),
        "UsageView": view(&[("limits", array(r("LimitsView")))]),
        "LimitsView": view(&[("agent", s()), ("at", i()), ("windows", array(r("WindowView")))]),
        "WindowView": view(&[
            ("minutes", i()),
            ("used_percent", n()),
            ("resets_at", i()),
            ("scope", maybe("string")),
        ]),
        "Notice": view(&[
            ("text", s()),
            ("block", maybe("string")),
            ("workspace", maybe("string")),
        ]),
        "Error": view(&[(
            "error",
            view(&[("code", s()), ("message", s())]),
        )]),
        "ToolResult": json!({
            "type": "object",
            "description": "The tool's text, and what it touched when Templum knows it; share_workspace adds its connection code, git its result.",
            "properties": { "text": s(), "workspace": s(), "block": s(), "code": s() },
            "required": ["text"],
        }),
    })
}

/// `components.schemas` of the camera and of a terminal stream's frames (apart from `schemas`:
/// a `json!` that big hits the macro's recursion limit).
fn view_schemas() -> Value {
    let (s, i, n, b) = (
        || kind("string"),
        || kind("integer"),
        || kind("number"),
        || kind("boolean"),
    );
    // `modes` is optional: a home older than the field sends frames without it.
    let mut frame = view(&[
        ("size", view(&[("cols", i()), ("rows", i())])),
        (
            "cursor",
            nullable(view(&[
                ("row", i()),
                ("col", i()),
                (
                    "shape",
                    json!({ "type": "string", "enum": ["block", "underline", "beam"] }),
                ),
                ("blinking", b()),
            ])),
        ),
        ("title", maybe("string")),
        ("whole", b()),
        ("rows", array(r("FrameRow"))),
    ]);
    frame["properties"]["modes"] = json!({
        "type": "object",
        "description": "What the program took at home; a flag is present only when true. Every frame carries it, and a frame whose only change is the modes has no rows.",
        "properties": { "full_screen": b(), "mouse": b(), "kitty_keys": b(), "mouse_moves": b() },
    });
    json!({
        "View": view(&[
            ("workspace", s()),
            ("x", n()),
            ("y", n()),
            ("w", n()),
            ("h", n()),
            ("zoom", n()),
        ]),
        "Frame": frame,
        "FrameRow": view(&[("y", i()), ("runs", array(r("FrameRun")))]),
        "FrameRun": json!({
            "type": "object",
            "description": "Text of one style. A color is absent for the theme's default; a flag is present only when true.",
            "properties": {
                "text": s(),
                "fg": r("Paint"),
                "bg": r("Paint"),
                "bold": b(),
                "dim": b(),
                "italic": b(),
                "underline": b(),
                "inverse": b(),
            },
            "required": ["text"],
        }),
        "Paint": json!({
            "type": ["integer", "string"],
            "description": "A palette slot 0-255 (the viewer's palette draws it) or \"#rrggbb\".",
        }),
    })
}

/// The errors a route can answer, with the spec's codes.
fn error(status: &str) -> (&'static str, &'static str) {
    match status {
        "400" => (
            "invalid",
            "Malformed JSON, an argument that doesn't parse, or a tool's argument error.",
        ),
        "401" => (
            "auth",
            "No token, a wrong, expired or revoked one, a used or stale ticket.",
        ),
        "403" => ("scope", "The token's scope is too low."),
        "404" => (
            "not_found",
            "No such route, tool or resource (a target the token doesn't see is this too).",
        ),
        "413" => ("too_large", "A body over 1 MiB."),
        "415" => (
            "json",
            "A POST, PUT or PATCH whose Content-Type isn't application/json.",
        ),
        "422" => (
            "refused",
            "The tool refused; its sentence says what to do next.",
        ),
        "429" => (
            "slow_down",
            "Too many failed authentications from the address, too many calls from the token, or too many streams. Retry-After says when.",
        ),
        _ => ("busy", "Too many connections, or Templum is quitting."),
    }
}

fn responses(ok: Value, statuses: &[&str]) -> Value {
    let mut out = Map::from_iter([("200".to_string(), ok)]);
    for status in statuses {
        let (code, why) = error(status);
        out.insert(
            status.to_string(),
            json!({
                "description": format!("`{code}`: {why}"),
                "content": { "application/json": { "schema": r("Error") } },
            }),
        );
    }
    Value::Object(out)
}

fn json_ok(schema: Value) -> Value {
    json!({ "description": "Done.", "content": { "application/json": { "schema": schema } } })
}

/// What a resource answers.
fn resource(resource: Resource) -> Value {
    match resource {
        Resource::State => r("State"),
        Resource::Workspaces => array(r("WorkspaceView")),
        Resource::Workspace => r("WorkspaceView"),
        Resource::Blocks | Resource::Agents => array(r("BlockView")),
        Resource::Block => r("BlockView"),
        Resource::Requests => array(r("RequestView")),
        Resource::Request => r("RequestView"),
        Resource::Boards => array(r("BoardView")),
        Resource::Board => r("BoardView"),
        Resource::Card => r("CardView"),
        Resource::Automations => array(r("AutomationView")),
        Resource::Automation => r("AutomationView"),
        Resource::Shares => array(r("Share")),
        Resource::Usage => r("UsageView"),
    }
}

/// `{x}` segments of a path, as path parameters.
fn path_parameters(path: &str) -> Vec<Value> {
    path.split('/')
        .filter_map(|segment| segment.strip_prefix('{')?.strip_suffix('}'))
        .map(|name| json!({ "name": name, "in": "path", "required": true, "schema": kind("string") }))
        .collect()
}

fn operation_id(method: &str, path: &str) -> String {
    let words: String = path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{}{}", method.to_lowercase(), words.trim_end_matches('_'))
}

fn tool_schema(name: &str) -> Value {
    registry::tools()
        .into_iter()
        .find(|tool| tool["name"] == name)
        .unwrap_or(Value::Null)
}

/// One operation of a route.
fn operation(route: &registry::Route) -> Value {
    let body = matches!(route.method, "POST" | "PUT" | "PATCH");
    let mut parameters = path_parameters(route.path);
    let mut op = json!({ "operationId": operation_id(route.method, route.path) });
    let (summary, scope, ok) = match route.does {
        Does::Read(what) => (
            format!("Reads {}.", route.path.trim_start_matches("/v1/")),
            "read".to_string(),
            json_ok(resource(what)),
        ),
        Does::Events => {
            op["description"] = json!(
                "Server-Sent Events: a `snapshot` of the state, then a `put` or `delete` per change of a \
                 collection (workspace, block, wire, request, board, card, automation, share, usage) and `notice` \
                 happenings; `: ping` comments keep it alive. With `view=true`, a `view` event (the \
                 shown workspace's camera, or null) each time it changes. Resumes with Last-Event-ID. \
                 A share's token opening it with a new `Templum-Mirror` session first sends the streams of \
                 its earlier session `event: replaced` (data `{}`) and ends them. \
                 A browser's EventSource can't set headers: it opens with a ticket from POST \
                 /v1/tickets."
            );
            op["security"] = json!([{ "bearer": [] }, { "ticket": [] }]);
            parameters.push(json!({
                "name": "Templum-Mirror",
                "in": "header",
                "schema": { "type": "string", "maxLength": 64 },
                "description": "A mirror's session, new at each connect: a share's token opening this stream with another session than its earlier connections carry replaces them."
            }));
            parameters.push(json!({
                "name": "view",
                "in": "query",
                "schema": { "type": "boolean" },
                "description": "true: the stream also carries `event: view` (the camera of the shown workspace, schema View, or null)."
            }));
            (
                "Streams the state's changes.".to_string(),
                "read".to_string(),
                json!({ "description": "An event stream.", "content": { "text/event-stream": { "schema": kind("string") } } }),
            )
        }
        Does::Frames => {
            op["description"] = json!(
                "Server-Sent Events: a terminal's live screen as `event: frame` (the schema \
                 `#/components/schemas/Frame`): first the whole screen, then only the rows that \
                 changed, at most 20 a second; a client that falls behind gets the newest screen \
                 whole. The stream ends when the block closes or leaves the token's workspaces. \
                 `: ping` comments keep it alive. A browser's EventSource can't set headers: it \
                 opens with a ticket from POST /v1/tickets."
            );
            op["security"] = json!([{ "bearer": [] }, { "ticket": [] }]);
            (
                "Streams a terminal's screen.".to_string(),
                "read".to_string(),
                json!({ "description": "An event stream.", "content": { "text/event-stream": { "schema": kind("string") } } }),
            )
        }
        Does::Me => (
            "Who the token is: its name, scope, workspaces, expiry and share, and this computer's name and Templum's version.".to_string(),
            "read".to_string(),
            json_ok(r("Me")),
        ),
        Does::Ticket => (
            "Makes a ticket: a key for one GET /v1/events or GET /v1/terminals/{block}/stream, 30 seconds, once.".to_string(),
            "read".to_string(),
            json_ok(view(&[
                ("ticket", kind("string")),
                ("expires_in", kind("integer")),
            ])),
        ),
        Does::Tools => (
            "Lists the registry: every tool's name, description, input schema and scope."
                .to_string(),
            "read".to_string(),
            json_ok(array(json!({ "type": "object" }))),
        ),
        Does::OpenApi => (
            "This document.".to_string(),
            "read".to_string(),
            json_ok(json!({ "type": "object" })),
        ),
        Does::Tool("") => (
            "Runs any tool of the registry by name; the body is its arguments.".to_string(),
            "per tool (see /v1/tools/{name})".to_string(),
            json_ok(r("ToolResult")),
        ),
        Does::Tool(name) => {
            let tool = tool_schema(name);
            if !body && let Some(props) = tool["inputSchema"]["properties"].as_object() {
                for (key, schema) in props {
                    if !route.path.contains(&format!("{{{key}}}")) {
                        parameters.push(json!({ "name": key, "in": "query", "schema": schema }));
                    }
                }
            }
            if body {
                // A saved automation's body is the automation itself.
                let schema = if name == "save_automation" {
                    tool["inputSchema"]["properties"]["automation"].clone()
                } else {
                    tool["inputSchema"].clone()
                };
                op["requestBody"] = json!({
                    "required": false,
                    "content": { "application/json": { "schema": schema } },
                });
            }
            (
                tool["description"].as_str().unwrap_or_default().to_string(),
                registry::nominal(name).to_string(),
                json_ok(r("ToolResult")),
            )
        }
    };
    op["summary"] = json!(summary);
    op["x-templum-scope"] = json!(scope);
    if !parameters.is_empty() {
        op["parameters"] = Value::Array(parameters);
    }
    let mut statuses = vec!["401", "404", "429", "503"];
    if matches!(route.does, Does::Frames) {
        statuses.push("422");
    }
    if matches!(route.does, Does::Tool(_)) {
        statuses.splice(0..0, ["400", "403"]);
        if body {
            statuses.extend(["413", "415"]);
        }
        statuses.push("422");
    }
    op["responses"] = responses(ok, &statuses);
    op
}

/// Every schema: the state's and the errors', then the camera's and the frames', then the shares'.
fn schemas_with_views() -> Value {
    let mut all = schemas();
    if let (Some(all), Value::Object(more)) = (all.as_object_mut(), view_schemas()) {
        all.extend(more);
    }
    let (s, i, b) = (|| kind("string"), || kind("integer"), || kind("boolean"));
    let scope = json!({ "type": "string", "enum": ["read", "control", "full"] });
    // The git tool's answer: its result beside the text, and git's stderr beside a refusal's message.
    all["ToolResult"]["properties"]["git"] =
        json!({ "description": "The git tool's result: its JSON depends on op." });
    all["Error"]["properties"]["error"]["properties"]["details"] = json!({
        "type": "string",
        "description": "A 422 of the git tool: git's whole stderr, at most 64 KiB.",
    });
    // A flag left out when false is optional, so it is added beside the always-present ones.
    all["BlockView"]["properties"]["read_only"] = json!({
        "type": "boolean",
        "description": "A terminal that takes no input (a subagent's transcript); left out otherwise.",
    });
    // Set only on a note block, and on a checklist block that shows a checklist.
    all["BlockView"]["properties"]["notes"] = r("NotesShown");
    all["BlockView"]["properties"]["checklist"] = i();
    // An agent's task and role, the deliveries agents asked for, and the rule a notice comes from.
    all["TaskView"] = view(&[
        ("id", s()),
        ("text", s()),
        ("by", maybe("string")),
        ("since", i()),
    ]);
    all["ScheduleView"] = view(&[
        ("id", s()),
        ("by", maybe("string")),
        ("to", s()),
        ("text", s()),
        ("next", i()),
        ("every", maybe("integer")),
        ("until", i()),
    ]);
    all["AgentView"]["properties"]["task"] = nullable(r("TaskView"));
    all["AgentView"]["properties"]["role"] = maybe("string");
    all["State"]["properties"]["schedules"] = array(r("ScheduleView"));
    all["Notice"]["properties"]["automation"] = i();
    // A drawing is a stroke (`points`) or a shape (`from`, `to`); `highlight` is left out when false.
    let point = || json!({ "type": "array", "items": { "type": "number" } });
    all["Drawing"] = view(&[("kind", s()), ("color", s()), ("width", kind("number"))]);
    all["Drawing"]["properties"]["highlight"] = b();
    all["Drawing"]["properties"]["points"] = array(point());
    all["Drawing"]["properties"]["from"] = point();
    all["Drawing"]["properties"]["to"] = point();
    all["Remote"] = view(&[
        ("computer", maybe("string")),
        ("host", s()),
        (
            "way",
            json!({ "type": "string", "enum": ["pinned", "public", "loopback"] }),
        ),
        (
            "status",
            json!({ "type": "string", "enum": ["connecting", "live", "reconnecting", "cut", "refused", "disconnected"] }),
        ),
        ("latency_ms", maybe("integer")),
    ]);
    // Set only on a workspace this Templum mirrors from another computer, so optional.
    all["WorkspaceView"]["properties"]["remote"] = r("Remote");
    all["Share"] = view(&[
        ("id", s()),
        ("workspace", s()),
        ("name", s()),
        ("scope", scope.clone()),
        ("created", i()),
        ("expires", maybe("integer")),
        (
            "way",
            json!({ "type": "string", "enum": ["public", "pinned"] }),
        ),
        ("address", s()),
        ("last", nullable(view(&[("at", i()), ("addr", s())]))),
        ("mirrored", b()),
    ]);
    all["Me"] = view(&[
        ("name", s()),
        ("scope", scope),
        (
            "workspaces",
            json!({ "type": ["array", "null"], "items": s() }),
        ),
        ("expires", maybe("integer")),
        ("share", maybe("string")),
        ("computer", s()),
        ("version", s()),
        ("api", i()),
    ]);
    all
}

/// The document.
pub fn document() -> Value {
    let mut paths: Map<String, Value> = Map::new();
    for route in ROUTES {
        paths
            .entry(route.path)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("a path is an object")
            .insert(route.method.to_lowercase(), operation(route));
    }
    for tool in registry::tools() {
        let name = tool["name"].as_str().unwrap_or_default();
        let path = format!("/v1/tools/{name}");
        let post = json!({
            "operationId": format!("tool_{name}"),
            "summary": tool["description"],
            "x-templum-scope": registry::nominal(name),
            "requestBody": { "required": true, "content": { "application/json": { "schema": tool["inputSchema"] } } },
            "responses": responses(
                json_ok(r("ToolResult")),
                &["400", "401", "403", "404", "413", "415", "422", "429", "503"],
            ),
        });
        paths.insert(path, json!({ "post": post }));
    }
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "The Templum API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Your Templum, from anywhere: tools, resources and a live event stream. Every request carries `Authorization: Bearer <token>`; a token is made in Settings ▸ API.",
        },
        "servers": [{ "url": "http://127.0.0.1:7411", "description": "Templum on this computer, with the API turned on." }],
        "security": [{ "bearer": [] }],
        "paths": paths,
        "components": {
            "securitySchemes": {
                "bearer": { "type": "http", "scheme": "bearer" },
                "ticket": { "type": "apiKey", "in": "query", "name": "ticket", "description": "From POST /v1/tickets; opens one GET /v1/events or GET /v1/terminals/{block}/stream." },
            },
            "schemas": schemas_with_views(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use std::sync::Arc;

    #[test]
    fn the_document_has_every_route_and_tool() {
        let doc = document();
        assert_eq!(doc["openapi"], "3.1.0");
        assert_eq!(doc["info"]["version"], env!("CARGO_PKG_VERSION"));
        for route in ROUTES {
            let op = &doc["paths"][route.path][route.method.to_lowercase()];
            assert!(op.is_object(), "{} {}", route.method, route.path);
            assert!(op["x-templum-scope"].is_string() && op["responses"]["200"].is_object());
        }
        for tool in registry::tools() {
            let path = format!("/v1/tools/{}", tool["name"].as_str().unwrap());
            assert!(doc["paths"][&path]["post"].is_object(), "{path}");
        }
        assert_eq!(
            doc["paths"]["/v1/tools/send_to_terminal"]["post"]["x-templum-scope"],
            "full"
        );
        assert_eq!(
            doc["paths"]["/v1/tools/overview"]["post"]["x-templum-scope"],
            "read"
        );
        assert_eq!(
            doc["paths"]["/v1/tools/answer_request"]["post"]["x-templum-scope"],
            "control or full"
        );
        assert_eq!(
            doc["paths"]["/v1/terminals/{block}/input"]["post"]["x-templum-scope"],
            "full"
        );
        assert_eq!(doc["paths"]["/v1/state"]["get"]["x-templum-scope"], "read");
        // The mirror's header and the `replaced` event are told where a mirror opens its stream.
        let events = &doc["paths"]["/v1/events"]["get"];
        assert!(
            events["description"]
                .as_str()
                .unwrap()
                .contains("event: replaced")
        );
        assert!(
            events["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == "Templum-Mirror" && p["in"] == "header")
        );
        assert_eq!(doc["security"], json!([{ "bearer": [] }]));
        assert_eq!(
            doc["paths"]["/v1/events"]["get"]["security"][1],
            json!({ "ticket": [] })
        );
        // A path parameter is declared, a tool's other arguments of a GET are query parameters.
        let read = &doc["paths"]["/v1/terminals/{block}/text"]["get"]["parameters"];
        let names: Vec<&str> = read
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        let mut names = names;
        names.sort_unstable();
        assert_eq!(names, ["block", "last_command", "lines"]);
        // Operation ids are unique.
        let mut ids = std::collections::HashSet::new();
        for (_, path) in doc["paths"].as_object().unwrap() {
            for (_, op) in path.as_object().unwrap() {
                assert!(
                    ids.insert(op["operationId"].as_str().unwrap().to_string()),
                    "{op}"
                );
            }
        }
        // A $ref anywhere resolves.
        fn refs(value: &Value, doc: &Value) {
            match value {
                Value::Object(map) => {
                    if let Some(Value::String(target)) = map.get("$ref") {
                        let pointer = target.trim_start_matches('#');
                        assert!(doc.pointer(pointer).is_some(), "{target}");
                    }
                    map.values().for_each(|v| refs(v, doc));
                }
                Value::Array(items) => items.iter().for_each(|v| refs(v, doc)),
                _ => {}
            }
        }
        refs(&doc, &doc);
    }

    /// `value` against `schema`: what the document promises. A small checker: `$ref`, `anyOf`,
    /// `type` (one or a list), `properties` (no key outside them), `required` and `items`.
    fn check(schema: &Value, value: &Value, doc: &Value, at: &str) -> Result<(), String> {
        if let Some(target) = schema["$ref"].as_str() {
            let schema = doc
                .pointer(target.trim_start_matches('#'))
                .ok_or(format!("{at}: no {target}"))?;
            return check(schema, value, doc, at);
        }
        if let Some(options) = schema["anyOf"].as_array() {
            return if options.iter().any(|o| check(o, value, doc, at).is_ok()) {
                Ok(())
            } else {
                Err(format!("{at}: none of the options fit {value}"))
            };
        }
        let types: Vec<&str> = match &schema["type"] {
            Value::String(one) => vec![one],
            Value::Array(list) => list.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if !types.is_empty() {
            let fits = |t: &str| match t {
                "null" => value.is_null(),
                "string" => value.is_string(),
                "boolean" => value.is_boolean(),
                "integer" => value.is_i64() || value.is_u64(),
                "number" => value.is_number(),
                "array" => value.is_array(),
                "object" => value.is_object(),
                _ => false,
            };
            if !types.iter().any(|t| fits(t)) {
                return Err(format!("{at}: {value} is not {types:?}"));
            }
        }
        if let (Some(properties), Some(object)) =
            (schema["properties"].as_object(), value.as_object())
        {
            for (key, inner) in object {
                let property = properties
                    .get(key)
                    .ok_or(format!("{at}: the schema has no \"{key}\""))?;
                check(property, inner, doc, &format!("{at}.{key}"))?;
            }
            for key in schema["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !object.contains_key(key) {
                    return Err(format!("{at}: \"{key}\" is missing"));
                }
            }
        }
        if let (Some(items), Some(list)) = (schema.get("items"), value.as_array()) {
            for (i, item) in list.iter().enumerate() {
                check(items, item, doc, &format!("{at}[{i}]"))?;
            }
        }
        Ok(())
    }

    /// One of everything, every option Some and every list one item long.
    fn full_state() -> State {
        State {
            app: App {
                version: "0.1.5".into(),
                api: 1,
                shown: Some("w3".into()),
            },
            workspaces: vec![Arc::new(WorkspaceView {
                id: "w3".into(),
                name: "API".into(),
                folder: Some("/p".into()),
                color: Some("blue".into()),
                shown: true,
                stopped: false,
                icon: Some("rocket".into()),
                opened: Some(1_761_559_000),
                remote: Some(RemoteView {
                    computer: Some("winx-desktop".into()),
                    host: "templum.example.com".into(),
                    way: "public".into(),
                    status: "live".into(),
                    latency_ms: Some(84),
                }),
            })],
            blocks: vec![Arc::new(BlockView {
                id: "b7k2m".into(),
                workspace: "w3".into(),
                kind: "terminal".into(),
                title: "Claude".into(),
                folder: Some("/p".into()),
                rect: Rect {
                    x: 1.0,
                    y: 2.0,
                    w: 3.0,
                    h: 4.0,
                },
                z: 3,
                opened_by: Some("templum-agent".into()),
                board: Some(3),
                notes: Some(NotesShown {
                    tabs: vec![7],
                    shown: Some(7),
                }),
                checklist: Some(12),
                agent: Some(AgentView {
                    agent: "claude".into(),
                    state: "needs_you".into(),
                    asks: Some("Allow?".into()),
                    subagents: 1,
                    since: Some(5),
                    model: Some("opus".into()),
                    effort: Some("high".into()),
                    usage: Some(SessionView {
                        context_percent: Some(4.0),
                        tokens: Some(9),
                        cost_usd: Some(1.5),
                    }),
                    task: Some(TaskView {
                        id: "t7".into(),
                        text: "Ship it.".into(),
                        by: Some("api".into()),
                        since: 6,
                    }),
                    role: Some("reviewer".into()),
                }),
                read_only: true,
            })],
            wires: vec![Arc::new(WireView {
                a: "a".into(),
                b: "b".into(),
                label: Some("l".into()),
            })],
            requests: vec![Arc::new(RequestView {
                id: "r12".into(),
                kind: "question".into(),
                block: Some("b7k2m".into()),
                workspace: Some("w3".into()),
                since: 7,
                summary: "Which?".into(),
                why: Why {
                    card: Some(CardRef {
                        id: 152,
                        title: "Attachments".into(),
                    }),
                    task: Some("t7".into()),
                    agent: Some("because".into()),
                },
                questions: vec![QuestionView {
                    question: "Which?".into(),
                    header: Some("DB".into()),
                    multi: true,
                    options: vec![OptionView {
                        label: "A".into(),
                        description: Some("d".into()),
                        preview: Some("p".into()),
                    }],
                }],
                plan: Some(PlanView {
                    title: Some("t".into()),
                    text: "x".into(),
                }),
                buttons: Some(["Allow".into(), "Deny".into()]),
                stopped: false,
                scope: crate::store::Scope::Full,
                tool: Some("send_to_terminal".into()),
                said: Some("s".into()),
                details: Some("d".into()),
                facts: Some("f".into()),
                reason: Some("r".into()),
            })],
            boards: vec![Arc::new(BoardView {
                id: 3,
                name: "Sprint".into(),
                columns: vec![ColumnView {
                    id: 7,
                    title: "Doing".into(),
                    cards: vec![40],
                }],
            })],
            cards: vec![Arc::new(CardView {
                id: 40,
                board: 3,
                column: 7,
                title: "Login".into(),
                description: "d".into(),
                labels: vec!["l".into()],
                priority: Some("high".into()),
                assignee: Some(Assignee {
                    block: Some("b".into()),
                    agent: Some("claude".into()),
                }),
                checklist: vec![ItemView {
                    id: 41,
                    text: "Build".into(),
                    done: true,
                }],
                comments: vec![CommentView {
                    number: 51,
                    by: "you".into(),
                    text: "t".into(),
                    at: 9,
                    edited: true,
                }],
                attachments: vec![AttachmentView {
                    name: "shot.png".into(),
                    size: 48128,
                }],
            })],
            notes: vec![Arc::new(NoteView {
                id: 7,
                name: "Plan".into(),
                text: "first".into(),
                color: Some("blue".into()),
                cut: true,
                rev: 1_761_000_000_000,
                changed: 1_761_000_000,
                workspace: None,
            })],
            checklists: vec![Arc::new(ChecklistView {
                id: 12,
                name: "Release".into(),
                items: vec![ItemView {
                    id: 13,
                    text: "Tag it".into(),
                    done: true,
                }],
            })],
            drawings: vec![Arc::new(DrawingsView {
                id: "w3".into(),
                drawings: vec![
                    DrawingView {
                        kind: "stroke".into(),
                        color: "blue".into(),
                        width: 2.0,
                        highlight: true,
                        points: vec![[0.0, 0.0], [1.0, 1.0]],
                        from: None,
                        to: None,
                    },
                    DrawingView {
                        kind: "arrow".into(),
                        color: "red".into(),
                        width: 3.0,
                        highlight: false,
                        points: vec![],
                        from: Some([0.0, 0.0]),
                        to: Some([5.0, 5.0]),
                    },
                ],
            })],
            automations: vec![Arc::new(AutomationView {
                id: 1,
                name: "Morning".into(),
                on: true,
                workspace: Some("w3".into()),
                when: json!({"daily": {"at": "09:00:00"}}),
                act: json!({"notify": {"text": "x"}}),
                max_runs: Some(3),
                cooldown: Some(4),
                runs: 3,
                last: Some(json!({"at": 1, "went": "ran", "about": "x"})),
                next: Some(10),
                sentence: "Every day".into(),
            })],
            schedules: vec![Arc::new(ScheduleView {
                id: "s3".into(),
                by: Some("b7k2m".into()),
                to: "k9x1d".into(),
                text: "Check the deploy".into(),
                next: 8,
                every: Some(600),
                until: 9,
            })],
            shares: vec![Arc::new(ShareView {
                id: "s4".into(),
                workspace: "w3".into(),
                name: "Another computer".into(),
                scope: crate::store::Scope::Full,
                created: 1,
                expires: Some(2),
                way: crate::store::Way::Pinned,
                address: "192.168.1.5:7411".into(),
                last: Some(LastView {
                    at: 3,
                    addr: "203.0.113.7".into(),
                }),
                mirrored: true,
            })],
            view: Some(ViewView {
                workspace: "w3".into(),
                x: 1.0,
                y: 2.0,
                w: 800.0,
                h: 600.0,
                zoom: 1.5,
            }),
            usage: Some(UsageView {
                limits: vec![LimitsView {
                    agent: "claude".into(),
                    at: 8,
                    windows: vec![WindowView {
                        minutes: 300,
                        used_percent: 62.0,
                        resets_at: 9,
                        scope: Some("s".into()),
                    }],
                }],
            }),
        }
    }

    #[test]
    fn the_frame_schema_matches_a_frame() {
        use std::sync::Arc;
        use templum_terminal::{Palette, TermOptions, Terminal};
        let doc = document();
        let v = Terminal::viewer(
            "v",
            (20, 3),
            Palette::default(),
            TermOptions::default(),
            Arc::new(|| {}),
        );
        v.feed(b"\x1b]2;vim\x07\x1b[1;2;3;4;7;31;48;2;1;2;3mstyled\x1b[0m\x1b[38;5;200mx\x1b[6 q");
        let frame =
            serde_json::to_value(crate::terminals::whole(&v.shared().screen().unwrap())).unwrap();
        assert_eq!(frame["title"], "vim");
        check(&r("Frame"), &frame, &doc, "frame").unwrap();
        assert_eq!(
            frame["modes"],
            json!({}),
            "a frame with no mode has an empty one"
        );
        v.feed(b"\x1b[?1049h");
        let modal =
            serde_json::to_value(crate::terminals::whole(&v.shared().screen().unwrap())).unwrap();
        assert_eq!(modal["modes"], json!({"full_screen": true}));
        check(&r("Frame"), &modal, &doc, "frame").unwrap();
        // A home older than the field sends none.
        let mut older = frame.clone();
        older.as_object_mut().unwrap().remove("modes");
        check(&r("Frame"), &older, &doc, "frame").unwrap();
        let mut odd = frame.clone();
        odd["modes"]["blink"] = json!(true);
        assert!(
            check(&r("Frame"), &odd, &doc, "frame")
                .unwrap_err()
                .contains("blink")
        );
        let mut bare = frame.clone();
        bare["cursor"] = Value::Null;
        bare["title"] = Value::Null;
        check(&r("Frame"), &bare, &doc, "frame").unwrap();
        let mut blink = frame;
        blink["rows"][0]["runs"][0]["blink"] = json!(true);
        assert!(
            check(&r("Frame"), &blink, &doc, "frame")
                .unwrap_err()
                .contains("blink")
        );
    }

    #[test]
    fn the_state_schema_matches_the_state() {
        let doc = document();
        let state = serde_json::to_value(full_state()).unwrap();
        check(&r("State"), &state, &doc, "state").unwrap();
        assert_eq!(state["blocks"][0]["read_only"], true);
        assert_eq!(state["workspaces"][0]["icon"], "rocket");
        assert_eq!(state["workspaces"][0]["remote"]["status"], "live");
        let mut local = state.clone();
        local["workspaces"][0]
            .as_object_mut()
            .unwrap()
            .remove("remote");
        check(&r("State"), &local, &doc, "state").unwrap();
        let mut quiet = state.clone();
        for key in ["read_only", "notes", "checklist"] {
            quiet["blocks"][0].as_object_mut().unwrap().remove(key);
        }
        for key in ["highlight", "points"] {
            quiet["drawings"][0]["drawings"][0]
                .as_object_mut()
                .unwrap()
                .remove(key);
        }
        for key in ["from", "to"] {
            quiet["drawings"][0]["drawings"][1]
                .as_object_mut()
                .unwrap()
                .remove(key);
        }
        check(&r("State"), &quiet, &doc, "state").unwrap();
        // An agent without a task or a role has both null.
        let mut bare = state.clone();
        bare["blocks"][0]["agent"]["task"] = Value::Null;
        bare["blocks"][0]["agent"]["role"] = Value::Null;
        check(&r("State"), &bare, &doc, "state").unwrap();
        assert_eq!(state["schedules"][0]["every"], 600);
        let mut store = crate::store::Store::default();
        store
            .mint("phone", crate::store::Scope::Read, None, None, 1)
            .unwrap();
        let me = serde_json::to_value(MeView::of(&store.tokens[0], "winx-desktop")).unwrap();
        check(&r("Me"), &me, &doc, "me").unwrap();
        // The checker itself: a key the schema lacks, a key missing, a wrong type.
        let mut extra = state.clone();
        extra["blocks"][0]["surprise"] = json!(1);
        assert!(
            check(&r("State"), &extra, &doc, "state")
                .unwrap_err()
                .contains("surprise")
        );
        let mut missing = state.clone();
        missing["blocks"][0].as_object_mut().unwrap().remove("rect");
        assert!(
            check(&r("State"), &missing, &doc, "state")
                .unwrap_err()
                .contains("rect")
        );
        let mut wrong = state;
        wrong["blocks"][0]["z"] = json!("front");
        assert!(check(&r("State"), &wrong, &doc, "state").is_err());
        // An empty state, with every Option None, fits too.
        let empty = serde_json::to_value(State::default()).unwrap();
        check(&r("State"), &empty, &doc, "state").unwrap();
        // And so do the event's notice and the errors.
        let notice = serde_json::to_value(Notice {
            text: "t".into(),
            block: None,
            workspace: Some("w3".into()),
            automation: None,
        })
        .unwrap();
        check(&r("Notice"), &notice, &doc, "notice").unwrap();
        check(
            &r("Error"),
            &json!({"error": {"code": "auth", "message": "m"}}),
            &doc,
            "error",
        )
        .unwrap();
    }
}
