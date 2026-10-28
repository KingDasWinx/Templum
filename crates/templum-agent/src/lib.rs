//! The Templum Agent (docs/specs/templum-agent.md): the headless CLI agent the person talks to
//! in the chat, its stream read into events, and the conversation they make; and a Claude Code
//! subagent's transcript read into the text its block shows. No UI.

use std::path::Path;

pub mod claude;
pub mod conversation;
pub mod engine;
pub mod fake;
pub mod follow;

pub use conversation::{ActionState, Conversation, Load, NoticeLevel, Own, Row, Target};
pub use engine::{Engine, EngineLine, Launch};

/// The Templum Agent's word where a terminal's id would be: the asker of the tasks it hands and
/// the `opened_by` of the blocks it opens. No terminal's id is one (theirs are `<pid>-<n>`).
pub const ASKER: &str = "templum-agent";

/// What one line of the engine's stream says, for the chat.
#[derive(Clone, Debug, PartialEq)]
pub enum ChatEvent {
    /// A turn starts in this session, on this model.
    Started {
        session: String,
        model: Option<String>,
    },
    /// The engine thinks before its text or its tool.
    Thinking,
    /// More of the text being written.
    TextDelta(String),
    /// A text block's whole text, which replaces what was streamed of it.
    Text(String),
    /// A tool's call begins: its id and its name, before its arguments.
    ToolStarted { id: String, name: String },
    /// A tool's call, whole.
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// A tool's answer.
    ToolResult {
        id: String,
        text: String,
        is_error: bool,
    },
    /// The engine took a message the person wrote: the turn that answers it starts.
    Taken(String),
    /// The turn ended: its time and cost when the engine says them, and its error when it failed.
    TurnEnded {
        ms: Option<u64>,
        cost_usd: Option<f64>,
        error: Option<String>,
    },
}

/// Writes `bytes` to `path` through a temporary file renamed over it, so a reader never sees
/// half of it.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().expect("a file in a folder");
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().expect("a file name").to_string_lossy();
    let temp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}
