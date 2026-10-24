//! A card's attachments: put on it in the open card, copied into the data folder, and kept with the work
//! of the dialog (docs/specs/kanban.md, "Attachments").

use std::path::{Path, PathBuf};

use super::card_draft::{cards, click_on, dialog_open, empty_board, open_card, press_key};
use super::*;
use crate::kanban::{Source, attach};

/// A workspace with a data folder, an empty board and one card on it:
/// (handle, workspace, data folder, kanban block, board, column, card).
fn with_a_card(
    cx: &mut TestAppContext,
) -> (
    AnyWindowHandle,
    Entity<Workspace>,
    tempfile::TempDir,
    BlockId,
    u64,
    u64,
    u64,
) {
    no_motion(cx);
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let (block, board, todo) = empty_board(handle, &ws, cx);
    let card = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_card(board, todo)).unwrap()
    });
    (handle, ws, data, block, board, todo, card)
}

fn source(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn drawn(handle: AnyWindowHandle, cx: &mut TestAppContext, id: String) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window
            .try_find(gpui_kit::ElementId::Name(id.into()))
            .is_some()
    })
    .unwrap()
}

/// Hovers an attachment, which shows its ✕, and clicks it.
fn remove(handle: AnyWindowHandle, cx: &mut TestAppContext, name: &str) {
    cx.update_window(handle, |_, window, cx| {
        window.hover(
            gpui_kit::ElementId::Name(format!("attachment-{name}").into()),
            cx,
        );
        window.render_frame(cx);
        let cross = format!("attachment-remove-{name}");
        window.click(gpui_kit::ElementId::Name(cross.into()), cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// The attachments' names the board's card holds.
fn names(ws: &Entity<Workspace>, cx: &mut TestAppContext, board: u64) -> Vec<String> {
    cards(ws, cx, board)
        .iter()
        .flat_map(|c| c.attachments.iter().map(|a| a.name.clone()))
        .collect()
}

/// Attaches `from` to the open card, as a drop or a paste would.
fn attach_to_open(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    board: u64,
    from: Vec<Source>,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let open = ws.card_work.clone().expect("a card is open");
            attach(ws, &open, board, from, window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn attach_shows_a_thumbnail_and_a_row_and_done_puts_them_on_the_card(cx: &mut TestAppContext) {
    let (handle, ws, data, block, board, _, card) = with_a_card(cx);
    let src = tempfile::tempdir().unwrap();
    let shot = source(src.path(), "shot.png", b"not really a png");
    let notes = source(src.path(), "notes.txt", b"hello");
    open_card(handle, &ws, cx, board, card);
    click_on(handle, cx, "attach-files");
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files && !options.directories && options.multiple);
        Some(vec![shot, notes])
    });
    cx.run_until_parked();
    assert!(
        drawn(handle, cx, "attachment-shot.png".into()),
        "a thumbnail"
    );
    assert!(drawn(handle, cx, "attachment-notes.txt".into()), "a row");
    assert!(
        !drawn(handle, cx, "attachment-copying-notes.txt".into()),
        "copied"
    );
    assert!(
        names(&ws, cx, board).is_empty(),
        "nothing on the board before Done"
    );
    click_on(handle, cx, "card-done");
    assert!(!dialog_open(handle, cx));
    assert_eq!(names(&ws, cx, board), ["shot.png", "notes.txt"]);
    let folder = crate::attachments::dir(data.path(), card);
    assert_eq!(std::fs::read(folder.join("notes.txt")).unwrap(), b"hello");
    assert!(folder.join("shot.png").exists());
    assert!(
        drawn(handle, cx, format!("attachments-{}-{card}", block.0)),
        "the face's paperclip"
    );
    ws.update(cx, |ws, cx| assert!(ws.undo_boards(cx)));
    assert!(
        names(&ws, cx, board).is_empty(),
        "one Ctrl+Z takes both off"
    );
    assert!(!drawn(
        handle,
        cx,
        format!("attachments-{}-{card}", block.0)
    ));
}

#[gpui_kit::test]
fn removing_an_attachment_changes_the_board_only_at_done(cx: &mut TestAppContext) {
    let (handle, ws, data, _, board, _, card) = with_a_card(cx);
    let src = tempfile::tempdir().unwrap();
    let kept = crate::attachments::copy_in(data.path(), card, &source(src.path(), "a.txt", b"a"));
    let gone = crate::attachments::copy_in(data.path(), card, &source(src.path(), "b.txt", b"b"));
    ws.update(cx, |ws, _| {
        ws.edit_boards(|b| {
            b.edit_card(board, card, |c| {
                c.attachments = vec![kept.clone().unwrap(), gone.clone().unwrap()]
            })
        })
    });
    open_card(handle, &ws, cx, board, card);
    remove(handle, cx, "b.txt");
    assert!(
        !drawn(handle, cx, "attachment-b.txt".into()),
        "gone from the dialog"
    );
    assert_eq!(
        names(&ws, cx, board),
        ["a.txt", "b.txt"],
        "the board still has it"
    );
    click_on(handle, cx, "card-done");
    assert_eq!(names(&ws, cx, board), ["a.txt"]);
    assert!(
        crate::attachments::dir(data.path(), card)
            .join("b.txt")
            .exists(),
        "its file stays until a start"
    );
    // Esc after a removal asks, as for any change.
    open_card(handle, &ws, cx, board, card);
    remove(handle, cx, "a.txt");
    press_key(handle, cx, "escape");
    assert!(drawn(handle, cx, "leave-discard".into()));
    click_on(handle, cx, "leave-discard");
    assert_eq!(names(&ws, cx, board), ["a.txt"], "discarded");
}

#[gpui_kit::test]
fn a_new_cards_files_go_in_its_reserved_folder(cx: &mut TestAppContext) {
    let (handle, ws, data, block, board, todo, _) = with_a_card(cx);
    let src = tempfile::tempdir().unwrap();
    let file = source(src.path(), "spec.pdf", b"%PDF");
    super::card_draft::click_add(handle, cx, block, todo);
    cx.simulate_input(handle, "With a file");
    attach_to_open(handle, &ws, cx, board, vec![Source::File(file)]);
    cx.run_until_parked();
    // The id taken for it is the last one given out.
    let reserved = ws.read_with(cx, |ws, _| ws.boards.next_id);
    assert_ne!(reserved, 0);
    assert!(
        crate::attachments::dir(data.path(), reserved)
            .join("spec.pdf")
            .exists()
    );
    assert!(
        cards(&ws, cx, board).iter().all(|c| c.id != reserved),
        "no card yet"
    );
    click_on(handle, cx, "card-done");
    let made = cards(&ws, cx, board)
        .into_iter()
        .find(|c| c.title == "With a file")
        .expect("created");
    assert_eq!(
        made.id, reserved,
        "Done gives the card the id its files were put under"
    );
    assert_eq!(made.attachments.len(), 1);
}

#[gpui_kit::test]
fn a_refused_file_notifies_and_attaches_nothing(cx: &mut TestAppContext) {
    let (handle, ws, data, _, board, _, card) = with_a_card(cx);
    let src = tempfile::tempdir().unwrap();
    let folder = src.path().join("build");
    std::fs::create_dir(&folder).unwrap();
    let good = source(src.path(), "ok.txt", b"ok");
    open_card(handle, &ws, cx, board, card);
    let before = notification_count(handle, cx);
    attach_to_open(
        handle,
        &ws,
        cx,
        board,
        vec![Source::File(folder), Source::File(good)],
    );
    cx.run_until_parked();
    assert_eq!(
        notification_count(handle, cx),
        before + 1,
        "one warning for the action"
    );
    assert!(
        drawn(handle, cx, "attachment-ok.txt".into()),
        "the good one is attached"
    );
    assert!(!drawn(handle, cx, "attachment-build".into()));
    assert!(
        !crate::attachments::dir(data.path(), card)
            .join("build")
            .exists()
    );
    click_on(handle, cx, "card-done");
    assert_eq!(names(&ws, cx, board), ["ok.txt"]);
}

#[gpui_kit::test]
fn a_copy_that_ends_late_follows_the_work(cx: &mut TestAppContext) {
    // Done: onto the card, as a step of Ctrl+Z of its own.
    let (handle, ws, data, _, board, _, card) = with_a_card(cx);
    let src = tempfile::tempdir().unwrap();
    open_card(handle, &ws, cx, board, card);
    let late = source(src.path(), "late.txt", b"late");
    attach_to_open(handle, &ws, cx, board, vec![Source::File(late.clone())]);
    assert!(
        drawn(handle, cx, "attachment-copying-late.txt".into()),
        "still copying"
    );
    click_on(handle, cx, "card-done");
    assert!(names(&ws, cx, board).is_empty(), "Done doesn't wait");
    cx.run_until_parked();
    assert_eq!(names(&ws, cx, board), ["late.txt"], "it reached the card");
    ws.update(cx, |ws, cx| assert!(ws.undo_boards(cx)));
    assert!(names(&ws, cx, board).is_empty(), "a step of its own");

    // Save draft: into the kept draft.
    open_card(handle, &ws, cx, board, card);
    attach_to_open(handle, &ws, cx, board, vec![Source::File(late.clone())]);
    press_key(handle, cx, "escape");
    click_on(handle, cx, "leave-save-draft");
    cx.run_until_parked();
    assert!(names(&ws, cx, board).is_empty());
    let kept = ws.read_with(cx, |ws, _| {
        ws.boards.draft(card).map(|d| d.work.attachments.len())
    });
    assert_eq!(kept, Some(1), "in the draft");

    // Discard: nowhere.
    open_card(handle, &ws, cx, board, card);
    let more = source(src.path(), "more.txt", b"more");
    attach_to_open(handle, &ws, cx, board, vec![Source::File(more)]);
    press_key(handle, cx, "escape");
    click_on(handle, cx, "leave-discard");
    cx.run_until_parked();
    assert!(names(&ws, cx, board).is_empty());
    ws.read_with(cx, |ws, _| assert!(ws.boards.draft(card).is_none()));
    assert!(
        crate::attachments::dir(data.path(), card)
            .join("more.txt")
            .exists(),
        "its file waits for the next start"
    );
}

#[gpui_kit::test]
fn without_a_data_folder_there_is_no_attachments_section(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let (_, board, todo) = empty_board(handle, &ws, cx);
    let card = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_card(board, todo)).unwrap()
    });
    open_card(handle, &ws, cx, board, card);
    assert!(dialog_open(handle, cx));
    assert!(!drawn(handle, cx, "attach-files".into()));
    assert!(!drawn(handle, cx, "card-attachments".into()));
    let src = tempfile::tempdir().unwrap();
    let file = source(src.path(), "x.txt", b"x");
    attach_to_open(handle, &ws, cx, board, vec![Source::File(file)]);
    cx.run_until_parked();
    click_on(handle, cx, "card-done");
    assert!(names(&ws, cx, board).is_empty(), "a drop attaches nothing");
}

/// Attaches the file `name` (with `bytes`) to `card`, through the dialog, and presses Done.
fn attach_and_finish(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    board: u64,
    card: u64,
    name: &str,
) {
    let src = tempfile::tempdir().unwrap();
    let file = source(src.path(), name, name.as_bytes());
    open_card(handle, ws, cx, board, card);
    attach_to_open(handle, ws, cx, board, vec![Source::File(file)]);
    cx.run_until_parked();
    click_on(handle, cx, "card-done");
}

/// The boards as the first run left them, for the next start to read.
fn save_boards(ws: &Entity<Workspace>, cx: &mut TestAppContext, data: &Path) {
    ws.read_with(cx, |ws, _| {
        crate::board::save(data, &ws.boards, 1).unwrap();
    });
}

#[gpui_kit::test]
fn a_start_deletes_a_deleted_cards_files(cx: &mut TestAppContext) {
    let (handle, ws, data, _, board, todo, card) = with_a_card(cx);
    let other = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_card(board, todo)).unwrap()
    });
    attach_and_finish(handle, &ws, cx, board, card, "gone.txt");
    attach_and_finish(handle, &ws, cx, board, other, "stays.txt");
    open_card(handle, &ws, cx, board, card);
    click_on(handle, cx, "delete-card");
    save_boards(&ws, cx, data.path());
    let folder = crate::attachments::dir(data.path(), card);
    assert!(
        folder.join("gone.txt").exists(),
        "nothing is deleted in the session"
    );
    // The next start reads the boards and deletes what no card holds.
    let _next = open_in(cx, None, Some(data.path().to_path_buf()));
    assert!(!folder.exists(), "the deleted card's folder is gone");
    let kept = crate::attachments::dir(data.path(), other).join("stays.txt");
    assert!(kept.exists(), "another card's file stays");
}

#[gpui_kit::test]
fn the_sweep_runs_only_on_boards_read_cleanly(cx: &mut TestAppContext) {
    let stray = |data: &Path, card: u64| {
        let folder = crate::attachments::dir(data, card);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("x.txt"), "x").unwrap();
        folder
    };
    // boards.json that can't be read is set aside, and the files go aside with it.
    let data = tempfile::tempdir().unwrap();
    let folder = stray(data.path(), 5);
    std::fs::write(crate::board::path(data.path()), "{ not json").unwrap();
    let _ = open_in(cx, None, Some(data.path().to_path_buf()));
    let bak = data.path().join("attachments.bak").join("5").join("x.txt");
    assert!(
        bak.exists() && !folder.exists(),
        "set aside: nothing is deleted"
    );
    // One that can't be opened at all: the same.
    let data = tempfile::tempdir().unwrap();
    let folder = stray(data.path(), 5);
    std::fs::create_dir(crate::board::path(data.path())).unwrap();
    let _ = open_in(cx, None, Some(data.path().to_path_buf()));
    assert!(
        folder.join("x.txt").exists(),
        "unreadable: nothing is deleted"
    );
    // None yet (the boards missing): the same.
    let data = tempfile::tempdir().unwrap();
    let folder = stray(data.path(), 5);
    let _ = open_in(cx, None, Some(data.path().to_path_buf()));
    assert!(folder.join("x.txt").exists(), "missing: nothing is deleted");
    // Read cleanly: a new card's kept draft keeps its reserved folder, a stray one goes.
    let data = tempfile::tempdir().unwrap();
    let (kept, lost) = (stray(data.path(), 7), stray(data.path(), 8));
    let mut boards = crate::board::Boards::default();
    let board = boards.add_board();
    let column = boards.board(board).unwrap().columns[0].id;
    let typed = crate::board::Card {
        id: 7,
        attachments: vec![crate::board::Attachment {
            name: "x.txt".into(),
            size: 1,
        }],
        ..crate::board::Card::new(7)
    };
    boards.next_id = 100;
    boards.keep_draft(crate::board::Draft {
        at: column,
        new: true,
        base: crate::board::Card::new(7),
        work: typed,
    });
    crate::board::save(data.path(), &boards, 1).unwrap();
    let _ = open_in(cx, None, Some(data.path().to_path_buf()));
    assert!(
        kept.join("x.txt").exists(),
        "a kept new-card draft's files stay"
    );
    assert!(!lost.exists(), "a folder no card names goes");
}

#[gpui_kit::test]
fn ctrl_z_of_a_deleted_card_brings_its_file_back(cx: &mut TestAppContext) {
    let (handle, ws, data, _, board, _, card) = with_a_card(cx);
    attach_and_finish(handle, &ws, cx, board, card, "back.txt");
    open_card(handle, &ws, cx, board, card);
    click_on(handle, cx, "delete-card");
    assert!(cards(&ws, cx, board).is_empty());
    ws.update(cx, |ws, cx| assert!(ws.undo_boards(cx)));
    assert_eq!(
        names(&ws, cx, board),
        ["back.txt"],
        "the card lists it again"
    );
    assert!(
        crate::attachments::dir(data.path(), card)
            .join("back.txt")
            .exists(),
        "and its file is where it was"
    );
}

/// The bytes of a PNG, so a pasted image is a real one.
fn png() -> gpui_kit::Image {
    gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, vec![0x89, b'P', b'N', b'G'])
}

#[gpui_kit::test]
fn pasting_a_screenshot_attaches_it_and_leaves_the_title(cx: &mut TestAppContext) {
    let (handle, ws, data, _, board, _, card) = with_a_card(cx);
    open_card(handle, &ws, cx, board, card);
    cx.write_to_clipboard(gpui_kit::ClipboardItem::from(png()));
    press_key(handle, cx, PASTE);
    cx.run_until_parked();
    assert!(drawn(handle, cx, "attachment-Pasted image.png".into()));
    click_on(handle, cx, "card-done");
    let card = cards(&ws, cx, board).remove(0);
    assert_eq!(card.title, "New card", "the title is as it was");
    assert_eq!(card.attachments.len(), 1);
    let file = crate::attachments::dir(data.path(), card.id).join("Pasted image.png");
    assert_eq!(std::fs::read(file).unwrap(), png().bytes);
}

#[gpui_kit::test]
fn a_paste_with_text_pastes_the_text(cx: &mut TestAppContext) {
    use gpui_kit::{ClipboardEntry, ClipboardItem, ClipboardString};
    let (handle, ws, _, _, board, _, card) = with_a_card(cx);
    open_card(handle, &ws, cx, board, card);
    cx.write_to_clipboard(ClipboardItem {
        entries: vec![
            ClipboardEntry::Image(png()),
            ClipboardEntry::String(ClipboardString::new("copied words".into())),
        ],
    });
    press_key(handle, cx, PASTE);
    cx.run_until_parked();
    assert!(!drawn(handle, cx, "attachment-Pasted image.png".into()));
    assert!(!drawn(
        handle,
        cx,
        "attachment-copying-Pasted image.png".into()
    ));
    click_on(handle, cx, "card-done");
    let card = cards(&ws, cx, board).remove(0);
    assert!(card.title.contains("copied words"), "{:?}", card.title);
    assert!(card.attachments.is_empty());
}

#[gpui_kit::test]
fn dropping_files_on_the_open_card_attaches_them(cx: &mut TestAppContext) {
    use gpui_kit::{ExternalPaths, FileDropEvent};
    let (handle, ws, data, _, board, _, card) = with_a_card(cx);
    let src = tempfile::tempdir().unwrap();
    let file = source(src.path(), "dropped.log", b"log");
    open_card(handle, &ws, cx, board, card);
    let center = cx
        .update_window(handle, |_, window, _| {
            window.find("card-attachments").bounds().center()
        })
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        let paths = ExternalPaths([file.clone()].into_iter().collect());
        let entered = FileDropEvent::Entered {
            position: center,
            paths,
        };
        window.dispatch_event(PlatformInput::FileDrop(entered), cx);
        window.dispatch_event(
            PlatformInput::FileDrop(FileDropEvent::Submit { position: center }),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(drawn(handle, cx, "attachment-dropped.log".into()));
    click_on(handle, cx, "card-done");
    assert_eq!(names(&ws, cx, board), ["dropped.log"]);
    assert!(
        crate::attachments::dir(data.path(), card)
            .join("dropped.log")
            .exists()
    );
}

#[gpui_kit::test]
fn a_set_aside_start_keeps_the_files_beside_the_bak_through_the_next_start(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    let folder = crate::attachments::dir(data.path(), 5);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("x.txt"), "x").unwrap();
    std::fs::write(crate::board::path(data.path()), "{ not json").unwrap();
    // The first start sets boards.json aside; the session saves fresh ones.
    let (_, first) = open_in(cx, None, Some(data.path().to_path_buf()));
    first.update(cx, |ws, cx| ws.save_canvas_now(cx));
    assert!(
        crate::board::path(data.path()).exists(),
        "fresh boards saved"
    );
    // The next start reads them cleanly and sweeps: the old files are not in its way.
    let _ = open_in(cx, None, Some(data.path().to_path_buf()));
    let bak = data.path().join("attachments.bak").join("5").join("x.txt");
    assert_eq!(std::fs::read_to_string(bak).unwrap(), "x");
    assert!(!folder.exists(), "and the session started without them");
}

#[gpui_kit::test]
fn a_copy_that_ends_after_done_goes_onto_the_card_not_into_a_reopened_dialog(
    cx: &mut TestAppContext,
) {
    let (handle, ws, _, _, board, _, card) = with_a_card(cx);
    let src = tempfile::tempdir().unwrap();
    open_card(handle, &ws, cx, board, card);
    let late = source(src.path(), "late.txt", b"late");
    attach_to_open(handle, &ws, cx, board, vec![Source::File(late)]);
    click_on(handle, cx, "card-done");
    open_card(handle, &ws, cx, board, card);
    cx.run_until_parked();
    assert_eq!(names(&ws, cx, board), ["late.txt"], "on the card");
    assert!(
        drawn(handle, cx, "attachment-late.txt".into()),
        "the dialog sees it there"
    );
    press_key(handle, cx, "escape");
    assert!(
        !dialog_open(handle, cx),
        "no change of the person's: Esc closes without asking"
    );
}

#[gpui_kit::test]
fn a_copy_that_ends_after_discard_is_not_taken_by_a_reopened_dialog(cx: &mut TestAppContext) {
    let (handle, ws, _, _, board, _, card) = with_a_card(cx);
    let src = tempfile::tempdir().unwrap();
    open_card(handle, &ws, cx, board, card);
    let late = source(src.path(), "late.txt", b"late");
    attach_to_open(handle, &ws, cx, board, vec![Source::File(late)]);
    press_key(handle, cx, "escape");
    click_on(handle, cx, "leave-discard");
    open_card(handle, &ws, cx, board, card);
    cx.run_until_parked();
    assert!(
        !drawn(handle, cx, "attachment-late.txt".into()),
        "discarded"
    );
    click_on(handle, cx, "card-done");
    assert!(names(&ws, cx, board).is_empty());
    // Delete card is the same.
    open_card(handle, &ws, cx, board, card);
    let more = source(src.path(), "more.txt", b"more");
    attach_to_open(handle, &ws, cx, board, vec![Source::File(more)]);
    click_on(handle, cx, "delete-card");
    ws.update(cx, |ws, cx| assert!(ws.undo_boards(cx)));
    open_card(handle, &ws, cx, board, card);
    cx.run_until_parked();
    assert!(!drawn(handle, cx, "attachment-more.txt".into()));
    assert!(names(&ws, cx, board).is_empty());
}
