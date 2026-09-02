use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use common::AppResult;
use serde::Serialize;

use crate::modules::require_admin;
use crate::state::AppState;

#[derive(Serialize)]
pub struct Overview {
    pub users: i64,
    pub masters: i64,
    pub categories: i64,
    pub orders: i64,
    pub orders_open: i64,
    pub offers: i64,
    pub feedback: i64,
    pub feedback_new: i64,
}

pub async fn overview(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Overview>> {
    let _admin = require_admin(&headers, &state.jwt_secret)?;

    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.auth_pool)
        .await?;
    let masters: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM masters")
        .fetch_one(&state.catalog_pool)
        .await?;
    let categories: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories")
        .fetch_one(&state.catalog_pool)
        .await?;
    let orders: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orders")
        .fetch_one(&state.orders_pool)
        .await?;
    let orders_open: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orders WHERE status = 'open'")
        .fetch_one(&state.orders_pool)
        .await?;
    let offers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM offers")
        .fetch_one(&state.orders_pool)
        .await?;
    let feedback: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM feedback")
        .fetch_one(&state.pool)
        .await?;
    let feedback_new: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM feedback WHERE status = 'new'")
            .fetch_one(&state.pool)
            .await?;

    Ok(Json(Overview {
        users,
        masters,
        categories,
        orders,
        orders_open,
        offers,
        feedback,
        feedback_new,
    }))
}