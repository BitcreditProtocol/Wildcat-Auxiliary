use bcr_common::wire::notification::NotificationRequest;
use bcr_wdc_shared::email::mailjet::EmailMessage;
use email_address::EmailAddress;
use tinytemplate::TinyTemplate;

mod template;

use template::{NewEbillQuoteNotificationEmail, NotificationEmail, map_to_fields};

pub fn build_email_notification_message(
    logo_url: &url::Url,
    from: &EmailAddress,
    to: &EmailAddress,
    title: &str,
    link: &url::Url,
    preferences_link: &url::Url,
) -> Result<EmailMessage, anyhow::Error> {
    let mut tt = TinyTemplate::new();
    tt.add_template("mail", NotificationEmail::template())?;

    let context = NotificationEmail {
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

const NOT_AVAILABLE: &str = "n/a";

pub fn admin_notification_subject(notification: &NotificationRequest) -> &'static str {
    match notification {
        NotificationRequest::NewEbillQuote { .. } => "New e-bill quote request",
    }
}

pub fn build_admin_notification_body(
    notification: NotificationRequest,
    wdc_dashboard_url: &url::Url,
) -> Result<String, anyhow::Error> {
    let mut tt = TinyTemplate::new();
    let na_url = url::Url::parse("https://example.com").unwrap();
    let body = match notification {
        NotificationRequest::NewEbillQuote { mut fields } => {
            tt.add_template("mail", NewEbillQuoteNotificationEmail::template())?;
            let drawee = fields
                .remove("drawee")
                .unwrap_or(String::from(NOT_AVAILABLE));
            let amount = fields
                .remove("amount")
                .unwrap_or(String::from(NOT_AVAILABLE));
            let maturity_date = fields
                .remove("maturity_date")
                .unwrap_or(String::from(NOT_AVAILABLE));
            let quote_link = fields
                .get("quote_id")
                .map(|quote_id| dashboard_quote_link(wdc_dashboard_url, quote_id))
                .transpose()?
                .unwrap_or(na_url);
            let context = NewEbillQuoteNotificationEmail {
                drawee,
                amount,
                maturity_date,
                quote_link,
                fields: map_to_fields(fields),
            };
            tt.render("mail", &context)?
        }
    };
    Ok(body)
}

/// link to the quote page of the wildcat dashboard: `<wdc_dashboard_url>/quotes/<quote_id>`
fn dashboard_quote_link(
    wdc_dashboard_url: &url::Url,
    quote_id: &str,
) -> Result<url::Url, anyhow::Error> {
    let mut link = wdc_dashboard_url.clone();
    link.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("invalid wdc dashboard url {wdc_dashboard_url}"))?
        .pop_if_empty()
        .push("quotes")
        .push(quote_id);
    Ok(link)
}
