//! Screenshot hook for `tools/screenshot.sh` where the shell may not record the screen (the
//! nono sandbox has no Screen Recording permission, so `screencapture` fails).
//!
//! With `QUESTION_DESK_SNAPSHOT=<file.png>` set, the app watches for `<file.png>.request`. When
//! it appears, the app optionally opens the dialog the request names (`settings`, `cancel`,
//! `submit`, `close`), then renders its windows with `Window::take_snapshot`: the main window to
//! `<file.png>`, an open dialog to `<file>-dialog.png`. Only the app's own window content is
//! captured; the native title bar and the menu bar (drawn by macOS) are not part of it.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Polls for requests; `on_request(dialog)` must return the windows to capture once the
/// dialog (if any) is open, as `(suffix, window)` pairs.
pub fn install(
    on_request: impl Fn(&str) + 'static,
    capture: impl Fn(&dyn Fn(&str, &slint::Window)) + 'static,
) -> Option<slint::Timer> {
    let out = PathBuf::from(std::env::var_os("QUESTION_DESK_SNAPSHOT")?);
    let request = out.with_extension("png.request");
    let capture = std::rc::Rc::new(capture);
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(250),
        move || {
            let Ok(dialog) = std::fs::read_to_string(&request) else {
                return;
            };
            let _ = std::fs::remove_file(&request);
            on_request(dialog.trim());
            let (out, capture) = (out.clone(), capture.clone());
            // Give a freshly opened dialog time to lay out and draw.
            slint::Timer::single_shot(Duration::from_millis(1500), move || {
                // Not from inside the timer callback: the Qt backend paints synchronously during
                // `take_snapshot`, which re-enters Slint's timer code and panics.
                let _ = slint::spawn_local(async move {
                    capture(&|suffix, window| {
                        let path = if suffix.is_empty() {
                            out.clone()
                        } else {
                            out.with_file_name(format!(
                                "{}-{suffix}.png",
                                out.file_stem().unwrap_or_default().to_string_lossy()
                            ))
                        };
                        match window.take_snapshot() {
                            Ok(pixels) => {
                                if let Err(e) = write_png(&path, &pixels) {
                                    log::error!("snapshot {}: {e}", path.display());
                                }
                            }
                            Err(e) => log::error!("snapshot failed: {e}"),
                        }
                    })
                });
            });
        },
    );
    Some(timer)
}

fn write_png(
    path: &Path,
    pixels: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = path.with_extension("png.tmp");
    let file = std::io::BufWriter::new(std::fs::File::create(&temp)?);
    let mut encoder = png::Encoder::new(file, pixels.width(), pixels.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()?
        .write_image_data(pixels.as_bytes())?;
    std::fs::rename(temp, path)?;
    Ok(())
}
