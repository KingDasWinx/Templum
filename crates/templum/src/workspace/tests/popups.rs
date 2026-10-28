//! The popups' shared chrome (crate::popup): Requests and Automations resize by Git's edges, full
//! screen fills the room, and a press outside closes any of them.

use super::*;
use gpui_kit::{ElementId, Pixels};

/// Which popup a test opens.
#[derive(Clone, Copy, Debug)]
enum Which {
    Inbox,
    Agents,
    Automations,
}

impl Which {
    /// Its box's element.
    fn id(self) -> &'static str {
        match self {
            Which::Inbox => "inbox",
            Which::Agents => "agents",
            Which::Automations => "automations",
        }
    }

    fn open(self, handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| match self {
                Which::Inbox => ws.open_inbox(None, window, cx),
                Which::Agents => ws.open_agents(window, cx),
                Which::Automations => ws.open_automations(window, cx),
            });
            window.render_frame(cx);
        })
        .unwrap();
    }

    fn is_open(self, ws: &Entity<Workspace>, cx: &mut TestAppContext) -> bool {
        ws.read_with(cx, |ws, _| match self {
            Which::Inbox => ws.inbox_open,
            Which::Agents => ws.agents_open,
            Which::Automations => ws.automations_open,
        })
    }
}

/// The box of the element `id` in the window, after a frame.
fn bounds(handle: AnyWindowHandle, cx: &mut TestAppContext, id: &'static str) -> Bounds<Pixels> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(id).bounds()
    })
    .unwrap()
}

fn drawn(handle: AnyWindowHandle, cx: &mut TestAppContext, id: &'static str) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
    .unwrap()
}

/// Presses the middle of the resize strip `zone`, drags `by` and lets go.
fn drag_zone(handle: AnyWindowHandle, cx: &mut TestAppContext, zone: &'static str, by: (f32, f32)) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let from = window.find(zone).bounds().center();
        window.drag(from, point(from.x + px(by.0), from.y + px(by.1)), cx);
    })
    .unwrap();
}

fn click_id(handle: AnyWindowHandle, cx: &mut TestAppContext, id: &'static str) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(ElementId::Name(id.into()), cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// Clicks the window at `at`, far from where any popup sits.
fn click_at(handle: AnyWindowHandle, cx: &mut TestAppContext, at: (f32, f32)) {
    cx.update_window(handle, |_, window, cx| {
        click(window, cx, at, 1);
        window.render_frame(cx);
    })
    .unwrap();
}

fn size_of(b: Bounds<Pixels>) -> (f32, f32) {
    (f32::from(b.size.width), f32::from(b.size.height))
}

fn close_to(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01
}

#[gpui_kit::test]
fn requests_and_automations_resize_by_gits_edges_and_share_the_size(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    Which::Automations.open(handle, &ws, cx);
    let before = bounds(handle, cx, "automations");
    assert!(close_to(
        size_of(before),
        (crate::git_block::SIZE.w, crate::git_block::SIZE.h)
    ));
    drag_zone(handle, cx, "popup-resize-top-right", (120.0, -80.0));
    let grown = bounds(handle, cx, "automations");
    assert!(close_to(
        size_of(grown),
        (
            crate::git_block::SIZE.w + 120.0,
            crate::git_block::SIZE.h + 80.0
        )
    ));
    assert_eq!(
        (grown.origin.x, grown.bottom()),
        (before.origin.x, before.bottom()),
        "it grows around its bottom-left corner"
    );
    let set = ws.read_with(cx, |ws, _| ws.git_size).expect("a size set");
    assert!(close_to((set.w, set.h), size_of(grown)));
    // The inbox opens at the size set, and its own right edge resizes it.
    Which::Inbox.open(handle, &ws, cx);
    assert_eq!(bounds(handle, cx, "inbox"), grown);
    drag_zone(handle, cx, "popup-resize-right", (-60.0, 0.0));
    let narrower = bounds(handle, cx, "inbox");
    assert!(close_to(
        size_of(narrower),
        (size_of(grown).0 - 60.0, size_of(grown).1)
    ));
}

#[gpui_kit::test]
fn full_screen_fills_the_room_and_gives_the_size_set_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    Which::Automations.open(handle, &ws, cx);
    let set = bounds(handle, cx, "automations");
    assert!(drawn(handle, cx, "popup-resize-top"));
    click_id(handle, cx, "popup-full");
    let (vp, origin) = cx
        .update_window(handle, |_, window, cx| {
            let ws = ws.read(cx);
            (ws.viewport(window), ws.origin)
        })
        .unwrap();
    let full = bounds(handle, cx, "automations");
    let gap = crate::popup::GAP;
    assert!(close_to(
        size_of(full),
        (vp.w - 2.0 * gap, vp.h - 2.0 * gap)
    ));
    assert!(
        (f32::from(full.origin.x) - f32::from(origin.x) - gap).abs() < 0.01
            && (f32::from(full.origin.y) - f32::from(origin.y) - gap).abs() < 0.01,
        "the canvas's room, beside the sidebar: {full:?} from {origin:?}"
    );
    assert!(!drawn(handle, cx, "popup-resize-top"), "no edges to drag");
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.git_size, None, "full screen sets no size")
    });
    // The other popups open full screen too, Git's among them.
    Which::Agents.open(handle, &ws, cx);
    assert_eq!(bounds(handle, cx, "agents"), full);
    // Restored: the size set before.
    click_id(handle, cx, "popup-full");
    assert_eq!(bounds(handle, cx, "agents"), set);
    assert!(drawn(handle, cx, "popup-resize-top"));
}

/// At full screen the Agents details keep to `popup::CONTENT_MAX`, centered in their column.
#[gpui_kit::test]
fn the_agents_details_keep_to_the_content_width_at_full_screen(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    super::agents_view::agent(handle, &ws, cx, 0);
    Which::Agents.open(handle, &ws, cx);
    click_id(handle, cx, "popup-full");
    let popup = bounds(handle, cx, "agents");
    let card = bounds(handle, cx, "agents-summary");
    // The column right of the list and its hairline, inside the frame's border.
    let left = f32::from(popup.left()) + 1.0 + crate::agents_view::LIST_W + 1.0;
    let right = f32::from(popup.right()) - 1.0;
    let max = crate::popup::CONTENT_MAX;
    assert!(right - left > max + 100.0, "room to spare: {popup:?}");
    assert!(f32::from(card.size.width) <= max, "{card:?}");
    let (l, r) = (
        f32::from(card.left()) - left,
        right - f32::from(card.right()),
    );
    assert!(
        (l - r).abs() < 1.0,
        "centered: {l} at the left, {r} at the right"
    );
}

#[gpui_kit::test]
fn git_has_the_same_full_screen(cx: &mut TestAppContext) {
    let repo = git_repo();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    let set = bounds(handle, cx, "git-popup");
    click_id(handle, cx, "popup-full");
    assert!(bounds(handle, cx, "git-popup").size.width > set.size.width);
    click_id(handle, cx, "popup-full");
    assert_eq!(bounds(handle, cx, "git-popup"), set);
}

#[gpui_kit::test]
fn a_press_outside_closes_each_popup_and_one_inside_doesnt(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    for which in [Which::Inbox, Which::Agents, Which::Automations] {
        which.open(handle, &ws, cx);
        let b = bounds(handle, cx, which.id());
        // Inside, on the header's empty middle: it stays.
        let inside = (f32::from(b.center().x), f32::from(b.top()) + 20.0);
        click_at(handle, cx, inside);
        assert!(which.is_open(&ws, cx), "{which:?}: a press inside");
        // Above it, over the canvas: it closes.
        let outside = (f32::from(b.center().x), f32::from(b.top()) - 40.0);
        click_at(handle, cx, outside);
        assert!(!which.is_open(&ws, cx), "{which:?}: a press outside");
        assert!(
            !drawn(handle, cx, "popup-backdrop"),
            "{which:?}: gone with it"
        );
    }
}

#[gpui_kit::test]
fn the_close_button_closes_each_popup(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    for (which, close) in [
        (Which::Inbox, "inbox-close"),
        (Which::Agents, "agents-close"),
        (Which::Automations, "automations-close"),
    ] {
        which.open(handle, &ws, cx);
        click_id(handle, cx, close);
        assert!(!which.is_open(&ws, cx), "{which:?}");
    }
}

#[test]
fn the_headers_count_what_is_there() {
    assert_eq!(crate::inbox::waiting_words(0), "Nothing waits for you");
    assert_eq!(
        crate::inbox::waiting_words(2),
        "2 requests wait for your answer"
    );
    assert_eq!(
        crate::agents_view::running_words(0, 0),
        "No agent is running"
    );
    assert_eq!(
        crate::agents_view::running_words(3, 1),
        "3 running · 1 needs you"
    );
    assert_eq!(crate::automations_view::count_words(1), "1 automation");
    assert_eq!(
        crate::inbox::where_words(Some("/src/api"), "Main"),
        "/src/api · Main"
    );
    assert_eq!(crate::inbox::where_words(None, "Main"), "Main");
    // A folder under the home is shortened as the Home's cards shorten it.
    let rest = std::path::Path::new("code").join("api");
    let folder = std::env::home_dir().unwrap().join(&rest);
    assert_eq!(
        crate::inbox::where_words(Some(&folder.display().to_string()), "Main"),
        format!("~/{} · Main", rest.display())
    );
}
