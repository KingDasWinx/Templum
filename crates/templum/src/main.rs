mod bench;
mod block;
mod config;
mod grid;
mod theme;
mod workspace;

use gpui_kit::{AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use workspace::Workspace;

fn main() {
    let bench = std::env::args().any(|a| a == "--bench");
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
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
                    ..Default::default()
                }),
                app_id: Some("templum".into()),
                // The bench must not be throttled to 30 fps when the window loses focus.
                inactive_frame_interval: if bench {
                    None
                } else {
                    Some(std::time::Duration::from_micros(33_333))
                },
                ..Default::default()
            };
            let (window, ws) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| Workspace::new(window, cx))
            })
            .expect("failed to open window");
            if bench {
                bench::run(window, ws, cx);
            }
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            cx.activate(true);
        });
}
