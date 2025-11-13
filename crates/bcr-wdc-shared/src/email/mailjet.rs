use anyhow::anyhow;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::error;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct MailJetConfig {
    pub sender: String,
    pub api_key: String,
    pub api_secret_key: String,
    pub url: url::Url,
    pub logo_url: url::Url,
}

pub struct MailjetClient {
    config: MailJetConfig,
    client: reqwest::Client,
}

impl MailjetClient {
    pub fn new(config: &MailJetConfig) -> Self {
        let client = reqwest::Client::new();
        Self {
            config: config.to_owned(),
            client,
        }
    }
}

#[async_trait]
pub trait EmailClient: Send + Sync {
    async fn send(&self, msg: EmailMessage) -> Result<(), anyhow::Error>;
}

/// A simple email message. We can add more features (like html, multi recipient, etc.) later.
#[derive(Debug, Clone)]
pub struct EmailMessage {
    pub from: String,
    pub to: String,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize)]
struct MailjetReq {
    #[serde(rename = "Messages")]
    pub messages: Vec<MailjetMessage>,
}

#[derive(Debug, Clone, Deserialize)]
struct MailjetResp {
    #[serde(rename = "Messages")]
    pub messages: Vec<MailjetRespMessage>,
}

#[derive(Debug, Clone, Serialize)]
struct MailjetMessage {
    #[serde(rename = "From")]
    pub from: MailjetFrom,
    #[serde(rename = "To")]
    pub to: Vec<MailjetTo>,
    #[serde(rename = "Subject")]
    pub subject: String,
    #[serde(rename = "HTMLPart")]
    pub html_part: String,
}

impl From<EmailMessage> for MailjetMessage {
    fn from(value: EmailMessage) -> Self {
        Self {
            from: MailjetFrom { email: value.from },
            to: vec![MailjetTo { email: value.to }],
            subject: value.subject,
            html_part: value.body,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct MailjetFrom {
    #[serde(rename = "Email")]
    pub email: String,
}

#[derive(Debug, Clone, Serialize)]
struct MailjetTo {
    #[serde(rename = "Email")]
    pub email: String,
}

#[derive(Debug, Clone, Deserialize)]
struct MailjetRespMessage {
    #[serde(rename = "Status")]
    pub status: String,
}

#[async_trait]
impl EmailClient for MailjetClient {
    async fn send(&self, msg: EmailMessage) -> Result<(), anyhow::Error> {
        let mailjet_msg = MailjetReq {
            messages: vec![MailjetMessage::from(msg)],
        };

        let url = self.config.url.join("/v3.1/send").expect("mailjet path");
        let request = self.client.post(url).json(&mailjet_msg).basic_auth(
            self.config.api_key.clone(),
            Some(self.config.api_secret_key.clone()),
        );
        let res = request.send().await.map_err(|e| {
            error!("Failed to send email: {e}");
            anyhow!("Failed to send email")
        })?;

        let resp: MailjetResp = res.json().await.map_err(|e| {
            error!("Failed to parse email response: {e}");
            anyhow!("Failed to parse email response")
        })?;

        match resp.messages.first() {
            Some(msg) => {
                if msg.status != "success" {
                    error!("Invalid email sending response: {}", &msg.status);
                    Err(anyhow!("Invalid email sending response: {}", &msg.status))
                } else {
                    Ok(())
                }
            }
            None => {
                error!("Invalid email response - got no status");
                Err(anyhow!("Invalid email response - got no status"))
            }
        }
    }
}
