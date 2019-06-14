//! # The Network Manager Library
//!
//! The Network Manager Library provides the essential
//! functionality for configuring Network Manager from Rust.

#[macro_use]
extern crate error_chain;

#[macro_use]
extern crate log;

#[macro_use]
extern crate bitflags;

extern crate dbus;

extern crate ascii;

#[allow(deprecated)]
pub mod errors;

mod dbus_nm;
mod dbus_api;
mod manager;
mod service;
mod connection;
mod device;
mod wifi;
mod ssid;

pub use manager::{Connectivity, NetworkManager};
pub use connection::{Connection, ConnectionSettings, ConnectionState};
pub use device::{Device, DeviceState, DeviceType};
pub use wifi::{AccessPoint, AccessPointCredentials, Security};
pub use service::ServiceState;
