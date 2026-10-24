// A graphical app on Windows: no console window opens behind it. Debug builds keep theirs, for
// eprintln! and panics.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod agents;
mod alert;
mod attachments;
mod autostart;
mod balloon;
mod bench;
mod block;
mod board;
mod chat;
mod checklist;
mod checklists;
mod chime;
mod config;
mod editor;
mod gallery;
mod git_block;
mod grid;
mod inbox;
mod ink;
mod instructions;
mod kanban;
mod keys;
mod launcher;
mod list_dialog;
mod markdown;
mod menu;
mod motion;
mod note;
mod notes;
mod prompt;
mod session;
mod settings;
mod sidebar;
mod silhouette;
mod space_dialog;
mod spaces_screen;
mod tab_strip;
mod theme;
mod theme_picker;
mod title_bar;
mod toolbar;
mod update;
mod usage;
mod welcome;
mod wires;
mod workspace;

use autostart::Autostart;
use gpui_kit::component::TitleBar;
use gpui_kit::component::notification::Notification;
use gpui_kit::{
    AppContext as _, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowDecorations, WindowOptions, px, size,
};
use workspace::Workspace;

fn main() {
    // Claude Code runs it as its status line, in the Claude Code Templum opens: no window.
    if std::env::args().nth(1).as_deref() == Some(templum_usage::claude::RELAY_FLAG) {
        relay();
        return;
    }
    // `templum mcp`, `templum hook claude`, `templum read <block>`…: no window
    // (docs/specs/connections.md).
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| templum_mcp::cli::is_verb(a)) {
        let code = templum_mcp::cli::run(
            &args,
            &templum_mcp::cli::Env::of_process(),
            Box::new(std::io::stdin()),
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
        std::process::exit(code);
    }
    let mixed = std::env::args().any(|a| a == "--bench-mixed");
    let far = std::env::args().any(|a| a == "--bench-far");
    let zoom = std::env::args().find_map(|a| a.strip_prefix("--bench-zoom=").map(str::to_string));
    let git = std::env::args().any(|a| a == "--bench-git");
    let bench = mixed || far || zoom.is_some() || git || std::env::args().any(|a| a == "--bench");
    // A build being worked on, beside the installed Templum: its own workspaces, settings and bus.
    let dev = std::env::args().any(|a| a == "--dev");
    // The agents installed where only .bashrc or .zshrc put them.
    #[cfg(unix)]
    if !bench {
        templum_terminal::read_shell_path();
    }
    // At sign-out and shutdown the canvas is saved while its terminals still run.
    #[cfg(windows)]
    templum_terminal::quit_before_terminals();
    let dirs = move |dir: Option<std::path::PathBuf>| if dev { dir.map(session::dev) } else { dir };
    // The bench starts from one terminal and must not touch the user's saved canvas.
    let data_dir = if bench { None } else { dirs(session::dir()) };
    let config_dir = dirs(config::dir());
    // One Templum per data folder: a second would restore the same canvas, resume the same
    // agents and write over the first's files. It keeps nothing instead.
    let lock = data_dir
        .as_deref()
        .map(|dir| session::lock(dir, session::QUITTING));
    let second = match &lock {
        Some(Err(e)) if e.kind() == std::io::ErrorKind::WouldBlock => true,
        Some(Err(e)) => {
            eprintln!("templum: couldn't lock the data folder: {e}");
            false
        }
        _ => false,
    };
    let data_dir = data_dir.filter(|_| !second);
    // The bench leaves the person's login items alone.
    let autostart = if bench { None } else { Autostart::locate(dev) };
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
                    title: Some(if dev { "Templum (dev)" } else { "Templum" }.into()),
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
            let (window, ws) = gpui_kit::open_window(options, cx, |window, cx| {
                if second {
                    let message = "Templum is already open in another window: this one starts \
                                   empty and keeps nothing, so the other's canvas stays as it is";
                    workspace::show_notification(window, cx, Notification::warning(message));
                }
                cx.new(|cx| Workspace::new(autostart, config_dir, data_dir, window, cx))
            })
            .expect("failed to open window");
            if let Some(kind) = zoom {
                bench::run_zoom(window, ws, kind, cx);
            } else if git {
                bench::run_git(window, ws, cx);
            } else if far {
                bench::run_far(window, ws, cx);
            } else if bench {
                bench::run(window, ws, mixed, cx);
            }
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            cx.activate(true);
        });
    // Free before an installer starts the next Templum.
    drop(lock);
    // Restart into an installer (Windows): it replaces Templum, so it starts once Templum is done.
    let installer = update::INSTALLER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(launch) = installer
        && let Err(e) = launch.spawn()
    {
        eprintln!("templum: couldn't start {}: {e}", launch.program.display());
    }
}

/// Hands Templum what Claude Code tells its status line, then prints the user's own status line
/// (docs/specs/usage.md).
fn relay() {
    use std::io::{Read as _, Write as _};
    let mut input = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut input);
    // The folder Templum named in the settings file: Claude Code's environment may name another.
    let data = std::env::args_os()
        .nth(2)
        .map(std::path::PathBuf::from)
        .or_else(session::dir);
    let dirs = data.map(|data| templum_usage::Dirs::of(&data));
    let terminal = std::env::var("TEMPLUM_TERMINAL").ok();
    // An account's Claude Code names its folder: its limits are that account's.
    let config_dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from);
    let out = match &dirs {
        Some(dirs) => templum_usage::claude::relay(
            &input,
            &dirs.relay,
            terminal.as_deref(),
            dirs.claude.as_deref(),
            config_dir.as_deref(),
        ),
        None => templum_usage::claude::relay(&input, std::path::Path::new(""), None, None, None),
    };
    let _ = std::io::stdout().write_all(&out);
}
