use bcr_common::wire::notification::NotificationRequest;
use bcr_wdc_shared::email::mailjet::EmailMessage;
use email_address::EmailAddress;
use tinytemplate::TinyTemplate;

mod template;

use template::{AdminNotificationEmail, MailAction, NotificationEmail, map_to_fields};

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
        NotificationRequest::EbillMatured { .. } => "E-bill matured",
        NotificationRequest::EbillRepaid { .. } => "E-bill repaid",
    }
}

pub fn build_admin_notification_body(
    notification: NotificationRequest,
    logo_url: &url::Url,
    wdc_dashboard_url: &url::Url,
) -> Result<String, anyhow::Error> {
    let title = admin_notification_subject(&notification).to_owned();
    let (fields, action) = match notification {
        NotificationRequest::NewEbillQuote { mut fields } => {
            for key in ["drawee", "amount", "maturity_date"] {
                fields
                    .entry(key.to_owned())
                    .or_insert_with(|| NOT_AVAILABLE.to_owned());
            }
            let action = fields
                .get("quote_id")
                .map(|quote_id| dashboard_link(wdc_dashboard_url, "quotes", quote_id))
                .transpose()?
                .map(|link| MailAction {
                    label: String::from("Go to quote"),
                    link,
                });
            (fields, action)
        }
        NotificationRequest::EbillMatured { fields }
        | NotificationRequest::EbillRepaid { fields } => {
            let action = fields
                .get("bill_id")
                .map(|bill_id| dashboard_link(wdc_dashboard_url, "bills", bill_id))
                .transpose()?
                .map(|link| MailAction {
                    label: String::from("Go to e-bill"),
                    link,
                });
            (fields, action)
        }
    };

    let mut tt = TinyTemplate::new();
    tt.add_template("mail", AdminNotificationEmail::template())?;
    let context = AdminNotificationEmail {
        logo_link: logo_url.to_owned(),
        title,
        fields: map_to_fields(fields),
        action,
    };
    Ok(tt.render("mail", &context)?)
}

fn dashboard_link(
    wdc_dashboard_url: &url::Url,
    section: &str,
    id: &str,
) -> Result<url::Url, anyhow::Error> {
    let mut link = wdc_dashboard_url.clone();
    link.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("invalid wdc dashboard url {wdc_dashboard_url}"))?
        .pop_if_empty()
        .push(section)
        .push(id);
    Ok(link)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn url(s: &str) -> url::Url {
        url::Url::parse(s).unwrap()
    }

    fn bill_fields() -> HashMap<String, String> {
        HashMap::from([
            ("bill_id".to_owned(), "bitcrmAoSjEdQG58".to_owned()),
            ("drawee".to_owned(), "Ebills prod".to_owned()),
            ("drawer".to_owned(), "Ebills drawer".to_owned()),
            ("amount".to_owned(), "2025 sat".to_owned()),
            ("maturity_date".to_owned(), "2026-11-25".to_owned()),
        ])
    }

    fn render(notification: NotificationRequest) -> String {
        build_admin_notification_body(
            notification,
            &url("https://example.com/logo.png"),
            &url("https://dashboard.example.com/"),
        )
        .unwrap()
    }

    #[test]
    fn dashboard_link_handles_trailing_slash() {
        let with = dashboard_link(&url("https://d.example.com/app/"), "bills", "b1").unwrap();
        let without = dashboard_link(&url("https://d.example.com/app"), "bills", "b1").unwrap();
        assert_eq!(with.as_str(), "https://d.example.com/app/bills/b1");
        assert_eq!(with, without);
    }

    #[test]
    fn repaid_email_links_to_bill() {
        let body = render(NotificationRequest::EbillRepaid {
            fields: bill_fields(),
        });
        assert!(body.contains("E-bill repaid"));
        assert!(body.contains("https://dashboard.example.com/bills/bitcrmAoSjEdQG58"));
        assert!(body.contains("Go to e-bill"));
        assert!(body.contains("Maturity date"));
        assert!(body.contains("https://example.com/logo.png"));
        let drawee = body.find("Drawee").unwrap();
        let amount = body.find("Amount").unwrap();
        let bill_id = body.find("Bill ID").unwrap();
        assert!(drawee < amount && amount < bill_id);
    }

    #[test]
    fn matured_email_links_to_bill() {
        let body = render(NotificationRequest::EbillMatured {
            fields: bill_fields(),
        });
        assert!(body.contains("E-bill matured"));
        assert!(body.contains("https://dashboard.example.com/bills/bitcrmAoSjEdQG58"));
    }

    #[test]
    fn email_without_id_has_no_button() {
        let mut fields = bill_fields();
        fields.remove("bill_id");
        let body = render(NotificationRequest::EbillMatured { fields });
        assert!(!body.contains("Go to e-bill"));
        assert!(!body.contains("/bills/"));
    }

    #[test]
    fn quote_email_links_to_quote_and_fills_missing_fields() {
        let fields = HashMap::from([
            ("quote_id".to_owned(), "q-123".to_owned()),
            ("some_extra".to_owned(), "x".to_owned()),
        ]);
        let body = render(NotificationRequest::NewEbillQuote { fields });
        assert!(body.contains("https://dashboard.example.com/quotes/q-123"));
        assert!(body.contains("Go to quote"));
        assert!(body.contains("Some extra"));
        assert!(body.contains(NOT_AVAILABLE));
    }

    #[test]
    fn values_are_html_escaped() {
        let mut fields = bill_fields();
        fields.insert("drawee".to_owned(), "<script>".to_owned());
        let body = render(NotificationRequest::EbillRepaid { fields });
        assert!(!body.contains("<script>"));
    }
}
