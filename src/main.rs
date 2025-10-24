use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use march5::effect::{self, EffectCanon};
use march5::prim::{self, PrimCanon};
use march5::{cid, create_store, derive_db_path, open_store, put_name};

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
    /// Manage primitive descriptors
    Prim {
        #[command(subcommand)]
        command: PrimCommand,
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

#[derive(Subcommand)]
enum PrimCommand {
    /// Insert or update a primitive descriptor
    Add {
        /// Logical primitive name
        name: String,
        /// Repeat per argument type (left-to-right)
        #[arg(long = "param", value_name = "TYPE")]
        params: Vec<String>,
        /// Repeat per result type
        #[arg(long = "result", value_name = "TYPE")]
        results: Vec<String>,
        /// Optional attribute entries in key=value form
        #[arg(long = "attr", value_name = "KEY=VALUE")]
        attrs: Vec<String>,
        /// Skip name_index registration
        #[arg(long = "no-register")]
        no_register: bool,
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
        Command::Prim { command } => {
            let store_path = require_store_path(cli.store.as_deref())?;
            cmd_prim(store_path, command)
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

fn cmd_prim(store: &Path, command: PrimCommand) -> Result<()> {
    match command {
        PrimCommand::Add {
            name,
            params,
            results,
            attrs,
            no_register,
        } => {
            let conn = open_store(store)?;
            let attrs_pairs = parse_attrs(&attrs)?;
            let param_refs: Vec<&str> = params.iter().map(|s| s.as_str()).collect();
            let result_refs: Vec<&str> = results.iter().map(|s| s.as_str()).collect();
            let attr_refs: Vec<(&str, &str)> = attrs_pairs
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            let spec = PrimCanon {
                params: &param_refs,
                results: &result_refs,
                attrs: &attr_refs,
            };
            let outcome = prim::store_prim(&conn, &spec)?;
            if !no_register {
                put_name(&conn, "prim", &name, &outcome.cid)?;
            }
            let cid_hex = cid::to_hex(&outcome.cid);
            if outcome.inserted {
                println!("stored prim `{name}` with cid {cid_hex}");
            } else {
                println!("prim `{name}` already present with cid {cid_hex}");
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

fn parse_attrs(entries: &[String]) -> Result<Vec<(String, String)>> {
    let mut pairs = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some((key, value)) = entry.split_once('=') else {
            bail!("invalid attr `{entry}`; expected key=value");
        };
        if key.is_empty() {
            bail!("attribute key cannot be empty in `{entry}`");
        }
        pairs.push((key.to_string(), value.to_string()));
    }
    Ok(pairs)
}
