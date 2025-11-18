use axum::{
    http::StatusCode,
    response::{Html, IntoResponse},
};
use bcr_common::core::NodeId;
use email_address::EmailAddress;
use serde::{Deserialize, Serialize};
use tinytemplate::TinyTemplate;
use tracing::error;
use uuid::Uuid;

use crate::email_preferences::PreferencesFlags;

pub const TEMPLATE: &str = r#"
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <style>
    :root \{
      --bg: #fefbf1;
      --card: #ffffff;
      --header: #faf5e8;
      --text: #111111;
      --muted: #333333;
      --divider: #e8e6e1;
      --primary: #2b2118;
    }

    * \{
        box-sizing: border-box
    }

    body \{
        margin: 0;
        background: var(--bg);
        color: var(--text);
        font: 16px/1.5 system-ui,Geist, sans-serif;
    }

    .container \{
        max-width: 700px;
        margin: 0 auto;
    }

    .header \{
        background: var(--header);
        padding: 18px 24px;
    }

    .logo \{ 
        display: block;
        height: 24px;
        width: auto;
    }

    .card \{
        background: var(--card);
    }

    .section \{
        padding: 12px 24px;
    }

    h1 \{
        margin: 0;
        font-size: 28px;
        line-height: 1.3;
        font-weight: 700
    }

    .cta-wrap \{
        display: flex;
        justify-content: center;
        padding: 28px 24px 36px;
    }

    .btn \{
        display: inline-block;
        background: var(--primary);
        color: #fff;
        text-decoration: none;
        padding: 12px 24px;
        border-radius: 10px;
        font-weight: 700;
    }

    .divider \{
        height: 1px;
        background: var(--divider);
        margin: 0 24px;
    }
  </style>
</head>
<body>
  <div class="container">
    <div class="header">
      <img class="logo" src="{logo_link}" alt="Bitcredit">
    </div>

    <div class="card">
      <div class="section">
        <h1>{title}</h1>
      </div>

      <div class="section">
          <div class="content">
            {{call content with content}}
          </div>
      </div>

      <div class="divider"></div>
    </div>

    <div style="height:24px"></div>
  </div>
</body>
</html>
"#;

pub const ERROR_SUCCESS_TEMPLATE: &str = r#"
    {msg}
"#;

pub const PREFERENCES_TEMPLATE: &str = r#"
    <h4>Email: {email} <br /> NodeID: {node_id} <br/ > Company NodeId: {company_node_id}</h4>
    <form action="/email/preferences/update_preferences" method="POST">
        <input type="hidden" name="pref_token" value="{ pref_token }"/>
        <div>
            <input {{if enabled}} checked {{endif}} type="checkbox" name="enabled" id="enabled" />
            <label for="enabled">Enabled</label>
        </div>
        <hr />
        {{ for flag in flags }}
        <div>
            <input {{if flag.checked }} checked {{endif}} type="checkbox" name="flags" value="{ flag.value }" id="flag{ flag.value }"/>
            <label for="flag{ flag.value }">{ flag.name }</label>
        </div>
        {{ endfor }}
        <div>
          <div class="cta-wrap">
            <button class="btn" type="submit">Submit</button>
          </div>
        </div>
    </form>
"#;

#[derive(Debug, Clone, Deserialize)]
pub struct ChangePreferencesPayload {
    pub pref_token: Uuid,
    pub enabled: Option<String>,
    pub flags: Option<Vec<i64>>,
}

#[derive(Debug, Serialize)]
pub struct PreferencesContext {
    pub content: PreferencesContextContent,
    pub title: String,
    pub logo_link: url::Url,
}

#[derive(Debug, Serialize)]
pub struct PreferencesContextContent {
    pub enabled: bool,
    pub pref_token: Uuid,
    pub email: EmailAddress,
    pub node_id: NodeId,
    pub company_node_id: Option<NodeId>,
    pub flags: Vec<PreferencesContextContentFlag>,
}

pub fn preferences_as_content_flags(value: PreferencesFlags) -> Vec<PreferencesContextContentFlag> {
    let all_flags = [
        (PreferencesFlags::BillSigned, "Bill Signed"),
        (PreferencesFlags::BillAccepted, "Bill Accepted"),
        (
            PreferencesFlags::BillAcceptanceRequested,
            "Bill Acceptance Requested",
        ),
        (
            PreferencesFlags::BillAcceptanceRejected,
            "Bill Acceptance Rejected",
        ),
        (
            PreferencesFlags::BillAcceptanceTimeout,
            "Bill Acceptance Timeout",
        ),
        (
            PreferencesFlags::BillAcceptanceRecourse,
            "Bill Acceptance Recourse",
        ),
        (
            PreferencesFlags::BillPaymentRequested,
            "Bill Payment Requested",
        ),
        (
            PreferencesFlags::BillPaymentRejected,
            "Bill Payment Rejected",
        ),
        (PreferencesFlags::BillPaymentTimeout, "Bill Payment Timeout"),
        (
            PreferencesFlags::BillPaymentRecourse,
            "Bill Payment Recourse",
        ),
        (
            PreferencesFlags::BillRecourseRejected,
            "Bill Recourse Rejected",
        ),
        (
            PreferencesFlags::BillRecourseTimeout,
            "Bill Recourse Timeout",
        ),
        (PreferencesFlags::BillSellOffered, "Bill Sell Offered"),
        (PreferencesFlags::BillBuyingRejected, "Bill Buying Rejected"),
        (PreferencesFlags::BillPaid, "Bill Paid"),
        (PreferencesFlags::BillRecoursePaid, "Bill Recourse Paid"),
        (PreferencesFlags::BillEndorsed, "Bill Endorsed"),
        (PreferencesFlags::BillSold, "Bill Sold"),
        (
            PreferencesFlags::BillMintingRequested,
            "Bill Minting Requested",
        ),
        (PreferencesFlags::BillNewQuote, "Bill New Quote"),
        (PreferencesFlags::BillQuoteApproved, "Bill Quote Approved"),
    ];

    all_flags
        .iter()
        .map(|(flag, name)| PreferencesContextContentFlag {
            checked: value.contains(*flag),
            value: flag.bits(),
            name: name.to_string(),
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct PreferencesContextContentFlag {
    pub checked: bool,
    pub value: i64,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorSuccessContext {
    pub content: ErrorSuccessContextContent,
    pub title: String,
    pub logo_link: url::Url,
}

#[derive(Debug, Serialize)]
pub struct ErrorSuccessContextContent {
    pub msg: String,
}

pub fn build_html_error(status: StatusCode, msg: &str, logo_url: &url::Url) -> impl IntoResponse {
    build_template(
        ERROR_SUCCESS_TEMPLATE,
        ErrorSuccessContext {
            content: ErrorSuccessContextContent {
                msg: msg.to_owned(),
            },
            title: "Error".to_owned(),
            logo_link: logo_url.to_owned(),
        },
        status,
    )
}

pub fn build_template<C>(content_tmpl: &str, ctx: C, status: StatusCode) -> impl IntoResponse
where
    C: Serialize,
{
    let mut tt = TinyTemplate::new();
    if let Err(e) = tt.add_template("base", TEMPLATE) {
        error!("error building base template: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response();
    }
    if let Err(e) = tt.add_template("content", content_tmpl) {
        error!("error building content template: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response();
    }

    let rendered = match tt.render("base", &ctx) {
        Ok(r) => r,
        Err(e) => {
            error!("error building template: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response();
        }
    };
    (status, Html(rendered)).into_response()
}
