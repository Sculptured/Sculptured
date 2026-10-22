//! How long a browser may keep the web app's files: the bundles Vite names by content forever, the
//! brand images a day, and the page itself only after checking it is current, so a deploy shows at once.
use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;

pub fn cache_control(path: &str) -> &'static str {
    if path.starts_with("/assets/") {
        "public, max-age=31536000, immutable"
    } else if path.starts_with("/brand/") {
        "public, max-age=86400"
    } else {
        "no-cache"
    }
}

pub async fn layer(req: Request, next: Next) -> Response {
    let rule = cache_control(req.uri().path());
    let mut res = next.run(req).await;
    if res.status().is_success() && !res.headers().contains_key(header::CACHE_CONTROL) {
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static(rule));
    }
    res
}

#[cfg(test)]
mod tests {

    #[test]
    fn bundles_are_kept_images_a_day_and_the_page_is_checked() {
        assert_eq!(
            crate::web_cache::cache_control("/assets/index-CtmLHbQF.js"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            crate::web_cache::cache_control("/brand/sculptured-coastal-emblem-256.png"),
            "public, max-age=86400"
        );
        assert_eq!(crate::web_cache::cache_control("/"), "no-cache");
        assert_eq!(
            crate::web_cache::cache_control("/team/x/live"),
            "no-cache",
            "a deep link serves the page"
        );
    }
}
