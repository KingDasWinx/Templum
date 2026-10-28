//! A kanban card's files: copied into `<data>/attachments/<card id>/`, named, measured, and the sweep
//! that deletes what no card holds (see docs/specs/kanban.md, "Attachments"). Pure functions of the disk.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::board::{Attachment, Boards};

/// The biggest file a card takes, and the biggest image drawn as a thumbnail.
pub(crate) const MAX: u64 = 50 * 1024 * 1024;
pub(crate) const THUMB_MAX: u64 = 10 * 1024 * 1024;

/// Kinds of file drawn as a thumbnail (GPUI decodes these).
const IMAGES: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// A name that is one file name: not empty, `.` or `..`, with no separator or NUL.
pub(crate) fn plain_name(name: &str) -> bool {
    // One normal component: on Windows `C:x` would make `join` leave the folder.
    let mut parts = Path::new(name).components();
    !name.contains(['/', '\\', '\0'])
        && matches!(parts.next(), Some(Component::Normal(_)))
        && parts.next().is_none()
}

/// Where card `card`'s files are.
pub(crate) fn dir(data: &Path, card: u64) -> PathBuf {
    data.join("attachments").join(card.to_string())
}

/// Copies `from` into the card's folder under its own name (a free one when taken). Why it
/// can't, in a few words, when it can't.
pub(crate) fn copy_in(data: &Path, card: u64, from: &Path) -> Result<Attachment, String> {
    let meta = fs::metadata(from).map_err(|e| format!("couldn't read it: {e}"))?;
    if meta.is_dir() {
        return Err("a folder".into());
    }
    if meta.len() > MAX {
        return Err("over 50 MB".into());
    }
    let name = from
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| plain_name(n))
        .ok_or("couldn't read it: it has no name")?;
    let mut source = File::open(from).map_err(|e| format!("couldn't read it: {e}"))?;
    create(data, card, &name, |out| {
        // One byte past the limit tells a file that grew since its size was read.
        let copied = io::copy(&mut (&mut source).take(MAX + 1), out)?;
        if copied > MAX {
            return Ok(Err("over 50 MB".to_string()));
        }
        Ok(Ok(copied))
    })
}

/// Writes `bytes` (a pasted image) into the card's folder as `name`, or a free name beside it.
pub(crate) fn write_in(
    data: &Path,
    card: u64,
    name: &str,
    bytes: &[u8],
) -> Result<Attachment, String> {
    if !plain_name(name) {
        return Err(format!("{name:?} is not a file name"));
    }
    if bytes.len() as u64 > MAX {
        return Err("over 50 MB".into());
    }
    create(data, card, name, |out| {
        out.write_all(bytes)?;
        Ok(Ok(bytes.len() as u64))
    })
}

/// Takes a free name for `name` in the card's folder (`create_new`: two copies at once never
/// share one), lets `fill` write it and removes it when that fails.
fn create(
    data: &Path,
    card: u64,
    name: &str,
    fill: impl FnOnce(&mut File) -> io::Result<Result<u64, String>>,
) -> Result<Attachment, String> {
    let folder = dir(data, card);
    fs::create_dir_all(&folder).map_err(|e| format!("couldn't keep it: {e}"))?;
    let (stem, ext) = {
        let path = Path::new(name);
        let stem = path
            .file_stem()
            .map_or(name, |s| s.to_str().unwrap_or(name));
        let ext = path.extension().and_then(|e| e.to_str());
        (stem.to_string(), ext.map(str::to_string))
    };
    for n in 1..10_000 {
        let taken = match (n, &ext) {
            (1, _) => name.to_string(),
            (n, Some(ext)) => format!("{stem} ({n}).{ext}"),
            (n, None) => format!("{stem} ({n})"),
        };
        let path = folder.join(&taken);
        let mut out = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(out) => out,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("couldn't keep it: {e}")),
        };
        let done = fill(&mut out).and_then(|r| out.flush().map(|_| r));
        return match done {
            Ok(Ok(size)) => Ok(Attachment { name: taken, size }),
            failed => {
                drop(out);
                fs::remove_file(&path).ok();
                Err(match failed {
                    Ok(Err(why)) => why,
                    Err(e) => format!("couldn't keep it: {e}"),
                    Ok(Ok(_)) => unreachable!(),
                })
            }
        };
    }
    Err("couldn't find a free name".into())
}

/// A size as the card shows it: bytes, KB or MB, a KB being 1024 bytes.
pub(crate) fn size_text(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * KB;
    match bytes {
        b if b < KB => format!("{b} B"),
        b if b < MB => format!("{} KB", b / KB),
        b => format!("{:.1} MB", b as f64 / MB as f64),
    }
}

/// Whether the card draws it as a thumbnail: an image kind (in any case) of `THUMB_MAX` or less.
pub(crate) fn thumbnail(a: &Attachment) -> bool {
    a.size <= THUMB_MAX
        && Path::new(&a.name)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| IMAGES.iter().any(|i| i.eq_ignore_ascii_case(e)))
}

/// Deletes in `<data>/attachments/` every entry no card and no kept draft names: the folders of
/// cards that are gone, the files a card no longer lists, a copy a crash cut short. A link is
/// removed as a link, never followed. Says how many entries it removed; no folder is 0. It runs
/// once, at start, on boards read cleanly: until then Ctrl+Z can bring any of them back.
pub(crate) fn sweep(data: &Path, boards: &Boards) -> io::Result<usize> {
    let root = data.join("attachments");
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let mut named: HashMap<u64, HashSet<&str>> = HashMap::new();
    let on_boards = boards
        .boards
        .iter()
        .flat_map(|b| &b.columns)
        .flat_map(|c| &c.cards);
    let in_drafts = boards.drafts.iter().flat_map(|d| [&d.base, &d.work]);
    for card in on_boards.chain(in_drafts) {
        let files = named.entry(card.id).or_default();
        files.extend(card.attachments.iter().map(|a| a.name.as_str()));
    }
    // A folder that can't be read or an entry that won't go stops nothing: the first error is
    // told at the end.
    let (mut removed, mut failed) = (0, None);
    for entry in entries {
        let Some(entry) = noted(entry, &mut failed) else {
            continue;
        };
        let path = entry.path();
        let listed = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse().ok())
            .and_then(|id| named.get(&id))
            .filter(|_| entry.file_type().is_ok_and(|t| t.is_dir()));
        let Some(listed) = listed else {
            removed += noted(remove_any(&path), &mut failed).map_or(0, |()| 1);
            continue;
        };
        let Some(files) = noted(fs::read_dir(&path), &mut failed) else {
            continue;
        };
        for file in files {
            let Some(file) = noted(file, &mut failed) else {
                continue;
            };
            if !file
                .file_name()
                .to_str()
                .is_some_and(|n| listed.contains(n))
            {
                removed += noted(remove_any(&file.path()), &mut failed).map_or(0, |()| 1);
            }
        }
        if listed.is_empty() {
            fs::remove_dir(&path).ok();
        }
    }
    failed.map_or(Ok(removed), Err)
}

/// `result`'s value, or none after keeping its error in `failed` unless one is kept already.
fn noted<T>(result: io::Result<T>, failed: &mut Option<io::Error>) -> Option<T> {
    result
        .map_err(|e| {
            failed.get_or_insert(e);
        })
        .ok()
}

/// `boards.json` was set aside as `boards.json.bak`: the attachments go aside with it, as
/// `attachments.bak` (replacing an older one), so the pair restores together and the session
/// starts with no files that its fresh boards would sweep, or reuse the folders of. True when a
/// folder was moved. The files move to a temporary name first and the older `.bak` goes only
/// after that worked (a file open in another app can make the rename fail on Windows): on any
/// failure the files are back where they were and the older `.bak` is as it was or gone no
/// further than that step.
pub(crate) fn set_aside(data: &Path) -> io::Result<bool> {
    let from = data.join("attachments");
    let (moving, to) = (
        data.join("attachments.moving"),
        data.join("attachments.bak"),
    );
    if fs::symlink_metadata(&from).is_err() {
        return Ok(false);
    }
    // A leftover of an attempt that was cut short is ours to clear.
    if fs::symlink_metadata(&moving).is_ok() {
        remove_any(&moving)?;
    }
    fs::rename(&from, &moving)?;
    let placed = (|| {
        if fs::symlink_metadata(&to).is_ok() {
            remove_any(&to)?;
        }
        fs::rename(&moving, &to)
    })();
    if let Err(e) = placed {
        fs::rename(&moving, &from).ok();
        return Err(e);
    }
    Ok(true)
}

/// Removes a file, a folder with what is in it, or a link (not what it points to).
fn remove_any(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Boards, Card, Draft};

    fn file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_copy_keeps_the_name_and_takes_a_free_one_when_taken() {
        let (src, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let first = file(src.path(), "x.png", b"one");
        let make = file(src.path(), "Makefile", b"all:");
        let a = copy_in(data.path(), 5, &first).unwrap();
        std::fs::write(&first, b"two").unwrap();
        let b = copy_in(data.path(), 5, &first).unwrap();
        assert_eq!((a.name.as_str(), a.size), ("x.png", 3));
        assert_eq!(b.name, "x (2).png");
        let c = copy_in(data.path(), 5, &first).unwrap();
        assert_eq!(c.name, "x (3).png");
        let (m1, m2) = (
            copy_in(data.path(), 5, &make).unwrap(),
            copy_in(data.path(), 5, &make).unwrap(),
        );
        assert_eq!(
            (m1.name.as_str(), m2.name.as_str()),
            ("Makefile", "Makefile (2)")
        );
        let folder = dir(data.path(), 5);
        assert_eq!(std::fs::read(folder.join("x.png")).unwrap(), b"one");
        assert_eq!(std::fs::read(folder.join("x (2).png")).unwrap(), b"two");
        assert_eq!(std::fs::read(folder.join("Makefile (2)")).unwrap(), b"all:");
        // Another card's folder is its own: the name is free there.
        assert_eq!(copy_in(data.path(), 6, &first).unwrap().name, "x.png");
    }

    #[test]
    fn a_file_over_50_mb_and_a_folder_are_refused_and_leave_nothing() {
        let (src, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let big = src.path().join("video.mov");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(MAX + 1)
            .unwrap();
        assert_eq!(copy_in(data.path(), 5, &big), Err("over 50 MB".to_string()));
        let sub = src.path().join("build");
        std::fs::create_dir(&sub).unwrap();
        assert_eq!(copy_in(data.path(), 5, &sub), Err("a folder".to_string()));
        let gone = src.path().join("gone.txt");
        let why = copy_in(data.path(), 5, &gone).unwrap_err();
        assert!(why.starts_with("couldn't read it"), "{why}");
        let left = std::fs::read_dir(dir(data.path(), 5))
            .map(|d| d.count())
            .unwrap_or(0);
        assert_eq!(left, 0, "no file stays behind");
        // The limit itself is allowed.
        let edge = src.path().join("edge.bin");
        std::fs::File::create(&edge).unwrap().set_len(MAX).unwrap();
        assert_eq!(copy_in(data.path(), 5, &edge).unwrap().size, MAX);
    }

    #[test]
    fn a_pasted_image_is_written() {
        let data = tempfile::tempdir().unwrap();
        let a = write_in(data.path(), 9, "Pasted image.png", b"png").unwrap();
        let b = write_in(data.path(), 9, "Pasted image.png", b"png2").unwrap();
        assert_eq!((a.name.as_str(), a.size), ("Pasted image.png", 3));
        assert_eq!(b.name, "Pasted image (2).png");
        let folder = dir(data.path(), 9);
        assert_eq!(
            std::fs::read(folder.join("Pasted image.png")).unwrap(),
            b"png"
        );
        assert_eq!(
            std::fs::read(folder.join("Pasted image (2).png")).unwrap(),
            b"png2"
        );
        assert!(
            write_in(data.path(), 9, "../x", b"no").is_err(),
            "not a plain name"
        );
    }

    #[test]
    fn sizes_read_in_bytes_kb_and_mb() {
        assert_eq!(size_text(12), "12 B");
        assert_eq!(size_text(48213), "47 KB");
        assert_eq!(size_text(1258291), "1.2 MB");
        assert_eq!(size_text(1023), "1023 B");
        assert_eq!(size_text(1024), "1 KB");
    }

    #[test]
    fn names_are_plain_or_not() {
        for ok in ["a.png", "Pasted image (2).png", "Makefile", ".env", "a..b"] {
            assert!(plain_name(ok), "{ok}");
        }
        for bad in ["", ".", "..", "a/b", "a\\b", "a\0b", "../x"] {
            assert!(!plain_name(bad), "{bad:?}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_drive_relative_name_is_not_plain() {
        for bad in ["C:x.png", "C:"] {
            assert!(!plain_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn setting_aside_moves_the_folder_beside_the_bak_and_replaces_an_older_one() {
        let data = tempfile::tempdir().unwrap();
        assert!(!set_aside(data.path()).unwrap(), "nothing to set aside");
        let old = dir(data.path(), 1);
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("a.txt"), "a").unwrap();
        assert!(set_aside(data.path()).unwrap());
        let bak = data.path().join("attachments.bak");
        assert_eq!(
            fs::read_to_string(bak.join("1").join("a.txt")).unwrap(),
            "a"
        );
        assert!(!data.path().join("attachments").exists());
        let newer = dir(data.path(), 2);
        fs::create_dir_all(&newer).unwrap();
        fs::write(newer.join("b.txt"), "b").unwrap();
        set_aside(data.path()).unwrap();
        assert!(bak.join("2").join("b.txt").exists());
        assert!(!bak.join("1").exists(), "the older bak is replaced");
    }

    #[cfg(unix)]
    #[test]
    fn a_set_aside_that_cant_move_the_files_keeps_them_and_the_older_bak() {
        use std::os::unix::fs::PermissionsExt as _;
        let data = tempfile::tempdir().unwrap();
        let folder = dir(data.path(), 1);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("new.txt"), "new").unwrap();
        let bak = data.path().join("attachments.bak").join("9");
        fs::create_dir_all(&bak).unwrap();
        fs::write(bak.join("old.txt"), "old").unwrap();
        // A folder nobody may write to can't have a name taken in it: the first rename fails.
        fs::set_permissions(data.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let result = set_aside(data.path());
        fs::set_permissions(data.path(), fs::Permissions::from_mode(0o755)).unwrap();
        if result.is_ok() {
            // Running as root: nothing is refused.
            return;
        }
        assert!(
            folder.join("new.txt").exists(),
            "the files stay where they were"
        );
        assert!(bak.join("old.txt").exists(), "the older bak isn't touched");
    }

    #[cfg(unix)]
    #[test]
    fn the_sweep_goes_on_past_folders_it_cant_read_and_tells_the_error() {
        use std::os::unix::fs::PermissionsExt as _;
        let data = tempfile::tempdir().unwrap();
        let (mut boards, _, first) = boards_with_a_card();
        // A second listed card with a higher id, made last: whichever order the folders are
        // read in (oldest, newest or hashed), a locked one comes before some stray.
        let board = boards.boards[0].id;
        let column = boards.boards[0].columns[0].id;
        let second = boards.add_card(board, column).unwrap();
        boards.edit_card(board, second, |c| {
            c.attachments = vec![Attachment {
                name: "keep.txt".into(),
                size: 1,
            }]
        });
        let lock = |card: u64| {
            let folder = dir(data.path(), card);
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("keep.txt"), "k").unwrap();
            fs::set_permissions(&folder, fs::Permissions::from_mode(0o000)).unwrap();
            folder
        };
        let unlock = |folder: &Path| {
            fs::set_permissions(folder, fs::Permissions::from_mode(0o755)).unwrap();
        };
        let a = lock(first);
        if fs::read_dir(&a).is_ok() {
            // Running as root: nothing is unreadable.
            unlock(&a);
            return;
        }
        let strays: Vec<u64> = (1000..1010).collect();
        for gone in &strays {
            let folder = dir(data.path(), *gone);
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("x"), "x").unwrap();
        }
        let b = lock(second);
        let result = sweep(data.path(), &boards);
        unlock(&a);
        unlock(&b);
        assert!(result.is_err(), "the error is told");
        for gone in strays {
            assert!(!dir(data.path(), gone).exists(), "{gone} was swept anyway");
        }
    }

    #[test]
    fn only_images_up_to_10_mb_get_a_thumbnail() {
        let a = |name: &str, size| Attachment {
            name: name.into(),
            size,
        };
        assert!(thumbnail(&a("shot.PNG", 5)));
        assert!(thumbnail(&a("p.jpeg", THUMB_MAX)));
        assert!(!thumbnail(&a("p.jpeg", THUMB_MAX + 1)));
        assert!(!thumbnail(&a("notes.txt", 5)));
        assert!(!thumbnail(&a("png", 5)), "no extension");
    }
    /// Boards with card 1 listing `keep.txt`, and the ids that holds: (boards, column, card).
    fn boards_with_a_card() -> (Boards, u64, u64) {
        let mut boards = Boards::default();
        let board = boards.add_board();
        let column = boards.board(board).unwrap().columns[0].id;
        let card = boards.add_card(board, column).unwrap();
        boards.edit_card(board, card, |c| {
            c.attachments = vec![Attachment {
                name: "keep.txt".into(),
                size: 1,
            }]
        });
        (boards, column, card)
    }

    fn files_in(data: &Path, card: u64) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir(data, card))
            .map(|d| {
                d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn the_sweep_removes_what_no_card_or_draft_names() {
        let data = tempfile::tempdir().unwrap();
        let (mut boards, column, card) = boards_with_a_card();
        let (gone, new_card) = (card + 100, card + 200);
        for (id, name) in [
            (card, "keep.txt"),
            (card, "old.txt"),
            (card, "cut-short.png"),
            (gone, "a.txt"),
            (gone, "b.txt"),
            (new_card, "typed.png"),
            (card, "late.txt"),
        ] {
            let folder = dir(data.path(), id);
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join(name), name).unwrap();
        }
        // A draft of a new card keeps the files it listed; one of the card keeps what its work
        // lists (a copy that ended late) and what its base lists (Ctrl+Z of a removal).
        let attached = |name: &str| Attachment {
            name: name.into(),
            size: 1,
        };
        boards.keep_draft(Draft {
            at: column,
            new: true,
            base: Card::new(new_card),
            work: Card {
                attachments: vec![attached("typed.png")],
                ..Card::new(new_card)
            },
        });
        let mut base = boards.boards[0].columns[0].cards[0].clone();
        base.attachments.push(attached("old.txt"));
        let mut work = base.clone();
        work.attachments = vec![attached("late.txt")];
        boards.keep_draft(Draft {
            at: card,
            new: false,
            base,
            work,
        });
        // Gone: the deleted card's folder and the copy cut short. Kept: what the drafts name.
        assert_eq!(sweep(data.path(), &boards).unwrap(), 2);
        assert_eq!(
            files_in(data.path(), card),
            ["keep.txt", "late.txt", "old.txt"]
        );
        assert_eq!(files_in(data.path(), new_card), ["typed.png"]);
        assert!(!dir(data.path(), gone).exists());
        // Without the draft's base, `old.txt` goes too.
        boards.drop_draft(card);
        assert_eq!(
            sweep(data.path(), &boards).unwrap(),
            2,
            "old.txt and late.txt"
        );
        assert_eq!(files_in(data.path(), card), ["keep.txt"]);
        // Nothing to sweep, or no folder at all.
        assert_eq!(sweep(data.path(), &boards).unwrap(), 0);
        assert_eq!(sweep(&data.path().join("nope"), &boards).unwrap(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn the_sweep_removes_a_link_as_a_link_and_never_follows_it() {
        let (data, outside) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        fs::write(outside.path().join("precious.txt"), "mine").unwrap();
        let root = data.path().join("attachments");
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("77")).unwrap();
        let (boards, _, card) = boards_with_a_card();
        let folder = dir(data.path(), card);
        fs::create_dir_all(&folder).unwrap();
        std::os::unix::fs::symlink(outside.path(), folder.join("link")).unwrap();
        assert_eq!(sweep(data.path(), &boards).unwrap(), 2);
        assert!(!root.join("77").exists() && !folder.join("link").exists());
        assert_eq!(
            fs::read_to_string(outside.path().join("precious.txt")).unwrap(),
            "mine"
        );
    }
}
