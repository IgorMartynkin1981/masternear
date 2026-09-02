use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Utc};
use common::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::modules::require_admin;
use crate::state::AppState;

const BCRYPT_COST: u32 = 10;

#[derive(Serialize, FromRow)]
pub struct UserDto {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

pub async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<UserDto>>> {
    let _admin = require_admin(&headers, &state.jwt_secret)?;
    let rows = sqlx::query_as::<_, UserDto>(
        "SELECT id, name, email, role, created_at FROM users ORDER BY id",
    )
    .fetch_all(&state.auth_pool)
    .await?;
    Ok(Json(rows))
}

pub async fn get_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> AppResult<Json<UserDto>> {
    let _admin = require_admin(&headers, &state.jwt_secret)?;
    let user = sqlx::query_as::<_, UserDto>(
        "SELECT id, name, email, role, created_at FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.auth_pool)
    .await?
    .ok_or_else(|| AppError::not_found("пользователь не найден"))?;
    Ok(Json(user))
}

#[derive(Deserialize)]
pub struct CreateUserReq {
    pub name: String,
    pub email: String,
    pub password: String,
    pub role: String,
}

pub async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateUserReq>,
) -> AppResult<Json<UserDto>> {
    let _admin = require_admin(&headers, &state.jwt_secret)?;

    let name = req.name.trim();
    let email = req.email.trim().to_lowercase();
    if name.is_empty() || email.is_empty() || req.password.is_empty() {
        return Err(AppError::bad_request("имя, email и пароль обязательны"));
    }
    if !matches!(req.role.as_str(), "customer" | "master" | "admin") {
        return Err(AppError::bad_request("недопустимая роль"));
    }

    let password_hash = bcrypt::hash(&req.password, BCRYPT_COST)
        .map_err(|e| AppError::internal(format!("ошибка хеширования: {e}")))?;

    let result = sqlx::query_as::<_, UserDto>(
        "INSERT INTO users (name, email, password_hash, role) VALUES ($1, $2, $3, $4) \
         RETURNING id, name, email, role, created_at",
    )
    .bind(name)
    .bind(&email)
    .bind(&password_hash)
    .bind(&req.role)
    .fetch_one(&state.auth_pool)
    .await;

    let user = match result {
        Ok(u) => u,
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            return Err(AppError::conflict("пользователь с таким email уже существует"));
        }
        Err(e) => return Err(e.into()),
    };
    Ok(Json(user))
}

#[derive(Deserialize)]
pub struct UpdateUserReq {
    pub name: Option<String>,
    pub email: Option<String>,
    pub role: Option<String>,
    pub password: Option<String>,
}

pub async fn update_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUserReq>,
) -> AppResult<Json<UserDto>> {
    let _admin = require_admin(&headers, &state.jwt_secret)?;

    let current: (String, String, String) = sqlx::query_as(
        "SELECT name, email, role FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.auth_pool)
    .await?
    .ok_or_else(|| AppError::not_found("пользователь не найден"))?;

    let name = req.name.unwrap_or(current.0);
    let email = req.email.unwrap_or(current.1).to_lowercase();
    let role = req.role.unwrap_or(current.2);
    if !matches!(role.as_str(), "customer" | "master" | "admin") {
        return Err(AppError::bad_request("недопустимая роль"));
    }

    let password_hash = match req.password {
        Some(p) if !p.is_empty() => Some(
            bcrypt::hash(&p, BCRYPT_COST)
                .map_err(|e| AppError::internal(format!("ошибка хеширования: {e}")))?,
        ),
        _ => None,
    };

    let result = if let Some(h) = password_hash {
        sqlx::query_as::<_, UserDto>(
            "UPDATE users SET name = $1, email = $2, role = $3, password_hash = $4 \
             WHERE id = $5 RETURNING id, name, email, role, created_at",
        )
        .bind(name)
        .bind(&email)
        .bind(&role)
        .bind(&h)
        .bind(id)
        .fetch_one(&state.auth_pool)
        .await
    } else {
        sqlx::query_as::<_, UserDto>(
            "UPDATE users SET name = $1, email = $2, role = $3 \
             WHERE id = $4 RETURNING id, name, email, role, created_at",
        )
        .bind(name)
        .bind(&email)
        .bind(&role)
        .bind(id)
        .fetch_one(&state.auth_pool)
        .await
    };

    let user = match result {
        Ok(u) => u,
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            return Err(AppError::conflict("пользователь с таким email уже существует"));
        }
        Err(e) => return Err(e.into()),
    };
    Ok(Json(user))
}

pub async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let _admin = require_admin(&headers, &state.jwt_secret)?;
    let cnt = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(&state.auth_pool)
        .await?
        .rows_affected();
    if cnt == 0 {
        return Err(AppError::not_found("пользователь не найден"));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}