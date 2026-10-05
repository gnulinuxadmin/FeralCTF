use crate::AppState;
use crate::handlers::admin::{
    announce, backup, ban_user, competition_end, competition_freeze, competition_start,
    create_challenge, create_file, create_hint, create_user, dashboard, delete_challenge,
    delete_file, delete_hint, disqualify_team, export_bundle, get_settings, get_teams, get_users,
    import_bundle, list_admin_challenges, list_files, list_hints, list_submissions, require_admin,
    reveal_flag, update_branding, update_challenge, update_file, update_hint,
    update_team_disqualified, update_user_password, update_user_role, update_user_team,
};
use crate::handlers::auth::{change_password, login, logout, me, register};
use crate::handlers::challenges::{get_challenge, list_challenges, submit_flag, unlock_hint};
use crate::handlers::scoreboard::{
    create_team, get_competition, get_scoreboard, get_scoreboard_graph, get_team_profile,
    join_team, list_announcements,
};
use crate::handlers::ws::ws_handler;
use axum::{
    Router,
    body::Body,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderValue, Method, StatusCode, Uri, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use rust_embed::RustEmbed;
use tower_http::cors::{AllowOrigin, CorsLayer};

#[derive(RustEmbed)]
#[folder = "frontend/"]
struct FrontendAssets;

/// Build the application router. Takes ownership of AppState so the admin
/// middleware can be baked in via `from_fn_with_state`.
pub fn create_router(state: AppState) -> Router {
    let cors = cors_layer(&state);
    let base_path = public_base_path(&state.config.server.base_url);
    let import_body_limit = usize::try_from(
        state
            .config
            .storage
            .max_file_size_mb
            .saturating_mul(1024 * 1024),
    )
    .unwrap_or(usize::MAX);
    let admin_router = Router::new()
        .route("/api/admin", get(dashboard))
        .route(
            "/api/admin/challenges",
            get(list_admin_challenges).post(create_challenge),
        )
        .route(
            "/api/admin/challenges/{id}",
            put(update_challenge).delete(delete_challenge),
        )
        .route("/api/admin/challenges/{id}/flag", get(reveal_flag))
        .route(
            "/api/admin/challenges/{id}/hints",
            get(list_hints).post(create_hint),
        )
        .route(
            "/api/admin/hints/{id}",
            put(update_hint).delete(delete_hint),
        )
        .route(
            "/api/admin/challenges/{id}/files",
            get(list_files).post(create_file),
        )
        .route(
            "/api/admin/files/{id}",
            put(update_file).delete(delete_file),
        )
        .route("/api/admin/submissions", get(list_submissions))
        .route("/api/admin/users", get(get_users).post(create_user))
        .route("/api/admin/users/{id}/team", put(update_user_team))
        .route("/api/admin/settings", get(get_settings))
        .route("/api/admin/branding", put(update_branding))
        .route("/api/admin/users/{id}/ban", post(ban_user))
        .route("/api/admin/users/{id}/role", put(update_user_role))
        .route("/api/admin/users/{id}/password", put(update_user_password))
        .route("/api/admin/teams", get(get_teams))
        .route("/api/admin/teams/{id}/disqualify", post(disqualify_team))
        .route(
            "/api/admin/teams/{id}/disqualified",
            put(update_team_disqualified),
        )
        .route("/api/admin/competition/start", post(competition_start))
        .route("/api/admin/competition/end", post(competition_end))
        .route("/api/admin/competition/freeze", post(competition_freeze))
        .route("/api/admin/announce", post(announce))
        .route("/api/admin/export", get(export_bundle))
        .route(
            "/api/admin/import",
            post(import_bundle).layer(DefaultBodyLimit::max(import_body_limit)),
        )
        .route("/api/admin/backup", get(backup))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_admin));

    let api_router = Router::new()
        // Auth routes
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        .route("/api/auth/password", put(change_password))
        // Challenge routes
        .route("/api/challenges", get(list_challenges))
        .route("/api/challenges/{id}", get(get_challenge))
        .route("/api/challenges/{id}/submit", post(submit_flag))
        .route(
            "/api/challenges/{challenge_id}/hints/{hint_id}/unlock",
            post(unlock_hint),
        )
        // Scoreboard routes
        .route("/api/scoreboard", get(get_scoreboard))
        .route("/api/scoreboard/graph", get(get_scoreboard_graph))
        .route("/api/teams/{id}", get(get_team_profile))
        .route("/api/teams", post(create_team))
        .route("/api/teams/join", post(join_team))
        .route("/api/competition", get(get_competition))
        .route("/api/announcements", get(list_announcements))
        // WebSocket
        .route("/ws", get(ws_handler))
        // Admin (require_admin middleware applied inside admin_router)
        .merge(admin_router);

    let app = if base_path.is_empty() {
        api_router
    } else {
        Router::new()
            .nest(&base_path, api_router.clone())
            .merge(api_router)
    };

    app.fallback(frontend)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security_headers,
        ))
        .layer(cors)
        .with_state(state)
}

fn cors_layer(state: &AppState) -> CorsLayer {
    let origins = state
        .config
        .server
        .allowed_origins
        .iter()
        .filter_map(|origin| origin.parse::<HeaderValue>().ok())
        .collect::<Vec<_>>();

    let layer = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    if origins.is_empty() {
        layer
    } else {
        layer.allow_origin(AllowOrigin::list(origins))
    }
}

/// Current branding, or the config default if the database is unavailable.
fn current_branding(state: &AppState) -> crate::competition::Branding {
    state
        .db
        .get()
        .ok()
        .and_then(|conn| {
            state
                .cache
                .get_or_load_branding(&conn, &state.config.competition)
                .ok()
        })
        .unwrap_or_else(|| crate::competition::Branding {
            name: state.config.competition.name.clone(),
            logo_url: None,
        })
}

/// The CSP allows images only from this origin plus the admin-configured
/// logo's origin.
fn content_security_policy(logo_origin: Option<&str>) -> String {
    let img_src = match logo_origin {
        Some(origin) => format!("img-src 'self' {origin}; "),
        None => String::new(),
    };
    format!("default-src 'self'; {img_src}connect-src 'self' ws: wss:")
}

async fn security_headers(State(state): State<AppState>, request: Request, next: Next) -> Response {
    // FERALCTF_SPEC.md §6.6 requires these headers on every response.
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    headers.insert(
        "Referrer-Policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    let csp = content_security_policy(current_branding(&state).logo_origin().as_deref());
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&csp).unwrap_or_else(|_| {
            HeaderValue::from_static("default-src 'self'; connect-src 'self' ws: wss:")
        }),
    );
    response
}

async fn frontend(State(state): State<AppState>, uri: Uri) -> Response {
    let base_path = public_base_path(&state.config.server.base_url);
    let request_path = strip_public_base_path(uri.path(), &base_path);
    let path = request_path.trim_start_matches('/');
    if path.starts_with("api/") || path == "ws" {
        return StatusCode::NOT_FOUND.into_response();
    }

    if path.is_empty() {
        let name = current_branding(&state).name;
        return index_response(&base_path, &name).unwrap_or_else(|| {
            (StatusCode::INTERNAL_SERVER_ERROR, "frontend missing").into_response()
        });
    }

    asset_response(path)
        .unwrap_or_else(|| error_page(StatusCode::NOT_FOUND, uri.path(), &base_path))
}

fn error_page(status: StatusCode, path: &str, base_path: &str) -> Response {
    let code = status.as_u16();
    let reason = status.canonical_reason().unwrap_or("error");
    let safe_path = path
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let stylesheet_path = app_path(base_path, "/style.css");
    let home_path = app_path(base_path, "/");
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>FeralCTF — {code}</title>
  <link rel="stylesheet" href="{stylesheet_path}">
</head>
<body>
  <div id="app">
    <header class="topbar">
      <div>
        <h1>FeralCTF</h1>
        <p class="muted">terminal capture console</p>
      </div>
    </header>
    <main class="error-page">
      <section class="panel error-panel">
        <p class="muted error-code">{code}</p>
        <h2 class="error-reason">{reason}</h2>
        <p class="muted"><code>{safe_path}</code> does not exist</p>
        <a class="error-home" href="{home_path}">← back to terminal</a>
      </section>
    </main>
  </div>
</body>
</html>"#
    );
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(html))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn index_response(base_path: &str, competition_name: &str) -> Option<Response> {
    let html = render_index_html(base_path, competition_name)?;
    match Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(html))
    {
        Ok(response) => Some(response),
        Err(_) => Some(StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    }
}

fn render_index_html(base_path: &str, competition_name: &str) -> Option<String> {
    let asset = FrontendAssets::get("index.html")?;
    let html = String::from_utf8(asset.data.into_owned()).ok()?;
    let safe_base_path = html_attr_escape(base_path);
    Some(
        html.replace("{{BASE_PATH}}", &safe_base_path)
            .replace("{{VERSION}}", env!("CARGO_PKG_VERSION"))
            .replace("{{COMPETITION_NAME}}", &html_attr_escape(competition_name)),
    )
}

fn asset_response(path: &str) -> Option<Response> {
    let asset = FrontendAssets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref());
    if matches!(path, "app.js" | "style.css") {
        builder = builder.header(header::CACHE_CONTROL, "no-cache");
    }
    match builder.body(Body::from(asset.data.into_owned())) {
        Ok(response) => Some(response),
        Err(_) => Some(StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    }
}

fn public_base_path(base_url: &str) -> String {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let path = if trimmed.starts_with('/') {
        trimmed.split(['?', '#']).next().unwrap_or("").to_string()
    } else if let Ok(uri) = trimmed.parse::<Uri>() {
        uri.path().to_string()
    } else {
        String::new()
    };

    normalize_base_path(&path)
}

fn normalize_base_path(path: &str) -> String {
    let trimmed = path.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "/" {
        return String::new();
    }

    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn app_path(base_path: &str, path: &str) -> String {
    let base_path = normalize_base_path(base_path);
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    format!("{base_path}{path}")
}

fn strip_public_base_path<'a>(path: &'a str, base_path: &str) -> &'a str {
    if base_path.is_empty() {
        return path;
    }

    if path == base_path {
        "/"
    } else if let Some(stripped) = path.strip_prefix(base_path) {
        if stripped.starts_with('/') {
            stripped
        } else {
            path
        }
    } else {
        path
    }
}

fn html_attr_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppCache, Config, WsHub, anticheat::RateLimiter, db};
    use axum::body::to_bytes;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[test]
    fn public_base_path_uses_config_url_path() {
        assert_eq!(public_base_path("http://localhost:8080"), "");
        assert_eq!(
            public_base_path("https://example.org/server/feralctf"),
            "/server/feralctf"
        );
        assert_eq!(
            public_base_path("https://example.org/server/feralctf/"),
            "/server/feralctf"
        );
        assert_eq!(public_base_path("/server/feralctf/"), "/server/feralctf");
    }

    #[test]
    fn render_index_html_injects_base_path() {
        let html = render_index_html("/server/feralctf", "FeralCTF").expect("embedded index");

        assert!(html.contains(r#"href="/server/feralctf/style.css""#));
        assert!(html.contains(r#"href="/server/feralctf/favicon.ico""#));
        assert!(html.contains(r#"href="/server/feralctf/favicon.png""#));
        assert!(html.contains(r#"src="/server/feralctf/app.js""#));
        assert!(html.contains(r#"name="feralctf-base-path" content="/server/feralctf""#));
        assert!(!html.contains("window.FERALCTF_BASE_PATH"));
    }

    #[test]
    fn render_index_html_injects_escaped_competition_name() {
        let html = render_index_html("", "Squirrels <&> CTF").expect("embedded index");
        assert!(html.contains("<title>Squirrels &lt;&amp;&gt; CTF</title>"));
        assert!(!html.contains("{{COMPETITION_NAME}}"));
    }

    #[test]
    fn csp_adds_only_the_logo_origin_to_img_src() {
        assert_eq!(
            content_security_policy(None),
            "default-src 'self'; connect-src 'self' ws: wss:"
        );
        assert_eq!(
            content_security_policy(Some("https://cdn.example.org")),
            "default-src 'self'; img-src 'self' https://cdn.example.org; connect-src 'self' ws: wss:"
        );
    }

    #[tokio::test]
    async fn branding_drives_title_and_csp() {
        let state = test_state("http://localhost:8080");
        {
            let conn = state.db.get().expect("conn");
            crate::competition::set_branding(
                &conn,
                Some("Squirrel Games"),
                Some("https://cdn.example.org/logo.png"),
                1,
            )
            .expect("branding");
        }
        let app = create_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let csp = response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            csp.contains("img-src 'self' https://cdn.example.org;"),
            "{csp}"
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let html = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(html.contains("<title>Squirrel Games</title>"));
    }

    #[test]
    fn render_index_html_injects_version() {
        let html = render_index_html("", "FeralCTF").expect("embedded index");
        assert!(html.contains(&format!(
            r#"name="feralctf-version" content="{}""#,
            env!("CARGO_PKG_VERSION")
        )));
        assert!(!html.contains("{{VERSION}}"));
        assert_eq!(env!("CARGO_PKG_VERSION"), "1.0.2");
    }

    #[test]
    fn render_index_html_preserves_root_deployment() {
        let html = render_index_html("", "FeralCTF").expect("embedded index");

        assert!(html.contains(r#"href="/style.css""#));
        assert!(html.contains(r#"href="/favicon.ico""#));
        assert!(html.contains(r#"href="/favicon.png""#));
        assert!(html.contains(r#"src="/app.js""#));
        assert!(html.contains(r#"name="feralctf-base-path" content="""#));
        assert!(!html.contains("window.FERALCTF_BASE_PATH"));
    }

    #[tokio::test]
    async fn prefixed_index_route_renders_configured_paths() {
        let app = create_router(test_state("https://example.org/server/feralctf"));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/server/feralctf/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let html = String::from_utf8(body.to_vec()).expect("utf8");

        assert!(html.contains(r#"href="/server/feralctf/style.css""#));
        assert!(html.contains(r#"href="/server/feralctf/favicon.ico""#));
        assert!(html.contains(r#"href="/server/feralctf/favicon.png""#));
        assert!(html.contains(r#"src="/server/feralctf/app.js""#));
        assert!(html.contains(r#"name="feralctf-base-path" content="/server/feralctf""#));
        assert!(!html.contains("window.FERALCTF_BASE_PATH"));
    }

    #[tokio::test]
    async fn prefixed_api_route_reaches_api_router() {
        let app = create_router(test_state("https://example.org/server/feralctf"));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/server/feralctf/api/auth/me")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn feralctf_prefix_routes_assets_and_public_api() {
        let app = create_router(test_state("https://server.tld/feralctf/"));

        let image_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/feralctf/feral10.jpg")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("image response");
        assert_eq!(image_response.status(), StatusCode::OK);

        let login_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/feralctf/api/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"username":"missing","password":"missing"}"#))
                    .expect("request"),
            )
            .await
            .expect("login response");
        assert_ne!(login_response.status(), StatusCode::NOT_FOUND);

        let scoreboard_response = app
            .oneshot(
                Request::builder()
                    .uri("/feralctf/api/scoreboard")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("scoreboard response");
        assert_eq!(scoreboard_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn prefixed_core_frontend_assets_revalidate() {
        let app = create_router(test_state("https://server.tld/feralctf/"));

        let index_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/feralctf/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("index response");
        assert_eq!(
            index_response.headers().get(header::CACHE_CONTROL),
            Some(&HeaderValue::from_static("no-cache"))
        );

        let app_js_response = app
            .oneshot(
                Request::builder()
                    .uri("/feralctf/app.js")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("app js response");
        assert_eq!(
            app_js_response.headers().get(header::CACHE_CONTROL),
            Some(&HeaderValue::from_static("no-cache"))
        );
    }

    #[tokio::test]
    async fn prefixed_favicon_assets_are_embedded() {
        let app = create_router(test_state("https://server.tld/feralctf/"));

        for path in ["/feralctf/favicon.ico", "/feralctf/favicon.png"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("favicon response");
            assert_eq!(response.status(), StatusCode::OK, "{path}");
        }
    }

    #[tokio::test]
    async fn register_route_is_available_at_root_and_configured_prefix() {
        let app = create_router(test_state("https://server.tld/feralctf/"));

        let root_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/auth/register")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"username":"ab","password":"short"}"#))
                    .expect("request"),
            )
            .await
            .expect("root register response");
        assert_ne!(root_response.status(), StatusCode::NOT_FOUND);

        let prefixed_response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/feralctf/api/auth/register")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"username":"ab","password":"short"}"#))
                    .expect("request"),
            )
            .await
            .expect("prefixed register response");
        assert_ne!(prefixed_response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn register_get_is_method_error_not_missing_route() {
        let app = create_router(test_state("https://server.tld/feralctf/"));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/feralctf/api/auth/register")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("register get response");

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn new_admin_routes_require_an_admin_session() {
        let state = test_state("http://admin-guard.test");
        let player_token = {
            let conn = state.db.get().expect("conn");
            let user = crate::models::user::User::create(
                &conn,
                &crate::models::user::RegisterRequest {
                    username: format!("guard-{}", uuid::Uuid::new_v4().simple()),
                    email: None,
                    password: String::new(),
                    team_name: None,
                    invite_code: None,
                },
                "hash",
            )
            .expect("user");
            drop(conn);
            let now = chrono::Utc::now().timestamp() as u64;
            let token = crate::auth::sign_jwt(
                &crate::auth::Claims {
                    sub: user.id,
                    role: "player".into(),
                    team_id: None,
                    iat: now,
                    exp: now + 3600,
                },
                &state.config.auth.jwt_secret,
            )
            .expect("jwt");
            crate::auth::create_session(&state.db, user.id, &token, 1).expect("session");
            token
        };
        let app = create_router(state);
        let routes = [
            (Method::GET, "/api/admin/challenges/1/flag", ""),
            (Method::GET, "/api/admin/challenges/1/hints", ""),
            (
                Method::POST,
                "/api/admin/challenges/1/hints",
                r#"{"content":"x","cost_points":0}"#,
            ),
            (Method::PUT, "/api/admin/hints/1", "{}"),
            (Method::DELETE, "/api/admin/hints/1", ""),
            (
                Method::POST,
                "/api/admin/challenges/1/files",
                r#"{"label":"x","url":"https://e.x/y"}"#,
            ),
            (Method::DELETE, "/api/admin/files/1", ""),
            (Method::POST, "/api/admin/users", "{}"),
            (Method::PUT, "/api/admin/users/1/team", r#"{"team":null}"#),
            (Method::GET, "/api/admin/settings", ""),
            (Method::PUT, "/api/admin/branding", "{}"),
        ];
        for (method, uri, body) in routes {
            for (token, expected) in [
                (None, StatusCode::UNAUTHORIZED),
                (Some(player_token.as_str()), StatusCode::FORBIDDEN),
            ] {
                let mut request = Request::builder()
                    .method(method.clone())
                    .uri(uri)
                    .header(header::CONTENT_TYPE, "application/json");
                if let Some(token) = token {
                    request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
                }
                let response = app
                    .clone()
                    .oneshot(request.body(Body::from(body)).expect("request"))
                    .await
                    .expect("response");
                assert_eq!(
                    response.status(),
                    expected,
                    "{method} {uri} token={}",
                    token.is_some()
                );
            }
        }
    }

    fn test_state(base_url: &str) -> AppState {
        let db_path = std::env::temp_dir().join(format!(
            "feralctf-routes-test-{}-{}.db",
            std::process::id(),
            base_url.len()
        ));
        let pool = db::init_pool(&db_path.to_string_lossy()).expect("db pool");
        let mut config = Config::default();
        config.server.base_url = base_url.to_string();

        AppState {
            db: pool,
            config: Arc::new(config),
            cache: Arc::new(AppCache::new()),
            ws_hub: Arc::new(WsHub::new()),
            rate_limiter: Arc::new(RateLimiter::new()),
        }
    }
}
