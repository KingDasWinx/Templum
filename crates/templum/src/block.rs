//! One terminal block: title bar, body (the grid) and resize handle, in screen space.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, div, px, rgb};
use templum_canvas::Block;

use crate::grid;
use crate::workspace::Workspace;

pub(crate) fn render(ws: &Workspace, block: Block, cx: &mut Context<Workspace>) -> AnyElement {
    let z = ws.canvas.camera.zoom();
    let r = ws.canvas.camera.rect_to_screen(block.rect);
    let pane = &ws.terminals[&block.id];
    let focused = ws.focused == Some(block.id);
    div()
        .absolute()
        .left(px(r.x))
        .top(px(r.y))
        .w(px(r.w))
        .h(px(r.h))
        .bg(rgb(0x181818))
        .overflow_hidden()
        .child(grid::body(
            pane.term.shared().clone(),
            pane.snapshot.clone(),
            ws.cell.clone(),
            z,
            focused,
            ws.marked.clone().filter(|_| focused),
            cx.entity(),
        ))
        .into_any_element()
}
