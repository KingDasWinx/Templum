//! Small dialogs over the window: a name to type, and a question to confirm.

use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, SharedString, Window};

/// A text field holding `initial`, selected. Enter or `ok` calls `apply` with the text (Enter
/// confirms the dialog by itself); the dialog closes when it returns true.
pub(crate) fn ask_text<V: 'static>(
    title: impl Into<SharedString>,
    ok: &'static str,
    initial: impl Into<SharedString>,
    apply: impl Fn(String, &mut Window, &mut App) -> bool + 'static,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let title = title.into();
    let initial = initial.into();
    let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
    let apply = Rc::new(
        move |input: &Entity<InputState>, window: &mut Window, cx: &mut App| {
            let text = input.read(cx).value().to_string();
            apply(text, window, cx)
        },
    );
    let field = input.clone();
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (apply, field) = (apply.clone(), field.clone());
        alert
            .confirm()
            .title(title.clone())
            .button_props(DialogButtonProps::default().ok_text(ok))
            .child(Input::new(&field))
            .on_ok(move |_, window, cx| apply(&field, window, cx))
    });
    // After opening: the dialog takes the focus when it opens.
    input.update(cx, |input, cx| {
        input.focus(window, cx);
        input.select_all(window, cx);
    });
}

/// A question with Cancel and `ok`; `ok` calls `on_ok`.
pub(crate) fn confirm(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    ok: &'static str,
    on_ok: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let (title, description) = (title.into(), description.into());
    let on_ok = Rc::new(on_ok);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let on_ok = on_ok.clone();
        alert
            .confirm()
            .title(title.clone())
            .description(description.clone())
            .button_props(DialogButtonProps::default().ok_text(ok))
            .on_ok(move |_, window, cx| {
                on_ok(window, cx);
                true
            })
    });
}
