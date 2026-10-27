//! A shell in a PTY, its input encoding and a non-blocking view of its grid. No UI.

mod agent;
mod find;
mod input;
mod integration;
mod link;
pub mod palette;
#[cfg(target_os = "linux")]
mod scope;
mod screen;
mod scrollback;
mod shells;
mod snapshot;
mod tap;
mod terminal;

#[cfg(unix)]
pub use agent::read_shell_path;
pub use agent::{
    Agent, Detect, agent_of, installed, locate, on_path, path_var, program_name, shell_path_dirs,
    shell_path_pending,
};
pub use alacritty_terminal::index::Side;
pub use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle, Rgb};
pub use find::{Find, Toward};
pub use input::{
    Button, KEY_NAMES, KeyInput, MouseInput, MouseKind, NamedKey, key_name, named_key, paste_bytes,
    text_names, typed_bytes, typed_text,
};
pub use integration::install;
pub use link::{Target, resolve};
pub use palette::Palette;
pub use screen::{Paint, Screen, Styled};
pub use scrollback::Scrollback;
pub use shells::{ShellOption, available_shells, default_shell, default_shell_label, is_shell};
pub use snapshot::{Bar, Cursor, Row, Run, Snapshot, Span};
pub use tap::{Mark, Outcome, Pattern, Status};
#[cfg(windows)]
pub use terminal::quit_before_terminals;
pub use terminal::{
    Feeder, Inspection, Modes, Notify, Processes, Running, SelectionKind, SharedTerm, SpawnOptions,
    TermOptions, Terminal, VIEWER_HISTORY, inspect, kill_left, shells_of, strays, terminate, tree,
};
