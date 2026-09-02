use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, header};
use axum::Json;
use common::AppResult;
use common::{AppError, require_auth};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};

use crate::state::AppState;

const SEED_CATEGORIES: &[&str] = &[
    "Сантехника",
    "Электрика",
    "Мелкий ремонт",
    "Сборка мебели",
    "Уборка",
    "Покраска",
    "Установка бытовой техники",
    "Другое",
];

pub async fn init_schema(pool: &PgPool) -> AppResult<()> {
    sqlx::query("CREATE EXTENSION IF NOT EXISTS postgis").execute(pool).await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS categories (
            id   BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS masters (
            id         BIGSERIAL PRIMARY KEY,
            user_id    BIGINT NOT NULL UNIQUE,
            name       TEXT NOT NULL,
            bio        TEXT NOT NULL DEFAULT '',
            city       TEXT NOT NULL DEFAULT '',
            lat        DOUBLE PRECISION,
            lng        DOUBLE PRECISION,
            rating     DOUBLE PRECISION NOT NULL DEFAULT 0,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"ALTER TABLE masters ADD COLUMN IF NOT EXISTS city TEXT NOT NULL DEFAULT ''"#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"ALTER TABLE masters ADD COLUMN IF NOT EXISTS lat DOUBLE PRECISION"#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"ALTER TABLE masters ADD COLUMN IF NOT EXISTS lng DOUBLE PRECISION"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"ALTER TABLE masters ADD COLUMN IF NOT EXISTS loc_point geography(Point, 4326)
           GENERATED ALWAYS AS (
             CASE WHEN lat IS NOT NULL AND lng IS NOT NULL
                  THEN ST_SetSRID(ST_MakePoint(lng, lat), 4326)::geography
                  ELSE NULL END
           ) STORED"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_masters_loc ON masters USING gist (loc_point)"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS master_price (
            master_id   BIGINT NOT NULL REFERENCES masters(id) ON DELETE CASCADE,
            category_id BIGINT NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
            price       DOUBLE PRECISION NOT NULL CHECK (price >= 0),
            PRIMARY KEY (master_id, category_id)
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS ratings (
            master_id   BIGINT NOT NULL REFERENCES masters(id) ON DELETE CASCADE,
            customer_id BIGINT NOT NULL,
            score       SMALLINT NOT NULL CHECK (score BETWEEN 1 AND 5),
            created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
            updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY (customer_id, master_id)
        )"#,
    )
    .execute(pool)
    .await?;

    for name in SEED_CATEGORIES {
        sqlx::query("INSERT INTO categories (name) VALUES ($1) ON CONFLICT (name) DO NOTHING")
            .bind(name)
            .execute(pool)
            .await?;
    }

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS photos (
            id             BIGSERIAL PRIMARY KEY,
            master_id      BIGINT REFERENCES masters(id) ON DELETE SET NULL,
            owner_user_id  BIGINT NOT NULL,
            url            TEXT NOT NULL,
            created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(Serialize, FromRow)]
pub struct CategoryDto {
    pub id: i64,
    pub name: String,
}

pub async fn list_categories(State(state): State<AppState>) -> AppResult<Json<Vec<CategoryDto>>> {
    let rows = sqlx::query_as::<_, CategoryDto>("SELECT id, name FROM categories ORDER BY id")
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct GeocodeReq {
    pub place: String,
}

#[derive(Serialize)]
pub struct GeocodeResp {
    pub lat: f64,
    pub lng: f64,
    pub display: String,
}

pub async fn geocode_place(
    Json(req): Json<GeocodeReq>,
) -> AppResult<Json<GeocodeResp>> {
    let place = req.place.trim();
    if place.is_empty() {
        return Err(AppError::bad_request("укажите место"));
    }
    let coords = crate::geocode::geocode(place).await?;
    Ok(Json(GeocodeResp {
        lat: coords.lat,
        lng: coords.lng,
        display: place.to_string(),
    }))
}

#[derive(Serialize, FromRow)]
pub struct MasterDto {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub bio: String,
    pub city: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub rating: f64,
    #[sqlx(skip)]
    pub rating_count: i64,
    #[sqlx(skip)]
    pub distance_km: Option<f64>,
    #[sqlx(skip)]
    pub prices: Vec<PriceDto>,
    #[sqlx(skip)]
    pub photos: Vec<String>,
}

async fn attach_photos(pool: &PgPool, masters: &mut [MasterDto]) -> AppResult<()> {
    for master in masters.iter_mut() {
        if master.id == 0 {
            continue;
        }
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT url FROM photos WHERE master_id = $1 ORDER BY id")
                .bind(master.id)
                .fetch_all(pool)
                .await?;
        master.photos = rows;
    }
    Ok(())
}

async fn attach_rating_counts(pool: &PgPool, masters: &mut [MasterDto]) -> AppResult<()> {
    if masters.is_empty() {
        return Ok(());
    }
    let ids: Vec<i64> = masters.iter().map(|m| m.id).collect();
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT master_id, COUNT(*) AS cnt FROM ratings WHERE master_id = ANY($1) GROUP BY master_id",
    )
    .bind(ids.as_slice())
    .fetch_all(pool)
    .await?;
    let counts: std::collections::HashMap<i64, i64> = rows.into_iter().collect();
    for master in masters {
        master.rating_count = counts.get(&master.id).copied().unwrap_or(0);
    }
    Ok(())
}

#[derive(Serialize)]
pub struct RatingResp {
    pub avg: f64,
    pub count: i64,
    pub my_score: i32,
}

#[derive(Serialize, FromRow)]
pub struct PriceDto {
    pub category_id: i64,
    pub category_name: String,
    pub price: f64,
}

#[derive(Deserialize)]
pub struct ListMastersQuery {
    pub category_id: Option<i64>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub radius_km: Option<f64>,
    #[serde(default = "default_sort")]
    pub sort: String,
}

fn default_sort() -> String {
    "rating".to_string()
}

async fn load_prices(pool: &PgPool, master_id: i64, to_currency: &str) -> AppResult<Vec<PriceDto>> {
    let rows = sqlx::query_as::<_, PriceDto>("\
        SELECT mp.category_id, c.name AS category_name, mp.price \
        FROM master_price mp JOIN categories c ON c.id = mp.category_id \
        WHERE mp.master_id = $1 ORDER BY c.name")
        .bind(master_id)
        .fetch_all(pool)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for mut r in rows {
        if to_currency.to_uppercase() != "USD" {
            r.price = common::rates::from_usd(r.price, to_currency).await?;
        }
        out.push(r);
    }
    Ok(out)
}

/// Верхний регистр или USD по умолчанию, если валюты нет/пустая.
fn norm_currency(c: &str) -> String {
    let c = c.trim();
    if c.is_empty() {
        "USD".to_string()
    } else {
        c.to_uppercase()
    }
}

/// Валюту пользователя берём из токена, если он есть (иначе USD).
fn currency_from_headers(headers: &HeaderMap, state: &AppState) -> String {
    let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return "USD".to_string();
    };
    match common::jwt::decode_token(token, &state.jwt_secret) {
        Ok(claims) => norm_currency(common::jwt::claims_currency(&claims)),
        Err(_) => "USD".to_string(),
    }
}

pub async fn list_masters(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListMastersQuery>,
) -> AppResult<Json<Vec<MasterDto>>> {
    let has_geo = query.lat.is_some() && query.lng.is_some();
    let radius = query.radius_km.unwrap_or(50.0);
    if radius <= 0.0 || !radius.is_finite() {
        return Err(AppError::bad_request("радиус должен быть больше нуля"));
    }
    let to_currency = currency_from_headers(&headers, &state);

    let masters: Vec<MasterDto> = if has_geo {
        geo_query(&state.pool, query.category_id, query.lat.unwrap(), query.lng.unwrap(), radius).await?
    } else if let Some(category_id) = query.category_id {
        sqlx::query_as::<_, MasterDto>("\
            SELECT m.id, m.user_id, m.name, m.bio, m.city, m.lat, m.lng, m.rating, m.created_at \
            FROM masters m \
            JOIN master_price mp ON mp.master_id = m.id AND mp.category_id = $1 \
            ORDER BY m.rating DESC, m.name")
            .bind(category_id)
            .fetch_all(&state.pool)
            .await?
    } else {
        sqlx::query_as::<_, MasterDto>("\
            SELECT id, user_id, name, bio, city, lat, lng, rating, created_at \
            FROM masters ORDER BY rating DESC, name")
            .fetch_all(&state.pool)
            .await?
    };

    let mut result = masters;
    for master in &mut result {
        master.prices = load_prices(&state.pool, master.id, &to_currency).await?;
    }
    attach_rating_counts(&state.pool, &mut result).await?;
    attach_photos(&state.pool, &mut result).await?;
    if query.sort == "distance" {
        result.sort_by(|a, b| {
            let da = a.distance_km.unwrap_or(f64::MAX);
            let db = b.distance_km.unwrap_or(f64::MAX);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    Ok(Json(result))
}

#[derive(FromRow)]
struct GeoMasterRow {
    id: i64,
    user_id: i64,
    name: String,
    bio: String,
    city: String,
    lat: Option<f64>,
    lng: Option<f64>,
    rating: f64,
    dist_m: f64,
}

async fn geo_query(
    pool: &PgPool,
    category_id: Option<i64>,
    lat: f64,
    lng: f64,
    radius_km: f64,
) -> AppResult<Vec<MasterDto>> {
    let origin = format!("ST_SetSRID(ST_MakePoint({lng}, {lat}), 4326)::geography");
    let dist_expr = format!("ST_Distance(m.loc_point, {origin}) AS dist_m");

    let sql = if let Some(category_id) = category_id {
        format!("\
            SELECT m.id, m.user_id, m.name, m.bio, m.city, m.lat, m.lng, m.rating, {dist_expr} \
            FROM masters m \
            JOIN master_price mp ON mp.master_id = m.id AND mp.category_id = {category_id} \
            WHERE m.loc_point IS NOT NULL \
              AND ST_DWithin(m.loc_point, {origin}, {radius_km} * 1000) \
            ORDER BY m.rating DESC, m.name")
    } else {
        format!("\
            SELECT m.id, m.user_id, m.name, m.bio, m.city, m.lat, m.lng, m.rating, {dist_expr} \
            FROM masters m \
            WHERE m.loc_point IS NOT NULL \
              AND ST_DWithin(m.loc_point, {origin}, {radius_km} * 1000) \
            ORDER BY m.rating DESC, m.name")
    };

    let rows: Vec<GeoMasterRow> = sqlx::query_as::<_, GeoMasterRow>(&sql)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| MasterDto {
            id: r.id,
            user_id: r.user_id,
            name: r.name,
            bio: r.bio,
            city: r.city,
            lat: r.lat,
            lng: r.lng,
            rating: r.rating,
            rating_count: 0,
            distance_km: Some(r.dist_m / 1000.0),
            prices: Vec::new(),
            photos: Vec::new(),
        })
        .collect())
}

fn require_master(headers: &HeaderMap, state: &AppState) -> AppResult<()> {
    let claims = require_auth(headers, &state.jwt_secret)?;
    if claims.role != "master" {
        return Err(AppError::forbidden("только для мастера"));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct UpsertProfileReq {
    name: String,
    bio: String,
    city: Option<String>,
    lat: Option<f64>,
    lng: Option<f64>,
}

pub async fn upsert_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UpsertProfileReq>,
) -> AppResult<Json<MasterDto>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;
    if claims.role != "master" {
        return Err(AppError::forbidden("только для мастера"));
    }

    let name = req.name.trim();
    let bio = req.bio.trim();
    if name.is_empty() {
        return Err(AppError::bad_request("имя обязательно"));
    }

    let mut city = req.city.unwrap_or_default().trim().to_string();
    let mut lat = req.lat;
    let mut lng = req.lng;

    if lat.is_none() || lng.is_none() {
        let place = if !city.is_empty() {
            city.clone()
        } else if req.lat.is_some() || req.lng.is_some() {
            "".into()
        } else {
            String::new()
        };

        if lat.is_none() || lng.is_none() {
            if place.is_empty() {
                // neither city nor coords -> keep empty (no geolocation)
                lat = None;
                lng = None;
            } else {
                let coords = crate::geocode::geocode(&place).await?;
                if lat.is_none() {
                    lat = Some(coords.lat);
                }
                if lng.is_none() {
                    lng = Some(coords.lng);
                }
                if city.is_empty() {
                    city = place;
                }
            }
        }
    }

    let master = sqlx::query_as::<_, MasterDto>("\
        INSERT INTO masters (user_id, name, bio, city, lat, lng) VALUES ($1, $2, $3, $4, $5, $6) \
        ON CONFLICT (user_id) DO UPDATE \
            SET name = EXCLUDED.name, bio = EXCLUDED.bio, \
                city = EXCLUDED.city, lat = EXCLUDED.lat, lng = EXCLUDED.lng \
        RETURNING id, user_id, name, bio, city, lat, lng, rating, created_at")
        .bind(claims.sub)
        .bind(name)
        .bind(bio)
        .bind(&city)
        .bind(lat)
        .bind(lng)
        .fetch_one(&state.pool)
        .await?;

    let mut dto = master;
    dto.prices = load_prices(&state.pool, dto.id, &norm_currency(&claims.currency)).await?;
    attach_rating_counts(&state.pool, std::slice::from_mut(&mut dto)).await?;
    attach_photos(&state.pool, std::slice::from_mut(&mut dto)).await?;
    Ok(Json(dto))
}

#[derive(Deserialize)]
pub struct RateReq {
    pub score: i32,
}

pub async fn rate_master(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(master_user_id): Path<i64>,
    Json(req): Json<RateReq>,
) -> AppResult<Json<RatingResp>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;
    if claims.role != "customer" {
        return Err(AppError::forbidden("оценивать могут только заказчики"));
    }
    if !(1..=5).contains(&req.score) {
        return Err(AppError::bad_request("оценка должна быть от 1 до 5 звёзд"));
    }
    if claims.sub == master_user_id {
        return Err(AppError::bad_request("нельзя оценить самого себя"));
    }

    let master_id: i64 = sqlx::query_scalar("SELECT id FROM masters WHERE user_id = $1")
        .bind(master_user_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("мастер не найден"))?;

    sqlx::query(
        "INSERT INTO ratings (master_id, customer_id, score) VALUES ($1, $2, $3) \
         ON CONFLICT (customer_id, master_id) \
         DO UPDATE SET score = EXCLUDED.score, updated_at = now()",
    )
    .bind(master_id)
    .bind(claims.sub)
    .bind(req.score as i16)
    .execute(&state.pool)
    .await?;

    sqlx::query(
        "UPDATE masters SET rating = \
         COALESCE((SELECT AVG(score) FROM ratings WHERE master_id = $1), 0) WHERE id = $1",
    )
    .bind(master_id)
    .execute(&state.pool)
    .await?;

    let avg: f64 = sqlx::query_scalar(
        "SELECT COALESCE(AVG(score), 0)::float8 FROM ratings WHERE master_id = $1",
    )
    .bind(master_id)
    .fetch_one(&state.pool)
    .await?;

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ratings WHERE master_id = $1")
        .bind(master_id)
        .fetch_one(&state.pool)
        .await?;

    let my_score: i16 = sqlx::query_scalar(
        "SELECT score FROM ratings WHERE master_id = $1 AND customer_id = $2",
    )
    .bind(master_id)
    .bind(claims.sub)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(RatingResp {
        avg,
        count,
        my_score: my_score as i32,
    }))
}

pub async fn my_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<MasterDto>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;

    let master = sqlx::query_as::<_, MasterDto>("\
        SELECT id, user_id, name, bio, city, lat, lng, rating, created_at \
        FROM masters WHERE user_id = $1")
        .bind(claims.sub)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("профиль мастера не заполнен"))?;

    let mut dto = master;
    dto.prices = load_prices(&state.pool, dto.id, &norm_currency(&claims.currency)).await?;
    attach_rating_counts(&state.pool, std::slice::from_mut(&mut dto)).await?;
    attach_photos(&state.pool, std::slice::from_mut(&mut dto)).await?;
    Ok(Json(dto))
}

#[derive(Deserialize)]
pub struct SetPriceReq {
    pub category_id: i64,
    pub price: f64,
}

pub async fn set_price(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SetPriceReq>,
) -> AppResult<Json<PriceDto>> {
    require_master(&headers, &state)?;
    let claims = require_auth(&headers, &state.jwt_secret)?;

    if req.price < 0.0 {
        return Err(AppError::bad_request("цена не может быть отрицательной"));
    }

    let cur = norm_currency(&claims.currency);
    // Введённая цена — в валюте пользователя; храним в базовой (USD).
    let price_usd = if cur == "USD" {
        req.price
    } else {
        common::rates::to_usd(req.price, &cur).await?
    };

    let master: (i64,) = sqlx::query_as("SELECT id FROM masters WHERE user_id = $1")
        .bind(claims.sub)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::bad_request("сначала заполните профиль мастера"))?;

    let category: (i64,) = sqlx::query_as("SELECT id FROM categories WHERE id = $1")
        .bind(req.category_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("категория не найдена"))?;

    let dto = sqlx::query_as::<_, PriceDto>("\
        INSERT INTO master_price (master_id, category_id, price) VALUES ($1, $2, $3) \
        ON CONFLICT (master_id, category_id) DO UPDATE SET price = EXCLUDED.price \
        RETURNING category_id, (SELECT name FROM categories WHERE id = category_id) AS category_name, price")
        .bind(master.0)
        .bind(category.0)
        .bind(price_usd)
        .fetch_one(&state.pool)
        .await?;

    let mut dto = dto;
    if cur != "USD" {
        dto.price = common::rates::from_usd(dto.price, &cur).await?;
    }

    Ok(Json(dto))
}