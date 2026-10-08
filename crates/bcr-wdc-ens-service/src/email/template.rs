use std::collections::HashMap;

/// Notification email sent to a user for an eBill event.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NotificationEmail {
    pub logo_link: url::Url,
    pub title: String,
    pub link: url::Url,
    pub preferences_link: url::Url,
}

impl NotificationEmail {
    pub fn template() -> &'static str {
        const TEMPLATE: &str = r#"
<!doctype html>
<html lang="en">
    <head>
        <meta http-equiv="Content-Type" content="text/html; charset=UTF-8">
        <meta name="viewport" content="width=device-width, initial-scale=1.0">
        <title>{title}</title>
    </head>
    <body style="margin:0; padding:0; background:#ffffff;">
        <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="100%">
            <tr>
                <td align="center">
                    <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="650" class="container" style="width:650px; max-width:650px;">
                        <tr>
                            <td class="px" style="padding:18px 24px; background:#fefbf1;">
                                <img src="{logo_link}"
                                     alt="Bitcredit" width="120" height="24"
                                     style="display:block; border:0; outline:none; text-decoration:none; height:auto;">
                            </td>
                        </tr>
                    </table>
                    <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="650" class="container" style="width:650px; max-width:650px; background:#ffffff;">
                        <tr style="background: #fefbf1;">
                            <td class="px" style="padding:15px 24px 8px 24px; font-family:Geist, system-ui, sans-serif; color:#111111;">
                                <h1 style="margin:0; font-size:24px; line-height:36px; font-weight:500;">
                                    {title}
                                </h1>
                            </td>
                        </tr>
                        <tr>
                            <td align="center" style="padding:60px 24px 36px 24px;">
                                <a href="{link}"
                                   style="background:#2b2118; color:#ffffff; text-decoration:none; display:inline-block;
                                          font-family:Geist, system-ui, sans-serif; font-size:14px; font-weight: 500;
                                          padding:12px 24px; border-radius:.5rem;">
                                    Go to e-bill
                                </a>
                            </td>
                        </tr>
                        <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="650" class="container" style="width:650px; max-width:650px;">
                            <tr><td style="height:44px; line-height:44px;">&nbsp;</td></tr>
                        </table>
                        <hr style="border: 1px solid #efefef; width: 600px;" />
                        <tr>
                            <td align="center" class="px" style="padding:16px 24px 28px 24px; font-family:Geist, system-ui, sans-serif; font-size:13px; line-height:20px; color:#333333;">
                                <a href="{preferences_link}" style="color:#333333; text-decoration:none;">Manage notification settings</a>
                                &nbsp;&nbsp;&nbsp;&nbsp;
                                <a href="{link}" style="color:#333333; text-decoration:none;">View in the browser</a>
                            </td>
                        </tr>
                    </table>
                    <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="650" class="container" style="width:650px; max-width:650px;">
                        <tr><td style="height:24px; line-height:24px;">&nbsp;</td></tr>
                    </table>
                </td>
            </tr>
        </table>
    </body>
</html>
"#;
        TEMPLATE
    }
}

/// A generic key/value entry rendered in a notification email.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MailField {
    pub key: String,
    pub value: String,
}

impl From<(String, String)> for MailField {
    fn from((key, value): (String, String)) -> Self {
        MailField { key, value }
    }
}

pub fn map_to_fields(map: HashMap<String, String>) -> Vec<MailField> {
    let fields: Vec<MailField> = map.into_iter().map(MailField::from).collect();
    fields
}

/// Admin notification email for a new eBill quote request.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NewEbillQuoteNotificationEmail {
    pub drawee: String,
    pub amount: String,
    pub maturity_date: String,
    pub quote_link: url::Url,
    pub fields: Vec<MailField>,
}

impl NewEbillQuoteNotificationEmail {
    pub fn template() -> &'static str {
        const TEMPLATE: &str = r#"
<!doctype html>
<html lang="en">
    <head>
        <meta http-equiv="Content-Type" content="text/html; charset=UTF-8">
        <meta name="viewport" content="width=device-width, initial-scale=1.0">
        <title>New e-bill quote request</title>
    </head>
    <body style="margin:0; padding:0; background:#ffffff;">
        <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="100%">
            <tr>
                <td align="center">
                    <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="650" class="container" style="width:650px; max-width:650px; background:#ffffff;">
                        <tr style="background: #fefbf1;">
                            <td class="px" style="padding:15px 24px 8px 24px; font-family:Geist, system-ui, sans-serif; color:#111111;">
                                <h1 style="margin:0; font-size:24px; line-height:36px; font-weight:500;">
                                    New e-bill quote request
                                </h1>
                            </td>
                        </tr>
                        <tr>
                            <td class="px" style="padding:24px 24px 36px 24px;">
                                <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="100%"
                                       style="font-family:Geist, system-ui, sans-serif; font-size:14px; line-height:20px; color:#111111;">
                                    <tr>
                                        <td style="padding:6px 12px 6px 0; border-bottom:1px solid #efefef; color:#333333; font-weight:700; white-space:nowrap;">Drawee</td>
                                        <td style="padding:6px 0; border-bottom:1px solid #efefef; font-weight:700; word-break:break-all;">{drawee}</td>
                                    </tr>
                                    <tr>
                                        <td style="padding:6px 12px 6px 0; border-bottom:1px solid #efefef; color:#333333; font-weight:700; white-space:nowrap;">Amount</td>
                                        <td style="padding:6px 0; border-bottom:1px solid #efefef; font-weight:700; word-break:break-all;">{amount}</td>
                                    </tr>
                                    <tr>
                                        <td style="padding:6px 12px 6px 0; border-bottom:1px solid #efefef; color:#333333; font-weight:700; white-space:nowrap;">Maturity date</td>
                                        <td style="padding:6px 0; border-bottom:1px solid #efefef; font-weight:700; word-break:break-all;">{maturity_date}</td>
                                    </tr>
                                    {{ for field in fields }}
                                    <tr>
                                        <td style="padding:6px 12px 6px 0; border-bottom:1px solid #efefef; color:#333333; font-weight:500; white-space:nowrap;">{field.key}</td>
                                        <td style="padding:6px 0; border-bottom:1px solid #efefef; word-break:break-all;">{field.value}</td>
                                    </tr>
                                    {{ endfor }}
                                </table>
                            </td>
                        </tr>
                        {{ if quote_link }}
                        <tr>
                            <td align="center" style="padding:0px 24px 36px 24px;">
                                <a href="{quote_link}"
                                   style="background:#2b2118; color:#ffffff; text-decoration:none; display:inline-block;
                                          font-family:Geist, system-ui, sans-serif; font-size:14px; font-weight: 500;
                                          padding:12px 24px; border-radius:.5rem;">
                                    Go to quote
                                </a>
                            </td>
                        </tr>
                        {{ endif }}
                    </table>
                    <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="650" class="container" style="width:650px; max-width:650px;">
                        <tr><td style="height:24px; line-height:24px;">&nbsp;</td></tr>
                    </table>
                </td>
            </tr>
        </table>
    </body>
</html>
"#;
        TEMPLATE
    }
}
