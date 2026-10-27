//! Outgoing email. Real mail goes out over SMTP (`SMTP_URL`, for example
//! AWS SES); without it, emails are written to the log so local development
//! works without a mail server.

use std::sync::{Arc, Mutex};

use lettre::{message::Mailbox, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email {
    pub to: String,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    #[error("SMTP_URL is not usable: {0}")]
    Smtp(#[from] lettre::transport::smtp::Error),
    #[error("MAIL_FROM must be an address like \"Acme <no-reply@acme.com>\"")]
    From,
}

#[derive(Clone)]
pub enum Mailer {
    Smtp {
        transport: AsyncSmtpTransport<Tokio1Executor>,
        from: Mailbox,
    },
    Log,
    /// Keeps every email in memory instead of sending it. For tests.
    Capture(Arc<Mutex<Vec<Email>>>),
}

impl Mailer {
    /// Reads `SMTP_URL` (e.g. `smtps://user:pass@email-smtp.us-east-1.amazonaws.com`)
    /// and `MAIL_FROM`. Without `SMTP_URL`, emails only go to the log.
    pub fn from_env() -> Result<Self, MailError> {
        let var = |name| std::env::var(name).ok().filter(|v| !v.is_empty());
        let Some(url) = var("SMTP_URL") else {
            tracing::warn!("SMTP_URL is not set; emails are written to the log, not sent");
            return Ok(Self::Log);
        };
        let from = var("MAIL_FROM")
            .and_then(|from| from.parse().ok())
            .ok_or(MailError::From)?;
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(&url)?.build();
        Ok(Self::Smtp { transport, from })
    }

    pub fn capture() -> (Self, Arc<Mutex<Vec<Email>>>) {
        let sent = Arc::default();
        (Self::Capture(Arc::clone(&sent)), sent)
    }

    /// Sends in the background: nobody should wait on the mail server, and
    /// how long a request takes mustn't give away whether an email went out.
    pub fn send(&self, email: Email) {
        match self {
            Self::Log => tracing::info!(
                to = %email.to,
                subject = %email.subject,
                body = ?email.body,
                "email not sent (SMTP_URL is not set)"
            ),
            Self::Capture(sent) => {
                if let Ok(mut sent) = sent.lock() {
                    sent.push(email);
                }
            }
            Self::Smtp { transport, from } => {
                let (transport, from) = (transport.clone(), from.clone());
                tokio::spawn(async move {
                    let to = email.to.clone();
                    let sent = match build(from, email) {
                        Some(message) => transport.send(message).await.map(drop),
                        None => {
                            tracing::warn!(%to, "can't email this address");
                            return;
                        }
                    };
                    if let Err(err) = sent {
                        tracing::error!(%err, %to, "sending email failed");
                    }
                });
            }
        }
    }
}

fn build(from: Mailbox, email: Email) -> Option<Message> {
    Message::builder()
        .from(from)
        .to(email.to.parse().ok()?)
        .subject(email.subject)
        .body(email.body)
        .ok()
}
