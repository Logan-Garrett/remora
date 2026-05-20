//! Authentication and authorization: JWT, password hashing, OAuth, RBAC.

use crate::db::Database;
use crate::state::AppState;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use hmac::{Hmac, Mac};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use remora_common::User;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// JWT Claims
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,  // user_id
    pub name: String, // display_name
    pub role: String,
    pub exp: usize,
    pub iat: usize,
}

pub fn create_jwt(user: &User, secret: &str, expiry_secs: u64) -> anyhow::Result<String> {
    let now = Utc::now().timestamp() as usize;
    let claims = Claims {
        sub: user.id.to_string(),
        name: user.display_name.clone(),
        role: user.role.clone(),
        iat: now,
        exp: now + expiry_secs as usize,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

pub fn decode_jwt(token: &str, secret: &str) -> Option<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .ok()
    .map(|data| data.claims)
}

// ---------------------------------------------------------------------------
// Password hashing
// ---------------------------------------------------------------------------

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("password hash error: {e}"))?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

// ---------------------------------------------------------------------------
// Token hashing (SHA-256 for refresh tokens and API keys)
// ---------------------------------------------------------------------------

pub fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn generate_random_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

// ---------------------------------------------------------------------------
// OAuth CSRF state parameter (self-validating HMAC)
// ---------------------------------------------------------------------------

type HmacSha256 = Hmac<Sha256>;

/// Generate a self-validating OAuth state parameter.
///
/// Without origin: `<uuid>.<hmac_hex>` (backward compatible).
/// With origin:    `<uuid>|<base64url_origin>.<hmac_hex>`.
///
/// The HMAC covers the full payload before the final `.`.
pub fn generate_oauth_state(secret: &str, origin: Option<&str>) -> String {
    let nonce = Uuid::new_v4().to_string();
    let payload = match origin {
        Some(o) => {
            use base64::engine::general_purpose::URL_SAFE_NO_PAD;
            use base64::Engine;
            let encoded = URL_SAFE_NO_PAD.encode(o.as_bytes());
            format!("{nonce}|{encoded}")
        }
        None => nonce,
    };
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());
    format!("{payload}.{sig}")
}

/// Validate an OAuth state parameter. Returns `None` if invalid, or
/// `Some(optional_origin)` on success.
pub fn validate_oauth_state(state: &str, secret: &str) -> Option<Option<String>> {
    let (payload, sig_hex) = state.rsplit_once('.')?;
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());
    use subtle::ConstantTimeEq;
    if !bool::from(sig_hex.as_bytes().ct_eq(expected.as_bytes())) {
        return None;
    }
    // Extract origin if present
    let origin = if let Some((_nonce, b64)) = payload.split_once('|') {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        String::from_utf8(URL_SAFE_NO_PAD.decode(b64).ok()?).ok()
    } else {
        None
    };
    Some(origin)
}

// ---------------------------------------------------------------------------
// Role enforcement
// ---------------------------------------------------------------------------

/// Role hierarchy: admin > member > viewer > guest
/// Returns the numeric level for comparison.
pub fn role_level(role: &str) -> u8 {
    match role {
        "admin" => 4,
        "member" => 3,
        "viewer" => 2,
        "guest" => 1,
        _ => 0,
    }
}

pub fn require_role(user: &User, minimum: &str) -> Result<(), StatusCode> {
    if role_level(&user.role) >= role_level(minimum) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

// ---------------------------------------------------------------------------
// REST endpoint request/response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RegisterBody {
    pub email: String,
    pub display_name: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct LoginBody {
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct RefreshBody {
    pub refresh_token: String,
}

#[derive(Serialize)]
struct AuthResponse {
    access_token: String,
    refresh_token: String,
    user: User,
}

#[derive(Deserialize)]
pub struct CreateApiKeyBody {
    #[serde(default)]
    pub label: String,
}

#[derive(Serialize)]
struct ApiKeyResponse {
    key: String,
    id: Uuid,
    label: String,
}

#[derive(Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: String,
    #[serde(default)]
    pub state: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn extract_bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()
        .map(|v| v.strip_prefix("Bearer ").unwrap_or(v))
}

fn extract_user_from_jwt(headers: &axum::http::HeaderMap, secret: &str) -> Option<Claims> {
    let token = extract_bearer(headers)?;
    decode_jwt(token, secret)
}

/// Extract the client IP from request headers. Checks `X-Forwarded-For` first,
/// then `X-Real-IP`, falling back to `127.0.0.1` if neither is present.
fn extract_client_ip(headers: &axum::http::HeaderMap) -> std::net::IpAddr {
    // X-Forwarded-For may contain a comma-separated list; take the first entry.
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = xff.split(',').next() {
            if let Ok(ip) = first.trim().parse::<std::net::IpAddr>() {
                return ip;
            }
        }
    }
    if let Some(xri) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        if let Ok(ip) = xri.trim().parse::<std::net::IpAddr>() {
            return ip;
        }
    }
    std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
}

// ---------------------------------------------------------------------------
// REST handlers
// ---------------------------------------------------------------------------

pub async fn register(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<RegisterBody>,
) -> impl IntoResponse {
    let ip = extract_client_ip(&headers);
    if !state.rate_limiter.check(ip, "register", 5).await {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response();
    }

    if body.email.is_empty() || body.password.is_empty() || body.display_name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "email, display_name, and password required",
        )
            .into_response();
    }
    if body.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            "password must be at least 8 characters",
        )
            .into_response();
    }

    // Check if email already exists. Return a generic error to avoid
    // leaking whether a given email is already registered (email enumeration).
    match state.db.get_user_by_email(&body.email).await {
        Ok(Some(_)) => {
            return (StatusCode::BAD_REQUEST, "registration failed").into_response();
        }
        Err(e) => {
            tracing::error!("get_user_by_email: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
        _ => {}
    }

    let pw_hash = match hash_password(&body.password) {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("hash_password: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "hash error").into_response();
        }
    };

    // Auto-promote the first registered user to admin
    let role = match state.db.count_users().await {
        Ok(0) => {
            tracing::info!("first user registration: auto-promoting to admin");
            "admin"
        }
        Ok(_) => "member",
        Err(e) => {
            tracing::error!("count_users: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
    };

    match state
        .db
        .create_user(&body.email, &body.display_name, Some(&pw_hash), role)
        .await
    {
        Ok(id) => {
            let user = User {
                id,
                email: body.email,
                display_name: body.display_name,
                role: role.to_string(),
                created_at: Utc::now(),
            };
            (StatusCode::CREATED, Json(serde_json::json!(user))).into_response()
        }
        Err(e) => {
            tracing::error!("create_user: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<LoginBody>,
) -> impl IntoResponse {
    let ip = extract_client_ip(&headers);
    if !state.rate_limiter.check(ip, "login", 10).await {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response();
    }

    if body.email.is_empty() || body.password.is_empty() {
        return (StatusCode::BAD_REQUEST, "email and password required").into_response();
    }

    // Fetch user
    let user = match state.db.get_user_by_email(&body.email).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response();
        }
        Err(e) => {
            tracing::error!("get_user_by_email: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
    };

    // Verify password
    let stored_hash = match state.db.get_password_hash(&body.email).await {
        Ok(Some(h)) => h,
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response();
        }
        Err(e) => {
            tracing::error!("get_password_hash: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
    };

    if !verify_password(&body.password, &stored_hash) {
        return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response();
    }

    // Generate tokens
    let access_token = match create_jwt(
        &user,
        &state.config.jwt_secret,
        state.config.jwt_expiry_secs,
    ) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("create_jwt: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "token error").into_response();
        }
    };

    let raw_refresh = generate_random_token();
    let refresh_hash = sha256_hex(&raw_refresh);
    let expires_at =
        Utc::now() + chrono::Duration::seconds(state.config.refresh_expiry_secs as i64);
    if let Err(e) = state
        .db
        .store_refresh_token(user.id, &refresh_hash, expires_at)
        .await
    {
        tracing::error!("store_refresh_token: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
    }

    Json(AuthResponse {
        access_token,
        refresh_token: raw_refresh,
        user,
    })
    .into_response()
}

pub async fn refresh(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<RefreshBody>,
) -> impl IntoResponse {
    let ip = extract_client_ip(&headers);
    if !state.rate_limiter.check(ip, "refresh", 30).await {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response();
    }

    if body.refresh_token.is_empty() {
        return (StatusCode::BAD_REQUEST, "refresh_token required").into_response();
    }

    let token_hash = sha256_hex(&body.refresh_token);

    // Atomic consume: DELETE + RETURNING in one query to prevent race conditions
    // where two concurrent requests could both validate the same token.
    let user_id = match state.db.consume_refresh_token(&token_hash).await {
        Ok(Some(uid)) => uid,
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, "invalid or expired refresh token").into_response();
        }
        Err(e) => {
            tracing::error!("consume_refresh_token: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
    };

    let user = match state.db.get_user_by_id(user_id).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, "user not found").into_response();
        }
        Err(e) => {
            tracing::error!("get_user_by_id: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
    };

    let access_token = match create_jwt(
        &user,
        &state.config.jwt_secret,
        state.config.jwt_expiry_secs,
    ) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("create_jwt: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "token error").into_response();
        }
    };

    // Issue a new refresh token (rotation: old one was already consumed above)
    let raw_refresh = generate_random_token();
    let refresh_hash = sha256_hex(&raw_refresh);
    let expires_at =
        Utc::now() + chrono::Duration::seconds(state.config.refresh_expiry_secs as i64);
    if let Err(e) = state
        .db
        .store_refresh_token(user.id, &refresh_hash, expires_at)
        .await
    {
        tracing::error!("store_refresh_token (refresh): {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
    }

    Json(AuthResponse {
        access_token,
        refresh_token: raw_refresh,
        user,
    })
    .into_response()
}

pub async fn me(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let claims = match extract_user_from_jwt(&headers, &state.config.jwt_secret) {
        Some(c) => c,
        None => {
            return (StatusCode::UNAUTHORIZED, "valid JWT required").into_response();
        }
    };

    let user_id: Uuid = match claims.sub.parse() {
        Ok(id) => id,
        Err(_) => return (StatusCode::UNAUTHORIZED, "invalid token").into_response(),
    };

    match state.db.get_user_by_id(user_id).await {
        Ok(Some(user)) => Json(serde_json::json!(user)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "user not found").into_response(),
        Err(e) => {
            tracing::error!("get_user_by_id: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// API keys
// ---------------------------------------------------------------------------

pub async fn create_api_key(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<CreateApiKeyBody>,
) -> impl IntoResponse {
    let claims = match extract_user_from_jwt(&headers, &state.config.jwt_secret) {
        Some(c) => c,
        None => {
            return (StatusCode::UNAUTHORIZED, "valid JWT required").into_response();
        }
    };
    let user_id: Uuid = match claims.sub.parse() {
        Ok(id) => id,
        Err(_) => return (StatusCode::UNAUTHORIZED, "invalid token").into_response(),
    };

    let raw_key = format!("rmk_{}", generate_random_token());
    let key_hash = sha256_hex(&raw_key);

    match state
        .db
        .create_api_key(user_id, &key_hash, &body.label)
        .await
    {
        Ok(id) => Json(ApiKeyResponse {
            key: raw_key,
            id,
            label: body.label,
        })
        .into_response(),
        Err(e) => {
            tracing::error!("create_api_key: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

pub async fn list_api_keys(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let claims = match extract_user_from_jwt(&headers, &state.config.jwt_secret) {
        Some(c) => c,
        None => {
            return (StatusCode::UNAUTHORIZED, "valid JWT required").into_response();
        }
    };
    let user_id: Uuid = match claims.sub.parse() {
        Ok(id) => id,
        Err(_) => return (StatusCode::UNAUTHORIZED, "invalid token").into_response(),
    };

    match state.db.list_api_keys(user_id).await {
        Ok(keys) => Json(keys).into_response(),
        Err(e) => {
            tracing::error!("list_api_keys: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

pub async fn revoke_api_key_endpoint(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(key_id): Path<Uuid>,
) -> impl IntoResponse {
    let claims = match extract_user_from_jwt(&headers, &state.config.jwt_secret) {
        Some(c) => c,
        None => {
            return (StatusCode::UNAUTHORIZED, "valid JWT required").into_response();
        }
    };
    let user_id: Uuid = match claims.sub.parse() {
        Ok(id) => id,
        Err(_) => return (StatusCode::UNAUTHORIZED, "invalid token").into_response(),
    };

    match state.db.revoke_api_key(key_id, user_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!("revoke_api_key: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// OAuth
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct GitHubUser {
    id: u64,
    login: String,
    email: Option<String>,
}

#[derive(Deserialize)]
struct GoogleUserInfo {
    sub: String,
    email: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
pub struct OAuthRedirectParams {
    /// The web client origin, threaded through the state for postMessage callback.
    pub origin: Option<String>,
}

pub async fn oauth_github_redirect(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(params): Query<OAuthRedirectParams>,
) -> impl IntoResponse {
    let ip = extract_client_ip(&headers);
    if !state.rate_limiter.check(ip, "oauth_github", 20).await {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response();
    }

    let (client_id, _) = match (
        &state.config.oauth_github_client_id,
        &state.config.oauth_github_client_secret,
    ) {
        (Some(id), Some(secret)) => (id.clone(), secret.clone()),
        _ => {
            return (StatusCode::NOT_IMPLEMENTED, "GitHub OAuth not configured").into_response();
        }
    };

    let oauth_state = generate_oauth_state(&state.config.jwt_secret, params.origin.as_deref());
    let redirect_base = &state.config.oauth_redirect_base_url;
    let raw_redirect = format!("{redirect_base}/auth/oauth/github/callback");
    let redirect_uri = urlencoding::encode(&raw_redirect);
    let encoded_state = urlencoding::encode(&oauth_state);
    let url = format!(
        "https://github.com/login/oauth/authorize?client_id={}&scope=user:email&state={}&redirect_uri={}",
        client_id, encoded_state, redirect_uri
    );
    axum::response::Redirect::temporary(&url).into_response()
}

pub async fn oauth_github_callback(
    State(state): State<Arc<AppState>>,
    Query(query): Query<OAuthCallbackQuery>,
) -> impl IntoResponse {
    let (client_id, client_secret) = match (
        &state.config.oauth_github_client_id,
        &state.config.oauth_github_client_secret,
    ) {
        (Some(id), Some(secret)) => (id.clone(), secret.clone()),
        _ => {
            return (StatusCode::NOT_IMPLEMENTED, "GitHub OAuth not configured").into_response();
        }
    };

    // Validate CSRF state parameter and extract optional web client origin
    let web_origin = match validate_oauth_state(&query.state, &state.config.jwt_secret) {
        Some(origin) => origin,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "invalid or missing OAuth state parameter",
            )
                .into_response();
        }
    };

    // Exchange code for access token
    let http = reqwest::Client::new();
    let token_resp = match http
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "client_id": client_id,
            "client_secret": client_secret,
            "code": query.code,
        }))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("github oauth token exchange: {e}");
            return (StatusCode::BAD_GATEWAY, "OAuth token exchange failed").into_response();
        }
    };

    #[derive(Deserialize)]
    struct TokenResp {
        access_token: Option<String>,
    }
    let token_body: TokenResp = match token_resp.json().await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("github oauth parse token: {e}");
            return (StatusCode::BAD_GATEWAY, "failed to parse token response").into_response();
        }
    };
    let access_token = match token_body.access_token {
        Some(t) => t,
        None => {
            return (StatusCode::UNAUTHORIZED, "no access token from GitHub").into_response();
        }
    };

    // Fetch GitHub user info
    let user_resp = match http
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", "remora-server")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("github user fetch: {e}");
            return (StatusCode::BAD_GATEWAY, "failed to fetch GitHub user").into_response();
        }
    };
    let gh_user: GitHubUser = match user_resp.json().await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("parse github user: {e}");
            return (StatusCode::BAD_GATEWAY, "failed to parse GitHub user").into_response();
        }
    };

    let provider_user_id = gh_user.id.to_string();
    let email = gh_user
        .email
        .unwrap_or_else(|| format!("{}@github.noemail", gh_user.login));
    let display_name = gh_user.login;

    oauth_complete_flow(
        state,
        "github",
        &provider_user_id,
        &email,
        &display_name,
        web_origin,
    )
    .await
}

pub async fn oauth_google_redirect(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(params): Query<OAuthRedirectParams>,
) -> impl IntoResponse {
    let ip = extract_client_ip(&headers);
    if !state.rate_limiter.check(ip, "oauth_google", 20).await {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response();
    }

    let (client_id, _) = match (
        &state.config.oauth_google_client_id,
        &state.config.oauth_google_client_secret,
    ) {
        (Some(id), Some(secret)) => (id.clone(), secret.clone()),
        _ => {
            return (StatusCode::NOT_IMPLEMENTED, "Google OAuth not configured").into_response();
        }
    };

    let oauth_state = generate_oauth_state(&state.config.jwt_secret, params.origin.as_deref());
    let redirect_base = &state.config.oauth_redirect_base_url;
    let raw_redirect = format!("{redirect_base}/auth/oauth/google/callback");
    let redirect_uri = urlencoding::encode(&raw_redirect);
    let encoded_state = urlencoding::encode(&oauth_state);
    let url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?\
         client_id={}&response_type=code&scope=openid%20email%20profile&\
         redirect_uri={}&state={}",
        client_id, redirect_uri, encoded_state
    );
    axum::response::Redirect::temporary(&url).into_response()
}

pub async fn oauth_google_callback(
    State(state): State<Arc<AppState>>,
    Query(query): Query<OAuthCallbackQuery>,
) -> impl IntoResponse {
    let (client_id, client_secret) = match (
        &state.config.oauth_google_client_id,
        &state.config.oauth_google_client_secret,
    ) {
        (Some(id), Some(secret)) => (id.clone(), secret.clone()),
        _ => {
            return (StatusCode::NOT_IMPLEMENTED, "Google OAuth not configured").into_response();
        }
    };

    // Validate CSRF state parameter and extract optional web client origin
    let web_origin = match validate_oauth_state(&query.state, &state.config.jwt_secret) {
        Some(origin) => origin,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "invalid or missing OAuth state parameter",
            )
                .into_response();
        }
    };

    let redirect_base = &state.config.oauth_redirect_base_url;
    let callback_uri = format!("{redirect_base}/auth/oauth/google/callback");

    let http = reqwest::Client::new();
    let token_resp = match http
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", query.code.as_str()),
            ("client_id", &client_id),
            ("client_secret", &client_secret),
            ("redirect_uri", callback_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("google oauth token exchange: {e}");
            return (StatusCode::BAD_GATEWAY, "OAuth token exchange failed").into_response();
        }
    };

    #[derive(Deserialize)]
    struct TokenResp {
        access_token: Option<String>,
    }
    let token_body: TokenResp = match token_resp.json().await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("google oauth parse token: {e}");
            return (StatusCode::BAD_GATEWAY, "failed to parse token response").into_response();
        }
    };
    let access_token = match token_body.access_token {
        Some(t) => t,
        None => {
            return (StatusCode::UNAUTHORIZED, "no access token from Google").into_response();
        }
    };

    let user_resp = match http
        .get("https://www.googleapis.com/oauth2/v3/userinfo")
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("google userinfo fetch: {e}");
            return (StatusCode::BAD_GATEWAY, "failed to fetch Google user info").into_response();
        }
    };
    let g_user: GoogleUserInfo = match user_resp.json().await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("parse google userinfo: {e}");
            return (StatusCode::BAD_GATEWAY, "failed to parse Google user info").into_response();
        }
    };

    let email = g_user
        .email
        .unwrap_or_else(|| format!("{}@google.noemail", g_user.sub));
    let display_name = g_user.name.unwrap_or_else(|| g_user.sub.clone());

    oauth_complete_flow(
        state,
        "google",
        &g_user.sub,
        &email,
        &display_name,
        web_origin,
    )
    .await
}

async fn oauth_complete_flow(
    state: Arc<AppState>,
    provider: &str,
    provider_user_id: &str,
    email: &str,
    display_name: &str,
    web_origin: Option<String>,
) -> axum::response::Response {
    // Check if user already linked via OAuth
    let user = match state.db.get_user_by_oauth(provider, provider_user_id).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            // Check if user exists by email
            match state.db.get_user_by_email(email).await {
                Ok(Some(existing)) => {
                    // Link OAuth to existing user
                    if let Err(e) = state
                        .db
                        .upsert_oauth_connection(existing.id, provider, provider_user_id)
                        .await
                    {
                        tracing::error!("upsert_oauth_connection: {e}");
                        return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
                    }
                    existing
                }
                Ok(None) => {
                    // Auto-promote the first registered user to admin
                    let role = match state.db.count_users().await {
                        Ok(0) => {
                            tracing::info!(
                                "first user registration (oauth): auto-promoting to admin"
                            );
                            "admin"
                        }
                        Ok(_) => "member",
                        Err(e) => {
                            tracing::error!("count_users (oauth): {e}");
                            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
                        }
                    };

                    // Create new user (no password - OAuth only)
                    match state.db.create_user(email, display_name, None, role).await {
                        Ok(id) => {
                            if let Err(e) = state
                                .db
                                .upsert_oauth_connection(id, provider, provider_user_id)
                                .await
                            {
                                tracing::error!("upsert_oauth_connection: {e}");
                                return (StatusCode::INTERNAL_SERVER_ERROR, "db error")
                                    .into_response();
                            }
                            User {
                                id,
                                email: email.to_string(),
                                display_name: display_name.to_string(),
                                role: role.to_string(),
                                created_at: Utc::now(),
                            }
                        }
                        Err(e) => {
                            tracing::error!("create_user (oauth): {e}");
                            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("get_user_by_email: {e}");
                    return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
                }
            }
        }
        Err(e) => {
            tracing::error!("get_user_by_oauth: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
        }
    };

    // Generate JWT
    let access_token = match create_jwt(
        &user,
        &state.config.jwt_secret,
        state.config.jwt_expiry_secs,
    ) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("create_jwt (oauth): {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "token error").into_response();
        }
    };

    let raw_refresh = generate_random_token();
    let refresh_hash = sha256_hex(&raw_refresh);
    let expires_at =
        Utc::now() + chrono::Duration::seconds(state.config.refresh_expiry_secs as i64);
    if let Err(e) = state
        .db
        .store_refresh_token(user.id, &refresh_hash, expires_at)
        .await
    {
        tracing::error!("store_refresh_token (oauth): {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response();
    }

    let auth = AuthResponse {
        access_token,
        refresh_token: raw_refresh,
        user,
    };

    // When a web_origin is present the callback was triggered from a browser
    // popup.  Return an HTML page that posts the auth data back to the opener
    // via postMessage (targeted to the exact origin for security) then closes.
    if let Some(origin) = web_origin {
        let auth_json = serde_json::to_string(&auth).unwrap_or_default();
        let origin_json = serde_json::to_string(&origin).unwrap_or_default();
        let html = format!(
            r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Remora</title></head>
<body><p>Logging in&hellip;</p>
<script>
(function(){{
var d={auth_json};d.type="remora-oauth-success";
window.opener.postMessage(d,{origin_json});
window.close();
}})();
</script></body></html>"#
        );
        return (
            StatusCode::OK,
            [("content-type", "text/html; charset=utf-8")],
            html,
        )
            .into_response();
    }

    Json(auth).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "test-secret-key";

    #[test]
    fn oauth_state_roundtrip_without_origin() {
        let state = generate_oauth_state(SECRET, None);
        let result = validate_oauth_state(&state, SECRET);
        assert_eq!(result, Some(None));
    }

    #[test]
    fn oauth_state_roundtrip_with_origin() {
        let origin = "https://example.com:3000";
        let state = generate_oauth_state(SECRET, Some(origin));
        let result = validate_oauth_state(&state, SECRET);
        assert_eq!(result, Some(Some(origin.to_string())));
    }

    #[test]
    fn oauth_state_rejects_tampered_hmac() {
        let state = generate_oauth_state(SECRET, None);
        let tampered = format!("{state}a"); // append to HMAC
        assert_eq!(validate_oauth_state(&tampered, SECRET), None);
    }

    #[test]
    fn oauth_state_rejects_wrong_secret() {
        let state = generate_oauth_state(SECRET, Some("https://legit.com"));
        assert_eq!(validate_oauth_state(&state, "wrong-secret"), None);
    }

    #[test]
    fn oauth_state_rejects_tampered_origin() {
        let state = generate_oauth_state(SECRET, Some("https://legit.com"));
        // Tamper with the base64 origin by flipping a character after the '|'
        let pipe_pos = state.find('|').expect("state must contain pipe");
        let dot_pos = state.rfind('.').expect("state must contain dot");
        let mut chars: Vec<char> = state.chars().collect();
        // Flip a char in the base64 segment
        let target = pipe_pos + 1;
        if target < dot_pos {
            chars[target] = if chars[target] == 'A' { 'B' } else { 'A' };
        }
        let tampered: String = chars.into_iter().collect();
        assert_eq!(validate_oauth_state(&tampered, SECRET), None);
    }

    #[test]
    fn oauth_state_rejects_empty_string() {
        assert_eq!(validate_oauth_state("", SECRET), None);
    }

    #[test]
    fn oauth_state_rejects_no_dot() {
        assert_eq!(validate_oauth_state("just-a-nonce", SECRET), None);
    }

    #[test]
    fn oauth_state_origin_with_special_chars() {
        let origin = "http://localhost:3000/#/callback?foo=bar&baz=qux";
        let state = generate_oauth_state(SECRET, Some(origin));
        let result = validate_oauth_state(&state, SECRET);
        assert_eq!(result, Some(Some(origin.to_string())));
    }
}
