//! SQLite-backed persistence helpers for March content-addressed objects.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, DatabaseName, OpenFlags, params};

/// Derive a database file path, appending `.march5.db` when no extension is supplied.
pub fn derive_db_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(name);
    if path.extension().is_none() {
        path.set_extension("march5.db");
    }
    path
}

/// Ensure directories required to create `path` exist.
pub fn ensure_parent_dirs(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
    }
    Ok(())
}

/// Create a new March store on disk and initialise schema/PRAGMA settings.
pub fn create_store(path: &Path) -> Result<Connection> {
    ensure_parent_dirs(path)?;
    if path.exists() {
        bail!("database already exists at {}", path.display());
    }

    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .with_context(|| format!("failed to create {}", path.display()))?;

    configure_pragmas(&conn)?;
    install_schema(&conn)?;
    Ok(conn)
}

/// Open an existing March store, applying PRAGMA preferences.
pub fn open_store(path: &Path) -> Result<Connection> {
    if !path.exists() {
        bail!("database not found at {}", path.display());
    }

    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .with_context(|| format!("failed to open {}", path.display()))?;
    configure_pragmas(&conn)?;
    install_schema(&conn)?;
    Ok(conn)
}

/// Apply recommended PRAGMA settings for the March store.
pub fn configure_pragmas(conn: &Connection) -> Result<()> {
    conn.pragma_update(Some(DatabaseName::Main), "journal_mode", &"WAL")?;
    conn.pragma_update(Some(DatabaseName::Main), "synchronous", &"NORMAL")?;
    conn.pragma_update(Some(DatabaseName::Main), "temp_store", &"MEMORY")?;
    conn.pragma_update(Some(DatabaseName::Main), "mmap_size", &268_435_456i64)?;
    conn.pragma_update(Some(DatabaseName::Main), "cache_size", &-262_144i64)?;
    Ok(())
}

/// Create tables and indexes if they do not yet exist.
pub fn install_schema(conn: &Connection) -> Result<()> {
    const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS object (
  cid   BLOB PRIMARY KEY,
  kind  TEXT NOT NULL,
  cbor  BLOB NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS name_index (
  scope TEXT NOT NULL,
  name  TEXT NOT NULL,
  cid   BLOB NOT NULL,
  PRIMARY KEY (scope, name)
);

CREATE TABLE IF NOT EXISTS code_cache (
  subgraph_cid BLOB NOT NULL,
  arch   TEXT NOT NULL,
  abi    TEXT NOT NULL,
  flags  INTEGER NOT NULL,
  blob   BLOB NOT NULL,
  PRIMARY KEY (subgraph_cid, arch, abi, flags)
);

CREATE INDEX IF NOT EXISTS object_kind_idx ON object(kind);
"#;

    conn.execute_batch(SCHEMA)?;
    Ok(())
}

/// Insert an object if missing; returns `true` when inserted.
pub fn put_object(conn: &Connection, cid: &[u8; 32], kind: &str, cbor: &[u8]) -> Result<bool> {
    let rows = conn.execute(
        "INSERT OR IGNORE INTO object (cid, kind, cbor) VALUES (?1, ?2, ?3)",
        params![&cid[..], kind, cbor],
    )?;
    Ok(rows > 0)
}

/// Register or update a name → CID mapping under `scope`.
pub fn put_name(conn: &Connection, scope: &str, name: &str, cid: &[u8; 32]) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO name_index (scope, name, cid) VALUES (?1, ?2, ?3)",
        params![scope, name, &cid[..]],
    )?;
    Ok(())
}
