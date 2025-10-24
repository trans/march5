use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use march5::effect::{self, EffectCanon};
use march5::{cid, create_store, derive_db_path, open_store};

#[derive(Parser)]
#[command(name = "march5", version, about = "March α₅ CLI tooling")]
struct Cli {
    /// Path to an existing March database
    #[arg(short = 'd', long = "db", global = true, value_name = "PATH")]
    store: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new March database initialised with schema and PRAGMAs
    New {
        /// Project name or path for the database file
        name: String,
    },
    /// Manage effect descriptors
    Effect {
        #[command(subcommand)]
        command: EffectCommand,
    },
}

#[derive(Subcommand)]
enum EffectCommand {
    /// Insert a canonical effect descriptor into the object store
    Add {
        /// Human-readable effect name
        name: String,
        /// Optional documentation string
        #[arg(long)]
        doc: Option<String>,
    },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::New { name } => cmd_new(&name),
        Command::Effect { command } => {
            let store_path = require_store_path(cli.store.as_deref())?;
            cmd_effect(store_path, command)
        }
    }
}

fn cmd_new(name: &str) -> Result<()> {
    let path = derive_db_path(name);
    let conn = create_store(&path)?;
    drop(conn);
    println!("created march database at {}", path.display());
    Ok(())
}

fn cmd_effect(store: &Path, command: EffectCommand) -> Result<()> {
    match command {
        EffectCommand::Add { name, doc } => {
            let conn = open_store(store)?;
            let spec = EffectCanon {
                name: &name,
                doc: doc.as_deref(),
            };
            let outcome = effect::store_effect(&conn, &spec)?;
            let cid_hex = cid::to_hex(&outcome.cid);
            if outcome.inserted {
                println!("stored effect `{name}` with cid {cid_hex}");
            } else {
                println!("effect `{name}` already present with cid {cid_hex}");
            }
        }
    }
    Ok(())
}

fn require_store_path(path: Option<&Path>) -> Result<&Path> {
    match path {
        Some(p) => Ok(p),
        None => bail!("specify --db PATH for this command"),
    }
}
