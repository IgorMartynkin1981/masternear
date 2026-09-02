use axum::routing::get;
use axum::Router;
use sqlx::PgPool;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod geocode;
mod handlers;
mod photos;
mod state;

use state::AppState;

const DEFAULT_ADDR: &str = "127.0.0.1:8082";

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL не задана");
    let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET не задана");
    let addr = std::env::var("SERVER_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    let upload_dir = std::env::var("UPLOAD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("uploads"));

    let pool = PgPool::connect(&database_url)
        .await
        .expect("не удалось подключиться к базе данных");
    handlers::init_schema(&pool)
        .await
        .expect("не удалось создать схему");

    let state = AppState {
        pool,
        jwt_secret,
        upload_dir: upload_dir.clone(),
    };

    let uploads_service = tower_http::services::ServeDir::new(&upload_dir);

    let app = Router::new()
        .nest_service("/uploads", uploads_service)
        .route("/api/categories", get(handlers::list_categories))
        .route("/api/masters", get(handlers::list_masters))
        .route("/api/masters/me", get(handlers::my_profile).post(handlers::upsert_profile))
        .route("/api/masters/me/prices", axum::routing::put(handlers::set_price))
        .route("/api/masters/me/photos", axum::routing::get(photos::my_photos).post(photos::upload_photo))
        .route("/api/masters/me/photos/{id}", axum::routing::delete(photos::delete_photo))
        .route("/api/masters/{user_id}/rating", axum::routing::post(handlers::rate_master))
        .route("/api/geocode", axum::routing::post(handlers::geocode_place))
        .route("/health", get(|| async { "ok" }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("не удалось занять порт");
    tracing::info!("catalog-service слушает http://{addr}");
    axum::serve(listener, app).await.expect("ошибка сервера");
}