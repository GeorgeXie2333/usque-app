//! Darwin privilege boundary. Never reuse Windows/Linux resource identities.
//! Production availability remains gated on externally observed acceptance.

pub use usque_platform::darwin_packet as packet;
pub mod auth;
pub mod backend;
pub mod boot;
pub mod command;
pub mod dns;
pub mod firewall;
pub mod journal;
pub mod network;
pub mod pf_reference;
pub mod resources;
pub mod routes;
pub mod server;
pub mod socket;
mod system_proxy;
pub mod transaction;
pub mod utun;

pub const INSTALL_DIRECTORY: &str = "/Library/PrivilegedHelperTools/com.usque.agent";
pub const STATE_DIRECTORY: &str = "/Library/Application Support/Usque/Agent";
pub const SOCKET_PATH: &str = "/Library/PrivilegedHelperTools/com.usque.agent/control.sock";
pub const ENGINE_PATH: &str = "/Library/PrivilegedHelperTools/com.usque.agent/usque-engine";
pub const JOURNAL_PATH: &str = "/Library/Application Support/Usque/Agent/network.json";
pub const INSTALL_RECORD_PATH: &str = "/Library/PrivilegedHelperTools/com.usque.agent/install.json";
pub const RESOURCE_PATH: &str = "/Library/Application Support/Usque/Agent/resources.json";
pub const MACOS_PROTOCOL_VERSION: u32 = 1;
// Change only with source-bound external acceptance on both architectures.
// No environment variable, GUI preference or install manifest can override it.
pub const RELEASE_APPROVED: bool = false;

/// Also recorded in each signed Mach-O so cross-built packages can verify
/// both helpers without executing them or relying on a mutable bundle plist.
#[used]
// SAFETY: this dedicated read-only Mach-O section contains one boolean byte.
#[unsafe(link_section = "__TEXT,__usque_tun")]
pub static TEST_TUN_ENABLED: bool = cfg!(feature = "macos-test-tun");
