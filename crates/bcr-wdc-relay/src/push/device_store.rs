use super::{Device, Platform};
use crate::db::PostgresStore;
use async_trait::async_trait;
use nostr::{event::EventId, key::PublicKey};

#[async_trait]
pub trait DeviceStoreApi: Send + Sync {
    async fn upsert(&self, device: &Device) -> Result<(), anyhow::Error>;
    async fn remove(&self, device: &Device) -> Result<bool, anyhow::Error>;
    async fn devices_for(&self, keys: &[PublicKey]) -> Result<Vec<Device>, anyhow::Error>;
    async fn record_submission(
        &self,
        event_id: &EventId,
        device: &Device,
        error: Option<&str>,
    ) -> Result<(), anyhow::Error>;
}

#[async_trait]
impl DeviceStoreApi for PostgresStore {
    async fn upsert(&self, device: &Device) -> Result<(), anyhow::Error> {
        self.pool
            .get()
            .await?
            .execute(
                "INSERT INTO push_devices (pubkey, platform, token, updated_at) \
                 VALUES ($1, $2, $3, now()) \
                 ON CONFLICT (pubkey, platform) \
                 DO UPDATE SET token = EXCLUDED.token, updated_at = EXCLUDED.updated_at",
                &[
                    &device.key.to_hex(),
                    &device.platform.as_str(),
                    &device.token,
                ],
            )
            .await?;
        Ok(())
    }

    async fn remove(&self, device: &Device) -> Result<bool, anyhow::Error> {
        let removed = self
            .pool
            .get()
            .await?
            .execute(
                "DELETE FROM push_devices WHERE pubkey = $1 AND platform = $2 AND token = $3",
                &[
                    &device.key.to_hex(),
                    &device.platform.as_str(),
                    &device.token,
                ],
            )
            .await?;
        Ok(removed > 0)
    }

    async fn devices_for(&self, keys: &[PublicKey]) -> Result<Vec<Device>, anyhow::Error> {
        let keys: Vec<String> = keys.iter().map(PublicKey::to_hex).collect();
        let rows = self
            .pool
            .get()
            .await?
            .query(
                "SELECT pubkey, platform, token FROM push_devices WHERE pubkey = ANY($1)",
                &[&keys],
            )
            .await?;
        Ok(rows
            .iter()
            .filter_map(|row| {
                let key: String = row.get(0);
                let platform: String = row.get(1);
                Some(Device {
                    key: PublicKey::from_hex(key.trim()).ok()?,
                    platform: Platform::parse(&platform)?,
                    token: row.get(2),
                })
            })
            .collect())
    }

    async fn record_submission(
        &self,
        event_id: &EventId,
        device: &Device,
        error: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        self.pool
            .get()
            .await?
            .execute(
                "INSERT INTO push_submissions (event_id, pubkey, platform, error, submitted_at) \
                 VALUES ($1, $2, $3, $4, now()) \
                 ON CONFLICT (event_id, pubkey, platform) \
                 DO UPDATE SET error = EXCLUDED.error, submitted_at = EXCLUDED.submitted_at",
                &[
                    &event_id.to_hex(),
                    &device.key.to_hex(),
                    &device.platform.as_str(),
                    &error,
                ],
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
pub use memory::MemoryDeviceStore;

#[cfg(test)]
mod memory {
    use super::*;
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Default)]
    pub struct MemoryDeviceStore {
        devices: Mutex<HashMap<(PublicKey, Platform), String>>,
        submissions: Mutex<Vec<(EventId, Device, Option<String>)>>,
    }

    impl MemoryDeviceStore {
        pub fn submissions(&self) -> Vec<(EventId, Device, Option<String>)> {
            self.submissions.lock().unwrap().clone()
        }

        pub fn tokens(&self) -> Vec<(PublicKey, Platform, String)> {
            let devices = self.devices.lock().unwrap();
            devices
                .iter()
                .map(|((key, platform), token)| (*key, *platform, token.clone()))
                .collect()
        }
    }

    #[async_trait]
    impl DeviceStoreApi for MemoryDeviceStore {
        async fn upsert(&self, device: &Device) -> Result<(), anyhow::Error> {
            self.devices
                .lock()
                .unwrap()
                .insert((device.key, device.platform), device.token.clone());
            Ok(())
        }

        async fn remove(&self, device: &Device) -> Result<bool, anyhow::Error> {
            let mut devices = self.devices.lock().unwrap();
            let slot = (device.key, device.platform);
            if devices.get(&slot) == Some(&device.token) {
                devices.remove(&slot);
                return Ok(true);
            }
            Ok(false)
        }

        async fn devices_for(&self, keys: &[PublicKey]) -> Result<Vec<Device>, anyhow::Error> {
            let devices = self.devices.lock().unwrap();
            Ok(devices
                .iter()
                .filter(|((k, _), _)| keys.contains(k))
                .map(|((k, platform), token)| Device {
                    key: *k,
                    platform: *platform,
                    token: token.clone(),
                })
                .collect())
        }

        async fn record_submission(
            &self,
            event_id: &EventId,
            device: &Device,
            error: Option<&str>,
        ) -> Result<(), anyhow::Error> {
            self.submissions.lock().unwrap().push((
                *event_id,
                device.clone(),
                error.map(str::to_owned),
            ));
            Ok(())
        }
    }
}
