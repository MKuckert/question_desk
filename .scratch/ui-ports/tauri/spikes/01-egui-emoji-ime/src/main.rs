//! Spike 1 (gate for ADR 0005): color emoji, emoji picker input, umlauts/IME in egui 0.37 on macOS.
//!
//! Run interactively:   cargo run --release
//! Automated render:    cargo run --release -- --screenshot out.png
//!
//! The screenshot mode renders the sample texts, saves a PNG of the real window and quits,
//! so color emoji rendering can be checked without a human. Input (emoji picker, dead keys,
//! IME) needs a human: follow the checklist in the window, then press "Print report".

mod grapheme;
#[cfg(target_os = "macos")]
mod macos_text_input;

use eframe::egui::{self, Color32, FontId, RichText, TextEdit};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use grapheme::GraphemeText;

const SAMPLE: &str = "Grüße, Fußgänger! äöü ÄÖÜ ß — 😀 🎉 👍🏽 👨‍👩‍👧 🇩🇪 ❤️ ✅ ⌘ 日本語";

const MARKDOWN: &str = "\
### Markdown (egui_commonmark)

**Überschrift mit Emoji 🚀** and *kursiv 🎨*

- Größe: ✅ passt
- Familie: 👨‍👩‍👧‍👦, Flagge: 🇩🇪, Hautton: 👋🏾
- `code with 🦀 and ü`
";

const CHECKLIST: &[&str] = &[
    "Emoji in the sample labels render in color (not monochrome, not ◻)",
    "Emoji keep their colors in tinted text (red/blue lines)",
    "ZWJ family 👨‍👩‍👧, flag 🇩🇪 and skin tone 👍🏽 render as ONE glyph each",
    "Menu bar shows Edit → Emoji & Symbols",
    "Ctrl+Cmd+Space (or the Edit menu item) opens the emoji picker",
    "Picked emoji is inserted into the single-line field",
    "Picked emoji is inserted into the multi-line field",
    "Picking a ZWJ/flag/skin-tone emoji inserts it whole (check the codepoints)",
    "Fn/Globe key (if set to 'Show Emoji & Symbols') also inserts",
    "Typing after picking: no doubled or lost characters",
    "Inserted emoji render in color inside the text field",
    "Umlauts äöüß typed directly (German layout) appear correctly",
    "Dead keys / Option+u then a → ä (US layout) produce the umlaut, no stray ¨",
    "IME (e.g. Japanese Kana/Romaji, Pinyin): preedit is shown, commit inserts text",
    "IME candidate window appears near the cursor",
    "Backspace/Delete remove a whole emoji (family, flag, skin tone) in one press",
    "Arrow keys step over a whole emoji (typing after one arrow press keeps it intact)",
    "Copy/paste of emoji text in and out of the field works",
    "Markdown block renders emoji in color and umlauts correctly",
];

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let screenshot_path = {
        let args: Vec<String> = std::env::args().collect();
        args.iter()
            .position(|a| a == "--screenshot")
            .and_then(|i| args.get(i + 1).cloned())
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 900.0])
            .with_title("Spike 1 — Grüße 😀"),
        ..Default::default()
    };
    eframe::run_native(
        "spike-egui-emoji-ime",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_zoom_factor(1.25);
            #[cfg(target_os = "macos")]
            macos_text_input::install(&cc.egui_ctx);
            Ok(Box::new(Spike::new(screenshot_path)))
        }),
    )
}

struct Spike {
    single_line: String,
    multi_line: String,
    events: Vec<String>,
    checks: Vec<bool>,
    markdown_cache: CommonMarkCache,
    screenshot_path: Option<String>,
    frame: u64,
}

impl Spike {
    fn new(screenshot_path: Option<String>) -> Self {
        // In screenshot mode, prefill the fields so the PNG also shows emoji inside TextEdit.
        let prefill = if screenshot_path.is_some() {
            SAMPLE
        } else {
            ""
        };
        Self {
            single_line: prefill.to_owned(),
            multi_line: prefill.to_owned(),
            events: Vec::new(),
            checks: vec![false; CHECKLIST.len()],
            markdown_cache: CommonMarkCache::default(),
            screenshot_path,
            frame: 0,
        }
    }

    fn record_events(&mut self, ui: &egui::Ui) {
        let events = ui.input(|i| i.events.clone());
        for event in events {
            let line = match &event {
                egui::Event::Text(text) => format!("Text {text:?}  [{}]", codepoints(text)),
                egui::Event::Ime(ime) => format!("Ime {ime:?}"),
                egui::Event::Paste(text) => format!("Paste {text:?}"),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => format!("Key {key:?} {modifiers:?}"),
                _ => continue,
            };
            log::info!("{line}");
            self.events.push(line);
        }
        let excess = self.events.len().saturating_sub(40);
        self.events.drain(..excess);
    }

    fn report(&self, ui: &egui::Ui) -> String {
        let mut out = String::from("# Spike 1 report\n\n## Checklist\n");
        for (item, ok) in CHECKLIST.iter().zip(&self.checks) {
            out += &format!("- [{}] {item}\n", if *ok { "x" } else { " " });
        }
        out += "\n## Fonts discovered via SystemFontProvider\n";
        for name in discovered_fonts(ui) {
            out += &format!("- {name}\n");
        }
        out += &format!(
            "\n## Single-line field\n{:?}\n[{}]\n",
            self.single_line,
            codepoints(&self.single_line)
        );
        out += &format!(
            "\n## Multi-line field\n{:?}\n[{}]\n",
            self.multi_line,
            codepoints(&self.multi_line)
        );
        out += "\n## Last input events\n";
        for e in &self.events {
            out += &format!("- {e}\n");
        }
        out
    }
}

impl eframe::App for Spike {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        #[cfg(target_os = "macos")]
        raw_input.events.extend(
            macos_text_input::take_pending()
                .into_iter()
                .map(egui::Event::Text),
        );
        let _ = raw_input;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame += 1;
        self.record_events(ui);

        if self.screenshot_path.is_some() {
            // System fonts are enumerated on a background thread; give them a moment.
            ui.ctx().request_repaint();
            if self.frame == 60
                && let Some(path) = self.screenshot_path.clone()
            {
                ui.ctx()
                    .request_screenshot(move |image| save_screenshot(&path, &image));
            }
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Spike 1 — color emoji, emoji picker, umlauts/IME (egui 0.37)");
            ui.separator();

            ui.label(RichText::new("Rendering").strong());
            ui.label(SAMPLE);
            ui.label(RichText::new(SAMPLE).font(FontId::proportional(32.0)));
            ui.label(RichText::new(SAMPLE).monospace());
            ui.label(RichText::new(format!("Tinted red: {SAMPLE}")).color(Color32::RED));
            ui.label(RichText::new(format!("Tinted blue: {SAMPLE}")).color(Color32::LIGHT_BLUE));
            let _ = ui.button("Button 👍 Übernehmen");
            ui.separator();

            CommonMarkViewer::new().show(ui, &mut self.markdown_cache, MARKDOWN);
            ui.separator();

            ui.label(RichText::new("Input").strong());
            ui.label("Single-line (try Ctrl+Cmd+Space, umlauts, dead keys, IME):");
            grapheme::show(
                ui,
                TextEdit::singleline(&mut GraphemeText(&mut self.single_line))
                    .desired_width(f32::INFINITY),
            );
            ui.small(codepoints(&self.single_line));
            ui.label("Multi-line:");
            grapheme::show(
                ui,
                TextEdit::multiline(&mut GraphemeText(&mut self.multi_line))
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            ui.small(codepoints(&self.multi_line));
            ui.separator();

            ui.label(RichText::new("Checklist").strong());
            for (item, ok) in CHECKLIST.iter().zip(self.checks.iter_mut()) {
                ui.checkbox(ok, *item);
            }
            if ui.button("Print report to stdout").clicked() {
                println!("{}", self.report(ui));
            }
            ui.separator();

            ui.collapsing("Fonts discovered via SystemFontProvider", |ui| {
                for name in discovered_fonts(ui) {
                    ui.monospace(name);
                }
            });
            ui.collapsing("Input events (Text / Ime / Key)", |ui| {
                for e in self.events.iter().rev() {
                    ui.monospace(e);
                }
            });
        });
    }
}

fn save_screenshot(path: &str, image: &egui::ColorImage) {
    save_png(path, image);
    std::process::exit(0);
}

fn save_png(path: &str, image: &egui::ColorImage) {
    let [w, h] = image.size;
    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    let file = std::fs::File::create(path).expect("create screenshot file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    writer.write_image_data(&rgba).expect("png data");
    log::info!("Saved {path} ({w}x{h})");
}

fn discovered_fonts(ui: &egui::Ui) -> Vec<String> {
    ui.fonts(|f| {
        f.discovered_fonts()
            .iter()
            .map(|f| f.name.clone())
            .collect()
    })
}

fn codepoints(text: &str) -> String {
    text.chars()
        .map(|c| format!("U+{:04X}", c as u32))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Headless check of the rendering half of the gate, without a GPU or window:
/// shapes the sample emoji with the system font provider (as eframe does on native)
/// and inspects the shaped glyphs and the CPU-rasterized font atlas.
///
/// `cargo test --release -- --nocapture` (writes the atlas to `target/font_atlas.png`).
#[cfg(test)]
mod headless {
    use super::*;
    use std::sync::Arc;

    const EMOJI: &[&str] = &["😀", "🎉", "👍🏽", "👨‍👩‍👧", "🇩🇪", "❤️", "✅", "🦀"];

    #[test]
    fn emoji_render_in_color_via_system_font_provider() {
        let ctx = egui::Context::default();
        ctx.add_font_provider(Arc::new(
            egui_system_font_provider::SystemFontProvider::new(),
        ));

        let mut atlas: Option<egui::ColorImage> = None;
        let mut report = Vec::new();
        for _ in 0..3 {
            let mut output = ctx.run_ui(Default::default(), |ui| {
                ui.label(RichText::new(SAMPLE).font(FontId::proportional(32.0)));
                ui.label(RichText::new(SAMPLE).color(Color32::RED));
                report = EMOJI
                    .iter()
                    .map(|emoji| {
                        let galley = ui.fonts_mut(|f| {
                            f.layout_no_wrap(
                                (*emoji).to_owned(),
                                FontId::proportional(32.0),
                                Color32::RED,
                            )
                        });
                        let glyphs: Vec<_> = galley.rows.iter().flat_map(|r| &r.glyphs).collect();
                        // epaint pads multi-char clusters with zero-width continuation
                        // glyphs (glyphs.len() == char count), so count visible ones.
                        let visible = glyphs.iter().filter(|g| g.advance_width > 0.0).count();
                        let colored = glyphs.iter().filter(|g| g.is_color).count();
                        (emoji.to_string(), visible, colored)
                    })
                    .collect();
            });
            let set = std::mem::take(&mut output.textures_delta.set);
            output.textures_delta.clear();
            for delta in set.into_values().flatten() {
                let egui::ImageData::Color(image) = &delta.image;
                match (delta.pos, atlas.as_mut()) {
                    (None, _) => atlas = Some((**image).clone()),
                    (Some([x, y]), Some(atlas)) => {
                        for row in 0..image.size[1] {
                            for col in 0..image.size[0] {
                                atlas[(x + col, y + row)] = image[(col, row)];
                            }
                        }
                    }
                    (Some(_), None) => panic!("partial texture update before full one"),
                }
            }
        }

        let fonts = ctx.fonts(|f| {
            f.discovered_fonts()
                .iter()
                .map(|f| f.name.clone())
                .collect::<Vec<_>>()
        });
        println!("Discovered fonts: {fonts:#?}");
        for (emoji, glyphs, colored) in &report {
            println!("{emoji:<4} visible_glyphs={glyphs} color_glyphs={colored}");
        }

        let atlas = atlas.expect("font atlas uploaded");
        let saturated = atlas
            .pixels
            .iter()
            .filter(|c| {
                let [r, g, b, a] = c.to_array();
                a > 128 && r.max(g).max(b) - r.min(g).min(b) > 80
            })
            .count();
        println!("Atlas {:?}, saturated pixels: {saturated}", atlas.size);
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/target/font_atlas.png");
        save_png(path, &atlas);

        assert!(
            fonts.iter().any(|f| f.contains("Apple Color Emoji")),
            "Apple Color Emoji was not discovered"
        );
        for (emoji, glyphs, colored) in &report {
            assert_eq!(*glyphs, 1, "{emoji} should shape to one visible glyph");
            assert_eq!(*colored, 1, "{emoji} should be a color glyph");
        }
        assert!(saturated > 1000, "atlas has no colored pixels");
    }
}
