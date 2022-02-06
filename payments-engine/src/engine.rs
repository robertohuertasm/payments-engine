use async_trait::async_trait;
use payments_engine_core::{
    account::Account,
    common::Amount,
    engine::{Engine as CoreEngine, EngineError, EngineResult},
    store::{Store, StoreError},
    transaction::{Transaction, TransactionInfo},
};
use tracing::instrument;
/// The [`Engine`] is responsible for processing all the transactions.
/// It also provides a way to get the current state of all the accounts.
pub struct Engine<S: Store> {
    store: S,
}

#[async_trait]
impl<S: Store> CoreEngine for Engine<S> {
    /// Processes the given [`Transaction`] and returns the resulting state of the [`Account`]
    #[instrument(skip(self))]
    async fn process_transaction(&self, transaction: Transaction) -> EngineResult<Account> {
        tracing::debug!("Processing transaction: {:?}", transaction);
        // validate transaction state
        let transaction_info = transaction.info().clone();
        if transaction.has_negative_amount() {
            tracing::error!(
                "Transaction with id {} has negative amount",
                transaction_info.id
            );
            return Err(EngineError::NegativeAmountTransaction {
                id: transaction_info.id,
            });
        }

        // storing the transaction in the store.
        // note that duplicated transactions are not allowed and
        // the store will return an error if the transaction already exists.
        let transaction = self.store.create_transaction(transaction).await?;

        let transaction_result: EngineResult<Account> = async {
            // get info about the account from the store
            let mut account = self.store.get_account(transaction_info.client_id).await?;

            // is the account locked?
            if account.locked {
                tracing::error!(
                    "Tried to apply transaction with id {} to a locked account {}",
                    transaction_info.id,
                    transaction_info.client_id
                );
                return Err(EngineError::LockedAccount {
                    id: transaction_info.client_id,
                    tx: transaction_info.id,
                });
            }

            // apply the transaction to the account in memory.
            // note that there might be a mutationn of the ref transaction
            // in case of disputes, resolves and chargebacks.
            self.apply_transaction(&mut account, &transaction).await?;

            // save the account back to the store
            self.store
                .upsert_account(&account)
                .await
                .map_err(EngineError::TransactionNotCommited)?;

            Ok(account)
        }
        .await;

        tracing::debug!("Transaction processed: {:?}", transaction_result);

        match transaction_result {
            Ok(account) => Ok(account),
            Err(e) => {
                // let's rollback the stored transaction.
                // NOTE: if the account is frozen we're rolling back all the transactions.
                // this could be easily changed by excluding LockedAccount errors.
                // For now, it seems like a sensible behavior due the simple implementation that we're aiming for.
                // IMPORTANT:
                // we're only rolling back deposit and withdrawals.
                // for the rest of transactions we're rolling back the transaction under_dispute flag in case the transaction didn't commit
                match transaction {
                    Transaction::Deposit { .. } | Transaction::Withdrawal { .. } => {
                        // rolling back
                        tracing::warn!("Rolling back transaction for tx {}", transaction_info.id);
                        if let Err(e) = self.store.delete_transaction(transaction_info.id).await {
                            tracing::error!(
                                "CRITICAL: Failed to rollback transaction: {}",
                                transaction_info.id
                            );
                            return Err(EngineError::Store(e));
                        }
                    }
                    Transaction::Dispute { .. }
                    | Transaction::Resolve { .. }
                    | Transaction::ChargeBack { .. } => {
                        // Rollback disputed state in the store if the error comes from the upsert_account layer
                        if let EngineError::TransactionNotCommited(_) = e {
                            // change the under_dispute_state
                            tracing::warn!(
                                "Rolling back transaction dispute state for tx {}",
                                transaction_info.id
                            );
                            self.store.toggle_under_dispute(transaction_info.id).await?;
                        }
                    }
                };

                Err(e)
            }
        }
    }

    /// Returns the current state of clients accounts.
    #[instrument(skip(self))]
    async fn report(
        &self,
    ) -> EngineResult<Box<dyn futures::Stream<Item = Account> + Unpin + Send>> {
        let stream = self.store.get_all_accounts().await?;
        Ok(stream)
    }
}

impl<S: Store> Engine<S> {
    /// Creates a new [`Engine`] with the given [`Store`].
    pub fn new(store: S) -> Self {
        Self { store }
    }

    async fn apply_transaction(
        &self,
        account: &mut Account,
        transaction: &Transaction,
    ) -> EngineResult<()> {
        match transaction {
            Transaction::Deposit { amount, .. } => self.deposit(account, amount).await,
            Transaction::Withdrawal { amount, .. } => self.withdrawal(account, amount).await,
            Transaction::Dispute { info } => self.dispute(account, info).await,
            Transaction::Resolve { info } => self.resolve(account, info).await,
            Transaction::ChargeBack { info } => self.chargeback(account, info).await,
        }
    }

    async fn deposit(&self, account: &mut Account, amount: &Amount) -> EngineResult<()> {
        account.available += amount;
        account.total += amount;
        Ok(())
    }

    async fn withdrawal(&self, account: &mut Account, amount: &Amount) -> EngineResult<()> {
        if account.available < *amount {
            tracing::error!(?account, "Insufficient available funds");
            return Err(EngineError::InsufficientAvailableFunds);
        }
        account.available -= amount;
        account.total -= amount;
        Ok(())
    }

    async fn dispute(&self, account: &mut Account, info: &TransactionInfo) -> EngineResult<()> {
        // if no ref, ignore
        let ref_transaction = self.store.get_transaction(info.id).await;
        match ref_transaction {
            Err(StoreError::NotFound { id }) => {
                tracing::info!("Ignoring dispute for transaction {}. No ref found", id);
                Ok(())
            }
            Err(e) => Err(EngineError::Store(e)),
            Ok(ref_tx) => {
                if let Transaction::Deposit {
                    info,
                    amount,
                    under_dispute,
                } = ref_tx
                {
                    if account.client != info.client_id {
                        return Err(wrong_client_error(account, &info));
                    } else if under_dispute {
                        tracing::error!(?account, "Double dispute for tx {}", info.id);
                        return Err(EngineError::DoubleDispute { id: info.id });
                    } else if account.available < amount {
                        tracing::error!(?account, "Insufficient available funds");
                        return Err(EngineError::InsufficientAvailableFunds);
                    }
                    // if everything is fine: update the account
                    account.available -= amount;
                    account.held += amount;
                    // set to under dispute
                    self.store
                        .set_transaction_under_dispute(info.id, true)
                        .await?;
                } else {
                    tracing::error!("Reference transaction {} is not a Deposit", info.id);
                    return Err(EngineError::WrongTransactionRef { id: info.id });
                }

                Ok(())
            }
        }
    }

    async fn resolve(&self, account: &mut Account, info: &TransactionInfo) -> EngineResult<()> {
        // if no ref, ignore
        let ref_transaction = self.store.get_transaction(info.id).await;
        match ref_transaction {
            Err(StoreError::NotFound { id }) => {
                tracing::info!("Ignoring resolve for transaction {}. No ref found", id);
                Ok(())
            }
            Err(e) => Err(EngineError::Store(e)),
            Ok(ref_tx) => {
                if let Transaction::Deposit {
                    info,
                    amount,
                    under_dispute,
                } = ref_tx
                {
                    if account.client != info.client_id {
                        return Err(wrong_client_error(account, &info));
                    } else if account.held < amount {
                        tracing::error!(?account, "Insufficient held funds");
                        return Err(EngineError::InsufficientHeldFunds);
                    } else if !under_dispute {
                        tracing::info!(
                            "Ignoring resolve for transaction {}. Not under dispute",
                            info.id
                        );
                        return Ok(());
                    }
                    // if everything is fine: update the account
                    account.held -= amount;
                    account.available += amount;
                    // set to not under dispute
                    self.store
                        .set_transaction_under_dispute(info.id, false)
                        .await?;
                } else {
                    tracing::error!("Reference transaction {} is not a Deposit", info.id);
                    return Err(EngineError::WrongTransactionRef { id: info.id });
                }

                Ok(())
            }
        }
    }

    async fn chargeback(&self, account: &mut Account, info: &TransactionInfo) -> EngineResult<()> {
        // if no ref, ignore
        let ref_transaction = self.store.get_transaction(info.id).await;
        match ref_transaction {
            Err(StoreError::NotFound { id }) => {
                tracing::info!("Ignoring chargeback for transaction {}. No ref found", id);
                Ok(())
            }
            Err(e) => Err(EngineError::Store(e)),
            Ok(ref_tx) => {
                if let Transaction::Deposit {
                    info,
                    amount,
                    under_dispute,
                } = ref_tx
                {
                    if account.client != info.client_id {
                        return Err(wrong_client_error(account, &info));
                    } else if account.held < amount {
                        tracing::error!(?account, "Insufficient held funds");
                        return Err(EngineError::InsufficientHeldFunds);
                    } else if !under_dispute {
                        tracing::info!(
                            "Ignoring chargeback for transaction {}. Not under dispute",
                            info.id
                        );
                        return Ok(());
                    }
                    // if everything is fine: update the account
                    account.held -= amount;
                    account.total -= amount;
                    account.locked = true;
                    // set to not under dispute
                    self.store
                        .set_transaction_under_dispute(info.id, false)
                        .await?;
                } else {
                    tracing::error!("Reference transaction {} is not a Deposit", info.id);
                    return Err(EngineError::WrongTransactionRef { id: info.id });
                }

                Ok(())
            }
        }
    }
}

fn wrong_client_error(account: &Account, info: &TransactionInfo) -> EngineError {
    tracing::error!(
        ?account,
        ?info,
        "Referenced transaction {} is not for the same client",
        info.id
    );
    EngineError::TransactionRefWrongClient {
        id: info.id,
        client: info.client_id,
        wrong_client: account.client,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use payments_engine_core::dec;
    use payments_engine_core::transaction::TransactionId;
    use payments_engine_store_memory::MemoryStore;
    use std::collections::HashMap;

    /// Asserts that a particular deposit is under a particular dispute state.
    fn assert_under_dispute(store: &MemoryStore, id: TransactionId, under_dispute_state: bool) {
        let deposits = store.deposits().read().unwrap();
        if let Some(&Transaction::Deposit { under_dispute, .. }) = deposits.get(&id) {
            assert_eq!(under_dispute, under_dispute_state);
        } else {
            panic!("Deposit not found");
        }
    }

    // test the public api

    #[tokio::test]
    async fn no_transaction_must_be_applied_if_the_account_is_locked() {
        // locked account
        let account = Account::seeded(1, dec!(10), Amount::ZERO, true);
        let store = MemoryStore::new();
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        assert!(account.locked);

        let engine = Engine::new(store.clone());

        let deposit = Transaction::deposit(2, 1, dec!(10));
        let withdrawal = Transaction::withdrawal(3, 1, dec!(10));
        let resolve = Transaction::resolve(1, 1);
        let chargeback = Transaction::chargeback(1, 1);

        assert_eq!(
            engine.process_transaction(deposit).await.unwrap_err(),
            EngineError::LockedAccount { id: 1, tx: 2 },
            "Deposit should fail if the account is locked"
        );
        assert_eq!(
            engine.process_transaction(withdrawal).await.unwrap_err(),
            EngineError::LockedAccount { id: 1, tx: 3 },
            "Withdrawal should fail if the account is locked"
        );
        assert_eq!(
            engine.process_transaction(resolve).await.unwrap_err(),
            EngineError::LockedAccount { id: 1, tx: 1 },
            "Resolve should fail if the account is locked"
        );
        assert_eq!(
            engine.process_transaction(chargeback).await.unwrap_err(),
            EngineError::LockedAccount { id: 1, tx: 1 },
            "Chargeback should fail if the account is locked"
        );

        // account in same state
        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        assert!(account.locked);
    }

    #[tokio::test]
    async fn on_deposit_available_and_total_must_be_increased() {
        let account = Account::new(1);
        let store = MemoryStore::new();
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, Amount::ZERO);

        let engine = Engine::new(store.clone());
        let deposit = Transaction::deposit(1, 1, dec!(10));
        let account = engine.process_transaction(deposit).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
    }

    #[tokio::test]
    async fn on_withdrawal_available_and_total_must_be_decreased() {
        let account = Account::seeded(1, dec!(10), Amount::ZERO, false);
        let store = MemoryStore::new();
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));

        let engine = Engine::new(store.clone());
        let withdrawal = Transaction::withdrawal(1, 1, dec!(8));
        let account = engine.process_transaction(withdrawal).await.unwrap();

        assert_eq!(account.available, dec!(2));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(2));
    }

    #[tokio::test]
    async fn on_withdrawal_if_available_is_not_enough_do_not_apply_transaction() {
        let account = Account::seeded(1, dec!(10), Amount::ZERO, false);
        let store = MemoryStore::new();
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));

        let engine = Engine::new(store.clone());
        let withdrawal = Transaction::withdrawal(1, 1, dec!(12));
        let err = engine.process_transaction(withdrawal).await.unwrap_err();

        // it should error
        assert_eq!(err, EngineError::InsufficientAvailableFunds);
        // it should not change the account
        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        // it should not rollback
        assert_eq!(store.transactions_len(), 0);
    }

    #[tokio::test]
    async fn on_dispute_available_should_decrease_held_increase_total_remain() {
        let account = Account::seeded(1, dec!(10), Amount::ZERO, false);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit(1, 1, dec!(10)));
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        assert_eq!(store.transactions_len(), 1);
        assert_eq!(store.accounts_len(), 1);

        let engine = Engine::new(store.clone());
        let dispute = Transaction::dispute(1, 1);
        let account = engine.process_transaction(dispute).await.unwrap();

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, dec!(10));
        assert_eq!(account.total, dec!(10));

        assert_under_dispute(&store, 1, true);
    }

    #[tokio::test]
    async fn on_dispute_the_referenced_tx_must_be_a_deposit() {
        // this case is not really possible in InMemoryStore
        // but it's useful to recreate it in case we use other kind of stores.
        let account = Account::seeded(1, dec!(10), Amount::ZERO, false);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit(1, 1, dec!(10)));
        // inserting a withdrawal directly.
        // this won't even happen with memory store, but it's useful to test the engine
        deposits.insert(2, Transaction::withdrawal(2, 1, dec!(1)));
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        assert_eq!(store.transactions_len(), 2);
        assert_eq!(store.accounts_len(), 1);

        let engine = Engine::new(store.clone());
        // referencing a withdrawal
        let dispute = Transaction::dispute(2, 1);
        let err = engine.process_transaction(dispute).await.unwrap_err();
        // it should error
        assert_eq!(err, EngineError::WrongTransactionRef { id: 2 });
        // it should not change the account
        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        // it should not rollback
        assert_eq!(store.transactions_len(), 2);
        // no disputes
        assert_under_dispute(&store, 1, false);
    }

    #[tokio::test]
    async fn on_dispute_do_not_apply_transaction_if_already_under_dispute() {
        let account = Account::seeded(1, dec!(10), Amount::ZERO, false);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit(1, 1, dec!(10)));
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        assert_eq!(store.transactions_len(), 1);
        assert_eq!(store.accounts_len(), 1);

        let engine = Engine::new(store.clone());
        let dispute = Transaction::dispute(1, 1);
        let account = engine.process_transaction(dispute).await.unwrap();

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, dec!(10));
        assert_eq!(account.total, dec!(10));

        assert_under_dispute(&store, 1, true);

        // double dispute
        let dispute = Transaction::dispute(1, 1);
        let err = engine.process_transaction(dispute).await.unwrap_err();
        assert_eq!(err, EngineError::DoubleDispute { id: 1 });
        // still in dispute
        assert_under_dispute(&store, 1, true);
    }

    #[tokio::test]
    async fn on_dispute_ignore_transaction_if_ref_transaction_does_not_exist() {
        let account = Account::seeded(1, dec!(10), Amount::ZERO, false);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit(1, 1, dec!(10)));
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        assert_eq!(store.transactions_len(), 1);
        assert_eq!(store.accounts_len(), 1);

        let engine = Engine::new(store.clone());
        let dispute = Transaction::dispute(2, 1);
        let account = engine.process_transaction(dispute).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));

        assert_under_dispute(&store, 1, false);
    }

    #[tokio::test]
    async fn on_dispute_error_if_no_enough_available_funds() {
        let account = Account::new(1);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit(1, 1, dec!(10)));
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, Amount::ZERO);
        assert_eq!(store.transactions_len(), 1);
        assert_eq!(store.accounts_len(), 1);

        let engine = Engine::new(store.clone());
        let dispute = Transaction::dispute(1, 1);
        let err = engine.process_transaction(dispute).await.unwrap_err();
        assert_eq!(err, EngineError::InsufficientAvailableFunds);

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, Amount::ZERO);

        assert_under_dispute(&store, 1, false);
    }

    #[tokio::test]
    async fn on_dispute_error_if_tx_client_is_wrong() {
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit(1, 1, dec!(10)));
        deposits.insert(2, Transaction::deposit(2, 2, dec!(20)));
        let account_1 = Account::seeded(1, dec!(10), Amount::ZERO, false);
        let account_2 = Account::seeded(2, dec!(20), Amount::ZERO, false);
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account_1).await.unwrap();
        store.upsert_account(&account_2).await.unwrap();

        assert_eq!(store.transactions_len(), 2);
        assert_eq!(store.accounts_len(), 2);

        let engine = Engine::new(store.clone());
        let dispute = Transaction::dispute(2, 1);
        let err = engine.process_transaction(dispute).await.unwrap_err();
        assert_eq!(
            err,
            EngineError::TransactionRefWrongClient {
                id: 2,
                client: 2,
                wrong_client: 1,
            }
        );
        // not under dispute
        assert_under_dispute(&store, 1, false);
        assert_under_dispute(&store, 2, false);
    }

    #[tokio::test]
    async fn on_resolve_held_should_decrease_available_increase_total_remain_and_no_dispute() {
        let account = Account::seeded(1, Amount::ZERO, dec!(10), false);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit_under_dispute(1, 1, dec!(10)));
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, dec!(10));
        assert_eq!(account.total, dec!(10));
        assert_eq!(store.transactions_len(), 1);
        assert_eq!(store.accounts_len(), 1);
        assert_under_dispute(&store, 1, true);

        let engine = Engine::new(store.clone());
        let resolve = Transaction::resolve(1, 1);
        let account = engine.process_transaction(resolve).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));

        // no longer under dispute
        assert_under_dispute(&store, 1, false);
    }

    #[tokio::test]
    async fn on_resolve_the_referenced_tx_must_be_a_deposit() {
        // this case is not really possible in InMemoryStore
        // but it's useful to recreate it in case we use other kind of stores.
        let account = Account::seeded(1, dec!(10), Amount::ZERO, false);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit_under_dispute(1, 1, dec!(10)));
        // inserting a withdrawal directly.
        // this won't even happen with memory store, but it's useful to test the engine
        deposits.insert(2, Transaction::withdrawal(2, 1, dec!(1)));
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        assert_eq!(store.transactions_len(), 2);
        assert_eq!(store.accounts_len(), 1);
        assert_under_dispute(&store, 1, true);

        let engine = Engine::new(store.clone());
        // referencing a withdrawal
        let resolve = Transaction::resolve(2, 1);
        let err = engine.process_transaction(resolve).await.unwrap_err();
        // it should error
        assert_eq!(err, EngineError::WrongTransactionRef { id: 2 });
        // it should not change the account
        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        // it should not rollback
        assert_eq!(store.transactions_len(), 2);
        // no disputes
        assert_under_dispute(&store, 1, true);
    }

    #[tokio::test]
    async fn on_resolve_ignore_tx_if_not_under_dispute() {
        let account = Account::seeded(1, Amount::ZERO, dec!(10), false);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit(1, 1, dec!(10)));
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, dec!(10));
        assert_eq!(account.total, dec!(10));
        assert_eq!(store.transactions_len(), 1);
        assert_eq!(store.accounts_len(), 1);
        assert_under_dispute(&store, 1, false);

        let engine = Engine::new(store.clone());
        let resolve = Transaction::resolve(1, 1);
        let account = engine.process_transaction(resolve).await.unwrap();

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, dec!(10));
        assert_eq!(account.total, dec!(10));
        // still no dispute
        assert_under_dispute(&store, 1, false);
    }

    #[tokio::test]
    async fn on_resolve_ignore_transaction_if_ref_transaction_does_not_exist() {
        let account = Account::seeded(1, dec!(10), Amount::ZERO, false);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit_under_dispute(1, 1, dec!(10)));
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        assert_eq!(store.transactions_len(), 1);
        assert_eq!(store.accounts_len(), 1);
        assert_under_dispute(&store, 1, true);

        let engine = Engine::new(store.clone());
        let resolve = Transaction::resolve(2, 1);
        let account = engine.process_transaction(resolve).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));

        assert_under_dispute(&store, 1, true);
    }

    #[tokio::test]
    async fn on_resolve_error_if_no_enough_available_funds() {
        let account = Account::seeded(1, Amount::ZERO, dec!(10), false);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit_under_dispute(1, 1, dec!(20)));
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, dec!(10));
        assert_eq!(account.total, dec!(10));
        assert_eq!(store.transactions_len(), 1);
        assert_eq!(store.accounts_len(), 1);
        assert_under_dispute(&store, 1, true);

        let engine = Engine::new(store.clone());
        let resolve = Transaction::resolve(1, 1);
        let err = engine.process_transaction(resolve).await.unwrap_err();

        assert_eq!(err, EngineError::InsufficientHeldFunds);

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, dec!(10));
        assert_eq!(account.total, dec!(10));

        // still under dispute
        assert_under_dispute(&store, 1, true);
    }

    #[tokio::test]
    async fn on_resolve_error_if_tx_client_is_wrong() {
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit_under_dispute(1, 1, dec!(10)));
        deposits.insert(2, Transaction::deposit_under_dispute(2, 2, dec!(20)));
        let account_1 = Account::seeded(1, dec!(10), Amount::ZERO, false);
        let account_2 = Account::seeded(2, dec!(20), Amount::ZERO, false);
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account_1).await.unwrap();
        store.upsert_account(&account_2).await.unwrap();

        assert_eq!(store.transactions_len(), 2);
        assert_eq!(store.accounts_len(), 2);

        let engine = Engine::new(store.clone());
        let resolve = Transaction::resolve(2, 1);
        let err = engine.process_transaction(resolve).await.unwrap_err();
        assert_eq!(
            err,
            EngineError::TransactionRefWrongClient {
                id: 2,
                client: 2,
                wrong_client: 1,
            }
        );
        // still under dispute
        assert_under_dispute(&store, 1, true);
        assert_under_dispute(&store, 2, true);
    }

    #[tokio::test]
    async fn on_chargeback_held_decrease_total_decrease_account_locked_and_no_dispute() {
        let account = Account::seeded(1, Amount::ZERO, dec!(10), false);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit_under_dispute(1, 1, dec!(10)));
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, dec!(10));
        assert_eq!(account.total, dec!(10));
        assert_eq!(store.transactions_len(), 1);
        assert_eq!(store.accounts_len(), 1);
        assert_under_dispute(&store, 1, true);

        let engine = Engine::new(store.clone());
        let chargeback = Transaction::chargeback(1, 1);
        let account = engine.process_transaction(chargeback).await.unwrap();

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, Amount::ZERO);
        assert!(account.locked);

        // no longer under dispute
        assert_under_dispute(&store, 1, false);
    }

    #[tokio::test]
    async fn on_chargeback_ignore_tx_if_not_under_dispute() {
        let account = Account::seeded(1, Amount::ZERO, dec!(10), false);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit(1, 1, dec!(10)));
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, dec!(10));
        assert_eq!(account.total, dec!(10));
        assert_eq!(store.transactions_len(), 1);
        assert_eq!(store.accounts_len(), 1);
        assert_under_dispute(&store, 1, false);

        let engine = Engine::new(store.clone());
        let chargeback = Transaction::chargeback(1, 1);
        let account = engine.process_transaction(chargeback).await.unwrap();

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, dec!(10));
        assert_eq!(account.total, dec!(10));
        // still no dispute
        assert_under_dispute(&store, 1, false);
    }

    #[tokio::test]
    async fn on_chargeback_ignore_transaction_if_ref_transaction_does_not_exist() {
        let account = Account::seeded(1, dec!(10), Amount::ZERO, false);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit_under_dispute(1, 1, dec!(10)));
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        assert_eq!(store.transactions_len(), 1);
        assert_eq!(store.accounts_len(), 1);
        assert_under_dispute(&store, 1, true);

        let engine = Engine::new(store.clone());
        let chargeback = Transaction::chargeback(2, 1);
        let account = engine.process_transaction(chargeback).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));

        assert_under_dispute(&store, 1, true);
    }

    #[tokio::test]
    async fn on_chargeback_the_referenced_tx_must_be_a_deposit() {
        // this case is not really possible in InMemoryStore
        // but it's useful to recreate it in case we use other kind of stores.
        let account = Account::seeded(1, dec!(10), Amount::ZERO, false);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit(1, 1, dec!(10)));
        // inserting a withdrawal directly.
        // this won't even happen with memory store, but it's useful to test the engine
        deposits.insert(2, Transaction::withdrawal(2, 1, dec!(1)));
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        assert_eq!(store.transactions_len(), 2);
        assert_eq!(store.accounts_len(), 1);

        let engine = Engine::new(store.clone());
        // referencing a withdrawal
        let chargeback = Transaction::chargeback(2, 1);
        let err = engine.process_transaction(chargeback).await.unwrap_err();
        // it should error
        assert_eq!(err, EngineError::WrongTransactionRef { id: 2 });
        // it should not change the account
        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        // it should not rollback
        assert_eq!(store.transactions_len(), 2);
        // no disputes
        assert_under_dispute(&store, 1, false);
    }

    #[tokio::test]
    async fn on_chargeback_error_if_tx_client_is_wrong() {
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit_under_dispute(1, 1, dec!(10)));
        deposits.insert(2, Transaction::deposit_under_dispute(2, 2, dec!(20)));
        let account_1 = Account::seeded(1, dec!(10), Amount::ZERO, false);
        let account_2 = Account::seeded(2, dec!(20), Amount::ZERO, false);
        let store = MemoryStore::seeded(Some(deposits), None);
        store.upsert_account(&account_1).await.unwrap();
        store.upsert_account(&account_2).await.unwrap();

        assert_eq!(store.transactions_len(), 2);
        assert_eq!(store.accounts_len(), 2);

        let engine = Engine::new(store.clone());
        let chargeback = Transaction::chargeback(2, 1);
        let err = engine.process_transaction(chargeback).await.unwrap_err();
        assert_eq!(
            err,
            EngineError::TransactionRefWrongClient {
                id: 2,
                client: 2,
                wrong_client: 1,
            }
        );
        // still under dispute
        assert_under_dispute(&store, 1, true);
        assert_under_dispute(&store, 2, true);
    }

    #[tokio::test]
    async fn rollback_transaction_under_dispute_state_if_tx_is_not_commited() {
        let account = Account::seeded(1, dec!(10), Amount::ZERO, false);
        let mut deposits = HashMap::new();
        deposits.insert(1, Transaction::deposit(1, 1, dec!(10)));
        let store = payments_engine_store_memory::MemoryStore::seeded(Some(deposits), None);

        store.upsert_account(&account).await.unwrap();

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));
        assert_eq!(store.transactions_len(), 1);
        assert_eq!(store.accounts_len(), 1);

        // provoke a failure when saving the account.
        // this will cause the transaction to be rolled back
        store.set_enable_upsert_account_failure(true);

        let engine = Engine::new(store.clone());

        // test dispute rollback
        let dispute = Transaction::dispute(1, 1);

        let err = engine
            .process_transaction(dispute.clone())
            .await
            .unwrap_err();

        assert_eq!(
            err,
            EngineError::TransactionNotCommited(StoreError::AccessError("Test Error".to_string()))
        );

        assert_eq!(account.available, dec!(10));
        assert_eq!(account.held, Amount::ZERO);
        assert_eq!(account.total, dec!(10));

        assert_under_dispute(&store, 1, false);

        // test resolve rollback
        store.set_enable_upsert_account_failure(false);

        let account = engine.process_transaction(dispute).await.unwrap();

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, dec!(10));
        assert_eq!(account.total, dec!(10));

        assert_under_dispute(&store, 1, true);

        store.set_enable_upsert_account_failure(true);

        let err = engine
            .process_transaction(Transaction::resolve(1, 1))
            .await
            .unwrap_err();

        assert_eq!(
            err,
            EngineError::TransactionNotCommited(StoreError::AccessError("Test Error".to_string()))
        );

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, dec!(10));
        assert_eq!(account.total, dec!(10));

        assert_under_dispute(&store, 1, true);

        // test chargeback rollback
        let err = engine
            .process_transaction(Transaction::chargeback(1, 1))
            .await
            .unwrap_err();

        assert_eq!(
            err,
            EngineError::TransactionNotCommited(StoreError::AccessError("Test Error".to_string()))
        );

        assert_eq!(account.available, Amount::ZERO);
        assert_eq!(account.held, dec!(10));
        assert_eq!(account.total, dec!(10));

        assert_under_dispute(&store, 1, true);
    }
}
