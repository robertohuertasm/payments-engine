use futures::StreamExt;
use payments_engine_core::engine::Engine;
use payments_engine_csv::{read_csv_async, write_csv_async, AsyncReader, AsyncWriter};
use tracing::instrument;

/// Processes all the transactions coming from an async reader
/// and writes the results to an async writer.
/// Note that this function is generic over a [`Engine`] implementation.
#[instrument(skip(reader, writer, engine))]
pub async fn process_transactions<E: Engine>(
    reader: &mut AsyncReader,
    writer: &mut AsyncWriter,
    engine: E,
) -> anyhow::Result<()> {
    let mut transaction_stream = read_csv_async(reader).await;

    while let Some(transaction) = transaction_stream.next().await {
        match transaction {
            Ok(transaction) => {
                if let Err(e) = engine.process_transaction(transaction).await {
                    tracing::error!(error=?e, "Error processing transaction: {}", e);
                }
            }
            Err(e) => tracing::error!("CSV deserialization error: {}", e),
        }
    }

    let report = engine.report().await?;
    write_csv_async(writer, report).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use payments_engine::Engine;
    use payments_engine_store_memory::MemoryStore;
    use tokio::io::BufWriter;

    #[tokio::test]
    async fn it_works() {
        let mut input = r"
        type,client,tx,amount
        deposit,1,1,100
        withdrawal,1,2,50
        deposit,2,3,100
        deposit,1,4,200
        dispute,1,4
        resolve,1,4
        dispute,2,3
        chargeback,2,3
        dispute,1,3"
            .as_bytes();

        let mut output = BufWriter::new(Vec::<u8>::new());

        let engine = Engine::new(MemoryStore::default());

        process_transactions(&mut input, &mut output, engine)
            .await
            .unwrap();

        let buffer = output.into_inner();
        let csv = String::from_utf8_lossy(&buffer);

        // the order is not guaranteed
        let expected = (csv
            == "client,available,held,total,locked\n1,250,0,250,false\n2,0,0,0,true\n")
            || (csv == "client,available,held,total,locked\n2,0,0,0,true\n1,250,0,250,false\n");

        assert!(expected);
    }
}
