use lettre::message::Mailbox;
use lettre::transport::smtp::AsyncSmtpTransport;
use lettre::Tokio1Executor;

#[derive(Clone)]
pub struct AppState {
    pub mailer: AsyncSmtpTransport<Tokio1Executor>,
    pub from: Mailbox,
}