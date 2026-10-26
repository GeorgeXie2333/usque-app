//! Volatile affinity for one user connection, independent of latency-cache TTL.
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Clones belong to the same connection, including automatic startup retries.
/// A new user connection gets a fresh instance; nothing here is serialized.
#[derive(Clone, Default)]
pub struct ConnectionSelection(Arc<Mutex<Option<Selected>>>);

struct Selected {
    account: Uuid,
    preference: Option<String>,
    node: String,
}

impl ConnectionSelection {
    pub fn selected(&self, account: Uuid, preference: Option<&str>) -> Option<String> {
        let mut saved = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if saved.as_ref().is_some_and(|selected| {
            selected.account != account || selected.preference.as_deref() != preference
        }) {
            *saved = None;
        }
        saved.as_ref().map(|selected| selected.node.clone())
    }

    pub fn remember(&self, account: Uuid, preference: Option<&str>, node: &str) {
        *self.0.lock().unwrap_or_else(|error| error.into_inner()) = Some(Selected {
            account,
            preference: preference.map(str::to_owned),
            node: node.to_owned(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_share_affinity_but_new_connections_and_preferences_do_not() {
        let account = Uuid::new_v4();
        let mut connection = ConnectionSelection::default();
        let retry = connection.clone();
        connection.remember(account, None, "original");
        assert_eq!(retry.selected(account, None).as_deref(), Some("original"));

        connection = ConnectionSelection::default();
        // A late completion from the old connection cannot pin the new one.
        retry.remember(account, None, "late");
        assert_eq!(connection.selected(account, None), None);
        connection.remember(account, None, "automatic");
        assert_eq!(connection.selected(account, Some("manual")), None);
        assert_eq!(connection.selected(account, None), None);
        connection.remember(account, None, "automatic");
        assert_eq!(connection.selected(Uuid::new_v4(), None), None);
    }
}
