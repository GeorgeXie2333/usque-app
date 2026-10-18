//! Linux privilege boundary. Linux resource identities and journals are kept
//! separate from Windows SID, HANDLE, LUID and WFP state.

pub mod auth;
pub mod backend;
mod command;
pub mod coordinator;
pub mod dns;
pub mod firewall;
pub mod journal;
pub mod network;
pub mod server;
pub mod transport;

pub const SOCKET_PATH: &str = "/run/usque/agent.sock";
pub const INSTALL_RECORD_PATH: &str = "/etc/usque/install.json";
pub const ENGINE_PATH: &str = "/usr/lib/usque/usque-engine";
pub const JOURNAL_PATH: &str = "/var/lib/usque/agent-linux.json";
