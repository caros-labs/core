//! Trusted-user management API. Socket ownership and peer UID are enforced.
//! Apps under the same UID share this trust boundary; per-app capabilities and
//! sandboxing are deliberately not claimed by this first management endpoint.
mod connection;
mod dispatch;
pub mod manifest;
mod mapping;
mod service;
mod socket;

pub use service::CoreServer;
