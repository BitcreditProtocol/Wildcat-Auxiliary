use bcr_common::{
    client::admin::quote::Client as QuoteClient,
    core::NodeId,
    wire::quotes::{InfoReply, InfoReplyDiscriminants, ListParam, UpdateQuoteResponse},
};
use chrono::{NaiveDate, Utc};
use std::collections::{HashMap, VecDeque};

type TStamp = chrono::DateTime<chrono::Utc>;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct AppConfig {
    pub quotes_url: url::Url,
    pub sleep_secs: u64,
    pub retention_period_secs: i64,
    pub max_requests_per_retention_period: usize,
    pub max_bill_offer_sum: u64,
    pub discount_percent: u64,
}

pub async fn main_loop(
    cfg: AppConfig,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<(), Box<dyn std::error::Error>> {
    let AppConfig {
        quotes_url,
        sleep_secs,
        retention_period_secs,
        max_requests_per_retention_period,
        max_bill_offer_sum,
        discount_percent,
    } = cfg;

    let quote_client = QuoteClient::new(quotes_url);

    let sleep = tokio::time::Duration::from_secs(sleep_secs);
    let retention_period = chrono::Duration::seconds(retention_period_secs);

    // sliding window activity log for holders
    let mut activity_log: HashMap<NodeId, VecDeque<TStamp>> = HashMap::new();

    loop {
        // find pending quotes
        let list_param = ListParam {
            status: Some(InfoReplyDiscriminants::Pending),
            ..Default::default()
        };
        let pending_quotes = quote_client.list(list_param).await?.quotes;

        for pending_quote in pending_quotes.iter() {
            let quote = quote_client.lookup(pending_quote.id).await?;
            let InfoReply::Pending {
                bill, submitted, ..
            } = quote
            else {
                tracing::warn!("Unexpected quote status: id: {:?}", quote);
                continue;
            };

            if bill.sum > max_bill_offer_sum {
                continue;
            }

            // validate activity period
            let holder = bill.endorsees.last().unwrap_or(&bill.payee).node_id();
            let last_requests = activity_log.entry(holder).or_default();

            // remove timestamps outside the window
            while let Some(front) = last_requests.front() {
                if submitted - *front < retention_period {
                    last_requests.pop_front();
                } else {
                    break;
                }
            }

            // if holder did more requests in the last $retention_period than we allow, deny to avoid spam
            if last_requests.len() >= max_requests_per_retention_period {
                quote_client.deny(pending_quote.id).await?;
                continue;
            }

            last_requests.push_back(submitted);

            // offer the quote
            let discounted = discount_sats(bill.sum, discount_percent, bill.maturity_date);
            let offer = quote_client
                .offer(
                    pending_quote.id,
                    bitcoin::Amount::from_sat(discounted),
                    None,
                )
                .await?;
            match offer {
                UpdateQuoteResponse::Denied => {
                    tracing::warn!("Offer for quote {} was denied", pending_quote.id);
                }
                UpdateQuoteResponse::Offered { discounted, .. } => {
                    tracing::info!(
                        "Created an offer for quote {} with {} sat discount",
                        pending_quote.id,
                        discounted.to_sat()
                    );
                }
            }
        }

        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!("Cancellation requested, exiting main loop.");
                break;
            },
            _ = tokio::time::sleep(sleep) => {
                tracing::info!("Waiting for the next iteration...");
            }
        }
    }

    Ok(())
}

// Act/360 helper to calculate discounted sum
pub fn discount_sats(sum: u64, discount_percent: u64, maturity_date: NaiveDate) -> u64 {
    if discount_percent == 0 {
        return sum;
    }
    let discount_percent = discount_percent.clamp(0, 99);
    let days = maturity_date
        .signed_duration_since(Utc::now().date_naive())
        .num_days()
        .clamp(1, 360) as u64;
    sum - (sum * discount_percent * days / 100 / 360)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn returns_sum_when_discount_percent_is_zero() {
        let maturity_date = Utc::now().date_naive() + Duration::days(180);
        assert_eq!(discount_sats(100_000, 0, maturity_date), 100_000);
    }

    #[test]
    fn calculates_half_year_discount() {
        let maturity_date = Utc::now().date_naive() + Duration::days(180);
        assert_eq!(discount_sats(100_000, 10, maturity_date), 95_000);
    }

    #[test]
    fn clamps_maturity_to_360_days() {
        let maturity_date = Utc::now().date_naive() + Duration::days(720);
        assert_eq!(discount_sats(100_000, 10, maturity_date), 90_000);
    }

    #[test]
    fn clamps_past_maturity_to_one_day() {
        let maturity_date = Utc::now().date_naive() - Duration::days(30);
        assert_eq!(discount_sats(360_000, 10, maturity_date), 359_900);
    }

    #[test]
    fn clamps_maturity_and_percent() {
        let maturity_date = Utc::now().date_naive() + Duration::days(720);
        assert_eq!(discount_sats(360_000, 150, maturity_date), 3600);
    }

    #[test]
    fn one_day() {
        let maturity_date = Utc::now().date_naive() + Duration::days(1);
        assert_eq!(discount_sats(360_000, 10, maturity_date), 359_900);
    }
}
