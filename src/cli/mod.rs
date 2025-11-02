mod commands;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

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
    /// Inspect and manage the in-memory global state store
    State {
        #[command(subcommand)]
        command: StateCommand,
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
    /// Manage guard quotations
    Guard {
        #[command(subcommand)]
        command: GuardCommand,
    },
    /// Interactive graph builder REPL
    Builder,
    /// Execute a word and print its result
    Run {
        name: String,
        /// Supply repeated --arg <literal> values for parameters
        #[arg(long = "arg")]
        args: Vec<String>,
        /// Provide arguments via YAML sequence (tags like !i64, !text, !tuple)
        #[arg(long = "args-yaml", value_name = "PATH")]
        args_yaml: Option<PathBuf>,
    },
    /// Apply a YAML catalog of effects/prims/words/snapshots
    Catalog {
        file: PathBuf,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Manage inet agents (ports-based node kinds)
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Manage inet rewrite rules
    Rule {
        #[command(subcommand)]
        command: RuleCommand,
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
        /// Explicit effect mask domains (e.g. io, state.write, test)
        #[arg(long = "emask", value_name = "DOMAIN")]
        emask: Vec<String>,
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
enum StateCommand {
    /// Print the current in-memory snapshot of the global store
    Snapshot,
    /// Clear the in-memory global store
    Reset,
    /// Persist the current global store snapshot
    Save {
        /// Optional name to register in the global store namespace
        #[arg(long = "name", value_name = "NAME")]
        name: Option<String>,
    },
    /// Load a persisted snapshot into the in-memory global store
    Load {
        /// Snapshot name registered in the global store namespace
        name: String,
    },
    /// List saved global store snapshots
    List {
        #[arg(long = "prefix")]
        prefix: Option<String>,
    },
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
enum AgentCommand {
    /// Insert an inet agent kind (principal port is index 0)
    Add {
        /// Register name in name_index (e.g., core/pair)
        #[arg(long = "name")]
        name: Option<String>,
        /// Human-readable agent kind name
        #[arg(long = "kind")]
        kind: String,
        /// Port names in declaration order (first is principal)
        #[arg(long = "port")]
        ports: Vec<String>,
        /// Optional documentation
        #[arg(long = "doc")]
        doc: Option<String>,
    },
    /// Show an agent's canonical JSON by name
    Show { name: String },
    /// List registered agents (optionally filtered by prefix)
    List {
        #[arg(long = "prefix")]
        prefix: Option<String>,
    },
}

#[derive(Subcommand)]
enum RuleCommand {
    /// Insert a rewrite rule (LHS pair -> rewiring description)
    Add {
        /// Register name in name_index (e.g., core/dispatch-apply)
        #[arg(long = "name")]
        name: Option<String>,
        /// Left-hand agent kind name (self)
        #[arg(long = "lhs-a")]
        lhs_a: String,
        /// Right-hand agent kind name (other)
        #[arg(long = "lhs-b")]
        lhs_b: String,
        /// Rewiring description (opaque syntax, e.g., S-expr)
        #[arg(long = "rewire")]
        rewire: String,
    },
    /// Show a rule's canonical JSON by name
    Show { name: String },
    /// List registered rules (optionally filtered by prefix)
    List {
        #[arg(long = "prefix")]
        prefix: Option<String>,
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
        /// Explicit effect mask domains (e.g. io, state.write, test)
        #[arg(long = "emask", value_name = "DOMAIN")]
        emask: Vec<String>,
        /// Attach guard CIDs or names (repeatable)
        #[arg(long = "guard", value_name = "CID_OR_NAME")]
        guards: Vec<String>,
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

#[derive(Subcommand)]
enum GuardCommand {
    /// Insert or update a guard descriptor
    Add {
        /// Optional human/name-index entry (e.g. namespace/guards/cond)
        #[arg(long = "name")]
        name: Option<String>,
        /// Root node CID for the guard quotation (expects RETURN root)
        #[arg(long = "root")]
        root: String,
        /// Repeat per argument type (left-to-right)
        #[arg(long = "param", value_name = "TYPE")]
        params: Vec<String>,
        /// Guard results (must be exactly one i64 for the interpreter)
        #[arg(long = "result", value_name = "TYPE")]
        results: Vec<String>,
        /// Skip name_index registration
        #[arg(long = "no-register")]
        no_register: bool,
    },
    /// Show a guard's canonical JSON by name
    Show { name: String },
    /// List registered guards (optionally filtered by prefix)
    List {
        #[arg(long = "prefix")]
        prefix: Option<String>,
    },
}

pub(crate) fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::New { name } => commands::cmd_new(&name),
        Command::Effect { command } => {
            let store_path = commands::require_store_path(cli.store.as_deref())?;
            commands::cmd_effect(store_path, command)
        }
        Command::Prim { command } => {
            let store_path = commands::require_store_path(cli.store.as_deref())?;
            commands::cmd_prim(store_path, command)
        }
        Command::Iface { command } => {
            let store_path = commands::require_store_path(cli.store.as_deref())?;
            commands::cmd_iface(store_path, command)
        }
        Command::Namespace { command } => {
            let store_path = commands::require_store_path(cli.store.as_deref())?;
            commands::cmd_namespace(store_path, command)
        }
        Command::Node { command } => {
            let store_path = commands::require_store_path(cli.store.as_deref())?;
            commands::cmd_node(store_path, command)
        }
        Command::Word { command } => {
            let store_path = commands::require_store_path(cli.store.as_deref())?;
            commands::cmd_word(store_path, command)
        }
        Command::Guard { command } => {
            let store_path = commands::require_store_path(cli.store.as_deref())?;
            commands::cmd_guard(store_path, command)
        }
        Command::Builder => {
            let store_path = commands::require_store_path(cli.store.as_deref())?;
            commands::cmd_builder(store_path)
        }
        Command::State { command } => commands::cmd_state(cli.store.as_deref(), command),
        Command::Run {
            name,
            args,
            args_yaml,
        } => {
            let store_path = commands::require_store_path(cli.store.as_deref())?;
            commands::cmd_run(store_path, &name, &args, args_yaml.as_deref())
        }
        Command::Catalog { file, dry_run } => {
            commands::cmd_catalog(cli.store.as_deref(), &file, dry_run)
        }
        Command::Agent { command } => {
            let store_path = commands::require_store_path(cli.store.as_deref())?;
            commands::cmd_agent(store_path, command)
        }
        Command::Rule { command } => {
            let store_path = commands::require_store_path(cli.store.as_deref())?;
            commands::cmd_rule(store_path, command)
        }
    }
}
