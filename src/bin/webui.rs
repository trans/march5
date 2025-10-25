use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use clap::Parser;
use march5::prim::load_prim_info;
use march5::word::load_word_info;
use march5::{
    TypeTag, cid, create_store, derive_db_path, get_name, list_names_for_cid, load_object_cbor,
    open_store,
};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_bytes::ByteBuf;
use serde_cbor::Value as CborValue;
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use std::fmt::Write as FmtWrite;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

#[derive(Parser, Debug)]
#[command(name = "march5-webui", about = "Lightweight March α₅ web UI server")]
struct Args {
    /// Address to listen on
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: String,

    /// Path to the March database
    #[arg(long = "db", default_value = "march5.db")]
    db_path: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut db_path = args.db_path;
    if !db_path.exists() {
        let inferred = if db_path.extension().is_some() {
            db_path.clone()
        } else {
            derive_db_path(&db_path.to_string_lossy())
        };
        create_store(&inferred)?;
        db_path = inferred;
    }

    let server = Server::http(&args.listen)
        .map_err(|err| anyhow!("failed to bind {}: {err}", args.listen))?;
    println!("March web UI listening on http://{}", args.listen);
    let db_path = Arc::new(db_path);

    for request in server.incoming_requests() {
        let db_path = Arc::clone(&db_path);
        if let Err(err) = handle_request(&db_path, request) {
            eprintln!("error handling request: {err}");
        }
    }
    Ok(())
}

fn handle_request(db_path: &Path, request: Request) -> Result<()> {
    if *request.method() != Method::Get {
        let response = Response::from_string("Only GET supported")
            .with_status_code(StatusCode(405))
            .with_header(content_type("text/plain"));
        request.respond(response)?;
        return Ok(());
    }

    let url = request.url();
    let (path, query) = split_query(url);
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let response = match segments.as_slice() {
        [] | [""] => match open_store(db_path) {
            Ok(conn) => match build_index_html(&conn) {
                Ok(html) => html_response(html),
                Err(err) => error_response(500, err),
            },
            Err(err) => error_response(500, err),
        },
        ["api", "iface", rest @ ..] if !rest.is_empty() => {
            let name = rest.join("/");
            match fetch_named_json(db_path, "iface", "interface", &name) {
                Ok(json) => json_response(json),
                Err(err) => error_response(404, err),
            }
        }
        ["api", "namespace", rest @ ..] if !rest.is_empty() => {
            let name = rest.join("/");
            match fetch_named_json(db_path, "namespace", "namespace", &name) {
                Ok(json) => json_response(json),
                Err(err) => error_response(404, err),
            }
        }
        ["api", "word", rest @ ..] if !rest.is_empty() => {
            let name = rest.join("/");
            match fetch_named_json(db_path, "word", "word", &name) {
                Ok(json) => json_response(json),
                Err(err) => error_response(404, err),
            }
        }
        ["api", "list", scope] => {
            let prefix = query.and_then(|q| parse_prefix(q));
            match list_scope_entries(db_path, scope, prefix.as_deref()) {
                Ok(entries) => json_response(entries),
                Err(err) => error_response(500, err),
            }
        }
        _ => error_response(404, anyhow!("unrecognised path")),
    };

    request.respond(response)?;
    Ok(())
}

fn fetch_named_json(db_path: &Path, scope: &str, label: &str, name: &str) -> Result<String> {
    let conn = open_store(db_path)?;
    let cid_bytes =
        get_name(&conn, scope, name)?.ok_or_else(|| anyhow!("{label} `{name}` not found"))?;
    let (kind, cbor) = load_object_cbor(&conn, &cid_bytes)?;
    let mut value = cbor_to_json(&cbor)?;
    if let JsonValue::Object(ref mut map) = value {
        map.insert("_cid".into(), JsonValue::String(cid::to_hex(&cid_bytes)));
        map.insert("_kind".into(), JsonValue::String(kind));
    }
    Ok(serde_json::to_string_pretty(&value)?)
}

fn list_scope_entries(db_path: &Path, scope: &str, prefix: Option<&str>) -> Result<String> {
    let conn = open_store(db_path)?;
    let pattern = prefix
        .map(|p| format!("{}%", p))
        .unwrap_or_else(|| "%".to_string());
    let mut stmt = conn.prepare(
        "SELECT name, cid FROM name_index WHERE scope = ?1 AND name LIKE ?2 ORDER BY name",
    )?;
    let mut rows = stmt.query(params![scope, pattern])?;
    let mut entries = Vec::new();
    while let Some(row) = rows.next()? {
        let name: String = row.get(0)?;
        let blob: Vec<u8> = row.get(1)?;
        let cid_bytes = cid::from_slice(&blob)?;
        entries.push(json!({ "name": name, "cid": cid::to_hex(&cid_bytes) }));
    }
    Ok(serde_json::to_string_pretty(&entries)?)
}

fn build_index_html(conn: &Connection) -> Result<String> {
    let namespaces = collect_namespace_rows(conn)?;
    let interfaces = collect_interface_rows(conn)?;
    let words = collect_word_rows(conn)?;
    let prims = collect_prim_rows(conn)?;

    let mut html = String::new();
    html.push_str(
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\" /><title>March α₅ Web UI</title>",
    );
    html.push_str(
        "<style>body{font-family:sans-serif;margin:2rem;}table.grid{border-collapse:collapse;margin-bottom:1.5rem;}table.grid th,table.grid td{border:1px solid #ccc;padding:0.35rem 0.6rem;text-align:left;}section{margin-bottom:2rem;}h2{margin-top:1.5rem;}code{background:#f4f4f4;padding:0.15rem 0.35rem;border-radius:4px;}</style>",
    );
    html.push_str("</head><body><h1>March α₅ Web UI</h1>");

    html.push_str(render_namespace_section(&namespaces).as_str());
    html.push_str(render_interface_section(&interfaces).as_str());
    html.push_str(render_word_section(&words).as_str());
    html.push_str(render_prim_section(&prims).as_str());

    html.push_str("</body></html>");
    Ok(html)
}

fn render_namespace_section(rows: &[NamespaceRow]) -> String {
    let mut out = String::new();
    out.push_str("<section><h2>Namespaces</h2>");
    if rows.is_empty() {
        out.push_str("<p>No namespaces registered.</p></section>");
        return out;
    }
    out.push_str("<table class=\"grid\"><thead><tr><th>Name</th><th>CID</th><th>Interface</th><th>Exports</th><th>Imports</th></tr></thead><tbody>");
    for row in rows {
        let _ = write!(
            out,
            "<tr><td><a href=\"{}\">{}</a></td><td><code>{}</code></td><td><code>{}</code></td><td>{}</td><td>{}</td></tr>",
            make_api_href("namespace", &row.name),
            escape_html(&row.name),
            escape_html(&row.cid_hex),
            escape_html(&row.iface_hex),
            render_namespace_exports(&row.exports),
            render_list(&row.imports)
        );
    }
    out.push_str("</tbody></table></section>");
    out
}

fn render_interface_section(rows: &[InterfaceRow]) -> String {
    let mut out = String::new();
    out.push_str("<section><h2>Interfaces</h2>");
    if rows.is_empty() {
        out.push_str("<p>No interfaces registered.</p></section>");
        return out;
    }
    out.push_str("<table class=\"grid\"><thead><tr><th>Name</th><th>CID</th><th>Symbols</th></tr></thead><tbody>");
    for row in rows {
        let _ = write!(
            out,
            "<tr><td><a href=\"{}\">{}</a></td><td><code>{}</code></td><td>{}</td></tr>",
            make_api_href("iface", &row.name),
            escape_html(&row.name),
            escape_html(&row.cid_hex),
            render_list(&row.symbol_summaries)
        );
    }
    out.push_str("</tbody></table></section>");
    out
}

fn render_word_section(rows: &[WordRow]) -> String {
    let mut out = String::new();
    out.push_str("<section><h2>Words</h2>");
    if rows.is_empty() {
        out.push_str("<p>No words registered.</p></section>");
        return out;
    }
    out.push_str("<table class=\"grid\"><thead><tr><th>Name</th><th>CID</th><th>Signature</th><th>Effects</th></tr></thead><tbody>");
    for row in rows {
        let _ = write!(
            out,
            "<tr><td><a href=\"{}\">{}</a></td><td><code>{}</code></td><td>{}</td><td>{}</td></tr>",
            make_api_href("word", &row.name),
            escape_html(&row.name),
            escape_html(&row.cid_hex),
            escape_html(&row.signature),
            escape_html(&render_effects(&row.effects))
        );
    }
    out.push_str("</tbody></table></section>");
    out
}

fn render_prim_section(rows: &[PrimRow]) -> String {
    let mut out = String::new();
    out.push_str("<section><h2>Primitives</h2>");
    if rows.is_empty() {
        out.push_str("<p>No primitives registered.</p></section>");
        return out;
    }
    out.push_str("<table class=\"grid\"><thead><tr><th>Name</th><th>CID</th><th>Signature</th><th>Effects</th></tr></thead><tbody>");
    for row in rows {
        let _ = write!(
            out,
            "<tr><td><a href=\"{}\">{}</a></td><td><code>{}</code></td><td>{}</td><td>{}</td></tr>",
            make_api_href("prim", &row.name),
            escape_html(&row.name),
            escape_html(&row.cid_hex),
            escape_html(&row.signature),
            escape_html(&render_effects(&row.effects))
        );
    }
    out.push_str("</tbody></table></section>");
    out
}

#[derive(Debug)]
struct NamespaceRow {
    name: String,
    cid_hex: String,
    iface_hex: String,
    exports: Vec<NsExport>,
    imports: Vec<String>,
}

#[derive(Debug)]
struct NsExport {
    alias: String,
    word_cid_hex: String,
    word_name: Option<String>,
}

#[derive(Debug)]
struct InterfaceRow {
    name: String,
    cid_hex: String,
    symbol_summaries: Vec<String>,
}

#[derive(Debug)]
struct WordRow {
    name: String,
    cid_hex: String,
    signature: String,
    effects: Vec<String>,
}

#[derive(Debug)]
struct PrimRow {
    name: String,
    cid_hex: String,
    signature: String,
    effects: Vec<String>,
}

const SEGMENT_ENCODE: &AsciiSet = &CONTROLS
    .add(b' ') // space
    .add(b'"')
    .add(b'\'')
    .add(b'`')
    .add(b'<')
    .add(b'>')
    .add(b'#')
    .add(b'?')
    .add(b'{')
    .add(b'}');

fn collect_namespace_rows(conn: &Connection) -> Result<Vec<NamespaceRow>> {
    let mut stmt =
        conn.prepare("SELECT name, cid FROM name_index WHERE scope = 'namespace' ORDER BY name")?;
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(0)?;
        let blob: Vec<u8> = row.get(1)?;
        Ok((name, blob))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (name, cid_blob) = row?;
        let cid_bytes = cid::from_slice(&cid_blob)?;
        let (_, cbor) = load_object_cbor(conn, &cid_bytes)?;
        let record: NamespaceRecord = serde_cbor::from_slice(&cbor)?;
        let iface_hex = cid::to_hex(&bytebuf_to_array(&record.iface)?);
        let exports = record
            .exports
            .into_iter()
            .map(|e| {
                let word_arr = bytebuf_to_array(&e.word)?;
                let word_cid_hex = cid::to_hex(&word_arr);
                let word_name = list_names_for_cid(conn, "word", &word_arr)?
                    .into_iter()
                    .next();
                Ok(NsExport {
                    alias: e.name,
                    word_cid_hex,
                    word_name,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let imports = record
            .imports
            .into_iter()
            .map(|buf| Ok(cid::to_hex(&bytebuf_to_array(&buf)?)))
            .collect::<Result<Vec<_>>>()?;
        out.push(NamespaceRow {
            name,
            cid_hex: cid::to_hex(&cid_bytes),
            iface_hex,
            exports,
            imports,
        });
    }
    Ok(out)
}

fn collect_interface_rows(conn: &Connection) -> Result<Vec<InterfaceRow>> {
    let mut stmt =
        conn.prepare("SELECT name, cid FROM name_index WHERE scope = 'iface' ORDER BY name")?;
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(0)?;
        let blob: Vec<u8> = row.get(1)?;
        Ok((name, blob))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (name, cid_blob) = row?;
        let cid_bytes = cid::from_slice(&cid_blob)?;
        let (_, cbor) = load_object_cbor(conn, &cid_bytes)?;
        let record: InterfaceRecord = serde_cbor::from_slice(&cbor)?;
        let mut summaries = Vec::new();
        for sym in record.names {
            let params = sym.ty.params.join(", ");
            let results = sym.ty.results.join(", ");
            let effects = sym
                .effects
                .into_iter()
                .map(|buf| Ok(cid::to_hex(&bytebuf_to_array(&buf)?)))
                .collect::<Result<Vec<_>>>()?;
            let effect_str = render_effects(&effects);
            let summary = if effect_str == "pure" {
                format!("{}({}) → ({})", sym.name, params, results)
            } else {
                format!("{}({}) → ({}) [{}]", sym.name, params, results, effect_str)
            };
            summaries.push(summary);
        }
        out.push(InterfaceRow {
            name,
            cid_hex: cid::to_hex(&cid_bytes),
            symbol_summaries: summaries,
        });
    }
    Ok(out)
}

fn collect_word_rows(conn: &Connection) -> Result<Vec<WordRow>> {
    let mut stmt =
        conn.prepare("SELECT name, cid FROM name_index WHERE scope = 'word' ORDER BY name")?;
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(0)?;
        let blob: Vec<u8> = row.get(1)?;
        Ok((name, blob))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (name, cid_blob) = row?;
        let cid_bytes = cid::from_slice(&cid_blob)?;
        let info = load_word_info(conn, &cid_bytes)?;
        let signature = format_signature(&info.params, &info.results);
        let effects = info
            .effects
            .into_iter()
            .map(|cid| cid::to_hex(&cid))
            .collect();
        out.push(WordRow {
            name,
            cid_hex: cid::to_hex(&cid_bytes),
            signature,
            effects,
        });
    }
    Ok(out)
}

fn collect_prim_rows(conn: &Connection) -> Result<Vec<PrimRow>> {
    let mut stmt =
        conn.prepare("SELECT name, cid FROM name_index WHERE scope = 'prim' ORDER BY name")?;
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(0)?;
        let blob: Vec<u8> = row.get(1)?;
        Ok((name, blob))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (name, cid_blob) = row?;
        let cid_bytes = cid::from_slice(&cid_blob)?;
        let info = load_prim_info(conn, &cid_bytes)?;
        let signature = format_signature(&info.params, &info.results);
        let effects = info
            .effects
            .into_iter()
            .map(|cid| cid::to_hex(&cid))
            .collect();
        out.push(PrimRow {
            name,
            cid_hex: cid::to_hex(&cid_bytes),
            signature,
            effects,
        });
    }
    Ok(out)
}

fn render_namespace_exports(exports: &[NsExport]) -> String {
    if exports.is_empty() {
        return "<em>none</em>".to_string();
    }
    let mut out = String::new();
    for export in exports {
        let target_display = if let Some(fqn) = &export.word_name {
            format!(
                "<a href=\"{}\">{}</a>",
                make_api_href("word", fqn),
                escape_html(fqn)
            )
        } else {
            format!("<code>{}</code>", escape_html(&export.word_cid_hex))
        };
        let _ = write!(
            out,
            "<div>{} → {}</div>",
            escape_html(&export.alias),
            target_display
        );
    }
    out
}

fn render_list(items: &[String]) -> String {
    if items.is_empty() {
        return "<em>none</em>".to_string();
    }
    let mut out = String::new();
    for item in items {
        let _ = write!(out, "<div>{}</div>", escape_html(item));
    }
    out
}

fn render_effects(effects: &[String]) -> String {
    if effects.is_empty() {
        "pure".to_string()
    } else {
        effects.join(", ")
    }
}

fn format_signature(params: &[TypeTag], results: &[TypeTag]) -> String {
    let param_list = params
        .iter()
        .map(|t| t.as_atom())
        .collect::<Vec<_>>()
        .join(", ");
    let result_list = results
        .iter()
        .map(|t| t.as_atom())
        .collect::<Vec<_>>()
        .join(", ");
    format!("({}) → ({})", param_list, result_list)
}

fn escape_html(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '&' => "&amp;".into(),
            '"' => "&quot;".into(),
            '\'' => "&#39;".into(),
            _ => c.to_string(),
        })
        .collect()
}

fn make_api_href(scope: &str, name: &str) -> String {
    let encoded = name
        .split('/')
        .map(|segment| utf8_percent_encode(segment, SEGMENT_ENCODE).to_string())
        .collect::<Vec<_>>()
        .join("/");
    format!("/api/{}/{}", scope, encoded)
}

fn bytebuf_to_array(buf: &ByteBuf) -> Result<[u8; 32]> {
    let slice = buf.as_slice();
    if slice.len() != 32 {
        bail!("expected 32-byte CID, found {} bytes", slice.len());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(slice);
    Ok(out)
}

fn cbor_to_json(bytes: &[u8]) -> Result<JsonValue> {
    let value: CborValue = serde_cbor::from_slice(bytes)?;
    Ok(cbor_value_to_json(value))
}

fn cbor_value_to_json(value: CborValue) -> JsonValue {
    match value {
        CborValue::Null => JsonValue::Null,
        CborValue::Bool(b) => JsonValue::Bool(b),
        CborValue::Integer(i) => {
            if let Some(v) = i64::try_from(i).ok() {
                JsonValue::from(v)
            } else if let Some(v) = u64::try_from(i).ok() {
                JsonValue::from(v)
            } else {
                JsonValue::String(i128::from(i).to_string())
            }
        }
        CborValue::Float(f) => {
            if f.is_finite() {
                JsonValue::from(f)
            } else {
                JsonValue::Null
            }
        }
        CborValue::Bytes(bytes) => JsonValue::String(bytes_to_hex(&bytes)),
        CborValue::Text(s) => JsonValue::String(s),
        CborValue::Array(items) => {
            JsonValue::Array(items.into_iter().map(cbor_value_to_json).collect())
        }
        CborValue::Map(entries) => {
            let mut map = JsonMap::new();
            for (k, v) in entries {
                let key = match k {
                    CborValue::Text(s) => s,
                    CborValue::Integer(i) => {
                        if let Some(val) = i64::try_from(i).ok() {
                            val.to_string()
                        } else if let Some(val) = u64::try_from(i).ok() {
                            val.to_string()
                        } else {
                            format!("{:?}", i)
                        }
                    }
                    other => format!("{:?}", other),
                };
                map.insert(key, cbor_value_to_json(v));
            }
            JsonValue::Object(map)
        }
        CborValue::Tag(_, boxed) => cbor_value_to_json(*boxed),
        _ => JsonValue::Null,
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

#[derive(Deserialize)]
struct NamespaceRecord {
    #[allow(dead_code)]
    kind: String,
    #[serde(default)]
    imports: Vec<ByteBuf>,
    exports: Vec<NamespaceExportRecord>,
    iface: ByteBuf,
}

#[derive(Deserialize)]
struct NamespaceExportRecord {
    name: String,
    word: ByteBuf,
}

#[derive(Deserialize)]
struct InterfaceRecord {
    #[allow(dead_code)]
    kind: String,
    names: Vec<InterfaceSymbolRecord>,
}

#[derive(Deserialize)]
struct InterfaceSymbolRecord {
    name: String,
    #[serde(rename = "type")]
    ty: InterfaceTypeRecord,
    #[serde(default)]
    effects: Vec<ByteBuf>,
}

#[derive(Deserialize)]
struct InterfaceTypeRecord {
    params: Vec<String>,
    results: Vec<String>,
}

fn html_response(body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_header(content_type("text/html; charset=utf-8"))
        .with_status_code(StatusCode(200))
}

fn json_response(body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_header(content_type("application/json"))
        .with_status_code(StatusCode(200))
}

fn error_response(status: u16, err: anyhow::Error) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = json!({ "error": err.to_string() }).to_string();
    Response::from_string(body)
        .with_header(content_type("application/json"))
        .with_status_code(StatusCode(status))
}

fn content_type(value: &str) -> Header {
    Header::from_bytes(&b"Content-Type"[..], value.as_bytes()).unwrap()
}

fn split_query(url: &str) -> (&str, Option<&str>) {
    match url.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (url, None),
    }
}

fn parse_prefix(query: &str) -> Option<String> {
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            if key == "prefix" {
                return Some(value.to_string());
            }
        }
    }
    None
}
