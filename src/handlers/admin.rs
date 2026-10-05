use axum::{
    extract::{Multipart, Path, Query, State},
    http::HeaderMap,
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};

use crate::{
    AppState, WsEvent, auth,
    errors::{AppError, HandlerResult},
    import_export::{self, ExportBundle, ImportOptions, ImportResult},
    models::{
        challenge::{Challenge, ChallengeFile, Hint, HintAdmin, validate_attachment_url},
        team::Team,
        user::UserPublic,
    },
    scoring,
};

// ---- request / response types ----

#[derive(Debug, Deserialize)]
pub struct CreateChallengeRequest {
    pub title: String,
    pub category: String,
    pub description: String,
    pub flag: String,
    pub flag_type: String,
    pub flag_case_sensitive: bool,
    pub points: i64,
    pub max_points: i64,
    pub min_points: i64,
    pub decay_rate: i64,
    pub author: Option<String>,
    pub tags: Vec<String>,
    pub unlock_requires: Option<i64>,
    pub is_hidden: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateChallengeRequest {
    pub title: Option<String>,
    pub category: Option<String>,
    pub description: Option<String>,
    pub flag: Option<String>,
    pub flag_type: Option<String>,
    pub flag_case_sensitive: Option<bool>,
    pub points: Option<i64>,
    pub max_points: Option<i64>,
    pub min_points: Option<i64>,
    pub decay_rate: Option<i64>,
    #[serde(default, deserialize_with = "double_option")]
    pub author: Option<Option<String>>,
    pub tags: Option<Vec<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub unlock_requires: Option<Option<i64>>,
    pub is_hidden: Option<bool>,
}

/// Distinguishes an absent field (`None`: keep) from an explicit `null`
/// (`Some(None)`: clear). Plain serde maps both to `None`.
fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Serialize)]
pub struct AdminChallenge {
    #[serde(flatten)]
    pub challenge: Challenge,
    pub hint_count: i64,
    pub file_count: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateHintRequest {
    pub content: String,
    pub cost_points: i64,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateHintRequest {
    pub content: Option<String>,
    pub cost_points: Option<i64>,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAttachmentRequest {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAttachmentRequest {
    pub label: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SubmissionsQuery {
    pub team_id: Option<i64>,
    pub challenge_id: Option<i64>,
    pub correct: Option<bool>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SubmissionRecord {
    pub id: i64,
    pub team_id: i64,
    pub user_id: i64,
    pub challenge_id: i64,
    pub flag: String,
    pub is_correct: bool,
    pub ip_address: Option<String>,
    pub submitted_at: i64,
    pub team_name: Option<String>,
    pub username: Option<String>,
    pub challenge_title: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PaginatedSubmissions {
    pub submissions: Vec<SubmissionRecord>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Deserialize)]
pub struct AnnounceRequest {
    pub title: String,
    pub body: String,
    pub challenge_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    pub attachments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportQuery {
    pub overwrite: Option<bool>,
    pub dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRoleRequest {
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserPasswordRequest {
    pub password: String,
    pub password_confirm: String,
}

#[derive(Debug, Serialize)]
pub struct AdminUser {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub team_id: Option<i64>,
    pub team_name: Option<String>,
}

/// `{"existing_id": 3}` or `{"new_name": "Red"}`; `null` means no team.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamAssignment {
    ExistingId(i64),
    NewName(String),
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub password_confirm: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub team: Option<TeamAssignment>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserTeamRequest {
    #[serde(default)]
    pub team: Option<TeamAssignment>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTeamDisqualifiedRequest {
    pub is_disqualified: bool,
}

// ---- admin auth middleware ----

pub async fn require_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)?;
    let claims = auth::verify_jwt(token, &state.config.auth.jwt_secret)?;
    if claims.role != "admin" {
        return Err(AppError::Forbidden);
    }
    if !auth::is_session_valid(&state.db, &auth::hash_token(token))? {
        return Err(AppError::Unauthorized);
    }
    Ok(next.run(request).await)
}

fn admin_audit_context(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(i64, Option<String>), AppError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)?;
    let claims = auth::verify_jwt(token, &state.config.auth.jwt_secret)?;
    if claims.role != "admin" {
        return Err(AppError::Forbidden);
    }
    Ok((claims.sub, request_ip(headers)))
}

fn request_ip(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned)
        })
}

// ---- dashboard ----

pub async fn dashboard(State(state): State<AppState>) -> HandlerResult<Json<serde_json::Value>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let challenge_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM challenges", [], |r| r.get(0))?;
    let team_count: i64 = conn.query_row("SELECT COUNT(*) FROM teams", [], |r| r.get(0))?;
    let user_count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
    let solve_count: i64 = conn.query_row("SELECT COUNT(*) FROM solves", [], |r| r.get(0))?;
    Ok(Json(serde_json::json!({
        "challenges": challenge_count,
        "teams": team_count,
        "users": user_count,
        "solves": solve_count,
    })))
}

// ---- challenge CRUD ----

pub async fn list_admin_challenges(
    State(state): State<AppState>,
) -> HandlerResult<Json<Vec<AdminChallenge>>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let challenges = Challenge::list_all(&conn)?
        .into_iter()
        .map(|challenge| {
            Ok(AdminChallenge {
                hint_count: Challenge::hint_count(&conn, challenge.id)?,
                file_count: Challenge::file_count(&conn, challenge.id)?,
                challenge,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(Json(challenges))
}

pub async fn create_challenge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateChallengeRequest>,
) -> HandlerResult<Json<Challenge>> {
    if req.title.trim().is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    if req.flag.trim().is_empty() {
        return Err(AppError::BadRequest("flag is required".into()));
    }
    validate_flag_settings(&req.flag_type, Some(&req.flag), req.unlock_requires, None)?;
    let slug = slugify(&req.title);
    let salt = generate_salt();
    let flag_hash = if req.flag_type == "regex" {
        req.flag.clone()
    } else {
        auth::hash_flag(&req.flag, &salt, req.flag_case_sensitive)
    };
    let tags_json = serde_json::to_string(&req.tags).unwrap_or_else(|_| "[]".into());
    let now = chrono::Utc::now().timestamp();

    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    // Regex patterns are already stored readable in flag_hash.
    let flag_ciphertext = if req.flag_type == "regex" {
        None
    } else {
        crate::flag_cipher::try_encrypt(&conn, req.flag.trim())
    };
    conn.execute(
        "INSERT INTO challenges (
            slug, title, description, category, flag_hash, flag_salt, flag_type,
            flag_case_sensitive, points, max_points, min_points, decay_rate,
            author, tags, unlock_requires, is_hidden, created_at, flag_ciphertext
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
        rusqlite::params![
            slug,
            req.title,
            req.description,
            req.category,
            flag_hash,
            salt,
            req.flag_type,
            req.flag_case_sensitive as i64,
            req.points,
            req.max_points,
            req.min_points,
            req.decay_rate,
            req.author,
            tags_json,
            req.unlock_requires,
            req.is_hidden as i64,
            now,
            flag_ciphertext,
        ],
    )?;
    let id = conn.last_insert_rowid();
    let challenge = Challenge::find_by_id(&conn, id)?
        .ok_or_else(|| anyhow::anyhow!("challenge not found after insert"))?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "challenge.create",
        Some(&format!("challenge:{id}")),
        Some(&challenge.title),
        ip.as_deref(),
    )?;
    state.cache.invalidate_challenges();
    Ok(Json(challenge))
}

pub async fn update_challenge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateChallengeRequest>,
) -> HandlerResult<Json<Challenge>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let existing = Challenge::find_by_id(&conn, id)?
        .ok_or_else(|| AppError::NotFound("challenge not found".into()))?;

    let title = req.title.as_deref().unwrap_or(&existing.title);
    let slug = if req.title.is_some() {
        slugify(title)
    } else {
        existing.slug.clone()
    };
    let flag_type = req.flag_type.as_deref().unwrap_or(&existing.flag_type);
    let case_sensitive = req
        .flag_case_sensitive
        .unwrap_or(existing.flag_case_sensitive);
    // The stored flag is a hash for static/dynamic and a pattern for regex, and
    // static hashes bake in case folding: changing either needs the flag again.
    let stored_form_changes = (flag_type == "regex") != (existing.flag_type == "regex")
        || (flag_type != "regex" && case_sensitive != existing.flag_case_sensitive);
    if stored_form_changes && req.flag.as_deref().is_none_or(|f| f.trim().is_empty()) {
        return Err(AppError::BadRequest(
            "changing flag type or case sensitivity requires entering the flag again".into(),
        ));
    }
    validate_flag_settings(
        flag_type,
        req.flag.as_deref(),
        req.unlock_requires.unwrap_or(existing.unlock_requires),
        Some(id),
    )?;
    let (flag_hash, flag_salt, flag_ciphertext) = if let Some(ref new_flag) = req.flag {
        let salt = generate_salt();
        if flag_type == "regex" {
            (new_flag.clone(), salt, None)
        } else {
            (
                auth::hash_flag(new_flag, &salt, case_sensitive),
                salt,
                crate::flag_cipher::try_encrypt(&conn, new_flag.trim()),
            )
        }
    } else {
        (
            existing.flag_hash.clone(),
            existing.flag_salt.clone(),
            existing.flag_ciphertext.clone(),
        )
    };
    let tags_json = req
        .tags
        .as_ref()
        .map(|t| serde_json::to_string(t).unwrap_or_else(|_| "[]".into()))
        .unwrap_or_else(|| existing.tags.clone().unwrap_or_else(|| "[]".into()));
    let author: Option<String> = req
        .author
        .unwrap_or_else(|| existing.author.clone().map(Some).unwrap_or(None));
    let unlock_requires: Option<i64> = req
        .unlock_requires
        .unwrap_or_else(|| existing.unlock_requires.map(Some).unwrap_or(None));

    conn.execute(
        "UPDATE challenges SET
            slug=?1, title=?2, description=?3, category=?4,
            flag_hash=?5, flag_salt=?6, flag_type=?7, flag_case_sensitive=?8,
            points=?9, max_points=?10, min_points=?11, decay_rate=?12,
            author=?13, tags=?14, unlock_requires=?15, is_hidden=?16, flag_ciphertext=?18
         WHERE id=?17",
        rusqlite::params![
            slug,
            title,
            req.description.as_deref().unwrap_or(&existing.description),
            req.category.as_deref().unwrap_or(&existing.category),
            flag_hash,
            flag_salt,
            flag_type,
            case_sensitive as i64,
            req.points.unwrap_or(existing.points),
            req.max_points.unwrap_or(existing.max_points),
            req.min_points.unwrap_or(existing.min_points),
            req.decay_rate.unwrap_or(existing.decay_rate),
            author,
            tags_json,
            unlock_requires,
            req.is_hidden
                .map(|b| b as i64)
                .unwrap_or(existing.is_hidden as i64),
            id,
            flag_ciphertext,
        ],
    )?;
    // Points, scoring mode or visibility may have changed team totals.
    scoring::recalculate_challenge_points(&conn, id)?;
    state.cache.invalidate_challenges();
    state.cache.invalidate_scoreboard();
    crate::handlers::scoreboard::broadcast_score_update(&state, &conn);
    let updated = Challenge::find_by_id(&conn, id)?
        .ok_or_else(|| anyhow::anyhow!("challenge not found after update"))?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "challenge.update",
        Some(&format!("challenge:{id}")),
        Some(&updated.title),
        ip.as_deref(),
    )?;
    Ok(Json(updated))
}

pub async fn delete_challenge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> HandlerResult<Json<serde_json::Value>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    // No foreign keys: remove dependent rows explicitly. Submissions are kept
    // as audit history.
    let tx = conn.unchecked_transaction()?;
    let rows = tx.execute(
        "DELETE FROM challenges WHERE id = ?1",
        rusqlite::params![id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound("challenge not found".into()));
    }
    tx.execute(
        "DELETE FROM hint_unlocks
         WHERE hint_id IN (SELECT id FROM hints WHERE challenge_id = ?1)",
        rusqlite::params![id],
    )?;
    tx.execute(
        "DELETE FROM hints WHERE challenge_id = ?1",
        rusqlite::params![id],
    )?;
    tx.execute(
        "DELETE FROM files WHERE challenge_id = ?1",
        rusqlite::params![id],
    )?;
    tx.execute(
        "DELETE FROM solves WHERE challenge_id = ?1",
        rusqlite::params![id],
    )?;
    tx.execute(
        "UPDATE challenges SET unlock_requires = NULL WHERE unlock_requires = ?1",
        rusqlite::params![id],
    )?;
    tx.commit()?;
    scoring::recalculate_all_team_scores(&conn)?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "challenge.delete",
        Some(&format!("challenge:{id}")),
        None,
        ip.as_deref(),
    )?;
    state.cache.invalidate_challenges();
    state.cache.invalidate_scoreboard();
    crate::handlers::scoreboard::broadcast_score_update(&state, &conn);
    Ok(Json(serde_json::json!({ "deleted": true })))
}

fn validate_flag_settings(
    flag_type: &str,
    flag: Option<&str>,
    unlock_requires: Option<i64>,
    challenge_id: Option<i64>,
) -> Result<(), AppError> {
    if !matches!(flag_type, "static" | "regex" | "dynamic") {
        return Err(AppError::BadRequest(
            "flag_type must be static, regex, or dynamic".into(),
        ));
    }
    if flag_type == "regex"
        && let Some(pattern) = flag
        && let Err(err) = regex::Regex::new(pattern)
    {
        return Err(AppError::BadRequest(format!("invalid regex flag: {err}")));
    }
    if unlock_requires.is_some() && unlock_requires == challenge_id {
        return Err(AppError::BadRequest(
            "a challenge cannot require itself".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct RevealedFlag {
    /// False for challenges created before reversible storage (hash only).
    pub stored: bool,
    pub flag_type: String,
    pub flag: Option<String>,
    /// Set when a stored copy exists but cannot be decrypted (e.g. the key
    /// row was replaced). Play is unaffected: submissions use the hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Admin-only flag reveal for verification. Audited; never used for checking.
pub async fn reveal_flag(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> HandlerResult<Json<RevealedFlag>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let challenge = require_challenge(&conn, id)?;
    let mut error = None;
    let flag = if challenge.flag_type == "regex" {
        Some(challenge.flag_hash.clone())
    } else {
        match challenge.flag_ciphertext.as_deref() {
            None => None,
            Some(ciphertext) => match crate::flag_cipher::decrypt(&conn, ciphertext) {
                Ok(flag) => Some(flag),
                Err(err) => {
                    error = Some(format!("{err}; re-enter the flag to store it again"));
                    None
                }
            },
        }
    };
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "challenge.flag_reveal",
        Some(&format!("challenge:{id}")),
        Some(&challenge.title),
        ip.as_deref(),
    )?;
    Ok(Json(RevealedFlag {
        stored: flag.is_some() || error.is_some(),
        flag_type: challenge.flag_type,
        flag,
        error,
    }))
}

// ---- hints ----

const MAX_HINT_LEN: usize = 4000;

fn validate_hint(content: &str, cost_points: i64, sort_order: Option<i64>) -> Result<(), AppError> {
    if content.trim().is_empty() {
        return Err(AppError::BadRequest("hint content is required".into()));
    }
    if content.chars().count() > MAX_HINT_LEN {
        return Err(AppError::BadRequest(format!(
            "hint content must be at most {MAX_HINT_LEN} characters"
        )));
    }
    if cost_points < 0 {
        return Err(AppError::BadRequest("hint cost cannot be negative".into()));
    }
    if sort_order.is_some_and(|order| order < 0) {
        return Err(AppError::BadRequest("hint order cannot be negative".into()));
    }
    Ok(())
}

fn require_challenge(conn: &crate::db::DbConn, id: i64) -> Result<Challenge, AppError> {
    Challenge::find_by_id(conn, id)?.ok_or_else(|| AppError::NotFound("challenge not found".into()))
}

pub async fn list_hints(
    State(state): State<AppState>,
    Path(challenge_id): Path<i64>,
) -> HandlerResult<Json<Vec<HintAdmin>>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    require_challenge(&conn, challenge_id)?;
    Ok(Json(Hint::list_admin(&conn, challenge_id)?))
}

pub async fn create_hint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(challenge_id): Path<i64>,
    Json(req): Json<CreateHintRequest>,
) -> HandlerResult<Json<Hint>> {
    validate_hint(&req.content, req.cost_points, req.sort_order)?;
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    require_challenge(&conn, challenge_id)?;
    let hint = Hint::create(
        &conn,
        challenge_id,
        req.content.trim(),
        req.cost_points,
        req.sort_order,
    )?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "hint.create",
        Some(&format!("hint:{}", hint.id)),
        Some(&format!(
            "challenge:{challenge_id} cost:{}",
            hint.cost_points
        )),
        ip.as_deref(),
    )?;
    state.cache.invalidate_challenges();
    Ok(Json(hint))
}

pub async fn update_hint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateHintRequest>,
) -> HandlerResult<Json<Hint>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let existing =
        Hint::find_by_id(&conn, id)?.ok_or_else(|| AppError::NotFound("hint not found".into()))?;
    let content = req
        .content
        .as_deref()
        .map(str::trim)
        .unwrap_or(&existing.content)
        .to_string();
    let cost_points = req.cost_points.unwrap_or(existing.cost_points);
    let sort_order = req.sort_order.unwrap_or(existing.sort_order);
    validate_hint(&content, cost_points, Some(sort_order))?;
    // Past unlocks keep the deduction recorded at unlock time.
    Hint::update(&conn, id, &content, cost_points, sort_order)?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "hint.update",
        Some(&format!("hint:{id}")),
        Some(&format!(
            "challenge:{} cost:{cost_points}",
            existing.challenge_id
        )),
        ip.as_deref(),
    )?;
    state.cache.invalidate_challenges();
    let updated = Hint::find_by_id(&conn, id)?
        .ok_or_else(|| anyhow::anyhow!("hint not found after update"))?;
    Ok(Json(updated))
}

pub async fn delete_hint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> HandlerResult<Json<serde_json::Value>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let existing =
        Hint::find_by_id(&conn, id)?.ok_or_else(|| AppError::NotFound("hint not found".into()))?;
    let refunded = Hint::delete(&conn, id)?;
    if refunded > 0 {
        scoring::recalculate_all_team_scores(&conn)?;
        state.cache.invalidate_scoreboard();
        crate::handlers::scoreboard::broadcast_score_update(&state, &conn);
    }
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "hint.delete",
        Some(&format!("hint:{id}")),
        Some(&format!(
            "challenge:{} refunded_unlocks:{refunded}",
            existing.challenge_id
        )),
        ip.as_deref(),
    )?;
    state.cache.invalidate_challenges();
    Ok(Json(
        serde_json::json!({ "deleted": true, "refunded_unlocks": refunded }),
    ))
}

// ---- attachments (external URLs) ----

fn validate_attachment_label(label: &str) -> Result<String, AppError> {
    let label = label.trim();
    if label.is_empty() {
        return Err(AppError::BadRequest("attachment label is required".into()));
    }
    Ok(label.to_string())
}

pub async fn list_files(
    State(state): State<AppState>,
    Path(challenge_id): Path<i64>,
) -> HandlerResult<Json<Vec<ChallengeFile>>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    require_challenge(&conn, challenge_id)?;
    Ok(Json(ChallengeFile::list_by_challenge(&conn, challenge_id)?))
}

pub async fn create_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(challenge_id): Path<i64>,
    Json(req): Json<CreateAttachmentRequest>,
) -> HandlerResult<Json<ChallengeFile>> {
    let label = validate_attachment_label(&req.label)?;
    let url = validate_attachment_url(&req.url)?;
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    require_challenge(&conn, challenge_id)?;
    let file = ChallengeFile::create(&conn, challenge_id, &label, &url)?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "file.create",
        Some(&format!("file:{}", file.id)),
        Some(&format!("challenge:{challenge_id} {url}")),
        ip.as_deref(),
    )?;
    state.cache.invalidate_challenges();
    Ok(Json(file))
}

pub async fn update_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateAttachmentRequest>,
) -> HandlerResult<Json<ChallengeFile>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let existing = ChallengeFile::find_by_id(&conn, id)?
        .ok_or_else(|| AppError::NotFound("attachment not found".into()))?;
    let label = validate_attachment_label(req.label.as_deref().unwrap_or(&existing.filename))?;
    // Legacy relative paths must be replaced by an absolute URL on any save.
    let url = validate_attachment_url(req.url.as_deref().unwrap_or(&existing.storage_path))?;
    ChallengeFile::update(&conn, id, &label, &url)?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "file.update",
        Some(&format!("file:{id}")),
        Some(&format!("challenge:{} {url}", existing.challenge_id)),
        ip.as_deref(),
    )?;
    state.cache.invalidate_challenges();
    let updated = ChallengeFile::find_by_id(&conn, id)?
        .ok_or_else(|| anyhow::anyhow!("attachment not found after update"))?;
    Ok(Json(updated))
}

pub async fn delete_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> HandlerResult<Json<serde_json::Value>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let existing = ChallengeFile::find_by_id(&conn, id)?
        .ok_or_else(|| AppError::NotFound("attachment not found".into()))?;
    ChallengeFile::delete(&conn, id)?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "file.delete",
        Some(&format!("file:{id}")),
        Some(&format!("challenge:{}", existing.challenge_id)),
        ip.as_deref(),
    )?;
    state.cache.invalidate_challenges();
    Ok(Json(serde_json::json!({ "deleted": true })))
}

// ---- submission log ----

pub async fn list_submissions(
    State(state): State<AppState>,
    Query(q): Query<SubmissionsQuery>,
) -> HandlerResult<Json<PaginatedSubmissions>> {
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(50).clamp(1, 200);
    let offset = (page - 1) * per_page;
    let correct_filter: Option<i64> = q.correct.map(|b| b as i64);

    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;

    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM submissions
         WHERE (?1 IS NULL OR team_id = ?1)
           AND (?2 IS NULL OR challenge_id = ?2)
           AND (?3 IS NULL OR is_correct = ?3)",
        rusqlite::params![q.team_id, q.challenge_id, correct_filter],
        |r| r.get(0),
    )?;

    let mut stmt = conn.prepare(
        "SELECT s.id, s.team_id, s.user_id, s.challenge_id, s.flag, s.is_correct,
                s.ip_address, s.submitted_at, t.name, u.username, c.title
         FROM submissions s
         LEFT JOIN teams t ON t.id = s.team_id
         LEFT JOIN users u ON u.id = s.user_id
         LEFT JOIN challenges c ON c.id = s.challenge_id
         WHERE (?1 IS NULL OR s.team_id = ?1)
           AND (?2 IS NULL OR s.challenge_id = ?2)
           AND (?3 IS NULL OR s.is_correct = ?3)
         ORDER BY s.submitted_at DESC, s.id DESC
         LIMIT ?4 OFFSET ?5",
    )?;
    let submissions = stmt
        .query_map(
            rusqlite::params![q.team_id, q.challenge_id, correct_filter, per_page, offset],
            |row| {
                Ok(SubmissionRecord {
                    id: row.get(0)?,
                    team_id: row.get(1)?,
                    user_id: row.get(2)?,
                    challenge_id: row.get(3)?,
                    flag: row.get(4)?,
                    is_correct: row.get::<_, i64>(5)? != 0,
                    ip_address: row.get(6)?,
                    submitted_at: row.get(7)?,
                    team_name: row.get(8)?,
                    username: row.get(9)?,
                    challenge_title: row.get(10)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(PaginatedSubmissions {
        submissions,
        total,
        page,
        per_page,
    }))
}

// ---- user management ----

pub async fn get_users(State(state): State<AppState>) -> HandlerResult<Json<Vec<AdminUser>>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let mut stmt = conn.prepare(
        "SELECT u.id, u.username, u.role, u.team_id, t.name
         FROM users u LEFT JOIN teams t ON t.id = u.team_id
         ORDER BY u.id",
    )?;
    let users = stmt
        .query_map([], |row| {
            Ok(AdminUser {
                id: row.get(0)?,
                username: row.get(1)?,
                role: row.get(2)?,
                team_id: row.get(3)?,
                team_name: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(users))
}

pub async fn ban_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> HandlerResult<Json<serde_json::Value>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let rows = conn.execute(
        "UPDATE users SET role = 'banned' WHERE id = ?1 AND role != 'admin'",
        rusqlite::params![id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound("user not found or is admin".into()));
    }
    conn.execute(
        "UPDATE sessions SET revoked = 1 WHERE user_id = ?1",
        rusqlite::params![id],
    )?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "user.ban",
        Some(&format!("user:{id}")),
        None,
        ip.as_deref(),
    )?;
    Ok(Json(serde_json::json!({ "banned": true })))
}

pub async fn update_user_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUserRoleRequest>,
) -> HandlerResult<Json<UserPublic>> {
    let role = req.role.trim();
    if !matches!(role, "admin" | "player" | "banned") {
        return Err(AppError::BadRequest(
            "role must be admin, player, or banned".into(),
        ));
    }

    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let existing_role = conn
        .query_row(
            "SELECT role FROM users WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => AppError::NotFound("user not found".into()),
            other => AppError::Database(other),
        })?;

    if existing_role == "admin" && role == "banned" {
        return Err(AppError::BadRequest(
            "remove admin rights before banning this user".into(),
        ));
    }
    if existing_role == "admin" && role != "admin" {
        let admin_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users WHERE role = 'admin'",
            [],
            |row| row.get(0),
        )?;
        if admin_count <= 1 {
            return Err(AppError::BadRequest(
                "cannot remove the last admin account".into(),
            ));
        }
    }

    conn.execute(
        "UPDATE users SET role = ?1 WHERE id = ?2",
        rusqlite::params![role, id],
    )?;
    conn.execute(
        "UPDATE sessions SET revoked = 1 WHERE user_id = ?1",
        rusqlite::params![id],
    )?;

    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "user.role_update",
        Some(&format!("user:{id}")),
        Some(&format!("{existing_role}->{role}")),
        ip.as_deref(),
    )?;

    let user = conn.query_row(
        "SELECT id, username, role, team_id FROM users WHERE id = ?1",
        rusqlite::params![id],
        |row| {
            Ok(UserPublic {
                id: row.get(0)?,
                username: row.get(1)?,
                role: row.get(2)?,
                team_id: row.get(3)?,
            })
        },
    )?;
    Ok(Json(user))
}

pub async fn update_user_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUserPasswordRequest>,
) -> HandlerResult<Json<serde_json::Value>> {
    if req.password.len() < 8 {
        return Err(AppError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }
    if req.password != req.password_confirm {
        return Err(AppError::BadRequest("passwords do not match".into()));
    }

    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let username = conn
        .query_row(
            "SELECT username FROM users WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => AppError::NotFound("user not found".into()),
            other => AppError::Database(other),
        })?;

    let password_hash = auth::hash_password(&req.password)?;
    conn.execute(
        "UPDATE users SET password_hash = ?1 WHERE id = ?2",
        rusqlite::params![password_hash, id],
    )?;
    drop(conn);
    auth::revoke_user_sessions(&state.db, id)?;

    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "user.password_update",
        Some(&format!("user:{id}")),
        Some(&username),
        ip.as_deref(),
    )?;

    Ok(Json(serde_json::json!({ "updated": true })))
}

fn admin_user_by_id(conn: &crate::db::DbConn, id: i64) -> Result<AdminUser, AppError> {
    conn.query_row(
        "SELECT u.id, u.username, u.role, u.team_id, t.name
         FROM users u LEFT JOIN teams t ON t.id = u.team_id
         WHERE u.id = ?1",
        rusqlite::params![id],
        |row| {
            Ok(AdminUser {
                id: row.get(0)?,
                username: row.get(1)?,
                role: row.get(2)?,
                team_id: row.get(3)?,
                team_name: row.get(4)?,
            })
        },
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => AppError::NotFound("user not found".into()),
        other => AppError::Database(other),
    })
}

/// Check a team assignment before anything is written, so a bad request
/// never leaves a half-created user or team behind.
fn check_team_assignment(
    conn: &crate::db::DbConn,
    assignment: Option<&TeamAssignment>,
    max_team_size: u32,
) -> Result<(), AppError> {
    match assignment {
        None => Ok(()),
        Some(TeamAssignment::ExistingId(team_id)) => {
            Team::find_by_id(conn, *team_id)?
                .ok_or_else(|| AppError::BadRequest("team not found".into()))?;
            Team::ensure_has_room(conn, *team_id, max_team_size)
        }
        Some(TeamAssignment::NewName(name)) => {
            let name = name.trim();
            if name.is_empty() {
                return Err(AppError::BadRequest("team name is required".into()));
            }
            if Team::find_by_name(conn, name)?.is_some() {
                return Err(AppError::BadRequest("team name already taken".into()));
            }
            Ok(())
        }
    }
}

/// Apply a checked assignment. Returns the resulting team id.
fn apply_team_assignment(
    conn: &crate::db::DbConn,
    user_id: i64,
    assignment: Option<&TeamAssignment>,
) -> Result<Option<i64>, AppError> {
    let team_id = match assignment {
        None => {
            conn.execute(
                "UPDATE users SET team_id = NULL WHERE id = ?1",
                rusqlite::params![user_id],
            )?;
            return Ok(None);
        }
        Some(TeamAssignment::ExistingId(team_id)) => *team_id,
        Some(TeamAssignment::NewName(name)) => Team::create(conn, name.trim())?.id,
    };
    Team::add_member(conn, team_id, user_id)?;
    Ok(Some(team_id))
}

pub async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateUserRequest>,
) -> HandlerResult<Json<AdminUser>> {
    let username = req.username.trim();
    crate::handlers::auth::validate_username(username)?;
    if req.password.len() < 8 {
        return Err(AppError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }
    if req.password != req.password_confirm {
        return Err(AppError::BadRequest("passwords do not match".into()));
    }
    let role = req.role.as_deref().unwrap_or("player").trim();
    if !matches!(role, "player" | "admin") {
        return Err(AppError::BadRequest("role must be player or admin".into()));
    }

    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    if crate::models::user::User::find_by_username(&conn, username)?.is_some() {
        return Err(AppError::BadRequest("username already taken".into()));
    }
    check_team_assignment(
        &conn,
        req.team.as_ref(),
        state.config.competition.max_team_size,
    )?;

    // Admin-created accounts bypass `registration_open` and get no session:
    // the person logs in with the assigned password.
    let password_hash = auth::hash_password(&req.password)?;
    let user = crate::models::user::User::create(
        &conn,
        &crate::models::user::RegisterRequest {
            username: username.to_string(),
            email: None,
            password: String::new(),
            team_name: None,
            invite_code: None,
        },
        &password_hash,
    )?;
    if role == "admin" {
        conn.execute(
            "UPDATE users SET role = 'admin' WHERE id = ?1",
            rusqlite::params![user.id],
        )?;
    }
    apply_team_assignment(&conn, user.id, req.team.as_ref())?;
    let created = admin_user_by_id(&conn, user.id)?;

    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "user.create",
        Some(&format!("user:{}", user.id)),
        Some(&format!(
            "{} role:{role} team:{}",
            created.username,
            created.team_name.as_deref().unwrap_or("-")
        )),
        ip.as_deref(),
    )?;
    state.cache.invalidate_scoreboard();
    Ok(Json(created))
}

pub async fn update_user_team(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUserTeamRequest>,
) -> HandlerResult<Json<AdminUser>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let existing = admin_user_by_id(&conn, id)?;
    if let Some(TeamAssignment::ExistingId(team_id)) = req.team
        && existing.team_id == Some(team_id)
    {
        return Ok(Json(existing));
    }
    check_team_assignment(
        &conn,
        req.team.as_ref(),
        state.config.competition.max_team_size,
    )?;
    apply_team_assignment(&conn, id, req.team.as_ref())?;
    // Past solves stay with the team that earned them; totals only need a
    // refresh for display.
    scoring::recalculate_all_team_scores(&conn)?;
    let updated = admin_user_by_id(&conn, id)?;
    drop(conn);
    // The JWT carries team_id, so force a fresh login.
    auth::revoke_user_sessions(&state.db, id)?;

    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "user.team_update",
        Some(&format!("user:{id}")),
        Some(&format!(
            "{}->{}",
            existing.team_name.as_deref().unwrap_or("-"),
            updated.team_name.as_deref().unwrap_or("-")
        )),
        ip.as_deref(),
    )?;
    state.cache.invalidate_scoreboard();
    Ok(Json(updated))
}

// ---- team management ----

pub async fn get_teams(State(state): State<AppState>) -> HandlerResult<Json<Vec<Team>>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let mut stmt = conn.prepare(
        "SELECT id, name, invite_code, score, last_solve_at, is_disqualified FROM teams ORDER BY score DESC",
    )?;
    let teams = stmt
        .query_map([], |row| {
            Ok(Team {
                id: row.get(0)?,
                name: row.get(1)?,
                invite_code: row.get(2)?,
                score: row.get(3)?,
                last_solve_at: row.get(4)?,
                is_disqualified: row.get::<_, i64>(5)? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(teams))
}

pub async fn disqualify_team(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> HandlerResult<Json<serde_json::Value>> {
    let Json(result) = update_team_disqualified(
        State(state),
        headers,
        Path(id),
        Json(UpdateTeamDisqualifiedRequest {
            is_disqualified: true,
        }),
    )
    .await?;
    Ok(Json(serde_json::json!({
        "disqualified": result.is_disqualified
    })))
}

pub async fn update_team_disqualified(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateTeamDisqualifiedRequest>,
) -> HandlerResult<Json<Team>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let existing: i64 = conn
        .query_row(
            "SELECT is_disqualified FROM teams WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .map_err(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => AppError::NotFound("team not found".into()),
            other => AppError::Database(other),
        })?;
    let new_value = i64::from(req.is_disqualified);
    let rows = conn.execute(
        "UPDATE teams SET is_disqualified = ?1, last_solve_at = CASE WHEN ?1 = 1 THEN NULL ELSE last_solve_at END WHERE id = ?2",
        rusqlite::params![new_value, id],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound("team not found".into()));
    }
    scoring::recalculate_all_team_scores(&conn)?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    let action = if req.is_disqualified {
        "team.disqualify"
    } else {
        "team.reinstate"
    };
    crate::db::audit(
        &conn,
        admin_id,
        action,
        Some(&format!("team:{id}")),
        Some(&format!("{}->{}", existing != 0, req.is_disqualified)),
        ip.as_deref(),
    )?;
    state.cache.invalidate_scoreboard();
    let team = Team::find_by_id(&conn, id)?
        .ok_or_else(|| anyhow::anyhow!("team not found after update"))?;
    Ok(Json(team))
}

// ---- settings ----

/// Competition settings come from config.toml and are read-only at runtime.
pub async fn get_settings(State(state): State<AppState>) -> HandlerResult<Json<serde_json::Value>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let branding = crate::competition::branding(&conn, &state.config.competition)?;
    Ok(Json(serde_json::json!({
        "competition": state.config.competition,
        "branding": branding,
        "max_import_mb": state.config.storage.max_file_size_mb,
    })))
}

#[derive(Debug, Deserialize)]
pub struct UpdateBrandingRequest {
    /// Empty or null reverts to `competition.name` from config.toml.
    #[serde(default)]
    pub name: Option<String>,
    /// Empty or null shows the built-in logo.
    #[serde(default)]
    pub logo_url: Option<String>,
}

pub async fn update_branding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UpdateBrandingRequest>,
) -> HandlerResult<Json<crate::competition::Branding>> {
    let name = req.name.as_deref().map(str::trim).filter(|n| !n.is_empty());
    if name.is_some_and(|n| n.chars().count() > crate::competition::MAX_NAME_LEN) {
        return Err(AppError::BadRequest(format!(
            "competition name must be at most {} characters",
            crate::competition::MAX_NAME_LEN
        )));
    }
    let logo_url = req
        .logo_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty());
    if logo_url.is_some_and(|u| !crate::models::challenge::is_absolute_attachment_url(u)) {
        return Err(AppError::BadRequest(
            "logo url must be an absolute http:// or https:// URL".into(),
        ));
    }

    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    crate::competition::set_branding(&conn, name, logo_url, chrono::Utc::now().timestamp())?;
    state.cache.invalidate_branding();
    let branding = crate::competition::branding(&conn, &state.config.competition)?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "settings.branding",
        Some("branding"),
        Some(&format!(
            "{} logo:{}",
            branding.name,
            branding.logo_url.as_deref().unwrap_or("-")
        )),
        ip.as_deref(),
    )?;
    // Clients refresh /api/competition (name + logo) on state_change.
    broadcast_competition_state(&state, &conn)?;
    Ok(Json(branding))
}

// ---- competition controls ----

fn broadcast_competition_state(state: &AppState, conn: &crate::db::DbConn) -> Result<(), AppError> {
    let status = crate::competition::status(conn, &state.config.competition)?;
    state.ws_hub.broadcast(WsEvent::StateChange {
        started: status.started,
        ended: status.ended,
        frozen: status.is_frozen(chrono::Utc::now().timestamp()),
    });
    // Freezing or reopening changes what the public scoreboard shows.
    crate::handlers::scoreboard::broadcast_score_update(state, conn);
    Ok(())
}

pub async fn competition_start(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HandlerResult<Json<serde_json::Value>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    crate::competition::start(&conn, chrono::Utc::now().timestamp())?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "competition.start",
        Some("competition"),
        None,
        ip.as_deref(),
    )?;
    broadcast_competition_state(&state, &conn)?;
    Ok(Json(serde_json::json!({ "started": true })))
}

pub async fn competition_end(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HandlerResult<Json<serde_json::Value>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    crate::competition::end(&conn, chrono::Utc::now().timestamp())?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "competition.end",
        Some("competition"),
        None,
        ip.as_deref(),
    )?;
    broadcast_competition_state(&state, &conn)?;
    Ok(Json(serde_json::json!({ "ended": true })))
}

pub async fn competition_freeze(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HandlerResult<Json<serde_json::Value>> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    crate::competition::freeze(&conn, chrono::Utc::now().timestamp())?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "competition.freeze",
        Some("competition"),
        None,
        ip.as_deref(),
    )?;
    broadcast_competition_state(&state, &conn)?;
    Ok(Json(serde_json::json!({ "frozen": true })))
}

pub async fn announce(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AnnounceRequest>,
) -> HandlerResult<Json<serde_json::Value>> {
    if req.title.trim().is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT INTO announcements (title, body, challenge_id, is_visible, created_at)
         VALUES (?1, ?2, ?3, 1, ?4)",
        rusqlite::params![req.title, req.body, req.challenge_id, now],
    )?;
    let (admin_id, ip) = admin_audit_context(&state, &headers)?;
    crate::db::audit(
        &conn,
        admin_id,
        "announcement.create",
        Some("announcement"),
        Some(&req.title),
        ip.as_deref(),
    )?;
    state.ws_hub.broadcast(WsEvent::Announcement {
        title: req.title,
        body: req.body,
    });
    Ok(Json(serde_json::json!({ "sent": true })))
}

// ---- import / export ----

pub async fn export_bundle(
    State(state): State<AppState>,
    Query(query): Query<ExportQuery>,
) -> HandlerResult<Response> {
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    if matches!(query.attachments.as_deref(), Some("zip")) {
        let zip_bytes = import_export::export_zip(&conn, &state.config)?;
        axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .header(axum::http::header::CONTENT_TYPE, "application/zip")
            .header(
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"feralctf-export.zip\"",
            )
            .body(axum::body::Body::from(zip_bytes))
            .map_err(|e| anyhow::anyhow!("response build: {e}"))
            .map(Ok)?
    } else {
        let inline_attachments = matches!(query.attachments.as_deref(), Some("inline"));
        let bundle = import_export::export(&conn, &state.config, inline_attachments)?;
        Ok(Json(bundle).into_response())
    }
}

pub async fn import_bundle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ImportQuery>,
    mut multipart: Multipart,
) -> HandlerResult<Json<ImportResult>> {
    let mut file_bytes = None;
    let mut attachment_zip: Option<Vec<u8>> = None;
    let mut overwrite = query.overwrite.unwrap_or(false);
    let mut dry_run = query.dry_run.unwrap_or(false);
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| AppError::BadRequest(format!("invalid multipart import: {err}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|err| AppError::BadRequest(format!("invalid import file: {err}")))?;
                file_bytes = Some(bytes.to_vec());
            }
            "attachments" => {
                let bytes = field.bytes().await.map_err(|err| {
                    AppError::BadRequest(format!("invalid attachments zip: {err}"))
                })?;
                attachment_zip = Some(bytes.to_vec());
            }
            "overwrite" => {
                let value = field.text().await.map_err(|err| {
                    AppError::BadRequest(format!("invalid overwrite field: {err}"))
                })?;
                overwrite = parse_bool(&value);
            }
            "dry_run" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| AppError::BadRequest(format!("invalid dry_run field: {err}")))?;
                dry_run = parse_bool(&value);
            }
            _ => {}
        }
    }
    let file_bytes = file_bytes.ok_or_else(|| AppError::BadRequest("file is required".into()))?;
    let bundle = import_export::detect_and_convert_ctfd(&file_bytes)?;
    let conn = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let options = ImportOptions { overwrite, dry_run };
    let attachments_dir = std::path::Path::new(&state.config.storage.attachments_path);
    if !dry_run && let Some(zip_bytes) = attachment_zip {
        import_export::extract_attachments_zip(&zip_bytes, attachments_dir)?;
    }
    let result = import_export::import(&conn, &bundle, Some(attachments_dir), &options)?;
    if !options.dry_run && result.valid {
        let (admin_id, ip) = admin_audit_context(&state, &headers)?;
        crate::db::audit(
            &conn,
            admin_id,
            "bundle.import",
            Some("import"),
            Some(&format!("created {}", result.challenges_created)),
            ip.as_deref(),
        )?;
        state.cache.invalidate_challenges();
        state.cache.invalidate_scoreboard();
    }
    Ok(Json(result))
}

// ---- backup ----

pub async fn backup(State(state): State<AppState>) -> Response {
    let result = do_backup(&state);
    match result {
        Ok((bytes, filename)) => axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .header(axum::http::header::CONTENT_TYPE, "application/octet-stream")
            .header(
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            )
            .body(axum::body::Body::from(bytes))
            .unwrap_or_else(|e| {
                AppError::Internal(anyhow::anyhow!("response build: {e}")).into_response()
            }),
        Err(e) => e.into_response(),
    }
}

fn do_backup(state: &AppState) -> Result<(Vec<u8>, String), AppError> {
    let timestamp = chrono::Utc::now().timestamp();
    let filename = format!("feralctf-backup-{timestamp}.db");
    let tmp_path =
        std::env::temp_dir().join(format!("feralctf-backup-{}.db", uuid::Uuid::new_v4()));
    let src = state
        .db
        .get()
        .map_err(|e| anyhow::anyhow!("db pool: {e}"))?;
    let mut dst =
        rusqlite::Connection::open(&tmp_path).map_err(|e| anyhow::anyhow!("backup open: {e}"))?;
    {
        let backup = rusqlite::backup::Backup::new(&src, &mut dst)
            .map_err(|e| anyhow::anyhow!("backup init: {e}"))?;
        backup
            .run_to_completion(100, std::time::Duration::ZERO, None)
            .map_err(|e| anyhow::anyhow!("backup run: {e}"))?;
    }
    drop(dst);
    let bytes = std::fs::read(&tmp_path).map_err(|e| anyhow::anyhow!("backup read: {e}"))?;
    let _ = std::fs::remove_file(&tmp_path);
    Ok((bytes, filename))
}

// ---- helpers ----

fn slugify(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c == ' ' { '-' } else { c })
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect()
}

fn generate_salt() -> String {
    use rand::RngCore;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let mut out = String::with_capacity(32);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

// ---- tests ----

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        WsHub, auth,
        cache::AppCache,
        config::Config,
        db,
        models::user::{RegisterRequest, User},
    };
    use axum::extract::State;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::sync::Arc;

    fn test_state() -> AppState {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        {
            let conn = pool.get().unwrap();
            db::run_migrations(&conn).unwrap();
        }
        let mut config = Config::default();
        config.auth.jwt_secret = "test-secret".to_string();
        AppState {
            db: pool,
            config: Arc::new(config),
            cache: Arc::new(AppCache::new()),
            ws_hub: Arc::new(WsHub::new()),
            rate_limiter: Arc::new(crate::anticheat::RateLimiter::new()),
        }
    }

    fn make_token(state: &AppState, role: &str) -> String {
        let (user_id, token) = {
            let conn = state.db.get().unwrap();
            let req = RegisterRequest {
                username: format!("user-{role}"),
                email: None,
                password: "password123".to_string(),
                team_name: None,
                invite_code: None,
            };
            let user = User::create(&conn, &req, "hash").unwrap();
            if role == "admin" {
                conn.execute(
                    "UPDATE users SET role = 'admin' WHERE id = ?1",
                    rusqlite::params![user.id],
                )
                .unwrap();
            }
            let now = chrono::Utc::now().timestamp() as u64;
            let claims = auth::Claims {
                sub: user.id,
                role: role.to_string(),
                team_id: None,
                iat: now,
                exp: now + 3600,
            };
            let token = auth::sign_jwt(&claims, &state.config.auth.jwt_secret).unwrap();
            (user.id, token)
        }; // conn dropped here — pool slot free for create_session
        auth::create_session(&state.db, user_id, &token, 1).unwrap();
        token
    }

    fn admin_headers(state: &AppState) -> HeaderMap {
        let token = make_token(state, "admin");
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        h
    }

    fn player_headers(state: &AppState) -> HeaderMap {
        let token = make_token(state, "player");
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        h
    }

    fn verify_admin_check(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
        let token = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?;
        let claims = auth::verify_jwt(token, &state.config.auth.jwt_secret)?;
        if claims.role != "admin" {
            return Err(AppError::Forbidden);
        }
        if !auth::is_session_valid(&state.db, &auth::hash_token(token))? {
            return Err(AppError::Unauthorized);
        }
        Ok(())
    }

    #[test]
    fn non_admin_jwt_returns_forbidden() {
        let state = test_state();
        let headers = player_headers(&state);
        let result = verify_admin_check(&state, &headers);
        assert!(matches!(result, Err(AppError::Forbidden)));
    }

    #[test]
    fn admin_jwt_passes_check() {
        let state = test_state();
        let headers = admin_headers(&state);
        assert!(verify_admin_check(&state, &headers).is_ok());
    }

    #[tokio::test]
    async fn create_challenge_hashes_flag() {
        let state = test_state();
        let req = CreateChallengeRequest {
            title: "Test Chal".to_string(),
            category: "web".to_string(),
            description: "desc".to_string(),
            flag: "flag{secret}".to_string(),
            flag_type: "static".to_string(),
            flag_case_sensitive: false,
            points: 100,
            max_points: 500,
            min_points: 50,
            decay_rate: 12,
            author: None,
            tags: vec!["web".to_string()],
            unlock_requires: None,
            is_hidden: true,
        };
        let headers = admin_headers(&state);
        let Json(challenge) = create_challenge(State(state), headers, Json(req))
            .await
            .unwrap();
        assert_ne!(challenge.flag_hash, "flag{secret}");
        assert_eq!(challenge.slug, "test-chal");
        assert!(!challenge.flag_hash.is_empty());
    }

    #[tokio::test]
    async fn disqualify_sets_score_to_zero_and_invalidates_cache() {
        let state = test_state();
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO teams (id, name, invite_code, score) VALUES (1, 'Cheaters', 'CHEAT123', 500)",
                [],
            )
            .unwrap();
        }
        // Populate cache so we can verify invalidation
        {
            let conn = state.db.get().unwrap();
            state.cache.get_or_build_scoreboard(&conn).unwrap();
        }
        assert!(state.cache.is_scoreboard_cached());

        let headers = admin_headers(&state);
        let Json(result) = disqualify_team(State(state.clone()), headers, Path(1))
            .await
            .unwrap();
        assert_eq!(result["disqualified"], true);
        assert!(!state.cache.is_scoreboard_cached());

        let conn = state.db.get().unwrap();
        let score: i64 = conn
            .query_row("SELECT score FROM teams WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(score, 0);
    }

    #[tokio::test]
    async fn ban_user_sets_role_to_banned() {
        let state = test_state();
        let conn = state.db.get().unwrap();
        let req = RegisterRequest {
            username: "target".to_string(),
            email: None,
            password: "password123".to_string(),
            team_name: None,
            invite_code: None,
        };
        let user = User::create(&conn, &req, "hash").unwrap();
        drop(conn);

        let headers = admin_headers(&state);
        let Json(result) = ban_user(State(state.clone()), headers, Path(user.id))
            .await
            .unwrap();
        assert_eq!(result["banned"], true);

        let conn = state.db.get().unwrap();
        let role: String = conn
            .query_row(
                "SELECT role FROM users WHERE id = ?1",
                rusqlite::params![user.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(role, "banned");
    }

    #[tokio::test]
    async fn update_user_role_promotes_demotes_bans_and_unbans() {
        let state = test_state();
        let conn = state.db.get().unwrap();
        let req = RegisterRequest {
            username: "role-target".to_string(),
            email: None,
            password: "password123".to_string(),
            team_name: None,
            invite_code: None,
        };
        let user = User::create(&conn, &req, "hash").unwrap();
        drop(conn);

        let headers = admin_headers(&state);
        let Json(promoted) = update_user_role(
            State(state.clone()),
            headers.clone(),
            Path(user.id),
            Json(UpdateUserRoleRequest {
                role: "admin".to_string(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(promoted.role, "admin");

        let Json(demoted) = update_user_role(
            State(state.clone()),
            headers.clone(),
            Path(user.id),
            Json(UpdateUserRoleRequest {
                role: "player".to_string(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(demoted.role, "player");

        let Json(banned) = update_user_role(
            State(state.clone()),
            headers.clone(),
            Path(user.id),
            Json(UpdateUserRoleRequest {
                role: "banned".to_string(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(banned.role, "banned");

        let Json(unbanned) = update_user_role(
            State(state),
            headers,
            Path(user.id),
            Json(UpdateUserRoleRequest {
                role: "player".to_string(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(unbanned.role, "player");
    }

    #[tokio::test]
    async fn update_user_role_rejects_removing_last_admin() {
        let state = test_state();
        let headers = admin_headers(&state);
        let admin_id: i64 = {
            let conn = state.db.get().unwrap();
            conn.query_row(
                "SELECT id FROM users WHERE username = 'user-admin'",
                [],
                |row| row.get(0),
            )
            .unwrap()
        };

        let result = update_user_role(
            State(state),
            headers,
            Path(admin_id),
            Json(UpdateUserRoleRequest {
                role: "player".to_string(),
            }),
        )
        .await;

        assert!(matches!(result, Err(AppError::BadRequest(_))));
    }

    #[tokio::test]
    async fn update_team_disqualified_bans_and_reinstates_team_scores() {
        let state = test_state();
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO teams (id, name, invite_code, score) VALUES (1, 'Toggle Team', 'TOGGLE123', 0)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO users (id, username, password_hash, role, team_id, created_at)
                 VALUES (42, 'solver', 'hash', 'player', 1, 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO challenges (
                    id, slug, title, description, category, flag_hash, flag_salt, flag_type,
                    flag_case_sensitive, points, max_points, min_points, decay_rate, created_at
                ) VALUES (5, 'toggle', 'Toggle', 'desc', 'misc', 'hash', 'salt', 'static',
                    0, 250, 250, 50, 12, 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO solves (team_id, user_id, challenge_id, solved_at)
                 VALUES (1, 42, 5, 10)",
                [],
            )
            .unwrap();
            scoring::recalculate_all_team_scores(&conn).unwrap();
            state.cache.get_or_build_scoreboard(&conn).unwrap();
        }
        assert!(state.cache.is_scoreboard_cached());

        let headers = admin_headers(&state);
        let Json(banned) = update_team_disqualified(
            State(state.clone()),
            headers.clone(),
            Path(1),
            Json(UpdateTeamDisqualifiedRequest {
                is_disqualified: true,
            }),
        )
        .await
        .unwrap();
        assert!(banned.is_disqualified);
        assert_eq!(banned.score, 0);
        assert!(!state.cache.is_scoreboard_cached());

        let Json(reinstated) = update_team_disqualified(
            State(state),
            headers,
            Path(1),
            Json(UpdateTeamDisqualifiedRequest {
                is_disqualified: false,
            }),
        )
        .await
        .unwrap();
        assert!(!reinstated.is_disqualified);
        assert_eq!(reinstated.score, 250);
    }

    #[tokio::test]
    async fn audit_log_records_required_admin_actions() {
        let state = test_state();
        let headers = admin_headers(&state);
        let req = CreateChallengeRequest {
            title: "Audit Chal".to_string(),
            category: "web".to_string(),
            description: "desc".to_string(),
            flag: "flag{audit}".to_string(),
            flag_type: "static".to_string(),
            flag_case_sensitive: true,
            points: 100,
            max_points: 500,
            min_points: 50,
            decay_rate: 12,
            author: None,
            tags: Vec::new(),
            unlock_requires: None,
            is_hidden: false,
        };

        let Json(challenge) = create_challenge(State(state.clone()), headers.clone(), Json(req))
            .await
            .unwrap();
        let _ = delete_challenge(State(state.clone()), headers.clone(), Path(challenge.id))
            .await
            .unwrap();
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO teams (id, name, invite_code, score) VALUES (7, 'Audit Team', 'AUDIT123', 500)",
                [],
            )
            .unwrap();
        }
        let _ = disqualify_team(State(state.clone()), headers, Path(7))
            .await
            .unwrap();

        let conn = state.db.get().unwrap();
        for action in ["challenge.create", "challenge.delete", "team.disqualify"] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM audit_log WHERE action = ?1",
                    rusqlite::params![action],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "missing audit action {action}");
        }
    }

    #[tokio::test]
    async fn admin_password_update_rejects_mismatch() {
        let state = test_state();
        let headers = admin_headers(&state);
        let target_id = {
            let conn = state.db.get().unwrap();
            let req = RegisterRequest {
                username: "target".to_string(),
                email: None,
                password: "password123".to_string(),
                team_name: None,
                invite_code: None,
            };
            User::create(&conn, &req, "hash").unwrap().id
        };

        let err = update_user_password(
            State(state),
            headers,
            Path(target_id),
            Json(UpdateUserPasswordRequest {
                password: "new-password123".to_string(),
                password_confirm: "different-password123".to_string(),
            }),
        )
        .await
        .unwrap_err();

        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[tokio::test]
    async fn admin_password_update_hashes_password_revokes_sessions_and_audits() {
        let state = test_state();
        let headers = admin_headers(&state);
        let (target_id, token) = {
            let conn = state.db.get().unwrap();
            let req = RegisterRequest {
                username: "target".to_string(),
                email: None,
                password: "password123".to_string(),
                team_name: None,
                invite_code: None,
            };
            let password_hash = auth::hash_password("password123").unwrap();
            let user = User::create(&conn, &req, &password_hash).unwrap();
            let now = chrono::Utc::now().timestamp() as u64;
            let claims = auth::Claims {
                sub: user.id,
                role: user.role.clone(),
                team_id: user.team_id,
                iat: now,
                exp: now + 3600,
            };
            let token = auth::sign_jwt(&claims, &state.config.auth.jwt_secret).unwrap();
            (user.id, token)
        };
        auth::create_session(&state.db, target_id, &token, 1).unwrap();

        let Json(response) = update_user_password(
            State(state.clone()),
            headers,
            Path(target_id),
            Json(UpdateUserPasswordRequest {
                password: "assigned-password123".to_string(),
                password_confirm: "assigned-password123".to_string(),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response["updated"], true);
        assert!(response.get("password").is_none());
        assert!(response.get("password_hash").is_none());
        assert!(!auth::is_session_valid(&state.db, &auth::hash_token(&token)).unwrap());

        let conn = state.db.get().unwrap();
        let user = User::find_by_id(&conn, target_id).unwrap().unwrap();
        assert!(auth::verify_password("assigned-password123", &user.password_hash).unwrap());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action = 'user.password_update'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn slugify_converts_title_correctly() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("SQL Injection!"), "sql-injection");
        assert_eq!(slugify("XSS <script>"), "xss-script");
    }

    #[tokio::test]
    async fn backup_produces_valid_sqlite_file() {
        let state = test_state();
        let (bytes, filename) = do_backup(&state).unwrap();
        assert!(filename.starts_with("feralctf-backup-"));
        assert!(filename.ends_with(".db"));
        // SQLite files start with the SQLite magic header
        assert_eq!(&bytes[..16], b"SQLite format 3\0");
    }

    fn sample_challenge_req(title: &str) -> CreateChallengeRequest {
        CreateChallengeRequest {
            title: title.to_string(),
            category: "web".to_string(),
            description: "desc".to_string(),
            flag: "flag{x}".to_string(),
            flag_type: "static".to_string(),
            flag_case_sensitive: false,
            points: 100,
            max_points: 100,
            min_points: 20,
            decay_rate: 10,
            author: None,
            tags: Vec::new(),
            unlock_requires: None,
            is_hidden: false,
        }
    }

    fn seed_team_with_solve(state: &AppState, challenge_id: i64) {
        let conn = state.db.get().unwrap();
        conn.execute(
            "INSERT INTO teams (id, name, invite_code, score) VALUES (1, 'T', 'TCODE001', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, team_id, created_at)
             VALUES (900, 'solver', 'h', 'player', 1, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO solves (team_id, user_id, challenge_id, solved_at) VALUES (1, 900, ?1, 5)",
            rusqlite::params![challenge_id],
        )
        .unwrap();
        scoring::recalculate_all_team_scores(&conn).unwrap();
    }

    fn team_score(state: &AppState) -> i64 {
        let conn = state.db.get().unwrap();
        conn.query_row("SELECT score FROM teams WHERE id = 1", [], |r| r.get(0))
            .unwrap()
    }

    #[tokio::test]
    async fn hint_crud_orders_audits_and_refunds_on_delete() {
        let state = test_state();
        let headers = admin_headers(&state);
        let Json(challenge) = create_challenge(
            State(state.clone()),
            headers.clone(),
            Json(sample_challenge_req("Hinted")),
        )
        .await
        .unwrap();
        let Json(other) = create_challenge(
            State(state.clone()),
            headers.clone(),
            Json(sample_challenge_req("Solved One")),
        )
        .await
        .unwrap();
        seed_team_with_solve(&state, other.id);

        let Json(first) = create_hint(
            State(state.clone()),
            headers.clone(),
            Path(challenge.id),
            Json(CreateHintRequest {
                content: "  look closer  ".to_string(),
                cost_points: 30,
                sort_order: None,
            }),
        )
        .await
        .unwrap();
        let Json(second) = create_hint(
            State(state.clone()),
            headers.clone(),
            Path(challenge.id),
            Json(CreateHintRequest {
                content: "second".to_string(),
                cost_points: 0,
                sort_order: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(first.content, "look closer");
        assert_eq!((first.sort_order, second.sort_order), (1, 2));

        for bad in [
            CreateHintRequest {
                content: " ".into(),
                cost_points: 1,
                sort_order: None,
            },
            CreateHintRequest {
                content: "x".into(),
                cost_points: -1,
                sort_order: None,
            },
            CreateHintRequest {
                content: "x".repeat(4001),
                cost_points: 1,
                sort_order: None,
            },
        ] {
            let err = create_hint(
                State(state.clone()),
                headers.clone(),
                Path(challenge.id),
                Json(bad),
            )
            .await
            .unwrap_err();
            assert!(matches!(err, AppError::BadRequest(_)));
        }

        {
            let conn = state.db.get().unwrap();
            Hint::unlock(&conn, 1, first.id, 30, 10).unwrap();
            scoring::recalculate_all_team_scores(&conn).unwrap();
        }
        assert_eq!(team_score(&state), 70);

        let Json(listed) = list_hints(State(state.clone()), Path(challenge.id))
            .await
            .unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].unlock_count, 1);

        let Json(updated) = update_hint(
            State(state.clone()),
            headers.clone(),
            Path(first.id),
            Json(UpdateHintRequest {
                content: None,
                cost_points: Some(99),
                sort_order: Some(3),
            }),
        )
        .await
        .unwrap();
        assert_eq!((updated.cost_points, updated.sort_order), (99, 3));
        assert_eq!(updated.content, "look closer");
        assert_eq!(team_score(&state), 70, "past deductions keep their cost");

        let Json(deleted) = delete_hint(State(state.clone()), headers, Path(first.id))
            .await
            .unwrap();
        assert_eq!(deleted["refunded_unlocks"], 1);
        assert_eq!(team_score(&state), 100);

        let conn = state.db.get().unwrap();
        for action in ["hint.create", "hint.update", "hint.delete"] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM audit_log WHERE action = ?1",
                    rusqlite::params![action],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(count >= 1, "missing audit action {action}");
        }
    }

    #[tokio::test]
    async fn attachments_require_absolute_urls() {
        let state = test_state();
        let headers = admin_headers(&state);
        let Json(challenge) = create_challenge(
            State(state.clone()),
            headers.clone(),
            Json(sample_challenge_req("Files")),
        )
        .await
        .unwrap();

        let err = create_file(
            State(state.clone()),
            headers.clone(),
            Path(challenge.id),
            Json(CreateAttachmentRequest {
                label: "dump".into(),
                url: "/file.zip".into(),
            }),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));

        let Json(file) = create_file(
            State(state.clone()),
            headers.clone(),
            Path(challenge.id),
            Json(CreateAttachmentRequest {
                label: "dump".into(),
                url: "https://example.com/file.zip".into(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(file.storage_path, "https://example.com/file.zip");

        let Json(renamed) = update_file(
            State(state.clone()),
            headers.clone(),
            Path(file.id),
            Json(UpdateAttachmentRequest {
                label: Some("capture".into()),
                url: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(renamed.filename, "capture");

        // A legacy relative row cannot be saved without fixing its URL.
        let legacy_id = {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO files (challenge_id, filename, storage_path, size_bytes, sha256)
                 VALUES (?1, 'old', 'files/old.bin', 0, '')",
                rusqlite::params![challenge.id],
            )
            .unwrap();
            conn.last_insert_rowid()
        };
        let err = update_file(
            State(state.clone()),
            headers.clone(),
            Path(legacy_id),
            Json(UpdateAttachmentRequest {
                label: Some("old file".into()),
                url: None,
            }),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));

        let Json(listed) = list_admin_challenges(State(state.clone())).await.unwrap();
        assert_eq!(listed[0].file_count, 2);

        let _ = delete_file(State(state.clone()), headers, Path(file.id))
            .await
            .unwrap();
        let Json(files) = list_files(State(state), Path(challenge.id)).await.unwrap();
        assert_eq!(files.len(), 1);
    }

    #[tokio::test]
    async fn update_challenge_recalculates_scores_and_guards_flag_form() {
        let state = test_state();
        let headers = admin_headers(&state);
        let Json(challenge) = create_challenge(
            State(state.clone()),
            headers.clone(),
            Json(sample_challenge_req("Scored")),
        )
        .await
        .unwrap();
        seed_team_with_solve(&state, challenge.id);
        assert_eq!(team_score(&state), 100);

        let empty_update = || UpdateChallengeRequest {
            title: None,
            category: None,
            description: None,
            flag: None,
            flag_type: None,
            flag_case_sensitive: None,
            points: None,
            max_points: None,
            min_points: None,
            decay_rate: None,
            author: None,
            tags: None,
            unlock_requires: None,
            is_hidden: None,
        };

        let Json(_) = update_challenge(
            State(state.clone()),
            headers.clone(),
            Path(challenge.id),
            Json(UpdateChallengeRequest {
                points: Some(250),
                ..empty_update()
            }),
        )
        .await
        .unwrap();
        assert_eq!(team_score(&state), 250);

        for req in [
            UpdateChallengeRequest {
                flag_type: Some("regex".into()),
                ..empty_update()
            },
            UpdateChallengeRequest {
                flag_case_sensitive: Some(true),
                ..empty_update()
            },
            UpdateChallengeRequest {
                flag_type: Some("regex".into()),
                flag: Some("flag{(".into()),
                ..empty_update()
            },
            UpdateChallengeRequest {
                unlock_requires: Some(Some(challenge.id)),
                ..empty_update()
            },
        ] {
            let err = update_challenge(
                State(state.clone()),
                headers.clone(),
                Path(challenge.id),
                Json(req),
            )
            .await
            .unwrap_err();
            assert!(matches!(err, AppError::BadRequest(_)));
        }

        let Json(cs) = update_challenge(
            State(state.clone()),
            headers,
            Path(challenge.id),
            Json(UpdateChallengeRequest {
                flag_case_sensitive: Some(true),
                flag: Some("flag{MiXed}".into()),
                ..empty_update()
            }),
        )
        .await
        .unwrap();
        assert!(cs.flag_case_sensitive);
        assert!(auth::verify_flag(
            "flag{MiXed}",
            &cs.flag_hash,
            &cs.flag_salt,
            true
        ));
        assert!(!auth::verify_flag(
            "flag{mixed}",
            &cs.flag_hash,
            &cs.flag_salt,
            true
        ));
    }

    #[tokio::test]
    async fn delete_challenge_cascades_and_recalculates() {
        let state = test_state();
        let headers = admin_headers(&state);
        let Json(doomed) = create_challenge(
            State(state.clone()),
            headers.clone(),
            Json(sample_challenge_req("Doomed")),
        )
        .await
        .unwrap();
        let mut dependent_req = sample_challenge_req("Dependent");
        dependent_req.unlock_requires = Some(doomed.id);
        let Json(dependent) =
            create_challenge(State(state.clone()), headers.clone(), Json(dependent_req))
                .await
                .unwrap();
        seed_team_with_solve(&state, doomed.id);
        {
            let conn = state.db.get().unwrap();
            let hint = Hint::create(&conn, doomed.id, "h", 10, None).unwrap();
            Hint::unlock(&conn, 1, hint.id, 10, 6).unwrap();
            ChallengeFile::create(&conn, doomed.id, "f", "https://example.com/f").unwrap();
            conn.execute(
                "INSERT INTO submissions (team_id, user_id, challenge_id, flag, is_correct, submitted_at)
                 VALUES (1, 900, ?1, 'flag{x}', 1, 5)",
                rusqlite::params![doomed.id],
            )
            .unwrap();
            scoring::recalculate_all_team_scores(&conn).unwrap();
        }
        assert_eq!(team_score(&state), 90);

        let _ = delete_challenge(State(state.clone()), headers, Path(doomed.id))
            .await
            .unwrap();
        assert_eq!(team_score(&state), 0);
        let conn = state.db.get().unwrap();
        for (table, expected) in [
            ("hints", 0),
            ("hint_unlocks", 0),
            ("files", 0),
            ("solves", 0),
            ("submissions", 1),
        ] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, expected, "{table}");
        }
        let prereq: Option<i64> = conn
            .query_row(
                "SELECT unlock_requires FROM challenges WHERE id = ?1",
                rusqlite::params![dependent.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(prereq, None);
    }

    fn create_user_req(username: &str, team: Option<TeamAssignment>) -> CreateUserRequest {
        CreateUserRequest {
            username: username.to_string(),
            password: "password123".to_string(),
            password_confirm: "password123".to_string(),
            role: None,
            team,
        }
    }

    #[tokio::test]
    async fn admin_creates_users_with_no_existing_or_new_team() {
        let state = test_state();
        let headers = admin_headers(&state);

        let Json(solo) = create_user(
            State(state.clone()),
            headers.clone(),
            Json(create_user_req("solo", None)),
        )
        .await
        .unwrap();
        assert_eq!((solo.role.as_str(), solo.team_id), ("player", None));

        let Json(alice) = create_user(
            State(state.clone()),
            headers.clone(),
            Json(create_user_req(
                "alice",
                Some(TeamAssignment::NewName(" Red ".into())),
            )),
        )
        .await
        .unwrap();
        assert_eq!(alice.team_name.as_deref(), Some("Red"));
        let red_id = alice.team_id.unwrap();

        let Json(bob) = create_user(
            State(state.clone()),
            headers.clone(),
            Json(create_user_req(
                "bob",
                Some(TeamAssignment::ExistingId(red_id)),
            )),
        )
        .await
        .unwrap();
        assert_eq!(bob.team_id, Some(red_id));

        let mut admin_req = create_user_req("helper", None);
        admin_req.role = Some("admin".into());
        let Json(helper) = create_user(State(state.clone()), headers.clone(), Json(admin_req))
            .await
            .unwrap();
        assert_eq!(helper.role, "admin");

        for bad in [
            create_user_req("alice", None),
            create_user_req("dupe-team", Some(TeamAssignment::NewName("Red".into()))),
            create_user_req("ghost", Some(TeamAssignment::ExistingId(9999))),
            CreateUserRequest {
                password_confirm: "different123".into(),
                ..create_user_req("mismatch", None)
            },
            CreateUserRequest {
                role: Some("banned".into()),
                ..create_user_req("weird", None)
            },
        ] {
            let err = create_user(State(state.clone()), headers.clone(), Json(bad))
                .await
                .unwrap_err();
            assert!(matches!(err, AppError::BadRequest(_)));
        }

        let conn = state.db.get().unwrap();
        // Failed requests leave nothing behind.
        let users: i64 = conn
            .query_row("SELECT COUNT(*) FROM users WHERE username IN ('dupe-team', 'ghost', 'mismatch', 'weird')", [], |r| r.get(0))
            .unwrap();
        assert_eq!(users, 0);
        // No session is created for the new account.
        let sessions: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE user_id = ?1",
                rusqlite::params![alice.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sessions, 0);
        let stored = crate::models::user::User::find_by_id(&conn, alice.id)
            .unwrap()
            .unwrap();
        assert!(auth::verify_password("password123", &stored.password_hash).unwrap());
    }

    #[tokio::test]
    async fn admin_create_user_respects_team_size_and_closed_registration() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        db::run_migrations(&pool.get().unwrap()).unwrap();
        let mut config = Config::default();
        config.auth.jwt_secret = "test-secret".to_string();
        config.competition.max_team_size = 1;
        config.competition.registration_open = false;
        let state = AppState {
            db: pool,
            config: Arc::new(config),
            cache: Arc::new(AppCache::new()),
            ws_hub: Arc::new(WsHub::new()),
            rate_limiter: Arc::new(crate::anticheat::RateLimiter::new()),
        };
        let headers = admin_headers(&state);
        let Json(first) = create_user(
            State(state.clone()),
            headers.clone(),
            Json(create_user_req(
                "first",
                Some(TeamAssignment::NewName("Tiny".into())),
            )),
        )
        .await
        .unwrap();
        let err = create_user(
            State(state),
            headers,
            Json(create_user_req(
                "second",
                Some(TeamAssignment::ExistingId(first.team_id.unwrap())),
            )),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(ref m) if m.contains("full")));
    }

    #[tokio::test]
    async fn reassigning_team_revokes_sessions_and_audits() {
        let state = test_state();
        let headers = admin_headers(&state);
        let Json(bob) = create_user(
            State(state.clone()),
            headers.clone(),
            Json(create_user_req(
                "bob",
                Some(TeamAssignment::NewName("Red".into())),
            )),
        )
        .await
        .unwrap();
        let token = {
            let now = chrono::Utc::now().timestamp() as u64;
            let claims = auth::Claims {
                sub: bob.id,
                role: bob.role.clone(),
                team_id: bob.team_id,
                iat: now,
                exp: now + 3600,
            };
            auth::sign_jwt(&claims, &state.config.auth.jwt_secret).unwrap()
        };
        auth::create_session(&state.db, bob.id, &token, 1).unwrap();

        let Json(moved) = update_user_team(
            State(state.clone()),
            headers.clone(),
            Path(bob.id),
            Json(UpdateUserTeamRequest {
                team: Some(TeamAssignment::NewName("Blue".into())),
            }),
        )
        .await
        .unwrap();
        assert_eq!(moved.team_name.as_deref(), Some("Blue"));
        assert!(!auth::is_session_valid(&state.db, &auth::hash_token(&token)).unwrap());

        let Json(teamless) = update_user_team(
            State(state.clone()),
            headers,
            Path(bob.id),
            Json(UpdateUserTeamRequest { team: None }),
        )
        .await
        .unwrap();
        assert_eq!(teamless.team_id, None);

        let conn = state.db.get().unwrap();
        let audits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action = 'user.team_update'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(audits, 2);
    }

    #[tokio::test]
    async fn banning_an_admin_requires_demotion_first() {
        let state = test_state();
        let headers = admin_headers(&state);
        let mut req = create_user_req("second-admin", None);
        req.role = Some("admin".into());
        let Json(other) = create_user(State(state.clone()), headers.clone(), Json(req))
            .await
            .unwrap();
        let err = update_user_role(
            State(state),
            headers,
            Path(other.id),
            Json(UpdateUserRoleRequest {
                role: "banned".into(),
            }),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn update_request_distinguishes_null_from_absent() {
        let cleared: UpdateChallengeRequest =
            serde_json::from_str(r#"{"author": null, "unlock_requires": null}"#).unwrap();
        assert_eq!(cleared.author, Some(None));
        assert_eq!(cleared.unlock_requires, Some(None));
        let untouched: UpdateChallengeRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(untouched.author, None);
        assert_eq!(untouched.unlock_requires, None);
        let set: UpdateChallengeRequest =
            serde_json::from_str(r#"{"unlock_requires": 4}"#).unwrap();
        assert_eq!(set.unlock_requires, Some(Some(4)));
    }

    #[tokio::test]
    async fn branding_update_validates_audits_and_reverts() {
        let state = test_state();
        let headers = admin_headers(&state);
        let Json(set) = update_branding(
            State(state.clone()),
            headers.clone(),
            Json(UpdateBrandingRequest {
                name: Some("  Squirrel Games ".into()),
                logo_url: Some("https://cdn.example.org/logo.png".into()),
            }),
        )
        .await
        .unwrap();
        assert_eq!(set.name, "Squirrel Games");
        assert_eq!(
            set.logo_url.as_deref(),
            Some("https://cdn.example.org/logo.png")
        );

        for bad in [
            UpdateBrandingRequest {
                name: None,
                logo_url: Some("/logo.png".into()),
            },
            UpdateBrandingRequest {
                name: None,
                logo_url: Some("javascript:alert(1)".into()),
            },
            UpdateBrandingRequest {
                name: Some("x".repeat(65)),
                logo_url: None,
            },
        ] {
            let err = update_branding(State(state.clone()), headers.clone(), Json(bad))
                .await
                .unwrap_err();
            assert!(matches!(err, AppError::BadRequest(_)));
        }

        let Json(reverted) = update_branding(
            State(state.clone()),
            headers,
            Json(UpdateBrandingRequest {
                name: Some(String::new()),
                logo_url: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(reverted.name, state.config.competition.name);
        assert_eq!(reverted.logo_url, None);

        let conn = state.db.get().unwrap();
        let audits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action = 'settings.branding'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(audits, 2);
    }

    #[tokio::test]
    async fn flags_are_encrypted_revealed_to_admins_and_checked_by_hash() {
        let state = test_state();
        let headers = admin_headers(&state);
        let mut req = sample_challenge_req("Secret");
        req.flag = " flag{Reveal_Me} ".into();
        let Json(created) = create_challenge(State(state.clone()), headers.clone(), Json(req))
            .await
            .unwrap();
        let ciphertext = created.flag_ciphertext.clone().expect("ciphertext stored");
        assert!(!ciphertext.contains("Reveal"));
        let json = serde_json::to_string(&created).unwrap();
        assert!(!json.contains("flag_ciphertext") && !json.contains(&ciphertext));
        let Json(listed) = list_admin_challenges(State(state.clone())).await.unwrap();
        assert!(
            !serde_json::to_string(&listed)
                .unwrap()
                .contains(&ciphertext)
        );

        let Json(revealed) = reveal_flag(State(state.clone()), headers.clone(), Path(created.id))
            .await
            .unwrap();
        assert!(revealed.stored);
        assert_eq!(revealed.flag.as_deref(), Some("flag{Reveal_Me}"));

        // Runtime checks use the hash even if the ciphertext is unreadable.
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "UPDATE challenges SET flag_ciphertext = 'garbage' WHERE id = ?1",
                rusqlite::params![created.id],
            )
            .unwrap();
        }
        let stored = {
            let conn = state.db.get().unwrap();
            Challenge::find_by_id(&conn, created.id).unwrap().unwrap()
        };
        assert!(auth::verify_flag(
            "FLAG{reveal_me}",
            &stored.flag_hash,
            &stored.flag_salt,
            false
        ));

        // A new flag re-encrypts; legacy rows report stored: false.
        let Json(updated) = update_challenge(
            State(state.clone()),
            headers.clone(),
            Path(created.id),
            Json(UpdateChallengeRequest {
                title: None,
                category: None,
                description: None,
                flag: Some("flag{second}".into()),
                flag_type: None,
                flag_case_sensitive: None,
                points: None,
                max_points: None,
                min_points: None,
                decay_rate: None,
                author: None,
                tags: None,
                unlock_requires: None,
                is_hidden: None,
            }),
        )
        .await
        .unwrap();
        assert!(updated.flag_ciphertext.is_some());
        let Json(again) = reveal_flag(State(state.clone()), headers.clone(), Path(created.id))
            .await
            .unwrap();
        assert_eq!(again.flag.as_deref(), Some("flag{second}"));
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "UPDATE challenges SET flag_ciphertext = NULL WHERE id = ?1",
                rusqlite::params![created.id],
            )
            .unwrap();
        }
        let Json(legacy) = reveal_flag(State(state.clone()), headers, Path(created.id))
            .await
            .unwrap();
        assert!(!legacy.stored && legacy.flag.is_none());

        let conn = state.db.get().unwrap();
        let audits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action = 'challenge.flag_reveal'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(audits, 3);
    }

    #[tokio::test]
    async fn regex_reveal_returns_the_pattern_without_ciphertext() {
        let state = test_state();
        let headers = admin_headers(&state);
        let mut req = sample_challenge_req("Pattern");
        req.flag_type = "regex".into();
        req.flag = r"^flag\{[0-9]+\}$".into();
        let Json(created) = create_challenge(State(state.clone()), headers.clone(), Json(req))
            .await
            .unwrap();
        assert_eq!(created.flag_ciphertext, None);
        let Json(revealed) = reveal_flag(State(state), headers, Path(created.id))
            .await
            .unwrap();
        assert!(revealed.stored);
        assert_eq!(revealed.flag_type, "regex");
        assert_eq!(revealed.flag.as_deref(), Some(r"^flag\{[0-9]+\}$"));
    }

    #[tokio::test]
    async fn challenge_writes_and_reveal_survive_a_missing_or_wrong_key() {
        let state = test_state();
        let headers = admin_headers(&state);
        {
            let conn = state.db.get().unwrap();
            conn.execute("DELETE FROM flag_cipher", []).unwrap();
        }
        let Json(created) = create_challenge(
            State(state.clone()),
            headers.clone(),
            Json(sample_challenge_req("No Key")),
        )
        .await
        .unwrap();
        assert_eq!(created.flag_ciphertext, None, "stored hash-only");
        assert!(auth::verify_flag(
            "flag{x}",
            &created.flag_hash,
            &created.flag_salt,
            false
        ));
        let Json(legacy) = reveal_flag(State(state.clone()), headers.clone(), Path(created.id))
            .await
            .unwrap();
        assert!(!legacy.stored && legacy.error.is_none());

        // Key restored but different from the one used to encrypt.
        {
            let conn = state.db.get().unwrap();
            crate::flag_cipher::ensure_key(&conn).unwrap();
            conn.execute(
                "UPDATE challenges SET flag_ciphertext = 'AAAAAAAAAAAAAAAAAAAAAAAA' WHERE id = ?1",
                rusqlite::params![created.id],
            )
            .unwrap();
        }
        let Json(broken) = reveal_flag(State(state), headers, Path(created.id))
            .await
            .unwrap();
        assert!(broken.stored && broken.flag.is_none());
        assert!(broken.error.unwrap().contains("re-enter"));
    }
}
