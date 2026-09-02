use axum::routing::get;
use axum::Router;
use sqlx::PgPool;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod handlers;
mod state;

use state::AppState;

const DEFAULT_ADDR: &str = "127.0.0.1:8084";

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL не задана");
    let catalog_database_url =
        std::env::var("CATALOG_DATABASE_URL").unwrap_or_else(|_| catalog_url(&database_url));
    let auth_database_url = std::env::var("AUTH_DATABASE_URL").unwrap_or_else(|_| {
        database_url
            .rsplit_once('/')
            .map_or_else(|| database_url.clone(), |(h, _)| format!("{h}/auth_db"))
    });
    let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET не задана");
    let notification_url =
        std::env::var("NOTIFICATION_SERVICE_URL").unwrap_or_else(|_| String::new());
    let addr = std::env::var("SERVER_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());

    let pool = PgPool::connect(&database_url)
        .await
        .expect("не удалось подключиться к базе данных заказов");
    let catalog_pool = PgPool::connect(&catalog_database_url)
        .await
        .expect("не удалось подключиться к каталогу мастеров");
    let auth_pool = PgPool::connect(&auth_database_url)
        .await
        .expect("не удалось подключиться к базе данных пользователей");

    handlers::init_schema(&pool)
        .await
        .expect("не удалось создать схему");

    let state = AppState {
        pool,
        catalog_pool,
        auth_pool,
        jwt_secret,
        notification_url,
    };

    let app = Router::new()
        .route("/api/orders", get(handlers::list_orders).post(handlers::create_order))
        .route("/api/orders/{id}", get(handlers::order_detail))
        .route("/api/orders/{id}/offers", axum::routing::post(handlers::place_offer))
        .route("/api/orders/{id}/select", axum::routing::post(handlers::select_offer))
        .route("/health", get(|| async { "ok" }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("не удалось занять порт");
    tracing::info!("order-service слушает http://{addr}");
    axum::serve(listener, app).await.expect("ошибка сервера");
}

fn catalog_url(database_url: &str) -> String {
    database_url.rsplit_once('/').map_or_else(
        || "postgres://postgres:postgres@127.0.0.1:5433/catalog_db".to_string(),
        |(base, _)| format!("{base}/catalog_db"),
    )
}