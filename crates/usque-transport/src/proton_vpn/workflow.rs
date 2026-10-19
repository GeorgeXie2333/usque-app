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
use std::path::Path;
use tokio_util::sync::CancellationToken;
use usque_core::{
    chain_exit::{
        ChainSource,
        store::{ChainProfileStore, ProfileCipher},
    },
    proton_vpn::{Record, Request, SessionSecrets, store::SessionStore},
};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct Credentials {
    username: String,
    password: String,
}

fn frame() -> Value {
    // Independent identity and truthful, minimal platform facts. No Android App
    // identity, model, attestation or fabricated human input is substituted.
    let platform = std::env::consts::OS;
    let description = format!("{platform}/usque {}", std::env::consts::ARCH);
    let mut weight = 1i64;
    let mut device = 0i64;
    for byte in description.bytes() {
        device += (byte as i8 as i64) * weight;
        weight = weight * 7 % 100000007;
    }
    json!({"Payload": { format!("usque-{platform}-v4-challenge-0"): {"v":"2.0.7","deviceName":device} }})
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
    interaction.phase("srp_info");
    let info = client
        .request(
            Method::POST,
            "/auth/v4/info",
            &Json(json!({"Username":credentials.username,"Intent":"Proton"})).0,
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
    interaction.phase("srp_proof");
    let mut payload = Json(frame());
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
    let response = client
        .request(Method::POST, "/auth/v4", &payload.0, interaction, cancel)
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
pub(super) async fn guest(
    client: &mut Client,
    interaction: &dyn Interaction,
    cancel: &CancellationToken,
) -> Result<(), Failure> {
    interaction.phase("anonymous_session");
    let response = client
        .request(
            Method::POST,
            "/auth/v4/sessions",
            &Json(frame()).0,
            interaction,
            cancel,
        )
        .await?;
    client.accept_session(&response.0)?;
    interaction.phase("credentialless");
    let response = client
        .request(
            Method::POST,
            "/auth/v4/credentialless",
            &Json(frame()).0,
            interaction,
            cancel,
        )
        .await?;
    client.accept_session(&response.0)?;
    Ok(())
}
pub(super) async fn permissions(
    client: &mut Client,
    interaction: &dyn Interaction,
    cancel: &CancellationToken,
    expected_user: Option<&str>,
) -> Result<u32, Failure> {
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
    let id = token(response.pointer("/User/ID"))?;
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
    client.accept_session(&response.0)
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
    pub cipher: &'a dyn ProfileCipher,
}
pub(super) async fn provision(
    client: &mut Client,
    request: &Request,
    input: Zeroizing<Vec<u8>>,
    storage: Storage<'_>,
    interaction: &dyn Interaction,
    cancel: &CancellationToken,
) -> Result<Uuid, Failure> {
    let Storage { parent, cipher } = storage;
    let generation = client.generation();
    client.checkpoint(generation, cancel)?;
    let sessions = SessionStore::new(parent, cipher);
    let existing_id = if request.action == "refresh" {
        Some(
            request
                .profile_id
                .ok_or_else(|| Failure::new("missing_profile"))?,
        )
    } else {
        None
    };
    let _lease = existing_id.map(|id| sessions.lease(id)).transpose()?;
    let mut existing = existing_id.map(|id| sessions.load(id)).transpose()?;
    let max_tier = if let Some(record) = existing.as_mut() {
        if record.rotation_pending {
            return Err(Failure::new("reauthentication_required"));
        }
        client.session = record.session.clone();
        record.rotation_pending = true;
        client.checkpoint(generation, cancel)?;
        sessions.save(record)?;
        refresh_session(client, interaction, cancel).await?;
        client.checkpoint(generation, cancel)?;
        record.max_tier =
            permissions(client, interaction, cancel, Some(&record.session.user_id)).await?;
        record.session = client.session.clone();
        record.rotation_pending = false;
        client.checkpoint(generation, cancel)?;
        sessions.save(record)?;
        record.max_tier
    } else {
        if request.action == "login" {
            login(client, input, interaction, cancel).await?;
        } else {
            guest(client, interaction, cancel).await?;
        }
        permissions(client, interaction, cancel, None).await?;
        let expected = Zeroizing::new(client.session.user_id.clone());
        refresh_session(client, interaction, cancel).await?;
        permissions(client, interaction, cancel, Some(&expected)).await?
    };
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
    let nodes = nodes::collect(&directory.0, &config.0)?;
    let selected = interaction
        .node(nodes.iter().map(|n| n.metadata.clone()).collect(), cancel)
        .await?;
    let node = nodes
        .into_iter()
        .find(|n| n.metadata.id == selected)
        .ok_or_else(|| Failure::new("invalid_node"))?;
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
    let certificate = certificate(client, &keys, interaction, cancel).await?;
    let secrets = nodes::configuration(&keys, &node)?;
    if cancel.is_cancelled() {
        return Err(Failure::new("cancelled"));
    }
    if !client.revoke_retired(&CancellationToken::new()).await {
        return Err(Failure::new("cleanup_failed_remote_state_unknown"));
    }
    client.checkpoint(generation, cancel)?;
    interaction.phase("secure_storage");
    let chain = ChainProfileStore::new(parent, cipher);
    let summary = if let Some(record) = &existing {
        if record.node.endpoint == node.endpoint && record.node.public_key == node.public_key {
            chain
                .list()?
                .into_iter()
                .find(|p| {
                    p.id == record.id
                        && p.revision == record.revision
                        && p.source == ChainSource::ProtonVpn
                })
                .ok_or_else(|| Failure::new("profile_revision_mismatch"))?
        } else {
            chain.replace_proton(record.id, record.revision, secrets)?
        }
    } else {
        chain.import(
            ChainSource::ProtonVpn,
            if request.name.is_empty() {
                "Proton VPN"
            } else {
                &request.name
            },
            secrets,
        )?
    };
    let record = Record {
        version: 1,
        id: summary.id,
        revision: summary.revision,
        name: summary.name.clone(),
        identity: existing
            .as_ref()
            .map_or_else(|| request.action.clone(), |r| r.identity.clone()),
        session: client.session.clone(),
        ed25519_seed: Zeroizing::new(keys.ed25519_seed().to_vec()),
        certificate_chain: certificate.pem,
        expires_at: certificate.expiration_time,
        refresh_at: certificate.refresh_time,
        max_tier,
        node,
        rotation_pending: false,
    };
    if let Err(failure) = client
        .checkpoint(generation, cancel)
        .map_err(|f| usque_core::proton_vpn::error(&f.reason))
        .and_then(|()| sessions.save(&record))
    {
        if existing.is_none() {
            let _ = chain.remove(summary.id, summary.edit_revision, &[]);
        }
        return Err(failure.into());
    }
    // Persisting the session deliberately transfers ownership to the encrypted
    // account record. A generated session-mode configuration must remain live.
    client.session = SessionSecrets::default();
    Ok(summary.id)
}

pub(super) async fn logout(
    client: &mut Client,
    id: Uuid,
    parent: &Path,
    cipher: &dyn ProfileCipher,
    interaction: &dyn Interaction,
    cancel: &CancellationToken,
) -> Result<Uuid, Failure> {
    let sessions = SessionStore::new(parent, cipher);
    let _lease = sessions.lease(id)?;
    let mut record = sessions.load(id)?;
    record.rotation_pending = true;
    sessions.save(&record)?;
    client.session = record.session.clone();
    interaction.phase("session_revoke");
    if !client.revoke_all(cancel).await {
        return Err(Failure::new("revocation_failed_remote_state_unknown"));
    }
    let chain = ChainProfileStore::new(parent, cipher);
    let summary = chain
        .list()?
        .into_iter()
        .find(|s| s.id == id && s.source == ChainSource::ProtonVpn)
        .ok_or_else(|| Failure::new("profile_not_found"))?;
    chain.remove(id, summary.edit_revision, &[])?;
    sessions.delete(id)?;
    Ok(Uuid::nil())
}

/// Explicit local-only removal. Never claims or attempts remote revocation.
/// Callers must retain the platform lifecycle lock and reject active/selected
/// references, just as for logout. Retrying after a partial deletion is safe.
pub(super) fn forget(id: Uuid, parent: &Path, cipher: &dyn ProfileCipher) -> Result<(), Failure> {
    let sessions = SessionStore::new(parent, cipher);
    let _lease = sessions.lease(id)?;
    let chain = ChainProfileStore::new(parent, cipher);
    if let Some(summary) = chain.list()?.into_iter().find(|s| s.id == id) {
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
