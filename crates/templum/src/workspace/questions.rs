//! An agent's question in its dialog (docs/specs/ask-user.md): its fields, what the person picks,
//! Send and Dismiss, its keys.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Focusable as _, Keystroke, Window, point, px};
use templum_mcp::Reply;
use templum_mcp::ask::{Ask, Form, Row};

use super::Workspace;
use super::delegate::NoticeKind;
use super::waits::{Field, Wait, Waiting, field_focused};

/// A dismissed question's answer.
const DISMISSED: &str = "The person dismissed the question without answering.";

/// The question `key`'s fields: an Other field per question (a question without options: its
/// field) and, asked with `questions`, the note. Typing in an Other field picks it; Enter in any
/// sends.
pub(super) fn fields(
    key: u64,
    ask: &Ask,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> (Vec<Field>, Option<Field>) {
    let others = ask
        .questions
        .iter()
        .enumerate()
        .map(|(q, question)| {
            let placeholder = if question.options.is_empty() {
                "Your answer"
            } else {
                "Something else"
            };
            field(key, Some(q), Some(placeholder), window, cx)
        })
        .collect();
    let note = match (ask.json, &ask.plan) {
        (_, Some(_)) => Some(field(key, None, Some("Comment (optional)"), window, cx)),
        (true, None) => Some(field(
            key,
            None,
            Some("Add a note for the agent"),
            window,
            cx,
        )),
        (false, None) => None,
    };
    (others, note)
}

/// A field of the question `key`: question `q`'s Other field, or with None the note.
fn field(
    key: u64,
    q: Option<usize>,
    placeholder: Option<&'static str>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Field {
    let input = cx.new(|cx| {
        let input = InputState::new(window, cx);
        match placeholder {
            Some(text) => input.placeholder(text),
            None => input,
        }
    });
    let events = cx.subscribe_in(
        &input,
        window,
        move |ws, input, event: &InputEvent, _, cx| match event {
            InputEvent::Change => {
                if let Some(q) = q {
                    let text = input.read(cx).value().to_string();
                    ws.other_typed(key, q, &text, cx);
                }
            }
            // Enter also goes on to the inbox's keys (`question_key`): answered there, once.
            InputEvent::PressEnter { .. } | InputEvent::Blur | InputEvent::Focus => {}
        },
    );
    (input, events)
}

impl Workspace {
    /// The question `key`, and what the person picked in it.
    fn form_of(&mut self, key: u64) -> Option<(&Ask, &mut Form)> {
        self.waits
            .iter_mut()
            .find(|w| w.key == key)
            .and_then(|w| match &mut w.what {
                Waiting::Question { ask, form, .. } => Some((&*ask, form)),
                _ => None,
            })
    }

    /// Question `q`'s Other field of the question `key` holds `text` now.
    fn other_typed(&mut self, key: u64, q: usize, text: &str, cx: &mut Context<Self>) {
        if let Some((ask, form)) = self.form_of(key) {
            form.typed(ask, q, text);
            cx.notify();
        }
    }

    /// The pointer on `row` of the question `key`: the highlight goes there.
    pub(crate) fn highlight_row(&mut self, key: u64, row: Row, cx: &mut Context<Self>) {
        if let Some((_, form)) = self.form_of(key)
            && form.highlight != Some(row)
        {
            form.highlight = Some(row);
            cx.notify();
        }
    }

    /// A click on option `i` of question `q` of the question `key`: picked, or checked, and
    /// nothing more: Send answers. A pick in a single-select question shows the next question
    /// without an answer, as Enter does. A plan's Approve and Reject answer with their click.
    pub(crate) fn pick_option(
        &mut self,
        key: u64,
        q: usize,
        i: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.question_too_soon(key, cx) {
            return;
        }
        let others = self.others(key, cx);
        let Some((ask, form)) = self.form_of(key) else {
            return;
        };
        form.pick(ask, q, i);
        let quick = ask.quick();
        let next = (!ask.questions[q].multi)
            .then(|| form.unanswered(ask, &others))
            .flatten();
        if let Some(next) = next.filter(|next| *next != form.tab) {
            form.show(ask, next);
            self.question_turned(key, window, cx);
        }
        cx.notify();
        if quick {
            self.send_question(key, window, cx);
        }
    }

    /// Show more / Show less on the previews of the question `key`.
    pub(crate) fn toggle_previews(&mut self, key: u64, cx: &mut Context<Self>) {
        if let Some(wait) = self.waits.iter().find(|w| w.key == key) {
            wait.more.set(!wait.more.get());
            cx.notify();
        }
    }

    /// A click on the tab of question `q` of the question `key`: that question shows.
    pub(crate) fn show_question(
        &mut self,
        key: u64,
        q: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((ask, form)) = self.form_of(key)
            && form.tab != q
        {
            form.show(ask, q);
            self.question_turned(key, window, cx);
            cx.notify();
        }
    }

    /// The question `key` shows another of its questions: the keyboard goes where the highlight
    /// went (off a hidden field), and the body back to its top, where the question starts.
    fn question_turned(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_highlight(key, window, cx);
        if let Some(wait) = self.waits.iter().find(|w| w.key == key) {
            wait.body.handle.set_offset(point(px(0.0), px(0.0)));
        }
    }

    /// Send, or Enter in a field: the question `key` is answered once each of its questions has
    /// an answer.
    pub(crate) fn send_question(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(wait) = self.waits.iter().find(|w| w.key == key) else {
            return;
        };
        if wait.too_soon(cx) {
            return;
        }
        let (Some((others, note)), Waiting::Question { ask, form, .. }) =
            (self.texts(wait, cx), &wait.what)
        else {
            return;
        };
        let Some(answers) = form.answers(ask, &others) else {
            return;
        };
        let reply = ask.reply(&answers, &note);
        // Its client stopped waiting: typed into its terminal, once its agent takes typing.
        let notice = matches!(wait.what, Waiting::Question { stopped: true, .. })
            .then(|| (wait.from.clone(), ask.notice(&reply.text)));
        self.close_question(key, reply, window, cx);
        if let Some((to, notice)) = notice {
            self.queue_notice(to, notice, NoticeKind::Other, cx);
        }
    }

    /// Dismiss: the question `key` is refused.
    pub(crate) fn dismiss_question(
        &mut self,
        key: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.question_too_soon(key, cx) {
            self.close_question(key, Reply::err(DISMISSED), window, cx);
        }
    }

    /// The wait `key` was drawn under TOO_SOON ago, or not yet.
    fn question_too_soon(&self, key: u64, cx: &App) -> bool {
        self.waits
            .iter()
            .find(|w| w.key == key)
            .is_none_or(|w| w.too_soon(cx))
    }

    /// The keys of the request `key` while the inbox has the keyboard (`inbox_keys`): ↑ ↓ Tab
    /// Shift+Tab move the highlight, ← → show the previous or next question (out of a field),
    /// Space picks, Enter picks and sends or moves on. True: the dialog took the key.
    pub(crate) fn question_key(
        &mut self,
        key: u64,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(wait) = self.waits.iter().find(|w| w.key == key) else {
            return false;
        };
        let m = &keystroke.modifiers;
        if m.control || m.alt || m.platform || m.function {
            return false;
        }
        let (by, turn) = match (keystroke.key.as_str(), m.shift) {
            ("down", false) | ("tab", false) => (Some(1), None),
            ("up", false) | ("tab", true) => (Some(-1), None),
            ("left", false) => (None, Some(-1)),
            ("right", false) => (None, Some(1)),
            ("space" | "enter", false) => (None, None),
            _ => return false,
        };
        // In a field, the highlight is its row, and Space and ← → are the field's: at the start
        // or the end of its text, the field leaves an arrow to its parents, which drop it.
        let typing = self.typing_row(wait, window, cx);
        if typing.is_some() && (keystroke.key == "space" || turn.is_some()) {
            return false;
        }
        if turn.is_some() && !matches!(&wait.what, Waiting::Question { ask, .. } if ask.tabs()) {
            return false;
        }
        if wait.too_soon(cx) {
            return true;
        }
        let Some((ask, form)) = self.form_of(key) else {
            return false;
        };
        if typing.is_some() {
            form.highlight = typing;
        }
        match (by, turn, keystroke.key.as_str()) {
            (Some(by), _, _) => {
                form.step(ask, by);
                self.focus_highlight(key, window, cx);
                self.reveal_highlight(key);
            }
            (_, Some(turn), _) => {
                form.turn(ask, turn);
                self.question_turned(key, window, cx);
            }
            (_, _, "space") => {
                if let Some(Row::Option(q, i)) = form.highlight {
                    form.pick(ask, q, i);
                }
            }
            (_, _, "enter") if form.highlight.is_some() => {
                self.question_enter(key, window, cx);
            }
            _ => {}
        }
        cx.notify();
        true
    }

    /// Enter on the question `key`: a single-select option highlighted is picked. Once each
    /// question has an answer Enter sends, except the Enter that just picked the last one, which
    /// moves the highlight to Send; before, the highlight moves to the next question without one.
    fn question_enter(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(wait) = self.waits.iter().find(|w| w.key == key) else {
            return;
        };
        if wait.too_soon(cx) {
            return;
        }
        let typing = self.typing_row(wait, window, cx);
        let others = self.others(key, cx);
        let Some((ask, form)) = self.form_of(key) else {
            return;
        };
        if typing.is_some() {
            form.highlight = typing;
        }
        let tab = form.tab;
        // An option picked now is only picked: the Enter that sends comes after it, on Send or
        // anywhere once the dialog is ready. A plan's Approve and Reject answer at once.
        let mut picking = false;
        if let Some(Row::Option(q, i)) = form.highlight
            && !ask.questions[q].multi
        {
            picking = !form.picked(q, i) && !ask.quick();
            form.pick(ask, q, i);
        }
        if form.answers(ask, &others).is_none() {
            form.next_unanswered(ask, &others);
        } else if picking {
            form.highlight = Some(Row::Send);
        } else {
            self.send_question(key, window, cx);
            return;
        }
        if form.tab != tab {
            self.question_turned(key, window, cx);
        } else {
            self.focus_highlight(key, window, cx);
        }
        self.reveal_highlight(key);
        cx.notify();
    }

    /// The highlight the keys moved: the dialog's body scrolls to show its row when it is drawn.
    fn reveal_highlight(&self, key: u64) {
        if let Some(wait) = self.waits.iter().find(|w| w.key == key)
            && let Waiting::Question { form, .. } = &wait.what
        {
            wait.reveal.set(form.highlight);
        }
    }

    /// The row of the question's field that has the keyboard, if one does.
    fn typing_row(&self, wait: &Wait, window: &Window, cx: &App) -> Option<Row> {
        let Waiting::Question { others, note, .. } = &wait.what else {
            return None;
        };
        let focused = |(input, _): &Field| input.read(cx).focus_handle(cx).is_focused(window);
        if note.as_ref().is_some_and(focused) {
            return Some(Row::Note);
        }
        others.iter().position(focused).map(Row::Other)
    }

    /// The highlight on an Other row or the note: its field takes the keyboard; elsewhere the
    /// inbox has it again.
    fn focus_highlight(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(wait) = self.waits.iter().find(|w| w.key == key) else {
            return;
        };
        let Waiting::Question {
            form, others, note, ..
        } = &wait.what
        else {
            return;
        };
        let field = match form.highlight {
            Some(Row::Other(q)) => others.get(q),
            Some(Row::Note) => note.as_ref(),
            _ => None,
        };
        match field {
            Some((input, _)) => input.update(cx, |input, cx| input.focus(window, cx)),
            None => window.focus(&self.inbox_focus, cx),
        }
    }

    /// Answers the question `key` with `reply`; the keyboard goes back to the inbox when one of
    /// its fields had it.
    fn close_question(
        &mut self,
        key: u64,
        reply: Reply,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(wait) = self.waits.iter().find(|w| w.key == key) else {
            return;
        };
        let typing = field_focused(wait, window, cx);
        let from = wait.from.clone();
        self.answer(key, reply, cx);
        self.seen_asking(&from);
        if typing {
            window.focus(&self.inbox_focus, cx);
        }
    }

    /// The Other texts of the question `key` (none when it isn't one).
    fn others(&self, key: u64, cx: &App) -> Vec<String> {
        self.waits
            .iter()
            .find(|w| w.key == key)
            .and_then(|w| self.texts(w, cx))
            .map(|(others, _)| others)
            .unwrap_or_default()
    }

    /// A question's Other texts and its note ("" without one).
    pub(crate) fn texts(&self, wait: &Wait, cx: &App) -> Option<(Vec<String>, String)> {
        let Waiting::Question { others, note, .. } = &wait.what else {
            return None;
        };
        let text = |(input, _): &Field| input.read(cx).value().to_string();
        Some((
            others.iter().map(text).collect(),
            note.as_ref().map(text).unwrap_or_default(),
        ))
    }
}
