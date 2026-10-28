//! The trash: what a discard throws away, kept 7 days (the newest 50 discards of each
//! repository), so it can be put back. One folder per discard in Templum's data folder: the files as they were, and a manifest.
//! See docs/specs/git.md, section 4, The trash.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Past either, a discard keeps no copy, and the question says so before it is done.
pub const MAX_FILES: usize = 500;
pub const MAX_BYTES: u64 = 100 * 1024 * 1024;
/// Discards older than this, or past the newest KEEP of their repository, are deleted. The
/// copies an API call made are only ever deleted by age: they may hold the person's work, and
/// more calls can't push them out. At KEEP of them younger than this, a call that needs a
/// copy is refused instead.
pub const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
pub const KEEP: usize = 50;

/// The file in a discard's folder that says an API call made it.
const API_MARK: &str = "api";

/// The trash's folder (`trash/` in Templum's data folder).
#[derive(Clone, Debug)]
pub struct Trash {
    dir: PathBuf,
    /// The copies it makes are marked as an API call's.
    api: bool,
}

/// One discard, as kept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    /// Its folder's name: they sort by time.
    pub id: String,
    /// The repository's root.
    pub root: PathBuf,
    /// Unix seconds.
    pub time: u64,
    /// What was done: "Discarded src/x.rs".
    pub what: String,
    /// Each path, relative to the root, and whether it existed before the discard.
    pub files: Vec<(String, bool)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Restored {
    /// Put back; the discard left the trash.
    Done,
    /// These files changed since the discard, so nothing was written: `force` overwrites them.
    Changed(Vec<String>),
}

/// How many files a discard of `paths` keeps, and their size.
pub fn measure(root: &Path, paths: &[String]) -> (usize, u64) {
    let bytes = paths
        .iter()
        .filter_map(|p| fs::symlink_metadata(root.join(p)).ok())
        .map(|m| m.len())
        .sum();
    (paths.len(), bytes)
}

/// A discard this size gets a copy.
pub fn fits(files: usize, bytes: u64) -> bool {
    files <= MAX_FILES && bytes <= MAX_BYTES
}

/// A file's state, to tell later whether it changed: its size and a hash of its bytes (a
/// link's own target, not what it points to), or `-` when it doesn't exist. Not its
/// modification time: a coarse clock misses an edit of the same size.
// ponytail: DefaultHasher may change with a Rust upgrade; a stamp from before then reads as a
// change, so the restore asks (and a forced one keeps a copy). A fixed hash if that bites.
fn stamp(path: &Path) -> String {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return "-".to_string();
    };
    let mut h = DefaultHasher::new();
    let read = if meta.file_type().is_symlink() {
        fs::read_link(path).map(|t| t.hash(&mut h))
    } else if meta.is_file() {
        hash_file(path, meta.len(), &mut h)
    } else {
        // A device or a pipe: reading it may never end.
        Err(io::ErrorKind::InvalidInput.into())
    };
    match read {
        Ok(()) => format!("{}:{:016x}", meta.len(), h.finish()),
        // Unreadable: unlike any hash taken while it could be read.
        Err(_) => format!("?{}", meta.len()),
    }
}

/// Hashes the file's bytes in chunks, as `Vec<u8>::hash` would the whole of them (its length
/// first, then the bytes), so a file of any size takes no more memory than a chunk.
fn hash_file(path: &Path, len: u64, h: &mut DefaultHasher) -> io::Result<()> {
    let mut file = fs::File::open(path)?;
    let mut chunk = [0u8; 64 * 1024];
    h.write_usize(len as usize);
    loop {
        match file.read(&mut chunk)? {
            0 => return Ok(()),
            n => h.write(&chunk[..n]),
        }
    }
}

/// What `Trash::restore` fails with when the copy of what it would write over couldn't be made:
/// nothing was written yet (`kept_nothing`).
#[derive(Debug)]
struct KeepFailed(io::Error);

impl fmt::Display for KeepFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for KeepFailed {}

fn keep_failed(e: io::Error) -> io::Error {
    io::Error::new(e.kind(), KeepFailed(e))
}

/// Whether `restore` failed for want of the copy of what it would write over (nothing was
/// written), not while writing the files back.
pub fn kept_nothing(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.is::<KeepFailed>())
}

impl Trash {
    pub fn new(dir: PathBuf) -> Trash {
        Trash { dir, api: false }
    }

    /// This trash for a call of the API: its copies are marked, and pruning keeps them apart
    /// from the person's own.
    pub fn for_api(&self) -> Trash {
        Trash {
            api: true,
            ..self.clone()
        }
    }

    /// Copies `paths` (relative to `root`) as they are now, before a discard. On a failure the
    /// half-made entry is removed: the caller must not discard then. For the API, with KEEP
    /// copies of its own younger than MAX_AGE already there for `root`, nothing is copied.
    pub fn put(
        &self,
        root: &Path,
        paths: &[String],
        what: &str,
        now: SystemTime,
    ) -> io::Result<Kept> {
        if self.api && self.api_copies(root, now) >= KEEP {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                format!(
                    "the trash already holds {KEEP} copies made through the API for {}: restore or clear them on that computer",
                    root.display()
                ),
            ));
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let since = now.duration_since(UNIX_EPOCH).unwrap_or_default();
        let id = format!(
            "{:013}-{}-{}",
            since.as_millis(),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let entry = self.dir.join(&id);
        let kept = Kept {
            id,
            root: root.to_path_buf(),
            time: since.as_secs(),
            what: what.to_string(),
            files: Vec::new(),
        };
        self.fill(&entry, kept, paths).inspect_err(|_| {
            let _ = fs::remove_dir_all(&entry);
        })
    }

    /// How many copies of API calls `root` has in the trash that aren't older than MAX_AGE.
    fn api_copies(&self, root: &Path, now: SystemTime) -> usize {
        let now = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        self.list()
            .iter()
            .filter(|k| k.root == root && now.saturating_sub(k.time) <= MAX_AGE.as_secs())
            .filter(|k| self.dir.join(&k.id).join(API_MARK).exists())
            .count()
    }

    fn fill(&self, entry: &Path, mut kept: Kept, paths: &[String]) -> io::Result<Kept> {
        let files = entry.join("files");
        fs::create_dir_all(&files)?;
        if self.api {
            fs::write(entry.join(API_MARK), "")?;
        }
        for (i, path) in paths.iter().enumerate() {
            let from = kept.root.join(path);
            let meta = fs::symlink_metadata(&from).ok();
            match &meta {
                Some(m) if m.file_type().is_symlink() => {
                    let target = fs::read_link(&from)?;
                    fs::write(
                        files.join(format!("{i}.link")),
                        target.to_string_lossy().as_bytes(),
                    )?;
                }
                Some(_) => {
                    fs::copy(&from, files.join(i.to_string()))?;
                }
                None => {}
            }
            kept.files.push((path.clone(), meta.is_some()));
        }
        let mut manifest = format!(
            "{}\0{}\0{}\0",
            kept.root.to_string_lossy(),
            kept.time,
            kept.what
        );
        for (path, existed) in &kept.files {
            manifest.push_str(&format!("{path}\0{}\0", u8::from(*existed)));
        }
        fs::write(entry.join("manifest"), manifest)?;
        Ok(kept)
    }

    /// After the discard: each file's state then, so a restore can tell one changed since.
    pub fn seal(&self, kept: &Kept) -> io::Result<()> {
        let after: String = kept
            .files
            .iter()
            .map(|(path, _)| stamp(&kept.root.join(path)) + "\0")
            .collect();
        fs::write(self.dir.join(&kept.id).join("after"), after)
    }

    /// Every discard kept, newest first. One that can't be read is left out.
    pub fn list(&self) -> Vec<Kept> {
        let mut all: Vec<Kept> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| read_manifest(&e.path(), e.file_name().to_string_lossy().into_owned()))
            .collect();
        all.sort_by(|a, b| b.id.cmp(&a.id));
        all
    }

    /// Puts discard `id` back. Unless `force`, a file changed since the discard stops it,
    /// nothing written, and is named. Done, the discard leaves the trash.
    pub fn restore(&self, id: &str, force: bool) -> io::Result<Restored> {
        let entry = self.entry(id)?;
        let kept = read_manifest(&entry, id.to_string()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "this discard is no longer in the trash",
            )
        })?;
        let changed = changed_since(&entry, &kept);
        if !force && !changed.is_empty() {
            return Ok(Restored::Changed(changed));
        }
        // Forced: what it writes over goes to the trash first, so it can be put back too.
        let over = if changed.is_empty() {
            None
        } else {
            // The API's copies are bounded in bytes too: every other copy goes through `fits`.
            let (files, bytes) = measure(&kept.root, &changed);
            if self.api && !fits(files, bytes) {
                return Err(keep_failed(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "too large to keep a copy of what it would write over: restore it on that computer",
                )));
            }
            Some(
                self.put(
                    &kept.root,
                    &changed,
                    "Overwritten by a restore",
                    SystemTime::now(),
                )
                .map_err(keep_failed)?,
            )
        };
        let files = entry.join("files");
        for (i, (path, existed)) in kept.files.iter().enumerate() {
            let to = kept.root.join(path);
            let link = files.join(format!("{i}.link"));
            let now = fs::symlink_metadata(&to).ok();
            let is_link = now.as_ref().is_some_and(|m| m.file_type().is_symlink());
            // A link is replaced, never written through; a file that didn't exist goes.
            if now.is_some() && (!existed || link.exists() || is_link) {
                fs::remove_file(&to)?;
            }
            if !existed {
                continue;
            }
            if let Some(dir) = to.parent() {
                fs::create_dir_all(dir)?;
            }
            if link.exists() {
                restore_link(&fs::read_to_string(&link)?, &to)?;
            } else {
                fs::copy(files.join(i.to_string()), &to)?;
            }
        }
        if let Some(over) = &over {
            // Unsealed, restoring it asks first (`changed_since`): nothing is lost.
            let _ = self.seal(over);
        }
        fs::remove_dir_all(&entry)?;
        Ok(Restored::Done)
    }

    pub fn remove(&self, id: &str) -> io::Result<()> {
        fs::remove_dir_all(self.entry(id)?)
    }

    /// The folder of discard `id`: an id is one name in the trash, as `list` gives them. One that
    /// is a path (`../x`, `a/b`, an absolute one) names nothing here, wherever it came from.
    fn entry(&self, id: &str) -> io::Result<PathBuf> {
        let mut parts = Path::new(id).components();
        match (parts.next(), parts.next()) {
            (Some(Component::Normal(name)), None) if name == id => Ok(self.dir.join(id)),
            _ => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "this discard is no longer in the trash",
            )),
        }
    }

    /// Deletes the discards older than MAX_AGE and those past the newest KEEP of their
    /// repository, the API's apart: those only by age; how many.
    pub fn prune(&self, now: SystemTime) -> io::Result<usize> {
        let now = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let mut removed = 0;
        let mut seen: HashMap<PathBuf, usize> = HashMap::new();
        for kept in self.list() {
            let mut past = false;
            if !self.dir.join(&kept.id).join(API_MARK).exists() {
                let n = seen.entry(kept.root.clone()).or_default();
                *n += 1;
                past = *n > KEEP;
            }
            if past || now.saturating_sub(kept.time) > MAX_AGE.as_secs() {
                self.remove(&kept.id)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// The files of `kept` that changed since its discard: their stamp now isn't the one `seal`
/// wrote. Without `after` (a crash between the copy and the discard, a failed seal), every one
/// there now counts as changed, so the restore asks rather than writes over it.
fn changed_since(entry: &Path, kept: &Kept) -> Vec<String> {
    let after = fs::read_to_string(entry.join("after")).ok();
    let mut then = after.as_deref().map(|a| a.split('\0'));
    kept.files
        .iter()
        .filter(|(path, _)| {
            let now = kept.root.join(path);
            match then.as_mut().and_then(|t| t.next()) {
                Some(then) => stamp(&now) != then,
                None => fs::symlink_metadata(&now).is_ok(),
            }
        })
        .map(|(path, _)| path.clone())
        .collect()
}

fn read_manifest(entry: &Path, id: String) -> Option<Kept> {
    let text = fs::read_to_string(entry.join("manifest")).ok()?;
    let mut f = text.split('\0');
    let (root, time, what) = (f.next()?, f.next()?.parse().ok()?, f.next()?);
    let mut files = Vec::new();
    while let (Some(path), Some(existed)) = (f.next(), f.next()) {
        files.push((path.to_string(), existed == "1"));
    }
    Some(Kept {
        id,
        root: PathBuf::from(root),
        time,
        what: what.to_string(),
        files,
    })
}

#[cfg(unix)]
fn restore_link(target: &str, to: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, to)
}

/// Without symlinks (git's core.symlinks=false on Windows) a link is a file holding its target.
#[cfg(not(unix))]
fn restore_link(target: &str, to: &Path) -> io::Result<()> {
    fs::write(to, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, tempfile::TempDir, Trash) {
        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let trash = Trash::new(data.path().join("trash"));
        (root, data, trash)
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn put_keeps_the_files_and_that_one_didnt_exist_and_restore_puts_them_back() {
        let (root, _data, trash) = setup();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/x.rs"), "mine\n").unwrap();
        let paths = vec!["src/x.rs".to_string(), "gone.txt".to_string()];
        let kept = trash
            .put(root.path(), &paths, "Discarded 2 files", at(1_800_000_000))
            .unwrap();
        assert_eq!(
            kept.files,
            [
                ("src/x.rs".to_string(), true),
                ("gone.txt".to_string(), false)
            ]
        );
        // The discard: x.rs back to the committed text, gone.txt (an unstaged deletion) back.
        std::fs::write(root.path().join("src/x.rs"), "committed\n").unwrap();
        std::fs::write(root.path().join("gone.txt"), "brought back by git\n").unwrap();
        trash.seal(&kept).unwrap();
        let listed = trash.list();
        assert_eq!(listed, std::slice::from_ref(&kept));
        assert_eq!(listed[0].what, "Discarded 2 files");
        assert_eq!(listed[0].root, root.path());
        assert_eq!(listed[0].time, 1_800_000_000);
        assert_eq!(trash.restore(&kept.id, false).unwrap(), Restored::Done);
        assert_eq!(
            std::fs::read_to_string(root.path().join("src/x.rs")).unwrap(),
            "mine\n"
        );
        assert!(
            !root.path().join("gone.txt").exists(),
            "it didn't exist before the discard"
        );
        assert!(
            trash.list().is_empty(),
            "a restored discard leaves the trash"
        );
    }

    #[test]
    fn a_file_changed_since_the_discard_is_named_and_left_alone_unless_forced() {
        let (root, _data, trash) = setup();
        std::fs::write(root.path().join("a.txt"), "mine\n").unwrap();
        let kept = trash
            .put(
                root.path(),
                &["a.txt".to_string()],
                "Discarded a.txt",
                SystemTime::now(),
            )
            .unwrap();
        std::fs::write(root.path().join("a.txt"), "committed\n").unwrap();
        trash.seal(&kept).unwrap();
        // Another length: a fast disk can keep the same modification time.
        std::fs::write(root.path().join("a.txt"), "edited since then\n").unwrap();
        assert_eq!(
            trash.restore(&kept.id, false).unwrap(),
            Restored::Changed(vec!["a.txt".to_string()])
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.txt")).unwrap(),
            "edited since then\n",
            "nothing written"
        );
        assert_eq!(trash.restore(&kept.id, true).unwrap(), Restored::Done);
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.txt")).unwrap(),
            "mine\n"
        );
    }

    #[test]
    fn pruning_drops_discards_older_than_a_week_and_past_the_newest_fifty() {
        let (root, _data, trash) = setup();
        std::fs::write(root.path().join("a.txt"), "x").unwrap();
        let now = 1_800_000_000;
        let a = ["a.txt".to_string()];
        let old = trash
            .put(root.path(), &a, "old", at(now - 8 * 24 * 3600))
            .unwrap();
        for i in 0..KEEP + 2 {
            trash
                .put(root.path(), &a, &format!("n{i}"), at(now - 100 + i as u64))
                .unwrap();
        }
        assert_eq!(
            trash.prune(at(now)).unwrap(),
            3,
            "the old one, and the two oldest past fifty"
        );
        let left = trash.list();
        assert_eq!(left.len(), KEEP);
        assert!(left.iter().all(|k| k.id != old.id));
        assert_eq!(left[0].what, format!("n{}", KEEP + 1), "newest first");
    }

    /// A token with Control can make copies without end (`resolve` of a conflicted file): they
    /// push out neither another repository's discards, nor those a person made, nor each other
    /// (an earlier copy may hold the person's work): at fifty, the next is refused.
    #[test]
    fn a_calls_copies_are_never_pushed_out_and_stop_at_fifty() {
        let (root, _data, trash) = setup();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.txt"), "x").unwrap();
        std::fs::write(other.path().join("a.txt"), "x").unwrap();
        let a = ["a.txt".to_string()];
        let now = 1_800_000_000;
        let theirs = trash.put(other.path(), &a, "other", at(now - 10)).unwrap();
        let mine = trash.put(root.path(), &a, "mine", at(now - 9)).unwrap();
        let api = trash.for_api();
        let first = api.put(root.path(), &a, "api0", at(now - 8)).unwrap();
        for i in 1..KEEP {
            api.put(root.path(), &a, &format!("api{i}"), at(now - 8 + i as u64))
                .unwrap();
            api.prune(at(now)).unwrap();
        }
        let refused = api.put(root.path(), &a, "one more", at(now)).unwrap_err();
        assert!(refused.to_string().contains("already holds 50 copies"));
        // Refused, not half-made: nothing was added.
        let left = trash.list();
        assert_eq!(left.len(), KEEP + 2);
        for kept in [&theirs, &mine, &first] {
            assert!(left.iter().any(|k| k.id == kept.id), "{}", kept.what);
        }
        // Another repository's calls, and the person's own, still get their copies.
        api.put(other.path(), &a, "other's", at(now)).unwrap();
        trash.put(root.path(), &a, "mine again", at(now)).unwrap();
        // A week later the old copies no longer count.
        api.put(root.path(), &a, "later", at(now + 8 * 24 * 3600))
            .unwrap();
    }

    /// The stamp hashes in chunks what `Vec<u8>::hash` hashed whole: one sealed by an earlier
    /// version still reads as unchanged.
    #[test]
    fn a_stamp_hashed_in_chunks_is_the_one_taken_whole() {
        let (root, _data, _trash) = setup();
        let bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(root.path().join("big"), &bytes).unwrap();
        let mut h = DefaultHasher::new();
        bytes.hash(&mut h);
        assert_eq!(
            stamp(&root.path().join("big")),
            format!("{}:{:016x}", bytes.len(), h.finish())
        );
        let before = stamp(&root.path().join("big"));
        let mut other = bytes;
        other[150_000] ^= 1;
        std::fs::write(root.path().join("big"), &other).unwrap();
        assert_ne!(
            stamp(&root.path().join("big")),
            before,
            "one byte, same size"
        );
    }

    #[test]
    fn measure_and_the_cap() {
        let (root, _data, _trash) = setup();
        std::fs::write(root.path().join("a"), "12345").unwrap();
        assert_eq!(
            measure(root.path(), &["a".to_string(), "missing".to_string()]),
            (2, 5)
        );
        assert!(fits(MAX_FILES, MAX_BYTES));
        assert!(!fits(MAX_FILES + 1, 0));
        assert!(!fits(1, MAX_BYTES + 1));
    }

    #[test]
    fn an_id_that_is_a_path_names_no_discard() {
        let (root, data, trash) = setup();
        let other = Trash::new(data.path().join("other"));
        let kept = discarded(root.path(), &other);
        std::fs::write(root.path().join("a.txt"), "changed\n").unwrap();
        // Another trash beside this one holds a real entry: a path to it is not an id of this.
        for id in [
            format!("../other/{}", kept.id),
            other.dir.join(&kept.id).display().to_string(),
            ".".to_string(),
            "..".to_string(),
        ] {
            assert!(trash.restore(&id, true).is_err(), "{id}");
            assert!(trash.remove(&id).is_err(), "{id}");
        }
        assert_eq!(other.list().len(), 1, "nothing was removed");
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.txt")).unwrap(),
            "changed\n",
            "nothing was written"
        );
    }

    #[test]
    fn a_discard_that_cant_be_kept_leaves_nothing_behind() {
        let (root, _data, trash) = setup();
        std::fs::create_dir(root.path().join("dir")).unwrap();
        // A folder isn't a file git lists: copying it fails, and the half-made entry goes.
        assert!(
            trash
                .put(root.path(), &["dir".to_string()], "x", SystemTime::now())
                .is_err()
        );
        assert!(trash.list().is_empty());
        assert!(trash.restore("no-such-entry", false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_comes_back_as_a_symlink() {
        let (root, _data, trash) = setup();
        std::os::unix::fs::symlink("target.txt", root.path().join("link")).unwrap();
        let kept = trash
            .put(
                root.path(),
                &["link".to_string()],
                "Discarded link",
                SystemTime::now(),
            )
            .unwrap();
        assert_eq!(
            kept.files,
            [("link".to_string(), true)],
            "a dangling link exists"
        );
        std::fs::remove_file(root.path().join("link")).unwrap();
        trash.seal(&kept).unwrap();
        assert_eq!(trash.restore(&kept.id, false).unwrap(), Restored::Done);
        assert_eq!(
            std::fs::read_link(root.path().join("link")).unwrap(),
            Path::new("target.txt")
        );
    }

    /// a.txt kept as "mine\n", then discarded to "committed\n" and sealed.
    fn discarded(root: &Path, trash: &Trash) -> Kept {
        std::fs::write(root.join("a.txt"), "mine\n").unwrap();
        let kept = trash
            .put(
                root,
                &["a.txt".to_string()],
                "Discarded a.txt",
                SystemTime::now(),
            )
            .unwrap();
        std::fs::write(root.join("a.txt"), "committed\n").unwrap();
        trash.seal(&kept).unwrap();
        kept
    }

    #[test]
    fn a_forced_restore_keeps_a_copy_of_what_it_writes_over() {
        let (root, _data, trash) = setup();
        let kept = discarded(root.path(), &trash);
        std::fs::write(root.path().join("a.txt"), "edited since then\n").unwrap();
        assert_eq!(trash.restore(&kept.id, true).unwrap(), Restored::Done);
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.txt")).unwrap(),
            "mine\n"
        );
        let left = trash.list();
        assert_eq!(left.len(), 1, "the edit, kept");
        assert_eq!(left[0].what, "Overwritten by a restore");
        assert_eq!(trash.restore(&left[0].id, false).unwrap(), Restored::Done);
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.txt")).unwrap(),
            "edited since then\n"
        );
    }

    #[test]
    fn without_a_seal_a_restore_asks() {
        let (root, _data, trash) = setup();
        std::fs::write(root.path().join("a.txt"), "mine\n").unwrap();
        // A crash between the copy and the discard: no `after`.
        let kept = trash
            .put(
                root.path(),
                &["a.txt".to_string()],
                "Discarded a.txt",
                SystemTime::now(),
            )
            .unwrap();
        std::fs::write(root.path().join("a.txt"), "edited since then\n").unwrap();
        assert_eq!(
            trash.restore(&kept.id, false).unwrap(),
            Restored::Changed(vec!["a.txt".to_string()])
        );
    }

    #[test]
    fn an_edit_of_the_same_size_and_time_is_still_a_change() {
        let (root, _data, trash) = setup();
        let kept = discarded(root.path(), &trash);
        let a = root.path().join("a.txt");
        let sealed = std::fs::metadata(&a).unwrap().modified().unwrap();
        // A coarse clock: same length, same modification time, other bytes.
        std::fs::write(&a, "COMMITTED\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&a)
            .unwrap()
            .set_modified(sealed)
            .unwrap();
        assert_eq!(
            trash.restore(&kept.id, false).unwrap(),
            Restored::Changed(vec!["a.txt".to_string()])
        );
    }

    // Hashing /dev/zero, a device and not a file, never ended.
    #[cfg(unix)]
    #[test]
    fn a_device_is_stamped_unreadable_not_read() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(stamp(Path::new("/dev/zero")));
        });
        let stamp = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("stamp read a device and never ended");
        assert_eq!(stamp, "?0");
    }
}
