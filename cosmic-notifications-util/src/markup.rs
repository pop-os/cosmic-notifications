use cosmic::{
    cosmic_theme,
    iced::core::text::Span,
    iced::{
        Font,
        font::{Style, Weight},
    },
};

// Handle break lines, etc. in the future
// Used only in `parse_html` function
fn _prepare_html(text: &str) -> String {
    let text = text
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        // handle break lines
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");

    text.to_owned()
}

fn collect_text(handle: &tl::NodeHandle, parser: &tl::Parser, buffer: &mut String) {
    if let Some(node) = handle.get(parser) {
        match node {
            tl::Node::Tag(tag) => {
                tag.children().top().iter().for_each(|t| {
                    collect_text(t, parser, buffer);
                });
            }
            tl::Node::Raw(bytes) => {
                buffer.push_str(&bytes.as_utf8_str());
            }
            _ => {}
        }
    }
}

pub fn strip_html(text: &str) -> String {
    if let Ok(dom) = tl::parse(text, tl::ParserOptions::default()) {
        let parser = dom.parser();
        let mut buffer = String::new();
        for node_handle in dom.children() {
            collect_text(node_handle, parser, &mut buffer);
        }
        buffer
    } else {
        text.to_string()
    }
}

// Sanitize only tags allowed by Freedesktop Notification Specifications
// https://specifications.freedesktop.org/notification/1.2/markup.html
// TODO: impl <img> tag handling
fn sanitize_html(tags: &[String], content: &str) -> Span<'static> {
    let mut font = Font::default();
    let mut span = Span::new(content.to_owned());

    for tag in tags {
        match tag.as_str() {
            "b" => font.weight = Weight::Bold,
            "i" => font.style = Style::Italic,
            "u" => span = span.underline(true),
            "a" => {
                let theme = cosmic_theme::Theme::preferred_theme();
                span = span.underline(true).color(theme.accent_text_color());
            }
            _ => {}
        }
    }

    span.font(font)
}

fn _handle_recursive(
    handle: &tl::NodeHandle,
    parser: &tl::Parser,
    tags: &mut Vec<String>,
    buffer: &mut Vec<Span<'static>>,
) {
    if let Some(node) = handle.get(parser) {
        match node {
            tl::Node::Tag(tag) => {
                let tag_name = tag.name().as_utf8_str();
                tags.push(tag_name.into_owned());

                tag.children().top().iter().for_each(|t| {
                    _handle_recursive(t, parser, tags, buffer);
                });

                tags.pop();
            }
            tl::Node::Raw(bytes) => {
                buffer.push(sanitize_html(tags, &bytes.as_utf8_str()));
            }
            _ => {}
        }
    }
}

pub fn html_to_spans(text: &str) -> Vec<Span<'static>> {
    let mut buffer = Vec::new();
    let html = _prepare_html(text);
    let dom = tl::parse(&html, tl::ParserOptions::default());

    if let Ok(vdom) = dom {
        let parser = vdom.parser();
        let elements = vdom.children();
        let mut tags = Vec::new();

        for node_handle in elements {
            _handle_recursive(node_handle, parser, &mut tags, &mut buffer);
        }
    } else {
        buffer.push(Span::new(strip_html(text)));
    }

    buffer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html() {
        assert_eq!(strip_html("<b>hello</b>"), "hello");
        assert_eq!(strip_html("<i>hello</i>"), "hello");
        assert_eq!(strip_html("<u>hello</u>"), "hello");
        assert_eq!(strip_html("<a>hello</a>"), "hello");
        assert_eq!(strip_html("<p>hello</p>"), "hello");
        assert_eq!(strip_html("<b><i><u><a>hello</a></u></i></b>"), "hello");
        assert_eq!(strip_html("hello"), "hello");
        assert_eq!(strip_html(""), "");
        assert_eq!(strip_html("<b>invalid"), "invalid");
    }

    #[test]
    fn test_html_to_spans_simple() {
        let spans = html_to_spans("hello");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "hello");
        assert_eq!(spans[0].font.unwrap().weight, Weight::Normal);
        assert_eq!(spans[0].font.unwrap().style, Style::Normal);
    }

    #[test]
    fn test_html_to_spans_bold() {
        let spans = html_to_spans("<b>hello</b>");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "hello");
        assert_eq!(spans[0].font.unwrap().weight, Weight::Bold);
    }

    #[test]
    fn test_html_to_spans_italic() {
        let spans = html_to_spans("<i>hello</i>");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "hello");
        assert_eq!(spans[0].font.unwrap().style, Style::Italic);
    }

    #[test]
    fn test_html_to_spans_underline() {
        let spans = html_to_spans("<u>hello</u>");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "hello");
        assert!(spans[0].underline);
    }

    #[test]
    fn test_html_to_spans_escaped() {
        let spans = html_to_spans("&lt;b&gt;hello&lt;/b&gt;");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "hello");
        assert_eq!(spans[0].font.unwrap().weight, Weight::Bold);
    }

    #[test]
    fn test_html_to_spans_mixed() {
        let spans = html_to_spans("hello <b>world</b>");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "hello ");
        assert_eq!(spans[0].font.unwrap().weight, Weight::Normal);
        assert_eq!(spans[1].text, "world");
        assert_eq!(spans[1].font.unwrap().weight, Weight::Bold);
    }

    #[test]
    fn test_html_to_spans_br() {
        let spans = html_to_spans("hello<br>world");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "hello\nworld");
    }

    #[test]
    fn test_html_to_spans_unclosed_tag() {
        let spans = html_to_spans("<b hello");
        assert!(spans.is_empty());
    }

    #[test]
    fn test_html_to_spans_malformed_tag() {
        let spans = html_to_spans("<");
        assert!(spans.is_empty());
    }
}
