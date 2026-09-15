//! Offline validation of a downloaded mirror. Opens no network socket or TUN.
use std::collections::BTreeMap;
use std::io::Read;
use usque_core::vpngate::{Catalogue, MAX_DIRECTORY_BYTES, Selection};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("expected a servers.json path")?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((MAX_DIRECTORY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let catalogue = Catalogue::parse(&bytes)?;
    let mut counts = BTreeMap::new();
    let list = catalogue.list(&usque_core::vpngate::ListQuery {
        include_unsupported: true,
        limit: 100,
        ..Default::default()
    });
    let mut accepted = 0;
    for offset in (0..list.total).step_by(100) {
        for server in catalogue
            .list(&usque_core::vpngate::ListQuery {
                include_unsupported: true,
                offset,
                limit: 100,
                ..Default::default()
            })
            .servers
        {
            if let Some(reason) = server.unsupported_reason {
                *counts.entry(format!("{reason:?}")).or_insert(0) += 1;
                continue;
            }
            let selection = Selection {
                server_id: server.id.clone(),
                config_sha256: server.config_sha256.clone(),
            };
            let (_, prepared) = catalogue.prepare(&selection)?;
            let mut native = usque_openvpn::Session::start(prepared.content(), prepared.remote)?;
            let validated = tokio::time::timeout(std::time::Duration::from_secs(3), async {
                loop {
                    match native.next_event().await? {
                        usque_openvpn::Event::Dial { .. } => {
                            return Ok::<_, usque_openvpn::Error>(());
                        }
                        usque_openvpn::Event::State { error: true, .. }
                        | usque_openvpn::Event::Stopped => {
                            return Err(usque_openvpn::Error::InvalidConfig);
                        }
                        _ => {}
                    }
                }
            })
            .await;
            native.shutdown().await?;
            validated??;
            accepted += 1;
        }
    }
    println!(
        "Validated records: {}; native-compatible TCP profiles: {accepted}; unsupported: {counts:?}",
        list.total
    );
    Ok(())
}
