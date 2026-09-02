use std::collections::HashMap;

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
        r#"CREATE TABLE IF NOT EXISTS orders (
            id                BIGSERIAL PRIMARY KEY,
            customer_id       BIGINT NOT NULL,
            category_id       BIGINT NOT NULL,
            title             TEXT NOT NULL,
            description       TEXT NOT NULL DEFAULT '',
            budget            DOUBLE PRECISION NOT NULL,
            status            TEXT NOT NULL DEFAULT 'open'
                              CHECK (status IN ('open', 'selected', 'closed')),
            lat               DOUBLE PRECISION,
            lng               DOUBLE PRECISION,
            selected_offer_id BIGINT,
            created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
            updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"ALTER TABLE orders ADD COLUMN IF NOT EXISTS lat DOUBLE PRECISION"#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"ALTER TABLE orders ADD COLUMN IF NOT EXISTS lng DOUBLE PRECISION"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS offers (
            id         BIGSERIAL PRIMARY KEY,
            order_id   BIGINT NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
            master_id  BIGINT NOT NULL,
            price      DOUBLE PRECISION NOT NULL,
            comment    TEXT NOT NULL DEFAULT '',
            accepted   BOOLEAN NOT NULL DEFAULT false,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            UNIQUE (order_id, master_id)
        )"#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

const ORDER_COLUMNS: &str = "id, customer_id, category_id, title, description, budget, \
                             status, lat, lng, selected_offer_id, created_at, updated_at";

#[derive(Serialize, FromRow)]
struct OrderRow {
    id: i64,
    customer_id: i64,
    category_id: i64,
    title: String,
    description: String,
    budget: f64,
    status: String,
    lat: Option<f64>,
    lng: Option<f64>,
    selected_offer_id: Option<i64>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize, FromRow)]
struct OfferRow {
    id: i64,
    order_id: i64,
    master_id: i64,
    price: f64,
    comment: String,
    accepted: bool,
    created_at: DateTime<Utc>,
}

#[derive(Serialize, Clone)]
pub struct OfferDto {
    id: i64,
    master_id: i64,
    master_name: Option<String>,
    master_rating: Option<f64>,
    price: f64,
    comment: String,
    accepted: bool,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct OrderDto {
    id: i64,
    customer_id: i64,
    category_id: i64,
    category_name: String,
    title: String,
    description: String,
    budget: f64,
    status: String,
    lat: Option<f64>,
    lng: Option<f64>,
    selected_offer_id: Option<i64>,
    my_offer: Option<OfferDto>,
    offers: Vec<OfferDto>,
    offers_count: i64,
    suggested_masters: Vec<SuggestedMaster>,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct SuggestedMaster {
    pub master_id: i64,
    pub name: String,
    pub rating: f64,
    pub distance_km: f64,
}

#[derive(Clone, FromRow)]
struct MasterInfo {
    user_id: i64,
    name: String,
    rating: f64,
}

async fn load_order(pool: &PgPool, id: i64) -> AppResult<Option<OrderRow>> {
    let sql = format!("SELECT {ORDER_COLUMNS} FROM orders WHERE id = $1");
    Ok(sqlx::query_as::<_, OrderRow>(&sql).bind(id).fetch_optional(pool).await?)
}

async fn load_offers(pool: &PgPool, order_id: i64) -> AppResult<Vec<OfferRow>> {
    Ok(sqlx::query_as::<_, OfferRow>("\
        SELECT id, order_id, master_id, price, comment, accepted, created_at \
        FROM offers WHERE order_id = $1 ORDER BY price ASC, created_at ASC")
        .bind(order_id)
        .fetch_all(pool)
        .await?)
}

async fn master_infos(catalog: &PgPool, ids: &[i64]) -> AppResult<HashMap<i64, MasterInfo>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query_as::<_, MasterInfo>("\
        SELECT user_id, name, rating FROM masters WHERE user_id = ANY($1)")
        .bind(ids)
        .fetch_all(catalog)
        .await?;
    Ok(rows.into_iter().map(|m| (m.user_id, m)).collect())
}

async fn category_name(catalog: &PgPool, id: i64) -> AppResult<String> {
    Ok(sqlx::query_scalar("SELECT name FROM categories WHERE id = $1")
        .bind(id)
        .fetch_optional(catalog)
        .await?
        .unwrap_or_else(|| format!("Категория #{id}")))
}

async fn order_dto(
    state: &AppState,
    row: OrderRow,
    viewer_id: i64,
    role: &str,
    to_currency: &str,
) -> AppResult<OrderDto> {
    let cat_name = category_name(&state.catalog_pool, row.category_id).await?;
    let offers = load_offers(&state.pool, row.id).await?;
    let master_ids: Vec<i64> = offers.iter().map(|o| o.master_id).collect();
    let infos = master_infos(&state.catalog_pool, &master_ids).await?;

    async fn convert(v: f64, cur: &str) -> AppResult<f64> {
        if cur.to_uppercase() == "USD" {
            Ok(v)
        } else {
            common::rates::from_usd(v, cur).await
        }
    }

    let mut offer_dtos = Vec::with_capacity(offers.len());
    for o in &offers {
        offer_dtos.push(OfferDto {
            id: o.id,
            master_id: o.master_id,
            master_name: infos.get(&o.master_id).map(|m| m.name.clone()),
            master_rating: infos.get(&o.master_id).map(|m| m.rating),
            price: convert(o.price, to_currency).await?,
            comment: o.comment.clone(),
            accepted: o.accepted,
            created_at: o.created_at,
        });
    }

    let budget = convert(row.budget, to_currency).await?;

    let (my_offer, visible) = if role == "master" {
        let idx = offers.iter().position(|o| o.master_id == viewer_id);
        (idx.map(|i| offer_dtos[i].clone()), Vec::new())
    } else if row.customer_id == viewer_id {
        (None, offer_dtos)
    } else {
        (None, Vec::new())
    };

    let suggested_masters = suggest_masters(
        &state.catalog_pool,
        row.category_id,
        row.lat,
        row.lng,
    )
    .await?;

    Ok(OrderDto {
        id: row.id,
        customer_id: row.customer_id,
        category_id: row.category_id,
        category_name: cat_name,
        title: row.title,
        description: row.description,
        budget,
        status: row.status,
        lat: row.lat,
        lng: row.lng,
        selected_offer_id: row.selected_offer_id,
        my_offer,
        offers: visible,
        offers_count: offers.len() as i64,
        suggested_masters,
        created_at: row.created_at,
    })
}

async fn suggest_masters(
    catalog: &PgPool,
    category_id: i64,
    lat: Option<f64>,
    lng: Option<f64>,
) -> AppResult<Vec<SuggestedMaster>> {
    let (Some(lat), Some(lng)) = (lat, lng) else {
        return Ok(Vec::new());
    };

    let origin = format!("ST_SetSRID(ST_MakePoint({lng}, {lat}), 4326)::geography");
    let sql = format!("\
        SELECT m.user_id, m.name, m.rating, ST_Distance(m.loc_point, {origin}) AS dist_m \
        FROM masters m \
        JOIN master_price mp ON mp.master_id = m.id AND mp.category_id = {category_id} \
        WHERE m.loc_point IS NOT NULL \
        ORDER BY dist_m ASC LIMIT 10");

    let rows: Vec<(i64, String, f64, f64)> = sqlx::query_as(&sql)
        .fetch_all(catalog)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(master_id, name, rating, dist_m)| SuggestedMaster {
            master_id,
            name,
            rating,
            distance_km: dist_m / 1000.0,
        })
        .collect())
}

fn is_positive(v: f64) -> bool {
    v > 0.0 && v.is_finite()
}

fn norm_currency(c: &str) -> String {
    let c = c.trim();
    if c.is_empty() {
        "USD".to_string()
    } else {
        c.to_uppercase()
    }
}

async fn user_email(state: &AppState, user_id: i64) -> Option<String> {
    sqlx::query_scalar::<_, String>("SELECT email FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.auth_pool)
        .await
        .ok()
        .flatten()
}

async fn notify_offer_placed(state: &AppState, order: &OrderRow, price: f64) {
    if state.notification_url.is_empty() {
        return;
    }
    let Some(email) = user_email(state, order.customer_id).await else {
        return;
    };
    let html = format!(
        "<h2>Вам поступило новое предложение</h2>\
         <p>По заказу «<b>{}</b>» мастер предложил цену <b>{} USD</b>.</p>\
         <p><a href=\"https://masternear.example\">Открыть заказ</a></p>",
        order.title.replace('<', "&lt;"),
        price
    );
    let subject = format!("MasterNear: новое предложение по заказу «{}»", order.title);
    if let Err(e) = common::email::send_email(&state.notification_url, &email, &subject, &html).await
    {
        tracing::warn!(error = %e, "не удалось отправить уведомление");
    }
}

async fn notify_order_selected(
    state: &AppState,
    order: &OrderRow,
    master_id: i64,
) {
    if state.notification_url.is_empty() {
        return;
    }
    let Some(email) = user_email(state, master_id).await else {
        return;
    };
    let html = format!(
        "<h2>Ваше предложение принято!</h2>\
         <p>Заказчик выбрал вас для работы «<b>{}</b>».</p>\
         <p><a href=\"https://masternear.example\">Перейти к заказу</a></p>",
        order.title.replace('<', "&lt;")
    );
    let subject = format!("MasterNear: вас выбрали для заказа «{}»", order.title);
    if let Err(e) = common::email::send_email(&state.notification_url, &email, &subject, &html).await
    {
        tracing::warn!(error = %e, "не удалось отправить уведомление");
    }
}

async fn notify_new_order(state: &AppState, order: &OrderRow) {
    if state.notification_url.is_empty() {
        return;
    }
    let master_ids: Vec<i64> =
        sqlx::query_scalar("SELECT user_id FROM masters WHERE id IN (SELECT master_id FROM master_price WHERE category_id = $1)")
            .bind(order.category_id)
            .fetch_all(&state.catalog_pool)
            .await
            .unwrap_or_default();
    for master_id in master_ids {
        let Some(email) = user_email(state, master_id).await else {
            continue;
        };
        let html = format!(
            "<h2>Новый заказ поблизости</h2>\
             <p>Появился заказ «<b>{}</b>» с бюджетом <b>{} USD</b> по вашей категории.</p>\
             <p><a href=\"https://masternear.example\">Посмотреть заказ</a></p>",
            order.title.replace('<', "&lt;"),
            order.budget
        );
        let subject = "MasterNear: новый заказ по вашей категории".to_string();
        if let Err(e) =
            common::email::send_email(&state.notification_url, &email, &subject, &html).await
        {
            tracing::warn!(error = %e, "не удалось отправить уведомление");
        }
    }
}

#[derive(Deserialize)]
pub struct CreateOrderReq {
    pub category_id: i64,
    pub title: String,
    pub description: Option<String>,
    pub budget: f64,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
}

pub async fn create_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateOrderReq>,
) -> AppResult<Json<OrderDto>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;
    if claims.role != "customer" {
        return Err(AppError::forbidden("заказ создаёт только заказчик"));
    }

    let title = req.title.trim();
    if title.is_empty() {
        return Err(AppError::bad_request("опишите работу"));
    }
    if !is_positive(req.budget) {
        return Err(AppError::bad_request("цена должна быть больше нуля"));
    }

    let category_exists: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM categories WHERE id = $1")
            .bind(req.category_id)
            .fetch_optional(&state.catalog_pool)
            .await?;
    if category_exists.is_none() {
        return Err(AppError::bad_request("категория не найдена"));
    }

    let cur = norm_currency(&claims.currency);
    let budget_usd = if cur == "USD" {
        req.budget
    } else {
        common::rates::to_usd(req.budget, &cur).await?
    };

    let row = sqlx::query_as::<_, OrderRow>(&format!(
        "INSERT INTO orders (customer_id, category_id, title, description, budget, lat, lng) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING {ORDER_COLUMNS}"
    ))
    .bind(claims.sub)
    .bind(req.category_id)
    .bind(title)
    .bind(req.description.unwrap_or_default())
    .bind(budget_usd)
    .bind(req.lat)
    .bind(req.lng)
    .fetch_one(&state.pool)
    .await?;

    notify_new_order(&state, &row).await;

    Ok(Json(order_dto(&state, row, claims.sub, &claims.role, &cur).await?))
}

pub async fn list_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<OrderDto>>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;

    let rows: Vec<OrderRow> = if claims.role == "master" {
        sqlx::query_as::<_, OrderRow>(&format!(
            "SELECT {ORDER_COLUMNS} FROM orders o \
             WHERE o.status = 'open' \
                OR EXISTS (SELECT 1 FROM offers f WHERE f.order_id = o.id AND f.master_id = $1) \
             ORDER BY o.created_at DESC"
        ))
        .bind(claims.sub)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as::<_, OrderRow>(&format!(
            "SELECT {ORDER_COLUMNS} FROM orders o \
             WHERE o.customer_id = $1 ORDER BY o.created_at DESC"
        ))
        .bind(claims.sub)
        .fetch_all(&state.pool)
        .await?
    };

    let cur = norm_currency(&claims.currency);
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        result.push(order_dto(&state, row, claims.sub, &claims.role, &cur).await?);
    }
    Ok(Json(result))
}

pub async fn order_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> AppResult<Json<OrderDto>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;

    let row = load_order(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("заказ не найден"))?;

    let allowed = if claims.role == "customer" {
        row.customer_id == claims.sub
    } else {
        row.status == "open"
            || sqlx::query_scalar::<_, i64>(
                "SELECT 1 FROM offers WHERE order_id = $1 AND master_id = $2 LIMIT 1",
            )
            .bind(id)
            .bind(claims.sub)
            .fetch_optional(&state.pool)
            .await?
            .is_some()
    };

    if !allowed {
        return Err(AppError::not_found("заказ не найден или доступ запрещён"));
    }

    Ok(Json(
        order_dto(&state, row, claims.sub, &claims.role, &norm_currency(&claims.currency)).await?,
    ))
}

#[derive(Deserialize)]
pub struct OfferReq {
    pub price: f64,
    pub comment: Option<String>,
}

pub async fn place_offer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(order_id): Path<i64>,
    Json(req): Json<OfferReq>,
) -> AppResult<Json<OfferDto>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;
    if claims.role != "master" {
        return Err(AppError::forbidden("предложения отправляют только мастера"));
    }
    if !is_positive(req.price) {
        return Err(AppError::bad_request("цена должна быть больше нуля"));
    }

    let order = load_order(&state.pool, order_id)
        .await?
        .ok_or_else(|| AppError::not_found("заказ не найден"))?;
    if order.customer_id == claims.sub {
        return Err(AppError::bad_request("нельзя предлагать работу самому себе"));
    }
    if order.status != "open" {
        return Err(AppError::bad_request("приём предложений по заказу уже закрыт"));
    }

    let comment = req.comment.unwrap_or_default();

    let cur = norm_currency(&claims.currency);
    let price_usd = if cur == "USD" {
        req.price
    } else {
        common::rates::to_usd(req.price, &cur).await?
    };

    let offer = sqlx::query_as::<_, OfferRow>("\
        INSERT INTO offers (order_id, master_id, price, comment) VALUES ($1, $2, $3, $4) \
        ON CONFLICT (order_id, master_id) DO UPDATE \
            SET price = EXCLUDED.price, comment = EXCLUDED.comment, updated_at = now() \
        RETURNING id, order_id, master_id, price, comment, accepted, created_at")
        .bind(order_id)
        .bind(claims.sub)
        .bind(price_usd)
        .bind(comment)
        .fetch_one(&state.pool)
        .await?;

    let ids = [offer.master_id];
    let infos = master_infos(&state.catalog_pool, &ids).await?;

    notify_offer_placed(&state, &order, offer.price).await;

    let price = if cur == "USD" {
        offer.price
    } else {
        common::rates::from_usd(offer.price, &cur).await?
    };

    Ok(Json(OfferDto {
        id: offer.id,
        master_id: offer.master_id,
        master_name: infos.get(&offer.master_id).map(|m| m.name.clone()),
        master_rating: infos.get(&offer.master_id).map(|m| m.rating),
        price,
        comment: offer.comment,
        accepted: offer.accepted,
        created_at: offer.created_at,
    }))
}

#[derive(Deserialize)]
pub struct SelectReq {
    pub offer_id: i64,
}

pub async fn select_offer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(order_id): Path<i64>,
    Json(req): Json<SelectReq>,
) -> AppResult<Json<OrderDto>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;

    let order = load_order(&state.pool, order_id)
        .await?
        .ok_or_else(|| AppError::not_found("заказ не найден"))?;
    if order.customer_id != claims.sub {
        return Err(AppError::forbidden("заказ выбирает только его владелец"));
    }
    if order.status != "open" {
        return Err(AppError::bad_request("по заказу уже выбран мастер"));
    }

    let offer_exists: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM offers WHERE id = $1 AND order_id = $2",
    )
    .bind(req.offer_id)
    .bind(order_id)
    .fetch_optional(&state.pool)
    .await?;
    if offer_exists.is_none() {
        return Err(AppError::bad_request("предложение не найдено"));
    }

    sqlx::query("UPDATE offers SET accepted = (id = $1) WHERE order_id = $2")
        .bind(req.offer_id)
        .bind(order_id)
        .execute(&state.pool)
        .await?;

    sqlx::query("\
        UPDATE orders SET status = 'selected', selected_offer_id = $1, updated_at = now() \
        WHERE id = $2")
        .bind(req.offer_id)
        .bind(order_id)
        .execute(&state.pool)
        .await?;

    let offer_master: Option<(i64,)> =
        sqlx::query_as("SELECT master_id FROM offers WHERE id = $1 AND order_id = $2")
            .bind(req.offer_id)
            .bind(order_id)
            .fetch_optional(&state.pool)
            .await?;
    let master_id = offer_master.map(|m| m.0);

    let updated = load_order(&state.pool, order_id).await?.unwrap();

    if let Some(master_id) = master_id {
        notify_order_selected(&state, &updated, master_id).await;
    }

    Ok(Json(
        order_dto(&state, updated, claims.sub, &claims.role, &norm_currency(&claims.currency)).await?,
    ))
}