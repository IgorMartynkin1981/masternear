use axum::extract::State;
use axum::{Json, http::HeaderMap};
use bcrypt::hash;
use chrono::{DateTime, Utc};
use common::AppResult;
use common::{AppError, require_auth};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};

use crate::state::AppState;
use crate::currencies;

const BCRYPT_COST: u32 = 12;

pub async fn init_schema(pool: &PgPool) -> AppResult<()> {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS users (
            id            BIGSERIAL PRIMARY KEY,
            name          TEXT NOT NULL,
            email         TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            role          TEXT NOT NULL CHECK (role IN ('customer', 'master', 'admin')),
            currency      TEXT NOT NULL DEFAULT 'USD',
            created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(pool)
    .await?;
    sqlx::query("ALTER TABLE users ADD COLUMN IF NOT EXISTS currency TEXT NOT NULL DEFAULT 'USD'")
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Deserialize)]
pub struct RegisterReq {
    name: String,
    email: String,
    password: String,
    role: String,
}

#[derive(Deserialize)]
pub struct LoginReq {
    email: String,
    password: String,
}

#[derive(Serialize, FromRow)]
pub struct UserDto {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub role: String,
    pub currency: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct AuthResp {
    pub token: String,
    pub user: UserDto,
}

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterReq>,
) -> AppResult<Json<AuthResp>> {
    let name = req.name.trim();
    let email = req.email.trim().to_lowercase();
    let role = req.role.trim();

    if name.is_empty() || email.is_empty() || req.password.is_empty() {
        return Err(AppError::bad_request("имя, email и пароль обязательны"));
    }
    if !matches!(role, "customer" | "master") {
        return Err(AppError::bad_request("роль должна быть customer или master"));
    }
    if req.password.len() < 8 {
        return Err(AppError::bad_request("пароль должен быть не короче 8 символов"));
    }

    let password_hash = hash(&req.password, BCRYPT_COST)
        .map_err(|e| AppError::internal(format!("ошибка хеширования: {e}")))?;

    let result = sqlx::query_as::<_, UserDto>("\
        INSERT INTO users (name, email, password_hash, role) VALUES ($1, $2, $3, $4) \
        RETURNING id, name, email, role, currency, created_at")
        .bind(name)
        .bind(&email)
        .bind(&password_hash)
        .bind(role)
        .fetch_one(&state.pool)
        .await;

    let user: UserDto = match result {
        Ok(user) => user,
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
            return Err(AppError::conflict("пользователь с таким email уже существует"));
        }
        Err(e) => return Err(e.into()),
    };

    let token = common::jwt::encode_token(user.id, &user.role, &user.currency, &state.jwt_secret)?;
    Ok(Json(AuthResp { token, user }))
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginReq>,
) -> AppResult<Json<AuthResp>> {
    let email = req.email.trim().to_lowercase();

    let row = sqlx::query_as::<_, UserDto>("\
        SELECT id, name, email, role, currency, created_at \
        FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.pool)
        .await?;

    let user = row.ok_or_else(|| AppError::unauthorized("неверный email или пароль"))?;

    let password_hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user.id)
        .fetch_one(&state.pool)
        .await?;

    let valid = bcrypt::verify(&req.password, &password_hash)
        .map_err(|e| AppError::internal(format!("ошибка проверки пароля: {e}")))?;
    if !valid {
        return Err(AppError::unauthorized("неверный email или пароль"));
    }

    let token = common::jwt::encode_token(user.id, &user.role, &user.currency, &state.jwt_secret)?;
    Ok(Json(AuthResp { token, user }))
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<UserDto>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;

    let user = sqlx::query_as::<_, UserDto>("\
        SELECT id, name, email, role, currency, created_at FROM users WHERE id = $1")
        .bind(claims.sub)
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(user))
}

#[derive(Serialize)]
pub struct SettingsResp {
    pub currency: String,
    pub currencies: Vec<currencies::Currency>,
}

#[derive(Deserialize)]
pub struct UpdateSettingsReq {
    pub currency: String,
}

pub async fn settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<SettingsResp>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;

    let currency: String = sqlx::query_scalar("SELECT currency FROM users WHERE id = $1")
        .bind(claims.sub)
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(SettingsResp {
        currency,
        currencies: currencies::ALL.to_vec(),
    }))
}

pub async fn update_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UpdateSettingsReq>,
) -> AppResult<Json<SettingsResp>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;

    let code = req.currency.trim().to_uppercase();
    if currencies::find(&code).is_none() {
        return Err(AppError::bad_request(format!("неизвестная валюта: {code}")));
    }

    sqlx::query("UPDATE users SET currency = $1 WHERE id = $2")
        .bind(&code)
        .bind(claims.sub)
        .execute(&state.pool)
        .await?;

    Ok(Json(SettingsResp {
        currency: code,
        currencies: currencies::ALL.to_vec(),
    }))
}