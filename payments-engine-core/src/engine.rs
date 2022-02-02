use crate::{
    account::Account,
    common::ClientId,
    store::StoreError,
    transaction::{Transaction, TransactionId},
};
use async_trait::async_trait;
use thiserror::Error;

/// The [`Engine`] is responsible for processing all the transactions.
/// It also provides a way to get the current state of all the accounts.
#[async_trait]
pub trait Engine: Send + Sync {
    /// Process a single transaction.
    async fn process_transaction(&self, transaction: Transaction) -> EngineResult<Account>;
    /// Get the current state of all the accounts.
    async fn report(&self)
        -> EngineResult<Box<dyn futures::Stream<Item = Account> + Unpin + Send>>;
}

/// Result for [`Engine`] operations.
pub type EngineResult<T> = Result<T, EngineError>;

/// Error type for [`Engine`] operations.
#[derive(Debug, Error, PartialEq)]
pub enum EngineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("Insufficient available funds")]
    InsufficientAvailableFunds,
    #[error("Insufficient held funds")]
    InsufficientHeldFunds,
    #[error("The referenced transaction {id} was not a deposit")]
    WrongTransactionRef { id: TransactionId },
    #[error(
        "The referenced transaction {id} with client {client} was not from client {wrong_client}"
    )]
    TransactionRefWrongClient {
        id: TransactionId,
        client: ClientId,
        wrong_client: ClientId,
    },
    #[error("Transaction with id {id} has negative amount")]
    NegativeAmountTransaction { id: TransactionId },
    #[error("Transaction with id {id} it's already under dispute")]
    DoubleDispute { id: TransactionId },
    #[error("Tried to apply transaction with id {tx} to a locked account {id}")]
    LockedAccount { id: ClientId, tx: TransactionId },
    #[error("Unknwon error: {0}")]
    UnknownError(String),
    #[error("Transaction was unable to complete. You may have unstable state.")]
    TransactionNotCommited(StoreError),
}
