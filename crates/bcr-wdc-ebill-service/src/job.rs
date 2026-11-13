// ----- standard library imports
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};
// ----- extra library imports
use bcr_common::{
    client::admin::notification::Client as NotificationClient, core::BillId,
    wire::notification::NotificationRequest,
};
use bcr_ebill_core::{
    application::bill::BitcreditBillResult,
    protocol::{
        Timestamp,
        blockchain::bill::participant::{BillIdentParticipant, BillParticipant},
    },
};
use bcr_wdc_ebill_service::AppController;
use tokio::time::{interval, sleep};
use tracing::{error, info, warn};
// ----- end imports

pub async fn run(
    app: AppController,
    notification_client: Option<NotificationClient>,
    job_runner_initial_delay_seconds: u64,
    job_runner_check_interval_seconds: u64,
) {
    sleep(Duration::from_secs(job_runner_initial_delay_seconds)).await;
    info!(
        "Job runner started after {job_runner_initial_delay_seconds}s of initial delay, running jobs every {job_runner_check_interval_seconds}s...",
    );

    let mut check_interval_tick = interval(Duration::from_secs(job_runner_check_interval_seconds));
    let mut bill_event_tracker = BillEventTracker::default();
    loop {
        check_interval_tick.tick().await;
        run_jobs(&app, &notification_client, &mut bill_event_tracker).await;
    }
}

async fn run_jobs(
    app: &AppController,
    notification_client: &Option<NotificationClient>,
    bill_event_tracker: &mut BillEventTracker,
) {
    tokio::join!(
        run_check_bill_payment_job(app.clone()),
        run_check_bill_offer_to_sell_payment_job(app.clone()),
        run_check_bill_recourse_payment_job(app.clone())
    );
    // explicitly not added to join! because we want to run this job after
    // all payment jobs are done and avoid any concurrency issues.
    run_check_bill_timeouts(app.clone()).await;
    // runs after the payment and timeout jobs, so it sees the updated bill states
    let now = Timestamp::now();
    run_check_bill_events(app, notification_client, bill_event_tracker, now).await;
}

async fn run_check_bill_payment_job(app: AppController) {
    info!("Running Check Bill Payment Job");
    if let Err(e) = app.bill_service.check_bills_payment().await {
        error!("Error while running Check Bill Payment Job: {e}");
    }
    info!("Finished running Check Bill Payment Job");
}

async fn run_check_bill_offer_to_sell_payment_job(app: AppController) {
    info!("Running Check Bill Offer to Sell Payment Job");
    if let Err(e) = app.bill_service.check_bills_offer_to_sell_payment().await {
        error!("Error while running Check Bill Offer to Sell Payment Job: {e}");
    }
    info!("Finished running Check Bill Offer to Sell Payment Job");
}

async fn run_check_bill_recourse_payment_job(app: AppController) {
    info!("Running Check Bill Recourse Payment Job");
    if let Err(e) = app.bill_service.check_bills_in_recourse_payment().await {
        error!("Error while running Check Bill Recourse Payment Job: {e}");
    }
    info!("Finished running Check Bill Recourse Payment Job");
}

async fn run_check_bill_timeouts(app: AppController) {
    info!("Running Check Bill Timeouts Job");
    let current_time = Timestamp::now();
    if let Err(e) = app.bill_service.check_bills_timeouts(current_time).await {
        error!("Error while running Check Bill Timeouts Job: {e}");
    }

    info!("Finished running Check Bill Timeouts Job");
}

async fn run_check_bill_events(
    app: &AppController,
    notification_client: &Option<NotificationClient>,
    tracker: &mut BillEventTracker,
    now: Timestamp,
) {
    info!("Running Check Bill Events Job");
    if let Err(e) = check_bill_events(app, notification_client, tracker, now).await {
        error!("Error while running Check Bill Events Job: {e}");
    }
    info!("Finished running Check Bill Events Job");
}

#[derive(Debug, Default)]
struct BillEventTracker {
    /// held bills that are not matured yet
    under_monitor: HashSet<BillId>,
    /// held bills that are matured but not repaid yet
    matured: HashSet<BillId>,
}

async fn check_bill_events(
    app: &AppController,
    notification_client: &Option<NotificationClient>,
    tracker: &mut BillEventTracker,
    now: Timestamp,
) -> Result<(), Box<dyn std::error::Error>> {
    let identity = app.identity_service.get_full_identity().await?;
    let own_node_id = identity.identity.node_id.clone();
    let bills = app
        .bill_service
        .get_bills(
            &BillParticipant::Ident(BillIdentParticipant::new(identity.identity)?),
            &identity.key_pair,
        )
        .await?;
    let mut held = HashSet::new();
    for bill in bills {
        let holder = bill
            .participants
            .endorsee
            .clone()
            .unwrap_or(bill.participants.payee.clone());
        if holder.node_id() != own_node_id {
            continue;
        }
        held.insert(bill.id.clone());
        let matured = bill.data.maturity_date.to_timestamp() <= now;
        let paid = bill.status.payment.paid;
        match (matured, paid) {
            (false, false) => {
                tracker.under_monitor.insert(bill.id);
            }
            // paid before maturity, nothing to report
            (false, true) => {
                tracker.under_monitor.remove(&bill.id);
            }
            (true, false) => {
                if tracker.under_monitor.remove(&bill.id) {
                    on_bill_matured(&bill, notification_client);
                }
                tracker.matured.insert(bill.id);
            }
            (true, true) => {
                if tracker.matured.remove(&bill.id) {
                    on_bill_repaid(&bill, notification_client);
                } else if tracker.under_monitor.remove(&bill.id) {
                    // matured and repaid since the last run
                    on_bill_matured(&bill, notification_client);
                    on_bill_repaid(&bill, notification_client);
                }
            }
        }
    }
    // forget the bills we don't hold anymore
    tracker.under_monitor.retain(|id| held.contains(id));
    tracker.matured.retain(|id| held.contains(id));
    info!(
        "Bill events: {} held bills, {} under monitor, {} matured not repaid",
        held.len(),
        tracker.under_monitor.len(),
        tracker.matured.len()
    );
    Ok(())
}

fn on_bill_matured(bill: &BitcreditBillResult, notification_client: &Option<NotificationClient>) {
    info!(
        "Bill {} matured on {} - sum: {} sat",
        bill.id,
        bill.data.maturity_date,
        bill.data.sum.as_sat()
    );
    let request = NotificationRequest::EbillMatured {
        fields: bill_notification_fields(bill),
    };
    send_notification(notification_client, request);
}

fn on_bill_repaid(bill: &BitcreditBillResult, notification_client: &Option<NotificationClient>) {
    info!(
        "Bill {} repaid - sum: {} sat",
        bill.id,
        bill.data.sum.as_sat()
    );
    let request = NotificationRequest::EbillRepaid {
        fields: bill_notification_fields(bill),
    };
    send_notification(notification_client, request);
}

fn bill_notification_fields(bill: &BitcreditBillResult) -> HashMap<String, String> {
    HashMap::from([
        (String::from("bill_id"), bill.id.to_string()),
        (
            String::from("drawee"),
            bill.participants.drawee.name.to_string(),
        ),
        (
            String::from("drawer"),
            bill.participants.drawer.name.to_string(),
        ),
        (
            String::from("amount"),
            format!("{} sat", bill.data.sum.as_sat()),
        ),
        (
            String::from("maturity_date"),
            bill.data.maturity_date.to_string(),
        ),
    ])
}

/// fire-and-forget: notifications are best-effort, failures are only logged
fn send_notification(
    notification_client: &Option<NotificationClient>,
    request: NotificationRequest,
) {
    let Some(client) = notification_client.clone() else {
        return;
    };
    tokio::spawn(async move {
        if let Err(e) = client.notify(request).await {
            warn!("Failed to send admin notification: {e}");
        }
    });
}
