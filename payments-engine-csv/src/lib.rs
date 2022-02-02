//! Library to asynchronously read CSV transactions from a stream and write the final account balances.
//!
//! It exposes a couple of functions for read [`read_csv_async`] and write [`write_csv_async`].
//!
//! The transactions must be in CSV format and must abide to the following structure:
//!
//! ```csv
//! type,client,tx,amount
//! deposit,1,1,100
//! withdrawal,1,2,50
//! deposit,2,3,100
//! ```
//!
//! Note that the reader is a little bit flexible with the columns and that `amount` is totally optional for some of the transaction types.
#![allow(clippy::module_name_repetitions)]

mod reader;
mod transaction;
mod writer;

pub use reader::{read_csv_async, AsyncReader};
pub use writer::{write_csv_async, AsyncWriter};
