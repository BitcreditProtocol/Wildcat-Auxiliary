use super::Device;
use crate::db::PostgresStore;
use async_trait::async_trait;

/// Push device registrations: at most one token per key and platform
#[async_trait]
pub trait DeviceStoreApi: Send + Sync {
    /// Stores the device's token for its key and platform, replacing any earlier token
    async fn upsert(&self, device: &Device) -> Result<(), anyhow::Error>;
    /// Removes the registration of `key` and `platform` if it holds `token`; returns whether
    /// one was removed
    async fn remove(&self, device: &Device) -> Result<bool, anyhow::Error>;
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
}

#[cfg(test)]
pub use memory::MemoryDeviceStore;

#[cfg(test)]
mod memory {
    use super::*;
    use crate::push::Platform;
    use nostr::key::PublicKey;
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
    }
}
