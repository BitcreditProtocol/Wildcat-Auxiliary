// ----- standard library imports
use std::{collections::HashSet, time::Duration};
// ----- extra library imports
use bcr_common::core::{BillId, NodeId};
use bcr_ebill_core::{
    application::bill::BitcreditBillResult,
    protocol::{
        Timestamp,
        blockchain::bill::participant::{BillIdentParticipant, BillParticipant},
    },
};
use bcr_wdc_ebill_service::AppController;
use tokio::time::{interval, sleep};
use tracing::{error, info};
// ----- end imports

pub async fn run(
    app: AppController,
    job_runner_initial_delay_seconds: u64,
    job_runner_check_interval_seconds: u64,
) {
    sleep(Duration::from_secs(job_runner_initial_delay_seconds)).await;
    info!(
        "Job runner started after {job_runner_initial_delay_seconds}s of initial delay, running jobs every {job_runner_check_interval_seconds}s...",
    );

    let mut check_interval_tick = interval(Duration::from_secs(job_runner_check_interval_seconds));
    let mut reported_matured_bills = HashSet::new();
    loop {
        check_interval_tick.tick().await;
        run_jobs(&app, &mut reported_matured_bills).await;
    }
}

async fn run_jobs(app: &AppController, reported_matured_bills: &mut HashSet<BillId>) {
    tokio::join!(
        run_check_bill_payment_job(app.clone()),
        run_check_bill_offer_to_sell_payment_job(app.clone()),
        run_check_bill_recourse_payment_job(app.clone())
    );
    // explicitly not added to join! because we want to run this job after
    // all payment jobs are done and avoid any concurrency issues.
    run_check_bill_timeouts(app.clone()).await;
    // runs after the payment and timeout jobs, so it sees the updated bill states
    run_check_bill_maturity(app, reported_matured_bills).await;
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

async fn run_check_bill_maturity(app: &AppController, reported: &mut HashSet<BillId>) {
    info!("Running Check Bill Maturity Job");
    if let Err(e) = check_bills_maturity(app, reported).await {
        error!("Error while running Check Bill Maturity Job: {e}");
    }
    info!("Finished running Check Bill Maturity Job");
}

async fn check_bills_maturity(
    app: &AppController,
    reported: &mut HashSet<BillId>,
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
    for bill in bills {
        if reported.contains(&bill.id) {
            continue;
        }
        if bill.daata.maturity_date > now {
            continue;
        }
        on_bill_matured(&bill);
        reported.insert(bill.id);
    }
    Ok(())
}


fn on_bill_matured(bill: &BitcreditBillResult) {
    info!(
        "Bill {} matured on {} - sum: {} sat",
        bill.id,
        bill.data.maturity_date,
        bill.data.sum.as_sat()
    );
}
