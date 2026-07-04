use std::{
    collections::{HashMap, hash_map::Entry},
    sync::{Arc, Mutex, Weak},
};

use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct DownloadCoordinator {
    locks: Arc<Mutex<HashMap<Uuid, Weak<AsyncMutex<()>>>>>,
}

impl DownloadCoordinator {
    pub async fn acquire(&self, request_id: Uuid) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self
                .locks
                .lock()
                .expect("download coordinator lock map poisoned");

            locks.retain(|_, weak| weak.strong_count() > 0);

            match locks.entry(request_id) {
                Entry::Occupied(mut entry) => match entry.get().upgrade() {
                    Some(lock) => lock,
                    None => {
                        let lock = Arc::new(AsyncMutex::new(()));
                        entry.insert(Arc::downgrade(&lock));
                        lock
                    }
                },
                Entry::Vacant(entry) => {
                    let lock = Arc::new(AsyncMutex::new(()));
                    entry.insert(Arc::downgrade(&lock));
                    lock
                }
            }
        };

        lock.lock_owned().await
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use tokio::sync::oneshot;
    use uuid::Uuid;

    use super::DownloadCoordinator;

    #[tokio::test]
    async fn serializes_downloads_for_the_same_request() {
        let coordinator = DownloadCoordinator::default();
        let request_id = Uuid::new_v4();
        let (started_tx, started_rx) = oneshot::channel();

        let first = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                let _guard = coordinator.acquire(request_id).await;
                let _ = started_tx.send(());
                tokio::time::sleep(Duration::from_millis(150)).await;
            })
        };

        started_rx.await.expect("first task should start");

        let wait_started = Instant::now();
        let second = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                let _guard = coordinator.acquire(request_id).await;
                wait_started.elapsed()
            })
        };

        let waited = second.await.expect("second task should finish");
        first.await.expect("first task should finish");

        assert!(waited >= Duration::from_millis(140));
    }
}
