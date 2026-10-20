//! Which `/api/community/*` requests the proxy forwards (C3 table, R5): exactly these methods and
//! paths, `*` standing for one segment of `[A-Za-z0-9_-]{1,100}`. Community's old cookie sign-in
//! routes (`login`, `callback`, `logout`, `sessions/revoke`) are not on it.
use axum::http::Method;

const ALLOWED: &[(&str, &str)] = &[
    ("GET", "me"), ("PATCH", "me"), ("PUT", "me/avatar"), ("DELETE", "me/avatar"),
    ("GET", "people"), ("GET", "users/*"), ("GET", "users/*/avatar"), ("POST", "users/*/report"), ("POST", "users/*/moderate"),
    ("GET", "blocks"), ("POST", "blocks"), ("GET", "blocks/*"), ("DELETE", "blocks/*"),
    ("POST", "blocks/*/save"), ("DELETE", "blocks/*/save"), ("POST", "blocks/*/report"), ("POST", "blocks/*/moderate"),
    ("GET", "saved"), ("GET", "mine"), ("GET", "reports"),
];

fn segment(s: &str) -> bool {
    (1..=100).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// `path` is what follows `/api/community/`, still percent-encoded and without the query, so an
/// encoded slash, a dot segment or an empty segment never matches.
pub fn allowed(method: &Method, path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.iter().all(|p| segment(p))
        && ALLOWED.iter().any(|(m, pattern)| {
            let want: Vec<&str> = pattern.split('/').collect();
            *m == method.as_str() && want.len() == parts.len() && want.iter().zip(&parts).all(|(w, p)| *w == "*" || w == p)
        })
}

#[cfg(test)]
mod tests {
    use super::allowed;
    use axum::http::Method;

    #[test]
    fn segments_match_exactly() {
        assert!(allowed(&Method::GET, "users/ana-lee/avatar") && allowed(&Method::DELETE, "blocks/b_1/save"));
        for p in ["users/a%2Fb", "users/..", "users/", "", "blocks/a/b/c", "users/a.b", &"x".repeat(101)] {
            assert!(!allowed(&Method::GET, p), "{p}");
        }
        assert!(!allowed(&Method::HEAD, "me") && !allowed(&Method::POST, "me"));
    }
}
