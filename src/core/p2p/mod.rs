//! Online play without port forwarding: the host's "Open to LAN" world is
//! reached through an EasyTier room (peer-to-peer where NAT allows, relayed
//! by shared nodes otherwise), and shows up in the guest's LAN list.
pub mod easytier;
pub mod health;
pub mod lan;
pub mod room;
pub mod scaffolding;
