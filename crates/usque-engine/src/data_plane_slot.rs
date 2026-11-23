//! Keep the protected session recoverable while macOS replaces its native runtime.
use crate::ActiveDataPlane;
use tokio::sync::{MappedMutexGuard, Mutex, MutexGuard, TryLockError, oneshot};

#[derive(Default)]
pub(crate) struct DataPlaneSlot(Mutex<Publication>);

#[derive(Default)]
struct Publication {
    active: Option<ActiveDataPlane>,
    returning: Option<oneshot::Receiver<ActiveDataPlane>>,
}

impl Publication {
    fn reclaim(&mut self) {
        if let Some(receiver) = &mut self.returning
            && let Ok(active) = receiver.try_recv()
        {
            self.active = Some(active);
            self.returning = None;
        }
    }
}

impl DataPlaneSlot {
    pub(crate) async fn lock(&self) -> MappedMutexGuard<'_, Option<ActiveDataPlane>> {
        self.lock_for_status().await.0
    }

    pub(crate) async fn lock_for_status(
        &self,
    ) -> (MappedMutexGuard<'_, Option<ActiveDataPlane>>, bool) {
        let mut publication = self.0.lock().await;
        publication.reclaim();
        let replacing = publication.returning.is_some();
        (MutexGuard::map(publication, |p| &mut p.active), replacing)
    }

    pub(crate) fn try_lock(
        &self,
    ) -> Result<MappedMutexGuard<'_, Option<ActiveDataPlane>>, TryLockError> {
        let mut publication = self.0.try_lock()?;
        publication.reclaim();
        Ok(MutexGuard::map(publication, |p| &mut p.active))
    }

    /// Lifecycle mutations are serialized by mutation_lock. Status and new
    /// connection intents can still observe and cancel an in-flight replacement.
    pub(crate) async fn borrow_vpn_for_replacement(&self) -> Option<ReplacementOwner> {
        let mut publication = self.0.lock().await;
        publication.reclaim();
        let active = publication
            .active
            .take_if(|active| active.runtime.is_vpn())?;
        let (sender, receiver) = oneshot::channel();
        publication.returning = Some(receiver);
        Some(ReplacementOwner(Some((active, sender))))
    }
}

pub(crate) struct ReplacementOwner(Option<(ActiveDataPlane, oneshot::Sender<ActiveDataPlane>)>);

impl ReplacementOwner {
    pub(crate) fn active(&mut self) -> &mut ActiveDataPlane {
        let (active, _) = self
            .0
            .as_mut()
            .expect("replacement owns the session until drop");
        active
    }
}

impl Drop for ReplacementOwner {
    fn drop(&mut self) {
        if let Some((active, sender)) = self.0.take() {
            // Synchronous even if the RPC future was cancelled mid-await.
            let _ = sender.send(active);
        }
    }
}
