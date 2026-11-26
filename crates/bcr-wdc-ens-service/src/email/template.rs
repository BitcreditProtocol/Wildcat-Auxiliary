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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MailField {
    pub label: String,
    pub value: String,
    pub mono: bool,
}

const KNOWN_FIELDS: &[(&str, &str, bool)] = &[
    ("drawee", "Drawee", false),
    ("drawer", "Drawer", false),
    ("payee", "Payee", false),
    ("amount", "Amount", false),
    ("maturity_date", "Maturity date", false),
    ("bill_id", "Bill ID", true),
    ("quote_id", "Quote ID", true),
];

fn humanize_key(key: &str) -> String {
    let spaced = key.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => spaced,
    }
}

pub fn map_to_fields(mut map: HashMap<String, String>) -> Vec<MailField> {
    let mut fields = Vec::with_capacity(map.len());
    for (key, label, mono) in KNOWN_FIELDS {
        if let Some(value) = map.remove(*key) {
            fields.push(MailField {
                label: (*label).to_owned(),
                value,
                mono: *mono,
            });
        }
    }
    let mut rest: Vec<(String, String)> = map.into_iter().collect();
    rest.sort_by(|a, b| a.0.cmp(&b.0));
    fields.extend(rest.into_iter().map(|(key, value)| MailField {
        label: humanize_key(&key),
        value,
        mono: false,
    }));
    fields
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MailAction {
    pub label: String,
    pub link: url::Url,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdminNotificationEmail {
    pub logo_link: url::Url,
    pub title: String,
    pub fields: Vec<MailField>,
    pub action: Option<MailAction>,
}

impl AdminNotificationEmail {
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
                    <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="650" class="container" style="width:650px; max-width:650px; background:#fefbf1;">
                        <tr>
                            <td class="px" style="padding:18px 24px;">
                                <img src="{logo_link}"
                                     alt="Bitcredit" width="120" height="24"
                                     style="display:block; border:0; outline:none; text-decoration:none; height:auto;">
                            </td>
                        </tr>
                        <tr>
                            <td class="px" style="padding:15px 24px 20px 24px; font-family:Geist, system-ui, sans-serif; color:#111111;">
                                <p style="margin:0 0 4px 0; font-size:12px; line-height:16px; font-weight:500; letter-spacing:.06em; text-transform:uppercase; color:#8d8579;">
                                    Wildcat admin
                                </p>
                                <h1 style="margin:0; font-size:24px; line-height:36px; font-weight:500;">
                                    {title}
                                </h1>
                            </td>
                        </tr>
                    </table>
                    <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="650" class="container" style="width:650px; max-width:650px; background:#ffffff;">
                        <tr>
                            <td class="px" style="padding:28px 24px 8px 24px;">
                                <table role="presentation" cellpadding="0" cellspacing="0" border="0" width="100%"
                                       style="font-family:Geist, system-ui, sans-serif; font-size:14px; line-height:20px; color:#111111; border:1px solid #efefef; border-radius:12px; border-collapse:separate;">
                                    {{ for field in fields }}
                                    <tr>
                                        <td style="padding:12px 16px; {{ if not @first }}border-top:1px solid #efefef; {{ endif }}color:#8d8579; white-space:nowrap; vertical-align:top; width:140px;">{field.label}</td>
                                        <td style="padding:12px 16px; {{ if not @first }}border-top:1px solid #efefef; {{ endif }}font-weight:500; word-break:break-all;{{ if field.mono }} font-family:ui-monospace, SFMono-Regular, Menlo, monospace; font-size:13px;{{ endif }}">{field.value}</td>
                                    </tr>
                                    {{ endfor }}
                                </table>
                            </td>
                        </tr>
                        {{ if action }}
                        <tr>
                            <td align="center" style="padding:32px 24px 12px 24px;">
                                <a href="{action.link}"
                                   style="background:#2b2118; color:#ffffff; text-decoration:none; display:inline-block;
                                          font-family:Geist, system-ui, sans-serif; font-size:14px; font-weight: 500;
                                          padding:12px 24px; border-radius:.5rem;">
                                    {action.label}
                                </a>
                            </td>
                        </tr>
                        {{ endif }}
                        <tr><td style="height:32px; line-height:32px;">&nbsp;</td></tr>
                        <tr>
                            <td class="px" style="padding:0 24px;">
                                <div style="border-top:1px solid #efefef; height:1px; line-height:1px;">&nbsp;</div>
                            </td>
                        </tr>
                        <tr>
                            <td align="center" class="px" style="padding:16px 24px 28px 24px; font-family:Geist, system-ui, sans-serif; font-size:13px; line-height:20px; color:#8d8579;">
                                You receive this email as a configured Wildcat admin recipient.
                                {{ if action }}<br><a href="{action.link}" style="color:#333333; text-decoration:underline;">Open in the dashboard</a>{{ endif }}
                            </td>
                        </tr>
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
