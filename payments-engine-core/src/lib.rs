#![allow(clippy::module_name_repetitions)]

//! Core types and traits for [payments-engine]
//!
//! Library authors that want to provide [`engine::Engine`] or [`store::Store`] implementations should use this crate.
pub mod engine;
mod models;
pub mod store;

// re-exporting decimal macros
pub use models::*;
pub use rust_decimal_macros::dec;
