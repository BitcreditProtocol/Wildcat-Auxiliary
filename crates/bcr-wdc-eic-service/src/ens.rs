use crate::{
    error::{Error, Result},
    service::EnsClient,
};
use anyhow::anyhow;
use async_trait::async_trait;
use bcr_common::core::NodeId;
use bcr_wdc_shared::wire::SetEmailPreferencesRequest;
use email_address::EmailAddress;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct EnsRestConfig {
    pub base_url: url::Url,
}

#[derive(Debug, Clone)]
pub struct Client {
    cl: reqwest::Client,
    base: reqwest::Url,
}

impl Client {
    pub fn new(base: url::Url) -> Self {
        Self {
            cl: reqwest::Client::new(),
            base,
        }
    }
}

#[async_trait]
impl EnsClient for Client {
    async fn set_email_preferences(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
        email: &EmailAddress,
    ) -> Result<()> {
        tracing::debug!(
            "EnsClient: set_email_preferences called with node id: {}, company_node_id: {:?}, email: {}",
            node_id,
            company_node_id,
            email
        );

        let payload = SetEmailPreferencesRequest {
            node_id: node_id.to_owned(),
            company_node_id: company_node_id.to_owned(),
            email: email.to_owned(),
        };

        let url = self
            .base
            .join("/v1/email/preferences")
            .expect("set email preferences path");

        let res = self
            .cl
            .post(url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| Error::EnsClient(anyhow!(e)))?;

        if res.status() == reqwest::StatusCode::BAD_REQUEST {
            return Err(Error::EnsClient(anyhow!("Invalid Request")));
        }

        if res.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(Error::EnsClient(anyhow!("No Entry Found")));
        }

        res.error_for_status()
            .map_err(|e| Error::EnsClient(anyhow!(e)))?;
        Ok(())
    }
}
