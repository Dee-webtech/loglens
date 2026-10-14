use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "loglens", version, about = "Lightweight multi-tenant log search")]
pub struct Cli {
    #[arg(long, global = true, default_value = "data")]
    pub data_dir: PathBuf,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    Serve {
        #[arg(long, default_value = "127.0.0.1:8080")]
        addr: String,
    },
    CreateTenant {
        name: String,
    },
    GenLogs {
        #[arg(long, default_value_t = 1000)]
        count: usize,
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
}

#[derive(Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub seal_bytes: usize,
    pub queue_cap: usize,
}

impl Config {
    pub fn new(data_dir: PathBuf) -> Config {
        Config {
            data_dir,
            seal_bytes: 4 << 20,
            queue_cap: 1024,
        }
    }
}