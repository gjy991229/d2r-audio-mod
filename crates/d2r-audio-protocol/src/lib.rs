//! Application-neutral contract shared by audio-telemetry Mod producers and consumers.
//!
//! This crate performs no game access, process capture, UI, configuration, or database work.

pub mod catalog;
pub mod item_catalog;
pub mod protocol;
pub mod rune_data;

pub use protocol::PROTOCOL_VERSION;
