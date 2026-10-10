//! A shell in a PTY, its input encoding and a non-blocking view of its grid. No UI.

mod input;
pub mod palette;

pub use alacritty_terminal::vte::ansi::Rgb;
pub use input::{KeyInput, encode_key, paste_bytes};
