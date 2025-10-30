use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use clap::{Parser, Subcommand};
use rusqlite::Connection;

use march5::effect::{self, EffectCanon};
use march5::iface::{self, IfaceCanon, IfaceSymbol};
use march5::namespace::{self, NamespaceCanon, NamespaceExport};
use march5::node::{self, NodeCanon, NodeInput, NodeKind, NodePayload};
use march5::prim::{self, PrimCanon};
use march5::types::effect_mask;
use march5::word::{self, WordCanon};
use march5::{
    TypeTag, Value, cid, create_store, derive_db_path, get_name, load_object_cbor, open_store,
    put_name, run_word,
};
use serde::Deserialize;

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
    /// Manage interface descriptors
    Iface {
        #[command(subcommand)]
        command: IfaceCommand,
    },
    /// Manage namespaces
    Namespace {
        #[command(subcommand)]
        command: NamespaceCommand,
    },
    /// Manage graph nodes
    Node {
        #[command(subcommand)]
        command: NodeCommand,
    },
    /// Manage words (graph entrypoints)
    Word {
        #[command(subcommand)]
        command: WordCommand,
    },
    /// Interactive graph builder REPL
    Builder,
    /// Execute a word and print its result
    Run {
        name: String,
        /// Supply repeated --arg <i64> values for parameters
        #[arg(long = "arg")]
        args: Vec<i64>,
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
        /// Declared effect CIDs
        #[arg(long = "effect", value_name = "CID")]
        effects: Vec<String>,
        /// Skip name_index registration
        #[arg(long = "no-register")]
        no_register: bool,
    },
}

#[derive(Subcommand)]
enum IfaceCommand {
    /// Insert or update an interface descriptor
    Add {
        /// Optional name_index registration
        #[arg(long = "register", value_name = "NAME")]
        register: Option<String>,
        /// Export specifications of the form `name(param,...) -> result,... | effectCID,...`
        #[arg(long = "name", required = true, value_name = "SPEC")]
        names: Vec<String>,
        /// Skip name_index registration
        #[arg(long = "no-register")]
        no_register: bool,
    },
    /// List registered interface names
    List {
        #[arg(long = "prefix")]
        prefix: Option<String>,
    },
    /// Show canonical JSON for an interface
    Show { name: String },
}

#[derive(Subcommand)]
enum NamespaceCommand {
    /// Insert or update a namespace descriptor
    Add {
        /// Optional namespace name for name_index registration
        #[arg(long = "name")]
        name: Option<String>,
        /// Optional precomputed interface CID; omit to derive automatically
        #[arg(long = "iface")]
        iface: Option<String>,
        /// Required interface CIDs for imports
        #[arg(long = "import", value_name = "CID")]
        imports: Vec<String>,
        /// Exported words as name=wordCID pairs
        #[arg(long = "export", value_name = "NAME=CID")]
        exports: Vec<String>,
        /// Skip name registration
        #[arg(long = "no-register")]
        no_register: bool,
    },
    /// List registered namespaces
    List {
        #[arg(long = "prefix")]
        prefix: Option<String>,
    },
    /// Show canonical JSON for a namespace by name
    Show { name: String },
}

#[derive(Subcommand)]
enum NodeCommand {
    /// Insert a literal node (currently supports i64 literals)
    Lit {
        #[arg(long = "ty")]
        ty: String,
        #[arg(long = "value")]
        value: i64,
        #[arg(long = "effect")]
        effects: Vec<String>,
    },
    /// Insert a primitive node
    Prim {
        #[arg(long = "ty")]
        ty: String,
        #[arg(long = "prim")]
        prim: String,
        #[arg(long = "input", value_name = "CID:PORT")]
        inputs: Vec<String>,
        #[arg(long = "effect")]
        effects: Vec<String>,
    },
    /// Insert a call node referencing a word CID
    Call {
        #[arg(long = "ty")]
        ty: String,
        #[arg(long = "word")]
        word: String,
        #[arg(long = "input", value_name = "CID:PORT")]
        inputs: Vec<String>,
        #[arg(long = "effect")]
        effects: Vec<String>,
    },
    /// Insert an argument node (ARG)
    Arg {
        #[arg(long = "ty")]
        ty: String,
        #[arg(long = "index")]
        index: u32,
        #[arg(long = "effect")]
        effects: Vec<String>,
    },
    /// Insert a load-global node
    LoadGlobal {
        #[arg(long = "ty")]
        ty: String,
        #[arg(long = "global")]
        global: String,
        #[arg(long = "effect")]
        effects: Vec<String>,
    },
}

#[derive(Subcommand)]
enum WordCommand {
    /// Insert or update a word descriptor
    Add {
        /// Optional human/name-index entry (e.g. namespace/foo)
        #[arg(long = "name")]
        name: Option<String>,
        #[arg(long = "root")]
        root: String,
        #[arg(long = "param", value_name = "TYPE")]
        params: Vec<String>,
        #[arg(long = "result", value_name = "TYPE")]
        results: Vec<String>,
        /// Declared effect CIDs
        #[arg(long = "effect", value_name = "CID")]
        effects: Vec<String>,
        #[arg(long = "no-register")]
        no_register: bool,
    },
    /// Show a word's canonical JSON by name
    Show { name: String },
    /// List registered words (optionally filtered by prefix)
    List {
        #[arg(long = "prefix")]
        prefix: Option<String>,
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
        Command::Iface { command } => {
            let store_path = require_store_path(cli.store.as_deref())?;
            cmd_iface(store_path, command)
        }
        Command::Namespace { command } => {
            let store_path = require_store_path(cli.store.as_deref())?;
            cmd_namespace(store_path, command)
        }
        Command::Node { command } => {
            let store_path = require_store_path(cli.store.as_deref())?;
            cmd_node(store_path, command)
        }
        Command::Word { command } => {
            let store_path = require_store_path(cli.store.as_deref())?;
            cmd_word(store_path, command)
        }
        Command::Builder => {
            let store_path = require_store_path(cli.store.as_deref())?;
            cmd_builder(store_path)
        }
        Command::Run { name, args } => {
            let store_path = require_store_path(cli.store.as_deref())?;
            cmd_run(store_path, &name, &args)
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
            effects,
            no_register,
        } => {
            let conn = open_store(store)?;
            let param_tags = parse_type_tags(&params)?;
            let result_tags = parse_type_tags(&results)?;
            let effect_cids = parse_cid_list(effects.iter().map(|s| s.as_str()))?;
            let spec = PrimCanon {
                params: &param_tags,
                results: &result_tags,
                effects: effect_cids.as_slice(),
                effect_mask: if effect_cids.is_empty() {
                    effect_mask::NONE
                } else {
                    effect_mask::IO
                },
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

fn cmd_iface(store: &Path, command: IfaceCommand) -> Result<()> {
    match command {
        IfaceCommand::Add {
            register,
            names,
            no_register,
        } => {
            let conn = open_store(store)?;
            let mut parsed = Vec::with_capacity(names.len());
            for spec in names {
                parsed.push(parse_iface_spec(&spec)?);
            }
            let iface = IfaceCanon { names: parsed };
            let outcome = iface::store_iface(&conn, &iface)?;
            if !no_register {
                if let Some(name) = &register {
                    put_name(&conn, "iface", name, &outcome.cid)?;
                }
            }
            let cid_hex = cid::to_hex(&outcome.cid);
            if outcome.inserted {
                println!("stored iface with cid {cid_hex}");
            } else {
                println!("iface already present with cid {cid_hex}");
            }
        }
        IfaceCommand::List { prefix } => {
            let conn = open_store(store)?;
            list_scope(
                &conn,
                "iface",
                prefix.as_deref(),
                "no interfaces registered",
            )?;
        }
        IfaceCommand::Show { name } => {
            let conn = open_store(store)?;
            show_named_object(&conn, "iface", "interface", &name)?;
        }
    }
    Ok(())
}

fn cmd_namespace(store: &Path, command: NamespaceCommand) -> Result<()> {
    match command {
        NamespaceCommand::Add {
            name,
            iface,
            imports,
            exports,
            no_register,
        } => {
            let conn = open_store(store)?;
            let imports = parse_cid_list(imports.iter().map(|s| s.as_str()))?;
            let export_pairs = parse_exports(&exports)?;
            let iface_cid = if let Some(iface_hex) = iface {
                cid::from_hex(&iface_hex)?
            } else {
                let iface_canon = iface::derive_from_exports(&conn, &export_pairs)?;
                iface::store_iface(&conn, &iface_canon)?.cid
            };
            let exports = export_pairs
                .iter()
                .map(|(name, word_cid)| NamespaceExport {
                    name: name.clone(),
                    word: *word_cid,
                })
                .collect();
            let ns = NamespaceCanon {
                imports,
                exports,
                iface: iface_cid,
            };
            let outcome = namespace::store_namespace(&conn, &ns)?;
            if !no_register {
                if let Some(name) = &name {
                    put_name(&conn, "namespace", name, &outcome.cid)?;
                }
            }
            let cid_hex = cid::to_hex(&outcome.cid);
            if outcome.inserted {
                println!("stored namespace with cid {cid_hex}");
            } else {
                println!("namespace already present with cid {cid_hex}");
            }
        }
        NamespaceCommand::List { prefix } => {
            let conn = open_store(store)?;
            list_scope(
                &conn,
                "namespace",
                prefix.as_deref(),
                "no namespaces registered",
            )?;
        }
        NamespaceCommand::Show { name } => {
            let conn = open_store(store)?;
            show_named_object(&conn, "namespace", "namespace", &name)?;
        }
    }
    Ok(())
}

fn cmd_node(store: &Path, command: NodeCommand) -> Result<()> {
    let conn = open_store(store)?;
    let outcome = match command {
        NodeCommand::Lit { ty, value, effects } => {
            let effects = parse_cid_list(effects.iter().map(|s| s.as_str()))?;
            let node = NodeCanon {
                kind: NodeKind::Lit,
                out: vec![ty],
                inputs: Vec::new(),
                vals: Vec::new(),
                deps: Vec::new(),
                effects,
                payload: NodePayload::LitI64(value),
            };
            node::store_node(&conn, &node)?
        }
        NodeCommand::Prim {
            ty,
            prim,
            inputs,
            effects,
        } => {
            let prim_cid = cid::from_hex(&prim)?;
            let inputs = parse_inputs(&inputs)?;
            let effects = parse_cid_list(effects.iter().map(|s| s.as_str()))?;
            let node = NodeCanon {
                kind: NodeKind::Prim,
                out: vec![ty],
                inputs,
                vals: Vec::new(),
                deps: Vec::new(),
                effects,
                payload: NodePayload::Prim(prim_cid),
            };
            node::store_node(&conn, &node)?
        }
        NodeCommand::Call {
            ty,
            word,
            inputs,
            effects,
        } => {
            let word_cid = cid::from_hex(&word)?;
            let inputs = parse_inputs(&inputs)?;
            let effects = parse_cid_list(effects.iter().map(|s| s.as_str()))?;
            let node = NodeCanon {
                kind: NodeKind::Call,
                out: vec![ty],
                inputs,
                vals: Vec::new(),
                deps: Vec::new(),
                effects,
                payload: NodePayload::Word(word_cid),
            };
            node::store_node(&conn, &node)?
        }
        NodeCommand::Arg { ty, index, effects } => {
            let effects = parse_cid_list(effects.iter().map(|s| s.as_str()))?;
            let node = NodeCanon {
                kind: NodeKind::Arg,
                out: vec![ty],
                inputs: Vec::new(),
                vals: Vec::new(),
                deps: Vec::new(),
                effects,
                payload: NodePayload::Arg(index),
            };
            node::store_node(&conn, &node)?
        }
        NodeCommand::LoadGlobal {
            ty,
            global,
            effects,
        } => {
            let global_cid = cid::from_hex(&global)?;
            let effects = parse_cid_list(effects.iter().map(|s| s.as_str()))?;
            let node = NodeCanon {
                kind: NodeKind::LoadGlobal,
                out: vec![ty],
                inputs: Vec::new(),
                vals: Vec::new(),
                deps: Vec::new(),
                effects,
                payload: NodePayload::Global(global_cid),
            };
            node::store_node(&conn, &node)?
        }
    };
    let cid_hex = cid::to_hex(&outcome.cid);
    if outcome.inserted {
        println!("stored node with cid {cid_hex}");
    } else {
        println!("node already present with cid {cid_hex}");
    }
    Ok(())
}

fn cmd_word(store: &Path, command: WordCommand) -> Result<()> {
    match command {
        WordCommand::Add {
            name,
            root,
            params,
            results,
            effects,
            no_register,
        } => {
            let conn = open_store(store)?;
            let root_cid = cid::from_hex(&root)?;
            let word = WordCanon {
                root: root_cid,
                params,
                results,
                effects: parse_cid_list(effects.iter().map(|s| s.as_str()))?,
                effect_mask: if effects.is_empty() {
                    effect_mask::NONE
                } else {
                    effect_mask::IO
                },
            };
            let outcome = word::store_word(&conn, &word)?;
            if !no_register {
                if let Some(name) = &name {
                    put_name(&conn, "word", name, &outcome.cid)?;
                }
            }
            let cid_hex = cid::to_hex(&outcome.cid);
            if outcome.inserted {
                println!("stored word with cid {cid_hex}");
            } else {
                println!("word already present with cid {cid_hex}");
            }
        }
        WordCommand::List { prefix } => {
            let conn = open_store(store)?;
            list_scope(&conn, "word", prefix.as_deref(), "no words registered")?;
        }
        WordCommand::Show { name } => {
            let conn = open_store(store)?;
            show_named_object(&conn, "word", "word", &name)?;
        }
    }
    Ok(())
}

fn cmd_builder(store: &Path) -> Result<()> {
    use std::io::{self, BufRead, Write};

    let conn = open_store(store)?;
    let mut builder = march5::GraphBuilder::new(&conn);
    let stdin = io::stdin();
    let mut input = String::new();
    let mut current_params: Option<Vec<TypeTag>> = None;

    println!(
        "March builder REPL. Commands: begin, lit, prim, call, dup, swap, over, stack, finish, reset, help, quit."
    );
    loop {
        print!("builder> ");
        io::stdout().flush().ok();
        input.clear();
        if stdin.lock().read_line(&mut input)? == 0 {
            break;
        }
        let line = input.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let cmd = parts.next().unwrap();
        let remaining: Vec<&str> = parts.collect();

        let result = match cmd {
            "help" => {
                println!(
                    "Commands:\n  begin [types...]        start a word with parameter types (e.g. begin i64 i64)\n  lit <i64>               push literal\n  prim <primCID>          apply primitive by CID\n  call <wordCID>          call existing word by CID\n  dup|swap|over           stack shuffles\n  stack                   show current stack depth\n  finish <result> [name]  finish word with result type and optional name\n  reset                   abandon current build\n  quit/exit               leave the REPL"
                );
                Ok(())
            }
            "quit" | "exit" => break,
            "begin" => {
                let tags =
                    parse_type_tags(&remaining.iter().map(|s| s.to_string()).collect::<Vec<_>>())?;
                builder.begin_word(&tags)?;
                current_params = Some(tags);
                println!(
                    "began word with {} parameter(s)",
                    current_params.as_ref().unwrap().len()
                );
                Ok(())
            }
            "reset" => {
                builder.begin_word(&[])?;
                current_params = Some(Vec::new());
                println!("state reset");
                Ok(())
            }
            "lit" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                if remaining.len() != 1 {
                    bail!("lit expects one argument");
                }
                let value: i64 = remaining[0].parse()?;
                builder.push_lit_i64(value)?;
                Ok(())
            }
            "prim" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                if remaining.len() != 1 {
                    bail!("prim expects CID argument");
                }
                let cid = cid::from_hex(remaining[0])?;
                builder.apply_prim(cid)?;
                Ok(())
            }
            "call" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                if remaining.len() != 1 {
                    bail!("call expects CID argument");
                }
                let cid = cid::from_hex(remaining[0])?;
                builder.apply_word(cid)?;
                Ok(())
            }
            "dup" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                builder.dup()
            }
            "swap" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                builder.swap()
            }
            "over" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                builder.over()
            }
            "stack" => {
                println!("stack depth: {}", builder.depth());
                Ok(())
            }
            "finish" => {
                ensure_builder_begun(&mut builder, &mut current_params)?;
                if remaining.is_empty() {
                    bail!("finish requires a result type, e.g., finish i64 [name]");
                }
                let result_tag = TypeTag::from_atom(remaining[0])?;
                let name = if remaining.len() > 1 {
                    Some(remaining[1].to_string())
                } else {
                    None
                };
                let params = current_params
                    .as_ref()
                    .ok_or_else(|| anyhow!("no word in progress; use begin first"))?;
                let cid = builder.finish_word(params, &[result_tag], name.as_deref())?;
                println!("stored word with cid {}", march5::cid::to_hex(&cid));
                current_params = None;
                Ok(())
            }
            _ => bail!("unknown command `{cmd}`; type `help`"),
        };

        if let Err(err) = result {
            eprintln!("error: {err}");
        }
    }

    Ok(())
}

fn ensure_builder_begun(
    builder: &mut march5::GraphBuilder<'_>,
    current_params: &mut Option<Vec<TypeTag>>,
) -> Result<()> {
    if current_params.is_none() {
        builder.begin_word(&[])?;
        *current_params = Some(Vec::new());
    }
    Ok(())
}

fn cmd_run(store: &Path, name: &str, args: &[i64]) -> Result<()> {
    let conn = open_store(store)?;
    let word_cid =
        get_name(&conn, "word", name)?.ok_or_else(|| anyhow!("word `{name}` not found"))?;
    let arg_values: Vec<Value> = args.iter().copied().map(Value::I64).collect();
    let outputs = run_word(&conn, &word_cid, &arg_values)?;
    match outputs.len() {
        0 => println!("()"),
        1 => println!("{}", outputs[0]),
        _ => {
            let body = outputs
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            println!("({body})");
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

/// Convert CLI type atoms into `TypeTag`s.
fn parse_type_tags(entries: &[String]) -> Result<Vec<TypeTag>> {
    entries.iter().map(|s| TypeTag::from_atom(s)).collect()
}

/// Parse export specs of the form `name=cid`.
fn parse_exports(entries: &[String]) -> Result<Vec<(String, [u8; 32])>> {
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some((name, cid_hex)) = entry.split_once('=') else {
            bail!("invalid export `{entry}`; expected name=wordCID");
        };
        let trimmed_name = name.trim();
        if trimmed_name.is_empty() {
            bail!("export name cannot be empty in `{entry}`");
        }
        let word_cid = cid::from_hex(cid_hex.trim())?;
        out.push((trimmed_name.to_string(), word_cid));
    }
    Ok(out)
}

/// Parse an iterator of hexadecimal CIDs into 32-byte arrays.
fn parse_cid_list<'a, I>(entries: I) -> Result<Vec<[u8; 32]>>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut cids = Vec::new();
    for entry in entries {
        cids.push(cid::from_hex(entry)?);
    }
    Ok(cids)
}

/// Parse `CID:PORT` strings into node inputs.
fn parse_inputs(entries: &[String]) -> Result<Vec<NodeInput>> {
    let mut inputs = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some((cid_hex, port_str)) = entry.split_once(':') else {
            bail!("invalid input `{entry}`; expected CID:PORT");
        };
        let cid = cid::from_hex(cid_hex)?;
        let port: u32 = port_str
            .parse()
            .map_err(|_| anyhow!("invalid port `{port_str}` in `{entry}`; expected integer"))?;
        inputs.push(NodeInput { cid, port });
    }
    Ok(inputs)
}

/// Dump the name_index rows for a given scope (optionally filtered by prefix).
fn list_scope(conn: &Connection, scope: &str, prefix: Option<&str>, empty_msg: &str) -> Result<()> {
    let sql_prefix =
        "SELECT name, cid FROM name_index WHERE scope = ?1 AND name LIKE ?2 ORDER BY name";
    let sql_all = "SELECT name, cid FROM name_index WHERE scope = ?1 ORDER BY name";

    let mut stmt = if prefix.is_some() {
        conn.prepare(sql_prefix)?
    } else {
        conn.prepare(sql_all)?
    };

    let mut rows = if let Some(prefix) = prefix {
        let pattern = format!("{prefix}%");
        stmt.query((scope, pattern))?
    } else {
        stmt.query([scope])?
    };

    let mut found = false;
    while let Some(row) = rows.next()? {
        let name: String = row.get(0)?;
        let cid_blob: Vec<u8> = row.get(1)?;
        let cid = cid::from_slice(&cid_blob)?;
        println!("{name} -> {}", cid::to_hex(&cid));
        found = true;
    }

    if !found {
        println!("{empty_msg}");
    }

    Ok(())
}

/// Parse a `--name` specification of the form `name(params) -> results | effects`.
fn parse_iface_spec(spec: &str) -> Result<IfaceSymbol> {
    let mut parts = spec.splitn(2, '|');
    let sig_part = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("invalid name spec `{spec}`"))?;
    let effects_part = parts.next().map(str::trim).unwrap_or("");

    let (name, params, results) = parse_signature(sig_part)?;

    let effects = if effects_part.is_empty() {
        Vec::new()
    } else {
        let effect_tokens: Vec<&str> = effects_part
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        parse_cid_list(effect_tokens.iter().copied())?
    };

    Ok(IfaceSymbol {
        name,
        params,
        results,
        effects,
    })
}

/// Split `name(params) -> results` into components.
fn parse_signature(spec: &str) -> Result<(String, Vec<String>, Vec<String>)> {
    let spec = spec.trim();
    let open_paren = spec
        .find('(')
        .ok_or_else(|| anyhow!("missing '(' in `{spec}`"))?;
    let name = spec[..open_paren].trim();
    if name.is_empty() {
        bail!("export name cannot be empty in `{spec}`");
    }

    let remainder = &spec[open_paren + 1..];
    let close_paren = remainder
        .find(')')
        .ok_or_else(|| anyhow!("missing ')' in `{spec}`"))?;
    let params_part = &remainder[..close_paren];
    let after_paren = remainder[close_paren + 1..].trim();
    let arrow = after_paren
        .strip_prefix("->")
        .ok_or_else(|| anyhow!("missing '->' in `{spec}`"))?;
    let results_part = arrow.trim();

    let params = parse_type_list(params_part)?;
    let results = parse_type_list(results_part)?;

    Ok((name.to_string(), params, results))
}

/// Turn a comma-separated list (optionally wrapped in parentheses) into type strings.
fn parse_type_list(spec: &str) -> Result<Vec<String>> {
    let mut s = spec.trim();
    if s.is_empty() {
        return Ok(Vec::new());
    }

    if s.starts_with('(') {
        if !s.ends_with(')') {
            bail!("unmatched parentheses in type list `{spec}`");
        }
        s = &s[1..s.len() - 1];
    }

    let mut types = Vec::new();
    for part in s.split(',') {
        let ty = part.trim();
        if ty.is_empty() {
            continue;
        }
        types.push(ty.to_string());
    }
    Ok(types)
}

fn cbor_to_pretty_json(bytes: &[u8]) -> Result<String> {
    let mut deserializer = serde_cbor::Deserializer::from_slice(bytes);
    let value = serde_cbor::Value::deserialize(&mut deserializer)?;
    let json = serde_json::to_string_pretty(&value)?;
    Ok(json)
}

fn show_named_object(conn: &Connection, scope: &str, label: &str, name: &str) -> Result<()> {
    let cid = get_name(conn, scope, name)?.ok_or_else(|| anyhow!("{label} `{name}` not found"))?;
    let (_kind, cbor) = load_object_cbor(conn, &cid)?;
    let json = cbor_to_pretty_json(&cbor)?;
    println!("{json}");
    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    use serde_cbor::Value;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn parse_exports_pairs() {
        let cid = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let exports = parse_exports(&vec![format!("sum={cid}")]).unwrap();
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].0, "sum");
        assert_eq!(march5::cid::to_hex(&exports[0].1), cid);
    }

    #[test]
    fn parse_exports_rejects_empty_name() {
        let cid = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let err = parse_exports(&vec![format!(" ={cid}")]).unwrap_err();
        assert!(err.to_string().contains("export name cannot be empty"));
    }

    #[test]
    fn cbor_json_round_trip() {
        let value = json!({"kind": "test", "value": 42});
        let bytes = serde_cbor::to_vec(&value).unwrap();
        let json = cbor_to_pretty_json(&bytes).unwrap();
        assert!(json.contains("\"kind\": \"test\""));
        assert!(json.contains("42"));
    }

    #[test]
    fn cmd_effect_add_persists_object() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("cli-effects.march5.db");
        let _ = create_store(&db_path)?;

        cmd_effect(
            &db_path,
            EffectCommand::Add {
                name: "io".to_string(),
                doc: Some("performs input/output".to_string()),
            },
        )?;

        let conn = open_store(&db_path)?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM object WHERE kind = 'effect'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    fn cmd_prim_add_registers_name() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("cli-prims.march5.db");
        let _ = create_store(&db_path)?;

        cmd_prim(
            &db_path,
            PrimCommand::Add {
                name: "demo/add".to_string(),
                params: vec!["i64".to_string(), "i64".to_string()],
                results: vec!["i64".to_string()],
                effects: vec![],
                no_register: false,
            },
        )?;

        let conn = open_store(&db_path)?;
        let cid = get_name(&conn, "prim", "demo/add")?.expect("name registered");
        let (kind, cbor) = load_object_cbor(&conn, &cid)?;
        assert_eq!(kind, "prim");

        let value = serde_cbor::from_slice::<Value>(&cbor)?;
        let array = match value {
            Value::Array(items) => items,
            _ => panic!("primitive CBOR must be array"),
        };
        assert_eq!(array.len(), 5);
        assert_eq!(array[0], Value::Integer(0));
        Ok(())
    }

    #[test]
    fn cmd_iface_add_registers_name() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("cli-iface.march5.db");
        let _ = create_store(&db_path)?;

        cmd_iface(
            &db_path,
            IfaceCommand::Add {
                register: Some("demo.iface/math".to_string()),
                names: vec!["hello() -> unit".to_string()],
                no_register: false,
            },
        )?;

        let conn = open_store(&db_path)?;
        let cid = get_name(&conn, "iface", "demo.iface/math")?.expect("name registered");
        let (_, cbor) = load_object_cbor(&conn, &cid)?;

        let hello_found = cbor.windows(b"hello".len()).any(|w| w == b"hello");
        assert!(hello_found);
        Ok(())
    }
}
