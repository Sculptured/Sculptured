//! Caps on agent- and runtime-supplied text, shared by hostd (captured events) and the model's own
//! `capped()` helpers, so a tool input is bounded the same way wherever it is stored.

/// Text, tool output, stderr and unrecognized payloads are capped at this many bytes.
pub const MAX_TEXT: usize = 64 * 1024;

/// The longest start of `s` that is at most `max` bytes, cut on a char boundary.
#[must_use]
pub fn head(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// The longest end of `s` that is at most `max` bytes, cut on a char boundary.
#[must_use]
pub fn tail(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

/// For `s` longer than `max` bytes: about its first half and last three eighths of `max`, and how many
/// bytes lie between them. None when `s` fits.
#[must_use]
pub fn middle(s: &str, max: usize) -> Option<(&str, usize, &str)> {
    if s.len() <= max {
        return None;
    }
    let (start, end) = (head(s, max / 2), tail(s, max * 3 / 8));
    Some((start, s.len() - start.len() - end.len(), end))
}

/// Cuts `s` to at most `max` bytes on a char boundary and says how much was dropped.
#[must_use]
pub fn truncate(s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let kept = head(&s, max).len();
    let dropped = s.len() - kept;
    let mut out = s;
    out.truncate(kept);
    out.push_str(&format!("\n[truncated {dropped} bytes]"));
    out
}

/// Per-string budget used when a tool input is over `MAX_TEXT` as a whole.
pub const MAX_INPUT_LEAF: usize = 4 * 1024;

fn json_len(v: &serde_json::Value) -> usize {
    serde_json::to_string(v)
        .map(|s| s.len())
        .unwrap_or(usize::MAX)
}

fn cap_leaves(
    v: serde_json::Value,
    max: usize,
    cut: fn(String, usize) -> String,
) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::String(s) => Value::String(cut(s, max)),
        Value::Array(a) => Value::Array(a.into_iter().map(|x| cap_leaves(x, max, cut)).collect()),
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, x)| (k, cap_leaves(x, max, cut)))
                .collect(),
        ),
        other => other,
    }
}

/// Caps a tool input at about `MAX_TEXT` serialized bytes. Long string leaves are
/// truncated (with the usual marker) and the object structure is kept, so keys and
/// fields such as `command` stay readable. Only an input with so many leaves that
/// even 256-byte leaves exceed the cap becomes a single truncated JSON string.
pub fn cap_input(input: serde_json::Value) -> serde_json::Value {
    cap_input_with(input, truncate)
}

/// Caps the tool input of a permission or approval prompt like `cap_input`, but a long string is cut in
/// the middle (`approval_cut`), so the end of a command (where `; rm -rf ~/work` would sit) stays in view.
/// Inputs up to `MAX_TEXT` are kept whole, so the full command stays in the evidence; above that the
/// middle is kept nowhere, and the marker says so (`approval_cut_unstored`).
pub fn cap_approval_input(input: serde_json::Value) -> serde_json::Value {
    cap_input_with(input, approval_cut_unstored)
}

/// Whether `cap_approval_input` keeps this input whole.
#[must_use]
pub fn approval_input_kept_whole(input: &serde_json::Value) -> bool {
    json_len(input) <= MAX_TEXT
}

fn cap_input_with(input: serde_json::Value, cut: fn(String, usize) -> String) -> serde_json::Value {
    if json_len(&input) <= MAX_TEXT {
        return input;
    }
    let mut budget = MAX_INPUT_LEAF;
    let mut capped = cap_leaves(input.clone(), budget, cut);
    while json_len(&capped) > MAX_TEXT && budget > 256 {
        budget /= 4;
        capped = cap_leaves(input.clone(), budget, cut);
    }
    if json_len(&capped) > MAX_TEXT {
        return serde_json::Value::String(cut(input.to_string(), MAX_TEXT));
    }
    capped
}

/// Cuts a permission or approval prompt's text (or one of its tool-input strings) longer than `max`
/// bytes in the middle: about the first half and the last three eighths of `max` stay, with a marker
/// between them naming how much is hidden. Never cut at the end, where a dangerous tail would vanish.
/// For text whose full form is stored elsewhere (the prompt's whole tool input).
#[must_use]
pub fn approval_cut(s: String, max: usize) -> String {
    middle_cut(s, max, |n| {
        format!("[{n} bytes not shown, read the full command before approving]")
    })
}

/// Like `approval_cut`, for text whose middle is stored nowhere (a command over the tool-input cap).
#[must_use]
pub fn approval_cut_unstored(s: String, max: usize) -> String {
    middle_cut(s, max, |n| {
        format!(
            "[{n} bytes not stored; this command is too long to show in full, deny it unless you trust the agent]"
        )
    })
}

fn middle_cut(s: String, max: usize, marker: impl Fn(usize) -> String) -> String {
    match middle(&s, max) {
        None => s,
        Some((start, hidden, end)) => format!("{start}\n{}\n{end}", marker(hidden)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_tail_and_middle_cut_on_char_boundaries() {
        let s = "ab€cd"; // € is 3 bytes: a b [€€€] c d
        assert_eq!(head(s, 3), "ab");
        assert_eq!(tail(s, 3), "cd");
        assert_eq!((head(s, 99), tail(s, 99)), (s, s));
        let long = "x".repeat(100);
        let (start, hidden, end) = middle(&long, 16).unwrap();
        assert_eq!((start.len(), hidden, end.len()), (8, 86, 6));
        assert!(middle("short", 16).is_none());
    }
}
