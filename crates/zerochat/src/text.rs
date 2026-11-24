//! Text cut to a byte budget without splitting a UTF-8 character.

/// The longest start of `text` that fits in `max` bytes.
pub fn truncate_utf8(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    &text[..cut]
}

/// The longest end of `text` that fits in `max` bytes.
pub fn tail_utf8(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let cut = text.len() - max;
    let cut = (cut..text.len())
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(text.len());
    &text[cut..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_fall_on_character_boundaries() {
        assert_eq!(truncate_utf8("hello", 10), "hello");
        assert_eq!(truncate_utf8("hello", 3), "hel");
        assert_eq!(
            truncate_utf8("aé", 2),
            "a",
            "é is two bytes: never half of it"
        );
        assert_eq!(tail_utf8("hello", 10), "hello");
        assert_eq!(tail_utf8("hello", 3), "llo");
        assert_eq!(tail_utf8("éa", 2), "a");
    }
}
