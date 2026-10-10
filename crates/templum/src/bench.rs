//! `templum --bench`: 20 terminals, 5 running `yes`, 10 s of animated pan and zoom.
//! Prints one JSON line (frame-interval percentiles, memory), then quits. Bar: p95 <= 16.7 ms.

use std::time::{Duration, Instant};

use gpui_kit::{AnyWindowHandle, App, AppContext as _, Entity};
use templum_canvas::{Point, Rect, ZOOM_STEPS, chrome};

use crate::workspace::{DEFAULT_COLS, DEFAULT_ROWS, Workspace};

pub(crate) fn run(window: AnyWindowHandle, ws: Entity<Workspace>, cx: &mut App) {
    let _ = cx.update_window(window, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let size = chrome::block_size(DEFAULT_COLS, DEFAULT_ROWS, ws.cell.size);
            for i in 1..20 {
                let (col, row) = ((i % 5) as f32, (i / 5) as f32);
                let rect = Rect::new(col * (size.w + 40.0), row * (size.h + 40.0), size.w, size.h);
                ws.open(rect, window, cx);
            }
        })
    });
    cx.spawn(async move |cx| {
        // Let the shells start before measuring idle memory.
        cx.background_executor()
            .timer(Duration::from_secs(3))
            .await;
        let idle_rss = rss_mb();
        let _ = cx.update_window(window, |_, _, cx| {
            ws.update(cx, |ws, cx| {
                for pane in ws.terminals.values().take(5) {
                    pane.term.write(&b"yes\r"[..]);
                }
                let center = ws
                    .canvas
                    .bounds()
                    .map(|b| Point::new(b.x + b.w / 2.0, b.y + b.h / 2.0))
                    .unwrap_or_default();
                let start = Instant::now();
                let mut last: Option<Instant> = None;
                let mut intervals: Vec<f64> = Vec::new();
                ws.on_frame = Some(Box::new(move |ws, window, cx| {
                    let now = Instant::now();
                    if let Some(prev) = last {
                        intervals.push((now - prev).as_secs_f64() * 1000.0);
                    }
                    last = Some(now);
                    let t = (now - start).as_secs_f32();
                    if t >= 10.0 {
                        intervals.sort_by(f64::total_cmp);
                        let p = |q: f64| percentile(&intervals, q);
                        let json =
                            |v: Option<f64>| v.map_or("null".to_string(), |v| format!("{v:.1}"));
                        println!(
                            r#"{{"frames":{},"p50_ms":{:.2},"p95_ms":{:.2},"p99_ms":{:.2},"max_ms":{:.2},"rss_mb":{},"idle_rss_mb":{}}}"#,
                            intervals.len() + 1,
                            p(50.0),
                            p(95.0),
                            p(99.0),
                            intervals.last().copied().unwrap_or(0.0),
                            json(rss_mb()),
                            json(idle_rss),
                        );
                        cx.quit();
                        return false;
                    }
                    // Zoom sweeps every step back and forth (4 per second) while panning in a circle.
                    let n = ZOOM_STEPS.len();
                    let k = (t / 0.25) as usize % (2 * (n - 1));
                    let target = if k < n { k } else { 2 * (n - 1) - k };
                    let cam = &mut ws.canvas.camera;
                    cam.zoom_step(target as i32 - cam.zoom_index() as i32, Point::default());
                    let z = cam.zoom();
                    let a = t * std::f32::consts::TAU / 4.0;
                    let vp = window.viewport_size();
                    cam.origin = Point::new(
                        center.x + 400.0 * a.cos() - f32::from(vp.width) / 2.0 / z,
                        center.y + 400.0 * a.sin() - f32::from(vp.height) / 2.0 / z,
                    );
                    window.request_animation_frame();
                    true
                }));
                cx.notify();
            })
        });
    })
    .detach();
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

/// Resident memory in MB (Linux only).
fn rss_mb() -> Option<f64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: f64 = status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some(kb / 1024.0)
}
