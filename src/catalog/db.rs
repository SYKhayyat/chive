//! SQLite — a derived working index.
//!
//! Per decision D9 the TOML catalog is the source of truth; this database is a
//! *derived* copy that [`replace`] rebuilds wholesale and [`load`] reads back.
//! It exists for fast queries (status, restorable-set) without reparsing TOML.
//! Nothing here decides truth; it only mirrors it.

use std::path::Path;

use rusqlite::Connection;

use crate::catalog::Catalog;
use crate::error::{Error, Result};
use crate::model::{Category, FileEntry, Source, Status};

const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
    path                  TEXT PRIMARY KEY,
    status                TEXT NOT NULL,
    category              TEXT,
    restore_method        TEXT,
    source                TEXT,
    not_restorable_reason TEXT,
    size                  INTEGER,
    modified              TEXT
);
";

/// Open (creating if absent) and migrate the working index at `path`.
pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).map_err(db_err)?;
    conn.execute_batch(SCHEMA).map_err(db_err)?;
    let v: i64 = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(SCHEMA_VERSION);
    if v != SCHEMA_VERSION {
        return Err(Error::Catalog(format!(
            "index schema version {v} is not supported (wanted {SCHEMA_VERSION}); rebuild with `chive scan`"
        )));
    }
    Ok(conn)
}

/// Wipe and rebuild the index from a catalog. Runs in a transaction so a crash
/// mid-rebuild cannot leave a half-new index.
pub fn replace(conn: &mut Connection, catalog: &Catalog) -> Result<()> {
    let tx = conn.transaction().map_err(db_err)?;
    tx.execute_batch("DELETE FROM files;").map_err(db_err)?;
    tx.execute("DELETE FROM meta WHERE key <> 'schema_version'", [])
        .map_err(db_err)?;

    set(&tx, "root", &catalog.root)?;
    set(&tx, "scanned_at", &catalog.scanned_at)?;
    set(&tx, "host", &catalog.host)?;

    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO files
                   (path, status, category, restore_method, source,
                    not_restorable_reason, size, modified)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .map_err(db_err)?;
        for e in catalog.files() {
            stmt.execute(rusqlite::params![
                e.path,
                e.status.as_str(),
                e.category.map(Category::as_str),
                e.restore_method,
                e.source.map(Source::as_str),
                e.not_restorable_reason,
                e.size,
                e.modified,
            ])
            .map_err(db_err)?;
        }
    }
    tx.commit().map_err(db_err)?;
    Ok(())
}

/// Read the whole index back into a [`Catalog`]. Errors if the index is empty
/// or has no meta — the caller distinguishes "nothing scanned yet".
pub fn load(conn: &Connection) -> Result<Catalog> {
    let root = get(conn, "root").ok_or(Error::MissingCatalog)?;
    let scanned_at = get(conn, "scanned_at").unwrap_or_default();
    let host = get(conn, "host").unwrap_or_default();

    let mut stmt = conn
        .prepare("SELECT path, status, category, restore_method, source, not_restorable_reason, size, modified FROM files")
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| {
            let status: String = r.get(1)?;
            let category: Option<String> = r.get(2)?;
            let source: Option<String> = r.get(4)?;
            Ok((
                r.get::<_, String>(0)?,
                status,
                category,
                r.get::<_, Option<String>>(3)?,
                source,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        })
        .map_err(db_err)?;

    let mut files = Vec::new();
    for row in rows {
        let (path, status, category, method, source, reason, size, modified) =
            row.map_err(db_err)?;
        files.push(FileEntry {
            path,
            status: parse::<Status>(&status)?,
            category: category.map(|c| parse::<Category>(&c)).transpose()?,
            restore_method: method,
            source: source.map(|s| parse::<Source>(&s)).transpose()?,
            not_restorable_reason: reason,
            size,
            modified,
        });
    }
    Catalog::new(root, scanned_at, host, files)
}

fn set(tx: &rusqlite::Transaction<'_>, key: &str, value: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map_err(db_err)?;
    Ok(())
}

fn get(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .ok()
}

fn parse<T: std::str::FromStr>(s: &str) -> Result<T> {
    s.parse()
        .map_err(|_| Error::Catalog(format!("unrecognised value {s:?} in index")))
}

fn db_err(e: rusqlite::Error) -> Error {
    Error::Catalog(e.to_string())
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

    /// Each call opens its own private, unique index file so tests never see
    /// each other's rows (the earlier shared-process-id path did).
    fn temp_conn() -> Connection {
        let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        let name = format!("chive-db-{}-{id}.sqlite", std::process::id());
        open(&std::env::temp_dir().join(name)).unwrap()
    }

    fn sample() -> Catalog {
        let e = FileEntry::new_restorable(
            "conf/init.el".into(),
            Some(Category::Config),
            "git checkout -- init.el".into(),
            Source::Verified,
            100,
            None,
        );
        let o = FileEntry::new_orphaned("tmp/x".into(), None, 3, None);
        Catalog::new("/home/u".into(), "t".into(), "h".into(), vec![e, o]).unwrap()
    }

    #[test]
    fn replace_then_load_round_trips() {
        let mut conn = temp_conn();
        let c = sample();
        replace(&mut conn, &c).unwrap();
        assert_eq!(load(&conn).unwrap(), c);
    }

    #[test]
    fn replace_is_idempotent_and_wipes_old_rows() {
        let mut conn = temp_conn();
        let a = sample();
        let b = sample();
        replace(&mut conn, &a).unwrap();
        replace(&mut conn, &b).unwrap();
        assert_eq!(load(&conn).unwrap().files().len(), 2);
    }

    #[test]
    fn load_missing_meta_reports_missing_catalog() {
        let conn = temp_conn();
        let err = load(&conn).unwrap_err();
        assert!(matches!(err, Error::MissingCatalog));
    }

    #[test]
    fn status_from_str_covers_all_spellings() {
        assert_eq!(Status::from_str("restorable"), Ok(Status::Restorable));
        assert_eq!(
            Status::from_str("not-restorable"),
            Ok(Status::NotRestorable)
        );
        assert_eq!(Status::from_str("temporary"), Ok(Status::Temporary));
        assert_eq!(Status::from_str("orphaned"), Ok(Status::Orphaned));
        assert!(Status::from_str("bogus").is_err());
    }
}
