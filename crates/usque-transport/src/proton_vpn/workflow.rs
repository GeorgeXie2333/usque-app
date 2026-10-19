use super::{
    Failure, Interaction,
    api::{Client, Json, token},
    crypto::{KeyMaterial, SrpProofs, validate_certificate},
    interaction::now,
    nodes,
};
use http::Method;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
use tokio_util::sync::CancellationToken;
use usque_core::{
    chain_exit::{
        ChainSource,
        store::{ChainProfileStore, ProfileCipher},
    },
    proton_vpn::{Record, Request, RevocationHandle, SessionSecrets, store::SessionStore},
};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct Credentials {
    username: String,
    password: String,
}

async fn login_request(
    client: &mut Client,
    path: &str,
    body: &Value,
    phase: &str,
    interaction: &dyn Interaction,
    cancel: &CancellationToken,
) -> Result<Json, Failure> {
    if !client.session.user_id.is_empty() {
        return Err(Failure::new("unexpected_authenticated_session"));
    }
    if client.uncertain {
        return Err(Failure::new("session_state_unknown"));
    }
    let generation = client.generation();
    client.checkpoint(generation, cancel)?;
    interaction.phase(phase);
    match client
        .request(Method::POST, path, body, interaction, cancel)
        .await
    {
        Err(failure)
            if failure.status == Some(401) && failure.code != Some(8002) && !client.uncertain =>
        {
            client.checkpoint(generation, cancel)?;
            // Core's outer API handler recovers an unauthenticated context on
            // HTTP 401. This client owns only the current setup job's tokens;
            // saved authenticated sessions must retain their rotation journal.
            if client.session.uid.is_empty() {
                anonymous_session(client, interaction, cancel).await?;
            } else {
                refresh_session(client, interaction, cancel).await?;
            }
            client.checkpoint(generation, cancel)?;
            interaction.phase(phase);
            // One retry with the same body, without carrying forward a previous
            // request's human-verification headers.
            client
                .request(Method::POST, path, body, interaction, cancel)
                .await
        }
        result => result,
    }
}

pub(super) async fn login(
    client: &mut Client,
    mut input: Zeroizing<Vec<u8>>,
    interaction: &dyn Interaction,
    cancel: &CancellationToken,
) -> Result<(), Failure> {
    let mut credentials: Credentials =
        serde_json::from_slice(&input).map_err(|_| Failure::new("invalid_credentials_input"))?;
    input.zeroize();
    if credentials.username.is_empty()
        || credentials.username.len() > 320
        || credentials.username.chars().any(char::is_control)
        || credentials.password.is_empty()
        || credentials.password.len() > 72
    {
        return Err(Failure::new("unsupported_credentials"));
    }
    let info = login_request(
        client,
        "/auth/v4/info",
        &Json(json!({"Username":credentials.username,"Intent":"Proton"})).0,
        "srp_info",
        interaction,
        cancel,
    )
    .await?;
    let proofs = SrpProofs::generate(
        info.get("Version")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| Failure::new("unsupported_srp_version"))?,
        &credentials.password,
        &token(info.get("Salt"))?,
        info.get("Modulus")
            .and_then(Value::as_str)
            .ok_or_else(|| Failure::new("invalid_srp_response"))?,
        &token(info.get("ServerEphemeral"))?,
    )?;
    credentials.password.zeroize();
    let mut payload = Json(client.config.payload(true));
    let fields = payload
        .0
        .as_object_mut()
        .ok_or_else(|| Failure::new("invalid_challenge"))?;
    fields.insert(
        "Username".into(),
        Value::String(credentials.username.clone()),
    );
    fields.insert(
        "ClientEphemeral".into(),
        Value::String(proofs.client_ephemeral().into()),
    );
    fields.insert(
        "ClientProof".into(),
        Value::String(proofs.client_proof().into()),
    );
    fields.insert(
        "SRPSession".into(),
        Value::String(token(info.get("SRPSession"))?),
    );
    let response = login_request(
        client,
        "/auth/v4",
        &payload.0,
        "srp_proof",
        interaction,
        cancel,
    )
    .await?;
    client.accept_session(&response.0)?;
    proofs.verify_server_proof(&token(response.get("ServerProof"))?)?;
    let second = response
        .get("2FA")
        .map(|second| second.get("Enabled").and_then(Value::as_u64));
    drop(response);
    drop(proofs);
    drop(payload);
    drop(info);
    drop(credentials);
    if let Some(second) = second {
        match second {
            Some(0) => (),
            Some(1) => {
                interaction.phase("totp");
                let code = interaction.totp(cancel).await?;
                client
                    .request(
                        Method::POST,
                        "/auth/v4/2fa",
                        &Json(json!({"TwoFactorCode":&*code})).0,
                        interaction,
                        cancel,
                    )
                    .await?;
            }
            _ => return Err(Failure::new("unsupported_second_factor")),
        }
    }
    Ok(())
}
async fn anonymous_session(
    client: &mut Client,
    interaction: &dyn Interaction,
    cancel: &CancellationToken,
) -> Result<(), Failure> {
    if !client.session.user_id.is_empty() {
        return Err(Failure::new("unexpected_authenticated_session"));
    }
    if client.uncertain {
        return Err(Failure::new("session_state_unknown"));
    }
    if client.session.uid.is_empty() {
        interaction.phase("anonymous_session");
        let response = client
            .request(
                Method::POST,
                "/auth/v4/sessions",
                &Json(client.config.payload(false)).0,
                interaction,
                cancel,
            )
            .await?;
        client.accept_session(&response.0)?;
        if !client.session.user_id.is_empty() {
            return Err(Failure::new("unexpected_authenticated_session"));
        }
    }
    Ok(())
}

pub(super) async fn guest(
    client: &mut Client,
    interaction: &dyn Interaction,
    cancel: &CancellationToken,
) -> Result<(), Failure> {
    anonymous_session(client, interaction, cancel).await?;
    interaction.phase("credentialless");
    let response = client
        .request(
            Method::POST,
            "/auth/v4/credentialless",
            &Json(client.config.payload(true)).0,
            interaction,
            cancel,
        )
        .await?;
    client.accept_session(&response.0)?;
    client.session.validate()?;
    Ok(())
}
pub(super) async fn permissions(
    client: &mut Client,
    interaction: &dyn Interaction,
    cancel: &CancellationToken,
    expected_user: Option<&str>,
    guest: bool,
) -> Result<u32, Failure> {
    let id = if guest {
        interaction.phase("session_scopes");
        let response = client
            .request(
                Method::GET,
                "/auth/v4/scopes",
                &Value::Null,
                interaction,
                cancel,
            )
            .await?;
        let scopes = response
            .get("Scopes")
            .and_then(Value::as_array)
            .filter(|scopes| scopes.len() <= 128)
            .ok_or_else(|| Failure::new("invalid_scopes_response"))?;
        for scope in scopes {
            token(Some(scope))?;
        }
        // Credentialless accounts have no ordinary Core user resource. Their
        // authenticated UserID comes from the login response; VPN permission
        // still has to be established by the protected /vpn/v2 request below.
        client.session.user_id.clone()
    } else {
        interaction.phase("protected_user");
        let response = client
            .request(
                Method::GET,
                "/core/v4/users",
                &Value::Null,
                interaction,
                cancel,
            )
            .await?;
        token(response.pointer("/User/ID"))?
    };
    if expected_user.is_some_and(|v| v != id)
        || !client.session.user_id.is_empty() && client.session.user_id != id
    {
        return Err(Failure::new("protected_user_mismatch"));
    }
    client.session.user_id = id;
    client.session.validate()?;
    interaction.phase("vpn_permissions");
    let response = client
        .request(Method::GET, "/vpn/v2", &Value::Null, interaction, cancel)
        .await?;
    if response.pointer("/VPN/Status").and_then(Value::as_u64) != Some(1)
        || response
            .pointer("/VPN/MaxConnect")
            .and_then(Value::as_u64)
            .is_none_or(|v| v < 1)
        || response.get("Delinquent").and_then(Value::as_u64) != Some(0)
    {
        return Err(Failure::new("vpn_permission_denied"));
    }
    response
        .pointer("/VPN/MaxTier")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| Failure::new("vpn_permission_denied"))
}
pub(super) async fn refresh_session(
    client: &mut Client,
    interaction: &dyn Interaction,
    cancel: &CancellationToken,
) -> Result<(), Failure> {
    interaction.phase("session_refresh");
    let payload = Json(
        json!({"UID":client.session.uid,"RefreshToken":client.session.refresh_token,"ResponseType":"token","GrantType":"refresh_token","RedirectURI":"http://protonmail.ch"}),
    );
    let response = client
        .request(
            Method::POST,
            "/auth/v4/refresh",
            &payload.0,
            interaction,
            cancel,
        )
        .await?;
    let expected_user = Zeroizing::new(client.session.user_id.clone());
    client.accept_session(&response.0)?;
    if !client.session.user_id.is_empty() && client.session.user_id != *expected_user {
        return Err(Failure::new("protected_user_mismatch"));
    }
    // Core refresh responses are SessionResponse, which need not contain a
    // UserID. Preserve the already authenticated account across token rotation.
    client.session.user_id.clone_from(&expected_user);
    Ok(())
}
pub(super) async fn certificate(
    client: &mut Client,
    keys: &KeyMaterial,
    interaction: &dyn Interaction,
    cancel: &CancellationToken,
) -> Result<super::crypto::Certificate, Failure> {
    interaction.phase("certificate");
    let response=client.request(Method::POST,"/vpn/v1/certificate",&Json(json!({"ClientPublicKey":keys.spki_pem(),"ClientPublicKeyMode":"EC","DeviceName":"Usque","Mode":"session","Features":[]})).0,interaction,cancel).await?;
    let pem = response
        .get("Certificate")
        .and_then(Value::as_str)
        .ok_or_else(|| Failure::new("invalid_certificate_response"))?;
    let expiry = response
        .get("ExpirationTime")
        .and_then(Value::as_i64)
        .ok_or_else(|| Failure::new("invalid_certificate_response"))?;
    let refresh = response
        .get("RefreshTime")
        .and_then(Value::as_i64)
        .ok_or_else(|| Failure::new("invalid_certificate_response"))?;
    Ok(validate_certificate(pem, keys, expiry, refresh, now())?)
}

pub(super) struct Storage<'a> {
    pub parent: &'a Path,
    pub cipher: Arc<dyn ProfileCipher>,
    pub accounts: super::AccountContext,
}
pub(super) async fn provision(
    client: &mut Client,
    request: &Request,
    input: Zeroizing<Vec<u8>>,
    storage: Storage<'_>,
    interaction: &dyn Interaction,
    cancel: &CancellationToken,
) -> Result<Uuid, Failure> {
    let Storage {
        parent,
        cipher,
        accounts,
    } = storage;
    let generation = client.generation();
    client.checkpoint(generation, cancel)?;
    let sessions = SessionStore::new(parent, &*cipher);
    if request.action == "refresh" {
        let id = request
            .profile_id
            .ok_or_else(|| Failure::new("missing_profile"))?;
        let _lease = sessions.lease(id)?;
        let mut record = sessions.load(id)?;
        if record.rotation_pending {
            return Err(Failure::new("reauthentication_required"));
        }
        client.session = record.session.clone();
        client.config.identity = record.api_identity.clone();
        let mut access = super::session::Access::new(
            super::manager::persistence(parent, cipher.clone(), id),
            client,
        );
        access.cleanup(client, &mut record, cancel).await?;
        access
            .permissions(client, &mut record, interaction, cancel)
            .await?;
        access
            .directory(client, &mut record, interaction, cancel)
            .await?;
        record.server_id = record.selected_server().map(str::to_owned);
        record.promote();
        access.save(&record, cancel).await?;
        client.session = SessionSecrets::default();
        return Ok(id);
    }
    let label = if request.action == "login" {
        let credentials: Credentials = serde_json::from_slice(&input)
            .map_err(|_| Failure::new("invalid_credentials_input"))?;
        let label = credentials.username.chars().take(64).collect::<String>();
        drop(credentials);
        login(client, input, interaction, cancel).await?;
        label
    } else {
        client.config = client.config.for_guest()?;
        guest(client, interaction, cancel).await?;
        "Proton guest".into()
    };
    let max_tier =
        permissions(client, interaction, cancel, None, request.action == "guest").await?;
    let permissions_at = super::interaction::now();
    let mut related: Vec<Record> = sessions
        .records()?
        .into_iter()
        .filter(|record| record.session.user_id == client.session.user_id)
        .collect();
    if related
        .iter()
        .any(|record| accounts.active.contains(&record.id))
    {
        return Err(Failure::new("profile_in_use"));
    }
    if request
        .profile_id
        .is_some_and(|id| !related.iter().any(|record| record.id == id))
    {
        return Err(Failure::new("protected_user_mismatch"));
    }
    related.sort_by_key(|record| {
        (
            request.profile_id != Some(record.id),
            accounts.selected != Some(record.id),
            record.rotation_pending,
            record.version == 1,
            record.id,
        )
    });
    let existing = related.into_iter().next();
    let _lease = existing
        .as_ref()
        .map(|record| sessions.lease(record.id))
        .transpose()?;
    interaction.phase("directory");
    let config = client
        .request(
            Method::GET,
            "/vpn/v2/clientconfig",
            &Value::Null,
            interaction,
            cancel,
        )
        .await?;
    let directory = client
        .request(
            Method::GET,
            "/vpn/v2/logicals?WithEntriesForProtocols=WireGuardUDP&WithState=true",
            &Value::Null,
            interaction,
            cancel,
        )
        .await?;
    let nodes = nodes::collect_for_tier(&directory.0, &config.0, max_tier)?;
    let keys = if let Some(record) = &existing {
        KeyMaterial::from_seed(
            record
                .ed25519_seed
                .as_slice()
                .try_into()
                .map_err(|_| Failure::new("invalid_saved_key"))?,
        )?
    } else {
        KeyMaterial::generate()?
    };
    // This peer binds the saved local profile/key. A fresh directory resolves
    // the actual peer only when connecting; it is never a saved authorization.
    let node = existing
        .as_ref()
        .map(|record| record.node.clone())
        .unwrap_or_else(|| nodes[0].clone());
    if !client.revoke_retired(&CancellationToken::new()).await {
        return Err(Failure::new("cleanup_failed_remote_state_unknown"));
    }
    client.checkpoint(generation, cancel)?;
    interaction.phase("secure_storage");
    let chain = ChainProfileStore::new(parent, &*cipher);
    let mut summary = if let Some(record) = &existing {
        chain
            .list()?
            .into_iter()
            .find(|profile| {
                profile.id == record.id
                    && profile.revision == record.revision
                    && profile.source == ChainSource::ProtonVpn
            })
            .ok_or_else(|| Failure::new("profile_revision_mismatch"))?
    } else {
        chain.import(
            ChainSource::ProtonVpn,
            if request.name.is_empty() {
                &label
            } else {
                &request.name
            },
            nodes::configuration(&keys, &node)?,
        )?
    };
    if existing.is_some() && (!request.name.is_empty() || summary.name == "Proton VPN") {
        summary = chain.rename(
            summary.id,
            summary.edit_revision,
            if request.name.is_empty() {
                &label
            } else {
                &request.name
            },
        )?;
    }
    let mut retired_sessions = existing
        .as_ref()
        .map(|record| record.retired_sessions.clone())
        .unwrap_or_default();
    if let Some(record) = &existing
        && record.session.uid != client.session.uid
        && !retired_sessions
            .iter()
            .any(|session| session.uid == record.session.uid)
        && let Some(handle) = RevocationHandle::capture(&record.session, &record.api_identity)
    {
        retired_sessions.push(handle);
    }
    retired_sessions.retain(|session| session.uid != client.session.uid);
    let mut record = Record {
        version: 2,
        id: summary.id,
        revision: summary.revision,
        name: summary.name.clone(),
        identity: request.action.clone(),
        api_identity: client.config.identity.clone(),
        session: client.session.clone(),
        ed25519_seed: Zeroizing::new(keys.ed25519_seed().to_vec()),
        certificate_chain: Zeroizing::new(String::new()),
        expires_at: 0,
        refresh_at: 0,
        max_tier,
        node,
        rotation_pending: false,
        server_id: existing
            .as_ref()
            .and_then(|record| record.selected_server().map(str::to_owned)),
        server_catalog: nodes.iter().map(|node| node.metadata.clone()).collect(),
        retired_sessions,
        catalog_revision: Uuid::new_v4(),
        tunnel_seed: existing
            .as_ref()
            .and_then(|record| record.tunnel_seed.clone()),
        key_recovery_at: existing.as_ref().map_or(0, |record| record.key_recovery_at),
        connection_cache: Some(usque_core::proton_vpn::ConnectionCache {
            permissions_at,
            ..Default::default()
        }),
    };
    record.promote();
    let selected = record.selected_server().map(str::to_owned);
    record
        .connection_cache
        .as_mut()
        .expect("new cache")
        .set_directory(&nodes, selected.as_deref(), super::interaction::now());
    if let Err(failure) = client
        .checkpoint(generation, cancel)
        .map_err(|failure| usque_core::proton_vpn::error(&failure.reason))
        .and_then(|()| sessions.save(&record))
    {
        if existing.is_none() {
            let _ = chain.remove(summary.id, summary.edit_revision, &[]);
        }
        return Err(failure.into());
    }
    // The durable account now owns the new session. Cleanup cannot revoke it
    // when an older, independently owned handle cannot be confirmed revoked.
    client.session = SessionSecrets::default();
    let access = super::session::Access::new(
        super::manager::persistence(parent, cipher, record.id),
        client,
    );
    access.cleanup(client, &mut record, cancel).await?;
    Ok(record.id)
}

fn removal_records(
    sessions: &SessionStore<'_>,
    chain: &ChainProfileStore<'_>,
    id: Uuid,
) -> Result<Vec<Record>, Failure> {
    let target = match sessions.load(id) {
        Ok(record) => record,
        Err(failure) if failure.reason == "session_not_found" => return Ok(Vec::new()),
        Err(failure) => return Err(failure.into()),
    };
    let mut records: Vec<_> = sessions
        .records()?
        .into_iter()
        .filter(|record| record.session.user_id == target.session.user_id)
        .collect();
    records.sort_by_key(|record| (record.id == id, record.id));
    let library = chain.list()?;
    for record in &records {
        if library
            .iter()
            .any(|summary| summary.id == record.id && summary.source != ChainSource::ProtonVpn)
        {
            return Err(Failure::new("invalid_profile_source"));
        }
    }
    Ok(records)
}
fn remove_local(
    sessions: &SessionStore<'_>,
    chain: &ChainProfileStore<'_>,
    id: Uuid,
) -> Result<(), Failure> {
    if let Some(summary) = chain.list()?.into_iter().find(|summary| summary.id == id) {
        if summary.source != ChainSource::ProtonVpn {
            return Err(Failure::new("invalid_profile_source"));
        }
        chain.remove(id, summary.edit_revision, &[])?;
    }
    match sessions.delete(id) {
        Ok(()) => Ok(()),
        Err(failure) if failure.reason == "session_not_found" => Ok(()),
        Err(failure) => Err(failure.into()),
    }
}

pub(super) async fn logout(
    client: &mut Client,
    id: Uuid,
    parent: &Path,
    cipher: &dyn ProfileCipher,
    interaction: &dyn Interaction,
    cancel: &CancellationToken,
) -> Result<Uuid, Failure> {
    let generation = client.generation();
    let sessions = SessionStore::new(parent, cipher);
    let chain = ChainProfileStore::new(parent, cipher);
    let mut records = removal_records(&sessions, &chain, id)?;
    let mut ids: Vec<_> = records.iter().map(|record| record.id).collect();
    ids.sort();
    let _leases: Vec<_> = ids
        .into_iter()
        .map(|id| sessions.lease(id))
        .collect::<Result<_, _>>()?;
    // Preserve every owned handle until remote revocation has been confirmed.
    // A partial failure remains explicit and can be followed by local removal.
    for record in &mut records {
        record.rotation_pending = true;
        sessions.save(record)?;
    }
    interaction.phase("session_revoke");
    let mut revoked = std::collections::BTreeSet::new();
    for record in records.iter().rev() {
        for handle in record
            .retired_sessions
            .iter()
            .cloned()
            .chain(RevocationHandle::capture(
                &record.session,
                &record.api_identity,
            ))
        {
            if !revoked.insert(handle.uid.clone()) {
                continue;
            }
            client.checkpoint(generation, cancel)?;
            client.config.identity = handle.api_identity.clone();
            client.session = handle.session();
            if !client.revoke_all(cancel).await {
                return Err(Failure::new("revocation_failed_remote_state_unknown"));
            }
        }
    }
    if records.is_empty() {
        remove_local(&sessions, &chain, id)?;
    } else {
        for record in records {
            remove_local(&sessions, &chain, record.id)?;
        }
    }
    Ok(Uuid::nil())
}

/// Explicit local-only removal of the account and its legacy profiles. Callers
/// retain the platform lifecycle lock and reject all active/selected references.
pub(super) fn forget(id: Uuid, parent: &Path, cipher: &dyn ProfileCipher) -> Result<(), Failure> {
    let sessions = SessionStore::new(parent, cipher);
    let chain = ChainProfileStore::new(parent, cipher);
    let records = removal_records(&sessions, &chain, id)?;
    let mut ids: Vec<_> = records.iter().map(|record| record.id).collect();
    if ids.is_empty() {
        ids.push(id);
    }
    let mut lock_ids = ids.clone();
    lock_ids.sort();
    let _leases: Vec<_> = lock_ids
        .into_iter()
        .map(|id| sessions.lease(id))
        .collect::<Result<_, _>>()?;
    for id in ids {
        remove_local(&sessions, &chain, id)?;
    }
    Ok(())
}
