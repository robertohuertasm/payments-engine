use payments_engine_core::{
    common::{Amount, ClientId},
    transaction::{Transaction as EngineTransaction, TransactionId, TransactionInfo},
};
use serde::{Deserialize, Serialize};

/// The different [`Transaction`] variants
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TransactionKind {
    /// Credit to the client's asset account.
    Deposit,
    /// Debit to the client's asset account.
    Withdrawal,
    // Represents a client's claim that a transaction was erroneus and should be reversed.
    Dispute,
    /// Represents a resolution to a dispute, releasing the associated held funds.
    Resolve,
    /// Represents the client reversing a transaction after a dispute.
    ChargeBack,
}

/// Represents a client's [`Account`] transaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transaction {
    /// The [`Transaction`] variant.
    #[serde(rename = "type")]
    pub kind: TransactionKind,
    /// The client ID.
    #[serde(rename = "client")]
    pub client_id: ClientId,
    /// The transaction ID.
    #[serde(rename = "tx")]
    pub id: TransactionId,
    /// The [`Transaction`] amount.
    /// It will be informed only for [`TransactionKind::Deposit`] and [`TransactionKind::Withdrawal`]
    #[serde(default)]
    pub amount: Option<Amount>,
}

impl From<Transaction> for EngineTransaction {
    fn from(tx: Transaction) -> Self {
        match tx.kind {
            TransactionKind::Deposit => Self::Deposit {
                info: TransactionInfo::new(tx.id, tx.client_id),
                amount: tx.amount.unwrap_or_default(),
                under_dispute: false,
            },
            TransactionKind::Withdrawal => Self::Withdrawal {
                info: TransactionInfo::new(tx.id, tx.client_id),
                amount: tx.amount.unwrap_or_default(),
            },
            TransactionKind::Dispute => Self::Dispute {
                info: TransactionInfo::new(tx.id, tx.client_id),
            },
            TransactionKind::Resolve => Self::Resolve {
                info: TransactionInfo::new(tx.id, tx.client_id),
            },
            TransactionKind::ChargeBack => Self::ChargeBack {
                info: TransactionInfo::new(tx.id, tx.client_id),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use payments_engine_core::dec;

    #[test]
    fn conversion_to_deposit_works() {
        let transaction = Transaction {
            kind: TransactionKind::Deposit,
            id: 1,
            client_id: 1,
            amount: Some(dec!(1.0000)),
        };

        let engine_transaction: EngineTransaction = transaction.clone().into();

        assert_eq!(
            engine_transaction,
            EngineTransaction::deposit(transaction.id, transaction.client_id, dec!(1.0000))
        );
    }

    #[test]
    fn conversion_to_deposit_with_no_amount_defaults_to_zero() {
        let transaction = Transaction {
            kind: TransactionKind::Deposit,
            id: 1,
            client_id: 1,
            amount: None,
        };

        let engine_transaction: EngineTransaction = transaction.clone().into();

        assert_eq!(
            engine_transaction,
            EngineTransaction::deposit(transaction.id, transaction.client_id, dec!(0.0000))
        );
    }

    #[test]
    fn conversion_to_withdrawal_works() {
        let transaction = Transaction {
            kind: TransactionKind::Withdrawal,
            id: 1,
            client_id: 1,
            amount: Some(dec!(1.0000)),
        };

        let engine_transaction: EngineTransaction = transaction.clone().into();

        assert_eq!(
            engine_transaction,
            EngineTransaction::withdrawal(transaction.id, transaction.client_id, dec!(1.0000))
        );
    }

    #[test]
    fn conversion_to_withdrawal_with_no_amount_defaults_to_zero() {
        let transaction = Transaction {
            kind: TransactionKind::Withdrawal,
            id: 1,
            client_id: 1,
            amount: None,
        };

        let engine_transaction: EngineTransaction = transaction.clone().into();

        assert_eq!(
            engine_transaction,
            EngineTransaction::withdrawal(transaction.id, transaction.client_id, dec!(0.0000))
        );
    }

    #[test]
    fn conversion_to_non_deposit_or_withdrawal_works() {
        let dispute = Transaction {
            kind: TransactionKind::Dispute,
            id: 1,
            client_id: 1,
            amount: None,
        };

        let resolve = Transaction {
            kind: TransactionKind::Resolve,
            id: 1,
            client_id: 1,
            amount: None,
        };

        let chargeback = Transaction {
            kind: TransactionKind::ChargeBack,
            id: 1,
            client_id: 1,
            amount: None,
        };

        let engine_dispute: EngineTransaction = dispute.clone().into();
        let engine_resolve: EngineTransaction = resolve.clone().into();
        let engine_chargeback: EngineTransaction = chargeback.clone().into();

        assert_eq!(
            engine_dispute,
            EngineTransaction::dispute(dispute.id, dispute.client_id),
        );

        assert_eq!(
            engine_resolve,
            EngineTransaction::resolve(resolve.id, resolve.client_id),
        );

        assert_eq!(
            engine_chargeback,
            EngineTransaction::chargeback(chargeback.id, chargeback.client_id),
        );
    }
}
