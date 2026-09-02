use sqlx::PgPool;

use crate::ws::WsState;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub auth_pool: PgPool,
    pub jwt_secret: String,
    pub ws_state: WsState,
    pub notification_url: String,
}