use std::path::Path;

use anyhow::{Result, anyhow, bail};

mod builder;
mod catalog;
mod effect;
mod guard;
mod iface;
mod namespace;
mod new;
mod node;
mod prim;
mod state;
mod util;
mod word;

pub(crate) use builder::cmd_builder;
pub(crate) use catalog::cmd_catalog;
pub(crate) use effect::cmd_effect;
pub(crate) use guard::cmd_guard;
pub(crate) use iface::cmd_iface;
pub(crate) use namespace::cmd_namespace;
pub(crate) use new::cmd_new;
pub(crate) use node::cmd_node;
pub(crate) use prim::cmd_prim;
pub(crate) use state::cmd_state;
pub(crate) use word::cmd_word;

pub(crate) use util::{list_scope, parse_cli_value, require_store_path, show_named_object};

use march5::inet;
use march5::yaml;
use march5::{cid, get_name, open_store, put_name, run_word};

pub(crate) fn cmd_run(
    store: &Path,
    name: &str,
    args: &[String],
    args_yaml: Option<&Path>,
) -> Result<()> {
    let conn = open_store(store)?;
    let word_cid =
        get_name(&conn, "word", name)?.ok_or_else(|| anyhow!("word `{name}` not found"))?;
    let arg_values = if let Some(path) = args_yaml {
        yaml::parse_values_from_file(path)?
    } else {
        args.iter()
            .map(|s| parse_cli_value(s))
            .collect::<Result<Vec<_>>>()?
    };
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

pub(crate) fn cmd_agent(store: &Path, command: super::AgentCommand) -> Result<()> {
    match command {
        super::AgentCommand::Add {
            name,
            kind,
            ports,
            doc,
        } => {
            if ports.is_empty() {
                bail!("specify at least the principal port via --port");
            }
            let conn = open_store(store)?;
            let ports_vec: Vec<&str> = ports.iter().map(|s| s.as_str()).collect();
            let agent = inet::AgentCanon {
                name: &kind,
                ports: &ports_vec,
                doc: doc.as_deref(),
            };
            let out = inet::store_agent(&conn, &agent)?;
            if let Some(n) = name {
                put_name(&conn, "agent", &n, &out.cid)?;
            }
            println!("stored agent `{kind}` with cid {}", cid::to_hex(&out.cid));
        }
        super::AgentCommand::List { prefix } => {
            let conn = open_store(store)?;
            list_scope(&conn, "agent", prefix.as_deref(), "no agents registered")?;
        }
        super::AgentCommand::Show { name } => {
            let conn = open_store(store)?;
            show_named_object(&conn, "agent", "agent", &name)?;
        }
    }
    Ok(())
}

pub(crate) fn cmd_rule(store: &Path, command: super::RuleCommand) -> Result<()> {
    match command {
        super::RuleCommand::Add {
            name,
            lhs_a,
            lhs_b,
            rewire,
        } => {
            let conn = open_store(store)?;
            let rule = inet::RuleCanon {
                lhs_a: &lhs_a,
                lhs_b: &lhs_b,
                body_syntax: &rewire,
            };
            let out = inet::store_rule(&conn, &rule)?;
            if let Some(n) = name {
                put_name(&conn, "rule", &n, &out.cid)?;
            }
            println!(
                "stored rule `({lhs_a} {lhs_b})` with cid {}",
                cid::to_hex(&out.cid)
            );
        }
        super::RuleCommand::List { prefix } => {
            let conn = open_store(store)?;
            list_scope(&conn, "rule", prefix.as_deref(), "no rules registered")?;
        }
        super::RuleCommand::Show { name } => {
            let conn = open_store(store)?;
            show_named_object(&conn, "rule", "rule", &name)?;
        }
    }
    Ok(())
}
