use super::{Device, Platform};
use crate::db::PostgresStore;
use async_trait::async_trait;
use nostr::key::PublicKey;

/// Push device registrations: at most one token per key and platform
#[async_trait]
pub trait DeviceStoreApi: Send + Sync {
    /// Stores the device's token for its key and platform, replacing any earlier token
    async fn upsert(&self, device: &Device) -> Result<(), anyhow::Error>;
    /// Removes the registration of `key` and `platform` if it holds `token`; returns whether
    /// one was removed
    async fn remove(&self, device: &Device) -> Result<bool, anyhow::Error>;
    /// The registrations of `key`, one per platform at most
    async fn devices_for(&self, key: &PublicKey) -> Result<Vec<Device>, anyhow::Error>;
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

    async fn devices_for(&self, key: &PublicKey) -> Result<Vec<Device>, anyhow::Error> {
        let rows = self
            .pool
            .get()
            .await?
            .query(
                "SELECT platform, token FROM push_devices WHERE pubkey = $1",
                &[&key.to_hex()],
            )
            .await?;
        Ok(rows
            .iter()
            .filter_map(|row| {
                let platform: String = row.get(0);
                Some(Device {
                    key: *key,
                    platform: Platform::parse(&platform)?,
                    token: row.get(1),
                })
            })
            .collect())
    }
}

#[cfg(test)]
pub use memory::MemoryDeviceStore;

#[cfg(test)]
mod memory {
    use super::*;
    use std::{collections::HashMap, sync::Mutex};

    /// Registrations keyed like the `push_devices` primary key
    #[derive(Default)]
    pub struct MemoryDeviceStore {
        devices: Mutex<HashMap<(PublicKey, Platform), String>>,
    }

    impl MemoryDeviceStore {
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

        async fn devices_for(&self, key: &PublicKey) -> Result<Vec<Device>, anyhow::Error> {
            let devices = self.devices.lock().unwrap();
            Ok(devices
                .iter()
                .filter(|((k, _), _)| k == key)
                .map(|((k, platform), token)| Device {
                    key: *k,
                    platform: *platform,
                    token: token.clone(),
                })
                .collect())
        }
    }
}
