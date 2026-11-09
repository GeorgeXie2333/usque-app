//! Session-owned advertising rules. Readers retain one complete verified revision.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use usque_geo::AdsRules;

#[derive(Default)]
struct State {
    enabled: bool,
    rules: Option<Arc<AdsRules>>,
}

#[derive(Default)]
pub(crate) struct LiveAds {
    state: RwLock<State>,
    cache_dir: Option<PathBuf>,
    stamp: Mutex<Option<Option<(SystemTime, u64)>>>,
    watching: AtomicBool,
}

impl LiveAds {
    pub(crate) fn snapshot(cache_dir: &Path, enabled: bool) -> Arc<Self> {
        let ads = Arc::new(Self {
            cache_dir: Some(cache_dir.to_owned()),
            ..Default::default()
        });
        ads.set_enabled(enabled);
        ads
    }
    pub(crate) fn watch(cache_dir: &Path, enabled: bool) -> Arc<Self> {
        let ads = Arc::new(Self {
            cache_dir: Some(cache_dir.to_owned()),
            ..Default::default()
        });
        ads.set_enabled(enabled);
        ads.start_watching();
        ads
    }

    pub(crate) fn start_watching(self: &Arc<Self>) {
        // Android downloads in the UI process while its session lives in :vpn.
        // Observe atomic cache replacements without touching platform networking.
        // The weak reference releases the watcher when its session is gone.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            if self.watching.swap(true, Ordering::AcqRel) {
                return;
            }
            let weak = Arc::downgrade(self);
            runtime.spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let Some(ads) = weak.upgrade() else { break };
                    if tokio::task::spawn_blocking(move || ads.refresh())
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
    }

    pub(crate) fn supported(&self) -> bool {
        self.cache_dir.is_some()
    }

    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.state
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .enabled = enabled;
        if enabled {
            self.refresh();
        }
    }

    pub(crate) fn refresh(&self) {
        if !self.state.read().unwrap_or_else(|e| e.into_inner()).enabled {
            return;
        }
        let Some(cache_dir) = &self.cache_dir else {
            return;
        };
        let next = std::fs::metadata(usque_geo::global_geosite_cache_path(cache_dir))
            .ok()
            .and_then(|meta| Some((meta.modified().ok()?, meta.len())));
        let mut stamp = self.stamp.lock().unwrap_or_else(|e| e.into_inner());
        if *stamp == Some(next) {
            return;
        }
        *stamp = Some(next);
        // Missing/corrupt replacements never erase an active good revision.
        let loaded = if self
            .state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .rules
            .is_some()
        {
            AdsRules::load_primary(cache_dir)
        } else {
            AdsRules::load(cache_dir)
        };
        if let Ok(rules) = loaded {
            self.state.write().unwrap_or_else(|e| e.into_inner()).rules = Some(Arc::new(rules));
        }
    }

    pub(crate) fn rules(&self) -> Option<Arc<AdsRules>> {
        let state = self.state.read().unwrap_or_else(|e| e.into_inner());
        state.enabled.then(|| state.rules.clone()).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GeoDirectPolicy, GeoRoute};

    fn catalog(host: &str) -> Vec<u8> {
        // Small, real GeoSite wire fixture: suffix category-ads-all.
        let mut domain = vec![8, 2, 18, u8::try_from(host.len()).unwrap()];
        domain.extend_from_slice(host.as_bytes());
        let mut site = vec![10, 16];
        site.extend_from_slice(b"category-ads-all");
        site.extend_from_slice(&[18, u8::try_from(domain.len()).unwrap()]);
        site.extend(domain);
        let mut bytes = vec![10, u8::try_from(site.len()).unwrap()];
        bytes.extend(site);
        bytes
    }

    #[test]
    fn live_revisions_and_toggles_preserve_exceptions_and_last_good_rules() {
        let dir = tempfile::tempdir().unwrap();
        let path = usque_geo::global_geosite_cache_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let profile = usque_core::Profile {
            routing: usque_core::RoutingSettings {
                ads_enabled: false,
                rules: vec![usque_core::RoutingRule {
                    id: uuid::Uuid::new_v4(),
                    kind: usque_core::RoutingMatch::Domain,
                    target: "safe.ads.test".into(),
                    action: usque_core::RoutingAction::Proxy,
                }],
            },
            ..Default::default()
        };
        let policy = GeoDirectPolicy::disabled()
            .with_custom_rules(&profile)
            .unwrap()
            .with_ads(dir.path());
        let existing_consumer = policy.clone();
        assert!(policy.supports_live_ads());
        std::fs::write(&path, catalog("ads.test")).unwrap();
        policy.update_ads_enabled(true);
        assert_eq!(existing_consumer.route_host("ads.test"), GeoRoute::Reject);
        assert_eq!(
            existing_consumer.route_host("safe.ads.test"),
            GeoRoute::Tunnel
        );
        let old_revision = policy.ads_revision().unwrap();
        std::fs::write(&path, b"invalid").unwrap();
        policy.update_ads_enabled(true);
        assert_eq!(
            policy.ads_revision().as_deref(),
            Some(old_revision.as_str())
        );
        assert_eq!(existing_consumer.route_host("ads.test"), GeoRoute::Reject);
        std::fs::write(&path, catalog("new-advertising.test")).unwrap();
        policy.update_ads_enabled(true);
        assert_ne!(policy.ads_revision().unwrap(), old_revision);
        assert_eq!(existing_consumer.route_host("ads.test"), GeoRoute::Tunnel);
        assert_eq!(
            existing_consumer.route_host("new-advertising.test"),
            GeoRoute::Reject
        );
        policy.update_ads_enabled(false);
        assert_eq!(
            existing_consumer.route_host("new-advertising.test"),
            GeoRoute::Tunnel
        );
        assert!(policy.ads_revision().is_none());
    }

    #[test]
    fn damaged_primary_keeps_newer_memory_rules_instead_of_loading_old_backup() {
        for missing in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = usque_geo::global_geosite_cache_path(dir.path());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                dir.path().join("geo/geosite/ads-last-good.dat"),
                catalog("old.test"),
            )
            .unwrap();
            std::fs::write(&path, catalog("current.test")).unwrap();
            let ads = LiveAds::watch(dir.path(), true);
            let revision = ads.rules().unwrap().revision().to_owned();
            if missing {
                std::fs::remove_file(&path).unwrap();
            } else {
                std::fs::write(&path, b"invalid").unwrap();
            }
            ads.refresh();
            assert_eq!(ads.rules().unwrap().revision(), revision);
            assert!(ads.rules().unwrap().contains("current.test"));
            assert!(!ads.rules().unwrap().contains("old.test"));
            // A fresh session still recovers from the last verified disk backup.
            let fresh = LiveAds::watch(dir.path(), true);
            assert!(fresh.rules().unwrap().contains("old.test"));
            std::fs::write(&path, catalog("repaired.test")).unwrap();
            ads.refresh();
            fresh.refresh();
            assert!(ads.rules().unwrap().contains("repaired.test"));
            assert!(fresh.rules().unwrap().contains("repaired.test"));
        }
    }

    #[test]
    fn watcher_loads_cross_process_cache_changes_after_jni_thread_handoff() {
        let dir = tempfile::tempdir().unwrap();
        let ads = LiveAds::watch(dir.path(), true);
        assert!(!ads.watching.load(Ordering::Acquire));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            ads.start_watching();
            ads.start_watching(); // Reused consumers must not start duplicate watchers.
            assert!(ads.watching.load(Ordering::Acquire));
            let weak = Arc::downgrade(&ads);
            let path = usque_geo::global_geosite_cache_path(dir.path());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, catalog("ads.test")).unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                while ads.rules().is_none() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            drop(ads);
            tokio::time::timeout(Duration::from_secs(5), async {
                while weak.upgrade().is_some() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        });
    }
}
