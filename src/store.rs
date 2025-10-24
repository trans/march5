use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, DatabaseName, OpenFlags, params};

pub fn derive_db_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(name);
    if path.extension().is_none() {
        path.set_extension("march5.db");
    }
    path
}

pub fn ensure_parent_dirs(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
    }
    Ok(())
}

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

pub fn configure_pragmas(conn: &Connection) -> Result<()> {
    conn.pragma_update(Some(DatabaseName::Main), "journal_mode", &"WAL")?;
    conn.pragma_update(Some(DatabaseName::Main), "synchronous", &"NORMAL")?;
    conn.pragma_update(Some(DatabaseName::Main), "temp_store", &"MEMORY")?;
    conn.pragma_update(Some(DatabaseName::Main), "mmap_size", &268_435_456i64)?;
    conn.pragma_update(Some(DatabaseName::Main), "cache_size", &-262_144i64)?;
    Ok(())
}

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

pub fn put_object(conn: &Connection, cid: &[u8; 32], kind: &str, cbor: &[u8]) -> Result<bool> {
    let rows = conn.execute(
        "INSERT OR IGNORE INTO object (cid, kind, cbor) VALUES (?1, ?2, ?3)",
        params![&cid[..], kind, cbor],
    )?;
    Ok(rows > 0)
}
