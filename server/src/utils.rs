use std::borrow::Cow;

pub fn html_escape(input: &str) -> Cow<'_, str> {
    let mut needs_escaping = false;
    for ch in input.chars() {
        match ch {
            '&' | '<' | '>' | '"' | '\'' => {
                needs_escaping = true;
                break;
            }
            _ => {}
        }
    }
    if !needs_escaping {
        return Cow::Borrowed(input);
    }
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    Cow::Owned(escaped)
}
