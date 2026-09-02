use axum::http::HeaderMap;
use common::{AppError, AppResult, require_auth};
use sqlx::PgPool;

pub mod dashboard;
pub mod feedback;
pub mod orders;
pub mod users;

pub struct Claims {
    pub sub: i64,
    pub role: String,
}

pub fn require_admin(headers: &HeaderMap, secret: &str) -> Result<Claims, AppError> {
    let claims = require_auth(headers, secret)?;
    if claims.role != "admin" {
        return Err(AppError::forbidden("доступно только администратору"));
    }
    Ok(Claims {
        sub: claims.sub,
        role: claims.role,
    })
}

pub async fn init_schema(pool: &PgPool) -> AppResult<()> {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS feedback (
            id         BIGSERIAL PRIMARY KEY,
            user_id    BIGINT,
            email      TEXT,
            message    TEXT NOT NULL,
            status     TEXT NOT NULL DEFAULT 'new' CHECK (status IN ('new', 'done')),
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(pool)
    .await?;
    Ok(())
}