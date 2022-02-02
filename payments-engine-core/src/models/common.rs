use rust_decimal::Decimal;

/// Id of Client, which is guaranteed to be unique.
pub type ClientId = u16;
/// Decimal value suitable for financial calculations.
pub type Amount = Decimal;
