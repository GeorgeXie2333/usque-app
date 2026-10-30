//! Provider-owned macOS VPN prototype. The Engine retains vault ownership.
//!
//! Compilation and isolated tests do not certify Network Extension deployment.
//! Production capability remains false until signed route/leak tests pass.
pub mod backend;
pub mod controller;
pub mod credential_rpc;
pub mod credentials;
pub mod ffi;
pub mod policy;
pub mod protocol;

pub use controller::Controller;
pub use protocol::*;
