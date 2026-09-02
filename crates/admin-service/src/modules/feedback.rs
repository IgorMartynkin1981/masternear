use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Utc};
use common::{AppError, AppResult, require_auth};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::modules::require_admin;
use crate::state::AppState;

#[derive(Serialize, FromRow)]
pub struct FeedbackDto {
    pub id: i64,
    pub user_id: Option<i64>,
    pub email: Option<String>,
    pub message: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateFeedbackReq {
    pub message: String,
    pub email: Option<String>,
}

pub async fn create_feedback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateFeedbackReq>,
) -> AppResult<Json<FeedbackDto>> {
    let message = req.message.trim();
    if message.is_empty() {
        return Err(AppError::bad_request("сообщение не может быть пустым"));
    }

    let auth = require_auth(&headers, &state.jwt_secret).ok();

    let row = sqlx::query_as::<_, FeedbackDto>(
        "INSERT INTO feedback (user_id, email, message) VALUES ($1, $2, $3) \
         RETURNING id, user_id, email, message, status, created_at",
    )
    .bind(auth.as_ref().map(|c| c.sub))
    .bind(req.email.as_deref())
    .bind(message)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(row))
}

pub async fn list_feedback(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<FeedbackDto>>> {
    let _admin = require_admin(&headers, &state.jwt_secret)?;
    let rows = sqlx::query_as::<_, FeedbackDto>(
        "SELECT id, user_id, email, message, status, created_at FROM feedback ORDER BY id DESC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn resolve_feedback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> AppResult<Json<FeedbackDto>> {
    let _admin = require_admin(&headers, &state.jwt_secret)?;
    let row = sqlx::query_as::<_, FeedbackDto>(
        "UPDATE feedback SET status = 'done' \
         WHERE id = $1 RETURNING id, user_id, email, message, status, created_at",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("обращение не найдено"))?;
    Ok(Json(row))
}