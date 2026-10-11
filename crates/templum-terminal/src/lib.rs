//! A shell in a PTY, its input encoding and a non-blocking view of its grid. No UI.

mod agent;
mod input;
pub mod palette;
mod shells;
mod snapshot;
mod terminal;

pub use agent::{Agent, Detect, agent_of, installed, program_name};
pub use alacritty_terminal::index::Side;
pub use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle, Rgb};
pub use input::{KeyInput, encode_key, paste_bytes};
pub use palette::Palette;
pub use shells::{ShellOption, available_shells, default_shell, default_shell_label};
pub use snapshot::{Cursor, Row, Run, Snapshot, Span};
pub use terminal::{
    Inspection, Notify, Processes, SelectionKind, SharedTerm, SpawnOptions, TermOptions, Terminal,
    inspect,
};
