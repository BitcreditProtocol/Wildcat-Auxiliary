use bcr_wdc_shared::email::mailjet::EmailMessage;
use serde::Serialize;
use tinytemplate::TinyTemplate;

mod template;

#[derive(Serialize)]
struct EmailConfirmationContext {
    pub logo_link: url::Url,
    pub confirmation_code: String,
}

pub fn build_email_confirmation_message(
    logo_url: &url::Url,
    from: &str,
    to: &str,
    confirmation_code: &str,
) -> Result<EmailMessage, anyhow::Error> {
    let mut tt = TinyTemplate::new();
    tt.add_template("mail", template::MAIL_CONFIRMATION_TEMPLATE)?;

    let context = EmailConfirmationContext {
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
