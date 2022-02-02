use crate::{
    account::Account,
    common::ClientId,
    transaction::{Transaction, TransactionId},
};
use async_trait::async_trait;
use thiserror::Error;

/// Error type for [`Store`] implementations.
#[derive(Error, Debug, PartialEq, Eq)]
pub enum StoreError {
    #[error("Transaction with id {id} not found")]
    NotFound { id: TransactionId },
    #[error("Transaction with id {id} already exists")]
    AlreadyExists { id: TransactionId },
    #[error("Error while accessing the store: {0}")]
    AccessError(String),
    #[error("Unknwon error: {0}")]
    UnknownError(String),
}

pub type StoreResult<T> = Result<T, StoreError>;

/// The [`Store`] traits is an abstraction over the storage of the transactions and accounts.
#[async_trait]
pub trait Store: Send + Sync {
    /// Gets a [`Transaction`] by its id.
    /// If it doesn't exist, it returns an [`StoreError::NotFound].
    async fn get_transaction(&self, id: TransactionId) -> StoreResult<Transaction>;
    /// Creates a new [`Transaction`] and returns it.
    /// If the [`Transaction`] already exists, it returns an [`StoreError::AlreadyExists`].
    async fn create_transaction(&self, transaction: Transaction) -> StoreResult<Transaction>;
    /// Deletes a [`Transaction`].
    async fn delete_transaction(&self, id: TransactionId) -> StoreResult<()>;
    /// Sets a [`Transaction`] under dispute.
    async fn set_transaction_under_dispute(
        &self,
        id: TransactionId,
        under_dispute: bool,
    ) -> StoreResult<()>;
    /// Toggles the under dispute flag
    async fn toggle_under_dispute(&self, id: TransactionId) -> StoreResult<()>;
    /// Gets the current state of the [`Account`].
    /// If the [`Account`] does not exist, it will return an empty [`Account`].
    /// Note that the account is not created in the [`Store`] yet.
    async fn get_account(&self, id: ClientId) -> StoreResult<Account>;
    /// Updates the state of the [`Account`].
    /// If the [`Account`] does not exist, it will create the [`Account`].
    async fn upsert_account(&self, account: &Account) -> StoreResult<()>;
    /// Returns the current balance of all the clients [`Account`].
    async fn get_all_accounts(
        &self,
    ) -> StoreResult<Box<dyn futures::Stream<Item = Account> + Unpin + Send>>;
}
