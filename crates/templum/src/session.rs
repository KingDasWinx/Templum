//! The saved canvases: one JSON file per workspace in the platform's data folder, read at startup
//! and written by snapshots, and the index that lists the workspaces (see
//! docs/specs/persistence.md and docs/specs/workspaces.md).

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use templum_canvas::{Camera, Rect};
use templum_terminal::Processes;

use crate::config;

/// The file format's version: raised only when an older Templum couldn't read the new files.
pub(crate) const VERSION: u32 = 1;
/// How often a snapshot is taken (and written when it changed).
pub(crate) const EVERY: Duration = Duration::from_secs(5);
/// The first workspace's id: the file the version without workspaces wrote.
pub(crate) const DEFAULT_ID: &str = "default";

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
        "macos" => home.map(|h| {
            h.join("Library")
                .join("Application Support")
                .join("Templum")
        }),
        _ => xdg
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".local").join("share")))
            .map(|d| d.join("templum")),
    }
}

/// A workspace's canvas file.
pub(crate) fn path(dir: &Path, id: &str) -> PathBuf {
    dir.join("workspaces").join(format!("{id}.json"))
}

pub(crate) fn index_path(dir: &Path) -> PathBuf {
    dir.join("workspaces").join("index.json")
}

/// The workspaces, in the sidebar's order, and what the window showed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Index {
    pub version: u32,
    pub workspaces: Vec<IndexEntry>,
    /// The active workspace's position.
    #[serde(default)]
    pub active: usize,
    /// The sidebar is shown.
    #[serde(default = "shown")]
    pub sidebar: bool,
}

fn shown() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct IndexEntry {
    /// Names its file; chosen once, never changed.
    pub id: String,
    pub name: String,
}

impl Index {
    /// No index yet (a first run, or the version without workspaces): one workspace, whose file
    /// is the one that version wrote.
    pub fn first_run() -> Index {
        Index {
            version: VERSION,
            workspaces: vec![IndexEntry {
                id: DEFAULT_ID.into(),
                name: "Main".into(),
            }],
            active: 0,
            sidebar: true,
        }
    }
}

/// Letters, digits, `-` and `_` only: an edited index can't name a file outside the folder.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The id for a new workspace: the smallest `w<n>` from 2 that isn't `taken`.
pub(crate) fn next_id(taken: impl Fn(&str) -> bool) -> String {
    (2..)
        .map(|n| format!("w{n}"))
        .find(|id| !taken(id))
        .expect("an unused id")
}

/// An id is taken by a workspace in the index, or by a file left in the folder (a deleted
/// workspace's `.bak`): a new workspace must never open an old canvas.
pub(crate) fn id_in_folder(dir: &Path, id: &str) -> bool {
    let file = path(dir, id);
    file.exists() || file.with_extension("json.bak").exists()
}

pub(crate) fn load_index(dir: &Path) -> Loaded<Index> {
    match read::<Index>(&index_path(dir)) {
        Loaded::Saved(mut index) => {
            let mut seen = Vec::new();
            index.workspaces.retain(|w| {
                let keep = valid_id(&w.id) && !seen.contains(&w.id);
                seen.push(w.id.clone());
                keep
            });
            if index.workspaces.is_empty() {
                return Loaded::Saved(Index::first_run());
            }
            index.active = index.active.min(index.workspaces.len() - 1);
            Loaded::Saved(index)
        }
        other => other,
    }
}

pub(crate) fn save_index(dir: &Path, index: &Index) -> io::Result<()> {
    let path = index_path(dir);
    let text = serde_json::to_string_pretty(index).map_err(io::Error::other)?;
    std::fs::create_dir_all(path.parent().expect("a workspaces folder"))?;
    config::write_whole(&path, &text)
}

/// A deleted workspace's canvas is kept as `<id>.json.bak`, replacing an older one.
pub(crate) fn set_aside(dir: &Path, id: &str) -> io::Result<()> {
    let path = path(dir, id);
    match std::fs::rename(&path, path.with_extension("json.bak")) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
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
    Kanban(KanbanItem),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct KanbanItem {
    pub rect: SavedRect,
    /// The board it shows; missing while it shows the board list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct TerminalItem {
    pub rect: SavedRect,
    /// Missing when the OS wouldn't say, or when the path isn't UTF-8 (JSON can't hold it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// "claude" or "codex": resumed in the block's folder. Any other value is ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

impl TerminalItem {
    /// The agent to resume: only a known one, so an edited file can't make Templum type anything
    /// else.
    pub fn agent(&self) -> Option<templum_terminal::Agent> {
        self.agent
            .as_deref()
            .and_then(templum_terminal::Agent::from_name)
    }
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
    /// Back to front.
    pub blocks: Vec<LayoutBlock>,
    pub focused: Option<usize>,
}

pub(crate) enum LayoutBlock {
    /// A terminal's rect and its processes.
    Terminal(Rect, Processes),
    /// A kanban block's rect and its board.
    Kanban(Rect, Option<u64>),
}

impl Layout {
    /// Reads the processes (folders) and builds the file's content. Slow: off the UI thread.
    pub fn snapshot(self) -> Saved {
        let processes: Vec<Processes> = self
            .blocks
            .iter()
            .filter_map(|b| match b {
                LayoutBlock::Terminal(_, p) => Some(*p),
                LayoutBlock::Kanban(..) => None,
            })
            .collect();
        let mut found = templum_terminal::inspect(&processes).into_iter();
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
                .map(|b| match b {
                    LayoutBlock::Terminal(rect, _) => {
                        // One answer per terminal, in order.
                        let found = found.next().unwrap_or_default();
                        Item::Terminal(TerminalItem {
                            rect: (*rect).into(),
                            folder: found.folder.and_then(utf8),
                            agent: found.agent.map(|a| a.name().to_string()),
                        })
                    }
                    LayoutBlock::Kanban(rect, board) => Item::Kanban(KanbanItem {
                        rect: (*rect).into(),
                        board: *board,
                    }),
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

/// What reading a saved file found.
#[derive(Debug, PartialEq)]
pub(crate) enum Loaded<T> {
    /// No file: a first run.
    Nothing,
    Saved(T),
    /// A file this Templum can't use, renamed to `to` so it is never written over.
    Moved {
        to: PathBuf,
        why: String,
    },
    /// The file couldn't be read, or set aside: nothing may be written this session.
    Unreadable(String),
}

/// A workspace's canvas.
pub(crate) fn load(dir: &Path, id: &str) -> Loaded<Saved> {
    read(&path(dir, id))
}

/// A file with a `version`: one this Templum can't use (newer, broken) is set aside as `.bak`.
pub(crate) fn read<T: DeserializeOwned>(path: &Path) -> Loaded<T> {
    let path = path.to_path_buf();
    // Bytes, not a string: a file that isn't UTF-8 is bad content to set aside, not a read failure.
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Loaded::Nothing,
        Err(e) => return Loaded::Unreadable(format!("{}: {e}", path.display())),
    };
    #[derive(Deserialize)]
    struct Version {
        version: u32,
    }
    let why = match serde_json::from_slice::<Version>(&bytes) {
        Ok(v) if v.version > VERSION => "it is from a newer Templum".to_string(),
        _ => match serde_json::from_slice::<T>(&bytes) {
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

/// Writes a workspace's canvas through [`write_newest`].
pub(crate) fn save(dir: &Path, id: &str, saved: &Saved, generation: u64) -> io::Result<()> {
    let text = serde_json::to_string_pretty(saved).map_err(io::Error::other)?;
    write_newest(&path(dir, id), &text, generation)
}

/// Writes `text` to `path` (creating its folder). Writes take turns, and one older than the last
/// written to the same file is dropped: a background snapshot that finishes after the one taken
/// on quit.
pub(crate) fn write_newest(path: &Path, text: &str, generation: u64) -> io::Result<()> {
    static WRITTEN: LazyLock<Mutex<HashMap<PathBuf, u64>>> = LazyLock::new(Default::default);
    let mut written = WRITTEN.lock().unwrap_or_else(|e| e.into_inner());
    let last = written.entry(path.to_path_buf()).or_default();
    if generation < *last {
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().expect("a data folder"))?;
    config::write_whole(path, text)?;
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
                    agent: Some("claude".into()),
                }),
                Item::Terminal(TerminalItem {
                    rect: SavedRect {
                        x: 800.0,
                        y: 10.0,
                        w: 500.0,
                        h: 300.0,
                    },
                    folder: None,
                    agent: None,
                }),
                Item::Kanban(KanbanItem {
                    rect: SavedRect {
                        x: -900.0,
                        y: 0.0,
                        w: 900.0,
                        h: 540.0,
                    },
                    board: Some(7),
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
        save(d.path(), DEFAULT_ID, &sample(), 1).unwrap();
        assert_eq!(load(d.path(), DEFAULT_ID), Loaded::Saved(sample()));
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
            "\"agent\":\"claude\"",
            "\"kind\":\"kanban\"",
            "\"board\":7",
            "\"focused\":1",
        ] {
            assert!(text.contains(part), "{part} in {text}");
        }
        assert_eq!(
            text.matches("folder").count(),
            1,
            "a missing folder isn't written"
        );
        assert_eq!(text.matches("agent").count(), 1, "nor a missing agent");
    }

    #[test]
    fn only_known_agents_are_resumed() {
        let item = |agent: Option<&str>| TerminalItem {
            rect: SavedRect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            folder: None,
            agent: agent.map(String::from),
        };
        assert_eq!(
            item(Some("claude")).agent(),
            Some(templum_terminal::Agent::Claude)
        );
        assert_eq!(
            item(Some("codex")).agent(),
            Some(templum_terminal::Agent::Codex)
        );
        assert_eq!(item(Some("rm -rf ~")).agent(), None);
        assert_eq!(item(None).agent(), None);
    }

    #[test]
    fn no_file_is_a_first_run() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(load(d.path(), DEFAULT_ID), Loaded::Nothing);
    }

    fn write(dir: &Path, text: &str) {
        std::fs::create_dir_all(dir.join("workspaces")).unwrap();
        std::fs::write(path(dir, DEFAULT_ID), text).unwrap();
    }

    fn moved(dir: &Path, text: &str) -> String {
        write(dir, text);
        let Loaded::Moved { to, why } = load(dir, DEFAULT_ID) else {
            panic!("{text} wasn't set aside");
        };
        assert_eq!(to, path(dir, DEFAULT_ID).with_extension("json.bak"));
        assert_eq!(
            std::fs::read_to_string(&to).unwrap(),
            text,
            "the old bytes are kept"
        );
        assert!(!path(dir, DEFAULT_ID).exists());
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
    fn a_file_that_isnt_utf8_is_set_aside_too() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("workspaces")).unwrap();
        // UTF-16, as some Windows tools write.
        std::fs::write(path(d.path(), DEFAULT_ID), [0xff, 0xfe, b'{', 0]).unwrap();
        assert!(
            matches!(load(d.path(), DEFAULT_ID), Loaded::Moved { .. }),
            "not Unreadable: that would stop every future save"
        );
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            r#"{"version":1,"later":true,"camera":{"x":1,"y":2,"zoom":1,"tilt":3},
               "items":[{"kind":"terminal","rect":{"x":0,"y":0,"w":10,"h":10},"color":"red"}]}"#,
        );
        let Loaded::Saved(saved) = load(d.path(), DEFAULT_ID) else {
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
        save(d.path(), DEFAULT_ID, &newer, 7).unwrap();
        save(d.path(), DEFAULT_ID, &sample(), 6).unwrap();
        assert_eq!(load(d.path(), DEFAULT_ID), Loaded::Saved(newer));
    }

    fn two() -> Index {
        Index {
            version: VERSION,
            workspaces: vec![
                IndexEntry {
                    id: DEFAULT_ID.into(),
                    name: "Main".into(),
                },
                IndexEntry {
                    id: "w2".into(),
                    name: "Side project".into(),
                },
            ],
            active: 1,
            sidebar: false,
        }
    }

    #[test]
    fn an_index_reads_back_equal() {
        let d = tempfile::tempdir().unwrap();
        save_index(d.path(), &two()).unwrap();
        assert_eq!(load_index(d.path()), Loaded::Saved(two()));
        let text = std::fs::read_to_string(index_path(d.path())).unwrap();
        assert!(text.contains("\"name\": \"Side project\""), "{text}");
    }

    #[test]
    fn no_index_is_one_workspace_named_main_in_default_json() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(load_index(d.path()), Loaded::Nothing);
        let first = Index::first_run();
        assert_eq!(first.workspaces.len(), 1);
        assert_eq!(first.workspaces[0].id, DEFAULT_ID);
        assert_eq!(first.workspaces[0].name, "Main");
        assert!(first.sidebar);
    }

    #[test]
    fn a_broken_index_is_set_aside() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("workspaces")).unwrap();
        std::fs::write(index_path(d.path()), "{nope").unwrap();
        assert!(matches!(load_index(d.path()), Loaded::Moved { .. }));
        assert!(index_path(d.path()).with_extension("json.bak").exists());
    }

    #[test]
    fn ids_that_could_leave_the_folder_are_dropped() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("workspaces")).unwrap();
        std::fs::write(
            index_path(d.path()),
            r#"{"version":1,"workspaces":[{"id":"../x","name":"A"},{"id":"a/b","name":"B"},
               {"id":"","name":"C"},{"id":"ok-1_2","name":"D"},{"id":"ok-1_2","name":"E"}],
               "active":9}"#,
        )
        .unwrap();
        let Loaded::Saved(index) = load_index(d.path()) else {
            panic!("not loaded");
        };
        let ids: Vec<&str> = index.workspaces.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["ok-1_2"], "unsafe and repeated ids go");
        assert_eq!(index.active, 0, "kept in range");
        assert!(index.sidebar, "missing: shown");
        std::fs::write(index_path(d.path()), r#"{"version":1,"workspaces":[]}"#).unwrap();
        assert_eq!(load_index(d.path()), Loaded::Saved(Index::first_run()));
    }

    #[test]
    fn a_new_id_is_the_smallest_unused() {
        assert_eq!(next_id(|id| id == "default"), "w2");
        assert_eq!(next_id(|id| ["default", "w2", "w4"].contains(&id)), "w3");
        let d = tempfile::tempdir().unwrap();
        save(d.path(), "w2", &sample(), 1).unwrap();
        set_aside(d.path(), "w2").unwrap();
        assert!(
            id_in_folder(d.path(), "w2"),
            "a deleted workspace's id isn't reused"
        );
        assert!(!id_in_folder(d.path(), "w3"));
    }

    #[test]
    fn a_deleted_workspace_keeps_its_file_as_bak() {
        let d = tempfile::tempdir().unwrap();
        save(d.path(), "w2", &sample(), 1).unwrap();
        set_aside(d.path(), "w2").unwrap();
        assert!(!path(d.path(), "w2").exists());
        assert!(path(d.path(), "w2").with_extension("json.bak").exists());
        set_aside(d.path(), "never-saved").unwrap();
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
                LayoutBlock::Terminal(Rect::new(0.0, 0.0, 10.0, 10.0), Processes::default()),
                LayoutBlock::Kanban(Rect::new(5.0, 5.0, 10.0, 10.0), Some(3)),
                LayoutBlock::Terminal(Rect::new(20.0, 0.0, 30.0, 10.0), Processes::default()),
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
            .map(|item| match item {
                Item::Terminal(t) => t.rect,
                Item::Kanban(k) => k.rect,
            })
            .collect();
        assert!(matches!(&saved.items[1], Item::Kanban(k) if k.board == Some(3)));
        assert_eq!(
            rects[2],
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
