//! `read_pdf`: a PDF's text, page by page, read on the agent's own computer. Text costs a small
//! fraction of what page images do, and an image stays in an agent's context for every later step,
//! so agents read PDFs with this first and look at a page as an image only when it has no text.
use serde_json::{Value, json};

/// Most text one call returns, in bytes; the rest is a later call away.
pub const BUDGET: usize = 60_000;
/// Fewer characters than this on a page means it is a scan or a drawing.
const BLANK: usize = 25;

pub fn tool() -> Value {
    json!({
        "name": "read_pdf",
        "description": "Read a PDF's text, page by page. Use this before opening a PDF any other way: text costs a small fraction of page images, and an image stays in your context for every later step. Pages with no text (scans, drawings) are named, so you can look at only those as images, and only if you need them. `pages` is optional, e.g. \"5\" or \"10-20\".",
        "inputSchema": { "type": "object", "properties": { "path": { "type": "string" }, "pages": { "type": "string" } }, "required": ["path"] }
    })
}

pub fn read(args: &Value) -> Result<Value, String> {
    let path = args["path"]
        .as_str()
        .filter(|p| !p.trim().is_empty())
        .ok_or("give the PDF's `path`")?;
    let full = std::env::current_dir()
        .map(|d| d.join(path))
        .unwrap_or_else(|_| path.into());
    let bytes = std::fs::read(&full).map_err(|e| format!("cannot read {}: {e}", full.display()))?;
    // Defense in depth for bugs in the third-party PDF parser, which already returns Result.
    // No shared state or locks cross this boundary. Correctness and memory safety do not depend
    // on catching a panic; with panic=abort the ordinary parser error path still applies.
    let pages = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(&bytes))
        .map_err(|_| "this PDF could not be read as text; look at its pages instead".to_string())?
        .map_err(|e| {
            format!("this PDF could not be read as text ({e}); look at its pages instead")
        })?;
    let (first, last) = range(args["pages"].as_str(), pages.len())?;
    Ok(json!({ "text": render(path, &pages, first, last) }))
}

/// `"5"` or `"10-20"`, 1-based and inclusive; all pages when absent.
fn range(pages: Option<&str>, total: usize) -> Result<(usize, usize), String> {
    let bad = || {
        format!(
            "`pages` must be a page or a range within the PDF's {total} pages, e.g. \"3\" or \"2-5\""
        )
    };
    let Some(p) = pages.map(str::trim).filter(|p| !p.is_empty()) else {
        return Ok((1, total.max(1)));
    };
    let (a, b) = p.split_once('-').unwrap_or((p, p));
    let (a, b): (usize, usize) = (
        a.trim().parse().map_err(|_| bad())?,
        b.trim().parse().map_err(|_| bad())?,
    );
    if a == 0 || a > b || b > total {
        return Err(bad());
    }
    Ok((a, b))
}

/// The pages' text under a heading each, until the budget; then where to continue.
pub fn render(name: &str, pages: &[String], first: usize, last: usize) -> String {
    let mut out = format!(
        "{name}: {} pages. Text of pages {first}-{last}:\n",
        pages.len()
    );
    let mut blank = Vec::new();
    for n in first..=last {
        let text = pages[n - 1].trim();
        if text.chars().count() < BLANK {
            blank.push(n);
            continue;
        }
        if out.len() + text.len() > BUDGET {
            out.push_str(&format!(
                "\n[Stopped before page {n} to keep this short: ask for pages {n}-{last} next.]\n"
            ));
            break;
        }
        out.push_str(&format!("\n--- Page {n} ---\n{text}\n"));
    }
    if !blank.is_empty() {
        let list = blank
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("\nNo text on page {list} (a scan or a drawing). Look at those pages as images only if you need what is on them.\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/text-then-blank.pdf").to_string()
    }

    #[test]
    fn gives_each_pages_text_and_names_the_pages_without_any() {
        let out = read(&json!({ "path": fixture() })).unwrap();
        let text = out["text"].as_str().unwrap();
        assert!(text.contains("2 pages"), "{text}");
        assert!(
            text.contains("--- Page 1 ---") && text.contains("Cessna 172 fuel system"),
            "{text}"
        );
        assert!(text.contains("No text on page 2"), "{text}");
    }

    #[test]
    fn reads_only_the_pages_asked_for() {
        let out = read(&json!({ "path": fixture(), "pages": "2" })).unwrap();
        let text = out["text"].as_str().unwrap();
        assert!(!text.contains("Cessna"), "{text}");
        assert!(text.contains("No text on page 2"), "{text}");
    }

    #[test]
    fn says_what_is_wrong_with_a_bad_request() {
        assert!(read(&json!({})).unwrap_err().contains("path"));
        assert!(
            read(&json!({ "path": "/no/such.pdf" }))
                .unwrap_err()
                .contains("cannot read")
        );
        assert!(
            read(&json!({ "path": fixture(), "pages": "9" }))
                .unwrap_err()
                .contains("2 pages")
        );
    }

    #[test]
    fn a_long_document_stops_at_the_budget_and_says_where_to_continue() {
        let pages: Vec<String> = (0..10)
            .map(|i| format!("page {i} ").repeat(3_000))
            .collect();
        let text = render("big.pdf", &pages, 1, 10);
        assert!(text.len() < BUDGET + 2_000, "{}", text.len());
        assert!(
            text.contains("ask for pages"),
            "{}",
            &text[text.len() - 300..]
        );
    }
}
