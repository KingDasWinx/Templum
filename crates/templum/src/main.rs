mod agents;
mod alert;
mod bench;
mod block;
mod board;
mod config;
mod grid;
mod kanban;
mod keys;
mod launcher;
mod menu;
mod motion;
mod prompt;
mod session;
mod settings;
mod sidebar;
mod theme;
mod theme_picker;
mod title_bar;
mod workspace;

use gpui_kit::component::TitleBar;
use gpui_kit::{
    AppContext as _, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowDecorations, WindowOptions, px, size,
};
use workspace::Workspace;

fn main() {
    let bench = std::env::args().any(|a| a == "--bench");
    gpui_kit::application()
        // Every Lucide icon, by name: the toolbar and the notes use ones outside the default set.
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            // Names Templum in system notifications; Windows shows none without it.
            cx.set_app_identity("KingDasWinx.Templum", "Templum");
            gpui_kit::init(cx);
            workspace::bind_keys(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1280.0), px(800.0)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Templum".into()),
                    ..TitleBar::title_bar_options()
                }),
                window_background: WindowBackgroundAppearance::Opaque,
                // Linux: our title bar instead of the desktop's (elsewhere `titlebar` does it).
                window_decorations: cfg!(target_os = "linux").then_some(WindowDecorations::Client),
                app_id: Some("templum".into()),
                // The bench must not be throttled to 30 fps when the window loses focus.
                inactive_frame_interval: if bench {
                    None
                } else {
                    Some(std::time::Duration::from_micros(33_333))
                },
                ..TitleBar::window_options()
            };
            // The bench starts from one terminal and must not touch the user's saved canvas.
            let data_dir = if bench { None } else { session::dir() };
            let (window, ws) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| Workspace::new(data_dir, window, cx))
            })
            .expect("failed to open window");
            if bench {
                bench::run(window, ws, cx);
            }
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            cx.activate(true);
        });
}
