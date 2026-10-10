//! The settings panel's fields: each one reads the active config and changes it through the
//! workspace, which applies the change on the next frame and saves it shortly after.

use gpui_kit::component::setting::{
    NumberFieldOptions, RenderOptions, SettingField, SettingGroup, SettingItem, SettingPage,
};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::prelude::*;
use gpui_kit::{App, Entity, SharedString, Subscription, Window, px};

use crate::config::{self, Blur, Config, Edit, Mode, Shape, ThemeConfig, WindowConfig};
use crate::workspace::Workspace;

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
struct Number {
    table: &'static str,
    key: &'static str,
    min: f64,
    max: f64,
    step: f64,
    get: fn(&Config) -> f64,
    set: fn(&mut Config, f64),
}

static SCROLLBACK: Number = Number {
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

fn number(ws: &Entity<Workspace>, n: &'static Number) -> SettingField<f64> {
    let read = ws.clone();
    let write = ws.clone();
    let field = SettingField::number_input(
        NumberFieldOptions {
            min: n.min,
            max: n.max,
            step: n.step,
        },
        move |cx| (n.get)(&read.read(cx).config),
        move |v, cx| {
            // Typed values may have decimals; the file gets whole numbers.
            let v = v.round();
            write.update(cx, |ws, cx| {
                ws.edit(Edit::set(n.table, n.key, v as i64), |c| (n.set)(c, v), cx)
            })
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
