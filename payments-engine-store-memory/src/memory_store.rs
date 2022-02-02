use async_trait::async_trait;
use payments_engine_core::{
    account::Account,
    common::ClientId,
    store::{Store, StoreError, StoreResult},
    transaction::{Transaction, TransactionId},
};
use std::{
    collections::HashMap,
    ops::Deref,
    sync::{Arc, RwLock},
};
use tracing::instrument;

/// In-Memory implementation of the Store trait.
/// Fairly useful for testing and simple scenarios.
///
/// Note that [`MemoryStore`] can be safely shared across different threads as it uses an inner [`std::sync::Arc`]. This basically means that whenever you clone a [`MemoryStore`] you´re using `Arc::clone()` under the hood.
///
/// # Important
/// This store only cares about [`Transaction::Dispute`] transactions so all the other variants are not really stored.
///
/// # Testing:
///
/// The inner hashmaps are exposed for testing purposes.
/// There's also a convenient flag to enable/disable error while upserting accounts.
#[derive(Debug, Default)]
pub struct MemoryStore(Arc<Inner>);

impl MemoryStore {
    /// Creates a new [`MemoryStore`]
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(Inner::default()))
    }

    /// Creates a new [`MemoryStore`] with the given deposits and accounts.
    #[must_use]
    pub fn seeded(
        deposits: Option<HashMap<TransactionId, Transaction>>,
        accounts: Option<HashMap<ClientId, Account>>,
    ) -> Self {
        Self(Arc::new(Inner::seeded(deposits, accounts)))
    }
}

impl Clone for MemoryStore {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Deref for MemoryStore {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[async_trait]
impl Store for MemoryStore {
    /// Gets a transaction by its id.
    /// If it doesn't exist, it returns an [`StoreError::NotFound].
    #[instrument(skip(self))]
    async fn get_transaction(&self, id: TransactionId) -> StoreResult<Transaction> {
        self.0.get_transaction(id).await
    }

    /// Creates a new [`Transaction`] and returns it.
    /// If the [`Transaction`] already exists, it returns an [`StoreError::AlreadyExists`].
    /// Note that this method is only storing [`Transaction::Deposit`] transactions.
    /// That's mainly because disputes, resolutions and chargebacks are only related to diposits,
    /// so it makes no sense to store withdrawals or any other kind of [`Transaction`].
    #[instrument(skip(self))]
    async fn create_transaction(&self, transaction: Transaction) -> StoreResult<Transaction> {
        self.0.create_transaction(transaction).await
    }

    /// Deletes a [`Transaction`].
    #[instrument(skip(self))]
    async fn delete_transaction(&self, id: TransactionId) -> StoreResult<()> {
        self.0.delete_transaction(id).await
    }

    /// Sets a [`Transaction`] under dispute.
    #[instrument(skip(self))]
    async fn set_transaction_under_dispute(
        &self,
        id: TransactionId,
        under_dispute: bool,
    ) -> StoreResult<()> {
        self.0
            .set_transaction_under_dispute(id, under_dispute)
            .await
    }

    /// Toggles the under dispute flag
    #[instrument(skip(self))]
    async fn toggle_under_dispute(&self, id: TransactionId) -> StoreResult<()> {
        self.0.toggle_under_dispute(id).await
    }

    /// Gets the current state of the [`Account`].
    /// If the [`Account`] does not exist, it will return an empty [`Account`].
    /// Note that the account is not created in the [`Store`] yet.
    #[instrument(skip(self))]
    async fn get_account(&self, id: ClientId) -> StoreResult<Account> {
        self.0.get_account(id).await
    }

    /// Updates the state of the [`Account`].
    /// If the [`Account`] does not exist, it will create the [`Account`].
    #[instrument(skip(self))]
    async fn upsert_account(&self, account: &Account) -> StoreResult<()> {
        self.0.upsert_account(account).await
    }

    /// Returns the current state of clients accounts.
    #[instrument(skip(self))]
    async fn get_all_accounts(
        &self,
    ) -> StoreResult<Box<dyn futures::Stream<Item = Account> + Unpin + Send>> {
        self.0.get_all_accounts().await
    }
}

/// Inner implementation of the [`MemoryStore`]
#[derive(Debug)]
pub struct Inner {
    #[cfg(any(test, feature = "testing"))]
    enable_upsert_account_failure: RwLock<bool>,
    deposits: RwLock<HashMap<TransactionId, Transaction>>,
    accounts: RwLock<HashMap<ClientId, Account>>,
}

impl Inner {
    /// Creates a new [`MemoryStore`]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new [`MemoryStore`] with the given deposits and accounts.
    #[must_use]
    pub fn seeded(
        deposits: Option<HashMap<TransactionId, Transaction>>,
        accounts: Option<HashMap<ClientId, Account>>,
    ) -> Self {
        Self {
            deposits: RwLock::new(deposits.unwrap_or_default()),
            accounts: RwLock::new(accounts.unwrap_or_default()),
            #[cfg(any(test, feature = "testing"))]
            enable_upsert_account_failure: RwLock::new(false),
        }
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn deposits(&self) -> &RwLock<HashMap<TransactionId, Transaction>> {
        &self.deposits
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn accounts(&self) -> &RwLock<HashMap<ClientId, Account>> {
        &self.accounts
    }

    /// Returns the length of the transactions map.
    ///
    /// # Panics
    ///
    /// This can panic, as we use a ``RwLock``.
    /// As this method is only used for testing, it is not a problem.
    #[cfg(any(test, feature = "testing"))]
    pub fn transactions_len(&self) -> usize {
        self.deposits.read().unwrap().len()
    }

    /// Returns the length of the accounts map.
    ///
    /// # Panics
    ///
    /// This can panic, as we use a ``RwLock``.
    /// As this method is only used for testing, it is not a problem.
    #[cfg(any(test, feature = "testing"))]
    pub fn accounts_len(&self) -> usize {
        self.accounts.read().unwrap().len()
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn set_enable_upsert_account_failure(&self, enable: bool) {
        self.enable_upsert_account_failure
            .write()
            .map(|mut failure| {
                *failure = enable;
            })
            .unwrap();
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn enable_upsert_account_failure(&self) -> bool {
        *self.enable_upsert_account_failure.read().unwrap()
    }
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            deposits: RwLock::new(HashMap::new()),
            accounts: RwLock::new(HashMap::new()),
            #[cfg(any(test, feature = "testing"))]
            enable_upsert_account_failure: RwLock::new(false),
        }
    }
}

#[async_trait]
impl Store for Inner {
    /// Gets a transaction by its id.
    /// If it doesn't exist, it returns an [`StoreError::NotFound].
    #[instrument(skip(self))]
    async fn get_transaction(&self, id: TransactionId) -> StoreResult<Transaction> {
        tracing::debug!("Getting transaction {}", id);
        let result = self
            .deposits
            .read()
            .map_err(|e| StoreError::AccessError(e.to_string()))
            .and_then(|deposits| {
                deposits
                    .get(&id)
                    .cloned()
                    .ok_or(StoreError::NotFound { id })
            });

        if result.is_err() {
            tracing::error!("Error while getting transaction: {:?}", result);
        }

        result
    }

    /// Creates a new [`Transaction`] and returns it.
    /// If the [`Transaction`] already exists, it returns an [`StoreError::AlreadyExists`].
    /// Note that this method is only storing [`Transaction::Deposit`] transactions.
    /// That's mainly because disputes, resolutions and chargebacks are only related to diposits,
    /// so it makes no sense to store withdrawals or any other kind of [`Transaction`].
    #[instrument(skip(self))]
    async fn create_transaction(&self, transaction: Transaction) -> StoreResult<Transaction> {
        tracing::debug!("Creating transaction: {:?}", transaction);
        if let Transaction::Deposit { .. } = transaction {
            let result = self
                .deposits
                .write()
                .map_err(|e| StoreError::AccessError(e.to_string()))
                .and_then(|mut deposits| {
                    let transaction_id = transaction.info().id;
                    if let std::collections::hash_map::Entry::Vacant(e) =
                        deposits.entry(transaction_id)
                    {
                        e.insert(transaction.clone());
                        Ok(transaction)
                    } else {
                        Err(StoreError::AlreadyExists { id: transaction_id })
                    }
                });

            if result.is_err() {
                tracing::error!("Error while trying to create transaction: {:?}", result);
            }

            result
        } else {
            Ok(transaction)
        }
    }

    /// Deletes a [`Transaction`].
    #[instrument(skip(self))]
    async fn delete_transaction(&self, id: TransactionId) -> StoreResult<()> {
        tracing::debug!("Deleting transaction: {:?}", id);
        self.deposits
            .write()
            .map_err(|e| StoreError::AccessError(e.to_string()))
            .map(|mut deposits| {
                deposits.remove(&id);
            })
    }

    /// Sets a [`Transaction`] under dispute.
    #[instrument(skip(self))]
    async fn set_transaction_under_dispute(
        &self,
        id: TransactionId,
        under_dispute: bool,
    ) -> StoreResult<()> {
        tracing::debug!(
            "Setting transaction {} under dispute to {}",
            id,
            under_dispute
        );
        self.deposits
            .write()
            .map_err(|e| StoreError::AccessError(e.to_string()))
            .map(|mut deposits| {
                if let Some(transaction) = deposits.get_mut(&id) {
                    transaction.set_under_dispute(under_dispute);
                }
            })
    }

    /// Toggles the under dispute flag
    #[instrument(skip(self))]
    async fn toggle_under_dispute(&self, id: TransactionId) -> StoreResult<()> {
        tracing::debug!("Toggling under dispute for transaction {}", id);
        self.deposits
            .write()
            .map_err(|e| StoreError::AccessError(e.to_string()))
            .map(|mut deposits| {
                if let Some(transaction) = deposits.get_mut(&id) {
                    transaction.toggle_under_dispute();
                }
            })
    }

    /// Gets the current state of the [`Account`].
    /// If the [`Account`] does not exist, it will return an empty [`Account`].
    /// Note that the account is not created in the [`Store`] yet.
    #[instrument(skip(self))]
    async fn get_account(&self, id: ClientId) -> StoreResult<Account> {
        tracing::debug!("Getting account: {}", id);
        let result = self
            .accounts
            .read()
            .map_err(|e| StoreError::AccessError(e.to_string()))
            .map(|accounts| {
                accounts
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| Account::new(id))
            });

        if result.is_err() {
            tracing::error!("Error while getting account: {:?}", result);
        }

        result
    }

    /// Updates the state of the [`Account`].
    /// If the [`Account`] does not exist, it will create the [`Account`].
    #[instrument(skip(self))]
    async fn upsert_account(&self, account: &Account) -> StoreResult<()> {
        tracing::debug!("Upserting account: {:?}", account);
        #[cfg(any(test, feature = "testing"))]
        {
            if self.enable_upsert_account_failure() {
                return Err(StoreError::AccessError("Test Error".to_string()));
            }
        }
        let result = self
            .accounts
            .write()
            .map_err(|e| StoreError::AccessError(e.to_string()))
            .map(|mut accounts| {
                accounts.insert(account.client, account.clone());
            });

        if result.is_err() {
            tracing::error!("Error while trying to create an account: {:?}", result);
        }

        result
    }

    /// Returns the current state of clients accounts.
    #[instrument(skip(self))]
    async fn get_all_accounts(
        &self,
    ) -> StoreResult<Box<dyn futures::Stream<Item = Account> + Unpin + Send>> {
        let result = self
            .accounts
            .read()
            .map_err(|e| StoreError::AccessError(e.to_string()))
            .map(|accounts| {
                Box::new(futures::stream::iter(
                    accounts.values().cloned().collect::<Vec<_>>(),
                ))
            })?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use payments_engine_core::dec;
    use std::collections::HashMap;

    #[tokio::test]
    async fn get_transaction_works() {
        let transaction_id = 1;
        let transaction = Transaction::deposit(transaction_id, 1, dec!(1.0001));
        let mut deposits = HashMap::new();
        deposits.insert(transaction_id, transaction.clone());

        let store = MemoryStore::seeded(Some(deposits), None);

        let result = store.get_transaction(transaction_id).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), transaction);
    }

    #[tokio::test]
    async fn get_transaction_returns_not_found_if_transaction_does_not_exist() {
        let transaction_id = 1;
        let transaction = Transaction::deposit(transaction_id, 1, dec!(1.0001));
        let mut deposits = HashMap::new();
        deposits.insert(transaction_id, transaction.clone());

        let store = MemoryStore::seeded(Some(deposits), None);

        let unexisting_transaction_id = 2;
        let result = store.get_transaction(unexisting_transaction_id).await;

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            StoreError::NotFound {
                id: unexisting_transaction_id
            }
        );
    }

    #[tokio::test]
    async fn create_transaction_works() {
        let store = MemoryStore::new();
        let transaction_id = 1;
        let transaction = Transaction::deposit(transaction_id, 1, dec!(1.0001));

        let result = store.create_transaction(transaction.clone()).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), transaction);

        let result = store.get_transaction(transaction_id).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), transaction);
    }

    #[tokio::test]
    async fn create_transaction_returns_already_exists_if_transaction_already_exists() {
        let transaction_id = 1;
        let transaction = Transaction::deposit(transaction_id, 1, dec!(1.0001));
        let mut deposits = HashMap::new();
        deposits.insert(transaction_id, transaction.clone());

        let store = MemoryStore::seeded(Some(deposits), None);

        let result = store.create_transaction(transaction.clone()).await;

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            StoreError::AlreadyExists { id: transaction_id }
        );
    }

    #[allow(unused_must_use)]
    #[tokio::test]
    async fn create_transaction_only_saves_deposits() {
        let deposit = Transaction::deposit(1, 1, dec!(1.0001));
        let store = MemoryStore::new();
        store.create_transaction(deposit.clone()).await;
        store.create_transaction(Transaction::withdrawal(1, 1, dec!(1.0001)));
        store.create_transaction(Transaction::dispute(1, 1));
        store.create_transaction(Transaction::resolve(1, 1));
        store.create_transaction(Transaction::chargeback(1, 1));

        let result = store.get_transaction(deposit.info().id).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), deposit);

        let withdrawal = store.get_transaction(2).await;
        let dispute = store.get_transaction(3).await;
        let resolve = store.get_transaction(4).await;
        let chargeback = store.get_transaction(5).await;

        assert!(withdrawal.is_err());
        assert!(dispute.is_err());
        assert!(resolve.is_err());
        assert!(chargeback.is_err());
    }

    #[tokio::test]
    async fn delete_transaction_works() {
        let txs = vec![
            Transaction::deposit(1, 1, dec!(1.0)),
            Transaction::deposit(2, 1, dec!(2.0)),
            Transaction::deposit(3, 1, dec!(3.0)),
        ];
        let mut deposits = HashMap::new();
        for tx in txs.into_iter() {
            deposits.insert(tx.info().id, tx);
        }

        let store = MemoryStore::seeded(Some(deposits), None);

        store.delete_transaction(1).await.unwrap();
        assert!(store.get_transaction(1).await.is_err());

        // deleting non-existing transaction should not fail
        assert!(store.delete_transaction(1).await.is_ok());
    }

    #[tokio::test]
    async fn get_account_works() {
        let account = Account::seeded(1, dec!(10.3001), dec!(5.40), false);
        let mut accounts = HashMap::new();
        accounts.insert(account.client, account.clone());

        let store = MemoryStore::seeded(None, Some(accounts));

        let result = store.get_account(account.client).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), account);
    }

    #[tokio::test]
    async fn get_account_returns_new_account_if_not_exists_but_does_not_create_it() {
        let store = MemoryStore::new();
        let result = store.get_account(1).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Account::new(1));
        assert_eq!(store.transactions_len(), 0)
    }

    #[tokio::test]
    async fn upsert_account_creates_new_account_if_does_not_exist() {
        let store = MemoryStore::new();
        let account = Account::seeded(1, dec!(10.3001), dec!(5.40), false);

        let result = store.upsert_account(&account).await;

        assert!(result.is_ok());

        let result = store.get_account(1).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), account);
        assert_eq!(store.accounts_len(), 1);
    }

    #[tokio::test]
    async fn upsert_account_updates_account_if_exists() {
        let mut accounts = HashMap::new();
        accounts.insert(1, Account::seeded(1, dec!(10.3001), dec!(5.40), false));

        let store = MemoryStore::seeded(None, Some(accounts));

        let update = Account::seeded(1, dec!(5), dec!(5.40), false);

        let result = store.upsert_account(&update).await;

        assert!(result.is_ok());

        let result = store.get_account(1).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), update);
        assert_eq!(store.accounts_len(), 1);
    }
}
