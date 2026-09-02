use std::path::PathBuf;

use axum::body::Bytes;
use axum::extract::multipart::Multipart;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::{Json, response::IntoResponse};
use common::{AppError, AppResult, require_auth};
use serde::Serialize;
use sqlx::FromRow;

use crate::state::AppState;

const MAX_FILE_SIZE: usize = 5 * 1024 * 1024;

#[derive(Serialize, FromRow)]
pub struct PhotoDto {
    pub id: i64,
    pub master_id: Option<i64>,
    pub owner_user_id: i64,
    pub url: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Загружает фото главного из multipart-запроса (поле "file").
pub async fn upload_photo(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let claims = require_auth(&headers, &state.jwt_secret)?;

    let mut field_data: Option<(String, Bytes)> = None;
    while let Some(field) = multipart.next_field().await.map_err(|e| {
        AppError::bad_request(format!("не удалось прочитать multipart: {e}"))
    })? {
        if field.name() == Some("file") {
            let filename = field.file_name().map(String::from).unwrap_or_else(|| "photo".to_string());
            let data = field.bytes().await.map_err(|e| {
                AppError::bad_request(format!("не удалось прочитать файл: {e}"))
            })?;
            field_data = Some((filename, data));
            break;
        }
    }

    let (filename, data) = field_data.ok_or_else(|| AppError::bad_request("файл не передан (поле file)"))?;

    if data.is_empty() {
        return Err(AppError::bad_request("файл пуст"));
    }
    if data.len() > MAX_FILE_SIZE {
        return Err(AppError::bad_request("файл больше 5 МБ"));
    }

    let ext = filename
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .filter(|e| matches!(e.as_str(), "jpg" | "jpeg" | "png" | "gif" | "webp"))
        .unwrap_or_else(|| "jpg".to_string());

    let file_name = format!("{}.{}", uuid::Uuid::new_v4(), ext);
    let dest = state.upload_dir.join(&file_name);

    tokio::fs::create_dir_all(state.upload_dir.as_path())
        .await
        .map_err(|e| AppError::internal(format!("не создать каталог загрузок: {e}")))?;
    tokio::fs::write(&dest, &data)
        .await
        .map_err(|e| AppError::internal(format!("не сохранить файл: {e}")))?;

    let url = format!("/uploads/{file_name}");
    let user_id = claims.sub;

    let owner: Option<i64> = if claims.role == "master" {
        sqlx::query_scalar("SELECT id FROM masters WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(&state.pool)
            .await?
    } else {
        None
    };

    let dto = sqlx::query_as::<_, PhotoDto>(
        "INSERT INTO photos (master_id, owner_user_id, url) VALUES ($1, $2, $3) \
         RETURNING id, master_id, owner_user_id, url, created_at",
    )
    .bind(owner)
    .bind(user_id)
    .bind(&url)
    .fetch_one(&state.pool)
    .await?;

    tracing::info!(user_id, url, "загружено фото");
    Ok(Json(dto))
}

/// Возвращает список фото пользователя (для панели мастера).
pub async fn my_photos(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<PhotoDto>>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;
    let rows = sqlx::query_as::<_, PhotoDto>(
        "SELECT id, master_id, owner_user_id, url, created_at FROM photos \
         WHERE owner_user_id = $1 ORDER BY id DESC",
    )
    .bind(claims.sub)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

/// Удаляет фото по id (только владелец).
pub async fn delete_photo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;

    let (url, owner): (String, i64) = sqlx::query_as(
        "SELECT url, owner_user_id FROM photos WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("фото не найдено"))?;

    if owner != claims.sub && claims.role != "admin" {
        return Err(AppError::forbidden("нельзя удалить чужое фото"));
    }

    sqlx::query("DELETE FROM photos WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;

    if let Some(name) = url.rsplit_once('/').map(|(_, n)| n) {
        let _ = tokio::fs::remove_file(PathBuf::from(&state.upload_dir).join(name)).await;
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}
