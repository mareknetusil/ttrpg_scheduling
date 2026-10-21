//! Markdown → sanitized HTML (DESIGN §7). Templates only ever get the output of `render`, as
//! `*_html` fields marked `|safe`; raw Markdown is never marked safe.

use pulldown_cmark::{Options, Parser, html};

pub fn render(source: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let mut unsafe_html = String::new();
    html::push_html(&mut unsafe_html, Parser::new_ext(source, options));

    // Task lists render as `<input disabled type="checkbox">`. Allow exactly that input and
    // nothing else, so raw HTML in the Markdown can't add text fields.
    ammonia::Builder::default()
        .add_tags(["input"])
        .add_tag_attributes("input", ["type", "checked", "disabled"])
        .add_tag_attributes("code", ["class"])
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("input", "type") => (value == "checkbox").then_some(value.into()),
            _ => Some(value.into()),
        })
        .set_tag_attribute_value("input", "disabled", "")
        .clean(&unsafe_html)
        .to_string()
}

/// Turns a title into a URL slug: lowercase ASCII letters and digits, separated by single `-`.
/// Accented letters are transliterated first ("Čarodějnice" → "carodejnice"). The result can be
/// empty (e.g. "???"); callers fall back to "page".
pub fn slugify(title: &str) -> String {
    let mut slug = String::new();
    for c in deunicode::deunicode(title).chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    slug.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_basic_markdown() {
        let html = render("# Hi\n\n**bold** ~~gone~~\n\n| a | b |\n|---|---|\n| 1 | 2 |");
        assert!(html.contains("<h1>Hi</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<del>gone</del>"));
        assert!(html.contains("<table>"));
    }

    #[test]
    fn strips_dangerous_html() {
        let html = render(
            "<script>alert(1)</script><a href=\"javascript:alert(1)\" onclick=\"x()\">x</a><img src=x onerror=alert(1)>",
        );
        assert!(!html.contains("<script"));
        assert!(!html.contains("javascript:"));
        assert!(!html.contains("onclick"));
        assert!(!html.contains("onerror"));
    }

    #[test]
    fn task_lists_keep_checkboxes_but_no_other_inputs() {
        let html = render("- [x] done\n- [ ] todo\n\n<input type=\"text\" name=\"x\">");
        assert_eq!(html.matches("type=\"checkbox\"").count(), 2);
        assert!(html.contains("checked"));
        assert!(!html.contains("type=\"text\""));
        assert!(!html.contains("name="));
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("The Big  Map!"), "the-big-map");
        assert_eq!(slugify("  --NPCs & Factions-- "), "npcs-factions");
        assert_eq!(slugify("Čarodějnice"), "carodejnice");
        assert_eq!(slugify("???"), "");
    }
}
