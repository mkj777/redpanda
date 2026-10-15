use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct Sanitized {
    pub html: String,
    /// The mail references remote resources (images, backgrounds). They stay in the markup but
    /// are blocked by the reading pane's CSP until the user allows them.
    pub has_remote: bool,
}

const EXTRA_TAGS: &[&str] = &[
    "center",
    "font",
    "style",
    "table",
    "tbody",
    "thead",
    "tfoot",
    "tr",
    "td",
    "th",
    "caption",
    "col",
    "colgroup",
    "span",
    "div",
    "section",
    "header",
    "footer",
    "article",
    "main",
    "nav",
    "figure",
    "figcaption",
    "picture",
    "big",
    "small",
    "u",
    "s",
    "strike",
    "hr",
];

const GENERIC_ATTRS: &[&str] = &[
    "style",
    "class",
    "id",
    "align",
    "valign",
    "bgcolor",
    "background",
    "width",
    "height",
    "border",
    "cellpadding",
    "cellspacing",
    "color",
    "face",
    "size",
    "dir",
    "lang",
    "title",
    "role",
    "colspan",
    "rowspan",
    "nowrap",
];

/// Cleans mail HTML: no scripts, forms, event handlers or dangerous URLs. `cid:` image
/// references are replaced with the data URLs from `cid_map`.
pub fn sanitize_html(html: &str, cid_map: &HashMap<String, String>) -> Sanitized {
    let has_remote = Arc::new(AtomicBool::new(false));
    let flag = has_remote.clone();
    let cid_map = cid_map.clone();
    let mut builder = ammonia::Builder::default();
    builder
        .rm_clean_content_tags(&["style"])
        .add_tags(EXTRA_TAGS)
        .add_generic_attributes(GENERIC_ATTRS)
        .add_tag_attributes("img", &["src", "alt", "width", "height", "border", "align"])
        .add_tag_attributes("a", &["href", "name"])
        .add_tag_attributes("font", &["color", "face", "size"])
        .url_schemes(HashSet::from(["http", "https", "mailto", "data", "cid", "tel"]))
        .link_rel(Some("noopener noreferrer"))
        .strip_comments(true)
        .attribute_filter(move |element, attribute, value| {
            filter_attribute(element, attribute, value, &cid_map, &flag)
        });
    let html = builder.clean(html).to_string();
    Sanitized {
        html,
        has_remote: has_remote.load(Ordering::Relaxed),
    }
}

fn filter_attribute<'u>(
    element: &str,
    attribute: &str,
    value: &'u str,
    cid_map: &HashMap<String, String>,
    has_remote: &AtomicBool,
) -> Option<Cow<'u, str>> {
    let lower = value.trim().to_ascii_lowercase();
    match attribute {
        "src" | "background" => {
            if let Some(cid) = lower.strip_prefix("cid:") {
                let original = &value.trim()[4..];
                return cid_map
                    .get(original)
                    .or_else(|| cid_map.get(cid))
                    .map(|url| Cow::Owned(url.clone()));
            }
            // The filter also sees tags that are dropped later (iframe, video), so only count
            // the ones that survive sanitizing.
            let remote =
                lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("//");
            if remote && (element == "img" || attribute == "background") {
                has_remote.store(true, Ordering::Relaxed);
            }
            if lower.starts_with("data:") && !(element == "img" && lower.starts_with("data:image/")) {
                return None;
            }
            Some(Cow::Borrowed(value))
        }
        "href" if lower.starts_with("data:") || lower.starts_with("cid:") => None,
        "style" => {
            if lower.contains("url(") && (lower.contains("http") || lower.contains("//")) {
                has_remote.store(true, Ordering::Relaxed);
            }
            // Old IE expressions and behaviours are the only script vectors in CSS.
            if lower.contains("expression(") || lower.contains("behavior:") || lower.contains("javascript:") {
                return None;
            }
            Some(Cow::Borrowed(value))
        }
        _ => Some(Cow::Borrowed(value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scripts_and_keeps_layout() {
        let s = sanitize_html(
            "<style>p{color:red}</style><table bgcolor=\"#fff\"><tr><td style=\"padding:4px\">x</td></tr></table>\
             <script>alert(1)</script><iframe src=\"https://x\"></iframe>",
            &HashMap::new(),
        );
        assert!(s.html.contains("<style>p{color:red}</style>"), "{}", s.html);
        assert!(s.html.contains("bgcolor=\"#fff\""));
        assert!(s.html.contains("padding:4px"));
        assert!(!s.html.contains("script"));
        assert!(!s.html.contains("iframe"));
        assert!(!s.has_remote);
    }

    #[test]
    fn detects_tracking_pixels_and_css_backgrounds() {
        let s = sanitize_html("<img src=\"https://t.example/p.gif\">", &HashMap::new());
        assert!(s.has_remote);
        let s = sanitize_html(
            "<div style=\"background:url(https://t.example/bg.png)\">x</div>",
            &HashMap::new(),
        );
        assert!(s.has_remote);
    }

    #[test]
    fn rejects_css_expressions_and_html_data_urls() {
        let s = sanitize_html(
            "<div style=\"width:expression(alert(1))\">a</div><a href=\"data:text/html,<script>\">b</a>\
             <img src=\"data:text/html;base64,AAAA\">",
            &HashMap::new(),
        );
        assert!(!s.html.contains("expression"), "{}", s.html);
        assert!(!s.html.contains("data:text/html"), "{}", s.html);
    }

    #[test]
    fn unknown_cid_is_dropped() {
        let s = sanitize_html("<img src=\"cid:missing\">", &HashMap::new());
        assert!(!s.html.contains("cid:"), "{}", s.html);
    }
}
