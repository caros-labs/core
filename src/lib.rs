//! Local process supervision for CarOS. No app protocols or display code live here.

#[cfg(not(unix))]
compile_error!("CarOS process supervision currently requires Unix process groups.");

pub mod microphone;
pub mod services;
pub mod supervisor;

pub mod api;
pub use caros_protocol::AppId;
