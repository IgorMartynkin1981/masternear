use axum::routing::get;
use axum::Router;
use common::AppResult;
use sqlx::PgPool;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

pub mod modules;
mod state;

use state::AppState;

const DEFAULT_ADDR: &str = "127.0.0.1:8085";
const ADMIN_EMAIL: &str = "admin@example.com";

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let admin_db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL не задана");
    let auth_db_url = std::env::var("AUTH_DATABASE_URL").unwrap_or_else(|_| db_url(&admin_db_url, "auth_db"));
    let catalog_db_url = std::env::var("CATALOG_DATABASE_URL").unwrap_or_else(|_| db_url(&admin_db_url, "catalog_db"));
    let orders_db_url = std::env::var("ORDERS_DATABASE_URL").unwrap_or_else(|_| db_url(&admin_db_url, "orders_db"));
    let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET не задана");
    let addr = std::env::var("SERVER_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());

    let pool = PgPool::connect(&admin_db_url)
        .await
        .expect("не удалось подключиться к базе данных админки");
    let auth_pool = PgPool::connect(&auth_db_url).await.expect("не удалось подключиться к auth_db");
    let catalog_pool = PgPool::connect(&catalog_db_url)
        .await
        .expect("не удалось подключиться к catalog_db");
    let orders_pool = PgPool::connect(&orders_db_url)
        .await
        .expect("не удалось подключиться к orders_db");

    modules::init_schema(&pool).await.expect("не удалось создать схему админки");
    ensure_admin(&auth_pool).await.expect("не удалось создать администратора");

    let state = AppState {
        pool,
        auth_pool,
        catalog_pool,
        orders_pool,
        jwt_secret,
    };

    let app = Router::new()
        .route("/api/admin/overview", get(modules::dashboard::overview))
        .route("/api/admin/users", get(modules::users::list_users))
        .route("/api/admin/users", axum::routing::post(modules::users::create_user))
        .route("/api/admin/users/{id}", get(modules::users::get_user))
        .route("/api/admin/users/{id}", axum::routing::put(modules::users::update_user))
        .route("/api/admin/users/{id}", axum::routing::delete(modules::users::delete_user))
        .route("/api/admin/orders", get(modules::orders::list_orders))
        .route("/api/admin/orders/{id}", get(modules::orders::get_order))
        .route("/api/admin/orders/{id}", axum::routing::put(modules::orders::update_order))
        .route("/api/admin/orders/{id}", axum::routing::delete(modules::orders::delete_order))
        .route("/api/feedback", axum::routing::post(modules::feedback::create_feedback))
        .route("/api/admin/feedback", get(modules::feedback::list_feedback))
        .route("/api/admin/feedback/{id}", axum::routing::patch(modules::feedback::resolve_feedback))
        .route("/health", get(|| async { "ok" }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("не удалось занять порт");
    tracing::info!("admin-service слушает http://{addr}");
    axum::serve(listener, app).await.expect("ошибка сервера");
}

fn db_url(base: &str, db: &str) -> String {
    base.rsplit_once('/').map_or_else(
        || format!("postgres://postgres:postgres@127.0.0.1:5433/{db}"),
        |(h, _)| format!("{h}/{db}"),
    )
}

async fn ensure_admin(auth: &PgPool) -> AppResult<()> {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS users (
            id            BIGSERIAL PRIMARY KEY,
            name          TEXT NOT NULL,
            email         TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            role          TEXT NOT NULL CHECK (role IN ('customer', 'master', 'admin')),
            created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(auth)
    .await?;

    let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(ADMIN_EMAIL)
        .fetch_optional(auth)
        .await?;
    if exists.is_none() {
        let password_hash = bcrypt::hash("admin123", 12)
            .map_err(|e| common::AppError::internal(format!("ошибка хеширования: {e}")))?;
        sqlx::query(
            "INSERT INTO users (name, email, password_hash, role) VALUES ($1, $2, $3, 'admin')",
        )
        .bind("Администратор")
        .bind(ADMIN_EMAIL)
        .bind(password_hash)
        .execute(auth)
        .await?;
        tracing::info!("создан администратор {ADMIN_EMAIL}");
    }
    Ok(())
}