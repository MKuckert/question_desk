//! Markdown from Agents, rendered to HTML in Rust and sanitised before it reaches the webview.
//!
//! The `body` of a Question is untrusted: an Agent (or whatever prompt-injected it) writes it.
//! `pulldown-cmark` passes raw HTML through, so everything goes through `ammonia` with an
//! allow-list: no scripts, styles, event handlers, forms, iframes or images (an image would be
//! a remote fetch, i.e. a tracking pixel; the CSP blocks those too). Links keep their `href`
//! for http(s)/mailto only and are opened in the system browser by the frontend.

use std::collections::HashSet;

use pulldown_cmark::{Options, Parser, html};

pub fn to_html(markdown: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let mut unsafe_html = String::with_capacity(markdown.len() * 3 / 2);
    html::push_html(&mut unsafe_html, Parser::new_ext(markdown, options));
    sanitize(&unsafe_html)
}

fn sanitize(html: &str) -> String {
    let tags: HashSet<&str> = [
        "a",
        "blockquote",
        "br",
        "code",
        "del",
        "em",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "hr",
        "input",
        "kbd",
        "li",
        "ol",
        "p",
        "pre",
        "s",
        "strong",
        "sup",
        "table",
        "tbody",
        "td",
        "th",
        "thead",
        "tr",
        "ul",
    ]
    .into_iter()
    .collect();
    ammonia::Builder::default()
        .tags(tags)
        .tag_attributes(
            [
                ("a", ["href", "title"].into_iter().collect()),
                // Task-list checkboxes; `disabled` keeps them read-only.
                ("input", ["type", "checked", "disabled"].into_iter().collect()),
                ("th", ["align"].into_iter().collect()),
                ("td", ["align"].into_iter().collect()),
                ("ol", ["start"].into_iter().collect()),
            ]
            .into_iter()
            .collect(),
        )
        .generic_attributes(HashSet::new())
        .url_schemes(["http", "https", "mailto"].into_iter().collect())
        .link_rel(Some("noopener noreferrer"))
        .attribute_filter(|tag, attr, value| match (tag, attr) {
            ("input", "type") if value != "checkbox" => None,
            _ => Some(value.into()),
        })
        .clean(html)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::to_html;

    #[test]
    fn renders_common_markdown() {
        let html = to_html("# Title\n\n**bold** and `code`\n\n| a | b |\n|---|---|\n| 1 | 2 |");
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<code>code</code>"));
        assert!(html.contains("<table>"));
    }

    #[test]
    fn strips_scripts_handlers_and_images() {
        let html = to_html(
            "<script>alert(1)</script><img src=x onerror=alert(1)>\
             <a href=\"javascript:alert(1)\" onclick=\"x()\">x</a><iframe src=//e></iframe>\
             <p style=\"color:red\">styled</p>",
        );
        assert!(!html.contains("script"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(!html.contains("onerror"), "{html}");
        assert!(!html.contains("onclick"), "{html}");
        assert!(!html.contains("javascript:"), "{html}");
        assert!(!html.contains("iframe"), "{html}");
        assert!(!html.contains("style="), "{html}");
    }

    #[test]
    fn keeps_safe_links_and_task_lists() {
        let html = to_html("[docs](https://example.com)\n\n- [x] done\n- [ ] todo");
        assert!(html.contains("href=\"https://example.com\""), "{html}");
        assert!(html.contains("rel=\"noopener noreferrer\""), "{html}");
        assert!(html.contains("type=\"checkbox\""), "{html}");
        assert!(html.contains("disabled"), "{html}");
    }

    #[test]
    fn keeps_emoji() {
        assert!(to_html("Ship it 🚀👩‍👩‍👧🇩🇪").contains("🚀👩‍👩‍👧🇩🇪"));
    }
}
