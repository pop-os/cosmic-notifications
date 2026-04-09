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
        // handle break lines
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");

    text.to_owned()
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
    }

    buffer
}
