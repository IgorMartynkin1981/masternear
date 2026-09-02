use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, broadcast};

use crate::state::AppState;

#[derive(Clone)]
pub struct WsState {
    pub channels: Arc<RwLock<HashMap<i64, broadcast::Sender<String>>>>,
    pub connections: Arc<RwLock<HashMap<i64, HashSet<i64>>>>,
}

impl WsState {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[derive(Deserialize)]
pub struct WsQuery {
    pub token: String,
    pub conversation_id: i64,
}

#[derive(Serialize)]
struct WsIncoming {
    #[serde(rename = "type")]
    msg_type: String,
    conversation_id: i64,
    sender_id: i64,
    text: String,
    created_at: String,
    message_id: i64,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<WsQuery>,
) -> impl IntoResponse {
    let claims = match common::jwt::decode_token(&query.token, &state.jwt_secret) {
        Ok(c) => c,
        Err(_) => return ws.on_upgrade(|_| async move {}).into_response(),
    };

    let conversation_id = query.conversation_id;

    let is_participant: Option<(i64,)> = match sqlx::query_as(
        "SELECT id FROM conversations WHERE id = $1 AND (customer_id = $2 OR master_id = $2)",
    )
    .bind(conversation_id)
    .bind(claims.sub)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = ?e, "ошибка проверки участника");
            None
        }
    };

    if is_participant.is_none() {
        return ws.on_upgrade(|_| async move {}).into_response();
    }

    let user_id = claims.sub;
    let ws_state = state.ws_state.clone();

    ws.on_upgrade(move |socket| handle_socket(socket, state, ws_state, conversation_id, user_id))
        .into_response()
}

async fn handle_socket(
    socket: WebSocket,
    state: AppState,
    ws_state: WsState,
    conversation_id: i64,
    user_id: i64,
) {
    let mut rx = {
        let channels = ws_state.channels.read().await;
        channels.get(&conversation_id).cloned().map(|tx| tx.subscribe())
    };
    if rx.is_none() {
        let (tx, new_rx) = broadcast::channel(256);
        ws_state.channels.write().await.insert(conversation_id, tx);
        rx = Some(new_rx);
    }
    let rx = rx.unwrap();

    {
        let mut conns = ws_state.connections.write().await;
        conns.entry(conversation_id).or_default().insert(user_id);
    }

    tracing::info!(user_id, conversation_id, "WebSocket подключён");

    let (mut sender, mut receiver) = socket.split();

    let mut send_task = tokio::spawn(async move {
        let mut rx = rx;
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let ws_state_clone = ws_state.clone();
    let pool = state.pool.clone();
    let conversation_id_clone = conversation_id;

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    if let Ok(incoming) = serde_json::from_str::<serde_json::Value>(&text) {
                        let msg_type = incoming.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if msg_type == "message" {
                            if let Some(text_val) = incoming.get("text").and_then(|v| v.as_str()) {
                                let trimmed = text_val.trim();
                                if trimmed.is_empty() {
                                    continue;
                                }

                                let result = sqlx::query_as::<_, (i64, String, String)>(
                                    "INSERT INTO messages (conversation_id, sender_id, text) \
                                     VALUES ($1, $2, $3) \
                                     RETURNING id, text, created_at::text",
                                )
                                .bind(conversation_id_clone)
                                .bind(user_id)
                                .bind(trimmed)
                                .fetch_one(&pool)
                                .await;

                                if let Ok((msg_id, _text, created_at)) = result {
                                    let out = WsIncoming {
                                        msg_type: "message".into(),
                                        conversation_id: conversation_id_clone,
                                        sender_id: user_id,
                                        text: trimmed.to_string(),
                                        created_at,
                                        message_id: msg_id,
                                    };

                                    if let Ok(json) = serde_json::to_string(&out) {
                                        let channels = ws_state_clone.channels.read().await;
                                        if let Some(tx) = channels.get(&conversation_id_clone) {
                                            let _ = tx.send(json);
                                        }
                                    }

                                    let column = if is_customer(&pool, conversation_id_clone, user_id).await {
                                        "customer_last_read"
                                    } else {
                                        "master_last_read"
                                    };
                                    let update_sql = format!(
                                        "UPDATE conversations SET {column} = \
                                         COALESCE((SELECT MAX(id) FROM messages WHERE conversation_id = $1), 0) \
                                         WHERE id = $1"
                                    );
                                    let _ = sqlx::query(&update_sql)
                                        .bind(conversation_id_clone)
                                        .execute(&pool)
                                        .await;
                                }
                            }
                        } else if msg_type == "typing" {
                            let out = serde_json::json!({
                                "type": "typing",
                                "conversation_id": conversation_id_clone,
                                "user_id": user_id,
                            });
                            if let Ok(json) = serde_json::to_string(&out) {
                                let channels = ws_state_clone.channels.read().await;
                                if let Some(tx) = channels.get(&conversation_id_clone) {
                                    let _ = tx.send(json);
                                }
                            }
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }

    {
        let mut conns = ws_state.connections.write().await;
        if let Some(set) = conns.get_mut(&conversation_id) {
            set.remove(&user_id);
            if set.is_empty() {
                conns.remove(&conversation_id);
            }
        }
    }

    tracing::info!(user_id, conversation_id, "WebSocket отключён");
}

async fn is_customer(pool: &sqlx::PgPool, conversation_id: i64, user_id: i64) -> bool {
    let result: Option<(i64,)> = sqlx::query_as(
        "SELECT customer_id FROM conversations WHERE id = $1 AND customer_id = $2",
    )
    .bind(conversation_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);
    result.is_some()
}
