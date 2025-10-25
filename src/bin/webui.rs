use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, anyhow};
use clap::Parser;
use march5::{cid, create_store, derive_db_path, get_name, load_object_cbor, open_store};
use rusqlite::params;
use serde_json::json;
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
        [] | [""] => html_response(index_page()),
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
    let mut value: serde_json::Value = serde_cbor::from_slice(&cbor)?;
    if let serde_json::Value::Object(ref mut map) = value {
        map.insert(
            "_cid".into(),
            serde_json::Value::String(cid::to_hex(&cid_bytes)),
        );
        map.insert("_kind".into(), serde_json::Value::String(kind));
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

fn index_page() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>March α₅ Web UI</title>
    <style>
      body { font-family: sans-serif; margin: 2rem; }
      pre { background: #f4f4f4; padding: 1rem; overflow-x: auto; }
      section { margin-bottom: 2rem; }
      input[type=text] { width: 280px; }
    </style>
  </head>
  <body>
    <h1>March α₅ Web UI</h1>
    <section>
      <h2>Helpful Endpoints</h2>
      <ul>
        <li><code>/api/list/iface?prefix=demo</code></li>
        <li><code>/api/iface/demo.math/iface</code></li>
        <li><code>/api/list/namespace</code></li>
        <li><code>/api/namespace/demo.math</code></li>
        <li><code>/api/list/word?prefix=demo.math/</code></li>
        <li><code>/api/word/demo.math/difference</code></li>
      </ul>
    </section>
  </body>
</html>
"#
    .to_string()
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
