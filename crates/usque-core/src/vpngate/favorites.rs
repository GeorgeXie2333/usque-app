//! Durable local snapshots and favorite membership are independent of pool refreshes.
use super::*;
use tokio_util::sync::CancellationToken;

// Both platforms have one service writer. Serialize its independently constructed
// stores too; atomically published files remain safe for concurrent readers.
pub(super) static STORE_WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());
const MAX_FAVORITE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FavoriteMetadata {
    pub config_sha256: String,
    pub saved_at_unix_ms: u64,
    pub latest_config_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeAction {
    #[default]
    Prepare,
    Favorite,
    UpdateFavorite,
    RemoveFavorite,
    Release,
    Cancel,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct NodeRequest {
    pub operation_id: String,
    pub action: NodeAction,
    #[serde(default)]
    pub server_id: String,
    #[serde(default)]
    pub config_sha256: String,
    #[serde(default)]
    pub expected_favorite_hash: String,
}
impl NodeRequest {
    pub fn validate(&self) -> Result<(), DirectoryError> {
        if uuid::Uuid::parse_str(&self.operation_id).is_err()
            || (self.action != NodeAction::Cancel
                && (!valid_id(&self.server_id) || !valid_hash(&self.config_sha256)))
            || (!self.expected_favorite_hash.is_empty()
                && !valid_hash(&self.expected_favorite_hash))
            || (self.action == NodeAction::UpdateFavorite && self.expected_favorite_hash.is_empty())
            || (self.action == NodeAction::Favorite && !self.expected_favorite_hash.is_empty())
        {
            return Err(DirectoryError::StaleSelection);
        }
        Ok(())
    }
    pub fn selection(&self) -> Selection {
        Selection {
            server_id: self.server_id.clone(),
            config_sha256: self.config_sha256.clone(),
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeProgress {
    pub operation_id: String,
    pub server_id: String,
    pub config_sha256: String,
    pub stage: String,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct StoredNode {
    version: u32,
    wire: WireServer,
    pool: Option<PoolMetadata>,
}
impl StoredNode {
    fn summary(&self) -> Result<ServerSummary, DirectoryError> {
        if self.version != 2 || !self.wire.openvpn_config_base64.is_empty() {
            return Err(DirectoryError::InvalidDirectory);
        }
        let mut summary = server_summary(&self.wire, validate_server_metadata(&self.wire)?, None);
        summary.pool = self.pool.clone();
        Ok(summary)
    }
    fn matches(&self, selection: &Selection) -> bool {
        self.wire.id == selection.server_id
            && self.wire.openvpn_config_sha256 == selection.config_sha256
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Favorite {
    node: StoredNode,
    saved_at_unix_ms: u64,
}
#[derive(Serialize, Deserialize)]
struct Favorites {
    version: u32,
    revision: u64,
    entries: BTreeMap<String, Favorite>,
}
impl Default for Favorites {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            entries: BTreeMap::new(),
        }
    }
}
#[derive(Serialize, Deserialize)]
struct ConfigObject {
    base64: String,
}

/// Each download owns a separate reference. Its cleanup must never release a
/// prepared UI draft, even when both refer to the same configuration object.
pub(super) struct NodePreparation<'a> {
    store: &'a CatalogueStore,
    selection: Selection,
    path: PathBuf,
}
impl NodePreparation<'_> {
    pub(super) fn prepare_local(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Option<ServerSummary>, DirectoryError> {
        self.store
            .prepare_local_at(&self.selection, &self.path, cancel)
    }
    pub(super) fn stage_configuration(
        &self,
        summary: ServerSummary,
        wire: WireServer,
        cancel: &CancellationToken,
    ) -> Result<ServerSummary, DirectoryError> {
        if summary.id != self.selection.server_id
            || summary.config_sha256 != self.selection.config_sha256
        {
            return Err(DirectoryError::StaleSelection);
        }
        self.store
            .stage_configuration_at(summary, wire, &self.path, cancel)
    }
    fn node(&self) -> Result<StoredNode, DirectoryError> {
        let bytes =
            bounded_read(&self.path, MAX_CONFIG_JSON_BYTES).map_err(|_| DirectoryError::CacheIo)?;
        let node: StoredNode =
            serde_json::from_slice(&bytes).map_err(|_| DirectoryError::InvalidDirectory)?;
        if !node.matches(&self.selection) {
            return Err(DirectoryError::StaleSelection);
        }
        self.store.validate_node(&node)?;
        Ok(node)
    }
    pub(super) fn retain_draft(&self, cancel: &CancellationToken) -> Result<(), DirectoryError> {
        let _guard = STORE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
        let node = self.node()?;
        if cancel.is_cancelled() {
            return Err(DirectoryError::Cancelled);
        }
        atomic_json(&self.store.node_path("prepared", &self.selection), &node)
    }
    pub(super) fn set_favorite(
        &self,
        request: &NodeRequest,
        revision: u64,
        cancel: &CancellationToken,
    ) -> Result<(), DirectoryError> {
        let _guard = STORE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
        self.store
            .set_favorite_locked(request, revision, cancel, || self.node())
    }
}
impl Drop for NodePreparation<'_> {
    fn drop(&mut self) {
        self.store.release_reference(&self.path);
    }
}

impl CatalogueStore {
    pub(super) fn prepare_operation(
        &self,
        selection: &Selection,
    ) -> Result<NodePreparation<'_>, DirectoryError> {
        if !valid_id(&selection.server_id) || !valid_hash(&selection.config_sha256) {
            return Err(DirectoryError::StaleSelection);
        }
        Ok(NodePreparation {
            store: self,
            selection: selection.clone(),
            path: self
                .directory
                .join("preparing")
                .join(format!("{}.json", uuid::Uuid::new_v4())),
        })
    }
    pub(super) fn node_path(&self, category: &str, selection: &Selection) -> PathBuf {
        self.directory.join(category).join(format!(
            "{}-{}.json",
            &selection.server_id[3..],
            selection.config_sha256
        ))
    }
    fn object_path(&self, hash: &str) -> PathBuf {
        self.directory.join("objects").join(format!("{hash}.json"))
    }
    fn favorites(&self) -> Result<Favorites, DirectoryError> {
        let bytes = match bounded_read(&self.directory.join("favorites.json"), MAX_DIRECTORY_BYTES)
        {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Favorites::default()),
            Err(_) => return Err(DirectoryError::CacheIo),
        };
        let favorites: Favorites =
            serde_json::from_slice(&bytes).map_err(|_| DirectoryError::InvalidDirectory)?;
        if favorites.version != 1 || favorites.entries.len() > MAX_SERVERS {
            return Err(DirectoryError::InvalidDirectory);
        }
        for (id, entry) in &favorites.entries {
            if *id != entry.node.summary()?.id {
                return Err(DirectoryError::InvalidDirectory);
            }
        }
        Ok(favorites)
    }
    pub fn favorite_revision(&self, request: &NodeRequest) -> Result<u64, DirectoryError> {
        let favorites = self.favorites()?;
        let existing = favorites
            .entries
            .get(&request.server_id)
            .map(|f| f.node.wire.openvpn_config_sha256.as_str())
            .unwrap_or("");
        if existing != request.expected_favorite_hash {
            return Err(DirectoryError::FavoriteChanged);
        }
        Ok(favorites.revision)
    }
    pub(super) fn local_node(&self, selection: &Selection) -> Result<StoredNode, DirectoryError> {
        if !valid_id(&selection.server_id) || !valid_hash(&selection.config_sha256) {
            return Err(DirectoryError::StaleSelection);
        }
        for category in ["selected", "prepared"] {
            if let Ok(bytes) =
                bounded_read(&self.node_path(category, selection), MAX_CONFIG_JSON_BYTES)
                && let Ok(node) = serde_json::from_slice::<StoredNode>(&bytes)
                && node.matches(selection)
            {
                node.summary()?;
                return Ok(node);
            }
        }
        self.favorites()?
            .entries
            .get(&selection.server_id)
            .filter(|f| f.node.matches(selection))
            .map(|f| f.node.clone())
            .ok_or(DirectoryError::StaleSelection)
    }
    pub(super) fn validate_node(
        &self,
        node: &StoredNode,
    ) -> Result<(ServerSummary, PreparedProfile), DirectoryError> {
        let mut summary = node.summary()?;
        let bytes = bounded_read(
            &self.object_path(&summary.config_sha256),
            MAX_CONFIG_JSON_BYTES,
        )
        .map_err(|_| DirectoryError::CacheIo)?;
        let object: ConfigObject =
            serde_json::from_slice(&bytes).map_err(|_| DirectoryError::InvalidDirectory)?;
        let mut wire = node.wire.clone();
        wire.openvpn_config_base64 = object.base64;
        let (_, content) = validate_server(&wire)?;
        let profile = prepare_profile(&content, summary.ip)
            .map_err(|_| DirectoryError::UnsupportedProfile)?;
        summary.unsupported_reason = None;
        Ok((summary, profile))
    }
    fn stage_configuration_at(
        &self,
        summary: ServerSummary,
        mut wire: WireServer,
        reference: &Path,
        cancel: &CancellationToken,
    ) -> Result<ServerSummary, DirectoryError> {
        let _guard = STORE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
        if cancel.is_cancelled() {
            return Err(DirectoryError::Cancelled);
        }
        let (_, content) = validate_server(&wire)?;
        prepare_profile(&content, summary.ip).map_err(|_| DirectoryError::UnsupportedProfile)?;
        let object = ConfigObject {
            base64: std::mem::take(&mut wire.openvpn_config_base64),
        };
        atomic_json(&self.object_path(&summary.config_sha256), &object)?;
        if cancel.is_cancelled() {
            return Err(DirectoryError::Cancelled);
        }
        let node = StoredNode {
            version: 2,
            wire,
            pool: summary.pool.clone(),
        };
        atomic_json(reference, &node)?;
        Ok(summary)
    }
    fn prepare_local_at(
        &self,
        selection: &Selection,
        reference: &Path,
        cancel: &CancellationToken,
    ) -> Result<Option<ServerSummary>, DirectoryError> {
        if let Ok(node) = self.local_node(selection) {
            let (summary, _) = self.validate_node(&node)?;
            // Hold this operation's own reference before another local action
            // can release the source favorite or draft.
            let _guard = STORE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
            if cancel.is_cancelled() {
                return Err(DirectoryError::Cancelled);
            }
            self.validate_node(&node)?;
            atomic_json(reference, &node)?;
            return Ok(Some(summary));
        }
        // Read old inline snapshots and old cached catalogues without rewriting them.
        let old = bounded_read(&self.selection_path(selection), MAX_CONFIG_JSON_BYTES)
            .ok()
            .and_then(|b| serde_json::from_slice::<WireServer>(&b).ok())
            .filter(|s| {
                s.id == selection.server_id && s.openvpn_config_sha256 == selection.config_sha256
            })
            .or_else(|| {
                self.load()
                    .ok()
                    .flatten()
                    .and_then(|(c, ..)| c.selected(selection).ok().cloned())
            });
        if let Some(wire) = old {
            let (summary, _) = validate_server(&wire)?;
            return self
                .stage_configuration_at(summary, wire, reference, cancel)
                .map(Some);
        }
        Ok(None)
    }
    pub fn set_favorite(
        &self,
        request: &NodeRequest,
        revision: u64,
        cancel: &CancellationToken,
    ) -> Result<(), DirectoryError> {
        let _guard = STORE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
        self.set_favorite_locked(request, revision, cancel, || {
            self.local_node(&request.selection())
        })
    }
    fn set_favorite_locked(
        &self,
        request: &NodeRequest,
        revision: u64,
        cancel: &CancellationToken,
        read_node: impl FnOnce() -> Result<StoredNode, DirectoryError>,
    ) -> Result<(), DirectoryError> {
        let mut favorites = self.favorites()?;
        if favorites.revision != revision || self.favorite_revision(request)? != revision {
            return Err(DirectoryError::FavoriteChanged);
        }
        if cancel.is_cancelled() {
            return Err(DirectoryError::Cancelled);
        }
        let node = read_node()?;
        if !node.matches(&request.selection()) {
            return Err(DirectoryError::StaleSelection);
        }
        self.validate_node(&node)?;
        let time = favorites
            .entries
            .get(&request.server_id)
            .map(|f| f.saved_at_unix_ms)
            .unwrap_or_else(super::download::now_ms);
        favorites.entries.insert(
            request.server_id.clone(),
            Favorite {
                node,
                saved_at_unix_ms: time,
            },
        );
        if favorites.entries.len() > MAX_SERVERS {
            return Err(DirectoryError::FavoriteLimit);
        }
        let mut hashes = HashSet::new();
        let mut total = 0_u64;
        for favorite in favorites.entries.values() {
            let hash = &favorite.node.wire.openvpn_config_sha256;
            if hashes.insert(hash) {
                total = total.saturating_add(
                    std::fs::metadata(self.object_path(hash))
                        .map_err(|_| DirectoryError::CacheIo)?
                        .len(),
                );
                if total > MAX_FAVORITE_BYTES {
                    return Err(DirectoryError::FavoriteLimit);
                }
            }
        }
        if cancel.is_cancelled() {
            return Err(DirectoryError::Cancelled);
        }
        favorites.revision = favorites
            .revision
            .checked_add(1)
            .ok_or(DirectoryError::FavoriteLimit)?;
        if serde_json::to_vec(&favorites)
            .map_err(|_| DirectoryError::CacheIo)?
            .len()
            > MAX_DIRECTORY_BYTES
        {
            return Err(DirectoryError::FavoriteLimit);
        }
        atomic_json(&self.directory.join("favorites.json"), &favorites)
    }
    pub fn remove_favorite(
        &self,
        request: &NodeRequest,
        retained: &[Selection],
    ) -> Result<(), DirectoryError> {
        request.validate()?;
        let _guard = STORE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
        self.favorite_revision(request)?;
        let mut favorites = self.favorites()?;
        favorites.entries.remove(&request.server_id);
        // Increment even if an in-flight addition has not yet created the entry.
        favorites.revision = favorites
            .revision
            .checked_add(1)
            .ok_or(DirectoryError::FavoriteLimit)?;
        atomic_json(&self.directory.join("favorites.json"), &favorites)?;
        // Membership has committed. A best-effort cache sweep must not report
        // an unsuccessful removal after the favorite was already removed.
        let _ = self.collect_objects(&favorites, retained);
        Ok(())
    }
    fn collect_objects(
        &self,
        favorites: &Favorites,
        retained: &[Selection],
    ) -> Result<(), DirectoryError> {
        let mut hashes: HashSet<String> = favorites
            .entries
            .values()
            .map(|f| f.node.wire.openvpn_config_sha256.clone())
            .collect();
        hashes.extend(retained.iter().map(|s| s.config_sha256.clone()));
        // Pins are independent durable references, including legacy selections.
        for category in ["selected", "prepared", "preparing"] {
            let directory = self.directory.join(category);
            if let Ok(entries) = std::fs::read_dir(directory) {
                for entry in entries {
                    let entry = entry.map_err(|_| DirectoryError::CacheIo)?;
                    if let Ok(bytes) = bounded_read(&entry.path(), MAX_CONFIG_JSON_BYTES)
                        && let Ok(node) = serde_json::from_slice::<StoredNode>(&bytes)
                        && node.summary().is_ok()
                    {
                        hashes.insert(node.wire.openvpn_config_sha256);
                    }
                }
            }
        }
        if let Ok(entries) = std::fs::read_dir(self.directory.join("objects")) {
            for entry in entries {
                let entry = entry.map_err(|_| DirectoryError::CacheIo)?;
                let name = entry.file_name();
                if let Some(hash) = name.to_str().and_then(|n| n.strip_suffix(".json"))
                    && valid_hash(hash)
                    && !hashes.contains(hash)
                {
                    std::fs::remove_file(entry.path()).map_err(|_| DirectoryError::CacheIo)?;
                }
            }
        }
        Ok(())
    }
    pub fn release_prepared(&self, selection: &Selection) {
        if valid_id(&selection.server_id) && valid_hash(&selection.config_sha256) {
            self.release_reference(&self.node_path("prepared", selection));
        }
    }
    fn release_reference(&self, reference: &Path) {
        let _guard = STORE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
        let _ = std::fs::remove_file(reference);
        if let Ok(favorites) = self.favorites() {
            let _ = self.collect_objects(&favorites, &[]);
        }
    }
    /// Called after settings commit, with the saved and live session references.
    /// Failure only defers garbage collection; it never rolls back saved settings.
    pub fn retain_selections(&self, retained: &[Selection]) -> Result<(), DirectoryError> {
        let _guard = STORE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
        let favorites = self.favorites()?;
        if let Ok(entries) = std::fs::read_dir(self.directory.join("selected")) {
            for entry in entries {
                let entry = entry.map_err(|_| DirectoryError::CacheIo)?;
                let bytes = bounded_read(&entry.path(), MAX_CONFIG_JSON_BYTES)
                    .map_err(|_| DirectoryError::CacheIo)?;
                let node: StoredNode =
                    serde_json::from_slice(&bytes).map_err(|_| DirectoryError::InvalidDirectory)?;
                node.summary()?;
                if !retained.iter().any(|selection| node.matches(selection)) {
                    std::fs::remove_file(entry.path()).map_err(|_| DirectoryError::CacheIo)?;
                }
            }
        }
        self.collect_objects(&favorites, retained)
    }
    /// Only the service owner calls this, before any preparation worker starts.
    pub fn recover_references(&self, retained: &[Selection]) -> Result<(), DirectoryError> {
        {
            let _guard = STORE_WRITE.lock().unwrap_or_else(|e| e.into_inner());
            for category in ["prepared", "preparing"] {
                let Ok(entries) = std::fs::read_dir(self.directory.join(category)) else {
                    continue;
                };
                for entry in entries {
                    let entry = entry.map_err(|_| DirectoryError::CacheIo)?;
                    let bytes = bounded_read(&entry.path(), MAX_CONFIG_JSON_BYTES)
                        .map_err(|_| DirectoryError::CacheIo)?;
                    let node: StoredNode = serde_json::from_slice(&bytes)
                        .map_err(|_| DirectoryError::InvalidDirectory)?;
                    node.summary()?;
                    std::fs::remove_file(entry.path()).map_err(|_| DirectoryError::CacheIo)?;
                }
            }
        }
        self.retain_selections(retained)
    }
    pub(super) fn list_with_favorites(
        &self,
        query: &ListQuery,
    ) -> Result<ServerList, DirectoryError> {
        if query.status_only {
            return Ok(ServerList::default());
        }
        let favorites = self.favorites()?;
        let pool = self.load_pool();
        // A broken/absent downloadable cache must not hide durable favorites.
        let mut list = if query.favorites_only {
            ServerList::default()
        } else if let Some((catalogue, time, source, _)) = pool.as_ref().map_err(Clone::clone)? {
            ServerList {
                servers: catalogue
                    .nodes
                    .iter()
                    .map(|s| s.summary())
                    .collect::<Result<Vec<_>, _>>()?,
                source_server_count: catalogue.nodes.len(),
                fetched_at_unix_ms: Some(*time),
                source_url: Some(source.clone()),
                source_fetched_at: Some(catalogue.index.source_fetched_at.clone()),
                ..Default::default()
            }
        } else if let Some((catalogue, time, source)) = self.load()? {
            ServerList {
                servers: catalogue.servers,
                source_server_count: catalogue.document.server_count,
                fetched_at_unix_ms: Some(time),
                source_url: Some(source),
                ..Default::default()
            }
        } else {
            ServerList::default()
        };
        let current: BTreeMap<_, _> = pool
            .as_ref()
            .ok()
            .and_then(|p| p.as_ref())
            .map(|(p, ..)| p.nodes.iter().map(|n| (n.wire.id.as_str(), n)).collect())
            .unwrap_or_default();
        if query.favorites_only {
            list.servers = favorites
                .entries
                .values()
                .map(|f| f.node.summary())
                .collect::<Result<_, _>>()?;
            if let Ok(Some((p, time, source, _))) = &pool {
                list.source_server_count = p.nodes.len();
                list.fetched_at_unix_ms = Some(*time);
                list.source_url = Some(source.clone());
                list.source_fetched_at = Some(p.index.source_fetched_at.clone());
            }
        }
        for server in &mut list.servers {
            if let Some(favorite) = favorites.entries.get(&server.id) {
                let latest = current.get(server.id.as_str());
                server.favorite = Some(FavoriteMetadata {
                    config_sha256: favorite.node.wire.openvpn_config_sha256.clone(),
                    saved_at_unix_ms: favorite.saved_at_unix_ms,
                    latest_config_sha256: latest
                        .filter(|n| {
                            n.wire.openvpn_config_sha256 != favorite.node.wire.openvpn_config_sha256
                        })
                        .map(|n| n.wire.openvpn_config_sha256.clone()),
                });
                if query.favorites_only {
                    if let Some(latest) =
                        latest.filter(|n| n.wire.openvpn_config_sha256 == server.config_sha256)
                    {
                        server.pool = latest.summary()?.pool;
                    } else if let Some(metadata) = &mut server.pool {
                        metadata.in_pool = latest.is_some();
                    }
                }
            }
        }
        list.favorite_count = favorites.entries.len();
        if query.favorites_only {
            list.servers.sort_by(|a, b| {
                b.favorite
                    .as_ref()
                    .map(|f| f.saved_at_unix_ms)
                    .cmp(&a.favorite.as_ref().map(|f| f.saved_at_unix_ms))
                    .then(a.id.cmp(&b.id))
            });
        } else {
            list.servers
                .sort_by(|a, b| b.score.cmp(&a.score).then(a.id.cmp(&b.id)));
        }
        list.servers
            .retain(|s| query.include_unsupported || s.unsupported_reason.is_none());
        let mut countries = BTreeMap::<Option<String>, CountrySummary>::new();
        for server in &list.servers {
            let country = countries
                .entry(server.country_code.clone())
                .or_insert_with(|| CountrySummary {
                    country_code: server.country_code.clone(),
                    country_name: server.country_name.clone(),
                    server_count: 0,
                });
            country.server_count += 1;
        }
        list.countries = countries.into_values().collect();
        list.servers.retain(|s| {
            if query.unknown_country {
                s.country_code.is_none()
            } else {
                query
                    .country_code
                    .as_ref()
                    .is_none_or(|code| s.country_code.as_ref() == Some(code))
            }
        });
        list.total = list.servers.len();
        let limit = if query.limit == 0 {
            100
        } else {
            query.limit.min(100)
        };
        list.servers = list
            .servers
            .into_iter()
            .skip(query.offset)
            .take(limit)
            .collect();
        Ok(list)
    }
}
