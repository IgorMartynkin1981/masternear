use axum::routing::get;
use axum::Router;
use sqlx::PgPool;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod handlers;
mod state;
mod ws;

use state::AppState;
use ws::WsState;

const DEFAULT_ADDR: &str = "127.0.0.1:8083";

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL не задана");
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
        .expect("не удалось подключиться к базе данных");
    let auth_pool = PgPool::connect(&auth_database_url)
        .await
        .expect("не удалось подключиться к базе данных пользователей");
    handlers::init_schema(&pool)
        .await
        .expect("не удалось создать схему");

    let state = AppState {
        pool,
        auth_pool,
        jwt_secret,
        ws_state: WsState::new(),
        notification_url,
    };

    let app = Router::new()
        .route("/api/chats", get(handlers::list_chats).post(handlers::create_chat))
        .route("/api/chats/{id}/messages", get(handlers::list_messages).post(handlers::send_message))
        .route("/ws", get(ws::ws_handler))
        .route("/health", get(|| async { "ok" }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("не удалось занять порт");
    tracing::info!("chat-service слушает http://{addr}");
    axum::serve(listener, app).await.expect("ошибка сервера");
}