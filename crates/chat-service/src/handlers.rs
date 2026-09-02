use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Utc};
use common::AppResult;
use common::{AppError, require_auth};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};

use crate::state::AppState;

pub async fn init_schema(pool: &PgPool) -> AppResult<()> {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS conversations (
            id          BIGSERIAL PRIMARY KEY,
            customer_id BIGINT NOT NULL,
            master_id   BIGINT NOT NULL,
            created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
            UNIQUE (customer_id, master_id)
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "ALTER TABLE conversations ADD COLUMN IF NOT EXISTS customer_last_read BIGINT NOT NULL DEFAULT 0",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "ALTER TABLE conversations ADD COLUMN IF NOT EXISTS master_last_read BIGINT NOT NULL DEFAULT 0",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS messages (
            id              BIGSERIAL PRIMARY KEY,
            conversation_id BIGINT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            sender_id       BIGINT NOT NULL,
            text            TEXT NOT NULL,
            created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

const EXTRA_SELECT: &str = r#"
    , COALESCE((
        SELECT COUNT(*) FROM messages m
        WHERE m.conversation_id = c.id
          AND m.sender_id <> $2
          AND m.id > (CASE WHEN $2 = c.customer_id THEN c.customer_last_read
                           WHEN $2 = c.master_id    THEN c.master_last_read
                           ELSE 0 END)
      ), 0) AS unread_count
    , (SELECT m.text       FROM messages m WHERE m.conversation_id = c.id ORDER BY m.id DESC LIMIT 1) AS last_message
    , (SELECT m.created_at FROM messages m WHERE m.conversation_id = c.id ORDER BY m.id DESC LIMIT 1) AS last_message_at
    , (SELECT m.sender_id  FROM messages m WHERE m.conversation_id = c.id ORDER BY m.id DESC LIMIT 1) AS last_sender_id
"#;

#[derive(Serialize, FromRow)]
pub struct ConversationDto {
    pub id: i64,
    pub customer_id: i64,
    pub master_id: i64,
    pub created_at: DateTime<Utc>,
    pub unread_count: i64,
    pub last_message: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub last_sender_id: Option<i64>,
}

async fn conversation_full(pool: &PgPool, id: i64, user_id: i64) -> AppResult<Option<ConversationDto>> {
    let sql = format!(
        r#"SELECT c.id, c.customer_id, c.master_id, c.created_at {EXTRA_SELECT}
           FROM conversations c WHERE c.id = $1"#
    );
    Ok(sqlx::query_as::<_, ConversationDto>(&sql)
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?)
}

async fn mark_read(pool: &PgPool, conversation_id: i64, is_customer: bool) -> AppResult<()> {
    let column = if is_customer { "customer_last_read" } else { "master_last_read" };
    let sql = format!(
        "UPDATE conversations SET {column} = \
         COALESCE((SELECT MAX(id) FROM messages WHERE conversation_id = $1), 0) \
         WHERE id = $1"
    );
    sqlx::query(&sql).bind(conversation_id).execute(pool).await?;
    Ok(())
}

#[derive(Deserialize)]
pub struct CreateChatReq {
    pub master_id: i64,
    pub first_message: Option<String>,
}

pub async fn create_chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateChatReq>,
) -> AppResult<Json<ConversationDto>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;
    if claims.role != "customer" {
        return Err(AppError::forbidden("чат создаёт только заказчик"));
    }
    if claims.sub == req.master_id {
        return Err(AppError::bad_request("нельзя создать чат с самим собой"));
    }

    sqlx::query("\
        INSERT INTO conversations (customer_id, master_id) VALUES ($1, $2) \
        ON CONFLICT (customer_id, master_id) DO NOTHING")
        .bind(claims.sub)
        .bind(req.master_id)
        .execute(&state.pool)
        .await?;

    let conversation_id: i64 = sqlx::query_scalar(
        "SELECT id FROM conversations WHERE customer_id = $1 AND master_id = $2",
    )
    .bind(claims.sub)
    .bind(req.master_id)
    .fetch_one(&state.pool)
    .await?;

    let conversation = conversation_full(&state.pool, conversation_id, claims.sub)
        .await?
        .ok_or_else(|| AppError::internal("чат не найден"))?;

    if let Some(text) = req.first_message.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        sqlx::query("\
            INSERT INTO messages (conversation_id, sender_id, text) VALUES ($1, $2, $3)")
            .bind(conversation.id)
            .bind(claims.sub)
            .bind(text)
            .execute(&state.pool)
            .await?;
    }

    mark_read(&state.pool, conversation.id, true).await?;

    Ok(Json(conversation))
}

pub async fn list_chats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<ConversationDto>>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;

    let sql = format!(
        r#"SELECT c.id, c.customer_id, c.master_id, c.created_at {EXTRA_SELECT}
           FROM conversations c
           WHERE $2 = c.customer_id OR $2 = c.master_id
           ORDER BY COALESCE((SELECT m.created_at FROM messages m
                              WHERE m.conversation_id = c.id ORDER BY m.id DESC LIMIT 1), c.created_at) DESC"#
    );
    let rows = sqlx::query_as::<_, ConversationDto>(&sql)
        .bind(claims.sub)
        .bind(claims.sub)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(rows))
}

#[derive(Serialize, FromRow)]
pub struct MessageDto {
    pub id: i64,
    pub conversation_id: i64,
    pub sender_id: i64,
    pub text: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct SendMessageReq {
    pub text: String,
}

async fn load_conversation(
    pool: &PgPool,
    conversation_id: i64,
    user_id: i64,
) -> AppResult<ConversationDto> {
    let sql = format!(
        r#"SELECT c.id, c.customer_id, c.master_id, c.created_at {EXTRA_SELECT}
           FROM conversations c WHERE c.id = $1 AND (c.customer_id = $2 OR c.master_id = $2)"#
    );
    sqlx::query_as::<_, ConversationDto>(&sql)
        .bind(conversation_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found("чат не найден или доступ запрещён"))
}

pub async fn list_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(conversation_id): Path<i64>,
) -> AppResult<Json<Vec<MessageDto>>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;
    let conversation = load_conversation(&state.pool, conversation_id, claims.sub).await?;

    let rows = sqlx::query_as::<_, MessageDto>("\
        SELECT id, conversation_id, sender_id, text, created_at \
        FROM messages WHERE conversation_id = $1 ORDER BY created_at ASC")
        .bind(conversation_id)
        .fetch_all(&state.pool)
        .await?;

    mark_read(
        &state.pool,
        conversation_id,
        claims.sub == conversation.customer_id,
    )
    .await?;

    Ok(Json(rows))
}

pub async fn send_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(conversation_id): Path<i64>,
    Json(req): Json<SendMessageReq>,
) -> AppResult<Json<MessageDto>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;
    let conversation = load_conversation(&state.pool, conversation_id, claims.sub).await?;

    let text = req.text.trim();
    if text.is_empty() {
        return Err(AppError::bad_request("сообщение не может быть пустым"));
    }

    let msg = sqlx::query_as::<_, MessageDto>("\
        INSERT INTO messages (conversation_id, sender_id, text) VALUES ($1, $2, $3) \
        RETURNING id, conversation_id, sender_id, text, created_at")
        .bind(conversation_id)
        .bind(claims.sub)
        .bind(text)
        .fetch_one(&state.pool)
        .await?;

    mark_read(
        &state.pool,
        conversation_id,
        claims.sub == conversation.customer_id,
    )
    .await?;

    if !state.notification_url.is_empty() {
        notify_message(&state, conversation_id, claims.sub, &conversation, text).await;
    }

    Ok(Json(msg))
}

async fn notify_message(
    state: &AppState,
    conversation_id: i64,
    sender_id: i64,
    conversation: &ConversationDto,
    text: &str,
) {
    let recipient_id = if sender_id == conversation.customer_id {
        conversation.master_id
    } else {
        conversation.customer_id
    };

    let recipient = match sqlx::query_as::<_, (String, String)>(
        "SELECT email, name FROM users WHERE id = $1",
    )
    .bind(recipient_id)
    .fetch_optional(&state.auth_pool)
    .await
    {
        Ok(Some(r)) => r,
        _ => return,
    };

    let sender_name = sqlx::query_scalar::<_, String>("SELECT name FROM users WHERE id = $1")
        .bind(sender_id)
        .fetch_optional(&state.auth_pool)
        .await
        .unwrap_or_else(|_| None)
        .unwrap_or_else(|| "Пользователь".to_string());

    let html = common::email::message_email_html(&sender_name, text);
    let subject = format!("MasterNear: новое сообщение в чате #{conversation_id}");
    if let Err(e) =
        common::email::send_email(&state.notification_url, &recipient.0, &subject, &html).await
    {
        tracing::warn!(error = %e, "не удалось отправить уведомление по email");
    }
}