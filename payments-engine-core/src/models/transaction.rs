use crate::common::{Amount, ClientId};
use serde::{Deserialize, Serialize};

/// Id of a [`Transaction`], which is guaranteed to be unique.
pub type TransactionId = u32;

/// Holds information about the transaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransactionInfo {
    /// Id of the transaction, globally unique.
    pub id: TransactionId,
    /// Id of the client.
    pub client_id: ClientId,
}

impl TransactionInfo {
    /// Creates a new [`TransactionInfo`] with the given parameters.
    #[must_use]
    pub const fn new(id: TransactionId, client_id: ClientId) -> Self {
        Self { id, client_id }
    }
}

/// A [`Transaction`] to be processed by the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Transaction {
    /// Credit to the client's asset account. It should increase the available and total funds of the client account.
    Deposit {
        info: TransactionInfo,
        amount: Amount,
        under_dispute: bool,
    },
    /// Debit to the client's asset account. It should decrease the available and total funds of the client account.
    Withdrawal {
        info: TransactionInfo,
        amount: Amount,
    },
    /// Represents a client's claim that a transaction was erroneus and should be reversed.
    /// Available funds should decrease, held funds should increase and total funds should remain the same.
    Dispute { info: TransactionInfo },
    /// Represents a resolution to a dispute, releasing the associated held funds.
    /// Held funds should decrease and available funds should increase. Total funds should remain the same.
    Resolve { info: TransactionInfo },
    /// Represents the client reversing a transaction after a dispute.
    /// Held funds and total funds should decrease. The client's account gets immediately frozen.
    ChargeBack { info: TransactionInfo },
}

impl Transaction {
    /// Creates a new [`Transaction::Deposit`] with the given parameters.
    #[must_use]
    pub const fn deposit(id: TransactionId, client_id: ClientId, amount: Amount) -> Self {
        Self::Deposit {
            info: TransactionInfo::new(id, client_id),
            amount,
            under_dispute: false,
        }
    }

    /// Creates a new [`Transaction::Deposit`] with the given parameters and sets the under dispute flag.
    #[must_use]
    pub const fn deposit_under_dispute(
        id: TransactionId,
        client_id: ClientId,
        amount: Amount,
    ) -> Self {
        Self::Deposit {
            info: TransactionInfo::new(id, client_id),
            amount,
            under_dispute: true,
        }
    }

    /// Sets the ``under_dispute`` flag to true or false if the [`Transaction`] is a [`Transaction::Deposit`].
    pub fn set_under_dispute(&mut self, disputed: bool) {
        if let Transaction::Deposit {
            ref mut under_dispute,
            ..
        } = self
        {
            *under_dispute = disputed;
        }
    }

    /// Toggles the ``under_dispute`` flag if [`Transaction`] is a [`Transaction::Deposit`].
    pub fn toggle_under_dispute(&mut self) {
        if let Transaction::Deposit {
            ref mut under_dispute,
            ..
        } = self
        {
            *under_dispute = !*under_dispute;
        }
    }

    /// Creates a new [`Transaction::Withdrawal`] with the given parameters.
    #[must_use]
    pub const fn withdrawal(id: TransactionId, client_id: ClientId, amount: Amount) -> Self {
        Self::Withdrawal {
            info: TransactionInfo::new(id, client_id),
            amount,
        }
    }

    /// Creates a new [`Transaction::Dispute`] with the given parameters.
    #[must_use]
    pub const fn dispute(id: TransactionId, client_id: ClientId) -> Self {
        Self::Dispute {
            info: TransactionInfo::new(id, client_id),
        }
    }

    /// Creates a new [`Transaction::Resolve`] with the given parameters.
    #[must_use]
    pub const fn resolve(id: TransactionId, client_id: ClientId) -> Self {
        Self::Resolve {
            info: TransactionInfo::new(id, client_id),
        }
    }

    /// Creates a new [`Transaction::ChargeBack`] with the given parameters.
    #[must_use]
    pub const fn chargeback(id: TransactionId, client_id: ClientId) -> Self {
        Self::ChargeBack {
            info: TransactionInfo::new(id, client_id),
        }
    }

    /// Returns a reference of the [`TransactionInfo`] of this [`Transaction`].
    #[must_use]
    pub const fn info(&self) -> &TransactionInfo {
        match self {
            Self::Deposit { info, .. }
            | Self::Withdrawal { info, .. }
            | Self::Dispute { info }
            | Self::Resolve { info }
            | Self::ChargeBack { info } => info,
        }
    }

    /// Returns the [`Amount`] associated to this [`Transaction`].
    #[must_use]
    pub const fn amount(&self) -> Option<Amount> {
        match self {
            Self::Deposit { amount, .. } | Self::Withdrawal { amount, .. } => Some(*amount),
            _ => None,
        }
    }

    /// Returns true if the amount is negative.
    #[must_use]
    pub fn has_negative_amount(&self) -> bool {
        self.amount().map_or(false, |amount| amount < Amount::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn tx_is_mutated_when_setting_under_deposit() {
        let mut deposit = Transaction::deposit(1, 1, dec!(1));
        deposit.set_under_dispute(true);
        assert_eq!(deposit, Transaction::deposit_under_dispute(1, 1, dec!(1)));
    }

    #[tokio::test]
    async fn tx_is_mutated_when_toggling_under_deposit() {
        let mut deposit_not_under = Transaction::deposit(1, 1, dec!(1));
        let mut deposit_under = Transaction::deposit_under_dispute(2, 1, dec!(1));
        deposit_not_under.toggle_under_dispute();
        deposit_under.toggle_under_dispute();
        assert_eq!(
            deposit_not_under,
            Transaction::deposit_under_dispute(1, 1, dec!(1))
        );
        assert_eq!(deposit_under, Transaction::deposit(2, 1, dec!(1)));
    }

    #[tokio::test]
    async fn has_negative_amount_works() {
        let deposit_negative = Transaction::deposit(1, 1, dec!(-1));
        let deposit_positive = Transaction::deposit_under_dispute(2, 1, dec!(1));
        let deposit_zero = Transaction::deposit_under_dispute(2, 1, Amount::ZERO);
        let dispute = Transaction::dispute(2, 1);

        assert!(deposit_negative.has_negative_amount());
        assert!(!deposit_positive.has_negative_amount());
        assert!(!deposit_zero.has_negative_amount());
        assert!(!dispute.has_negative_amount());
    }
}
