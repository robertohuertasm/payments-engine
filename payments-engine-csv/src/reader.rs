use super::transaction::Transaction;
use payments_engine_core::transaction::Transaction as EngineTransaction;
use tokio_stream::StreamExt;
use tracing::instrument;

pub type AsyncReader = dyn tokio::io::AsyncRead + Send + Sync + Unpin;

/// Reads a CSV file asynchronously.
#[instrument(skip(reader))]
pub async fn read_csv_async(
    reader: &mut AsyncReader,
) -> impl futures::Stream<Item = Result<EngineTransaction, anyhow::Error>> + '_ {
    csv_async::AsyncReaderBuilder::new()
        .flexible(true)
        .trim(csv_async::Trim::All)
        .create_reader(reader)
        .into_records()
        .map(|record| {
            record
                .and_then(|r| {
                    r.deserialize::<Transaction>(None)
                        .map(std::convert::Into::into)
                })
                .map_err(anyhow::Error::from)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{FutureExt, TryStreamExt};
    use payments_engine_core::dec;

    const ERR: &'static str = "err";

    #[tokio::test]
    async fn reads_csv_async_works_ok() {
        let mut input = r"
        type,client,tx,amount
        deposit,1,10,100
        deposito,1,11,100.0
        withdrawal,1,12,200.0
        resolve,1,13,
        resolve,1,14, 100.000
        dispute,1,15,
        dispute,1,16, 100.000
        chargeback,1,17,
        chargeback,1,18, 100.000
        deposit,1,19,5.001
        withdrawal,1,20,43.3423
        withdrawal,1,21,
        deposit,1,22,"
            .as_bytes();

        let result = read_csv_async(&mut input)
            .map(|tx| tx.map_err(|_| ERR))
            .await
            .collect::<Vec<_>>()
            .await;

        let expected = vec![
            Ok(EngineTransaction::deposit(10, 1, dec!(100.00))),
            Err(ERR),
            Ok(EngineTransaction::withdrawal(12, 1, dec!(200.00))),
            Ok(EngineTransaction::resolve(13, 1)),
            Ok(EngineTransaction::resolve(14, 1)),
            Ok(EngineTransaction::dispute(15, 1)),
            Ok(EngineTransaction::dispute(16, 1)),
            Ok(EngineTransaction::chargeback(17, 1)),
            Ok(EngineTransaction::chargeback(18, 1)),
            Ok(EngineTransaction::deposit(19, 1, dec!(5.001))),
            Ok(EngineTransaction::withdrawal(20, 1, dec!(43.3423))),
            Ok(EngineTransaction::withdrawal(21, 1, dec!(0.0))),
            Ok(EngineTransaction::deposit(22, 1, dec!(0.0))),
        ];

        assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn reads_csv_async_works_ok_with_untrimmed_content() {
        let mut input = r"
        type    ,client,        tx,     amount
            deposit   ,1  , 10,   100
          deposito,      1, 11 , 100.0,
        withdrawal,1,12,   200.0
        resolve,1,     13,
        resolve,1,    14, 100.000,
        dispute   ,1,   15,
         dispute  ,1,    16,   100.000,
           chargeback ,1,17,
         chargeback     ,1, 18, 100.000,
           deposit  ,1, 19, 5.001
        withdrawal, 1,    20,  43.3423
         withdrawal ,1, 21   ,  
            deposit   , 1, 22,  "
            .as_bytes();

        let result = read_csv_async(&mut input)
            .map(|tx| tx.map_err(|_| ERR))
            .await
            .collect::<Vec<_>>()
            .await;

        let expected = vec![
            Ok(EngineTransaction::deposit(10, 1, dec!(100.00))),
            Err(ERR),
            Ok(EngineTransaction::withdrawal(12, 1, dec!(200.00))),
            Ok(EngineTransaction::resolve(13, 1)),
            Ok(EngineTransaction::resolve(14, 1)),
            Ok(EngineTransaction::dispute(15, 1)),
            Ok(EngineTransaction::dispute(16, 1)),
            Ok(EngineTransaction::chargeback(17, 1)),
            Ok(EngineTransaction::chargeback(18, 1)),
            Ok(EngineTransaction::deposit(19, 1, dec!(5.001))),
            Ok(EngineTransaction::withdrawal(20, 1, dec!(43.3423))),
            Ok(EngineTransaction::withdrawal(21, 1, dec!(0.0))),
            Ok(EngineTransaction::deposit(22, 1, dec!(0.0))),
        ];

        assert_eq!(result, expected)
    }

    #[tokio::test]
    async fn reads_csv_async_works_ok_with_no_trailing_comma() {
        let mut input = r"
        type,client,tx,amount
        dispute,1,10
        resolve,1,11"
            .as_bytes();

        let result = read_csv_async(&mut input)
            .map(|tx| tx.map_err(|e| e.to_string()))
            .await
            .collect::<Vec<_>>()
            .await;

        let expected = vec![
            Ok(EngineTransaction::dispute(10, 1)),
            Ok(EngineTransaction::resolve(11, 1)),
        ];

        assert_eq!(result, expected)
    }
}
