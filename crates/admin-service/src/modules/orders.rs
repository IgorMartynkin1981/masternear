use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Utc};
use common::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::modules::require_admin;
use crate::state::AppState;

#[derive(Serialize, FromRow)]
pub struct OrderDto {
    pub id: i64,
    pub customer_id: i64,
    pub category_id: i64,
    pub title: String,
    pub description: String,
    pub budget: f64,
    pub status: String,
    pub selected_offer_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn list_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<OrderDto>>> {
    let _admin = require_admin(&headers, &state.jwt_secret)?;
    let rows = sqlx::query_as::<_, OrderDto>(
        "SELECT id, customer_id, category_id, title, description, budget, status, \
         selected_offer_id, created_at, updated_at FROM orders ORDER BY id DESC",
    )
    .fetch_all(&state.orders_pool)
    .await?;
    Ok(Json(rows))
}

pub async fn get_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> AppResult<Json<OrderDto>> {
    let _admin = require_admin(&headers, &state.jwt_secret)?;
    let row = sqlx::query_as::<_, OrderDto>(
        "SELECT id, customer_id, category_id, title, description, budget, status, \
         selected_offer_id, created_at, updated_at FROM orders WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.orders_pool)
    .await?
    .ok_or_else(|| AppError::not_found("заказ не найден"))?;
    Ok(Json(row))
}

#[derive(Deserialize)]
pub struct UpdateOrderReq {
    pub title: Option<String>,
    pub description: Option<String>,
    pub budget: Option<f64>,
    pub status: Option<String>,
}

pub async fn update_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateOrderReq>,
) -> AppResult<Json<OrderDto>> {
    let _admin = require_admin(&headers, &state.jwt_secret)?;

    let current: OrderDto = sqlx::query_as(
        "SELECT id, customer_id, category_id, title, description, budget, status, \
         selected_offer_id, created_at, updated_at FROM orders WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.orders_pool)
    .await?
    .ok_or_else(|| AppError::not_found("заказ не найден"))?;

    let title = req.title.unwrap_or(current.title);
    let description = req.description.unwrap_or(current.description);
    let budget = req.budget.unwrap_or(current.budget);
    let status = req.status.unwrap_or(current.status);

    if title.trim().is_empty() {
        return Err(AppError::bad_request("название не может быть пустым"));
    }
    if budget <= 0.0 {
        return Err(AppError::bad_request("бюджет должен быть больше нуля"));
    }
    if !matches!(status.as_str(), "open" | "selected" | "closed") {
        return Err(AppError::bad_request("недопустимый статус"));
    }

    let row = sqlx::query_as::<_, OrderDto>(
        "UPDATE orders SET title = $1, description = $2, budget = $3, status = $4, updated_at = now() \
         WHERE id = $5 RETURNING id, customer_id, category_id, title, description, budget, status, \
         selected_offer_id, created_at, updated_at",
    )
    .bind(title)
    .bind(description)
    .bind(budget)
    .bind(status)
    .bind(id)
    .fetch_one(&state.orders_pool)
    .await?;

    Ok(Json(row))
}

pub async fn delete_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let _admin = require_admin(&headers, &state.jwt_secret)?;
    let cnt = sqlx::query("DELETE FROM orders WHERE id = $1")
        .bind(id)
        .execute(&state.orders_pool)
        .await?
        .rows_affected();
    if cnt == 0 {
        return Err(AppError::not_found("заказ не найден"));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}