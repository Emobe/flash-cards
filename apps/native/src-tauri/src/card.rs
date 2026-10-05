//! The `card` URI scheme. It serves the card-frame page and nothing else, so that the page gets
//! its own Content-Security-Policy instead of the app's (ADR 0005, findings 6 and 7).

use std::borrow::Cow;

use tauri::http::{Response, StatusCode, header};

/// The trusted card-frame page, shared with the web build (`packages/ui/src/card/frame.html`).
const FRAME_HTML: &str = include_str!("../../../../packages/ui/src/card/frame.html");

/// Must equal the policy in `frame.html`'s `<meta>` (a test checks). The header adds `sandbox`, so
/// the page stays sandboxed even if something loads it outside the card iframe.
const FRAME_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
img-src blob: data:; media-src blob: data:; font-src blob: data:";

pub fn respond(path: &str) -> Response<Cow<'static, [u8]>> {
    let builder = Response::builder().header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    let response = if path == "/frame.html" {
        builder
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(
                header::CONTENT_SECURITY_POLICY,
                format!("{FRAME_CSP}; sandbox allow-scripts"),
            )
            .body(Cow::Borrowed(FRAME_HTML.as_bytes()))
    } else {
        builder
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Cow::Borrowed(&b"Not found"[..]))
    };
    response.expect("static response parts are valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_the_frame_page_with_a_sandboxing_csp() {
        let response = respond("/frame.html");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        let csp = response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        assert!(csp.ends_with("; sandbox allow-scripts"));
        assert!(csp.starts_with("default-src 'none'"));
        assert!(!csp.contains("connect-src"));
        assert!(response.body().starts_with(b"<!doctype html>"));
    }

    #[test]
    fn serves_nothing_else() {
        for path in [
            "/",
            "/index.html",
            "/frame.html/",
            "/../frame.html",
            "/sample.png",
        ] {
            assert_eq!(respond(path).status(), StatusCode::NOT_FOUND, "{path}");
        }
    }

    #[test]
    fn the_header_policy_matches_the_page_meta_policy() {
        let meta = format!("http-equiv=\"Content-Security-Policy\" content=\"{FRAME_CSP}\"");
        assert!(FRAME_HTML.contains(&meta));
    }
}
