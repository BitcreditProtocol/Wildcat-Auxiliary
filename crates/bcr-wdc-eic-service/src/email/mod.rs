use bcr_common::core::NodeId;
use bcr_wdc_shared::{TStamp, email::mailjet::EmailMessage, now};
use bitcoin::secp256k1::schnorr::Signature;
use email_address::EmailAddress;
use serde::Serialize;
use time::Duration;
use tinytemplate::TinyTemplate;

mod template;

#[derive(Debug, Clone)]
pub struct Registration {
    pub node_id: NodeId,
    pub company_node_id: Option<NodeId>,
    pub email: EmailAddress,
    pub mint_signature: Signature,
    pub created_at: TStamp,
}

#[derive(Debug, Clone)]
pub struct Confirmation {
    pub node_id: NodeId,
    pub company_node_id: Option<NodeId>,
    pub email: EmailAddress,
    pub confirmation_code: String,
    pub wrong_entries: usize,
    pub created_at: TStamp,
}

impl Confirmation {
    /// Maximum age of an email confirmation
    const EMAIL_CONFIRMATION_EXPIRY: Duration = Duration::days(1);

    /// Maximum number of wrong confirmation code entries, before it gets deleted to avoid brute forcing other peoples addresses
    const MAX_WRONG_TRIES: usize = 5;
    pub fn is_expired(&self) -> bool {
        now()
            > self
                .created_at
                .checked_add(Self::EMAIL_CONFIRMATION_EXPIRY)
                .expect("valid addition")
    }

    pub fn has_too_many_retries(&self) -> bool {
        self.wrong_entries >= Self::MAX_WRONG_TRIES
    }

    pub fn matches_confirmation_code(&self, code: &str) -> bool {
        self.confirmation_code == code
    }
}

#[derive(Serialize)]
struct ConfirmationContext {
    pub logo_link: url::Url,
    pub confirmation_code: String,
}

pub fn build_confirmation_message(
    logo_url: &url::Url,
    from: &EmailAddress,
    to: &EmailAddress,
    confirmation_code: &str,
) -> Result<EmailMessage, anyhow::Error> {
    let mut tt = TinyTemplate::new();
    tt.add_template("mail", template::MAIL_CONFIRMATION_TEMPLATE)?;

    let context = ConfirmationContext {
        logo_link: logo_url.to_owned(),
        confirmation_code: confirmation_code.to_string(),
    };

    let rendered = tt.render("mail", &context)?;

    Ok(EmailMessage {
        from: from.to_owned(),
        to: to.to_owned(),
        subject: "Please confirm your E-Mail".to_owned(),
        body: rendered,
    })
}
