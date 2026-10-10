//! The settings panel: it slides in from the right over the canvas, and each field reads the
//! active config and changes it through the workspace, which applies the change on the next frame
//! and saves it shortly after.

use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{
    InputEvent, InputState, NumberInput, NumberInputEvent, StepAction,
};
use gpui_kit::component::setting::{
    RenderOptions, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, TITLE_BAR_HEIGHT};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, BoxShadow, ClickEvent, Context, Entity, SharedString, Subscription,
    TestSupportExt, Window, div, point, px,
};

use crate::config::{self, Blur, Config, Edit, Mode, Shape, ThemeConfig, WindowConfig};
use crate::grid::hsla;
use crate::motion;
use crate::workspace::Workspace;

/// The panel's width, at most 60% of the canvas.
const WIDTH: f32 = 480.0;

/// The open panel, or the closed one still sliding out.
pub(crate) struct Panel {
    pub since: Instant,
    pub closing: bool,
}

pub(crate) fn render(
    ws: &Workspace,
    panel: &Panel,
    now: Instant,
    canvas_w: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.theme.ui;
    let width = WIDTH.min(canvas_w * 0.6);
    let t = motion::slide(motion::progress(panel.since, now, motion::SLIDE));
    let shown = if panel.closing { 1.0 - t } else { t };
    let entity = cx.entity();

    let close = div()
        .id("settings-close")
        .flex()
        .items_center()
        .justify_center()
        .size(px(22.0))
        .rounded(px(6.0))
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .child(
            Icon::new(IconName::Close)
                .size(px(14.0))
                .text_color(ui.title),
        )
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.close_settings(window, cx)))
        .test_support();
    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(36.0))
        .px(px(12.0))
        .border_b_1()
        .border_color(ui.hairline)
        .child(div().text_size(px(13.0)).child("Settings"))
        .child(close);

    let path = ws.config_dir.as_ref().map(|d| d.join(config::FILE));
    let status = match (&path, &ws.save_error) {
        (None, _) => Some((
            "No config folder: changes last until Templum quits".to_string(),
            ui.title_dim,
        )),
        (Some(_), Some(error)) => Some((error.clone(), hsla(ws.theme.palette.ansi[1], 1.0))),
        (Some(_), None) => None,
    };
    let footer = div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .p(px(12.0))
        .border_t_1()
        .border_color(ui.hairline)
        .text_size(px(12.0))
        .when_some(path, |el, path| {
            el.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .child(
                        div()
                            .truncate()
                            .text_color(ui.title_dim)
                            .child(path.display().to_string()),
                    )
                    .child(
                        Button::new("edit-config")
                            .label("Edit config.toml")
                            .small()
                            .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                                ws.open_config(window, cx)
                            })),
                    ),
            )
        })
        .when_some(status, |el, (text, color)| {
            el.child(
                div()
                    .id("settings-status")
                    .text_color(color)
                    .child(text)
                    .test_support(),
            )
        });

    div()
        .id("settings-panel")
        .absolute()
        .top(TITLE_BAR_HEIGHT)
        .bottom(px(0.0))
        .right(px(-width * (1.0 - shown)))
        .w(px(width))
        .flex()
        .flex_col()
        .bg(hsla(ws.theme.palette.background, 1.0))
        .text_color(hsla(ws.theme.palette.foreground, 1.0))
        .border_l_1()
        .border_color(ui.hairline)
        .shadow(vec![BoxShadow {
            color: ui.shadow,
            offset: point(px(-8.0), px(0.0)),
            blur_radius: px(32.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .track_focus(&ws.settings_focus)
        .on_key_down(cx.listener(Workspace::on_settings_key))
        // Nothing under the panel (blocks, the canvas) gets its presses or its wheel.
        .occlude()
        .child(header)
        .child(
            div().flex_1().min_h(px(0.0)).child(
                Settings::new("settings")
                    .sidebar_width(px(140.0))
                    .sidebar_size_range(px(120.0)..px(200.0))
                    .pages(pages(ws, &entity)),
            ),
        )
        .child(footer)
        .test_support()
        .into_any_element()
}

/// A dropdown setting: its options (value, label), the option the config is at, and what
/// choosing one does to the config and writes to the file.
struct Choice {
    table: &'static str,
    key: &'static str,
    options: &'static [(&'static str, &'static str)],
    get: fn(&Config) -> &'static str,
    set: fn(&mut Config, &str),
    toml: fn(&str) -> toml_edit::Value,
}

static MODE: Choice = Choice {
    table: "theme",
    key: "mode",
    options: &[("system", "System"), ("light", "Light"), ("dark", "Dark")],
    get: |c| match c.theme.mode {
        Mode::System => "system",
        Mode::Light => "light",
        Mode::Dark => "dark",
    },
    set: |c, v| {
        c.theme.mode = match v {
            "light" => Mode::Light,
            "dark" => Mode::Dark,
            _ => Mode::System,
        }
    },
    toml: |v| v.into(),
};

static GLASS: Choice = Choice {
    table: "window",
    key: "blur",
    options: &[("auto", "Auto"), ("on", "On"), ("off", "Off")],
    get: |c| match c.window.blur {
        Blur::Auto => "auto",
        Blur::On => "on",
        Blur::Off => "off",
    },
    set: |c, v| {
        c.window.blur = match v {
            "on" => Blur::On,
            "off" => Blur::Off,
            _ => Blur::Auto,
        }
    },
    // config.toml spells them "auto", true and false.
    toml: |v| match v {
        "on" => true.into(),
        "off" => false.into(),
        _ => "auto".into(),
    },
};

static CURSOR: Choice = Choice {
    table: "cursor",
    key: "shape",
    options: &[
        ("block", "Block"),
        ("bar", "Bar"),
        ("underline", "Underline"),
    ],
    get: |c| match c.cursor.shape {
        Shape::Block => "block",
        Shape::Bar => "bar",
        Shape::Underline => "underline",
    },
    set: |c, v| {
        c.cursor.shape = match v {
            "bar" => Shape::Bar,
            "underline" => Shape::Underline,
            _ => Shape::Block,
        }
    },
    toml: |v| v.into(),
};

/// A whole-number setting and its limits.
pub(crate) struct Number {
    table: &'static str,
    key: &'static str,
    min: f64,
    max: f64,
    step: f64,
    get: fn(&Config) -> f64,
    set: fn(&mut Config, f64),
}

pub(crate) static SCROLLBACK: Number = Number {
    table: "terminal",
    key: "scrollback",
    min: 0.0,
    max: 100_000.0,
    step: 1000.0,
    get: |c| c.terminal.scrollback as f64,
    set: |c, v| c.terminal.scrollback = v as usize,
};

/// "Reset to default" removes `table.key` from config.toml. Its button shows while the value
/// isn't the default.
fn reset<T>(
    field: SettingField<T>,
    ws: &Entity<Workspace>,
    table: &'static str,
    key: &'static str,
    is_default: impl Fn(&Config) -> bool + 'static,
    to_default: impl Fn(&mut Config) + 'static,
) -> SettingField<T> {
    let read = ws.clone();
    let write = ws.clone();
    field.on_reset(
        move |cx| !is_default(&read.read(cx).config),
        move |_, cx| {
            write.update(cx, |ws, cx| {
                ws.edit(Edit::remove(table, key), &to_default, cx)
            })
        },
    )
}

fn dropdown(ws: &Entity<Workspace>, choice: &'static Choice) -> SettingField<SharedString> {
    let options = choice
        .options
        .iter()
        .map(|(value, label)| (SharedString::from(*value), SharedString::from(*label)))
        .collect();
    let read = ws.clone();
    let write = ws.clone();
    let field = SettingField::dropdown(
        options,
        move |cx| (choice.get)(&read.read(cx).config).into(),
        move |value: SharedString, cx| {
            write.update(cx, |ws, cx| {
                ws.edit(
                    Edit::set(choice.table, choice.key, (choice.toml)(&value)),
                    |c| (choice.set)(c, &value),
                    cx,
                )
            })
        },
    );
    let default = (choice.get)(&Config::default());
    reset(
        field,
        ws,
        choice.table,
        choice.key,
        move |c| (choice.get)(c) == default,
        move |c| (choice.set)(c, default),
    )
}

/// A number field's editor. The value applies on Enter, when the field loses the focus, or on a
/// step (+/−, ↑/↓), never while typing: scrollback passing through "2" on the way to "20000"
/// would trim every terminal's history.
pub(crate) struct NumberState {
    pub input: Entity<InputState>,
    /// The config's value last shown: a different one (a hand edit, a reset) replaces the text.
    shown: f64,
    _events: [Subscription; 2],
}

impl NumberState {
    pub fn new(
        ws: Entity<Workspace>,
        n: &'static Number,
        value: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // No step, min or max on the input: it then leaves stepping to us (NumberInputEvent).
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value.to_string()));
        let typed = {
            let ws = ws.clone();
            cx.subscribe_in(
                &input,
                window,
                move |_, input, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur)
                        && let Ok(v) = input.read(cx).value().trim().parse::<f64>()
                    {
                        set_number(&ws, n, v, cx);
                    }
                },
            )
        };
        let stepped = cx.subscribe_in(
            &input,
            window,
            move |_, input, event: &NumberInputEvent, window, cx| {
                let NumberInputEvent::Step(action) = event;
                let current = input
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<f64>()
                    .unwrap_or_else(|_| (n.get)(&ws.read(cx).config));
                let v = match action {
                    StepAction::Increment => current + n.step,
                    StepAction::Decrement => current - n.step,
                }
                .clamp(n.min, n.max);
                input.update(cx, |input, cx| input.set_value(v.to_string(), window, cx));
                set_number(&ws, n, v, cx);
            },
        );
        Self {
            input,
            shown: value,
            _events: [typed, stepped],
        }
    }
}

/// Typed values may have decimals or go past the limits; the file gets whole numbers within them.
fn set_number(ws: &Entity<Workspace>, n: &'static Number, v: f64, cx: &mut App) {
    let v = v.clamp(n.min, n.max).round();
    ws.update(cx, |ws, cx| {
        ws.edit(Edit::set(n.table, n.key, v as i64), |c| (n.set)(c, v), cx)
    });
}

fn number(ws: &Entity<Workspace>, n: &'static Number) -> SettingField<SharedString> {
    let handle = ws.clone();
    let field = SettingField::render(
        move |options: &RenderOptions, window: &mut Window, cx: &mut App| {
            let value = (n.get)(&handle.read(cx).config);
            let ws = handle.clone();
            let key = SharedString::from(format!("number-{}.{}", n.table, n.key));
            let state = window.use_keyed_state(key, cx, move |window, cx| {
                NumberState::new(ws, n, value, window, cx)
            });
            state.update(cx, |state, cx| {
                if state.shown != value {
                    state.shown = value;
                    state.input.update(cx, |input, cx| {
                        input.set_value(value.to_string(), window, cx)
                    });
                }
            });
            NumberInput::new(&state.read(cx).input)
                .disabled(options.is_disabled())
                .w_32()
        },
    );
    let default = (n.get)(&Config::default());
    reset(
        field,
        ws,
        n.table,
        n.key,
        move |c| (n.get)(c) == default,
        move |c| (n.set)(c, default),
    )
}

/// `theme.light` or `theme.dark`: the built-in themes and the user's themes folder.
fn theme(
    ws: &Entity<Workspace>,
    names: Vec<SharedString>,
    dark: bool,
) -> SettingField<SharedString> {
    let key = if dark { "dark" } else { "light" };
    let read = ws.clone();
    let write = ws.clone();
    let field = SettingField::scrollable_dropdown(
        names.into_iter().map(|n| (n.clone(), n)).collect(),
        move |cx| read.read(cx).config.theme.name(dark).to_string().into(),
        move |name: SharedString, cx| {
            write.update(cx, |ws, cx| {
                ws.edit(
                    Edit::set("theme", key, name.to_string()),
                    |c| set_theme(c, dark, name.to_string()),
                    cx,
                )
            })
        },
    );
    reset(
        field,
        ws,
        "theme",
        key,
        move |c| c.theme.name(dark) == ThemeConfig::default().name(dark),
        move |c| set_theme(c, dark, ThemeConfig::default().name(dark).to_string()),
    )
}

fn set_theme(c: &mut Config, dark: bool, name: String) {
    if dark {
        c.theme.dark = name;
    } else {
        c.theme.light = name;
    }
}

fn blink(ws: &Entity<Workspace>) -> SettingField<bool> {
    let read = ws.clone();
    let write = ws.clone();
    let field = SettingField::switch(
        move |cx| read.read(cx).config.cursor.blink,
        move |on, cx| {
            write.update(cx, |ws, cx| {
                ws.edit(
                    Edit::set("cursor", "blink", on),
                    |c| c.cursor.blink = on,
                    cx,
                )
            })
        },
    );
    reset(
        field,
        ws,
        "cursor",
        "blink",
        |c| !c.cursor.blink,
        |c| c.cursor.blink = false,
    )
}

/// What the file gets for an opacity of `percent`%: 72 → 0.72.
fn opacity_value(percent: f32) -> f64 {
    f64::from(percent.round()) / 100.0
}

/// The slider's state lives in the window, keyed by the field.
struct OpacitySlider {
    slider: Entity<SliderState>,
    _change: Subscription,
}

fn opacity(ws: &Entity<Workspace>) -> SettingField<SharedString> {
    let handle = ws.clone();
    let field = SettingField::render(
        move |options: &RenderOptions, window: &mut Window, cx: &mut App| {
            let percent = (handle.read(cx).config.window.opacity * 100.0).round();
            let ws = handle.clone();
            let state = window.use_keyed_state("opacity-slider", cx, move |_, cx| {
                let slider = cx.new(|_| {
                    SliderState::new()
                        .min(0.0)
                        .max(100.0)
                        .step(1.0)
                        .default_value(percent)
                });
                let _change = cx.subscribe(&slider, move |_, _, event: &SliderEvent, cx| {
                    if let SliderEvent::Change(value) = event {
                        set_opacity(&ws, value.start(), cx);
                    }
                });
                OpacitySlider { slider, _change }
            });
            let slider = state.read(cx).slider.clone();
            // A hand edit of the file, or "reset to default", moves the thumb.
            if slider.read(cx).value().start() != percent {
                slider.update(cx, |s, cx| s.set_value(percent, window, cx));
            }
            Slider::new(&slider)
                .disabled(options.is_disabled())
                .w(px(160.0))
        },
    );
    let default = WindowConfig::default().opacity;
    reset(
        field,
        ws,
        "window",
        "opacity",
        move |c| c.window.opacity == default,
        move |c| c.window.opacity = default,
    )
}

fn set_opacity(ws: &Entity<Workspace>, percent: f32, cx: &mut App) {
    let value = opacity_value(percent);
    ws.update(cx, |ws, cx| {
        ws.edit(
            Edit::set("window", "opacity", value),
            |c| c.window.opacity = value as f32,
            cx,
        )
    });
}

/// The panel's sections, built every frame from the workspace's state.
pub(crate) fn pages(ws: &Workspace, entity: &Entity<Workspace>) -> Vec<SettingPage> {
    let names: Vec<SharedString> = ws
        .themes
        .iter()
        .map(|e| SharedString::from(e.theme.name.clone()))
        .collect();
    vec![
        SettingPage::new("Appearance").group(
            SettingGroup::new().items([
                SettingItem::new("Mode", dropdown(entity, &MODE))
                    .description("System follows the desktop's light or dark setting"),
                SettingItem::new("Light theme", theme(entity, names.clone(), false))
                    .keywords(["color", "colour"]),
                SettingItem::new("Dark theme", theme(entity, names, true))
                    .keywords(["color", "colour"]),
                SettingItem::new("Glass", dropdown(entity, &GLASS))
                    .description("Blur the desktop behind the canvas")
                    .keywords(["blur", "transparent"]),
                SettingItem::new("Canvas opacity", opacity(entity))
                    .description("How much the canvas covers the glass")
                    .keywords(["transparency", "alpha"])
                    .disabled(!ws.translucent),
            ]),
        ),
        SettingPage::new("Terminal").group(
            SettingGroup::new().items([
                SettingItem::new("Cursor", dropdown(entity, &CURSOR)),
                SettingItem::new("Blink", blink(entity)).keywords(["cursor"]),
                SettingItem::new("Scrollback", number(entity, &SCROLLBACK))
                    .description("Lines of history per terminal")
                    .keywords(["history", "lines"]),
            ]),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_choice_reads_back_what_it_writes() {
        for choice in [&MODE, &GLASS, &CURSOR] {
            for (value, _) in choice.options {
                let mut c = Config::default();
                (choice.set)(&mut c, value);
                assert_eq!((choice.get)(&c), *value, "{}.{}", choice.table, choice.key);
                let text = format!(
                    "[{}]\n{} = {}\n",
                    choice.table,
                    choice.key,
                    (choice.toml)(value)
                );
                let parsed = config::parse(&text).unwrap();
                assert_eq!((choice.get)(&parsed), *value, "{text}");
            }
        }
    }

    #[test]
    fn numbers_read_back_what_they_write() {
        for n in [&SCROLLBACK] {
            for v in [n.min, n.step, n.max] {
                let mut c = Config::default();
                (n.set)(&mut c, v);
                assert_eq!((n.get)(&c), v, "{}.{}", n.table, n.key);
                let text = format!("[{}]\n{} = {}\n", n.table, n.key, v as i64);
                assert_eq!((n.get)(&config::parse(&text).unwrap()), v, "{text}");
            }
        }
    }

    #[test]
    fn opacity_percent_reads_back_from_the_file() {
        assert_eq!(
            toml_edit::Value::from(opacity_value(72.0)).to_string(),
            "0.72"
        );
        for p in 0..=100 {
            let p = p as f32;
            let text = format!(
                "[window]\nopacity = {}\n",
                toml_edit::Value::from(opacity_value(p))
            );
            let c = config::parse(&text).unwrap();
            assert_eq!((c.window.opacity * 100.0).round(), p, "{text}");
        }
    }
}
