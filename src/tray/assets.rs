//! Embedded popover bundle and the `aiub://` protocol that serves it.
//!
//! The page drives privileged host commands, so the protocol answers only
//! requests for its own files: one scheme, one authority, `GET`, and an exact
//! path table. Everything else gets an empty error response; no request is
//! ever mapped onto the filesystem.
//!
//! Keep this file self-contained (no `super::`/`crate::` items):
//! `tests/macos_webview.rs` includes it by path to load the real bundle.

use std::borrow::Cow;

use wry::http::header::{
    ALLOW, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HeaderValue, X_CONTENT_TYPE_OPTIONS,
};
use wry::http::{Method, Request, Response, StatusCode};

const INDEX_HTML: &str = include_str!(concat!(env!("OUT_DIR"), "/popover/index.html"));
const POPOVER_CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/popover/popover.css"));
const POPOVER_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/popover/popover.js"));

/// The page lives at `aiub://localhost/index.html`. That origin also keys the
/// persisted WebView data store, so it must not change.
const SCHEME: &str = "aiub";
const AUTHORITY: &str = "localhost";

/// Only the bundle's own module script and stylesheet may load. Inline styles
/// stay allowed: React style props and the `<style>` elements Radix injects
/// need them. Fetch directives left out fall back to `default-src 'none'`, so
/// nothing is fetched, framed, or embedded; the last three do not fall back.
const POLICY: &str = concat!(
    "default-src 'none'; ",
    "script-src 'self'; ",
    "style-src 'self' 'unsafe-inline'; ",
    "base-uri 'none'; ",
    "form-action 'none'; ",
    "frame-ancestors 'none'",
);

/// Answers the WebView's `aiub` custom-protocol requests.
///
/// No CORS header: the page and its files share one origin, so granting any
/// other origin (let alone `*`) would only widen access.
pub fn response(request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let uri = request.uri();
    if uri.scheme_str() != Some(SCHEME)
        || uri.authority().map(|authority| authority.as_str()) != Some(AUTHORITY)
    {
        return refusal(StatusCode::FORBIDDEN);
    }
    if *request.method() != Method::GET {
        let mut response = refusal(StatusCode::METHOD_NOT_ALLOWED);
        response
            .headers_mut()
            .insert(ALLOW, HeaderValue::from_static("GET"));
        return response;
    }
    // WebKit never attaches a body to the page's own GETs.
    if !request.body().is_empty() {
        return refusal(StatusCode::BAD_REQUEST);
    }
    // Exact targets only: a query, a trailing slash or an encoded or dotted
    // variant is a different target, not a way into another file.
    let file = match uri.query() {
        None => embedded(uri.path()),
        Some(_) => None,
    };
    match file {
        Some((body, content_type)) => hardened(StatusCode::OK, content_type, body.as_bytes()),
        None => refusal(StatusCode::NOT_FOUND),
    }
}

/// The bundle by exact request path. `/` stays the document, as before.
fn embedded(path: &str) -> Option<(&'static str, &'static str)> {
    match path {
        "/" | "/index.html" => Some((INDEX_HTML, "text/html; charset=utf-8")),
        "/popover.css" => Some((POPOVER_CSS, "text/css; charset=utf-8")),
        "/popover.js" => Some((POPOVER_JS, "text/javascript; charset=utf-8")),
        _ => None,
    }
}

fn refusal(status: StatusCode) -> Response<Cow<'static, [u8]>> {
    hardened(status, "text/plain; charset=utf-8", b"")
}

/// Every answer, refusals included, carries its type and the policy. Built
/// without the fallible builder, so no error path can serve a body bare.
fn hardened(
    status: StatusCode,
    content_type: &'static str,
    body: &'static [u8],
) -> Response<Cow<'static, [u8]>> {
    let mut response = Response::new(Cow::Borrowed(body));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(POLICY));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use wry::http::header::ACCESS_CONTROL_ALLOW_ORIGIN;

    fn request(method: Method, uri: &str, body: &[u8]) -> Request<Vec<u8>> {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(body.to_vec())
            .expect("valid test request")
    }

    fn get(uri: &str) -> Response<Cow<'static, [u8]>> {
        response(request(Method::GET, uri, b""))
    }

    fn header<'a>(response: &'a Response<Cow<'static, [u8]>>, name: &str) -> Option<&'a str> {
        response
            .headers()
            .get(name)
            .map(|value| value.to_str().expect("ASCII header"))
    }

    /// Headers every answer carries, whatever its status.
    fn assert_hardened(response: &Response<Cow<'static, [u8]>>, target: &str) {
        assert_eq!(
            header(response, "x-content-type-options"),
            Some("nosniff"),
            "{target}"
        );
        assert_eq!(
            header(response, "content-security-policy"),
            Some(POLICY),
            "{target}"
        );
        assert!(
            response
                .headers()
                .get(ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none(),
            "{target} grants CORS"
        );
    }

    fn assert_refused(response: &Response<Cow<'static, [u8]>>, status: StatusCode, target: &str) {
        assert_eq!(response.status(), status, "{target}");
        assert!(response.body().is_empty(), "{target} leaked a body");
        assert_eq!(
            header(response, "content-type"),
            Some("text/plain; charset=utf-8"),
            "{target}"
        );
        assert_hardened(response, target);
    }

    #[test]
    fn serves_each_embedded_file_with_its_exact_type() {
        let files = [
            ("/", INDEX_HTML, "text/html; charset=utf-8"),
            ("/index.html", INDEX_HTML, "text/html; charset=utf-8"),
            ("/popover.css", POPOVER_CSS, "text/css; charset=utf-8"),
            ("/popover.js", POPOVER_JS, "text/javascript; charset=utf-8"),
        ];
        for (path, body, content_type) in files {
            let target = format!("aiub://localhost{path}");
            let response = get(&target);
            assert_eq!(response.status(), StatusCode::OK, "{target}");
            assert_eq!(response.body().as_ref(), body.as_bytes(), "{target}");
            assert_eq!(
                header(&response, "content-type"),
                Some(content_type),
                "{target}"
            );
            assert_hardened(&response, &target);
        }
    }

    #[test]
    fn refuses_foreign_schemes_and_authorities() {
        let foreign = [
            "https://localhost/index.html",
            // The Windows WebView2 origin is not this page's origin.
            "http://aiub.localhost/index.html",
            "AIUB://localhost/index.html",
            "aiub://LOCALHOST/index.html",
            "aiub://localhost.evil.example/index.html",
            "aiub://evil.example/index.html",
            "aiub://user@localhost/index.html",
            "aiub://localhost:8080/index.html",
            "aiub://127.0.0.1/index.html",
            // No scheme or authority at all.
            "/index.html",
        ];
        for target in foreign {
            assert_refused(&get(target), StatusCode::FORBIDDEN, target);
        }
        let bare = Request::new(Vec::new());
        assert_refused(&response(bare), StatusCode::FORBIDDEN, "default request");
    }

    #[test]
    fn answers_only_get() {
        let others = [
            Method::HEAD,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
            Method::TRACE,
            Method::from_bytes(b"PROPFIND").expect("extension method"),
        ];
        for method in others {
            let label = method.to_string();
            let response = response(request(method, "aiub://localhost/index.html", b""));
            assert_refused(&response, StatusCode::METHOD_NOT_ALLOWED, &label);
            assert_eq!(header(&response, "allow"), Some("GET"), "{label}");
        }
    }

    #[test]
    fn refuses_a_get_that_carries_a_body() {
        let response = response(request(
            Method::GET,
            "aiub://localhost/popover.js",
            br#"{"cmd":"quit"}"#,
        ));
        assert_refused(&response, StatusCode::BAD_REQUEST, "GET with body");
    }

    #[test]
    fn serves_no_target_outside_the_exact_table() {
        let unknown = [
            "/index.htm",
            "/INDEX.HTML",
            "/Index.html",
            "/popover.js/",
            "/popover.js.map",
            "/assets/logo.svg",
            "/src/main.tsx",
            "//index.html",
            "/./index.html",
            "/../index.html",
            "/../../../../etc/passwd",
            "/%2e%2e/%2e%2e/etc/passwd",
            "/..%2fCargo.toml",
            "/index%2ehtml",
            "/popover.js%00",
            "/popover.js\\..\\index.html",
            "/popover.css?v=1",
            "/index.html?",
            "/?cmd=quit",
        ];
        for path in unknown {
            let target = format!("aiub://localhost{path}");
            assert_refused(&get(&target), StatusCode::NOT_FOUND, &target);
        }
    }

    #[test]
    fn policy_denies_everything_the_bundle_does_not_load() {
        let directives: Vec<(&str, Vec<&str>)> = POLICY
            .split(';')
            .map(|directive| {
                let mut words = directive.split_ascii_whitespace();
                let name = words.next().expect("directive name");
                (name, words.collect())
            })
            .collect();
        let sources = |name: &str| {
            directives
                .iter()
                .find(|(directive, _)| *directive == name)
                .map(|(_, sources)| sources.clone())
                .unwrap_or_else(|| panic!("{name} missing"))
        };

        // Absent fetch directives (connect, frame, img, font, media, object,
        // worker...) fall back to this; nothing may widen it but the two below.
        assert_eq!(sources("default-src"), ["'none'"]);
        let fetch: Vec<&str> = directives
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| name.ends_with("-src") && *name != "default-src")
            .collect();
        assert_eq!(fetch, ["script-src", "style-src"]);
        // Only the bundle's module: no inline code, eval, or remote script.
        assert_eq!(sources("script-src"), ["'self'"]);
        assert_eq!(sources("style-src"), ["'self'", "'unsafe-inline'"]);
        // These three have no fallback, so they must be spelled out.
        for name in ["base-uri", "form-action", "frame-ancestors"] {
            assert_eq!(sources(name), ["'none'"], "{name}");
        }
    }

    #[test]
    fn the_page_references_only_files_this_protocol_serves() {
        // `script-src 'self'` blocks inline code, so every script is a file.
        for tag in INDEX_HTML.split("<script").skip(1) {
            let open = &tag[..tag.find('>').expect("closed script tag")];
            assert!(open.contains(" src=\""), "inline script: <script{open}>");
        }
        let references: Vec<&str> = ["src=\"", "href=\""]
            .into_iter()
            .flat_map(|attribute| INDEX_HTML.split(attribute).skip(1))
            .map(|rest| &rest[..rest.find('"').expect("closed attribute")])
            .collect();
        assert!(!references.is_empty(), "the page loads no bundle");
        for reference in references {
            assert!(
                !reference.contains(':'),
                "{reference} is not an embedded file"
            );
            let path = reference.trim_start_matches("./").trim_start_matches('/');
            let target = format!("aiub://localhost/{path}");
            assert_eq!(
                get(&target).status(),
                StatusCode::OK,
                "{reference} is not served"
            );
        }
    }
}
