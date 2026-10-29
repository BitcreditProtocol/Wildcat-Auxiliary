use super::{health_router, run};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use std::{
    collections::VecDeque,
    future::pending,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{oneshot, watch},
    task::{JoinHandle, JoinSet},
};
use tower::ServiceExt;

enum StartOutcome {
    Failure,
    Running(Option<oneshot::Receiver<bool>>),
    Empty,
    Finished,
}

#[derive(Default)]
struct ConsumerState {
    outcomes: Mutex<VecDeque<StartOutcome>>,
    starts: AtomicUsize,
    cleanups: AtomicUsize,
    live_tasks: AtomicUsize,
    max_live_tasks: AtomicUsize,
    cleanup_live_counts: Mutex<Vec<usize>>,
}

struct LiveTask(Arc<ConsumerState>);

impl Drop for LiveTask {
    fn drop(&mut self) {
        self.0.live_tasks.fetch_sub(1, Ordering::SeqCst);
    }
}

fn spawn_live_task(
    tasks: &mut JoinSet<()>,
    state: Arc<ConsumerState>,
    exit: Option<oneshot::Receiver<bool>>,
) {
    tasks.spawn(async move {
        let live = state.live_tasks.fetch_add(1, Ordering::SeqCst) + 1;
        state.max_live_tasks.fetch_max(live, Ordering::SeqCst);
        let _guard = LiveTask(state);
        match exit {
            Some(exit) => {
                assert!(
                    !exit.await.unwrap_or(false),
                    "simulated consumer task panic"
                );
            }
            None => pending::<()>().await,
        }
    });
}

struct Harness {
    state: Arc<ConsumerState>,
    readiness: watch::Receiver<bool>,
    supervisor: JoinHandle<()>,
}

impl Harness {
    fn new(outcomes: Vec<StartOutcome>, restart_interval: Duration) -> Self {
        let state = Arc::new(ConsumerState {
            outcomes: Mutex::new(outcomes.into()),
            ..ConsumerState::default()
        });
        let start_state = state.clone();
        let cleanup_state = state.clone();
        let (sender, readiness) = watch::channel(false);
        let supervisor = tokio::spawn(run(
            move || {
                let state = start_state.clone();
                async move {
                    state.starts.fetch_add(1, Ordering::SeqCst);
                    let outcome = state
                        .outcomes
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or(StartOutcome::Running(None));
                    let mut tasks = JoinSet::new();
                    match outcome {
                        StartOutcome::Failure => return Err("transient start failure"),
                        StartOutcome::Empty => return Ok(tasks),
                        StartOutcome::Finished => {
                            tasks.spawn(async {});
                        }
                        StartOutcome::Running(exit) => {
                            spawn_live_task(&mut tasks, state.clone(), exit);
                            spawn_live_task(&mut tasks, state, None);
                        }
                    }
                    Ok(tasks)
                }
            },
            move || {
                cleanup_state.cleanups.fetch_add(1, Ordering::SeqCst);
                cleanup_state
                    .cleanup_live_counts
                    .lock()
                    .unwrap()
                    .push(cleanup_state.live_tasks.load(Ordering::SeqCst));
                async {}
            },
            restart_interval,
            sender,
        ));
        Self {
            state,
            readiness,
            supervisor,
        }
    }

    fn starts(&self) -> usize {
        self.state.starts.load(Ordering::SeqCst)
    }

    async fn status(&self) -> StatusCode {
        health_status(self.readiness.clone()).await
    }

    async fn stop(mut self) {
        self.supervisor.abort();
        assert!((&mut self.supervisor).await.unwrap_err().is_cancelled());
        settle().await;
        assert_eq!(self.state.live_tasks.load(Ordering::SeqCst), 0);
        assert_eq!(self.status().await, StatusCode::SERVICE_UNAVAILABLE);
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.supervisor.abort();
    }
}

async fn health_status(readiness: watch::Receiver<bool>) -> StatusCode {
    health_router(readiness)
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

async fn settle() {
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn retries_a_failed_start_and_restores_readiness() {
    let harness = Harness::new(vec![StartOutcome::Failure], Duration::from_secs(3600));
    settle().await;
    assert_eq!(harness.starts(), 1);
    tokio::time::advance(Duration::from_millis(999)).await;
    settle().await;
    assert_eq!(
        harness.starts(),
        1,
        "failed starts should wait before retrying"
    );
    tokio::time::advance(Duration::from_millis(1)).await;
    settle().await;
    assert_eq!(
        harness.starts(),
        2,
        "a transient start failure must not kill the consumer loop"
    );
    assert_eq!(harness.state.cleanups.load(Ordering::SeqCst), 1);
    assert_eq!(*harness.state.cleanup_live_counts.lock().unwrap(), vec![0]);
    assert_eq!(harness.status().await, StatusCode::OK);
    harness.stop().await;
}

#[tokio::test(start_paused = true)]
async fn repeated_start_failures_use_capped_exponential_backoff() {
    let harness = Harness::new(
        (0..10).map(|_| StartOutcome::Failure).collect(),
        Duration::from_secs(3600),
    );
    settle().await;
    assert_eq!(harness.starts(), 1);

    for (attempt, delay) in [1, 2, 4, 8, 16, 32, 60, 60].into_iter().enumerate() {
        tokio::time::advance(Duration::from_secs(delay) - Duration::from_millis(1)).await;
        settle().await;
        assert_eq!(
            harness.starts(),
            attempt + 1,
            "retry happened before its delay"
        );
        assert_eq!(harness.status().await, StatusCode::SERVICE_UNAVAILABLE);
        tokio::time::advance(Duration::from_millis(1)).await;
        settle().await;
        assert_eq!(harness.starts(), attempt + 2);
    }
    harness.stop().await;
}

async fn assert_child_exit_recovers(panic: bool) {
    let (exit, receiver) = oneshot::channel();
    let harness = Harness::new(
        vec![StartOutcome::Running(Some(receiver))],
        Duration::from_secs(3600),
    );
    settle().await;
    assert_eq!(harness.status().await, StatusCode::OK);
    assert_eq!(harness.state.live_tasks.load(Ordering::SeqCst), 2);

    exit.send(panic).unwrap();
    settle().await;
    assert_eq!(harness.starts(), 1);
    assert_eq!(harness.status().await, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(harness.state.live_tasks.load(Ordering::SeqCst), 0);
    assert_eq!(harness.state.cleanups.load(Ordering::SeqCst), 1);
    assert_eq!(*harness.state.cleanup_live_counts.lock().unwrap(), vec![0]);

    tokio::time::advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(harness.starts(), 2);
    assert_eq!(harness.status().await, StatusCode::OK);
    assert_eq!(harness.state.live_tasks.load(Ordering::SeqCst), 2);
    assert_eq!(harness.state.max_live_tasks.load(Ordering::SeqCst), 2);
    harness.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_finished_child_stops_its_siblings_before_recovery() {
    assert_child_exit_recovers(false).await;
}

#[tokio::test(start_paused = true)]
async fn a_panicking_child_stops_its_siblings_before_recovery() {
    assert_child_exit_recovers(true).await;
}

#[tokio::test(start_paused = true)]
async fn an_empty_start_result_is_unavailable_and_retried() {
    let harness = Harness::new(vec![StartOutcome::Empty], Duration::from_secs(3600));
    settle().await;
    assert_eq!(harness.starts(), 1);
    assert_eq!(harness.status().await, StatusCode::SERVICE_UNAVAILABLE);
    tokio::time::advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(harness.starts(), 2);
    assert_eq!(harness.status().await, StatusCode::OK);
    harness.stop().await;
}

#[tokio::test(start_paused = true)]
async fn rapidly_exiting_children_do_not_reset_backoff() {
    let harness = Harness::new(
        vec![StartOutcome::Finished, StartOutcome::Finished],
        Duration::from_secs(3600),
    );
    settle().await;
    assert_eq!(harness.starts(), 1);
    tokio::time::advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(harness.starts(), 2);
    tokio::time::advance(Duration::from_millis(1999)).await;
    settle().await;
    assert_eq!(harness.starts(), 2);
    assert_eq!(harness.status().await, StatusCode::SERVICE_UNAVAILABLE);
    tokio::time::advance(Duration::from_millis(1)).await;
    settle().await;
    assert_eq!(harness.starts(), 3);
    assert_eq!(harness.status().await, StatusCode::OK);
    harness.stop().await;
}

#[tokio::test(start_paused = true)]
async fn scheduled_restarts_clean_up_the_previous_consumer() {
    let harness = Harness::new(vec![], Duration::from_secs(10));
    settle().await;
    assert_eq!(harness.starts(), 1);
    assert_eq!(harness.status().await, StatusCode::OK);
    tokio::time::advance(Duration::from_millis(9999)).await;
    settle().await;
    assert_eq!(harness.starts(), 1);
    tokio::time::advance(Duration::from_millis(1)).await;
    settle().await;
    assert_eq!(harness.starts(), 2);
    assert_eq!(harness.state.cleanups.load(Ordering::SeqCst), 1);
    assert_eq!(*harness.state.cleanup_live_counts.lock().unwrap(), vec![0]);
    assert_eq!(harness.state.live_tasks.load(Ordering::SeqCst), 2);
    assert_eq!(harness.state.max_live_tasks.load(Ordering::SeqCst), 2);
    assert_eq!(harness.status().await, StatusCode::OK);
    harness.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_stable_consumer_resets_backoff_after_recovery() {
    let (exit, receiver) = oneshot::channel();
    let harness = Harness::new(
        vec![StartOutcome::Failure, StartOutcome::Running(Some(receiver))],
        Duration::from_secs(3600),
    );
    settle().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(harness.starts(), 2);
    assert_eq!(harness.status().await, StatusCode::OK);
    tokio::time::advance(Duration::from_secs(60)).await;
    exit.send(false).unwrap();
    settle().await;
    assert_eq!(harness.status().await, StatusCode::SERVICE_UNAVAILABLE);
    tokio::time::advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(
        harness.starts(),
        3,
        "a stable consumer should reset the retry delay"
    );
    assert_eq!(harness.status().await, StatusCode::OK);
    harness.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_scheduled_restart_resets_backoff_after_recovery() {
    let harness = Harness::new(
        vec![
            StartOutcome::Failure,
            StartOutcome::Running(None),
            StartOutcome::Failure,
        ],
        Duration::from_secs(10),
    );
    settle().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(harness.starts(), 2);
    tokio::time::advance(Duration::from_secs(10)).await;
    settle().await;
    assert_eq!(harness.starts(), 3);
    assert_eq!(harness.status().await, StatusCode::SERVICE_UNAVAILABLE);
    tokio::time::advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(harness.starts(), 4);
    assert_eq!(harness.status().await, StatusCode::OK);
    harness.stop().await;
}

#[tokio::test(start_paused = true)]
async fn health_requires_a_running_consumer_and_a_live_supervisor() {
    let (sender, readiness) = watch::channel(false);
    assert_eq!(
        health_status(readiness.clone()).await,
        StatusCode::SERVICE_UNAVAILABLE
    );
    sender.send(true).unwrap();
    assert_eq!(health_status(readiness.clone()).await, StatusCode::OK);
    drop(sender);
    assert_eq!(
        health_status(readiness).await,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test(start_paused = true)]
async fn aborting_during_backoff_stops_retries() {
    let harness = Harness::new(vec![StartOutcome::Failure], Duration::from_secs(3600));
    settle().await;
    let state = harness.state.clone();
    assert_eq!(harness.starts(), 1);
    harness.stop().await;
    tokio::time::advance(Duration::from_secs(3600)).await;
    settle().await;
    assert_eq!(state.starts.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn aborting_a_running_supervisor_stops_all_consumer_tasks() {
    let harness = Harness::new(vec![], Duration::from_secs(3600));
    settle().await;
    assert_eq!(harness.state.live_tasks.load(Ordering::SeqCst), 2);
    assert_eq!(harness.status().await, StatusCode::OK);
    harness.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_zero_restart_interval_does_not_spin() {
    let harness = Harness::new(vec![], Duration::ZERO);
    settle().await;
    assert_eq!(harness.starts(), 1);
    tokio::time::advance(Duration::from_millis(999)).await;
    settle().await;
    assert_eq!(harness.starts(), 1);
    tokio::time::advance(Duration::from_millis(1)).await;
    settle().await;
    assert_eq!(harness.starts(), 2);
    harness.stop().await;
}

#[tokio::test(start_paused = true)]
async fn aborting_during_startup_closes_readiness() {
    let (sender, readiness) = watch::channel(false);
    let supervisor = tokio::spawn(run(
        pending::<Result<JoinSet<()>, &'static str>>,
        || async {},
        Duration::from_secs(240),
        sender,
    ));
    settle().await;
    assert_eq!(
        health_status(readiness.clone()).await,
        StatusCode::SERVICE_UNAVAILABLE
    );
    supervisor.abort();
    assert!(supervisor.await.unwrap_err().is_cancelled());
    assert!(readiness.has_changed().is_err());
    assert_eq!(
        health_status(readiness).await,
        StatusCode::SERVICE_UNAVAILABLE
    );
}
