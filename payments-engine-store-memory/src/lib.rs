//! In-Memory ['`Store`'] implementation.
//!
//! Useful for simple storage and for testing.
//!
//! Use the `testing` feature to enable some handy methods for testing purposes.
mod memory_store;

pub use memory_store::MemoryStore;
