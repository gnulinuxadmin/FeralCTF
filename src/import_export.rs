use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    auth,
    config::Config,
    db::DbConn,
    errors::AppError,
    models::challenge::{Challenge, ChallengeFile},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompetitionMeta {
    pub name: String,
    pub dynamic_scoring: bool,
    pub score_freeze_minutes_before_end: u32,
    pub max_team_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportBundle {
    pub feralctf_export_version: u32,
    pub exported_at: String,
    pub competition: CompetitionMeta,
    pub categories: Vec<String>,
    pub challenges: Vec<ExportChallenge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportChallenge {
    pub slug: String,
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
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub hints: Vec<ExportHint>,
    #[serde(default)]
    pub files: Vec<ExportFile>,
    pub unlock_requires: Option<String>,
    pub is_hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag_salt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportHint {
    pub order: i64,
    pub cost: i64,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportFile {
    pub filename: String,
    pub sha256: String,
    pub size_bytes: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportOptions {
    pub overwrite: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub valid: bool,
    pub challenges_created: usize,
    pub challenges_skipped: usize,
    pub challenges_overwritten: usize,
    pub attachment_warnings: Vec<String>,
    pub validation_errors: Vec<String>,
    pub preview: Vec<ImportPreviewItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreviewItem {
    pub slug: String,
    pub action: String,
}

pub fn export(
    conn: &DbConn,
    config: &Config,
    inline_attachments: bool,
) -> Result<ExportBundle, AppError> {
    let challenges = Challenge::list_all(conn)?;
    let mut categories = HashSet::new();
    let mut exported = Vec::with_capacity(challenges.len());
    for challenge in challenges {
        categories.insert(challenge.category.clone());
        exported.push(export_challenge(
            conn,
            config,
            &challenge,
            inline_attachments,
        )?);
    }
    let mut categories = categories.into_iter().collect::<Vec<_>>();
    categories.sort();

    Ok(ExportBundle {
        feralctf_export_version: 1,
        exported_at: chrono::Utc::now().to_rfc3339(),
        competition: CompetitionMeta {
            name: config.competition.name.clone(),
            dynamic_scoring: config.competition.dynamic_scoring,
            score_freeze_minutes_before_end: config.competition.score_freeze_minutes_before_end,
            max_team_size: config.competition.max_team_size,
        },
        categories,
        challenges: exported,
    })
}

pub fn import(
    conn: &DbConn,
    bundle: &ExportBundle,
    attachments_dir: Option<&Path>,
    options: &ImportOptions,
) -> Result<ImportResult, AppError> {
    let mut result = validate_bundle(conn, bundle, options)?;
    if !result.valid || options.dry_run {
        return Ok(result);
    }

    conn.execute_batch("BEGIN IMMEDIATE")?;
    let write_result = apply_import(conn, bundle, attachments_dir, options, &mut result);
    match write_result {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(result)
        }
        Err(err) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(err)
        }
    }
}

pub fn detect_and_convert_ctfd(raw: &[u8]) -> Result<ExportBundle, AppError> {
    if let Ok(bundle) = serde_json::from_slice::<ExportBundle>(raw) {
        return Ok(bundle);
    }

    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(raw) {
        return ctfd_value_to_bundle(&value);
    }

    let reader = Cursor::new(raw);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|err| AppError::BadRequest(format!("invalid import format: {err}")))?;
    let mut json_values = Vec::new();
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|err| anyhow::anyhow!("zip read failed: {err}"))?;
        if !file.name().ends_with(".json") {
            continue;
        }
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|err| anyhow::anyhow!("zip json read failed: {err}"))?;
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
            json_values.push(value);
        }
    }

    for value in &json_values {
        if let Ok(bundle) = serde_json::from_value::<ExportBundle>(value.clone()) {
            return Ok(bundle);
        }
        if let Ok(bundle) = ctfd_value_to_bundle(value) {
            return Ok(bundle);
        }
    }

    Err(AppError::BadRequest(
        "no FeralCTF or CTFd export found".to_string(),
    ))
}

fn export_challenge(
    conn: &DbConn,
    config: &Config,
    challenge: &Challenge,
    inline_attachments: bool,
) -> Result<ExportChallenge, AppError> {
    let unlock_requires = match challenge.unlock_requires {
        Some(id) => Challenge::find_by_id(conn, id)?.map(|c| c.slug),
        None => None,
    };

    Ok(ExportChallenge {
        slug: challenge.slug.clone(),
        title: challenge.title.clone(),
        category: challenge.category.clone(),
        description: challenge.description.clone(),
        flag: if challenge.flag_type == "regex" {
            challenge.flag_hash.clone()
        } else {
            String::new()
        },
        flag_type: challenge.flag_type.clone(),
        flag_case_sensitive: challenge.flag_case_sensitive,
        points: challenge.points,
        max_points: challenge.max_points,
        min_points: challenge.min_points,
        decay_rate: challenge.decay_rate,
        author: challenge.author.clone(),
        tags: parse_tags(challenge.tags.as_deref()),
        hints: export_hints(conn, challenge.id)?,
        files: export_files(conn, config, challenge.id, inline_attachments)?,
        unlock_requires,
        is_hidden: challenge.is_hidden,
        flag_hash: Some(challenge.flag_hash.clone()),
        flag_salt: Some(challenge.flag_salt.clone()),
    })
}

fn export_hints(conn: &DbConn, challenge_id: i64) -> Result<Vec<ExportHint>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT sort_order, cost_points, content
         FROM hints WHERE challenge_id = ?1 ORDER BY sort_order, id",
    )?;
    let hints = stmt
        .query_map(params![challenge_id], |row| {
            Ok(ExportHint {
                order: row.get(0)?,
                cost: row.get(1)?,
                content: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(hints)
}

fn export_files(
    conn: &DbConn,
    config: &Config,
    challenge_id: i64,
    inline_attachments: bool,
) -> Result<Vec<ExportFile>, AppError> {
    let files = ChallengeFile::list_by_challenge(conn, challenge_id)?;
    let mut exported = Vec::with_capacity(files.len());
    for file in files {
        let data = if inline_attachments && file.size_bytes <= 5 * 1024 * 1024 {
            let path = stored_file_path(config, &file.storage_path);
            match fs::read(path) {
                Ok(bytes) => Some(BASE64.encode(bytes)),
                Err(_) => None,
            }
        } else {
            None
        };
        exported.push(ExportFile {
            filename: file.filename,
            sha256: file.sha256,
            size_bytes: file.size_bytes,
            data,
        });
    }
    Ok(exported)
}

fn validate_bundle(
    conn: &DbConn,
    bundle: &ExportBundle,
    options: &ImportOptions,
) -> Result<ImportResult, AppError> {
    let mut result = ImportResult {
        valid: true,
        challenges_created: 0,
        challenges_skipped: 0,
        challenges_overwritten: 0,
        attachment_warnings: Vec::new(),
        validation_errors: Vec::new(),
        preview: Vec::new(),
    };

    if bundle.feralctf_export_version != 1 {
        result
            .validation_errors
            .push("unsupported export version".to_string());
    }

    let mut slugs = HashSet::new();
    for challenge in &bundle.challenges {
        if challenge.slug.trim().is_empty() {
            result
                .validation_errors
                .push("challenge slug is required".to_string());
        }
        if !slugs.insert(challenge.slug.clone()) {
            result
                .validation_errors
                .push(format!("duplicate challenge slug: {}", challenge.slug));
        }
        if challenge.title.trim().is_empty() {
            result
                .validation_errors
                .push(format!("challenge {} title is required", challenge.slug));
        }
        if let Some(required) = &challenge.unlock_requires {
            let exists_in_bundle = bundle.challenges.iter().any(|c| &c.slug == required);
            let exists_in_db = Challenge::find_by_slug(conn, required)?.is_some();
            if !exists_in_bundle && !exists_in_db {
                result.validation_errors.push(format!(
                    "challenge {} unlock_requires unknown slug {}",
                    challenge.slug, required
                ));
            }
        }
        if challenge.flag_type == "dynamic" {
            result.attachment_warnings.push(format!(
                "challenge {} is dynamic and should be reviewed after import",
                challenge.slug
            ));
        }
    }

    result.valid = result.validation_errors.is_empty();
    if !result.valid {
        return Ok(result);
    }

    for challenge in &bundle.challenges {
        let existing = Challenge::find_by_slug(conn, &challenge.slug)?;
        let action = match existing {
            None => {
                result.challenges_created += 1;
                "create"
            }
            Some(existing) if challenge_matches_existing(conn, challenge, &existing)? => {
                result.challenges_skipped += 1;
                "skip"
            }
            Some(_) if options.overwrite => {
                result.challenges_overwritten += 1;
                "overwrite"
            }
            Some(_) => {
                result.challenges_skipped += 1;
                result.attachment_warnings.push(format!(
                    "challenge {} exists and differs; skipped",
                    challenge.slug
                ));
                "skip"
            }
        };
        result.preview.push(ImportPreviewItem {
            slug: challenge.slug.clone(),
            action: action.to_string(),
        });
    }

    Ok(result)
}

fn apply_import(
    conn: &DbConn,
    bundle: &ExportBundle,
    attachments_dir: Option<&Path>,
    options: &ImportOptions,
    result: &mut ImportResult,
) -> Result<(), AppError> {
    let mut slug_to_id = existing_and_imported_ids(conn, bundle)?;
    let mut touched_slugs = HashSet::new();
    for challenge in &bundle.challenges {
        let existing = Challenge::find_by_slug(conn, &challenge.slug)?;
        if let Some(existing) = existing {
            if challenge_matches_existing(conn, challenge, &existing)? || !options.overwrite {
                continue;
            }
            conn.execute(
                "DELETE FROM hints WHERE challenge_id = ?1",
                params![existing.id],
            )?;
            conn.execute(
                "DELETE FROM files WHERE challenge_id = ?1",
                params![existing.id],
            )?;
            update_challenge(conn, challenge, existing.id, &slug_to_id)?;
            import_hints(conn, challenge, existing.id)?;
            import_files(conn, challenge, existing.id, attachments_dir, result)?;
            touched_slugs.insert(challenge.slug.clone());
        } else {
            let id = insert_challenge(conn, challenge, &slug_to_id)?;
            slug_to_id.insert(challenge.slug.clone(), id);
            import_hints(conn, challenge, id)?;
            import_files(conn, challenge, id, attachments_dir, result)?;
            touched_slugs.insert(challenge.slug.clone());
        }
    }
    update_unlock_requirements(conn, bundle, &slug_to_id, &touched_slugs)?;
    Ok(())
}

fn existing_and_imported_ids(
    conn: &DbConn,
    bundle: &ExportBundle,
) -> Result<HashMap<String, i64>, AppError> {
    let mut map = HashMap::new();
    for challenge in &bundle.challenges {
        if let Some(existing) = Challenge::find_by_slug(conn, &challenge.slug)? {
            map.insert(challenge.slug.clone(), existing.id);
        }
    }
    Ok(map)
}

fn insert_challenge(
    conn: &DbConn,
    challenge: &ExportChallenge,
    slug_to_id: &HashMap<String, i64>,
) -> Result<i64, AppError> {
    let now = chrono::Utc::now().timestamp();
    let (flag_hash, flag_salt) = stored_flag(challenge);
    conn.execute(
        "INSERT INTO challenges (
            slug, title, description, category, flag_hash, flag_salt, flag_type,
            flag_case_sensitive, points, max_points, min_points, decay_rate,
            author, tags, unlock_requires, is_hidden, created_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        params![
            challenge.slug,
            challenge.title,
            challenge.description,
            challenge.category,
            flag_hash,
            flag_salt,
            challenge.flag_type,
            challenge.flag_case_sensitive as i64,
            challenge.points,
            challenge.max_points,
            challenge.min_points,
            challenge.decay_rate,
            challenge.author,
            tags_json(&challenge.tags)?,
            challenge
                .unlock_requires
                .as_ref()
                .and_then(|slug| slug_to_id.get(slug).copied()),
            challenge.is_hidden as i64,
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn update_challenge(
    conn: &DbConn,
    challenge: &ExportChallenge,
    id: i64,
    slug_to_id: &HashMap<String, i64>,
) -> Result<(), AppError> {
    let (flag_hash, flag_salt) = stored_flag(challenge);
    conn.execute(
        "UPDATE challenges SET
            title = ?1, description = ?2, category = ?3, flag_hash = ?4, flag_salt = ?5,
            flag_type = ?6, flag_case_sensitive = ?7, points = ?8, max_points = ?9,
            min_points = ?10, decay_rate = ?11, author = ?12, tags = ?13,
            unlock_requires = ?14, is_hidden = ?15
         WHERE id = ?16",
        params![
            challenge.title,
            challenge.description,
            challenge.category,
            flag_hash,
            flag_salt,
            challenge.flag_type,
            challenge.flag_case_sensitive as i64,
            challenge.points,
            challenge.max_points,
            challenge.min_points,
            challenge.decay_rate,
            challenge.author,
            tags_json(&challenge.tags)?,
            challenge
                .unlock_requires
                .as_ref()
                .and_then(|slug| slug_to_id.get(slug).copied()),
            challenge.is_hidden as i64,
            id,
        ],
    )?;
    Ok(())
}

fn update_unlock_requirements(
    conn: &DbConn,
    bundle: &ExportBundle,
    slug_to_id: &HashMap<String, i64>,
    touched_slugs: &HashSet<String>,
) -> Result<(), AppError> {
    for challenge in &bundle.challenges {
        if !touched_slugs.contains(&challenge.slug) {
            continue;
        }
        let id = slug_to_id
            .get(&challenge.slug)
            .ok_or_else(|| anyhow::anyhow!("missing imported challenge id"))?;
        let unlock_id = challenge
            .unlock_requires
            .as_ref()
            .and_then(|slug| slug_to_id.get(slug).copied());
        conn.execute(
            "UPDATE challenges SET unlock_requires = ?1 WHERE id = ?2",
            params![unlock_id, id],
        )?;
    }
    Ok(())
}

fn import_hints(conn: &DbConn, challenge: &ExportChallenge, id: i64) -> Result<(), AppError> {
    for hint in &challenge.hints {
        conn.execute(
            "INSERT INTO hints (challenge_id, content, cost_points, sort_order)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, hint.content, hint.cost, hint.order],
        )?;
    }
    Ok(())
}

fn import_files(
    conn: &DbConn,
    challenge: &ExportChallenge,
    id: i64,
    attachments_dir: Option<&Path>,
    result: &mut ImportResult,
) -> Result<(), AppError> {
    for file in &challenge.files {
        let storage_path = format!("{}/{}", challenge.slug, file.filename);
        if let Some(data) = &file.data {
            if let Some(dir) = attachments_dir {
                let bytes = BASE64
                    .decode(data)
                    .map_err(|err| AppError::BadRequest(format!("invalid base64 file: {err}")))?;
                let target = dir.join(&storage_path);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|err| anyhow::anyhow!("attachment dir create failed: {err}"))?;
                }
                fs::write(&target, bytes)
                    .map_err(|err| anyhow::anyhow!("attachment write failed: {err}"))?;
            } else {
                result.attachment_warnings.push(format!(
                    "inline attachment {} for {} not written: no attachments dir",
                    file.filename, challenge.slug
                ));
            }
        } else if let Some(dir) = attachments_dir {
            let candidate = dir.join(&storage_path);
            if !candidate.exists() && !dir.join(&file.filename).exists() {
                result.attachment_warnings.push(format!(
                    "attachment {} for {} not found",
                    file.filename, challenge.slug
                ));
            }
        } else {
            result.attachment_warnings.push(format!(
                "attachment {} for {} has no inline data",
                file.filename, challenge.slug
            ));
        }

        conn.execute(
            "INSERT INTO files (challenge_id, filename, storage_path, size_bytes, sha256)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                file.filename,
                storage_path,
                file.size_bytes,
                file.sha256
            ],
        )?;
    }
    Ok(())
}

fn challenge_matches_existing(
    conn: &DbConn,
    challenge: &ExportChallenge,
    existing: &Challenge,
) -> Result<bool, AppError> {
    let unlock_slug = match existing.unlock_requires {
        Some(id) => Challenge::find_by_id(conn, id)?.map(|c| c.slug),
        None => None,
    };
    let hints = export_hints(conn, existing.id)?;
    let files = ChallengeFile::list_by_challenge(conn, existing.id)?
        .into_iter()
        .map(|file| ExportFile {
            filename: file.filename,
            sha256: file.sha256,
            size_bytes: file.size_bytes,
            data: None,
        })
        .collect::<Vec<_>>();

    Ok(existing.title == challenge.title
        && existing.description == challenge.description
        && existing.category == challenge.category
        && flag_matches(challenge, existing)
        && existing.flag_type == challenge.flag_type
        && existing.flag_case_sensitive == challenge.flag_case_sensitive
        && existing.points == challenge.points
        && existing.max_points == challenge.max_points
        && existing.min_points == challenge.min_points
        && existing.decay_rate == challenge.decay_rate
        && existing.author == challenge.author
        && parse_tags(existing.tags.as_deref()) == challenge.tags
        && unlock_slug == challenge.unlock_requires
        && existing.is_hidden == challenge.is_hidden
        && hints == challenge.hints
        && files_match_ignoring_data(&files, &challenge.files))
}

fn flag_matches(challenge: &ExportChallenge, existing: &Challenge) -> bool {
    if challenge.flag_hash.as_deref() == Some(existing.flag_hash.as_str())
        && challenge.flag_salt.as_deref() == Some(existing.flag_salt.as_str())
    {
        return true;
    }
    if challenge.flag_type == "regex" {
        existing.flag_hash == challenge.flag
    } else {
        auth::verify_flag(&challenge.flag, &existing.flag_hash, &existing.flag_salt)
    }
}

fn files_match_ignoring_data(a: &[ExportFile], b: &[ExportFile]) -> bool {
    let mut a = a
        .iter()
        .map(|f| (&f.filename, &f.sha256, f.size_bytes))
        .collect::<Vec<_>>();
    let mut b = b
        .iter()
        .map(|f| (&f.filename, &f.sha256, f.size_bytes))
        .collect::<Vec<_>>();
    a.sort();
    b.sort();
    a == b
}

fn stored_flag(challenge: &ExportChallenge) -> (String, String) {
    if let (Some(hash), Some(salt)) = (&challenge.flag_hash, &challenge.flag_salt) {
        return (hash.clone(), salt.clone());
    }
    if challenge.flag_type == "regex" {
        return (challenge.flag.clone(), String::new());
    }
    let salt = generate_salt();
    (auth::hash_flag(&challenge.flag, &salt), salt)
}

fn ctfd_value_to_bundle(value: &serde_json::Value) -> Result<ExportBundle, AppError> {
    let challenges_value = value
        .get("challenges")
        .or_else(|| value.pointer("/db/challenges"))
        .or_else(|| value.pointer("/data/challenges"))
        .ok_or_else(|| AppError::BadRequest("not a CTFd export".to_string()))?;
    let challenges = challenges_value
        .as_array()
        .ok_or_else(|| AppError::BadRequest("CTFd challenges must be an array".to_string()))?;

    let mut exported = Vec::with_capacity(challenges.len());
    let mut categories = HashSet::new();
    for item in challenges {
        let title = string_field(item, &["name", "title"]).unwrap_or_else(|| "challenge".into());
        let category = string_field(item, &["category"]).unwrap_or_else(|| "misc".into());
        categories.insert(category.clone());
        let challenge_type = string_field(item, &["type"]).unwrap_or_else(|| "standard".into());
        let flag_type = match challenge_type.as_str() {
            "regex" => "regex",
            "dynamic" => "dynamic",
            _ => "static",
        }
        .to_string();
        let points = int_field(item, &["value", "points"]).unwrap_or(0);
        let flag = first_ctfd_flag(item).unwrap_or_default();
        exported.push(ExportChallenge {
            slug: string_field(item, &["slug"]).unwrap_or_else(|| slugify(&title)),
            title,
            category,
            description: strip_html(&string_field(item, &["description"]).unwrap_or_default()),
            flag,
            flag_type,
            flag_case_sensitive: false,
            points,
            max_points: int_field(item, &["initial", "max_points"]).unwrap_or(points),
            min_points: int_field(item, &["minimum", "min_points"]).unwrap_or(points),
            decay_rate: int_field(item, &["decay", "decay_rate"]).unwrap_or(12),
            author: string_field(item, &["author"]),
            tags: ctfd_strings(item.get("tags")),
            hints: ctfd_hints(item.get("hints")),
            files: ctfd_files(item.get("files")),
            unlock_requires: None,
            is_hidden: challenge_type == "dynamic",
            flag_hash: None,
            flag_salt: None,
        });
    }
    let mut categories = categories.into_iter().collect::<Vec<_>>();
    categories.sort();

    Ok(ExportBundle {
        feralctf_export_version: 1,
        exported_at: chrono::Utc::now().to_rfc3339(),
        competition: CompetitionMeta {
            name: "CTFd Import".to_string(),
            dynamic_scoring: true,
            score_freeze_minutes_before_end: 0,
            max_team_size: 4,
        },
        categories,
        challenges: exported,
    })
}

fn first_ctfd_flag(item: &serde_json::Value) -> Option<String> {
    item.get("flags")
        .and_then(|flags| flags.as_array())
        .and_then(|flags| flags.first())
        .and_then(|flag| {
            flag.get("content")
                .or_else(|| flag.get("flag"))
                .and_then(|v| v.as_str())
        })
        .map(ToString::to_string)
}

fn ctfd_hints(value: Option<&serde_json::Value>) -> Vec<ExportHint> {
    value
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .enumerate()
                .map(|(index, item)| ExportHint {
                    order: int_field(item, &["order"]).unwrap_or(index as i64 + 1),
                    cost: int_field(item, &["cost"]).unwrap_or(0),
                    content: string_field(item, &["content", "hint"]).unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn ctfd_files(value: Option<&serde_json::Value>) -> Vec<ExportFile> {
    ctfd_strings(value)
        .into_iter()
        .map(|filename| ExportFile {
            filename: PathBuf::from(&filename)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&filename)
                .to_string(),
            sha256: String::new(),
            size_bytes: 0,
            data: None,
        })
        .collect()
}

fn ctfd_strings(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.as_str()
                        .map(ToString::to_string)
                        .or_else(|| string_field(item, &["value", "name", "path", "location"]))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn string_field(value: &serde_json::Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(|v| v.as_str()))
        .map(ToString::to_string)
}

fn int_field(value: &serde_json::Value, names: &[&str]) -> Option<i64> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(|v| v.as_i64()))
}

fn strip_html(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
}

fn parse_tags(raw: Option<&str>) -> Vec<String> {
    raw.and_then(|tags| serde_json::from_str::<Vec<String>>(tags).ok())
        .unwrap_or_default()
}

fn tags_json(tags: &[String]) -> Result<String, AppError> {
    serde_json::to_string(tags).map_err(|err| anyhow::anyhow!("tags json failed: {err}").into())
}

fn stored_file_path(config: &Config, storage_path: &str) -> PathBuf {
    let path = Path::new(storage_path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(&config.storage.attachments_path).join(path)
    }
}

fn generate_salt() -> String {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex(&hasher.finalize()[..16])
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn slugify(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_whitespace() { '-' } else { c })
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect()
}

pub fn export_zip(conn: &DbConn, config: &Config) -> Result<Vec<u8>, AppError> {
    use std::io::Write as _;

    let bundle = export(conn, config, false)?;
    let json = serde_json::to_vec_pretty(&bundle)
        .map_err(|e| anyhow::anyhow!("json serialize: {e}"))?;

    let buf = Cursor::new(Vec::<u8>::new());
    let mut zip = zip::ZipWriter::new(buf);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("challenges.json", opts)
        .map_err(|e| anyhow::anyhow!("zip entry: {e}"))?;
    zip.write_all(&json)
        .map_err(|e| anyhow::anyhow!("zip write: {e}"))?;

    for challenge in &Challenge::list_all(conn)? {
        for file in ChallengeFile::list_by_challenge(conn, challenge.id)? {
            let disk_path = stored_file_path(config, &file.storage_path);
            if let Ok(bytes) = fs::read(&disk_path) {
                let entry = format!("{}/{}", challenge.slug, file.filename);
                zip.start_file(&entry, opts)
                    .map_err(|e| anyhow::anyhow!("zip entry: {e}"))?;
                zip.write_all(&bytes)
                    .map_err(|e| anyhow::anyhow!("zip write: {e}"))?;
            }
        }
    }

    let buf = zip.finish().map_err(|e| anyhow::anyhow!("zip finish: {e}"))?;
    Ok(buf.into_inner())
}

pub fn extract_attachments_zip(zip_bytes: &[u8], dest: &Path) -> Result<(), AppError> {
    use std::io;

    let cursor = Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| AppError::BadRequest(format!("invalid attachments zip: {e}")))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| anyhow::anyhow!("zip read: {e}"))?;
        if file.is_dir() {
            continue;
        }
        // Strip any path components that could escape dest (e.g. ".." or absolute roots)
        let safe_path: PathBuf = file
            .name()
            .split('/')
            .filter(|part| !part.is_empty() && *part != "..")
            .map(std::path::Path::new)
            .collect();
        if safe_path.as_os_str().is_empty() {
            continue;
        }
        let outpath = dest.join(safe_path);
        if let Some(parent) = outpath.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("create dir: {e}"))?;
        }
        let mut out = fs::File::create(&outpath)
            .map_err(|e| anyhow::anyhow!("create attachment: {e}"))?;
        io::copy(&mut file, &mut out)
            .map_err(|e| anyhow::anyhow!("write attachment: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;

    fn test_conn() -> DbConn {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .expect("pool");
        {
            let conn = pool.get().expect("conn");
            db::run_migrations(&conn).expect("migrations");
        }
        pool.get().expect("conn")
    }

    fn bundle() -> ExportBundle {
        ExportBundle {
            feralctf_export_version: 1,
            exported_at: "2026-05-12T00:00:00Z".to_string(),
            competition: CompetitionMeta {
                name: "Test".to_string(),
                dynamic_scoring: true,
                score_freeze_minutes_before_end: 0,
                max_team_size: 4,
            },
            categories: vec!["web".to_string()],
            challenges: vec![ExportChallenge {
                slug: "jwt-jockey".to_string(),
                title: "JWT Jockey".to_string(),
                category: "web".to_string(),
                description: "desc".to_string(),
                flag: "flag{ok}".to_string(),
                flag_type: "static".to_string(),
                flag_case_sensitive: false,
                points: 100,
                max_points: 500,
                min_points: 50,
                decay_rate: 12,
                author: Some("n0tf0und".to_string()),
                tags: vec!["jwt".to_string()],
                hints: vec![ExportHint {
                    order: 1,
                    cost: 25,
                    content: "hint".to_string(),
                }],
                files: Vec::new(),
                unlock_requires: None,
                is_hidden: false,
                flag_hash: None,
                flag_salt: None,
            }],
        }
    }

    #[test]
    fn import_dry_run_writes_nothing() {
        let conn = test_conn();
        let result = import(
            &conn,
            &bundle(),
            None,
            &ImportOptions {
                overwrite: false,
                dry_run: true,
            },
        )
        .expect("dry run");
        assert!(result.valid);
        assert_eq!(result.challenges_created, 1);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM challenges", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 0);
    }

    #[test]
    fn import_is_idempotent() {
        let conn = test_conn();
        let options = ImportOptions {
            overwrite: false,
            dry_run: false,
        };
        import(&conn, &bundle(), None, &options).expect("first import");
        let second = import(&conn, &bundle(), None, &options).expect("second import");
        assert_eq!(second.challenges_created, 0);
        assert_eq!(second.challenges_skipped, 1);
    }
}
