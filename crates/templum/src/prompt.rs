//! Small dialogs over the window: a name to type, a question to confirm, a question with its
//! own buttons.

use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, SharedString, Window, div, px};

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

/// A field of several lines (about 6 rows), empty, with the keyboard. Ctrl+Enter or `ok` calls
/// `apply` with the text; the dialog closes when it returns true. A text field that takes a long
/// text, a task for an agent for one.
pub(crate) fn ask_lines<V: 'static>(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    ok: &'static str,
    apply: impl Fn(String, &mut Window, &mut App) -> bool + 'static,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let (title, description) = (title.into(), description.into());
    let input = cx.new(|cx| TextareaState::new(window, cx).rows(6));
    let apply = Rc::new(
        move |input: &Entity<TextareaState>, window: &mut Window, cx: &mut App| {
            let text = input.read(cx).value().to_string();
            apply(text, window, cx)
        },
    );
    // Ctrl+Enter confirms from the field (Enter is a new line).
    {
        let (field, apply) = (input.clone(), apply.clone());
        window
            .subscribe(&input, cx, move |_, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter {
                    secondary: true, ..
                } = event
                    && apply(&field, window, cx)
                {
                    window.close_dialog(cx);
                }
            })
            .detach();
    }
    let field = input.clone();
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (apply, field) = (apply.clone(), field.clone());
        alert
            .confirm()
            .title(title.clone())
            .description(description.clone())
            .w(px(480.0))
            .button_props(DialogButtonProps::default().ok_text(ok))
            .child(Textarea::new(&field))
            .on_ok(move |_, window, cx| apply(&field, window, cx))
    });
    input.update(cx, |input, cx| input.focus(window, cx));
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
    #[cfg(test)]
    crate::workspace::told(&format!("{title} {description}"));
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

/// Save, Don't save or Cancel. `on_save` says whether it saved (the dialog stays otherwise);
/// `on_discard` goes on without saving.
pub(crate) fn save_or_discard(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    on_save: impl Fn(&mut Window, &mut App) -> bool + 'static,
    on_discard: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let (title, description) = (title.into(), description.into());
    let (on_save, on_discard) = (Rc::new(on_save), Rc::new(on_discard));
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (save, discard) = (on_save.clone(), on_discard.clone());
        alert
            .confirm()
            .title(title.clone())
            .description(description.clone())
            .footer(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(Button::new("discard").label("Don't save").on_click(
                        move |_, window, cx| {
                            window.close_dialog(cx);
                            discard(window, cx);
                        },
                    ))
                    .child(Button::new("save").label("Save").primary().on_click(
                        move |_, window, cx| {
                            if save(window, cx) {
                                window.close_dialog(cx);
                            }
                        },
                    )),
            )
    });
}

/// What a `choose` button does.
pub(crate) type Act = Rc<dyn Fn(&mut Window, &mut App)>;

/// One button of `choose`.
pub(crate) struct Choice {
    /// The button's element id (the tests click it).
    pub id: &'static str,
    pub label: SharedString,
    pub primary: bool,
    pub act: Act,
}

/// A question with its own buttons, right to left as given reversed (the first is the main
/// one, drawn last). Each closes the dialog, then acts. An empty `description` shows none.
pub(crate) fn choose(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    choices: Vec<Choice>,
    window: &mut Window,
    cx: &mut App,
) {
    let (title, description) = (title.into(), description.into());
    let choices = Rc::new(choices);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let buttons = choices.iter().rev().map(|c| {
            let act = c.act.clone();
            let button = Button::new(c.id)
                .label(c.label.clone())
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    act(window, cx);
                });
            if c.primary { button.primary() } else { button }
        });
        let alert = alert.confirm().title(title.clone());
        let alert = if description.is_empty() {
            alert
        } else {
            alert.description(description.clone())
        };
        alert.footer(
            div()
                .flex()
                .flex_wrap()
                .justify_end()
                .gap(px(8.0))
                .children(buttons),
        )
    });
}

/// A dialog showing `text` in the terminal font, with OK.
pub(crate) fn details(
    title: impl Into<SharedString>,
    text: String,
    mono: SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    let title = title.into();
    window.open_alert_dialog(cx, move |alert, _, _| {
        alert
            .confirm()
            .title(title.clone())
            .w(px(640.0))
            .button_props(DialogButtonProps::default().show_cancel(false))
            .child(
                div()
                    .id("details")
                    .max_h(px(360.0))
                    .overflow_y_scroll()
                    .font_family(mono.clone())
                    .text_size(px(12.0))
                    .child(text.clone()),
            )
    });
}

/// Two text fields (placeholder, initial text). Enter or `ok` calls `apply` with both; the
/// dialog closes when it returns true.
pub(crate) fn ask_two<V: 'static>(
    title: impl Into<SharedString>,
    ok: &'static str,
    first: (&'static str, String),
    second: (&'static str, String),
    apply: impl Fn(String, String, &mut Window, &mut App) -> bool + 'static,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let title = title.into();
    let field = |(placeholder, initial): (&'static str, String),
                 window: &mut Window,
                 cx: &mut Context<V>| {
        cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(initial)
        })
    };
    let (a, b) = (field(first, window, cx), field(second, window, cx));
    let apply = Rc::new(apply);
    let (fa, fb) = (a.clone(), b.clone());
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (apply, fa, fb) = (apply.clone(), fa.clone(), fb.clone());
        alert
            .confirm()
            .title(title.clone())
            .w(px(520.0))
            .button_props(DialogButtonProps::default().ok_text(ok))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(Input::new(&fa))
                    .child(Input::new(&fb)),
            )
            .on_ok(move |_, window, cx| {
                let (x, y) = (
                    fa.read(cx).value().to_string(),
                    fb.read(cx).value().to_string(),
                );
                apply(x, y, window, cx)
            })
    });
    // After opening: the dialog takes the focus when it opens.
    a.update(cx, |input, cx| input.focus(window, cx));
}

/// A text field and a checkbox (`flag`, off at first). Enter or `ok` calls `apply` with the
/// text and the checkbox; the dialog closes when it returns true.
pub(crate) fn ask_text_and_flag<V: 'static>(
    title: impl Into<SharedString>,
    ok: &'static str,
    placeholder: impl Into<SharedString>,
    flag: &'static str,
    apply: impl Fn(String, bool, &mut Window, &mut App) -> bool + 'static,
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let title = title.into();
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
    let checked = Rc::new(std::cell::Cell::new(false));
    let apply = Rc::new(apply);
    let (field, state) = (input.clone(), checked.clone());
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (apply, field, state) = (apply.clone(), field.clone(), state.clone());
        let toggle = state.clone();
        alert
            .confirm()
            .title(title.clone())
            .button_props(DialogButtonProps::default().ok_text(ok))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(Input::new(&field))
                    .child(
                        Checkbox::new("flag")
                            .label(flag)
                            .checked(state.get())
                            .on_click(move |on, _, _| toggle.set(*on)),
                    ),
            )
            .on_ok(move |_, window, cx| {
                let text = field.read(cx).value().to_string();
                apply(text, state.get(), window, cx)
            })
    });
    input.update(cx, |input, cx| input.focus(window, cx));
}
