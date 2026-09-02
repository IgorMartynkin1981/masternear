use serde::Serialize;

use crate::{AppError, AppResult};

#[derive(Serialize)]
struct EmailPayload {
    to: String,
    subject: String,
    html: String,
}

/// Отправляет письмо через notification-service. Если URL не задан — просто пропускает
/// (уведомления опциональны) и логирует.
pub async fn send_email(
    notification_url: &str,
    to: &str,
    subject: &str,
    html: &str,
) -> AppResult<()> {
    if notification_url.is_empty() {
        return Ok(());
    }

    let url = format!("{notification_url}/internal/email");
    let payload = EmailPayload {
        to: to.to_string(),
        subject: subject.to_string(),
        html: html.to_string(),
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| AppError::internal(format!("не удалось отправить письмо: {e}")))?;

    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "notification-service вернул ошибку");
    }
    Ok(())
}

/// Короткий helper для html-уведомления о новом сообщении.
pub fn message_email_html(sender_name: &str, text: &str) -> String {
    let safe = text.replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<h2>Новое сообщение в MasterNear</h2>\
         <p>Вам написал <b>{}</b>:</p>\
         <p style=\"padding:12px;background:#f5f5f5;border-radius:6px;\">{}</p>\
         <p><a href=\"https://masternear.example\">Открыть чат</a></p>",
        sender_name.replace('<', "&lt;"),
        safe
    )
}
