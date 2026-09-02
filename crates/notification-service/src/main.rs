use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use lettre::message::header::ContentType;
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde::Deserialize;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod state;

use state::AppState;

const DEFAULT_ADDR: &str = "127.0.0.1:8086";

#[derive(Deserialize)]
pub struct SendEmailReq {
    pub to: String,
    pub subject: String,
    pub html: String,
}

async fn send_email(
    State(state): State<AppState>,
    Json(req): Json<SendEmailReq>,
) -> StatusCode {
    let Ok(recipient) = req.to.parse::<Mailbox>() else {
        return StatusCode::BAD_REQUEST;
    };
    let from = state.from.clone();

    let email = match Message::builder()
        .from(from)
        .to(recipient)
        .subject(req.subject)
        .header(ContentType::TEXT_HTML)
        .body(req.html)
    {
        Ok(m) => m,
        Err(_) => return StatusCode::BAD_REQUEST,
    };

    match state.mailer.send(email).await {
        Ok(_) => StatusCode::OK,
        Err(e) => {
            tracing::error!(error = %e, "не удалось отправить письмо");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let addr = std::env::var("SERVER_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    let smtp_host = std::env::var("SMTP_HOST").expect("SMTP_HOST не задан");
    let smtp_port = std::env::var("SMTP_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(587);
    let smtp_user = std::env::var("SMTP_USER").unwrap_or_default();
    let smtp_password = std::env::var("SMTP_PASSWORD").unwrap_or_default();
    let from_addr: lettre::Address = std::env::var("MAIL_FROM")
        .expect("MAIL_FROM не задан")
        .parse()
        .expect("MAIL_FROM должен быть валидным email");
    let from_name = std::env::var("MAIL_FROM_NAME").unwrap_or_else(|_| "MasterNear".to_string());
    let from_mailbox = Mailbox::new(Some(from_name), from_addr);

    let creds = Credentials::new(smtp_user.clone(), smtp_password.clone());
    let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp_host)
        .ok()
        .map(|b| {
            b.port(smtp_port)
                .credentials(creds.clone())
                .build::<Tokio1Executor>()
        })
        .unwrap_or_else(|| {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp_host)
                .port(smtp_port)
                .credentials(creds)
                .build()
        });

    let state = AppState {
        mailer,
        from: from_mailbox,
    };

    let app = Router::new()
        .route("/internal/email", post(send_email))
        .route("/health", get(|| async { "ok" }));

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("не удалось занять порт");
    tracing::info!("notification-service слушает http://{addr}");
    axum::serve(listener, app.with_state(state)).await.expect("ошибка сервера");
}
