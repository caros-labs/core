//! Core-owned provider processes with exclusive, administrator-defined resources.
//! Reservations coordinate cooperating providers; they are not an OS sandbox.
mod registry;
pub use registry::{ServiceRegistry, ServiceSpec};
