//! HTML documents for the native web views that show a Question's Markdown `body` and an
//! Option's `preview`.
//!
//! The pages follow the system: `color-scheme: light dark` lets WebKit/WebView2 pick the
//! light or dark palette with the app, the font is the platform UI font at the size the
//! native controls use, and links take the system accent. Each page reports its content
//! height to the app (`qd` script message), so the view can grow to fit short content.

use pulldown_cmark::{Options, Parser, html};

/// Name of the script message handler pages post their height to.
pub const HEIGHT_HANDLER: &str = "qd";

/// The `body` of a Question as a full HTML page.
pub fn body_page(markdown: &str, font_size: f32) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let mut out = String::new();
    // Raw HTML from an Agent is shown as text, not run: the page has no business
    // loading scripts or remote content an Agent slipped into a body.
    let events = Parser::new_ext(markdown, options).map(|event| match event {
        pulldown_cmark::Event::Html(text) | pulldown_cmark::Event::InlineHtml(text) => {
            pulldown_cmark::Event::Text(text)
        }
        other => other,
    });
    html::push_html(&mut out, events);
    page(&out, font_size, "")
}

/// An Option's `preview`: monospaced, as written, scrolling sideways if it must.
pub fn preview_page(preview: Option<&str>, font_size: f32) -> String {
    let content = match preview {
        Some(text) => format!("<pre>{}</pre>", escape(text)),
        None => "<p class=\"hint\">Hover an option to see its preview.</p>".to_owned(),
    };
    page(
        &content,
        font_size,
        "html { background: color-mix(in srgb, CanvasText 6%, Canvas); min-height: 100%; } \
         body { padding: 8px 10px; } pre { margin: 0; border: 0; background: none; padding: 0; }",
    )
}

fn page(content: &str, font_size: f32, extra_css: &str) -> String {
    let size = css_size(font_size);
    let mono = css_size(font_size - 1.0);
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:">
<style>
:root {{ color-scheme: light dark; }}
html, body {{ margin: 0; padding: 0; }}
body {{
  font-family: -apple-system, system-ui, "Segoe UI", "Cantarell", "Ubuntu", sans-serif;
  font-size: {size}; line-height: 1.45; padding: 2px 0;
  -webkit-user-select: text; user-select: text; overflow-wrap: anywhere;
}}
a {{ color: -apple-system-control-accent; color: LinkText; }}
@supports (color: -apple-system-control-accent) {{ a {{ color: -apple-system-control-accent; }} }}
h1, h2, h3, h4 {{ margin: 0.9em 0 0.4em; line-height: 1.25; }}
h1 {{ font-size: 1.35em; }} h2 {{ font-size: 1.2em; }} h3, h4 {{ font-size: 1.05em; }}
p, ul, ol, table, pre, blockquote {{ margin: 0.5em 0; }}
body > :first-child {{ margin-top: 0; }} body > :last-child {{ margin-bottom: 0; }}
ul, ol {{ padding-left: 1.6em; }}
code, pre {{ font-family: ui-monospace, "SF Mono", Menlo, Consolas, "DejaVu Sans Mono", monospace; font-size: {mono}; }}
code {{ background: color-mix(in srgb, CanvasText 8%, transparent); padding: 0.1em 0.3em; border-radius: 4px; }}
pre {{ background: color-mix(in srgb, CanvasText 6%, transparent); padding: 8px 10px; border-radius: 6px; overflow-x: auto; }}
pre code {{ background: none; padding: 0; }}
blockquote {{ border-left: 3px solid color-mix(in srgb, CanvasText 25%, transparent); padding-left: 10px; color: color-mix(in srgb, CanvasText 70%, transparent); }}
table {{ border-collapse: collapse; }}
th, td {{ border: 1px solid color-mix(in srgb, CanvasText 20%, transparent); padding: 4px 8px; text-align: left; vertical-align: top; }}
th {{ background: color-mix(in srgb, CanvasText 5%, transparent); }}
hr {{ border: 0; border-top: 1px solid color-mix(in srgb, CanvasText 20%, transparent); }}
.hint {{ color: GrayText; margin: 0; }}
{extra_css}
</style></head>
<body>{content}
<script>
(function () {{
  function post() {{
    // The body's own height: the document's would never be less than the view.
    var h = Math.ceil(document.body.getBoundingClientRect().height);
    var w = Math.ceil(document.documentElement.scrollWidth);
    var msg = h + "," + w;
    try {{
      if (window.{HEIGHT_HANDLER} && window.{HEIGHT_HANDLER}.postMessage) {{ window.{HEIGHT_HANDLER}.postMessage(msg); }}
      else if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.{HEIGHT_HANDLER}) {{ window.webkit.messageHandlers.{HEIGHT_HANDLER}.postMessage(msg); }}
      else if (window.chrome && window.chrome.webview) {{ window.chrome.webview.postMessage(msg); }}
    }} catch (e) {{}}
  }}
  window.addEventListener("load", post);
  if (window.ResizeObserver) {{ new ResizeObserver(post).observe(document.body); }}
  post();
}})();
</script>
</body></html>"#
    )
}

/// CSS length for a font size in points. WebKit on macOS maps CSS px to points 1:1;
/// WebView2 and WebKitGTK follow the 96-dpi CSS reference, so points are points there.
fn css_size(points: f32) -> String {
    if cfg!(target_os = "macos") {
        format!("{points:.1}px")
    } else {
        format!("{points:.1}pt")
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_html_is_shown_as_text() {
        let page = body_page("hi <script>alert(1)</script>", 13.0);
        assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    }

    #[test]
    fn preview_is_escaped_and_preformatted() {
        let page = preview_page(Some("a < b\n  c"), 13.0);
        assert!(page.contains("<pre>a &lt; b\n  c</pre>"));
    }
}
