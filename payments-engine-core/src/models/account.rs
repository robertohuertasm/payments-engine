use crate::common::{Amount, ClientId};
use serde::{Deserialize, Serialize};

const MAX_DISPLAY_PRECISION: u32 = 4;

/// Represents the current state of the client's account.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Account {
    /// id of the client.
    pub client: ClientId,
    /// The current available funds of the account.
    pub available: Amount,
    /// The current held funds of the account.
    pub held: Amount,
    /// The total funds of the account, available and held.
    pub total: Amount,
    /// Whether the account is locked. An account is locked if a charge back occurs.
    pub locked: bool,
}

impl Account {
    /// Creates a new [`Account`] for the specified client.
    #[must_use]
    pub const fn new(client: ClientId) -> Self {
        Self {
            client,
            available: Amount::ZERO,
            held: Amount::ZERO,
            total: Amount::ZERO,
            locked: false,
        }
    }

    /// Creates a new [`Account`] with the specified arguments.
    #[must_use]
    pub fn seeded(client: ClientId, available: Amount, held: Amount, locked: bool) -> Self {
        Self {
            client,
            available,
            held,
            total: available + held,
            locked,
        }
    }

    /// Mutates the [`Account`] for displaying purposes and sets the ammounts up to 4 decimal places.
    pub fn to_max_display_precision(&mut self) {
        self.available = rescale_to_max_precision(self.available);
        self.held = rescale_to_max_precision(self.held);
        self.total = rescale_to_max_precision(self.total);
    }
}

fn rescale_to_max_precision(mut amount: Amount) -> Amount {
    if amount.scale() > MAX_DISPLAY_PRECISION {
        amount.rescale(MAX_DISPLAY_PRECISION);
    }
    amount
}
