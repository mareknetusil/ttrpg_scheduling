//! Markdown → sanitized HTML (DESIGN §7). Templates only ever get the output of `render`, as
//! `*_html` fields marked `|safe`; raw Markdown is never marked safe.

use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, TagEnd, TextMergeStream, html};

pub fn render(source: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    // TextMergeStream joins adjacent text events, so "[!info] Title" arrives as one piece
    let events: Vec<Event> = TextMergeStream::new(Parser::new_ext(source, options)).collect();
    let mut unsafe_html = String::new();
    html::push_html(&mut unsafe_html, callouts(events).into_iter());

    // Task lists render as `<input disabled type="checkbox">`. Allow exactly that input and
    // nothing else, so raw HTML in the Markdown can't add text fields.
    ammonia::Builder::default()
        .add_tags(["input"])
        .add_tag_attributes("input", ["type", "checked", "disabled"])
        .add_tag_attributes("code", ["class"])
        .add_tag_attributes("div", ["class"])
        .add_tag_attributes("p", ["class"])
        .add_tag_attributes("details", ["class", "open"])
        .add_tag_attributes("summary", ["class"])
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("input", "type") => (value == "checkbox").then_some(value.into()),
            // Only the callout classes, so raw HTML can't borrow the site's own classes (e.g.
            // the fixed-position `.toast`)
            ("div" | "p" | "details" | "summary", "class") => value
                .split_whitespace()
                .all(|class| class.starts_with("callout"))
                .then_some(value.into()),
            _ => Some(value.into()),
        })
        .set_tag_attribute_value("input", "disabled", "")
        .clean(&unsafe_html)
        .to_string()
}

/// Obsidian-style callouts. A blockquote whose first line is `[!type] Optional title`:
///
/// ```markdown
/// > [!warning] Bring snacks
/// > The shop is closed on Sundays.
/// ```
///
/// becomes `<div class="callout callout--warning">` with a `<p class="callout__title">` and the
/// rest of the blockquote as its content. `[!type]-` / `[!type]+` make it collapsible
/// (`<details>`, closed / open). Known types: info, warning, todo; any other type looks like
/// info. Without a title, the type's name is the title ("Warning").
fn callouts(events: Vec<Event<'_>>) -> Vec<Event<'_>> {
    let mut out = Vec::with_capacity(events.len());
    // One entry per blockquote we're inside: its closing HTML if it became a callout
    let mut open: Vec<Option<&'static str>> = Vec::new();
    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            Event::Start(Tag::BlockQuote(_)) => {
                let header = match (events.get(i + 1), events.get(i + 2)) {
                    (Some(Event::Start(Tag::Paragraph)), Some(Event::Text(text))) => {
                        CalloutHeader::parse(text)
                    }
                    _ => None,
                };
                let Some(header) = header else {
                    open.push(None);
                    out.push(events[i].clone());
                    i += 1;
                    continue;
                };

                let (start, title_tag, title_end, end) = if let Some(expanded) = header.fold {
                    let open_attr = if expanded { " open" } else { "" };
                    (
                        format!(
                            "<details class=\"callout callout--{}\"{open_attr}>",
                            header.kind
                        ),
                        "<summary class=\"callout__title\">",
                        "</summary>",
                        "</details>",
                    )
                } else {
                    (
                        format!("<div class=\"callout callout--{}\">", header.kind),
                        "<p class=\"callout__title\">",
                        "</p>",
                        "</div>",
                    )
                };
                out.push(Event::Html(start.into()));
                out.push(Event::Html(title_tag.into()));
                let title_start = out.len();
                if !header.title.is_empty() {
                    out.push(Event::Text(header.title.into()));
                }
                // The rest of the title line may hold formatting: `[!info] The **big** day`
                let mut j = i + 3;
                while let Some(event) = events.get(j) {
                    if matches!(
                        event,
                        Event::SoftBreak | Event::HardBreak | Event::End(TagEnd::Paragraph)
                    ) {
                        break;
                    }
                    out.push(event.clone());
                    j += 1;
                }
                if out.len() == title_start {
                    out.push(Event::Text(header.default_title.into()));
                }
                out.push(Event::Html(title_end.into()));
                match events.get(j) {
                    // The first paragraph was only the title line: drop it
                    Some(Event::End(TagEnd::Paragraph)) => {}
                    // The paragraph goes on after the title line: reopen it for the content
                    _ => out.push(Event::Start(Tag::Paragraph)),
                }
                open.push(Some(end));
                i = j + 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                match open.pop().flatten() {
                    Some(end) => out.push(Event::Html(end.into())),
                    None => out.push(events[i].clone()),
                }
                i += 1;
            }
            event => {
                out.push(event.clone());
                i += 1;
            }
        }
    }
    out
}

/// The `[!type]± title` line of a callout.
struct CalloutHeader {
    /// The CSS modifier: "info", "warning" or "todo"
    kind: &'static str,
    /// The type as written, capitalized: the title when none is given
    default_title: String,
    /// `None`: not collapsible; `Some(true)`: collapsible, starts open (`+`)
    fold: Option<bool>,
    title: String,
}

impl CalloutHeader {
    fn parse(text: &CowStr) -> Option<CalloutHeader> {
        let rest = text.strip_prefix("[!")?;
        let (name, rest) = rest.split_once(']')?;
        let valid = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
        if name.is_empty() || !name.chars().all(valid) {
            return None;
        }
        let name = name.to_ascii_lowercase();
        let (fold, rest) = match rest.strip_prefix('-') {
            Some(rest) => (Some(false), rest),
            None => match rest.strip_prefix('+') {
                Some(rest) => (Some(true), rest),
                None => (None, rest),
            },
        };
        let kind = match name.as_str() {
            "warning" => "warning",
            "todo" => "todo",
            _ => "info",
        };
        let mut default_title = name.replace(['-', '_'], " ");
        default_title[..1].make_ascii_uppercase();
        Some(CalloutHeader {
            kind,
            default_title,
            fold,
            // trim_start only: "Bring **snacks**" needs the space before the bold part
            title: rest.trim_start().to_string(),
        })
    }
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
    fn callouts() {
        let html = render("> [!warning] Bring **snacks**\n> The shop is closed.\n>\n> - chips");
        assert_eq!(
            html,
            "<div class=\"callout callout--warning\"><p class=\"callout__title\">Bring <strong>snacks</strong></p>\n<p>The shop is closed.</p>\n<ul>\n<li>chips</li>\n</ul>\n</div>"
        );
        // no title: the type is the title; unknown types look like info
        let html = render("> [!TODO]\n> Buy dice");
        assert!(html.starts_with("<div class=\"callout callout--todo\"><p class=\"callout__title\">Todo</p>\n<p>Buy dice</p>"));
        let html = render("> [!house-rule] Crits");
        assert!(html.contains("callout--info"), "{html}");
        assert!(html.contains("Crits</p></div>"), "{html}");
        let html = render("> [!house-rule]");
        assert!(html.contains(">House rule</p>"), "{html}");
        // collapsible
        let html = render("> [!info]- Spoilers\n> The butler did it.");
        assert!(html.starts_with("<details class=\"callout callout--info\"><summary class=\"callout__title\">Spoilers</summary>"), "{html}");
        assert!(!html.contains(" open"));
        assert!(render("> [!info]+ Open").contains(" open"));
        // nested, and plain blockquotes stay blockquotes
        let html = render("> [!info] Outer\n> > [!todo] Inner\n> > x\n\n> just a quote");
        assert_eq!(html.matches("</div>").count(), 2, "{html}");
        assert!(
            html.contains("<blockquote>\n<p>just a quote</p>\n</blockquote>"),
            "{html}"
        );
        // not a callout
        assert!(render("> [!] x").contains("<blockquote>"));
        assert!(render("> [!a b] x").contains("<blockquote>"));
    }

    #[test]
    fn callout_classes_cant_be_abused() {
        // a raw type can't escape the class attribute, and raw HTML keeps only callout classes
        let html = render("> [!info\"onclick=x] t");
        assert!(
            html.starts_with("<blockquote>"),
            "not a valid type, so not a callout"
        );
        let html =
            render("<div class=\"toast\">x</div><div class=\"callout callout--todo\">y</div>");
        assert!(!html.contains("toast"));
        assert!(html.contains("callout callout--todo"));
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("The Big  Map!"), "the-big-map");
        assert_eq!(slugify("  --NPCs & Factions-- "), "npcs-factions");
        assert_eq!(slugify("Čarodějnice"), "carodejnice");
        assert_eq!(slugify("???"), "");
    }
}
