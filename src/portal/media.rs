//! ROM upload and paginated on-device screenshot endpoints.

use super::{PortalStateResponse, ScreenshotEntry, ScreenshotsPageResponse, SharedCtx};
use crate::state::GameRule;
use axum::{
    body::Bytes,
    extract::{Path as AxumPath, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
pub(super) struct UploadRomQuery {
    filename: String,
    system: Option<String>,
}

fn infer_system_from_extension(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "gba" => Some("GBA"),
        "gbc" => Some("GBC"),
        "gb" => Some("GB"),
        "sfc" | "smc" => Some("SNES"),
        "nes" => Some("NES"),
        "md" | "gen" | "smd" => Some("MD"),
        "sms" => Some("SMS"),
        "gg" => Some("GG"),
        "pce" => Some("PCE"),
        "ngp" | "ngc" => Some("NGP"),
        "ws" | "wsc" => Some("WS"),
        "nds" => Some("NDS"),
        "n64" | "z64" | "v64" => Some("N64"),
        "chd" | "pbp" | "cue" | "iso" => Some("PS1"),
        _ => None,
    }
}

fn sanitize_filename(raw: &str) -> Option<String> {
    let base = raw
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or("")
        .trim();
    if base.is_empty() || base == "." || base == ".." || base.contains('\0') {
        return None;
    }
    Some(base.to_string())
}

pub(super) async fn handle_upload_rom(
    State(ctx): State<SharedCtx>,
    Query(q): Query<UploadRomQuery>,
    body: Bytes,
) -> Result<Json<PortalStateResponse>, (StatusCode, String)> {
    if body.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Empty file".to_string()));
    }
    let filename = sanitize_filename(&q.filename)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Invalid filename".to_string()))?;
    let ext = std::path::Path::new(&filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let requested_sys = q
        .system
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("AUTO"));
    let system = match requested_sys {
        Some(s) => {
            let clean: String = s
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if clean.is_empty() {
                return Err((StatusCode::BAD_REQUEST, "Invalid system folder".to_string()));
            }
            clean.to_ascii_uppercase()
        }
        None => infer_system_from_extension(ext)
            .ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "Select a system folder for archive files (.zip/.7z)".to_string(),
                )
            })?
            .to_string(),
    };

    let rom_root = {
        let guard = ctx.lock().unwrap();
        guard
            .rom_roots
            .iter()
            .find(|p| p.is_dir())
            .or_else(|| guard.rom_roots.first())
            .cloned()
            .ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "No ROM directory configured".to_string(),
                )
            })?
    };

    let target_dir = rom_root.join(&system);
    std::fs::create_dir_all(&target_dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let target_path = target_dir.join(&filename);
    std::fs::write(&target_path, &body)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut guard = ctx.lock().unwrap();
    guard.touch_activity();
    let game_id = format!("{}/{}", system, filename);
    guard
        .store
        .config
        .games
        .entry(game_id)
        .or_insert(GameRule {
            allowed: true,
            custom_title: None,
            core_override: None,
        })
        .allowed = true;
    guard
        .store
        .save_config()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(guard.build_portal_state()))
}

fn screenshot_dir(data_dir: &std::path::Path, dev_mode: bool) -> PathBuf {
    if !dev_mode {
        for candidate in [
            PathBuf::from("/run/muos/storage/screenshot"),
            PathBuf::from("/mnt/mmc/MUOS/screenshot"),
        ] {
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    data_dir.join("screenshots")
}

fn is_screenshot_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".bmp")
}

pub(super) fn count_screenshots(data_dir: &std::path::Path, dev_mode: bool) -> usize {
    let dir = screenshot_dir(data_dir, dev_mode);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return 0;
    };
    rd.flatten()
        .filter(|e| {
            e.metadata().map(|m| m.is_file()).unwrap_or(false)
                && is_screenshot_file(&e.file_name().to_string_lossy())
        })
        .count()
}

fn list_screenshots_paginated(
    data_dir: &std::path::Path,
    dev_mode: bool,
    offset: usize,
    limit: usize,
) -> ScreenshotsPageResponse {
    let dir = screenshot_dir(data_dir, dev_mode);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return ScreenshotsPageResponse {
            total: 0,
            offset,
            limit,
            items: Vec::new(),
        };
    };
    let mut items: Vec<(std::time::SystemTime, ScreenshotEntry)> = rd
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !is_screenshot_file(&name) {
                return None;
            }
            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            Some((
                mtime,
                ScreenshotEntry {
                    name,
                    size: meta.len(),
                },
            ))
        })
        .collect();
    items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.name.cmp(&a.1.name)));
    let total = items.len();
    let page = items
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|(_, e)| e)
        .collect();
    ScreenshotsPageResponse {
        total,
        offset,
        limit,
        items: page,
    }
}

#[derive(Deserialize)]
pub(super) struct ListScreenshotsQuery {
    offset: Option<usize>,
    limit: Option<usize>,
}

pub(super) async fn handle_list_screenshots(
    State(ctx): State<SharedCtx>,
    Query(q): Query<ListScreenshotsQuery>,
) -> Json<ScreenshotsPageResponse> {
    let offset = q.offset.unwrap_or(0);
    let limit = q.limit.unwrap_or(12).clamp(1, 50);
    let (data_dir, dev_mode) = {
        let mut guard = ctx.lock().unwrap();
        guard.touch_activity();
        (guard.store.data_dir.clone(), guard.dev_mode)
    };
    Json(list_screenshots_paginated(&data_dir, dev_mode, offset, limit))
}

pub(super) async fn handle_delete_screenshot(
    State(ctx): State<SharedCtx>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let Some(clean_name) = sanitize_filename(&name) else {
        return Err((StatusCode::BAD_REQUEST, "Invalid screenshot name".to_string()));
    };
    if clean_name != name || !is_screenshot_file(&clean_name) {
        return Err((StatusCode::BAD_REQUEST, "Invalid screenshot name".to_string()));
    }
    let (data_dir, dev_mode) = {
        let mut guard = ctx.lock().unwrap();
        guard.touch_activity();
        (guard.store.data_dir.clone(), guard.dev_mode)
    };
    let path = screenshot_dir(&data_dir, dev_mode).join(&clean_name);
    if !path.is_file() {
        return Err((StatusCode::NOT_FOUND, "Screenshot not found".to_string()));
    }
    std::fs::remove_file(&path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let remaining = count_screenshots(&data_dir, dev_mode);
    Ok(Json(serde_json::json!({
        "ok": true,
        "screenshot_count": remaining
    })))
}

#[derive(Deserialize)]
pub(super) struct ScreenshotQuery {
    inline: Option<u8>,
}

pub(super) async fn handle_get_screenshot(
    State(ctx): State<SharedCtx>,
    AxumPath(name): AxumPath<String>,
    Query(q): Query<ScreenshotQuery>,
) -> Response {
    let Some(clean_name) = sanitize_filename(&name) else {
        return (StatusCode::BAD_REQUEST, "Invalid screenshot name").into_response();
    };
    if clean_name != name {
        return (StatusCode::BAD_REQUEST, "Invalid screenshot name").into_response();
    }
    let dir = {
        let mut guard = ctx.lock().unwrap();
        guard.touch_activity();
        screenshot_dir(&guard.store.data_dir, guard.dev_mode)
    };
    let path = dir.join(&clean_name);
    let Ok(bytes) = std::fs::read(&path) else {
        return (StatusCode::NOT_FOUND, "Screenshot not found").into_response();
    };
    let lower = clean_name.to_ascii_lowercase();
    let content_type = if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") {
        "image/jpeg"
    } else {
        "image/bmp"
    };
    let disp = if q.inline == Some(1) {
        format!("inline; filename=\"{}\"", clean_name.replace('"', ""))
    } else {
        format!("attachment; filename=\"{}\"", clean_name.replace('"', ""))
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::CONTENT_DISPOSITION, disp),
        ],
        bytes,
    )
        .into_response()
}

