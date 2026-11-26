use super::{Device, device_store::DeviceStoreApi};
use async_trait::async_trait;
use nostr::{
    event::{Event, EventId, Kind},
    filter::Filter,
    types::Timestamp,
};
use nostr_database::{
    DatabaseEventStatus, Features, NostrDatabase, SaveEventStatus, error::Error as DatabaseError,
};
use serde::Serialize;
use std::{
    collections::{BTreeSet, HashSet, VecDeque},
    fmt,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{error, info, warn};

/// Jobs waiting for the worker; a full queue drops new jobs, which wallets recover by catch-up
const QUEUE_CAPACITY: usize = 4096;
/// Event ids remembered to keep one job per stored event
const SEEN_CAPACITY: usize = 16_384;

type DbFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, DatabaseError>> + Send + 'a>>;

/// What a push carries: only what a wallet needs to fetch the event from this relay
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PushPayload {
    pub event_id: EventId,
    pub relay_url: String,
    pub kind: u16,
}

/// One push to deliver: the payload and the registered devices of the event's recipients
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushJob {
    pub payload: PushPayload,
    pub devices: Vec<Device>,
}

/// Delivers a push to devices
#[async_trait]
pub trait PushSender: Send + Sync {
    async fn send(&self, payload: &PushPayload, devices: &[Device]) -> Result<(), anyhow::Error>;
}

/// Sender used while no push provider is configured
pub struct LogSender;

#[async_trait]
impl PushSender for LogSender {
    async fn send(&self, payload: &PushPayload, devices: &[Device]) -> Result<(), anyhow::Error> {
        info!(
            "No push provider configured, not pushing {} to {} device(s)",
            payload.event_id,
            devices.len()
        );
        Ok(())
    }
}

#[derive(Debug, Default)]
struct SeenIds {
    order: VecDeque<EventId>,
    ids: HashSet<EventId>,
}

impl SeenIds {
    fn contains(&self, id: &EventId) -> bool {
        self.ids.contains(id)
    }

    fn insert(&mut self, id: EventId) {
        if self.ids.insert(id) {
            self.order.push_back(id);
            if self.order.len() > SEEN_CAPACITY
                && let Some(oldest) = self.order.pop_front()
            {
                self.ids.remove(&oldest);
            }
        }
    }
}

/// Queue of push jobs with at most one job per event id
#[derive(Debug, Clone)]
pub struct PushQueue {
    tx: mpsc::Sender<PushJob>,
    seen: Arc<Mutex<SeenIds>>,
}

impl PushQueue {
    pub fn new() -> (Self, mpsc::Receiver<PushJob>) {
        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
        let queue = Self {
            tx,
            seen: Arc::default(),
        };
        (queue, rx)
    }

    /// Queues the job unless its event id was queued before; returns whether it was queued
    pub fn enqueue(&self, job: PushJob) -> bool {
        let mut seen = self.seen.lock().expect("push queue lock");
        if seen.contains(&job.payload.event_id) {
            return false;
        }
        let id = job.payload.event_id;
        match self.tx.try_send(job) {
            Ok(()) => {
                seen.insert(id);
                true
            }
            Err(e) => {
                warn!("Dropping push for {id}: {e}");
                false
            }
        }
    }
}

/// Sends queued jobs until every queue handle is dropped; a job runs only once its event can be
/// fetched from `database`
pub fn spawn_worker(
    mut jobs: mpsc::Receiver<PushJob>,
    database: Arc<dyn NostrDatabase>,
    sender: Arc<dyn PushSender>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(job) = jobs.recv().await {
            let id = job.payload.event_id;
            match database.event_by_id(&id).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    warn!("Not pushing {id}: event is not stored");
                    continue;
                }
                Err(e) => {
                    error!("Not pushing {id}: event lookup failed: {e}");
                    continue;
                }
            }
            if let Err(e) = sender.send(&job.payload, &job.devices).await {
                error!("Push for {id} failed: {e}");
            }
        }
    })
}

/// A database that queues a push once a gift wrap for a registered recipient is saved
pub struct PushingDatabase {
    inner: Arc<dyn NostrDatabase>,
    devices: Arc<dyn DeviceStoreApi>,
    queue: PushQueue,
    relay_url: String,
}

impl fmt::Debug for PushingDatabase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PushingDatabase")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

impl PushingDatabase {
    pub fn new(
        inner: Arc<dyn NostrDatabase>,
        devices: Arc<dyn DeviceStoreApi>,
        queue: PushQueue,
        relay_url: String,
    ) -> Self {
        Self {
            inner,
            devices,
            queue,
            relay_url,
        }
    }

    async fn queue_push(&self, event: &Event) {
        let mut devices = Vec::new();
        let recipients: BTreeSet<_> = event.tags.public_keys().collect();
        for recipient in recipients {
            match self.devices.devices_for(&recipient).await {
                Ok(found) => devices.extend(found),
                Err(e) => error!("Device lookup for {recipient} failed: {e}"),
            }
        }
        if devices.is_empty() {
            return;
        }
        self.queue.enqueue(PushJob {
            payload: PushPayload {
                event_id: event.id,
                relay_url: self.relay_url.clone(),
                kind: event.kind.as_u16(),
            },
            devices,
        });
    }
}

impl NostrDatabase for PushingDatabase {
    fn backend(&self) -> &'static str {
        self.inner.backend()
    }

    fn features(&self) -> Features {
        self.inner.features()
    }

    fn save_event<'a>(&'a self, event: &'a Event) -> DbFuture<'a, SaveEventStatus> {
        Box::pin(async move {
            let status = self.inner.save_event(event).await?;
            if status.is_success() && event.kind == Kind::GiftWrap {
                self.queue_push(event).await;
            }
            Ok(status)
        })
    }

    fn check_id<'a>(&'a self, event_id: &'a EventId) -> DbFuture<'a, DatabaseEventStatus> {
        self.inner.check_id(event_id)
    }

    fn event_by_id<'a>(&'a self, event_id: &'a EventId) -> DbFuture<'a, Option<Event>> {
        self.inner.event_by_id(event_id)
    }

    fn count(&self, filter: Filter) -> DbFuture<'_, usize> {
        self.inner.count(filter)
    }

    fn query(&self, filter: Filter) -> DbFuture<'_, BTreeSet<Event>> {
        self.inner.query(filter)
    }

    fn negentropy_items(&self, filter: Filter) -> DbFuture<'_, Vec<(EventId, Timestamp)>> {
        self.inner.negentropy_items(filter)
    }

    fn delete(&self, filter: Filter) -> DbFuture<'_, ()> {
        self.inner.delete(filter)
    }

    fn wipe(&self) -> DbFuture<'_, ()> {
        self.inner.wipe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::{Platform, device_store::MemoryDeviceStore};
    use nostr::{
        event::{EventBuilder, FinalizeEvent, Tag},
        key::{Keys, PublicKey},
    };
    use std::collections::HashMap;

    /// Stores events by id; `report_duplicates` mirrors the Postgres store, which rejects a
    /// second save of one id
    #[derive(Debug, Default)]
    struct TestDatabase {
        events: Mutex<HashMap<EventId, Event>>,
        report_duplicates: bool,
    }

    impl NostrDatabase for TestDatabase {
        fn backend(&self) -> &'static str {
            "test"
        }

        fn features(&self) -> Features {
            Features {
                persistent: false,
                event_expiration: false,
                full_text_search: false,
                request_to_vanish: false,
            }
        }

        fn save_event<'a>(&'a self, event: &'a Event) -> DbFuture<'a, SaveEventStatus> {
            Box::pin(async move {
                let mut events = self.events.lock().unwrap();
                if self.report_duplicates && events.contains_key(&event.id) {
                    return Ok(SaveEventStatus::Rejected(
                        nostr_database::RejectedReason::Duplicate,
                    ));
                }
                events.insert(event.id, event.clone());
                Ok(SaveEventStatus::Success)
            })
        }

        fn check_id<'a>(&'a self, event_id: &'a EventId) -> DbFuture<'a, DatabaseEventStatus> {
            Box::pin(async move {
                Ok(match self.events.lock().unwrap().contains_key(event_id) {
                    true => DatabaseEventStatus::Saved,
                    false => DatabaseEventStatus::NotExistent,
                })
            })
        }

        fn event_by_id<'a>(&'a self, event_id: &'a EventId) -> DbFuture<'a, Option<Event>> {
            Box::pin(async move { Ok(self.events.lock().unwrap().get(event_id).cloned()) })
        }

        fn count(&self, _filter: Filter) -> DbFuture<'_, usize> {
            Box::pin(async move { Ok(self.events.lock().unwrap().len()) })
        }

        fn query(&self, _filter: Filter) -> DbFuture<'_, BTreeSet<Event>> {
            Box::pin(async move { Ok(self.events.lock().unwrap().values().cloned().collect()) })
        }

        fn delete(&self, _filter: Filter) -> DbFuture<'_, ()> {
            Box::pin(async move { Ok(()) })
        }

        fn wipe(&self) -> DbFuture<'_, ()> {
            Box::pin(async move { Ok(()) })
        }
    }

    /// Records each push and whether its event was fetchable from the database at that moment
    struct RecordingSender {
        database: Arc<dyn NostrDatabase>,
        sent: Mutex<Vec<(PushPayload, Vec<Device>, bool)>>,
    }

    #[async_trait]
    impl PushSender for RecordingSender {
        async fn send(
            &self,
            payload: &PushPayload,
            devices: &[Device],
        ) -> Result<(), anyhow::Error> {
            let fetchable = self
                .database
                .event_by_id(&payload.event_id)
                .await?
                .is_some();
            self.sent
                .lock()
                .unwrap()
                .push((payload.clone(), devices.to_vec(), fetchable));
            Ok(())
        }
    }

    struct Setup {
        database: PushingDatabase,
        sender: Arc<RecordingSender>,
        worker: JoinHandle<()>,
        devices: Arc<MemoryDeviceStore>,
    }

    fn setup(report_duplicates: bool) -> Setup {
        let inner: Arc<dyn NostrDatabase> = Arc::new(TestDatabase {
            report_duplicates,
            ..Default::default()
        });
        let devices = Arc::new(MemoryDeviceStore::default());
        let (queue, jobs) = PushQueue::new();
        let sender = Arc::new(RecordingSender {
            database: inner.clone(),
            sent: Mutex::default(),
        });
        let worker = spawn_worker(jobs, inner.clone(), sender.clone());
        let database = PushingDatabase::new(
            inner,
            devices.clone(),
            queue,
            "wss://relay.example.com".to_string(),
        );
        Setup {
            database,
            sender,
            worker,
            devices,
        }
    }

    impl Setup {
        async fn register(&self, key: PublicKey, platform: Platform, token: &str) {
            let device = Device {
                key,
                platform,
                token: token.to_string(),
            };
            self.devices.upsert(&device).await.unwrap();
        }

        /// Stops the queue and waits for the worker to send everything queued
        async fn drain(self) -> Vec<(PushPayload, Vec<Device>, bool)> {
            drop(self.database);
            self.worker.await.unwrap();
            self.sender.sent.lock().unwrap().clone()
        }
    }

    fn event(kind: Kind, recipient: &PublicKey) -> Event {
        EventBuilder::new(kind, "sealed")
            .tag(Tag::public_key(*recipient))
            .finalize(&Keys::generate())
            .unwrap()
    }

    #[tokio::test]
    async fn push_queue_gift_wrap_for_registered_recipient_sends_payload() {
        let recipient = Keys::generate().public_key();
        let setup = setup(true);
        setup.register(recipient, Platform::Android, "a1").await;
        setup.register(recipient, Platform::Ios, "i1").await;
        let wrap = event(Kind::GiftWrap, &recipient);

        setup.database.save_event(&wrap).await.unwrap();
        let sent = setup.drain().await;

        assert_eq!(sent.len(), 1);
        let (payload, devices, _) = &sent[0];
        assert_eq!(
            payload,
            &PushPayload {
                event_id: wrap.id,
                relay_url: "wss://relay.example.com".to_string(),
                kind: 1059,
            }
        );
        assert_eq!(devices.len(), 2);
        assert!(devices.iter().all(|d| d.key == recipient));
    }

    #[tokio::test]
    async fn push_queue_duplicate_save_enqueues_one_job() {
        let recipient = Keys::generate().public_key();
        let setup = setup(true);
        setup.register(recipient, Platform::Android, "a1").await;
        let wrap = event(Kind::GiftWrap, &recipient);

        setup.database.save_event(&wrap).await.unwrap();
        let second = setup.database.save_event(&wrap).await.unwrap();
        let sent = setup.drain().await;

        assert!(!second.is_success());
        assert_eq!(sent.len(), 1);
    }

    #[tokio::test]
    async fn push_queue_event_id_queued_once_when_store_accepts_duplicates() {
        let recipient = Keys::generate().public_key();
        let setup = setup(false);
        setup.register(recipient, Platform::Android, "a1").await;
        let wrap = event(Kind::GiftWrap, &recipient);

        setup.database.save_event(&wrap).await.unwrap();
        setup.database.save_event(&wrap).await.unwrap();
        let sent = setup.drain().await;

        assert_eq!(sent.len(), 1);
    }

    #[tokio::test]
    async fn push_queue_other_kinds_enqueue_none() {
        let recipient = Keys::generate().public_key();
        let setup = setup(true);
        setup.register(recipient, Platform::Android, "a1").await;

        for kind in [Kind::TextNote, Kind::Seal, Kind::PrivateDirectMessage] {
            setup
                .database
                .save_event(&event(kind, &recipient))
                .await
                .unwrap();
        }

        assert!(setup.drain().await.is_empty());
    }

    #[tokio::test]
    async fn push_queue_unregistered_recipient_enqueues_none() {
        let registered = Keys::generate().public_key();
        let stranger = Keys::generate().public_key();
        let setup = setup(true);
        setup.register(registered, Platform::Ios, "i1").await;

        setup
            .database
            .save_event(&event(Kind::GiftWrap, &stranger))
            .await
            .unwrap();

        assert!(setup.drain().await.is_empty());
    }

    #[tokio::test]
    async fn push_queue_event_fetchable_when_sender_runs() {
        let recipient = Keys::generate().public_key();
        let setup = setup(true);
        setup.register(recipient, Platform::Android, "a1").await;
        let wraps: Vec<_> = (0..5).map(|_| event(Kind::GiftWrap, &recipient)).collect();

        for wrap in &wraps {
            setup.database.save_event(wrap).await.unwrap();
        }
        let sent = setup.drain().await;

        assert_eq!(sent.len(), wraps.len());
        assert!(sent.iter().all(|(_, _, fetchable)| *fetchable));
    }

    #[tokio::test]
    async fn push_queue_job_for_unstored_event_is_not_sent() {
        let recipient = Keys::generate().public_key();
        let (queue, jobs) = PushQueue::new();
        let inner: Arc<dyn NostrDatabase> = Arc::new(TestDatabase::default());
        let sender = Arc::new(RecordingSender {
            database: inner.clone(),
            sent: Mutex::default(),
        });
        let worker = spawn_worker(jobs, inner, sender.clone());
        let wrap = event(Kind::GiftWrap, &recipient);

        queue.enqueue(PushJob {
            payload: PushPayload {
                event_id: wrap.id,
                relay_url: "wss://relay.example.com".to_string(),
                kind: 1059,
            },
            devices: vec![],
        });
        drop(queue);
        worker.await.unwrap();

        assert!(sender.sent.lock().unwrap().is_empty());
    }
}
