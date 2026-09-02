use axum::routing::get;
use axum::{Router, routing::post};
use sqlx::PgPool;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod state;

mod handlers;

mod currencies;

mod profile;

use state::AppState;

const DEFAULT_ADDR: &str = "127.0.0.1:8081";

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL не задана");
    let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET не задана");
    let addr = std::env::var("SERVER_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());

    let pool = PgPool::connect(&database_url)
        .await
        .expect("не удалось подключиться к базе данных");
    handlers::init_schema(&pool)
        .await
        .expect("не удалось создать схему");

    let state = AppState { pool, jwt_secret };

    let app = Router::new()
        .route("/api/auth/register", post(handlers::register))
        .route("/api/auth/login", post(handlers::login))
        .route("/api/auth/me", get(handlers::me))
        .route("/api/auth/settings", get(handlers::settings).put(handlers::update_settings))
        .route("/api/auth/profile", get(profile::profile).put(profile::update_profile))
        .route("/api/auth/profile/password", axum::routing::post(profile::change_password))
        .route("/health", get(|| async { "ok" }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("не удалось занять порт");
    tracing::info!("auth-service слушает http://{addr}");
    axum::serve(listener, app).await.expect("ошибка сервера");
}