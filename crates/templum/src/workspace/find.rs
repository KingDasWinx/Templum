//! Finding text in a terminal's history: a field over the terminal's top right corner.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, Focusable as _, Subscription, Window};
use templum_canvas::BlockId;
use templum_terminal::{Find, Toward};

use super::Workspace;

/// A terminal's find field, while it is open.
pub(crate) struct FindBar {
    pub input: Entity<InputState>,
    /// The search for the text typed; None while it is empty.
    find: Option<Find>,
    /// The text typed matches nothing.
    pub missing: bool,
    _events: Subscription,
}

impl FindBar {
    pub fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.input.read(cx).focus_handle(cx).is_focused(window)
    }
}

impl Workspace {
    /// The `find` command: the focused terminal's field, opened (or its text selected again)
    /// with the keyboard.
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self
            .space
            .focused
            .filter(|id| self.space.terminal(*id).is_some())
        else {
            return;
        };
        let input = match self.space.terminal(id).and_then(|p| p.find.as_ref()) {
            Some(bar) => bar.input.clone(),
            None => {
                let input =
                    cx.new(|cx| InputState::new(window, cx).placeholder("Find in the terminal"));
                let _events =
                    cx.subscribe_in(&input, window, move |ws, _, event: &InputEvent, _, cx| {
                        match event {
                            InputEvent::Change => ws.search(id, Toward::Here, cx),
                            // Enter goes up the history, older; Shift+Enter down.
                            InputEvent::PressEnter { shift, .. } => ws.search(
                                id,
                                if *shift { Toward::Newer } else { Toward::Older },
                                cx,
                            ),
                            _ => {}
                        }
                    });
                if let Some(pane) = self.space.terminal_mut(id) {
                    pane.find = Some(Box::new(FindBar {
                        input: input.clone(),
                        find: None,
                        missing: false,
                        _events,
                    }));
                }
                input
            }
        };
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        // A mirrored terminal searches the history it has: home's is read, and the search is made
        // again when it lands.
        self.remote_history(id);
        cx.notify();
    }

    /// Searches the terminal `id` again for the text in its open field, from the match shown: its
    /// history changed.
    pub(super) fn search_again(&mut self, id: BlockId, cx: &mut Context<Self>) {
        self.search(id, Toward::Here, cx);
    }

    /// Searches the terminal `id` for its field's text, `toward` the match shown.
    fn search(&mut self, id: BlockId, toward: Toward, cx: &mut Context<Self>) {
        let Some(pane) = self.space.terminal_mut(id) else {
            return;
        };
        let Some(bar) = pane.find.as_mut() else {
            return;
        };
        if toward == Toward::Here {
            bar.find = Find::new(&bar.input.read(cx).value());
        }
        bar.missing = match bar.find.as_mut() {
            Some(find) => !pane.term.shared().find(find, toward),
            None => false,
        };
        cx.notify();
    }

    /// Closes the focused terminal's field; the match stays selected (to copy it) and the
    /// terminal gets the keyboard back.
    pub(super) fn close_find(&mut self, id: BlockId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self.space.terminal_mut(id) {
            pane.find = None;
        }
        self.focus_block(id, window, cx);
    }

    /// The clipboard's text typed into the terminal `id`'s find field.
    pub(super) fn paste_into_find(
        &mut self,
        id: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if let Some(bar) = self.space.terminal(id).and_then(|p| p.find.as_ref()) {
            bar.input
                .update(cx, |input, cx| input.insert(text, window, cx));
        }
    }

    /// The terminal whose find field has the keyboard.
    pub(super) fn finding(&self, window: &Window, cx: &App) -> Option<BlockId> {
        self.space
            .terminals()
            .find(|(_, pane)| pane.find.as_ref().is_some_and(|b| b.has_focus(window, cx)))
            .map(|(id, _)| id)
    }
}
