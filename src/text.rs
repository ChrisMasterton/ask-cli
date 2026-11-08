//! Shared UTF-8 byte-boundary handling for prompt data and tool descriptions.

pub(crate) fn truncate_utf8_bytes(value: &str, max_bytes: usize) -> (&str, bool) {
    if value.len() <= max_bytes {
        return (value, false);
    }

    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    (&value[..end], true)
}
