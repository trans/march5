use std::path::Path;

use anyhow::{Result, anyhow, bail};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json;

use march5::db;
use march5::node::NodeInput;
use march5::types::{EffectMask, effect_mask};
use march5::{TypeTag, Value, cid, get_name, load_object_cbor};

pub(crate) fn require_store_path(path: Option<&Path>) -> Result<&Path> {
    match path {
        Some(p) => Ok(p),
        None => bail!("specify --db PATH for this command"),
    }
}

pub(crate) fn parse_type_tags(entries: &[String]) -> Result<Vec<TypeTag>> {
    entries.iter().map(|s| TypeTag::from_atom(s)).collect()
}

pub(crate) fn parse_exports(entries: &[String]) -> Result<Vec<(String, [u8; 32])>> {
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

pub(crate) fn parse_cid_list<'a, I>(entries: I) -> Result<Vec<[u8; 32]>>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut cids = Vec::new();
    for entry in entries {
        cids.push(cid::from_hex(entry)?);
    }
    Ok(cids)
}

pub(crate) fn parse_inputs(entries: &[String]) -> Result<Vec<NodeInput>> {
    let mut inputs = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some((cid_hex, port_str)) = entry.split_once(':') else {
            bail!("invalid input `{entry}`; expected CID:PORT");
        };
        let port: u32 = port_str.parse()?;
        let cid_bytes = cid::from_hex(cid_hex)?;
        inputs.push(NodeInput {
            cid: cid_bytes,
            port,
        });
    }
    Ok(inputs)
}

pub(crate) fn parse_effect_mask_flags(entries: &[String]) -> Result<EffectMask> {
    let mut mask = effect_mask::NONE;
    for entry in entries {
        let flag = entry.trim().to_ascii_lowercase();
        if flag.is_empty() {
            continue;
        }
        match flag.as_str() {
            "io" => mask |= effect_mask::IO,
            "state" => mask |= effect_mask::STATE_READ | effect_mask::STATE_WRITE,
            "state.read" | "state_read" | "state-read" => mask |= effect_mask::STATE_READ,
            "state.write" | "state_write" | "state-write" => mask |= effect_mask::STATE_WRITE,
            "test" => mask |= effect_mask::TEST,
            "metric" => mask |= effect_mask::METRIC,
            other => bail!("unknown effect mask domain `{other}`"),
        }
    }
    Ok(mask)
}

pub(crate) fn parse_cli_value(token: &str) -> Result<Value> {
    if token == "~" || token.eq_ignore_ascii_case("null") {
        return Ok(Value::Unit);
    }
    if let Ok(i) = token.parse::<i64>() {
        return Ok(Value::I64(i));
    }
    if let Ok(f) = token.parse::<f64>() {
        return Ok(Value::F64(f));
    }
    Ok(Value::Text(token.to_string()))
}

pub(crate) fn list_scope(
    conn: &Connection,
    scope: &str,
    prefix: Option<&str>,
    empty_msg: &str,
) -> Result<()> {
    let entries = db::list_names(conn, scope, prefix)?;
    if entries.is_empty() {
        println!("{empty_msg}");
        return Ok(());
    }

    for entry in entries {
        println!("{} -> {}", entry.name, cid::to_hex(&entry.cid));
    }

    Ok(())
}

pub(crate) fn parse_iface_spec(spec: &str) -> Result<march5::iface::IfaceSymbol> {
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

    Ok(march5::iface::IfaceSymbol {
        name,
        params,
        results,
        effects,
    })
}

pub(crate) fn parse_signature(spec: &str) -> Result<(String, Vec<String>, Vec<String>)> {
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

pub(crate) fn parse_type_list(spec: &str) -> Result<Vec<String>> {
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

pub(crate) fn cbor_to_pretty_json(bytes: &[u8]) -> Result<String> {
    let mut deserializer = serde_cbor::Deserializer::from_slice(bytes);
    let value = serde_cbor::Value::deserialize(&mut deserializer)?;
    let json = serde_json::to_string_pretty(&value)?;
    Ok(json)
}

pub(crate) fn show_named_object(
    conn: &Connection,
    scope: &str,
    label: &str,
    name: &str,
) -> Result<()> {
    let cid = get_name(conn, scope, name)?.ok_or_else(|| anyhow!("{label} `{name}` not found"))?;
    let (_kind, cbor) = load_object_cbor(conn, &cid)?;
    let json = cbor_to_pretty_json(&cbor)?;
    println!("{json}");
    Ok(())
}

pub(crate) fn lookup_named_cid(conn: &Connection, scope: &str, name: &str) -> Result<[u8; 32]> {
    if let Some(cid) = get_name(conn, scope, name)? {
        return Ok(cid);
    }
    if name.len() == 64 && name.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(cid::from_hex(name)?);
    }
    bail!("{scope} `{name}` not found in name index")
}
