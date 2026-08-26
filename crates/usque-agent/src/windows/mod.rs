//! Windows-specific service, caller authentication, and network backend.
//!
//! The backend is split into small modules so code that mutates Wintun, WFP,
//! routes, DNS, or WinINet state can be audited independently.

pub mod auth;
pub mod backend;
pub mod network;
pub mod packet_session;
pub mod server;
pub mod service_config;
pub mod state_security;
pub mod system_proxy;
pub mod wfp;
pub mod wintun;
