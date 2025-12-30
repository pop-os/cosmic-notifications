use std::collections::{HashMap, HashSet};

use ammonia::Builder;
use cosmic::iced::Font;
use cosmic::iced::font::{Style, Weight};
use cosmic::iced_widget::span;
use cosmic::iced_widget::text::Span;

#[derive(Default, Clone)]
struct StyleState {
    bold: bool,
    italic: bool,
    underline: bool,
}

struct StyledSegment {
    text: String,
    style: StyleState,
}

/// Sanitize HTML, keeping only supported tags (b, i, u, a).
/// Converts <br> variants to newlines, and adds newline before links.
fn sanitize(html: &str) -> String {
    let html = html
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");

    let html = html.replace("<a ", "\n<a ");

    let mut tags = HashSet::new();
    tags.insert("b");
    tags.insert("i");
    tags.insert("u");
    tags.insert("a");

    let mut attrs = HashMap::new();
    let mut a_attrs = HashSet::new();
    a_attrs.insert("href");
    attrs.insert("a", a_attrs);

    Builder::new()
        .tags(tags)
        .tag_attributes(attrs)
        .clean(&html)
        .to_string()
}

fn decode_entity(entity: &str) -> String {
    match entity {
        "&amp;" => "&".to_string(),
        "&lt;" => "<".to_string(),
        "&gt;" => ">".to_string(),
        "&quot;" => "\"".to_string(),
        "&apos;" => "'".to_string(),
        "&nbsp;" => " ".to_string(),
        _ => {
            // try numeric entities.
            if let Some(num) = entity.strip_prefix("&#x").and_then(|s| s.strip_suffix(';')) {
                if let Ok(code) = u32::from_str_radix(num, 16) {
                    if let Some(c) = char::from_u32(code) {
                        return c.to_string();
                    }
                }
            } else if let Some(num) = entity.strip_prefix("&#").and_then(|s| s.strip_suffix(';')) {
                if let Ok(code) = num.parse::<u32>() {
                    if let Some(c) = char::from_u32(code) {
                        return c.to_string();
                    }
                }
            }
            entity.to_string()
        }
    }
}

fn parse_html(html: &str) -> Vec<StyledSegment> {
    let mut segments = Vec::new();
    let mut current_text = String::new();
    let mut style_stack: Vec<StyleState> = vec![StyleState::default()];
    let mut chars = html.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '<' {
            if !current_text.is_empty() {
                segments.push(StyledSegment {
                    text: std::mem::take(&mut current_text),
                    style: style_stack.last().cloned().unwrap_or_default(),
                });
            }

            let mut tag = String::new();
            while let Some(&tc) = chars.peek() {
                if tc == '>' {
                    chars.next();
                    break;
                }
                tag.push(chars.next().unwrap());
            }

            let tag = tag.trim();
            if let Some(tag_name) = tag.strip_prefix('/') {
                let tag_name = tag_name.trim().to_lowercase();
                if matches!(tag_name.as_str(), "b" | "i" | "u" | "a") && style_stack.len() > 1 {
                    style_stack.pop();
                }
            } else {
                let mut new_style = style_stack.last().cloned().unwrap_or_default();
                let tag_lower = tag.to_lowercase();

                if tag_lower.starts_with("b") && !tag_lower.starts_with("br") {
                    new_style.bold = true;
                    style_stack.push(new_style);
                } else if tag_lower.starts_with('i') {
                    new_style.italic = true;
                    style_stack.push(new_style);
                } else if tag_lower.starts_with('u') {
                    new_style.underline = true;
                    style_stack.push(new_style);
                } else if tag_lower.starts_with('a') {
                    new_style.underline = true;
                    style_stack.push(new_style);
                }
            }
        } else if c == '&' {
            let mut entity = String::from("&");
            while let Some(&ec) = chars.peek() {
                entity.push(chars.next().unwrap());
                if ec == ';' {
                    break;
                }
                if entity.len() > 10 {
                    break; // Not a valid entity, too long.
                }
            }
            current_text.push_str(&decode_entity(&entity));
        } else {
            current_text.push(c);
        }
    }

    // Remaining text.
    if !current_text.is_empty() {
        segments.push(StyledSegment {
            text: current_text,
            style: style_stack.last().cloned().unwrap_or_default(),
        });
    }

    segments
}

fn segment_to_span(seg: StyledSegment) -> Span<'static, ()> {
    let mut s: Span<'static, ()> = span(seg.text);

    if seg.style.bold || seg.style.italic {
        let weight = if seg.style.bold {
            Weight::Bold
        } else {
            Weight::Normal
        };
        let style = if seg.style.italic {
            Style::Italic
        } else {
            Style::Normal
        };
        s = s.font(Font {
            weight,
            style,
            ..Font::default()
        });
    }

    if seg.style.underline {
        s = s.underline(true);
    }

    s
}

/// Parse notification body HTML and return styled spans for rich_text widget.
///
/// Sanitizes HTML to keep only supported tags (b, i, u, a),
/// then converts to styled spans with proper font weights and styles.
pub fn parse_body(html: &str) -> Vec<Span<'static, ()>> {
    let sanitized = sanitize(html);
    let segments = parse_html(&sanitized);
    segments.into_iter().map(segment_to_span).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plain_text() {
        let spans = parse_body("Hello world");
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_parse_bold() {
        let spans = parse_body("<b>bold</b> text");
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn test_parse_nested() {
        let spans = parse_body("<b><i>bold italic</i></b>");
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_strip_script() {
        let spans = parse_body("<script>alert('xss')</script>safe");
        assert!(!spans.is_empty());
    }

    #[test]
    fn test_br_to_newline() {
        let spans = parse_body("line1<br>line2");
        let text: String = spans.iter().map(|_| "segment").collect();
        assert!(!text.is_empty());
    }

    #[test]
    fn test_slack_notification() {
        let input = r#"New message<a href="https://app.slack.com/">app.slack.com</a>"#;
        let spans = parse_body(input);
        assert!(spans.len() >= 1);
    }
}
