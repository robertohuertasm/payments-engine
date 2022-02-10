use futures::StreamExt;
use payments_engine_core::account::Account;
use tracing::instrument;

pub type AsyncWriter = dyn tokio::io::AsyncWrite + Send + Sync + Unpin;

/// Writes a CSV asynchronously with information about the [`Account`] balances.
#[instrument(skip(writer, account_stream))]
pub async fn write_csv_async(
    writer: &mut AsyncWriter,
    mut account_stream: impl futures::Stream<Item = Account> + Send + Unpin,
) -> anyhow::Result<()> {
    let mut writer = csv_async::AsyncSerializer::from_writer(writer);

    while let Some(mut account) = account_stream.next().await {
        account.to_max_display_precision();
        writer.serialize(account).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use payments_engine_core::dec;
    use tokio::io::BufWriter;

    #[tokio::test]
    async fn writes_csv_async_ok() {
        let input = vec![
            Account::seeded(1, dec!(23.2320), dec!(0.0000), false),
            Account::seeded(2, dec!(4.0), dec!(2.2101), true),
            Account::seeded(3, dec!(23.2320), dec!(0.00000000), false),
        ];
        let account_stream = futures::stream::iter(input);
        let mut writer = BufWriter::new(Vec::<u8>::new());

        let result = write_csv_async(&mut writer, account_stream).await;

        assert!(result.is_ok());

        let buffer = writer.into_inner();
        let csv = String::from_utf8_lossy(&buffer);

        assert_eq!(
            csv,
            "client,available,held,total,locked\n1,23.2320,0.0000,23.2320,false\n2,4.0,2.2101,6.2101,true\n3,23.2320,0.0000,23.2320,false\n"
        );
    }

    #[tokio::test]
    async fn limits_precision_to_four_digits() {
        let input = vec![Account::seeded(1, dec!(23.2320), dec!(1.0000), false)];
        let account_stream = futures::stream::iter(input);
        let mut writer = BufWriter::new(Vec::<u8>::new());

        let result = write_csv_async(&mut writer, account_stream).await;

        assert!(result.is_ok());

        let buffer = writer.into_inner();
        let csv = String::from_utf8_lossy(&buffer);

        assert_eq!(
            csv,
            "client,available,held,total,locked\n1,23.2320,1.0000,24.2320,false\n"
        );
    }

    #[tokio::test]
    async fn does_not_add_4_places_if_precision_is_less_four_digits() {
        let input = vec![Account::seeded(1, dec!(23.2320), dec!(1.0), false)];
        let account_stream = futures::stream::iter(input);
        let mut writer = BufWriter::new(Vec::<u8>::new());

        let result = write_csv_async(&mut writer, account_stream).await;

        assert!(result.is_ok());

        let buffer = writer.into_inner();
        let csv = String::from_utf8_lossy(&buffer);

        assert_eq!(
            csv,
            "client,available,held,total,locked\n1,23.2320,1.0,24.2320,false\n"
        );
    }
}
