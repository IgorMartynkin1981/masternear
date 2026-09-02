use axum::http::{HeaderMap, header};
use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use crate::AppError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub role: String,
    pub exp: usize,
}

pub const TOKEN_TTL_HOURS: i64 = 24;

pub fn encode_token(user_id: i64, role: &str, secret: &str) -> Result<String, AppError> {
    let exp = (Utc::now() + Duration::hours(TOKEN_TTL_HOURS)).timestamp() as usize;
    let claims = Claims {
        sub: user_id,
        role: role.to_string(),
        exp,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::internal(format!("не удалось создать токен: {e}")))
}

pub fn decode_token(token: &str, secret: &str) -> Result<Claims, AppError> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|_| AppError::unauthorized("недействительный или истёкший токен"))
}

/// Достаёт Bearer-токен из заголовков и возвращает его claims.
pub fn require_auth(headers: &HeaderMap, secret: &str) -> Result<Claims, AppError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::unauthorized("отсутствует заголовок Authorization"))?;
    decode_token(token, secret)
}