//! The `templum` command line (docs/specs/connections.md, The templum command line): the MCP
//! bridge, the agents' hooks, and a verb per tool.

use std::io::{self, BufRead as _, BufReader, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::bus::{self, Stream};
use crate::{hooks, rpc};

/// The first arguments that make `templum` a command instead of the app.
const VERBS: &[&str] = &[
    "mcp",
    "control",
    "hook",
    "whoami",
    "blocks",
    "read",
    "send",
    "notify",
    "ask",
    "connect",
    "delegate",
    "reply",
    "wait",
    "spawn",
    "kanban",
    "note",
    "checklist",
    "open",
    "wire",
    "keys",
    "interrupt",
    "stop",
    "worktree",
    "schedule",
    "unschedule",
    "plan",
    "new",
    "close",
];
/// How long a session's start waits for Templum's instructions (docs/specs/instructions.md): the
/// hooks' own limits are 5 s, and Templum answers in milliseconds.
const ANSWER_WITHIN: Duration = Duration::from_secs(2);
const READ: &str = "read <block> [-n <lines>] [--last] [--json]";
const ASK: &str = "ask <question> [--option <answer>]… [--json]";
const WAIT: &str = "wait <block> [--for idle|needs_you|command_finished|attention] \
                    [--match <pattern>] [--lines <n>] [--timeout <seconds>] [--json]";
const WIRE: &str = "wire <a> <b> [--json]";
const SPAWN: &str = "spawn [<agent>] [--fork] [--role <role>] [--folder <folder>] [--worktree <name>] \
                     [--prompt <text>] [--model <model>] [--effort <effort>] [--json]";
const KEYS: &str = "keys <block> <key>… [--json]";
const KANBAN: &str = "kanban list [<board>] [--card <card>] | add <title> [--column <column>] \
                      [--description <text>] [--board <board>] [--priority <priority>] [--for <who>] \
                      [--checklist <text>] | edit <card> [--priority <priority>] [--for <who>] [--add <text>] \
                      [--check <item>]… [--uncheck <item>]… | move <card> <column> | comment <card> <text> \
                      | edit-comment <card> <comment> <text> | delete-comment <card> <comment> \
                      | claim <card> [--json]";
const NOTE: &str = "note read <block> | note append <block> <text> \
                    | note replace <block> <text> [--old <text>] [--json]";
const SCHEDULE: &str = "schedule <block> <text> [--at HH:MM] [--after <minutes>] \
                        [--every <minutes>] [--json]";
const WORKTREE: &str = "worktree remove <name> [--folder <folder>] [--discard] | merge <name> [--folder <folder>] [--json]";
const PLAN: &str = "plan <file> [--title <title>] [--json]";
const NEW: &str = "new note [<text>] | new checklist <name> [--items <text>] \
                   | new terminal [--folder <folder>] [--command <command>] [--json]";
const CHECKLIST: &str = "checklist read <block> | add <block> <text> | check <block> <item> \
                         | uncheck <block> <item> | edit <block> <item> <text> | delete <block> <item> \
                         [--json]";

pub fn is_verb(arg: &str) -> bool {
    VERBS.contains(&arg)
}

/// Where a command runs: its terminal's `TEMPLUM_BUS` and `TEMPLUM_TERMINAL`.
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub bus: Option<String>,
    pub terminal: Option<String>,
    /// The Templum Agent's secret (`TEMPLUM_CONTROL`), only in its engine's environment.
    pub control: Option<String>,
}

impl Env {
    pub fn of_process() -> Env {
        Env {
            bus: std::env::var("TEMPLUM_BUS").ok(),
            terminal: std::env::var("TEMPLUM_TERMINAL").ok(),
            control: std::env::var("TEMPLUM_CONTROL").ok(),
        }
    }
}

/// Runs the command `args` (the verb first) and returns its exit code: 0 done, 1 refused or
/// failed, 2 a usage error, outside Templum, or Templum gone. A hook always returns 0 and prints
/// nothing but, as a session starts, the person's instructions: an agent mustn't stop over
/// Templum, and Claude Code reads a hook's output.
pub fn run(
    args: &[String],
    env: &Env,
    mut stdin: Box<dyn Read + Send>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let (verb, rest) = match args.split_first() {
        Some((verb, rest)) => (verb.as_str(), rest),
        None => ("", &[][..]),
    };
    if verb == "hook" {
        hook(rest, env, &mut *stdin, stdout);
        return 0;
    }
    if verb == "control" {
        return control(rest, env, stdin, stdout, stderr);
    }
    let call = if verb == "mcp" {
        None
    } else {
        match call_of(verb, rest) {
            Ok(mut call) => {
                if call.name == "propose_plan" {
                    let from = call.arguments["plan"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let mut text = String::new();
                    let read = match from.as_str() {
                        "-" => stdin.read_to_string(&mut text).map(|_| text),
                        file => std::fs::read_to_string(file),
                    };
                    match read {
                        Ok(text) => call.arguments["plan"] = json!(text),
                        Err(e) => {
                            let _ = writeln!(stderr, "templum: couldn't read {from}: {e}");
                            return 2;
                        }
                    }
                }
                // `reply -`: the answer is standard input, whatever its length, unquoted. Bytes
                // that aren't UTF-8 (a command's output in another encoding) become U+FFFD, not
                // a refusal.
                if call.name == "reply" && call.arguments["text"] == "-" {
                    let mut bytes = Vec::new();
                    match stdin.read_to_end(&mut bytes) {
                        Ok(_) => {
                            call.arguments["text"] = json!(String::from_utf8_lossy(&bytes));
                        }
                        Err(e) => {
                            let _ = writeln!(stderr, "templum: couldn't read stdin: {e}");
                            return 2;
                        }
                    }
                }
                Some(call)
            }
            Err(usage) => {
                let _ = writeln!(stderr, "usage: templum {usage}");
                return 2;
            }
        }
    };
    let client = if call.is_some() { "cli" } else { "mcp" };
    let stream = match open(env, client) {
        Ok(stream) => stream,
        Err(why) => {
            let _ = writeln!(stderr, "templum: {why}");
            return 2;
        }
    };
    match call {
        None => bridge(stdin, stdout, stream),
        Some(call) => one_call(stream, &call, stdout, stderr),
    }
}

/// A verb's tool call, and whether it prints the whole result.
struct CallArgs {
    name: &'static str,
    arguments: Value,
    json: bool,
}

/// `path` from the folder the command runs in, `cwd`: Templum would read a relative path from the
/// caller's folder, which for a command an agent runs is the agent's, not its shell tool's. `~` is
/// left for Templum, which reads it as home.
fn absolute(path: &str, cwd: Option<PathBuf>) -> String {
    let home = path
        .strip_prefix('~')
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '\\']));
    match cwd {
        Some(cwd) if !home => cwd.join(path).to_string_lossy().into_owned(),
        _ => path.to_string(),
    }
}

fn call_of(verb: &str, args: &[String]) -> Result<CallArgs, &'static str> {
    let json = args.iter().any(|a| a == "--json");
    let args: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--json")
        .collect();
    let (name, arguments) = match (verb, args.as_slice()) {
        ("whoami", []) => ("whoami", json!({})),
        ("blocks", []) => ("list_blocks", json!({})),
        ("read", [block, more @ ..]) => {
            let mut arguments = json!({ "block": block });
            let mut more = more.iter();
            while let Some(arg) = more.next() {
                match *arg {
                    "--last" => arguments["last_command"] = json!(true),
                    "-n" => {
                        let n: u64 = more.next().and_then(|n| n.parse().ok()).ok_or(READ)?;
                        arguments["lines"] = json!(n);
                    }
                    _ => return Err(READ),
                }
            }
            ("read_terminal", arguments)
        }
        ("send", [block, text @ ..]) if !text.is_empty() => {
            let enter = text.contains(&"--enter");
            let text: Vec<&str> = text.iter().copied().filter(|a| *a != "--enter").collect();
            (
                "send_to_terminal",
                json!({ "block": block, "text": text.join(" "), "enter": enter }),
            )
        }
        ("notify", text) if !text.is_empty() => ("notify", json!({ "text": text.join(" ") })),
        ("ask", words) => {
            let (mut question, mut options) = (Vec::new(), Vec::new());
            let mut words = words.iter();
            while let Some(word) = words.next() {
                if *word == "--option" {
                    options.push(*words.next().ok_or(ASK)?);
                } else {
                    question.push(*word);
                }
            }
            if question.is_empty() {
                return Err(ASK);
            }
            let mut arguments = json!({ "question": question.join(" ") });
            if !options.is_empty() {
                arguments["options"] = json!(options);
            }
            ("ask_user", arguments)
        }
        ("connect", [block]) => ("connect", json!({ "block": block })),
        ("delegate", [block, task @ ..]) if task.iter().any(|a| *a != "--wait") => {
            let wait = task.contains(&"--wait");
            let task: Vec<&str> = task.iter().copied().filter(|a| *a != "--wait").collect();
            (
                "delegate",
                json!({ "block": block, "task": task.join(" "), "wait": wait }),
            )
        }
        // The task its shell is on, answered with what comes on standard input (`run` reads it).
        ("reply", ["-"]) => ("reply", json!({ "text": "-" })),
        ("reply", [block, text @ ..]) if !text.is_empty() => {
            ("reply", json!({ "block": block, "text": text.join(" ") }))
        }
        ("wait", [block, more @ ..]) => {
            let mut arguments = json!({ "block": block });
            let mut more = more.iter();
            while let Some(flag) = more.next() {
                let value = *more.next().ok_or(WAIT)?;
                match *flag {
                    "--for" => arguments["event"] = json!(value),
                    "--match" => arguments["pattern"] = json!(value),
                    "--lines" => {
                        let lines: u64 = value.parse().map_err(|_| WAIT)?;
                        arguments["lines"] = json!(lines);
                    }
                    "--timeout" => {
                        let seconds: u64 = value.parse().map_err(|_| WAIT)?;
                        arguments["timeout"] = json!(seconds);
                    }
                    _ => return Err(WAIT),
                }
            }
            let name = if arguments.get("event").is_some() || arguments.get("pattern").is_some() {
                "wait_for"
            } else {
                "wait_for_idle"
            };
            (name, arguments)
        }
        ("spawn", all @ [_, ..]) => {
            // The agent may be left out of a fork, its first word then a flag.
            let (mut arguments, more) = match all {
                [agent, more @ ..] if !agent.starts_with("--") => (json!({ "agent": agent }), more),
                more => (json!({}), more),
            };
            let mut more = more.iter();
            while let Some(flag) = more.next() {
                if *flag == "--fork" {
                    arguments["fork"] = json!(true);
                    continue;
                }
                let key = match *flag {
                    "--role" => "role",
                    "--folder" => "folder",
                    "--worktree" => "worktree",
                    "--prompt" => "prompt",
                    "--model" => "model",
                    "--effort" => "effort",
                    _ => return Err(SPAWN),
                };
                let value = *more.next().ok_or(SPAWN)?;
                arguments[key] = match key {
                    // From the folder the command runs in, as `open`'s path.
                    "folder" => json!(absolute(value, std::env::current_dir().ok())),
                    _ => json!(value),
                };
            }
            ("spawn_agent", arguments)
        }
        ("kanban", ["list", more @ ..]) => {
            let mut arguments = json!({});
            let mut more = more.iter();
            while let Some(arg) = more.next() {
                match *arg {
                    "--card" => arguments["card"] = json!(more.next().ok_or(KANBAN)?),
                    board if !board.starts_with("--") && arguments.get("board").is_none() => {
                        arguments["board"] = json!(board)
                    }
                    _ => return Err(KANBAN),
                }
            }
            ("list_cards", arguments)
        }
        ("kanban", ["add", more @ ..]) => {
            let (mut title, mut arguments) = (Vec::new(), json!({}));
            let mut more = more.iter();
            while let Some(word) = more.next() {
                let key = match *word {
                    "--column" => "column",
                    "--description" => "description",
                    "--board" => "board",
                    "--priority" => "priority",
                    "--for" => "for",
                    "--checklist" => "checklist",
                    word => {
                        title.push(word);
                        continue;
                    }
                };
                arguments[key] = json!(more.next().ok_or(KANBAN)?);
            }
            if title.is_empty() {
                return Err(KANBAN);
            }
            arguments["title"] = json!(title.join(" "));
            ("create_card", arguments)
        }
        ("kanban", ["edit", card, more @ ..]) if !more.is_empty() => {
            let mut arguments = json!({ "card": card });
            let mut more = more.iter();
            while let Some(option) = more.next() {
                let value = json!(more.next().ok_or(KANBAN)?);
                match *option {
                    "--priority" => arguments["priority"] = value,
                    "--for" => arguments["for"] = value,
                    // Each --add is an item more.
                    "--add" => {
                        arguments["add_items"] = match arguments["add_items"].as_str() {
                            Some(before) => {
                                json!(format!("{before}\n{}", value.as_str().unwrap_or("")))
                            }
                            None => value,
                        }
                    }
                    "--check" | "--uncheck" => {
                        let key = &option[2..];
                        if arguments.get(key).is_none() {
                            arguments[key] = json!([]);
                        }
                        arguments[key].as_array_mut().expect("a list").push(value);
                    }
                    _ => return Err(KANBAN),
                }
            }
            ("edit_card", arguments)
        }
        ("kanban", ["move", card, column @ ..]) if !column.is_empty() => (
            "move_card",
            json!({ "card": card, "column": column.join(" ") }),
        ),
        ("kanban", ["comment", card, text @ ..]) if !text.is_empty() => (
            "comment_card",
            json!({ "card": card, "text": text.join(" ") }),
        ),
        ("kanban", ["edit-comment", card, comment, text @ ..]) if !text.is_empty() => (
            "edit_comment",
            json!({ "card": card, "comment": comment, "text": text.join(" ") }),
        ),
        ("kanban", ["delete-comment", card, comment]) => (
            "delete_comment",
            json!({ "card": card, "comment": comment }),
        ),
        ("kanban", ["claim", card]) => ("claim_card", json!({ "card": card })),
        ("note", ["read", block]) => ("read_note", json!({ "block": block })),
        ("note", ["append", block, text @ ..]) if !text.is_empty() => (
            "append_note",
            json!({ "block": block, "text": text.join(" ") }),
        ),
        ("note", ["replace", block, rest @ ..]) => {
            let (text, old) = match rest {
                [text @ .., "--old", old] => (text, Some(old)),
                text => (text, None),
            };
            if text.is_empty() {
                return Err(NOTE);
            }
            let mut arguments = json!({ "block": block, "text": text.join(" ") });
            if let Some(old) = old {
                arguments["old"] = json!(old);
            }
            ("replace_note", arguments)
        }
        ("checklist", ["read", block]) => ("read_checklist", json!({ "block": block })),
        ("checklist", ["edit", block, item, text @ ..]) if !text.is_empty() => (
            "edit_item",
            json!({ "block": block, "item": item, "text": text.join(" ") }),
        ),
        ("checklist", ["delete", block, item]) => {
            ("delete_item", json!({ "block": block, "item": item }))
        }
        ("checklist", ["add", block, text @ ..]) if !text.is_empty() => (
            "add_items",
            json!({ "block": block, "text": text.join(" ") }),
        ),
        ("checklist", ["check", block, item]) => {
            ("check_item", json!({ "block": block, "item": item }))
        }
        ("checklist", ["uncheck", block, item]) => (
            "check_item",
            json!({ "block": block, "item": item, "done": false }),
        ),
        ("open", [target]) => {
            // `path:line`, or `path:line:column` as compilers and grep print it (the column is
            // dropped); a Windows drive's `C:` is part of the path.
            fn split(at: &str) -> Option<(&str, u64)> {
                let (path, n) = at.rsplit_once(':')?;
                (!path.is_empty()).then_some((path, n.parse().ok()?))
            }
            let cwd = std::env::current_dir().ok();
            let mut arguments = json!({});
            match split(target) {
                Some((at, column)) => {
                    let (path, line) = split(at).unwrap_or((at, column));
                    arguments["path"] = json!(absolute(path, cwd));
                    arguments["line"] = json!(line);
                }
                None => arguments["path"] = json!(absolute(target, cwd)),
            }
            ("open_file", arguments)
        }
        ("wire", [a, b]) => ("wire", json!({ "a": a, "b": b })),
        ("keys", [block, keys @ ..]) if !keys.is_empty() => {
            ("send_keys", json!({ "block": block, "keys": keys }))
        }
        ("interrupt", [block]) => ("interrupt", json!({ "block": block })),
        ("close", [block]) => ("close_block", json!({ "block": block })),
        ("stop", [block]) => ("stop_agent", json!({ "block": block })),
        ("schedule", [block, rest @ ..]) => {
            let mut arguments = json!({ "block": block });
            let mut words = Vec::new();
            let mut rest = rest.iter();
            while let Some(word) = rest.next() {
                match *word {
                    "--at" => arguments["at"] = json!(rest.next().ok_or(SCHEDULE)?),
                    "--after" | "--every" => {
                        let minutes: u64 =
                            rest.next().ok_or(SCHEDULE)?.parse().map_err(|_| SCHEDULE)?;
                        arguments[&word[2..]] = json!(minutes);
                    }
                    word => words.push(word),
                }
            }
            if words.is_empty() {
                return Err(SCHEDULE);
            }
            arguments["text"] = json!(words.join(" "));
            ("schedule", arguments)
        }
        ("unschedule", [id]) => ("cancel_schedule", json!({ "schedule": id })),
        ("worktree", [how @ ("remove" | "merge"), name, more @ ..]) => {
            let mut arguments = json!({ "worktree": name });
            let mut more = more.iter();
            while let Some(flag) = more.next() {
                match *flag {
                    // From the folder the command runs in, as `spawn --folder`.
                    "--folder" => {
                        let folder = more.next().ok_or(WORKTREE)?;
                        arguments["folder"] = json!(absolute(folder, std::env::current_dir().ok()));
                    }
                    "--discard" if *how == "remove" => arguments["discard"] = json!(true),
                    _ => return Err(WORKTREE),
                }
            }
            (
                if *how == "remove" {
                    "remove_worktree"
                } else {
                    "merge_worktree"
                },
                arguments,
            )
        }
        // The file is read by `run`, which has the standard input `-` names.
        ("plan", [file]) => ("propose_plan", json!({ "plan": file })),
        ("plan", [file, "--title", title]) => {
            ("propose_plan", json!({ "plan": file, "title": title }))
        }
        ("new", ["note", text @ ..]) => {
            let mut arguments = json!({ "kind": "note" });
            if !text.is_empty() {
                arguments["text"] = json!(text.join(" "));
            }
            ("create_block", arguments)
        }
        ("new", ["checklist", name, more @ ..]) => {
            let arguments = match more {
                [] => json!({ "kind": "checklist", "name": name }),
                ["--items", items] => json!({ "kind": "checklist", "name": name, "items": items }),
                _ => return Err(NEW),
            };
            ("create_block", arguments)
        }
        ("new", ["terminal", more @ ..]) => {
            let mut arguments = json!({ "kind": "terminal" });
            let mut more = more.iter();
            while let Some(flag) = more.next() {
                let value = *more.next().ok_or(NEW)?;
                match *flag {
                    // From the folder the command runs in, as `spawn --folder`.
                    "--folder" => {
                        arguments["folder"] = json!(absolute(value, std::env::current_dir().ok()))
                    }
                    "--command" => arguments["command"] = json!(value),
                    _ => return Err(NEW),
                }
            }
            ("create_block", arguments)
        }
        ("whoami", _) => return Err("whoami [--json]"),
        ("blocks", _) => return Err("blocks [--json]"),
        ("read", _) => return Err(READ),
        ("send", _) => return Err("send <block> <text> [--enter] [--json]"),
        ("connect", _) => return Err("connect <block> [--json]"),
        ("spawn", _) => return Err(SPAWN),
        ("delegate", _) => return Err("delegate <block> <task> [--wait] [--json]"),
        ("reply", _) => return Err("reply [<block>] <text>|- [--json]"),
        ("wait", _) => return Err(WAIT),
        ("kanban", _) => return Err(KANBAN),
        ("note", _) => return Err(NOTE),
        ("checklist", _) => return Err(CHECKLIST),
        ("open", _) => return Err("open <path>[:<line>] [--json]"),
        ("wire", _) => return Err(WIRE),
        ("keys", _) => return Err(KEYS),
        ("interrupt", _) => return Err("interrupt <block> [--json]"),
        ("close", _) => return Err("close <block> [--json]"),
        ("stop", _) => return Err("stop <block> [--json]"),
        ("schedule", _) => return Err(SCHEDULE),
        ("unschedule", _) => return Err("unschedule <id> [--json]"),
        ("worktree", _) => return Err(WORKTREE),
        ("plan", _) => return Err(PLAN),
        ("new", _) => return Err(NEW),
        _ => return Err("notify <text> [--json]"),
    };
    Ok(CallArgs {
        name,
        arguments,
        json,
    })
}

/// Connects to the bus of the terminal this runs in and says hello.
fn open(env: &Env, client: &str) -> Result<Stream, &'static str> {
    // An empty bus is a Templum's that has none.
    let bus = env.bus.as_ref().filter(|bus| !bus.is_empty());
    let (Some(address), Some(terminal)) = (bus, &env.terminal) else {
        return Err("not inside a Templum terminal");
    };
    let gone = "Templum isn't running";
    let (mut stream, token) = bus::connect(address).map_err(|_| gone)?;
    stream
        .write_all(bus::hello_line(terminal, client, token.as_deref(), None).as_bytes())
        .map_err(|_| gone)?;
    Ok(stream)
}

/// `templum control`, the Templum Agent's own MCP server (docs/specs/templum-agent.md): a bridge
/// as `templum mcp` is, its hello carrying the secret only the engine's process has.
fn control(
    args: &[String],
    env: &Env,
    stdin: Box<dyn Read + Send>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    if !args.is_empty() {
        let _ = writeln!(stderr, "usage: templum control");
        return 2;
    }
    let bus = env.bus.as_ref().filter(|bus| !bus.is_empty());
    let (Some(address), Some(secret)) = (bus, &env.control) else {
        let _ = writeln!(
            stderr,
            "templum: the control server is only for the Templum Agent"
        );
        return 2;
    };
    let hello =
        |token: Option<String>| bus::hello_line("", "control", token.as_deref(), Some(secret));
    let stream = bus::connect(address).and_then(|(mut stream, token)| {
        stream.write_all(hello(token).as_bytes())?;
        Ok(stream)
    });
    match stream {
        Ok(stream) => bridge(stdin, stdout, stream),
        Err(_) => {
            let _ = writeln!(stderr, "templum: Templum isn't running");
            2
        }
    }
}

/// Copies the MCP client's lines (stdin) to the bus and the bus's back (stdout) until the bus
/// closes: once the client is done writing and answered, or when Templum is gone.
fn bridge(mut stdin: Box<dyn Read + Send>, stdout: &mut dyn Write, stream: Stream) -> i32 {
    let Ok(mut to_bus) = stream.try_clone() else {
        return 2;
    };
    std::thread::spawn(move || {
        let _ = io::copy(&mut stdin, &mut to_bus);
        let _ = to_bus.finish();
    });
    let mut from_bus = stream;
    let _ = io::copy(&mut from_bus, stdout);
    let _ = stdout.flush();
    0
}

/// Sends one tool call and prints its answer: its text, or with `--json` the whole result. It
/// stays connected until the answer: its end tells Templum it is gone (Ctrl+C), and what it
/// waited for is withdrawn.
fn one_call(
    mut stream: Stream,
    call: &CallArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let request = rpc::request(
        1,
        "tools/call",
        json!({ "name": call.name, "arguments": call.arguments }),
    );
    if stream.write_all(request.as_bytes()).is_err() {
        let _ = writeln!(stderr, "templum: Templum isn't running");
        return 2;
    }
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        let Ok(answer) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if answer["id"] != 1 {
            continue;
        }
        if let Some(message) = answer["error"]["message"].as_str() {
            let _ = writeln!(stderr, "templum: {message}");
            return 1;
        }
        let result = &answer["result"];
        let failed = result["isError"] == true;
        let text = if call.json {
            result.to_string()
        } else {
            result["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        };
        let out: &mut dyn Write = if failed { stderr } else { stdout };
        let _ = write!(out, "{text}");
        if !text.ends_with('\n') {
            let _ = writeln!(out);
        }
        return i32::from(failed);
    }
    let _ = writeln!(stderr, "templum: Templum closed the connection");
    2
}

/// An agent's hook: tells the bus its event and, as a session starts, asks for the person's
/// instructions on the same connection and prints them in the agent's shape. Silent when
/// Templum isn't there, doesn't answer within `ANSWER_WITHIN`, or has none.
fn hook(args: &[String], env: &Env, stdin: &mut dyn Read, stdout: &mut dyn Write) {
    let (Some(agent), Some(input)) = (args.first(), hook_input(args, stdin)) else {
        return;
    };
    let event = hook_event(args, &input);
    let starts = hooks::session_start(agent, args.get(1).map(String::as_str), &input);
    if event.is_none() && !starts {
        return;
    }
    let Ok(mut stream) = open(env, "hook") else {
        return;
    };
    if let Some(event) = event {
        let Some(line) = serde_json::to_value(event)
            .ok()
            .map(|event| rpc::notification(hooks::METHOD, event))
        else {
            return;
        };
        if stream.write_all(line.as_bytes()).is_err() {
            return;
        }
    }
    if !starts {
        let _ = stream.finish();
        return;
    }
    let text = instructions(stream, agent, ANSWER_WITHIN).unwrap_or_default();
    if let Some(output) = hooks::start_output(agent, &text) {
        let _ = writeln!(stdout, "{output}");
    }
}

/// What an agent's hook hands over, on stdin for all of them. None for an agent Templum doesn't
/// hook, and for input that isn't JSON.
fn hook_input(args: &[String], stdin: &mut dyn Read) -> Option<Value> {
    match args.first()?.as_str() {
        // An older Templum's `notify` (its JSON as an argument): its stdin is Codex's terminal,
        // which reading would steal the person's keys from.
        "codex" if args.len() > 1 => return None,
        "claude" | "codex" | "gemini" | "copilot" | "opencode" | "qwen" | "pi" => {}
        _ => return None,
    }
    let mut input = String::new();
    stdin.read_to_string(&mut input).ok()?;
    serde_json::from_str(&input).ok()
}

/// The event an agent's hook input `input` says. None for what Templum doesn't follow.
fn hook_event(args: &[String], input: &Value) -> Option<hooks::AgentEvent> {
    match args.first()?.as_str() {
        "claude" => hooks::claude(input),
        "gemini" => hooks::gemini(input),
        "codex" => hooks::codex(input),
        "copilot" => match hooks::copilot(args.get(1)?, input)? {
            hooks::AgentEvent::Stopped {
                message: None,
                transcript: Some(path),
            } => Some(hooks::AgentEvent::Stopped {
                message: copilot_answer(&path),
                transcript: Some(path),
            }),
            event => Some(event),
        },
        // Templum's plugin, in OpenCode and Kilo Code.
        "opencode" => hooks::opencode(input),
        "qwen" => hooks::qwen(input),
        // Templum's extension in Pi.
        "pi" => hooks::pi(input),
        _ => None,
    }
}

/// Asks the bus, on the connection of a hook that already said its event, for `agent`'s
/// instructions, and waits for the answer `within` at most. None for no answer, an error, or an
/// answer without a text; an empty text means none apply.
fn instructions(mut stream: Stream, agent: &str, within: Duration) -> Option<String> {
    let deadline = Instant::now() + within;
    let request = rpc::request(1, hooks::INSTRUCTIONS, json!({ "agent": agent }));
    stream.write_all(request.as_bytes()).ok()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let text = loop {
        // A read's limit is for each read: each gets what is left.
        let left = deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())?;
        reader.get_ref().set_read_timeout(Some(left)).ok()?;
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let Ok(answer) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if answer["id"] == 1 {
            break answer["result"]["text"].as_str()?.to_string();
        }
    };
    let _ = reader.get_ref().finish();
    Some(text)
}

/// The answer in Copilot CLI's transcript at `path` (its last 256 KB): it writes the file in
/// batches, so it may come a moment after its `agentStop` hook; waited for 3 s at most, under
/// the hook's 5.
fn copilot_answer(path: &str) -> Option<String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let answer = tail(path, 256 * 1024).and_then(|t| hooks::copilot_answer(&t));
        if answer.is_some() || std::time::Instant::now() >= deadline {
            return answer;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// The last `max` bytes of the file at `path`, from the first whole line in them.
fn tail(path: &str, max: u64) -> Option<String> {
    use std::io::{Seek as _, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(max))).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    Some(match len > max {
        true => text
            .split_once('\n')
            .map_or("", |(_, rest)| rest)
            .to_string(),
        false => text.into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{Bus, test_bus};
    use crate::rpc::Incoming;
    use crate::tools::{Reply, tool_result};

    fn env(bus: &Bus) -> Env {
        Env {
            bus: Some(bus.env.clone()),
            terminal: Some("123-4".into()),
            control: None,
        }
    }

    /// A wait longer than this fails the test instead of hanging it.
    const LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

    fn run_with(args: &[&str], env: &Env, stdin: impl AsRef<[u8]>) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (env, stdin) = (env.clone(), stdin.as_ref().to_vec());
        let (done, ran) = std::sync::mpsc::channel();
        // On a thread of its own: a command that never ends can't hang the test.
        std::thread::spawn(move || {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = run(
                &args,
                &env,
                Box::new(io::Cursor::new(stdin)),
                &mut out,
                &mut err,
            );
            let _ = done.send((
                code,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            ));
        });
        ran.recv_timeout(LIMIT).expect("the command ended")
    }

    #[test]
    fn the_bridge_says_hello_then_copies_both_ways() {
        let (_dir, bus) = test_bus(|m| {
            if let Incoming::Request { id, .. } = &m.incoming {
                let hello = json!({ "terminal": m.hello.terminal, "client": m.hello.client });
                let _ = m.reply.send(rpc::result(id, hello));
            }
        });
        let (code, out, _) = run_with(&["mcp"], &env(&bus), rpc::request(7, "ping", Value::Null));
        assert_eq!(code, 0);
        let answer: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(answer["id"], 7);
        assert_eq!(
            answer["result"],
            json!({ "terminal": "123-4", "client": "mcp" })
        );
    }

    #[test]
    fn a_verb_is_one_tool_call_and_prints_its_text() {
        let (seen, calls) = std::sync::mpsc::channel();
        let (_dir, bus) = test_bus(move |m| {
            if let Incoming::Request { id, params, .. } = &m.incoming {
                let _ = seen.send(params.clone());
                let reply = if params["arguments"]["block"] == "nope" {
                    Reply::err("nope isn't connected to you")
                } else {
                    Reply::ok("line 1\nline 2")
                };
                let _ = m.reply.send(tool_result(id, &reply));
            }
        });
        let (code, out, _) = run_with(&["read", "b7k2m", "-n", "10"], &env(&bus), "");
        assert_eq!((code, out.as_str()), (0, "line 1\nline 2\n"));
        assert_eq!(
            calls.recv_timeout(LIMIT).unwrap(),
            json!({ "name": "read_terminal", "arguments": { "block": "b7k2m", "lines": 10 } })
        );
        let (code, out, err) = run_with(&["read", "nope"], &env(&bus), "");
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (1, "", "nope isn't connected to you\n")
        );
        assert_eq!(
            calls.recv_timeout(LIMIT).unwrap()["arguments"],
            json!({ "block": "nope" })
        );
        run_with(
            &["send", "b7k2m", "make", "test", "--enter"],
            &env(&bus),
            "",
        );
        assert_eq!(
            calls.recv_timeout(LIMIT).unwrap()["arguments"],
            json!({ "block": "b7k2m", "text": "make test", "enter": true })
        );
        let (code, out, _) = run_with(&["blocks", "--json"], &env(&bus), "");
        assert_eq!(code, 0);
        assert_eq!(
            serde_json::from_str::<Value>(&out).unwrap()["content"][0]["text"],
            "line 1\nline 2"
        );
    }

    /// A command stays connected until its answer comes: Templum hears it end only then. One
    /// stopped with Ctrl+C ends it early, and what it waited for is withdrawn.
    #[test]
    fn a_command_stays_connected_until_its_answer() {
        let (seen, order) = std::sync::mpsc::channel();
        let (_dir, bus) = test_bus(move |m| match &m.incoming {
            Incoming::Request { id, .. } => {
                let (id, reply, seen) = (id.clone(), m.reply.clone(), seen.clone());
                let _ = seen.send("asked");
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    let _ = seen.send("answered");
                    let _ = reply.send(tool_result(&id, &Reply::ok("Yes")));
                });
            }
            Incoming::Notification { method, .. } if method == crate::bus::CLOSED => {
                let _ = seen.send("closed");
            }
            _ => {}
        });
        assert_eq!(run_with(&["whoami"], &env(&bus), "").1, "Yes\n");
        let order: Vec<&str> = (0..3).map(|_| order.recv_timeout(LIMIT).unwrap()).collect();
        assert_eq!(order, ["asked", "answered", "closed"]);
    }

    #[test]
    fn ask_sends_the_question_and_its_options() {
        let (seen, calls) = std::sync::mpsc::channel();
        let (_dir, bus) = test_bus(move |m| {
            if let Incoming::Request { id, params, .. } = &m.incoming {
                let _ = seen.send(params.clone());
                let _ = m.reply.send(tool_result(id, &Reply::ok("Yes")));
            }
        });
        let args = [
            "ask", "Ship", "it?", "--option", "Yes", "--option", "Not yet",
        ];
        let (code, out, _) = run_with(&args, &env(&bus), "");
        assert_eq!((code, out.as_str()), (0, "Yes\n"));
        assert_eq!(
            calls.recv_timeout(LIMIT).unwrap(),
            json!({ "name": "ask_user", "arguments": { "question": "Ship it?", "options": ["Yes", "Not yet"] } })
        );
        run_with(&["ask", "When?"], &env(&bus), "");
        assert_eq!(
            calls.recv_timeout(LIMIT).unwrap()["arguments"],
            json!({ "question": "When?" })
        );
        run_with(&["connect", "k9x1d"], &env(&bus), "");
        assert_eq!(
            calls.recv_timeout(LIMIT).unwrap(),
            json!({ "name": "connect", "arguments": { "block": "k9x1d" } })
        );
        let (code, _, err) = run_with(&["ask", "--option", "Yes"], &env(&bus), "");
        assert_eq!(code, 2);
        assert!(err.starts_with("usage: templum ask"), "{err}");
    }

    #[test]
    fn the_control_verbs_send_their_calls() {
        let (seen, calls) = std::sync::mpsc::channel();
        let (_dir, bus) = test_bus(move |m| {
            if let Incoming::Request { id, params, .. } = &m.incoming {
                let _ = seen.send(params.clone());
                let _ = m.reply.send(tool_result(id, &Reply::ok("ok")));
            }
        });
        for (args, sent) in [
            (
                &["keys", "k9x1d", "2", "enter"][..],
                json!({ "name": "send_keys", "arguments": { "block": "k9x1d", "keys": ["2", "enter"] } }),
            ),
            (
                &["interrupt", "k9x1d"][..],
                json!({ "name": "interrupt", "arguments": { "block": "k9x1d" } }),
            ),
            (
                &["close", "k9x1d"][..],
                json!({ "name": "close_block", "arguments": { "block": "k9x1d" } }),
            ),
            (
                &["stop", "k9x1d"][..],
                json!({ "name": "stop_agent", "arguments": { "block": "k9x1d" } }),
            ),
            (
                &[
                    "spawn",
                    "codex",
                    "--model",
                    "gpt-5.6-luna",
                    "--effort",
                    "high",
                ][..],
                json!({ "name": "spawn_agent", "arguments": { "agent": "codex", "model": "gpt-5.6-luna", "effort": "high" } }),
            ),
        ] {
            assert_eq!(run_with(args, &env(&bus), "").0, 0, "{args:?}");
            assert_eq!(calls.recv_timeout(LIMIT).unwrap(), sent, "{args:?}");
        }
        for (args, usage) in [
            (&["keys", "k9x1d"][..], "usage: templum keys"),
            (&["interrupt"][..], "usage: templum interrupt"),
            (&["close", "a", "b"][..], "usage: templum close"),
            (&["stop"][..], "usage: templum stop"),
            (&["spawn", "codex", "--effort"][..], "usage: templum spawn"),
        ] {
            let (code, _, err) = run_with(args, &env(&bus), "");
            assert_eq!(code, 2, "{args:?}");
            assert!(err.starts_with(usage), "{args:?}: {err}");
        }
    }

    /// `templum plan` sends a file's text, or its standard input's with `-`, as the plan.
    #[test]
    fn plan_sends_the_files_text() {
        let (seen, calls) = std::sync::mpsc::channel();
        let (_dir, bus) = test_bus(move |m| {
            if let Incoming::Request { id, params, .. } = &m.incoming {
                let _ = seen.send(params.clone());
                let _ = m.reply.send(tool_result(id, &Reply::err("Rejected.")));
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "## Steps\n1. a\n").unwrap();
        let path = file.to_string_lossy().into_owned();
        let (code, _, err) = run_with(&["plan", &path, "--title", "T"], &env(&bus), "");
        assert_eq!(code, 1, "a rejection exits 1: {err}");
        assert_eq!(
            calls.recv_timeout(LIMIT).unwrap(),
            json!({ "name": "propose_plan", "arguments": { "plan": "## Steps\n1. a\n", "title": "T" } })
        );
        run_with(&["plan", "-"], &env(&bus), "from stdin");
        assert_eq!(
            calls.recv_timeout(LIMIT).unwrap(),
            json!({ "name": "propose_plan", "arguments": { "plan": "from stdin" } })
        );
        let missing = dir.path().join("nope.md").to_string_lossy().into_owned();
        let (code, _, err) = run_with(&["plan", &missing], &env(&bus), "");
        assert_eq!(code, 2, "{err}");
        assert!(err.starts_with("templum: couldn't read "), "{err}");
    }

    /// `templum reply -` answers the task its shell is on with its standard input, whole; with a
    /// block it writes to that agent, from the arguments or from `-`.
    #[test]
    fn reply_reads_its_answer_from_stdin() {
        let (seen, calls) = std::sync::mpsc::channel();
        let (_dir, bus) = test_bus(move |m| {
            if let Incoming::Request { id, params, .. } = &m.incoming {
                let _ = seen.send(params.clone());
                let _ = m.reply.send(tool_result(id, &Reply::ok("ok")));
            }
        });
        for (args, stdin, arguments) in [
            (
                &["reply", "-"][..],
                "Line 42.\n",
                json!({ "text": "Line 42.\n" }),
            ),
            (
                &["reply", "k9x1d", "-"][..],
                "Line 42.",
                json!({ "block": "k9x1d", "text": "Line 42." }),
            ),
            (
                &["reply", "k9x1d", "Line", "42."][..],
                "ignored",
                json!({ "block": "k9x1d", "text": "Line 42." }),
            ),
        ] {
            let (code, _, err) = run_with(args, &env(&bus), stdin);
            assert_eq!(code, 0, "{args:?}: {err}");
            assert_eq!(
                calls.recv_timeout(LIMIT).unwrap(),
                json!({ "name": "reply", "arguments": arguments }),
                "{args:?}"
            );
        }
        let (code, _, err) = run_with(&["reply"], &env(&bus), "");
        assert_eq!(code, 2, "{err}");
        assert!(
            err.starts_with("usage: templum reply [<block>] <text>|- [--json]"),
            "{err}"
        );
    }

    /// An answer that isn't UTF-8 (a command's output in another encoding) is still sent, its
    /// bad bytes as U+FFFD.
    #[test]
    fn reply_takes_stdin_that_isnt_utf8() {
        let (seen, calls) = std::sync::mpsc::channel();
        let (_dir, bus) = test_bus(move |m| {
            if let Incoming::Request { id, params, .. } = &m.incoming {
                let _ = seen.send(params.clone());
                let _ = m.reply.send(tool_result(id, &Reply::ok("ok")));
            }
        });
        let (code, _, err) = run_with(&["reply", "-"], &env(&bus), b"caf\xe9 ok\n");
        assert_eq!(code, 0, "{err}");
        assert_eq!(
            calls.recv_timeout(LIMIT).unwrap(),
            json!({ "name": "reply", "arguments": { "text": "caf\u{fffd} ok\n" } })
        );
    }

    #[test]
    fn delegate_reply_and_wait_send_their_calls() {
        let (seen, calls) = std::sync::mpsc::channel();
        let (_dir, bus) = test_bus(move |m| {
            if let Incoming::Request { id, params, .. } = &m.incoming {
                let _ = seen.send(params.clone());
                let _ = m.reply.send(tool_result(id, &Reply::ok("ok")));
            }
        });
        for (args, sent) in [
            (
                &["delegate", "k9x1d", "fix", "the", "tests", "--wait"][..],
                json!({ "name": "delegate", "arguments": { "block": "k9x1d", "task": "fix the tests", "wait": true } }),
            ),
            (
                &["reply", "b7k2m", "all", "green"][..],
                json!({ "name": "reply", "arguments": { "block": "b7k2m", "text": "all green" } }),
            ),
            (
                &["wait", "k9x1d"][..],
                json!({ "name": "wait_for_idle", "arguments": { "block": "k9x1d" } }),
            ),
            (
                &["wait", "k9x1d", "--timeout", "60"][..],
                json!({ "name": "wait_for_idle", "arguments": { "block": "k9x1d", "timeout": 60 } }),
            ),
            (
                &["wait", "k9x1d", "--for", "needs_you"][..],
                json!({ "name": "wait_for", "arguments": { "block": "k9x1d", "event": "needs_you" } }),
            ),
            (
                // The flags in either order.
                &[
                    "wait",
                    "k9x1d",
                    "--timeout",
                    "60",
                    "--for",
                    "command_finished",
                ][..],
                json!({ "name": "wait_for", "arguments": { "block": "k9x1d", "timeout": 60, "event": "command_finished" } }),
            ),
            (
                &["note", "replace", "4qm2a", "- ship it", "--old", "- ship"][..],
                json!({ "name": "replace_note", "arguments": { "block": "4qm2a", "text": "- ship it", "old": "- ship" } }),
            ),
            (
                &["note", "replace", "4qm2a", "All", "new"][..],
                json!({ "name": "replace_note", "arguments": { "block": "4qm2a", "text": "All new" } }),
            ),
            (
                &["checklist", "edit", "4qm2a", "41", "Build", "it"][..],
                json!({ "name": "edit_item", "arguments": { "block": "4qm2a", "item": "41", "text": "Build it" } }),
            ),
            (
                &["checklist", "delete", "4qm2a", "41"][..],
                json!({ "name": "delete_item", "arguments": { "block": "4qm2a", "item": "41" } }),
            ),
            (
                &[
                    "schedule", "k9x1d", "check", "it", "--at", "15:00", "--every", "10",
                ][..],
                json!({ "name": "schedule", "arguments": { "block": "k9x1d", "text": "check it", "at": "15:00", "every": 10 } }),
            ),
            (
                &["worktree", "remove", "w", "--discard"][..],
                json!({ "name": "remove_worktree", "arguments": { "worktree": "w", "discard": true } }),
            ),
            (
                &["worktree", "merge", "w", "--folder", "api"][..],
                json!({ "name": "merge_worktree", "arguments": { "worktree": "w", "folder": std::env::current_dir().unwrap().join("api") } }),
            ),
            (
                &["schedule", "k9x1d", "go", "--after", "5"][..],
                json!({ "name": "schedule", "arguments": { "block": "k9x1d", "text": "go", "after": 5 } }),
            ),
            (
                &["unschedule", "s3"][..],
                json!({ "name": "cancel_schedule", "arguments": { "schedule": "s3" } }),
            ),
            (
                &["spawn", "--role", "reviewer", "--prompt", "review it"][..],
                json!({ "name": "spawn_agent", "arguments": { "role": "reviewer", "prompt": "review it" } }),
            ),
            (
                &["spawn", "--fork", "--prompt", "review it"][..],
                json!({ "name": "spawn_agent", "arguments": { "fork": true, "prompt": "review it" } }),
            ),
            (
                &["spawn", "claude", "--fork", "--prompt", "x"][..],
                json!({ "name": "spawn_agent", "arguments": { "agent": "claude", "fork": true, "prompt": "x" } }),
            ),
            (
                &["new", "note", "first", "notes"][..],
                json!({ "name": "create_block", "arguments": { "kind": "note", "text": "first notes" } }),
            ),
            (
                &["new", "checklist", "Plan", "--items", "- [ ] Read"][..],
                json!({ "name": "create_block", "arguments": { "kind": "checklist", "name": "Plan", "items": "- [ ] Read" } }),
            ),
            (
                &[
                    "new",
                    "terminal",
                    "--folder",
                    "web",
                    "--command",
                    "npm run dev",
                ][..],
                json!({ "name": "create_block", "arguments": { "kind": "terminal", "folder": std::env::current_dir().unwrap().join("web"), "command": "npm run dev" } }),
            ),
            (
                &["wait", "t4q8p", "--match", "ERROR", "--lines", "5"][..],
                json!({ "name": "wait_for", "arguments": { "block": "t4q8p", "pattern": "ERROR", "lines": 5 } }),
            ),
            (
                &[
                    "spawn",
                    "codex",
                    "--worktree",
                    "fix-tests",
                    "--prompt",
                    "fix the tests",
                ][..],
                json!({ "name": "spawn_agent", "arguments": { "agent": "codex", "worktree": "fix-tests", "prompt": "fix the tests" } }),
            ),
            (
                // A relative folder goes from the folder the command runs in, as `open`'s path.
                &["spawn", "codex", "--folder", "web"][..],
                json!({ "name": "spawn_agent", "arguments": { "agent": "codex", "folder": std::env::current_dir().unwrap().join("web") } }),
            ),
        ] {
            assert_eq!(run_with(args, &env(&bus), "").0, 0, "{args:?}");
            assert_eq!(calls.recv_timeout(LIMIT).unwrap(), sent);
        }
        assert_eq!(
            run_with(&["delegate", "k9x1d", "--wait"], &env(&bus), "").0,
            2,
            "no task"
        );
        assert_eq!(
            run_with(&["spawn", "codex", "--folder"], &env(&bus), "").0,
            2,
            "no folder"
        );
        assert_eq!(
            run_with(&["wait", "k9x1d", "--for"], &env(&bus), "").0,
            2,
            "no event"
        );
        assert_eq!(
            run_with(&["new", "kanban"], &env(&bus), "").0,
            2,
            "no such kind"
        );
        assert_eq!(
            run_with(&["wait", "k9x1d", "--timeout", "soon"], &env(&bus), "").0,
            2,
            "no number"
        );
    }

    #[test]
    fn a_hook_sends_its_event_prints_nothing_and_never_fails() {
        let (seen, events) = std::sync::mpsc::channel();
        let (_dir, bus) = test_bus(move |m| {
            // Each hook's connection ends after its event: that isn't an event.
            if !matches!(&m.incoming, Incoming::Notification { method, .. } if method == crate::bus::CLOSED)
            {
                let _ = seen.send((m.hello.client, m.incoming));
            }
        });
        let stop = r#"{"hook_event_name":"Stop","last_assistant_message":"Done.","transcript_path":"/t.jsonl"}"#;
        assert_eq!(
            run_with(&["hook", "claude"], &env(&bus), stop),
            (0, String::new(), String::new())
        );
        assert_eq!(
            events.recv_timeout(LIMIT).unwrap(),
            (
                "hook".to_string(),
                Incoming::Notification {
                    method: "templum/event".into(),
                    params: json!({ "event": "stopped", "message": "Done.", "transcript": "/t.jsonl" }),
                }
            )
        );
        // A permission dialog: told to Templum, and nothing printed, so the dialog still decides.
        let asked = r#"{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{}}"#;
        assert_eq!(
            run_with(&["hook", "claude"], &env(&bus), asked),
            (0, String::new(), String::new())
        );
        assert_eq!(
            events.recv_timeout(LIMIT).unwrap().1,
            Incoming::Notification {
                method: "templum/event".into(),
                params: json!({ "event": "needs_you", "message": "Allow Bash?" }),
            }
        );
        // Codex's, on stdin. An argument is an older Templum's `notify`: its stdin is Codex's
        // terminal, never read (the next event heard is the stdin one's, not two of them).
        let codex = r#"{"hook_event_name":"Stop","last_assistant_message":"Fixed."}"#;
        let notify = r#"{"type":"agent-turn-complete","last-assistant-message":"Old."}"#;
        assert_eq!(
            run_with(&["hook", "codex", notify], &env(&bus), codex),
            (0, String::new(), String::new())
        );
        assert_eq!(
            run_with(&["hook", "codex"], &env(&bus), codex),
            (0, String::new(), String::new())
        );
        assert_eq!(
            events.recv_timeout(LIMIT).unwrap().1,
            Incoming::Notification {
                method: "templum/event".into(),
                params: json!({ "event": "stopped", "message": "Fixed." }),
            }
        );
        // Gemini CLI's, on stdin as Claude Code's: its stdout must stay empty too.
        let gemini = r#"{"hook_event_name":"AfterAgent","prompt_response":"Done."}"#;
        assert_eq!(
            run_with(&["hook", "gemini"], &env(&bus), gemini),
            (0, String::new(), String::new())
        );
        assert_eq!(
            events.recv_timeout(LIMIT).unwrap().1,
            Incoming::Notification {
                method: "templum/event".into(),
                params: json!({ "event": "stopped", "message": "Done." }),
            }
        );
        // Copilot CLI's names its event in its words, and its answer is in its transcript.
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("events.jsonl");
        std::fs::write(
            &transcript,
            concat!(
                r#"{"type":"user.message","data":{"content":"hi"}}"#,
                "\n",
                r#"{"type":"assistant.message","data":{"content":"Done."}}"#,
                "\n",
            ),
        )
        .unwrap();
        let ended = json!({ "transcriptPath": transcript }).to_string();
        assert_eq!(
            run_with(&["hook", "copilot", "agentStop"], &env(&bus), &ended),
            (0, String::new(), String::new())
        );
        assert_eq!(
            events.recv_timeout(LIMIT).unwrap().1,
            Incoming::Notification {
                method: "templum/event".into(),
                params: json!({ "event": "stopped", "message": "Done.", "transcript": transcript }),
            }
        );
        // Copilot writes its transcript in batches: the answer may come a moment after the hook.
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&transcript)
            .unwrap();
        writeln!(
            file,
            r#"{{"type":"user.message","data":{{"content":"again"}}}}"#
        )
        .unwrap();
        let late = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            writeln!(
                file,
                r#"{{"type":"assistant.message","data":{{"content":"Later."}}}}"#
            )
            .unwrap();
        });
        assert_eq!(
            run_with(&["hook", "copilot", "agentStop"], &env(&bus), &ended),
            (0, String::new(), String::new())
        );
        late.join().unwrap();
        assert_eq!(
            events.recv_timeout(LIMIT).unwrap().1,
            Incoming::Notification {
                method: "templum/event".into(),
                params: json!({ "event": "stopped", "message": "Later.", "transcript": transcript }),
            }
        );
        // Without its event: nothing.
        assert_eq!(
            run_with(&["hook", "copilot"], &env(&bus), &ended),
            (0, String::new(), String::new())
        );
        // Templum's plugin in OpenCode and Kilo Code.
        let idle = r#"{"type":"session.idle","properties":{"sessionID":"s"},"answer":"Ok."}"#;
        assert_eq!(
            run_with(&["hook", "opencode"], &env(&bus), idle),
            (0, String::new(), String::new())
        );
        assert_eq!(
            events.recv_timeout(LIMIT).unwrap().1,
            Incoming::Notification {
                method: "templum/event".into(),
                params: json!({ "event": "stopped", "message": "Ok." }),
            }
        );
        // Outside Templum, and with what Templum doesn't follow: nothing, quietly.
        assert_eq!(
            run_with(&["hook", "claude"], &Env::default(), stop),
            (0, String::new(), String::new())
        );
        assert_eq!(
            run_with(&["hook", "claude"], &env(&bus), "not json"),
            (0, String::new(), String::new())
        );
    }

    /// A bus that answers `templum/instructions` with `text` and sends every other message
    /// (but a connection's end) to the channel, with its client's name.
    fn instructing_bus(
        text: &'static str,
    ) -> (
        tempfile::TempDir,
        Bus,
        std::sync::mpsc::Receiver<(String, Incoming)>,
    ) {
        let (seen, heard) = std::sync::mpsc::channel();
        let (dir, bus) = test_bus(move |m| {
            if matches!(&m.incoming, Incoming::Notification { method, .. } if method == crate::bus::CLOSED)
            {
                return;
            }
            if let Incoming::Request { id, .. } = &m.incoming {
                let _ = m.reply.send(rpc::result(id, json!({ "text": text })));
            }
            let _ = seen.send((m.hello.client, m.incoming));
        });
        (dir, bus, heard)
    }

    fn started(source: &str) -> String {
        json!({ "hook_event_name": "SessionStart", "source": source }).to_string()
    }

    fn context_line(text: &str) -> String {
        format!(
            "{}\n",
            json!({ "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": text } })
        )
    }

    #[test]
    fn a_session_start_prints_templums_instructions() {
        let (_dir, bus, heard) = instructing_bus("Be brief.");
        assert_eq!(
            run_with(&["hook", "claude"], &env(&bus), started("startup")),
            (0, context_line("Be brief."), String::new())
        );
        assert_eq!(
            heard.recv_timeout(LIMIT).unwrap(),
            (
                "hook".to_string(),
                Incoming::Notification {
                    method: "templum/event".into(),
                    params: json!({ "event": "started" }),
                }
            )
        );
        assert_eq!(
            heard.recv_timeout(LIMIT).unwrap(),
            (
                "hook".to_string(),
                Incoming::Request {
                    id: json!(1),
                    method: "templum/instructions".into(),
                    params: json!({ "agent": "claude" }),
                }
            )
        );
    }

    #[test]
    fn copilots_start_prints_its_own_shape() {
        let (_dir, bus, heard) = instructing_bus("Be brief.");
        assert_eq!(
            run_with(&["hook", "copilot", "sessionStart"], &env(&bus), "{}"),
            (
                0,
                format!("{}\n", json!({ "additionalContext": "Be brief." })),
                String::new()
            )
        );
        let asked = heard.iter().take(2).last().unwrap().1;
        assert!(matches!(asked, Incoming::Request { params, .. } if params["agent"] == "copilot"));
    }

    #[test]
    fn a_compaction_gets_the_instructions_again_and_no_event() {
        let (_dir, bus, heard) = instructing_bus("Be brief.");
        assert_eq!(
            run_with(&["hook", "claude"], &env(&bus), started("compact")),
            (0, context_line("Be brief."), String::new())
        );
        // The compaction leaves its turn working: the request is the only thing Templum hears.
        assert!(matches!(
            heard.recv_timeout(LIMIT).unwrap().1,
            Incoming::Request { method, .. } if method == "templum/instructions"
        ));
        assert!(heard.try_recv().is_err());
    }

    #[test]
    fn no_instructions_print_nothing() {
        let (_dir, bus, _heard) = instructing_bus("");
        assert_eq!(
            run_with(&["hook", "claude"], &env(&bus), started("startup")),
            (0, String::new(), String::new())
        );
    }

    #[test]
    fn a_templum_that_doesnt_answer_costs_at_most_the_limit() {
        // A bus that hears the request and never answers.
        let (_dir, bus) = test_bus(|_| {});
        let hook = std::time::Instant::now();
        let stream = open(&env(&bus), "hook").unwrap();
        assert_eq!(
            instructions(stream, "claude", std::time::Duration::from_millis(200)),
            None
        );
        assert!(hook.elapsed() < std::time::Duration::from_secs(1));
        // Outside Templum: nothing, at once.
        assert_eq!(
            run_with(&["hook", "claude"], &Env::default(), started("startup")),
            (0, String::new(), String::new())
        );
    }

    #[test]
    fn only_a_session_start_asks_and_prints() {
        let (_dir, bus, heard) = instructing_bus("Be brief.");
        for hook in [
            r#"{"hook_event_name":"Stop","last_assistant_message":"Done."}"#,
            r#"{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{}}"#,
            r#"{"hook_event_name":"UserPromptSubmit","prompt":"hi"}"#,
        ] {
            assert_eq!(
                run_with(&["hook", "claude"], &env(&bus), hook),
                (0, String::new(), String::new())
            );
        }
        assert_eq!(
            run_with(&["hook", "copilot", "agentStop"], &env(&bus), "{}"),
            (0, String::new(), String::new())
        );
        while let Ok((_, heard)) = heard.recv_timeout(std::time::Duration::from_millis(300)) {
            assert!(
                !matches!(heard, Incoming::Request { .. }),
                "{heard:?} asked for instructions"
            );
        }
    }

    #[test]
    fn a_files_tail_starts_at_a_whole_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t");
        std::fs::write(&path, "first line\nsecond\nthird\n").unwrap();
        let path = path.to_str().unwrap();
        assert_eq!(tail(path, 10).as_deref(), Some("third\n"));
        assert_eq!(
            tail(path, 100).as_deref(),
            Some("first line\nsecond\nthird\n")
        );
        assert_eq!(tail("/nonexistent/t", 10), None);
    }

    #[test]
    fn templum_control_without_its_variable_says_so_and_exits_2() {
        let (_dir, bus) = test_bus(|_| {});
        for env in [
            Env {
                control: None,
                ..env(&bus)
            },
            Env {
                bus: None,
                control: Some("s3cret".into()),
                terminal: None,
            },
            Env {
                bus: Some(String::new()),
                control: Some("s3cret".into()),
                terminal: None,
            },
        ] {
            let (code, out, err) = run_with(&["control"], &env, "");
            assert_eq!((code, out.as_str()), (2, ""));
            assert_eq!(
                err,
                "templum: the control server is only for the Templum Agent\n"
            );
        }
    }

    #[test]
    fn templum_control_says_hello_with_its_secret_then_bridges() {
        let (_dir, bus) = test_bus(|m| {
            if let Incoming::Request { id, .. } = &m.incoming {
                let hello = json!({
                    "terminal": m.hello.terminal, "client": m.hello.client, "control": m.hello.control,
                });
                let _ = m.reply.send(rpc::result(id, hello));
            }
        });
        let env = Env {
            control: Some("s3cret".into()),
            terminal: None,
            ..env(&bus)
        };
        let (code, out, _) = run_with(&["control"], &env, rpc::request(7, "ping", Value::Null));
        assert_eq!(code, 0);
        let answer: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(answer["id"], 7);
        assert_eq!(
            answer["result"],
            json!({ "terminal": "", "client": "control", "control": "s3cret" })
        );
    }

    #[test]
    fn outside_templum_or_with_it_gone_a_command_exits_2() {
        let (code, out, err) = run_with(&["blocks"], &Env::default(), "");
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(err.contains("not inside a Templum terminal"), "{err}");
        // A Templum with no bus gives its terminals an empty one.
        let no_bus = Env {
            bus: Some(String::new()),
            terminal: Some("1-1".into()),
            control: None,
        };
        let (code, _, err) = run_with(&["blocks"], &no_bus, "");
        assert_eq!(code, 2);
        assert!(err.contains("not inside a Templum terminal"), "{err}");
        let gone = Env {
            bus: Some(if cfg!(unix) {
                "unix:/nonexistent/templum.sock".into()
            } else {
                "tcp:127.0.0.1:9:x".into()
            }),
            terminal: Some("1-1".into()),
            control: None,
        };
        let (code, out, err) = run_with(&["mcp"], &gone, "");
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(err.contains("Templum isn't running"), "{err}");
        let (code, _, err) = run_with(&["read"], &gone, "");
        assert_eq!(code, 2);
        assert!(err.starts_with("usage: templum read"), "{err}");
    }

    #[test]
    fn only_the_verbs_are_commands() {
        assert!(is_verb("mcp") && is_verb("hook") && is_verb("read"));
        assert!(is_verb("control"));
        assert!(is_verb("ask"));
        assert!(is_verb("wire"));
        assert!(is_verb("keys") && is_verb("interrupt") && is_verb("close"));
        assert!(is_verb("kanban") && is_verb("note") && is_verb("open"));
        assert!(is_verb("checklist"));
        assert!(!is_verb("git"));
        assert!(!is_verb("--bench") && !is_verb("--claude-statusline") && !is_verb(""));
    }

    #[test]
    fn wire_sends_its_call_and_json_prints_the_whole_result() {
        let (seen, calls) = std::sync::mpsc::channel();
        let (_dir, bus) = test_bus(move |m| {
            if let Incoming::Request { id, params, .. } = &m.incoming {
                let _ = seen.send(params.clone());
                let _ = m.reply.send(tool_result(id, &Reply::ok("Wired.")));
            }
        });
        let (code, out, _) = run_with(&["wire", "b7k2m", "k9x1d", "--json"], &env(&bus), "");
        assert_eq!(code, 0);
        assert_eq!(
            serde_json::from_str::<Value>(&out).unwrap(),
            json!({ "content": [{ "type": "text", "text": "Wired." }] })
        );
        assert_eq!(
            calls.recv_timeout(LIMIT).unwrap(),
            json!({ "name": "wire", "arguments": { "a": "b7k2m", "b": "k9x1d" } })
        );
        let (code, _, err) = run_with(&["wire", "b7k2m"], &env(&bus), "");
        assert_eq!(code, 2);
        assert!(err.starts_with("usage: templum wire"), "{err}");
    }

    /// Every verb that makes a tool call takes --json, and its usage says so.
    #[test]
    fn every_verbs_usage_offers_json() {
        let (none, one): ([String; 0], [String; 1]) = ([], ["x".into()]);
        for verb in VERBS
            .iter()
            .copied()
            .filter(|v| !matches!(*v, "mcp" | "control" | "hook"))
        {
            let usages: Vec<&str> = [&none[..], &one[..]]
                .into_iter()
                .filter_map(|args| call_of(verb, args).err())
                .collect();
            assert!(!usages.is_empty(), "{verb}");
            for usage in usages {
                assert!(usage.starts_with(verb), "{verb}: {usage}");
                assert!(usage.ends_with("[--json]"), "{usage}");
            }
        }
    }

    #[test]
    fn open_reads_a_relative_path_from_the_folder_it_runs_in() {
        let cwd = std::env::temp_dir();
        let whole = cwd.join("a.rs").to_string_lossy().into_owned();
        assert_eq!(
            absolute("src/a.rs", Some(cwd.clone())),
            cwd.join("src/a.rs").to_string_lossy()
        );
        assert_eq!(absolute(&whole, Some("elsewhere".into())), whole);
        assert_eq!(
            absolute("~/a.rs", Some(cwd)),
            "~/a.rs",
            "Templum reads ~ as home"
        );
        assert_eq!(absolute("a.rs", None), "a.rs");
    }

    #[test]
    fn the_content_verbs_send_their_calls() {
        let (seen, calls) = std::sync::mpsc::channel();
        let (_dir, bus) = test_bus(move |m| {
            if let Incoming::Request { id, params, .. } = &m.incoming {
                let _ = seen.send(params.clone());
                let _ = m.reply.send(tool_result(id, &Reply::ok("ok")));
            }
        });
        // A relative path goes from the folder the command runs in (a Windows drive's path is
        // whole there, and Path::join keeps it as it is).
        let here = std::env::current_dir().unwrap();
        let at = |path: &str| here.join(path).to_string_lossy().into_owned();
        for (args, sent) in [
            (
                &["kanban", "list"][..],
                json!({ "name": "list_cards", "arguments": {} }),
            ),
            (
                &["kanban", "list", "Sprint", "--card", "12"][..],
                json!({ "name": "list_cards", "arguments": { "board": "Sprint", "card": "12" } }),
            ),
            (
                &[
                    "kanban",
                    "add",
                    "Fix",
                    "login",
                    "--column",
                    "Doing",
                    "--description",
                    "It 500s.",
                ][..],
                json!({ "name": "create_card", "arguments": { "title": "Fix login", "column": "Doing", "description": "It 500s." } }),
            ),
            (
                &[
                    "kanban",
                    "add",
                    "Fix",
                    "--priority",
                    "high",
                    "--for",
                    "codex",
                    "--checklist",
                    "a",
                ][..],
                json!({ "name": "create_card", "arguments": { "title": "Fix", "priority": "high", "for": "codex", "checklist": "a" } }),
            ),
            (
                &[
                    "kanban",
                    "edit",
                    "12",
                    "--check",
                    "3",
                    "--check",
                    "4",
                    "--priority",
                    "low",
                    "--add",
                    "Docs",
                    "--add",
                    "Notes",
                    "--uncheck",
                    "5",
                    "--for",
                    "anyone",
                ][..],
                json!({ "name": "edit_card", "arguments": { "card": "12", "check": ["3", "4"], "priority": "low",
                        "add_items": "Docs\nNotes", "uncheck": ["5"], "for": "anyone" } }),
            ),
            (
                &["kanban", "move", "12", "In", "review"][..],
                json!({ "name": "move_card", "arguments": { "card": "12", "column": "In review" } }),
            ),
            (
                &["kanban", "comment", "12", "On", "it."][..],
                json!({ "name": "comment_card", "arguments": { "card": "12", "text": "On it." } }),
            ),
            (
                &["kanban", "edit-comment", "12", "41", "Fixed", "it."][..],
                json!({ "name": "edit_comment", "arguments": { "card": "12", "comment": "41", "text": "Fixed it." } }),
            ),
            (
                &["kanban", "delete-comment", "12", "41"][..],
                json!({ "name": "delete_comment", "arguments": { "card": "12", "comment": "41" } }),
            ),
            (
                &["kanban", "claim", "12"][..],
                json!({ "name": "claim_card", "arguments": { "card": "12" } }),
            ),
            (
                &["note", "read", "4qm2a"][..],
                json!({ "name": "read_note", "arguments": { "block": "4qm2a" } }),
            ),
            (
                &["note", "append", "4qm2a", "Next:", "tests"][..],
                json!({ "name": "append_note", "arguments": { "block": "4qm2a", "text": "Next: tests" } }),
            ),
            (
                &["checklist", "read", "4qm2a"][..],
                json!({ "name": "read_checklist", "arguments": { "block": "4qm2a" } }),
            ),
            (
                &["checklist", "add", "4qm2a", "Run", "tests"][..],
                json!({ "name": "add_items", "arguments": { "block": "4qm2a", "text": "Run tests" } }),
            ),
            (
                &["checklist", "check", "4qm2a", "13"][..],
                json!({ "name": "check_item", "arguments": { "block": "4qm2a", "item": "13" } }),
            ),
            (
                &["checklist", "uncheck", "4qm2a", "13"][..],
                json!({ "name": "check_item", "arguments": { "block": "4qm2a", "item": "13", "done": false } }),
            ),
            (
                &["open", "src/main.rs:12"][..],
                json!({ "name": "open_file", "arguments": { "path": at("src/main.rs"), "line": 12 } }),
            ),
            (
                &["open", "README.md"][..],
                json!({ "name": "open_file", "arguments": { "path": at("README.md") } }),
            ),
            (
                &["open", r"C:\src\a.rs"][..],
                json!({ "name": "open_file", "arguments": { "path": at(r"C:\src\a.rs") } }),
            ),
            (
                &["open", r"C:\src\a.rs:12"][..],
                json!({ "name": "open_file", "arguments": { "path": at(r"C:\src\a.rs"), "line": 12 } }),
            ),
            // path:line:column, as compilers and grep print it: the column is dropped.
            (
                &["open", "src/main.rs:12:5"][..],
                json!({ "name": "open_file", "arguments": { "path": at("src/main.rs"), "line": 12 } }),
            ),
        ] {
            assert_eq!(run_with(args, &env(&bus), "").0, 0, "{args:?}");
            assert_eq!(calls.recv_timeout(LIMIT).unwrap(), sent, "{args:?}");
        }
        for (args, usage) in [
            (&["kanban"][..], "usage: templum kanban"),
            (
                &["kanban", "add", "--column", "Doing"][..],
                "usage: templum kanban",
            ),
            (&["kanban", "move", "12"][..], "usage: templum kanban"),
            (&["kanban", "edit", "12"][..], "usage: templum kanban"),
            (
                &["kanban", "edit", "12", "--check"][..],
                "usage: templum kanban",
            ),
            (&["kanban", "list", "--card"][..], "usage: templum kanban"),
            (&["note", "append", "4qm2a"][..], "usage: templum note"),
            (
                &["checklist", "add", "4qm2a"][..],
                "usage: templum checklist",
            ),
            (
                &["checklist", "check", "4qm2a"][..],
                "usage: templum checklist",
            ),
            (&["open"][..], "usage: templum open"),
        ] {
            let (code, _, err) = run_with(args, &env(&bus), "");
            assert_eq!(code, 2, "{args:?}");
            assert!(err.starts_with(usage), "{args:?}: {err}");
        }
    }
}
