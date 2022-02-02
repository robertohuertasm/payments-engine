mod process;

use payments_engine::Engine;
use payments_engine_store_memory::MemoryStore;
use std::env::current_dir;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "🏹 Payments Engine CLI",
    author("💻  Roberto Huertas <roberto.huertas@outlook.com"),
    long_about = "🧰  Small utility to process payments from a csv file"
)]
pub struct Cli {
    /// The path to the csv file containing the transactions
    #[structopt(parse(from_os_str))]
    pub path: std::path::PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::from_args();
    dotenv::dotenv().ok();
    set_up_tracing();
    tracing::info!("Starting the Payments Engine CLI");
    let file_path = current_dir()?.join(cli.path);

    let mut reader = tokio::fs::File::open(file_path).await?;
    let engine = Engine::new(MemoryStore::default());
    let mut writer = tokio::io::stdout();

    process::process_transactions(&mut reader, &mut writer, engine).await?;
    Ok(())
}

fn set_up_tracing() {
    let tracing = tracing_subscriber::fmt()
        .with_timer(tracing_subscriber::fmt::time::UtcTime::rfc_3339())
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env());

    if cfg!(debug_assertions) {
        tracing.pretty().init();
    } else {
        tracing.json().init();
    }
}
