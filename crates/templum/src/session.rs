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
use serde::{Deserialize, Deserializer, Serialize};
use templum_canvas::{
    BlockId, Camera, Color, Drawing, MAX_WIDTH, MIN_WIDTH, Point, Rect, ShapeKind, Split, Tile,
};
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

/// How long a Templum starting waits for one quitting (still saving) to let go of the data
/// folder: reopened right after quitting, it isn't a second Templum.
pub(crate) const QUITTING: Duration = Duration::from_secs(3);

/// Takes the data folder `dir` for this Templum while the file returned stays open
/// (docs/specs/persistence.md, One Templum per data folder): `WouldBlock` when another Templum
/// still has it after `wait`. The system lets go when Templum ends, also when it crashes.
pub(crate) fn lock(dir: &Path, wait: Duration) -> io::Result<std::fs::File> {
    std::fs::create_dir_all(dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("templum.lock"))?;
    let until = std::time::Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// `dir` for `--dev`: its sibling `<name>-dev` (`~/.local/share/templum-dev`).
pub(crate) fn dev(dir: PathBuf) -> PathBuf {
    let mut name = dir.file_name().unwrap_or_default().to_os_string();
    name.push("-dev");
    dir.with_file_name(name)
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
    /// The Git popup's size the person dragged it to; none until they do. Kept as dragged: the
    /// window it was set in may be smaller or larger than the one it is drawn in.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "usable_size"
    )]
    pub git_size: Option<SavedSize>,
    /// The card dialog's description height the person dragged it to, pixels; none until they
    /// do, and the field grows with its text. Kept as dragged, like `git_size`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "usable_height"
    )]
    pub description_h: Option<f32>,
}

/// A width and a height, pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedSize {
    pub w: f32,
    pub h: f32,
}

/// A size with both sides above zero; anything else (a typo, a negative side) is none. A number
/// too large for JSON (`1e400`) fails the whole index, as anywhere else in it.
fn usable_size<'de, D: Deserializer<'de>>(d: D) -> Result<Option<SavedSize>, D::Error> {
    Ok(
        serde_json::from_value::<SavedSize>(serde_json::Value::deserialize(d)?)
            .ok()
            .filter(|s| s.w > 0.0 && s.h > 0.0 && s.w.is_finite() && s.h.is_finite()),
    )
}

/// A height above zero; anything else is none, as `usable_size`.
fn usable_height<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f32>, D::Error> {
    Ok(
        serde_json::from_value::<f32>(serde_json::Value::deserialize(d)?)
            .ok()
            .filter(|h| *h > 0.0 && h.is_finite()),
    )
}

fn shown() -> bool {
    true
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct IndexEntry {
    /// Names its file; chosen once, never changed.
    pub id: String,
    pub name: String,
    /// A Lucide icon's name; one Templum doesn't know is none.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "known_icon"
    )]
    pub icon: Option<String>,
    /// One of `space_dialog::COLORS`; another is none.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "known_color"
    )]
    pub color: Option<String>,
    /// A file in `workspaces/icons/`, shown instead of the icon; a name that could leave the
    /// folder is none.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "safe_image"
    )]
    pub image: Option<String>,
    /// Where its new terminals start.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "any_folder"
    )]
    pub folder: Option<PathBuf>,
    /// Its programs were ended; opening it starts them again.
    #[serde(
        default,
        skip_serializing_if = "std::ops::Not::not",
        deserialize_with = "any_true"
    )]
    pub stopped: bool,
    /// When it was last opened (shown), Unix seconds: the Home's "Opened 3 h ago". A value that
    /// doesn't read is none.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "any_time"
    )]
    pub opened: Option<u64>,
    /// A workspace of another computer (docs/specs/templum-to-templum.md, Persistence on the
    /// mirror): no `<id>.json` is ever written for it. A value that doesn't read is none.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "any_remote"
    )]
    pub remote: Option<RemoteEntry>,
}

/// Where a remote's key is kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Keeping {
    Keyring,
    File,
    #[default]
    None,
}

/// What the index keeps of a remote workspace: where it connects and how, never a secret (the
/// token is in the keyring, under `entry`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct RemoteEntry {
    /// The code's address.
    pub url: String,
    /// `pinned`, `public` or `loopback`.
    pub way: String,
    pub pin: Option<String>,
    /// Home's workspace id.
    pub home: Option<String>,
    /// The keyring entry's name.
    pub entry: String,
    pub key: Keeping,
    pub trust_added_authority: bool,
    /// The Cloudflare Access service token's Client ID.
    pub access_id: Option<String>,
    /// `http://host:port`, without credentials.
    pub proxy: Option<String>,
    pub camera: Option<SavedCamera>,
}

// The keys a workspace's look added: a value Templum can't use (a typo, a newer version's) is
// none, never a reason to set the whole list aside.

/// A string, or none for anything else.
fn any_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(s) => Some(s),
        _ => None,
    })
}

fn known_icon<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(any_string(d)?.filter(|n| crate::space_dialog::icon(n).is_some()))
}

fn known_color<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(any_string(d)?.filter(|n| crate::space_dialog::COLORS.contains(&n.as_str())))
}

fn safe_image<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(any_string(d)?.filter(|n| safe_file_name(n)))
}

fn any_folder<'de, D: Deserializer<'de>>(d: D) -> Result<Option<PathBuf>, D::Error> {
    Ok(any_string(d)?.map(PathBuf::from))
}

fn any_remote<'de, D: Deserializer<'de>>(d: D) -> Result<Option<RemoteEntry>, D::Error> {
    Ok(serde_json::from_value(serde_json::Value::deserialize(d)?).ok())
}

fn any_time<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Ok(serde_json::Value::deserialize(d)?.as_u64())
}

fn any_true<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    Ok(serde_json::Value::deserialize(d)? == serde_json::Value::Bool(true))
}

/// Letters, digits, `-`, `_` and `.`, and no `..`: a name that stays in its folder.
fn safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
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
                ..IndexEntry::default()
            }],
            active: 0,
            sidebar: true,
            git_size: None,
            description_h: None,
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
            // The one shown last may be stopped (every workspace was): Templum opens on the Home,
            // which holds it stopped.
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

/// The images workspaces show instead of their icons.
pub(crate) fn icons_dir(dir: &Path) -> PathBuf {
    dir.join("workspaces").join("icons")
}

/// The image kinds a workspace can show.
pub(crate) const IMAGE_KINDS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "svg"];

/// The largest image a workspace takes.
pub(crate) const IMAGE_MAX: u64 = 4 * 1024 * 1024;

/// Why `file` can't be a workspace's image: not one of IMAGE_KINDS, too big, or unreadable.
pub(crate) fn image_refused(file: &Path) -> Option<String> {
    let kind = file
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !IMAGE_KINDS.contains(&kind.as_str()) {
        return Some("Choose a PNG, JPG, WebP or SVG image".into());
    }
    match std::fs::metadata(file) {
        Ok(m) if m.len() > IMAGE_MAX => Some("That image is over 4 MB".into()),
        Ok(_) => None,
        Err(err) => Some(format!("Couldn't read the image: {err}")),
    }
}

/// Copies `file` as workspace `id`'s image, `<id>.<kind>` in `icons_dir`, in place of an older
/// one; gives its name.
pub(crate) fn copy_icon(dir: &Path, id: &str, file: &Path) -> io::Result<String> {
    if let Some(why) = image_refused(file) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, why));
    }
    let kind = file
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let icons = icons_dir(dir);
    std::fs::create_dir_all(&icons)?;
    let name = format!("{id}.{kind}");
    // Copied aside first: the file chosen may be the older image itself.
    let part = icons.join(format!("{name}.part"));
    std::fs::copy(file, &part)?;
    for older in IMAGE_KINDS {
        remove_icon(dir, &format!("{id}.{older}"))?;
    }
    std::fs::rename(&part, icons.join(&name))?;
    Ok(name)
}

/// Removes a workspace's image (`name` in `icons_dir`); one already gone is fine.
pub(crate) fn remove_icon(dir: &Path, name: &str) -> io::Result<()> {
    if !safe_file_name(name) {
        return Ok(());
    }
    match std::fs::remove_file(icons_dir(dir).join(name)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
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
    /// The folders agents were last opened in on this canvas, newest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent: Vec<String>,
    /// The drawings, above the blocks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ink: Vec<InkItem>,
    /// Grid mode's tiles, by item index (docs/specs/grid.md); the camera and the items' rects are
    /// the canvas's still.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<SavedTile>,
    /// The wires between the items, by their `uid`s; an older Templum ignores them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wires: Vec<SavedWire>,
    /// The workspace's Git; an older Templum ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<SavedGit>,
}

/// Grid mode's tree of tiles: an item, by its index in `items`, or a pair. `split` is "beside"
/// (side by side), anything else one above the other: a word no Templum knows never loses the
/// file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum SavedTile {
    Item(usize),
    Pair {
        split: String,
        a: Box<SavedTile>,
        b: Box<SavedTile>,
    },
}

/// A wire between two blocks, by their block ids (docs/specs/connections.md, Files).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedWire {
    pub a: String,
    pub b: String,
    /// Its name (Rename wire…), when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl SavedTile {
    /// `tile` with each block as its item's index (`index`); None when a block isn't an item.
    pub fn of(tile: &Tile, index: &impl Fn(BlockId) -> Option<usize>) -> Option<SavedTile> {
        Some(match tile {
            Tile::Leaf(id) => SavedTile::Item(index(*id)?),
            Tile::Pair(split, a, b) => SavedTile::Pair {
                split: match split {
                    Split::Beside => "beside",
                    Split::Above => "above",
                }
                .to_string(),
                a: Box::new(SavedTile::of(a, index)?),
                b: Box::new(SavedTile::of(b, index)?),
            },
        })
    }

    /// The tiles, `block` giving each item's block: items that didn't come back are left out (a
    /// pair left with one half is that half). None without any, or when an item is in it twice.
    pub fn tile(&self, block: impl Fn(usize) -> Option<BlockId> + Copy) -> Option<Tile> {
        let mut items = Vec::new();
        self.items(&mut items);
        let count = items.len();
        items.sort_unstable();
        items.dedup();
        if items.len() != count {
            return None;
        }
        self.build(block)
    }

    fn items(&self, into: &mut Vec<usize>) {
        match self {
            SavedTile::Item(i) => into.push(*i),
            SavedTile::Pair { a, b, .. } => {
                a.items(into);
                b.items(into);
            }
        }
    }

    fn build(&self, block: impl Fn(usize) -> Option<BlockId> + Copy) -> Option<Tile> {
        match self {
            SavedTile::Item(i) => block(*i).map(Tile::Leaf),
            SavedTile::Pair { split, a, b } => match (a.build(block), b.build(block)) {
                (Some(a), Some(b)) => Some(Tile::Pair(
                    if split == "beside" {
                        Split::Beside
                    } else {
                        Split::Above
                    },
                    Box::new(a),
                    Box::new(b),
                )),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            },
        }
    }
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
    Note(NoteItem),
    Checklist(ChecklistItem),
    Editor(EditorItem),
    Git(GitItem),
}

impl Item {
    pub fn rect_mut(&mut self) -> &mut SavedRect {
        match self {
            Item::Terminal(t) => &mut t.rect,
            Item::Kanban(k) => &mut k.rect,
            Item::Note(n) => &mut n.rect,
            Item::Checklist(c) => &mut c.rect,
            Item::Editor(e) => &mut e.rect,
            Item::Git(g) => &mut g.rect,
        }
    }

    /// The block's id (docs/specs/connections.md); empty in a file from before.
    pub fn uid(&self) -> &str {
        match self {
            Item::Terminal(t) => &t.uid,
            Item::Kanban(k) => &k.uid,
            Item::Note(n) => &n.uid,
            Item::Checklist(c) => &c.uid,
            Item::Editor(e) => &e.uid,
            Item::Git(g) => &g.uid,
        }
    }

    pub fn set_uid(&mut self, uid: String) {
        match self {
            Item::Terminal(t) => t.uid = uid,
            Item::Kanban(k) => k.uid = uid,
            Item::Note(n) => n.uid = uid,
            Item::Checklist(c) => c.uid = uid,
            Item::Editor(e) => e.uid = uid,
            Item::Git(g) => g.uid = uid,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct NoteItem {
    /// The block's id for agents and wires (docs/specs/connections.md); a file from before has
    /// none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uid: String,
    pub rect: SavedRect,
    /// The notes it shows as tabs, by their ids in `notes.json`, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<u64>,
    /// The tab on screen, by its index in `notes`.
    #[serde(default)]
    pub active: usize,
    /// A file from before `notes.json`: its one note's text, moved there when it is read.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Before `notes.json` too: that note's color, by name ("yellow"); none when missing or
    /// unknown.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::board::known_label"
    )]
    pub color: Option<crate::board::Label>,
}

/// A checklist block (docs/specs/checklist.md).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ChecklistItem {
    /// The block's id for agents and wires (docs/specs/connections.md).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uid: String,
    pub rect: SavedRect,
    /// The checklist it shows, by its id in `checklists.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checklist: Option<u64>,
    /// A file from before one checklist per block: the checklists it showed as tabs, in order,
    /// read, never written. The block shows the one on screen.
    #[serde(default, skip_serializing)]
    pub checklists: Vec<u64>,
    /// Before that too: the tab on screen, by its index in `checklists`.
    #[serde(default, skip_serializing)]
    pub active: usize,
}

/// An editor block (docs/specs/cockpit.md, section 7).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct EditorItem {
    /// The block's id for agents and wires (docs/specs/connections.md); a file from before has
    /// none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uid: String,
    pub rect: SavedRect,
    pub folder: String,
    /// The open files, relative to the folder, in tab order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open: Vec<String>,
    /// The active tab's index in `open`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<usize>,
    /// The file tree is shown.
    #[serde(default = "shown")]
    pub tree: bool,
}

/// A Git block of an older Templum: read, never written. Its Git becomes the workspace's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct GitItem {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uid: String,
    pub rect: SavedRect,
    #[serde(flatten)]
    pub git: SavedGit,
}

/// A workspace's Git (docs/specs/git.md): the popup's folder and what it was showing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedGit {
    pub folder: String,
    /// The tab shown: "changes", "history", "branches" or "stashes".
    #[serde(default = "changes")]
    pub tab: String,
    /// The commit message being written, left out when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// "folded": the left column is folded. Left out when it isn't.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lists: String,
    /// "split": the diff panels are side by side. Left out when they aren't.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub diff: String,
}

fn changes() -> String {
    "changes".to_string()
}

/// A drawing as the file holds it (docs/specs/cockpit.md, section 4): a `stroke` with `points`,
/// or a `line`, `arrow`, `rect` or `ellipse` with `from` and `to`. Points are `[x, y]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct InkItem {
    pub kind: String,
    pub color: String,
    pub width: f32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub highlight: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<[f32; 2]>,
}

impl InkItem {
    pub fn of(drawing: &Drawing) -> InkItem {
        let xy = |p: &Point| [p.x, p.y];
        match drawing {
            Drawing::Stroke {
                points,
                width,
                color,
                highlight,
            } => InkItem {
                kind: "stroke".into(),
                color: color.name().into(),
                width: *width,
                highlight: *highlight,
                points: points.iter().map(xy).collect(),
                from: None,
                to: None,
            },
            Drawing::Shape {
                kind,
                from,
                to,
                width,
                color,
            } => InkItem {
                kind: kind.name().into(),
                color: color.name().into(),
                width: *width,
                highlight: false,
                points: Vec::new(),
                from: Some(xy(from)),
                to: Some(xy(to)),
            },
        }
    }

    /// None for an unknown kind, a stroke under 2 points, a shape missing an end, or a number
    /// that isn't finite. An unknown color is the foreground; the width is clamped.
    pub fn drawing(&self) -> Option<Drawing> {
        let color = Color::from_name(&self.color).unwrap_or(Color::Foreground);
        if !self.width.is_finite() {
            return None;
        }
        let width = self.width.clamp(MIN_WIDTH, MAX_WIDTH);
        let point =
            |p: [f32; 2]| (p[0].is_finite() && p[1].is_finite()).then(|| Point::new(p[0], p[1]));
        if self.kind == "stroke" {
            let points = self
                .points
                .iter()
                .map(|p| point(*p))
                .collect::<Option<Vec<_>>>()?;
            if points.len() < 2 {
                return None;
            }
            return Some(Drawing::Stroke {
                points,
                width,
                color,
                highlight: self.highlight,
            });
        }
        Some(Drawing::Shape {
            kind: ShapeKind::from_name(&self.kind)?,
            from: point(self.from?)?,
            to: point(self.to?)?,
            width,
            color,
        })
    }
}

/// The API's drawing is the canvas file's, field for field: the file's format stays the file's.
impl From<&InkItem> for templum_api::model::DrawingView {
    fn from(item: &InkItem) -> Self {
        templum_api::model::DrawingView {
            kind: item.kind.clone(),
            color: item.color.clone(),
            width: item.width,
            highlight: item.highlight,
            points: item.points.clone(),
            from: item.from,
            to: item.to,
        }
    }
}

impl From<&templum_api::model::DrawingView> for InkItem {
    fn from(view: &templum_api::model::DrawingView) -> Self {
        InkItem {
            kind: view.kind.clone(),
            color: view.color.clone(),
            width: view.width,
            highlight: view.highlight,
            points: view.points.clone(),
            from: view.from,
            to: view.to,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct KanbanItem {
    /// The block's id for agents and wires (docs/specs/connections.md); a file from before has
    /// none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uid: String,
    pub rect: SavedRect,
    /// The board it shows; missing while it shows the board list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct TerminalItem {
    /// The block's id for agents and wires (docs/specs/connections.md); a file from before has
    /// none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uid: String,
    pub rect: SavedRect,
    /// Missing when the OS wouldn't say, or when the path isn't UTF-8 (JSON can't hold it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// The agent open in it, by id: resumed in the block's folder when Templum knows that id
    /// (one of its own, or one in `[agents]`); any other value is ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// The agent's session, by its id: the one resumed, so blocks in the same folder each get
    /// their own back. Missing when the agent's files don't tell it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// The block id of the agent that opened it with `spawn_agent`, which may close it; none for
    /// one the person opened or pasted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened_by: Option<String>,
    /// The model and the effort `spawn_agent` gave its agent, typed again when it resumes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// The account whose variable its shell has (docs/specs/accounts.md, Its block): its agent
    /// is that account. One Templum doesn't know gives a plain terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// The role `spawn_agent` opened it as: its words typed again on resume, while it exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// The shell it was opened with from a menu, by its label ("fish", "PowerShell 7"): started
    /// again when it is among the shells found here, else the default. A label, never a program
    /// or arguments: the file names a shell and runs no command (docs/specs/persistence.md,
    /// Safety). Missing: the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
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
    /// Folders agents were opened in, newest first.
    pub recent: Vec<PathBuf>,
    /// The agents to look for in the terminals.
    pub agents: std::sync::Arc<Vec<templum_terminal::Detect>>,
    /// The accounts: a terminal's account names its agent found there.
    pub accounts: std::sync::Arc<Vec<crate::agents::Home>>,
    pub ink: Vec<InkItem>,
    pub grid: Option<SavedTile>,
    pub wires: Vec<SavedWire>,
    pub git: Option<SavedGit>,
}

pub(crate) enum LayoutBlock {
    /// A terminal as the UI thread knows it: its id, rect, opener, model and effort, and as its
    /// agent the one they were typed for (else the one last seen); its folder, agent and session
    /// come from its processes, read in the background. While the agent of its saved item, typed
    /// back, isn't up yet, that item: its folder, agent and session are kept.
    Terminal(TerminalItem, Processes, Option<Box<TerminalItem>>),
    /// Any other kind: its item, complete, built on the UI thread.
    Saved(Item),
}

impl Layout {
    /// Reads the processes (folders, agents, and the agents' sessions in their files, under
    /// Templum's data folder `data`) and builds the file's content, with what was found in each
    /// terminal, in order. Slow: off the UI thread.
    pub fn snapshot(self, data: Option<&Path>) -> (Saved, Vec<templum_terminal::Inspection>) {
        let processes: Vec<Processes> = self
            .blocks
            .iter()
            .filter_map(|b| match b {
                LayoutBlock::Terminal(_, p, _) => Some(p.clone()),
                LayoutBlock::Saved(_) => None,
            })
            .collect();
        let homes: Vec<Option<&crate::agents::Home>> = self
            .blocks
            .iter()
            .filter_map(|b| match b {
                LayoutBlock::Terminal(item, _, _) => Some(
                    item.account
                        .as_deref()
                        .and_then(|a| self.accounts.iter().find(|h| h.id == a)),
                ),
                LayoutBlock::Saved(_) => None,
            })
            .collect();
        let inspections =
            name_accounts(templum_terminal::inspect(&processes, &self.agents), &homes);
        let sessions = data.map_or_else(Vec::new, |data| {
            session_ids(&procs(&inspections, &homes), data)
        });
        let mut sessions = sessions.into_iter();
        let mut found = inspections.iter().cloned();
        let saved = Saved {
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
                    LayoutBlock::Terminal(item, _, resuming) => {
                        // One answer per terminal, in order.
                        let found = found.next().unwrap_or_default();
                        let session = sessions.next().flatten();
                        // Its saved agent, typed back, isn't up yet: the next start resumes it
                        // still.
                        if let Some(saved) = resuming.as_ref().filter(|_| found.agent.is_none()) {
                            return Item::Terminal(TerminalItem {
                                folder: saved.folder.clone(),
                                agent: saved.agent.clone(),
                                session: saved.session.clone(),
                                ..item.clone()
                            });
                        }
                        // The model and the effort are the agent the UI thread knew there: gone,
                        // or another one found (typed by hand), they aren't its.
                        let theirs = item.agent.is_none() || item.agent == found.agent;
                        Item::Terminal(TerminalItem {
                            folder: found.folder.and_then(utf8),
                            // The agent's files first: they follow a switch inside it.
                            session: session.or(found.session),
                            agent: found.agent,
                            model: item.model.clone().filter(|_| theirs),
                            effort: item.effort.clone().filter(|_| theirs),
                            ..item.clone()
                        })
                    }
                    LayoutBlock::Saved(item) => item.clone(),
                })
                .collect(),
            focused: self.focused,
            recent: self.recent.into_iter().filter_map(utf8).collect(),
            ink: self.ink,
            grid: self.grid,
            wires: self.wires,
            git: self.git,
        };
        (saved, inspections)
    }
}

/// What a snapshot found in each terminal, in order, with the agent of a terminal's account
/// (`homes`, one per terminal) named by that account.
fn name_accounts(
    found: Vec<templum_terminal::Inspection>,
    homes: &[Option<&crate::agents::Home>],
) -> Vec<templum_terminal::Inspection> {
    found
        .into_iter()
        .zip(homes.iter().chain(std::iter::repeat(&None)))
        .map(|(mut f, home)| {
            f.agent = crate::agents::in_block(f.agent.as_deref(), *home);
            f
        })
        .collect()
}

/// The processes of what `name_accounts` found: an account's is its agent's, in the account's
/// folder.
fn procs(
    found: &[templum_terminal::Inspection],
    homes: &[Option<&crate::agents::Home>],
) -> Vec<templum_usage::Proc> {
    found
        .iter()
        .zip(homes.iter().chain(std::iter::repeat(&None)))
        .map(|(f, home)| {
            let agent = f.agent.clone().unwrap_or_default();
            let home = home.filter(|h| h.id == agent);
            templum_usage::Proc {
                agent: home.map_or(agent, |h| h.agent.clone()),
                pid: f.pid.unwrap_or(0),
                started: f.started.unwrap_or(0),
                cwd: f.folder.clone(),
                // The relay's file isn't read for a session's id.
                terminal: String::new(),
                home: home.map(|h| h.state.clone()),
                account: home.map(|h| h.id.clone()),
            }
        })
        .collect()
}

/// The session each process has open, in order, from the agents' files.
fn session_ids(procs: &[templum_usage::Proc], data: &Path) -> Vec<Option<String>> {
    let dirs = crate::agents::dirs(data);
    templum_usage::session_ids(procs, &dirs, crate::notes::now())
}

/// A path as JSON can hold it.
pub(crate) fn utf8(path: PathBuf) -> Option<String> {
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

/// Makes an edited file's ids (`ids`: all of them, in the file's order) safe to use: an id met
/// before gets a new one, so deleting one thing never deletes another, and so does 0, an item's
/// missing id (ids start at 1); `next_id` passes them all, so none is given out twice.
pub(crate) fn unique_ids<'a>(ids: impl IntoIterator<Item = &'a mut u64>, next_id: &mut u64) {
    let ids: Vec<&mut u64> = ids.into_iter().collect();
    let mut next = ids.iter().map(|id| **id).fold(*next_id, u64::max);
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        if *id == 0 || !seen.insert(*id) {
            next += 1;
            *id = next;
        }
    }
    *next_id = next;
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

    #[test]
    fn a_note_color_reads_back_and_unknown_or_missing_is_none() {
        let read = |json: &str| match serde_json::from_str::<Item>(json).unwrap() {
            Item::Note(n) => n,
            other => panic!("{other:?}"),
        };
        let base = r#"{"kind":"note","rect":{"x":0,"y":0,"w":1,"h":1},"text":"x""#;
        assert_eq!(read(&format!("{base}}}")).color, None);
        assert_eq!(read(&format!(r#"{base},"color":"teal"}}"#)).color, None);
        let note = read(&format!(r#"{base},"color":"yellow"}}"#));
        assert_eq!(note.color, Some(crate::board::Label::Yellow));
        let back = serde_json::to_string(&Item::Note(note)).unwrap();
        assert!(back.contains(r#""color":"yellow""#), "{back}");
        let plain = serde_json::to_string(&Item::Note(read(&format!("{base}}}")))).unwrap();
        assert!(!plain.contains("color"), "{plain}");
    }

    #[test]
    fn a_canvas_with_a_checklist_reads_back_equal() {
        let item = Item::Checklist(ChecklistItem {
            uid: "4qm2a".into(),
            rect: SavedRect {
                x: 1.0,
                y: 2.0,
                w: 320.0,
                h: 360.0,
            },
            checklist: Some(3),
            checklists: Vec::new(),
            active: 0,
        });
        let text = serde_json::to_string(&item).unwrap();
        assert!(text.contains(r#""kind":"checklist""#), "{text}");
        assert!(text.contains(r#""checklist":3"#), "{text}");
        assert_eq!(serde_json::from_str::<Item>(&text).unwrap(), item);
        let Item::Checklist(mut none) = item else {
            unreachable!()
        };
        none.checklist = None;
        let text = serde_json::to_string(&Item::Checklist(none)).unwrap();
        assert!(!text.contains(r#""checklist":"#), "{text}");
    }

    /// A file from before one checklist per block names its tabs: they are read, never written.
    #[test]
    fn a_checklist_blocks_old_tabs_are_read_and_not_written() {
        let text = r#"{"kind":"checklist","rect":{"x":0,"y":0,"w":320,"h":360},
                      "checklists":[3,5],"active":1}"#;
        let item: Item = serde_json::from_str(text).unwrap();
        let Item::Checklist(c) = &item else {
            panic!("{item:?}")
        };
        assert_eq!(
            (c.checklist, c.checklists.clone(), c.active),
            (None, vec![3, 5], 1)
        );
        let back = serde_json::to_string(&item).unwrap();
        assert!(
            !back.contains("checklists") && !back.contains("active"),
            "{back}"
        );
    }

    #[test]
    fn a_terminal_keeps_who_opened_it_its_model_and_effort() {
        let text = r#"{"kind":"terminal","rect":{"x":0,"y":0,"w":1,"h":1},"agent":"codex",
                      "opened_by":"b7k2m","model":"gpt-5.6-luna","effort":"high"}"#;
        let item: Item = serde_json::from_str(text).unwrap();
        let Item::Terminal(t) = &item else {
            panic!("{item:?}")
        };
        assert_eq!(
            (
                t.opened_by.as_deref(),
                t.model.as_deref(),
                t.effort.as_deref()
            ),
            (Some("b7k2m"), Some("gpt-5.6-luna"), Some("high"))
        );
        let back = serde_json::to_string(&item).unwrap();
        for part in [
            r#""opened_by":"b7k2m""#,
            r#""model":"gpt-5.6-luna""#,
            r#""effort":"high""#,
        ] {
            assert!(back.contains(part), "{back}");
        }
        // A file from before has none, and none is written.
        let older: Item =
            serde_json::from_str(r#"{"kind":"terminal","rect":{"x":0,"y":0,"w":1,"h":1}}"#)
                .unwrap();
        let Item::Terminal(o) = &older else {
            panic!("{older:?}")
        };
        assert_eq!((&o.opened_by, &o.model, &o.effort), (&None, &None, &None));
        let plain = serde_json::to_string(&older).unwrap();
        assert!(
            !plain.contains("opened_by") && !plain.contains("model") && !plain.contains("effort"),
            "{plain}"
        );
    }

    #[test]
    fn a_terminal_keeps_its_role() {
        let text = r#"{"kind":"terminal","rect":{"x":0,"y":0,"w":1,"h":1},"agent":"codex",
                      "role":"reviewer"}"#;
        let item: Item = serde_json::from_str(text).unwrap();
        let Item::Terminal(t) = &item else {
            panic!("{item:?}")
        };
        assert_eq!(t.role.as_deref(), Some("reviewer"));
        let back = serde_json::to_string(&item).unwrap();
        assert!(back.contains(r#""role":"reviewer""#), "{back}");
        let older: Item =
            serde_json::from_str(r#"{"kind":"terminal","rect":{"x":0,"y":0,"w":1,"h":1}}"#)
                .unwrap();
        let Item::Terminal(o) = &older else {
            panic!("{older:?}")
        };
        assert_eq!(o.role, None);
        assert!(!serde_json::to_string(&older).unwrap().contains("role"));
    }

    #[test]
    fn an_items_account_is_kept() {
        let text = r#"{"kind":"terminal","rect":{"x":0,"y":0,"w":1,"h":1},"agent":"claude2",
                      "account":"claude2"}"#;
        let item: Item = serde_json::from_str(text).unwrap();
        let Item::Terminal(t) = &item else {
            panic!("{item:?}")
        };
        assert_eq!(t.account.as_deref(), Some("claude2"));
        let back = serde_json::to_string(&item).unwrap();
        assert!(back.contains(r#""account":"claude2""#), "{back}");
        let older: Item =
            serde_json::from_str(r#"{"kind":"terminal","rect":{"x":0,"y":0,"w":1,"h":1}}"#)
                .unwrap();
        let Item::Terminal(o) = &older else {
            panic!("{older:?}")
        };
        assert_eq!(o.account, None);
        assert!(!serde_json::to_string(&older).unwrap().contains("account"));
    }

    #[test]
    fn an_accounts_agent_is_named_by_it_and_read_in_its_folder() {
        use templum_terminal::Inspection;
        let home = crate::agents::Home {
            id: "claude2".into(),
            agent: "claude".into(),
            state: PathBuf::from("/data/accounts/claude2"),
        };
        let found = |agent: &str, pid: u32| Inspection {
            agent: Some(agent.into()),
            pid: Some(pid),
            ..Default::default()
        };
        let homes = [Some(&home), None, Some(&home)];
        let named = name_accounts(
            vec![found("claude", 7), found("claude", 8), found("codex", 9)],
            &homes,
        );
        let agents: Vec<_> = named.iter().map(|f| f.agent.as_deref()).collect();
        assert_eq!(agents, [Some("claude2"), Some("claude"), Some("codex")]);
        let procs = procs(&named, &homes);
        assert_eq!(
            (
                procs[0].agent.as_str(),
                &procs[0].home,
                procs[0].account.as_deref()
            ),
            ("claude", &Some(home.state.clone()), Some("claude2"))
        );
        assert_eq!((procs[1].agent.as_str(), &procs[1].home), ("claude", &None));
        assert_eq!((procs[2].agent.as_str(), &procs[2].home), ("codex", &None));
    }

    #[test]
    fn an_old_git_block_still_reads_and_a_workspaces_git_round_trips() {
        // An older Templum's Git block: read, with its tab defaulting to Changes.
        let text = r#"{"kind":"git","rect":{"x":1,"y":2,"w":800,"h":560},"folder":"/p"}"#;
        let item: Item = serde_json::from_str(text).unwrap();
        let Item::Git(g) = &item else {
            panic!("{item:?}")
        };
        assert_eq!(
            (g.git.folder.as_str(), g.git.tab.as_str()),
            ("/p", "changes")
        );
        // The workspace's Git, with a commit message being written, comes back as it was.
        let mut saved = sample();
        let mut git = g.git.clone();
        git.summary = "Fix the build".into();
        git.diff = "split".into();
        saved.git = Some(git.clone());
        let back = serde_json::to_string(&saved).unwrap();
        assert!(!back.contains("lists"), "not folded, no key: {back}");
        let read: Saved = serde_json::from_str(&back).unwrap();
        assert_eq!(read.git, Some(git));
        saved.git = None;
        let back = serde_json::to_string(&saved).unwrap();
        assert!(!back.contains(r#""git""#), "none, no key: {back}");
    }

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
                    uid: String::new(),
                    rect: SavedRect {
                        x: 0.0,
                        y: 0.0,
                        w: 720.0,
                        h: 460.0,
                    },
                    folder: Some("/home/u/proj".into()),
                    agent: Some("claude".into()),
                    session: Some("0d5e5f4b-1c2d-4e3f-8a9b-0c1d2e3f4a5b".into()),
                    opened_by: None,
                    model: None,
                    effort: None,
                    account: None,
                    role: None,
                    shell: None,
                }),
                Item::Terminal(TerminalItem {
                    uid: String::new(),
                    rect: SavedRect {
                        x: 800.0,
                        y: 10.0,
                        w: 500.0,
                        h: 300.0,
                    },
                    folder: None,
                    agent: None,
                    session: None,
                    opened_by: None,
                    model: None,
                    effort: None,
                    account: None,
                    role: None,
                    shell: None,
                }),
                Item::Kanban(KanbanItem {
                    uid: String::new(),
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
            recent: vec!["/home/u/proj".into(), "/srv/site".into()],
            ink: Vec::new(),
            grid: None,
            wires: Vec::new(),
            git: None,
        }
    }

    #[test]
    fn wires_are_written_by_block_id_with_their_labels_and_left_out_when_there_are_none() {
        let mut saved = sample();
        assert!(
            !serde_json::to_string(&saved).unwrap().contains("wires"),
            "a file without wires keeps an older Templum's shape"
        );
        saved.wires = vec![
            SavedWire {
                a: "b7k2m".into(),
                b: "4qm2a".into(),
                label: None,
            },
            SavedWire {
                a: "b7k2m".into(),
                b: "t4q8p".into(),
                label: Some("dev server".into()),
            },
        ];
        let text = serde_json::to_string(&saved).unwrap();
        assert!(
            text.contains(
                r#""wires":[{"a":"b7k2m","b":"4qm2a"},{"a":"b7k2m","b":"t4q8p","label":"dev server"}]"#
            ),
            "{text}"
        );
        assert_eq!(serde_json::from_str::<Saved>(&text).unwrap(), saved);
    }

    #[test]
    fn an_editor_item_round_trips_and_its_tree_defaults_to_shown() {
        let text = r#"{"kind":"editor","rect":{"x":1,"y":2,"w":900,"h":600},"folder":"/p","open":["src/main.rs"],"active":0}"#;
        let item: Item = serde_json::from_str(text).unwrap();
        let Item::Editor(e) = &item else {
            panic!("{item:?}")
        };
        assert!(e.tree, "missing: shown");
        assert_eq!(e.open, ["src/main.rs"]);
        assert_eq!(e.active, Some(0));
        let back = serde_json::to_string(&item).unwrap();
        assert!(back.contains(r#""kind":"editor""#), "{back}");
        assert!(back.contains(r#""tree":true"#), "{back}");
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
    fn dev_keeps_its_own_folder_beside_the_installed_one() {
        let base = PathBuf::from("/home/u").join(".config");
        assert_eq!(dev(base.join("templum")), base.join("templum-dev"));
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
            "\"session\":\"0d5e5f4b-1c2d-4e3f-8a9b-0c1d2e3f4a5b\"",
            "\"kind\":\"kanban\"",
            "\"board\":7",
            "\"focused\":1",
            "\"recent\":[\"/home/u/proj\",\"/srv/site\"]",
        ] {
            assert!(text.contains(part), "{part} in {text}");
        }
        assert_eq!(
            text.matches("folder").count(),
            1,
            "a missing folder isn't written"
        );
        assert_eq!(text.matches("agent").count(), 1, "nor a missing agent");
        assert_eq!(text.matches("session").count(), 1, "nor a missing session");
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
    fn a_note_item_reads_back_and_an_old_one_keeps_its_text() {
        let d = tempfile::tempdir().unwrap();
        let mut saved = sample();
        saved.items.push(Item::Note(NoteItem {
            uid: String::new(),
            rect: SavedRect {
                x: 1.0,
                y: 2.0,
                w: 360.0,
                h: 280.0,
            },
            notes: vec![3, 5],
            active: 1,
            text: String::new(),
            color: None,
        }));
        save(d.path(), "n", &saved, 1).unwrap();
        assert_eq!(load(d.path(), "n"), Loaded::Saved(saved));
        let text = r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[
            {"kind":"note","rect":{"x":0,"y":0,"w":300,"h":200},"text":"t","preview":true}]}"#;
        std::fs::write(path(d.path(), "m"), text).unwrap();
        let Loaded::Saved(read) = load(d.path(), "m") else {
            panic!("not read");
        };
        assert!(matches!(&read.items[0], Item::Note(n) if n.text == "t" && n.notes.is_empty()));
    }

    #[test]
    fn a_grid_reads_back_by_item_index() {
        let d = tempfile::tempdir().unwrap();
        let mut saved = sample();
        let tile = Tile::Pair(
            Split::Beside,
            Box::new(Tile::Leaf(BlockId(7))),
            Box::new(Tile::Leaf(BlockId(3))),
        );
        let index = |id: BlockId| [BlockId(3), BlockId(7)].iter().position(|b| *b == id);
        saved.grid = SavedTile::of(&tile, &index);
        save(d.path(), "g", &saved, 1).unwrap();
        let text = std::fs::read_to_string(path(d.path(), "g")).unwrap();
        assert!(text.contains(r#""split": "beside""#), "{text}");
        let Loaded::Saved(read) = load(d.path(), "g") else {
            panic!("not read");
        };
        assert_eq!(read, saved);
        let back = read
            .grid
            .unwrap()
            .tile(|i| [BlockId(3), BlockId(7)].get(i).copied());
        assert_eq!(back, Some(tile));
    }

    #[test]
    fn a_grid_leaves_out_items_that_didnt_come_back() {
        let grid: SavedTile =
            serde_json::from_str(r#"{"split":"plaid","a":0,"b":{"split":"beside","a":1,"b":2}}"#)
                .unwrap();
        // Item 1 couldn't start: its pair is item 2 alone. An unknown split is one above the other.
        let tile = grid.tile(|i| (i != 1).then_some(BlockId(i as u64)));
        assert_eq!(
            tile,
            Some(Tile::Pair(
                Split::Above,
                Box::new(Tile::Leaf(BlockId(0))),
                Box::new(Tile::Leaf(BlockId(2)))
            ))
        );
        assert_eq!(grid.tile(|_| None), None, "nothing came back");
        let twice: SavedTile = serde_json::from_str(r#"{"split":"beside","a":0,"b":0}"#).unwrap();
        assert_eq!(
            twice.tile(|i| Some(BlockId(i as u64))),
            None,
            "an item twice"
        );
    }

    #[test]
    fn ink_round_trips_and_an_old_file_has_none() {
        use templum_canvas::{Color, Drawing, Point, ShapeKind};
        let d = tempfile::tempdir().unwrap();
        let drawings = vec![
            Drawing::Stroke {
                points: vec![Point::new(1.0, 2.0), Point::new(3.5, 4.0)],
                width: 4.0,
                color: Color::Red,
                highlight: false,
            },
            Drawing::Stroke {
                points: vec![Point::new(0.0, 0.0), Point::new(100.0, 0.0)],
                width: 16.0,
                color: Color::Yellow,
                highlight: true,
            },
            Drawing::Shape {
                kind: ShapeKind::Arrow,
                from: Point::new(0.0, 0.0),
                to: Point::new(120.0, 40.0),
                width: 2.0,
                color: Color::Foreground,
            },
        ];
        let mut saved = sample();
        saved.ink = drawings.iter().map(InkItem::of).collect();
        save(d.path(), "i", &saved, 1).unwrap();
        let Loaded::Saved(read) = load(d.path(), "i") else {
            panic!("not read");
        };
        assert_eq!(read, saved);
        let back: Vec<Drawing> = read.ink.iter().filter_map(InkItem::drawing).collect();
        assert_eq!(back, drawings);
        let text = std::fs::read_to_string(path(d.path(), "i")).unwrap();
        assert!(text.contains(r#""kind": "stroke""#) && text.contains(r#""highlight": true"#));
        assert_eq!(text.matches("highlight").count(), 1, "false is left out");
        // The file plan 1 wrote has no ink.
        let old = sample();
        save(d.path(), "o", &old, 1).unwrap();
        assert!(
            !std::fs::read_to_string(path(d.path(), "o"))
                .unwrap()
                .contains("ink")
        );
        assert_eq!(load(d.path(), "o"), Loaded::Saved(old));
    }

    #[test]
    fn bad_ink_entries_are_dropped_or_fixed() {
        use templum_canvas::{Color, Drawing};
        let item = |json: &str| serde_json::from_str::<InkItem>(json).unwrap().drawing();
        assert_eq!(
            item(r#"{"kind":"stroke","color":"plaid","width":1000,"points":[[0,0],[1,1]]}"#),
            Some(Drawing::Stroke {
                points: vec![
                    templum_canvas::Point::new(0.0, 0.0),
                    templum_canvas::Point::new(1.0, 1.0)
                ],
                width: 64.0,
                color: Color::Foreground,
                highlight: false,
            }),
            "unknown color: foreground; width clamped"
        );
        assert!(
            item(r#"{"kind":"stroke","color":"red","width":0,"points":[[0,0],[1,1]]}"#)
                .is_some_and(|d| d.width() == 1.0)
        );
        assert_eq!(
            item(r#"{"kind":"stroke","color":"red","width":4,"points":[[0,0]]}"#),
            None,
            "one point"
        );
        assert_eq!(
            item(r#"{"kind":"blob","color":"red","width":4,"from":[0,0],"to":[1,1]}"#),
            None
        );
        assert_eq!(
            item(r#"{"kind":"line","color":"red","width":4,"from":[0,0]}"#),
            None,
            "no end"
        );
        assert!(
            serde_json::from_str::<InkItem>(
                r#"{"kind":"stroke","color":"red","width":4,"points":[[0,0],[1,null]]}"#
            )
            .is_err(),
            "null isn't a number: the item doesn't parse, the file is set aside"
        );
        let nan = InkItem {
            kind: "line".into(),
            color: "red".into(),
            width: f32::NAN,
            highlight: false,
            points: Vec::new(),
            from: Some([0.0, 0.0]),
            to: Some([1.0, 1.0]),
        };
        assert_eq!(nan.drawing(), None, "a width that isn't a number");
    }

    #[test]
    fn a_file_this_templum_cant_use_is_set_aside() {
        let d = tempfile::tempdir().unwrap();
        moved(d.path(), "{nope");
        let newer = r#"{"version":2,"camera":{"x":0,"y":0,"zoom":1},"items":[]}"#;
        assert_eq!(moved(d.path(), newer), "it is from a newer Templum");
        let unknown = r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[{"kind":"sticker","text":"hi"}]}"#;
        moved(d.path(), unknown);
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
        assert!(saved.recent.is_empty(), "missing: no recent folders");
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
                    ..IndexEntry::default()
                },
                IndexEntry {
                    id: "w2".into(),
                    name: "Side project".into(),
                    ..IndexEntry::default()
                },
            ],
            active: 1,
            sidebar: false,
            git_size: None,
            description_h: None,
        }
    }

    #[test]
    fn a_remote_entry_round_trips_and_an_odd_one_is_none() {
        let d = tempfile::tempdir().unwrap();
        let mut index = two();
        index.workspaces[1].remote = Some(RemoteEntry {
            url: "https://192.168.1.5:7411".into(),
            way: "pinned".into(),
            pin: Some("ab".repeat(32)),
            home: Some("w3".into()),
            entry: "remote-ab12cd34".into(),
            key: Keeping::Keyring,
            camera: Some(SavedCamera {
                x: 10.0,
                y: -5.0,
                zoom: 0.5,
            }),
            ..RemoteEntry::default()
        });
        save_index(d.path(), &index).unwrap();
        let text = std::fs::read_to_string(index_path(d.path())).unwrap();
        assert!(
            !text.contains("tpl_") && text.contains("\"key\": \"keyring\""),
            "{text}"
        );
        assert_eq!(load_index(d.path()), Loaded::Saved(index));
        // A plain entry writes no "remote"; an odd value reads as none; an older index has none.
        let plain = serde_json::to_string(&two().workspaces[0]).unwrap();
        assert!(!plain.contains("remote"), "{plain}");
        for odd in ["7", "null", "\"x\"", "[1]"] {
            let entry = format!(r#"{{"id":"r1","name":"x","remote":{odd}}}"#);
            assert_eq!(
                serde_json::from_str::<IndexEntry>(&entry).unwrap().remote,
                None
            );
        }
        let old = r#"{"id":"w2","name":"x"}"#;
        assert_eq!(
            serde_json::from_str::<IndexEntry>(old).unwrap().remote,
            None
        );
    }

    #[test]
    fn the_git_popups_size_reads_back_and_a_size_that_cant_be_used_is_none() {
        let d = tempfile::tempdir().unwrap();
        let mut index = two();
        index.git_size = Some(SavedSize { w: 820.0, h: 610.5 });
        save_index(d.path(), &index).unwrap();
        assert_eq!(load_index(d.path()), Loaded::Saved(index));
        // Not written until the person sets one.
        save_index(d.path(), &two()).unwrap();
        let text = std::fs::read_to_string(index_path(d.path())).unwrap();
        assert!(!text.contains("git_size"), "{text}");
        // A value Templum can't use never sets the whole list aside.
        for bad in [
            r#""big""#,
            "null",
            r#"{"w":-5,"h":300}"#,
            r#"{"w":0,"h":300}"#,
            r#"{"w":9}"#,
        ] {
            std::fs::write(
                index_path(d.path()),
                format!(
                    r#"{{"version":1,"workspaces":[{{"id":"a","name":"A"}}],"git_size":{bad}}}"#
                ),
            )
            .unwrap();
            let Loaded::Saved(read) = load_index(d.path()) else {
                panic!("{bad}: the list was set aside");
            };
            assert_eq!(read.git_size, None, "{bad}");
            assert_eq!(read.workspaces.len(), 1, "{bad}");
        }
    }

    #[test]
    fn the_card_descriptions_height_reads_back_and_one_that_cant_be_used_is_none() {
        let d = tempfile::tempdir().unwrap();
        let mut index = two();
        index.description_h = Some(312.5);
        save_index(d.path(), &index).unwrap();
        assert_eq!(load_index(d.path()), Loaded::Saved(index));
        save_index(d.path(), &two()).unwrap();
        let text = std::fs::read_to_string(index_path(d.path())).unwrap();
        assert!(!text.contains("description_h"), "{text}");
        for bad in [r#""tall""#, "null", "-40", "0", "[1]"] {
            std::fs::write(
                index_path(d.path()),
                format!(
                    r#"{{"version":1,"workspaces":[{{"id":"a","name":"A"}}],"description_h":{bad}}}"#
                ),
            )
            .unwrap();
            let Loaded::Saved(read) = load_index(d.path()) else {
                panic!("{bad}: the list was set aside");
            };
            assert_eq!(read.description_h, None, "{bad}");
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
    fn an_index_entry_keeps_its_look_folder_and_state() {
        let d = tempfile::tempdir().unwrap();
        let mut index = two();
        index.active = 0;
        index.workspaces[1] = IndexEntry {
            icon: Some("rocket".into()),
            color: Some("blue".into()),
            image: Some("w2.png".into()),
            folder: Some(PathBuf::from("/home/me/side")),
            stopped: true,
            ..index.workspaces[1].clone()
        };
        save_index(d.path(), &index).unwrap();
        assert_eq!(load_index(d.path()), Loaded::Saved(index));
    }

    #[test]
    fn an_index_from_before_loads_and_saves_unchanged() {
        let d = tempfile::tempdir().unwrap();
        let text = r#"{"version":1,"workspaces":[{"id":"default","name":"Main"}],"active":0}"#;
        std::fs::create_dir_all(d.path().join("workspaces")).unwrap();
        std::fs::write(index_path(d.path()), text).unwrap();
        let Loaded::Saved(index) = load_index(d.path()) else {
            panic!("unread")
        };
        assert_eq!(index.workspaces[0], Index::first_run().workspaces[0]);
        let written = serde_json::to_string(&index).unwrap();
        for key in ["icon", "color", "image", "folder", "stopped"] {
            assert!(!written.contains(key), "{key} in {written}");
        }
    }

    #[test]
    fn an_unknown_color_or_icon_keeps_the_rest_of_the_index() {
        let d = tempfile::tempdir().unwrap();
        let text = r#"{"version":1,"workspaces":[
            {"id":"default","name":"Main","icon":"no-such-icon","color":"orange",
             "image":7,"folder":["x"],"stopped":"yes"},
            {"id":"w2","name":"Side","icon":"rocket","color":"accent","stopped":true}
        ],"active":1}"#;
        std::fs::create_dir_all(d.path().join("workspaces")).unwrap();
        std::fs::write(index_path(d.path()), text).unwrap();
        let Loaded::Saved(index) = load_index(d.path()) else {
            panic!("unread")
        };
        let (main, side) = (&index.workspaces[0], &index.workspaces[1]);
        assert_eq!(main.name, "Main");
        assert!(main.image.is_none() && main.folder.is_none() && !main.stopped);
        assert_eq!((main.icon.as_deref(), main.color.as_deref()), (None, None));
        assert_eq!(side.icon.as_deref(), Some("rocket"));
        assert_eq!(side.color.as_deref(), Some("accent"));
        assert!(
            side.stopped,
            "the active one too: every workspace was stopped"
        );
    }

    #[test]
    fn unsafe_image_names_are_dropped() {
        let entry = |image: &str| {
            let text = format!(r#"{{"id":"w2","name":"x","image":{image:?}}}"#);
            serde_json::from_str::<IndexEntry>(&text).unwrap().image
        };
        assert_eq!(entry("w2.png").as_deref(), Some("w2.png"));
        for bad in ["../x.png", "a/b.png", "a\\b.png", "..", ""] {
            assert_eq!(entry(bad), None, "{bad}");
        }
    }

    #[test]
    fn copying_an_icon_replaces_the_older_one_and_refuses_big_files() {
        let d = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let png = src.path().join("a.PNG");
        std::fs::write(&png, b"png").unwrap();
        assert_eq!(copy_icon(d.path(), "w2", &png).unwrap(), "w2.png");
        let webp = src.path().join("b.webp");
        std::fs::write(&webp, b"webp").unwrap();
        assert_eq!(copy_icon(d.path(), "w2", &webp).unwrap(), "w2.webp");
        let mut left: Vec<_> = std::fs::read_dir(icons_dir(d.path()))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["w2.webp"]);
        // The older image chosen again is copied before it is removed.
        let again = icons_dir(d.path()).join("w2.webp");
        assert_eq!(copy_icon(d.path(), "w2", &again).unwrap(), "w2.webp");
        assert_eq!(std::fs::read(&again).unwrap(), b"webp");
        let big = src.path().join("big.jpg");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(IMAGE_MAX + 1)
            .unwrap();
        assert!(copy_icon(d.path(), "w2", &big).is_err());
        let text = src.path().join("a.txt");
        std::fs::write(&text, b"x").unwrap();
        assert!(copy_icon(d.path(), "w2", &text).is_err());
        remove_icon(d.path(), "w2.webp").unwrap();
        remove_icon(d.path(), "w2.webp").unwrap();
        assert!(!again.exists());
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

    #[test]
    fn one_templum_holds_a_data_folder() {
        let d = tempfile::tempdir().unwrap();
        let first = lock(d.path(), Duration::ZERO).unwrap();
        let second = lock(d.path(), Duration::from_millis(100)).unwrap_err();
        assert_eq!(second.kind(), io::ErrorKind::WouldBlock);
        drop(first);
        // A shell another test is starting holds the file until its exec (fork): a moment.
        lock(d.path(), Duration::from_secs(1)).expect("free once the first Templum ends");
    }

    /// Reopened right after quitting: the first Templum, still saving, lets go in a moment.
    #[test]
    fn a_templum_waits_for_the_one_quitting() {
        let d = tempfile::tempdir().unwrap();
        let first = lock(d.path(), Duration::ZERO).unwrap();
        let quitting = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(first);
        });
        lock(d.path(), QUITTING).expect("taken once the first is done");
        quitting.join().unwrap();
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
                LayoutBlock::Terminal(
                    TerminalItem {
                        uid: "t1".into(),
                        rect: Rect::new(0.0, 0.0, 10.0, 10.0).into(),
                        folder: None,
                        agent: None,
                        session: None,
                        opened_by: Some("b7k2m".into()),
                        model: Some("opus".into()),
                        effort: Some("high".into()),
                        account: None,
                        role: None,
                        shell: None,
                    },
                    Processes::default(),
                    None,
                ),
                LayoutBlock::Saved(Item::Kanban(KanbanItem {
                    uid: String::new(),
                    rect: Rect::new(5.0, 5.0, 10.0, 10.0).into(),
                    board: Some(3),
                })),
                LayoutBlock::Terminal(
                    TerminalItem {
                        uid: "t2".into(),
                        rect: Rect::new(20.0, 0.0, 30.0, 10.0).into(),
                        folder: None,
                        agent: None,
                        session: None,
                        opened_by: None,
                        model: None,
                        effort: None,
                        account: None,
                        role: None,
                        shell: None,
                    },
                    Processes::default(),
                    None,
                ),
            ],
            focused: Some(1),
            recent: vec![PathBuf::from("/a"), PathBuf::from("/b")],
            agents: Default::default(),
            accounts: Default::default(),
            ink: Vec::new(),
            grid: None,
            wires: Vec::new(),
            git: None,
        };
        let (saved, _) = layout.snapshot(None);
        assert_eq!(saved.recent, ["/a", "/b"], "recent folders, newest first");
        let uids: Vec<&str> = saved.items.iter().map(Item::uid).collect();
        assert_eq!(uids, ["t1", "", "t2"], "each block keeps its id");
        let Item::Terminal(first) = &saved.items[0] else {
            panic!()
        };
        assert_eq!(
            (
                first.opened_by.as_deref(),
                first.model.as_deref(),
                first.effort.as_deref()
            ),
            (Some("b7k2m"), Some("opus"), Some("high")),
            "what the UI thread knows of it, kept"
        );
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
                Item::Note(n) => n.rect,
                Item::Checklist(c) => c.rect,
                Item::Editor(e) => e.rect,
                Item::Git(g) => g.rect,
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

    /// The UI thread knew Codex in the terminal, and the snapshot finds it gone (or another agent
    /// there, `claude` typed by hand): its model and effort aren't written with what is found, so
    /// a quit right then doesn't resume that agent with Codex's flags.
    #[test]
    fn a_snapshot_drops_the_model_of_an_agent_no_longer_there() {
        let layout = Layout {
            camera: Camera::at(templum_canvas::Point::new(0.0, 0.0), 1.0),
            blocks: vec![LayoutBlock::Terminal(
                TerminalItem {
                    uid: "t1".into(),
                    rect: Rect::new(0.0, 0.0, 10.0, 10.0).into(),
                    folder: None,
                    agent: Some("codex".into()),
                    session: None,
                    opened_by: Some("b7k2m".into()),
                    model: Some("gpt-5.6-luna".into()),
                    effort: Some("high".into()),
                    account: None,
                    role: None,
                    shell: None,
                },
                Processes::default(),
                None,
            )],
            focused: None,
            recent: Vec::new(),
            agents: Default::default(),
            accounts: Default::default(),
            ink: Vec::new(),
            grid: None,
            wires: Vec::new(),
            git: None,
        };
        let (saved, _) = layout.snapshot(None);
        let Item::Terminal(t) = &saved.items[0] else {
            panic!()
        };
        assert_eq!(
            (t.opened_by.as_deref(), &t.agent, &t.model, &t.effort),
            (Some("b7k2m"), &None, &None, &None)
        );
    }
}
