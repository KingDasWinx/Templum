//! The saved canvas: one JSON file per workspace in the platform's data folder, read at startup
//! and written by snapshots (see docs/specs/persistence.md).

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use templum_canvas::{Camera, Rect};
use templum_terminal::Processes;

use crate::config;

/// The file format's version: raised only when an older Templum couldn't read the new files.
pub(crate) const VERSION: u32 = 1;
/// How often a snapshot is taken (and written when it changed).
pub(crate) const EVERY: Duration = Duration::from_secs(5);
/// The only workspace for now.
const DEFAULT: &str = "default.json";

/// Templum's data folder: `$XDG_DATA_HOME/templum` or `~/.local/share/templum` on Linux,
/// `~/Library/Application Support/Templum` on macOS, `%APPDATA%\templum` on Windows.
pub(crate) fn dir() -> Option<PathBuf> {
    dir_from(
        std::env::consts::OS,
        std::env::var_os("APPDATA"),
        std::env::var_os("XDG_DATA_HOME"),
        std::env::home_dir(),
    )
}

fn dir_from(
    os: &str,
    appdata: Option<OsString>,
    xdg: Option<OsString>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    match os {
        "windows" => appdata.map(|d| PathBuf::from(d).join("templum")),
        "macos" => home.map(|h| h.join("Library").join("Application Support").join("Templum")),
        _ => xdg
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".local").join("share")))
            .map(|d| d.join("templum")),
    }
}

/// The default workspace's file.
pub(crate) fn path(dir: &Path) -> PathBuf {
    dir.join("workspaces").join(DEFAULT)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Saved {
    pub version: u32,
    pub camera: SavedCamera,
    /// Back to front: the last is drawn on top.
    pub items: Vec<Item>,
    /// The focused item's index in `items`.
    #[serde(default)]
    pub focused: Option<usize>,
}

/// The world point at the canvas's top left, and the zoom factor.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedCamera {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum Item {
    Terminal(TerminalItem),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct TerminalItem {
    pub rect: SavedRect,
    /// Missing when the OS wouldn't say, or when the path isn't UTF-8 (JSON can't hold it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl From<Rect> for SavedRect {
    fn from(r: Rect) -> Self {
        Self {
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
        }
    }
}

impl From<SavedRect> for Rect {
    fn from(r: SavedRect) -> Self {
        Rect::new(r.x, r.y, r.w, r.h)
    }
}

/// The canvas as the UI thread has it: what a snapshot needs before the processes are read.
pub(crate) struct Layout {
    pub camera: Camera,
    /// Back to front: each block's rect and its terminal's processes.
    pub blocks: Vec<(Rect, Processes)>,
    pub focused: Option<usize>,
}

impl Layout {
    /// Reads the processes (folders) and builds the file's content. Slow: off the UI thread.
    pub fn snapshot(self) -> Saved {
        let processes: Vec<Processes> = self.blocks.iter().map(|(_, p)| *p).collect();
        let found = templum_terminal::inspect(&processes);
        Saved {
            version: VERSION,
            camera: SavedCamera {
                x: self.camera.origin.x,
                y: self.camera.origin.y,
                zoom: self.camera.zoom(),
            },
            items: self
                .blocks
                .iter()
                .zip(found)
                .map(|((rect, _), found)| {
                    Item::Terminal(TerminalItem {
                        rect: (*rect).into(),
                        folder: found.folder.and_then(utf8),
                    })
                })
                .collect(),
            focused: self.focused,
        }
    }
}

/// A path as JSON can hold it.
fn utf8(path: PathBuf) -> Option<String> {
    path.into_os_string().into_string().ok()
}

/// What reading the saved canvas found.
#[derive(Debug, PartialEq)]
pub(crate) enum Loaded {
    /// No file: a first run.
    Nothing,
    Saved(Saved),
    /// A file this Templum can't use, renamed to `to` so it is never written over.
    Moved { to: PathBuf, why: String },
    /// The file couldn't be read, or set aside: nothing may be written this session.
    Unreadable(String),
}

pub(crate) fn load(dir: &Path) -> Loaded {
    let path = path(dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Loaded::Nothing,
        Err(e) => return Loaded::Unreadable(format!("{}: {e}", path.display())),
    };
    #[derive(Deserialize)]
    struct Version {
        version: u32,
    }
    let why = match serde_json::from_str::<Version>(&text) {
        Ok(v) if v.version > VERSION => "it is from a newer Templum".to_string(),
        _ => match serde_json::from_str::<Saved>(&text) {
            Ok(saved) => return Loaded::Saved(saved),
            Err(e) => e.to_string(),
        },
    };
    let to = path.with_extension("json.bak");
    match std::fs::rename(&path, &to) {
        Ok(()) => Loaded::Moved { to, why },
        Err(e) => Loaded::Unreadable(format!(
            "{}: {why}, and it couldn't be set aside: {e}",
            path.display()
        )),
    }
}

/// Writes `saved`. Writes take turns, and one older than the last written to the same file is
/// dropped: a background snapshot that finishes after the one taken on quit.
pub(crate) fn save(dir: &Path, saved: &Saved, generation: u64) -> io::Result<()> {
    static WRITTEN: LazyLock<Mutex<HashMap<PathBuf, u64>>> = LazyLock::new(Default::default);
    let mut written = WRITTEN.lock().unwrap_or_else(|e| e.into_inner());
    let path = path(dir);
    let last = written.entry(path.clone()).or_default();
    if generation < *last {
        return Ok(());
    }
    let text = serde_json::to_string_pretty(saved).map_err(io::Error::other)?;
    std::fs::create_dir_all(path.parent().expect("a workspaces folder"))?;
    config::write_whole(&path, &text)?;
    *last = generation;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Saved {
        Saved {
            version: VERSION,
            camera: SavedCamera {
                x: -120.0,
                y: 40.0,
                zoom: 1.25,
            },
            items: vec![
                Item::Terminal(TerminalItem {
                    rect: SavedRect {
                        x: 0.0,
                        y: 0.0,
                        w: 720.0,
                        h: 460.0,
                    },
                    folder: Some("/home/u/proj".into()),
                }),
                Item::Terminal(TerminalItem {
                    rect: SavedRect {
                        x: 800.0,
                        y: 10.0,
                        w: 500.0,
                        h: 300.0,
                    },
                    folder: None,
                }),
            ],
            focused: Some(1),
        }
    }

    #[test]
    fn dir_follows_each_platform() {
        let home = Some(PathBuf::from("/home/u"));
        assert_eq!(
            dir_from("linux", None, Some("/x".into()), home.clone()),
            Some(PathBuf::from("/x").join("templum"))
        );
        assert_eq!(
            dir_from("linux", None, Some("".into()), home.clone()),
            Some(
                PathBuf::from("/home/u")
                    .join(".local")
                    .join("share")
                    .join("templum")
            )
        );
        assert_eq!(
            dir_from("macos", None, None, home.clone()),
            Some(
                PathBuf::from("/home/u")
                    .join("Library")
                    .join("Application Support")
                    .join("Templum")
            )
        );
        assert_eq!(
            dir_from(
                "windows",
                Some("C:/Users/u/AppData/Roaming".into()),
                None,
                home
            ),
            Some(PathBuf::from("C:/Users/u/AppData/Roaming").join("templum"))
        );
        assert_eq!(dir_from("linux", None, None, None), None);
    }

    #[test]
    fn a_saved_canvas_reads_back_equal() {
        let d = tempfile::tempdir().unwrap();
        save(d.path(), &sample(), 1).unwrap();
        assert_eq!(load(d.path()), Loaded::Saved(sample()));
    }

    #[test]
    fn the_file_is_the_spec_format() {
        let text = serde_json::to_string(&sample()).unwrap();
        for part in [
            "\"version\":1",
            "\"camera\":{\"x\":-120.0,\"y\":40.0,\"zoom\":1.25}",
            "\"kind\":\"terminal\"",
            "\"rect\":{\"x\":0.0,\"y\":0.0,\"w\":720.0,\"h\":460.0}",
            "\"folder\":\"/home/u/proj\"",
            "\"focused\":1",
        ] {
            assert!(text.contains(part), "{part} in {text}");
        }
        assert_eq!(
            text.matches("folder").count(),
            1,
            "a missing folder isn't written"
        );
    }

    #[test]
    fn no_file_is_a_first_run() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(load(d.path()), Loaded::Nothing);
    }

    fn write(dir: &Path, text: &str) {
        std::fs::create_dir_all(dir.join("workspaces")).unwrap();
        std::fs::write(path(dir), text).unwrap();
    }

    fn moved(dir: &Path, text: &str) -> String {
        write(dir, text);
        let Loaded::Moved { to, why } = load(dir) else {
            panic!("{text} wasn't set aside");
        };
        assert_eq!(to, path(dir).with_extension("json.bak"));
        assert_eq!(
            std::fs::read_to_string(&to).unwrap(),
            text,
            "the old bytes are kept"
        );
        assert!(!path(dir).exists());
        why
    }

    #[test]
    fn a_file_this_templum_cant_use_is_set_aside() {
        let d = tempfile::tempdir().unwrap();
        moved(d.path(), "{nope");
        let newer = r#"{"version":2,"camera":{"x":0,"y":0,"zoom":1},"items":[]}"#;
        assert_eq!(moved(d.path(), newer), "it is from a newer Templum");
        let note = r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[{"kind":"note","text":"hi"}]}"#;
        moved(d.path(), note);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            r#"{"version":1,"later":true,"camera":{"x":1,"y":2,"zoom":1,"tilt":3},
               "items":[{"kind":"terminal","rect":{"x":0,"y":0,"w":10,"h":10},"color":"red"}]}"#,
        );
        let Loaded::Saved(saved) = load(d.path()) else {
            panic!("not loaded");
        };
        assert_eq!(saved.items.len(), 1);
        assert_eq!(saved.focused, None, "missing: nothing focused");
    }

    #[test]
    fn an_older_write_never_replaces_a_newer_one() {
        let d = tempfile::tempdir().unwrap();
        let mut newer = sample();
        newer.focused = Some(0);
        save(d.path(), &newer, 7).unwrap();
        save(d.path(), &sample(), 6).unwrap();
        assert_eq!(load(d.path()), Loaded::Saved(newer));
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_isnt_utf8_is_left_out() {
        use std::os::unix::ffi::OsStringExt as _;
        let odd = PathBuf::from(OsString::from_vec(vec![b'/', 0xff]));
        assert_eq!(utf8(odd), None);
        assert_eq!(utf8(PathBuf::from("/ok")), Some("/ok".to_string()));
    }

    #[test]
    fn a_snapshot_keeps_the_order_and_the_focus() {
        let camera = Camera::at(templum_canvas::Point::new(5.0, 6.0), 1.5);
        let layout = Layout {
            camera,
            // No processes: their folders are unknown.
            blocks: vec![
                (Rect::new(0.0, 0.0, 10.0, 10.0), Processes::default()),
                (Rect::new(20.0, 0.0, 30.0, 10.0), Processes::default()),
            ],
            focused: Some(1),
        };
        let saved = layout.snapshot();
        assert_eq!(saved.version, VERSION);
        assert_eq!(
            saved.camera,
            SavedCamera {
                x: 5.0,
                y: 6.0,
                zoom: 1.5
            }
        );
        let rects: Vec<SavedRect> = saved
            .items
            .iter()
            .map(|Item::Terminal(t)| t.rect)
            .collect();
        assert_eq!(
            rects[1],
            SavedRect {
                x: 20.0,
                y: 0.0,
                w: 30.0,
                h: 10.0
            }
        );
        assert_eq!(saved.focused, Some(1));
    }
}
