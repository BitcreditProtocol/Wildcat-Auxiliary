use bcr_wdc_shared::email::mailjet::EmailMessage;
use email_address::EmailAddress;
use serde::Serialize;
use tinytemplate::TinyTemplate;

mod template;

#[derive(Serialize)]
struct NotificationContext {
    pub logo_link: url::Url,
    pub title: String,
    pub link: url::Url,
    pub preferences_link: url::Url,
}

pub fn build_email_notification_message(
    logo_url: &url::Url,
    from: &EmailAddress,
    to: &EmailAddress,
    title: &str,
    link: &url::Url,
    preferences_link: &url::Url,
) -> Result<EmailMessage, anyhow::Error> {
    let mut tt = TinyTemplate::new();
    tt.add_template("mail", template::NOTIFICATION_MAIL_TEMPLATE)?;

    let context = NotificationContext {
        logo_link: logo_url.to_owned(),
        title: title.to_owned(),
        link: link.to_owned(),
        preferences_link: preferences_link.to_owned(),
    };

    let rendered = tt.render("mail", &context)?;

    Ok(EmailMessage {
        from: from.to_owned(),
        to: to.to_owned(),
        subject: title.to_owned(),
        body: rendered,
    })
}
