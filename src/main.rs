use clap::Parser;
use loglens::api::{router, AppState};
use loglens::config::{Cli, Cmd, Config};
use loglens::db::Db;
use loglens::ingest::{gen_logs, Engine};
use loglens::metrics::Metrics;
use loglens::now_ms;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

type BoxErr = Box<dyn std::error::Error>;

#[tokio::main]
async fn main() {
    if let Err(e) = real_main().await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn real_main() -> Result<(), BoxErr> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::CreateTenant { name } => {
            std::fs::create_dir_all(&cli.data_dir)?;
            let db = Db::open(&cli.data_dir.join("meta.db"))?;
            let key = db.create_tenant(&name)?;
            println!("tenant '{name}' created");
            println!("API key (shown once, not stored): {key}");
        }
        Cmd::GenLogs { count, seed } => {
            let start = now_ms().saturating_sub(count as u64 * 7);
            for d in gen_logs(count, seed, start) {
                println!("{}", serde_json::to_string(&d)?);
            }
        }
        Cmd::Serve { addr } => serve(cli.data_dir, addr).await?,
    }
    Ok(())
}

async fn serve(data_dir: PathBuf, addr: String) -> Result<(), BoxErr> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    std::fs::create_dir_all(&data_dir)?;
    let metrics = Arc::new(Metrics::default());
    let db = Arc::new(Db::open(&data_dir.join("meta.db"))?);
    let (engine, rx) = Engine::new(Config::new(data_dir), metrics.clone())?;
    let writer = engine.clone();
    std::thread::spawn(move || writer.run(rx));
    let app = router(AppState {
        engine: engine.clone(),
        db,
        metrics,
    });
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await?;
    engine.flush().await;
    tracing::info!("memtables flushed, bye");
    Ok(())
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}
