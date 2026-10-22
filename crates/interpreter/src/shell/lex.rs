//! Quote-aware shell words and command segments, without expansion.
/// Words of `s`, with simple quote handling (no expansion).
fn words(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut quote, mut quoted) =
        (Vec::new(), String::new(), None::<char>, false);
    let matching_s = s.chars();
    for c in matching_s {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                quoted = true;
            }
            None if c.is_whitespace() => {
                if !cur.is_empty() || quoted {
                    out.push(std::mem::take(&mut cur));
                    quoted = false;
                }
            }
            None => cur.push(c),
        }
    }
    if !cur.is_empty() || quoted {
        out.push(cur);
    }
    out
}

/// `/bin/bash -lc 'X'` (how Codex runs commands) is `X`.
#[must_use]
pub fn unwrap_shell(cmd: &str) -> String {
    let w = words(cmd);
    let base = |s: &str| s.rsplit('/').next().unwrap_or(s).to_string();
    if w.len() >= 3
        && matches!(base(&w[0]).as_str(), "bash" | "sh" | "zsh")
        && matches!(w[1].as_str(), "-c" | "-lc" | "-cl")
    {
        return w[2].clone();
    }
    cmd.to_string()
}

/// One simple command: its words after env assignments and wrappers, and whether it ran under sudo.
pub(super) struct Seg {
    pub(super) words: Vec<String>,
    pub(super) sudo: bool,
}

fn seg(text: &str) -> Option<Seg> {
    let mut w = words(text);
    let mut sudo = false;
    loop {
        let first = w.first()?;
        let assignment = first.split_once('=').is_some_and(|(k, _)| {
            !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        if assignment
            || matches!(
                first.as_str(),
                "time" | "nohup" | "env" | "command" | "exec" | "nice"
            )
        {
            w.remove(0);
        } else if first == "sudo" {
            sudo = true;
            w.remove(0);
        } else {
            break;
        }
    }
    Some(Seg { words: w, sudo })
}

/// Simple commands split on `&&`, `||`, `;`, `|` and newlines, outside quotes.
pub(super) fn segments(cmd: &str) -> Vec<Seg> {
    let mut chars = cmd.chars().peekable();
    let (mut parts, mut current, mut quote) = (Vec::new(), String::new(), None);
    while let Some(character) = chars.next() {
        match quote {
            Some(end) => {
                if character == end {
                    quote = None;
                }
                current.push(character);
            }
            None if matches!(character, '\'' | '"') => {
                quote = Some(character);
                current.push(character);
            }
            None if matches!(character, ';' | '\n' | '|')
                || (character == '&' && chars.peek() == Some(&'&')) =>
            {
                parts.push(std::mem::take(&mut current));
                if matches!(character, '|' | '&') && chars.peek() == Some(&character) {
                    chars.next();
                }
            }
            None => current.push(character),
        }
    }
    parts.push(current);
    parts.iter().filter_map(|part| seg(part)).collect()
}
