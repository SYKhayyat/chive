//! SQLite — a derived working index.
//!
//! Per decision D9 the TOML catalog is the source of truth; this database is a
//! *derived* copy that [`replace`] rebuilds wholesale and [`load`] reads back.
//! It exists for fast queries (status, restorable-set) without reparsing TOML.
//! Nothing here decides truth; it only mirrors it.

use std::path::Path;

use rusqlite::Connection;

use crate::act::{Act, ActKind, ActLog};
use crate::catalog::Catalog;
use crate::error::{Error, Result};
use crate::model::{Category, FileEntry, Origin, Source, Verdict};

const SCHEMA_VERSION: i64 = 2;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
    path           TEXT PRIMARY KEY,
    verdict        TEXT NOT NULL,
    category       TEXT,
    restore_method TEXT,
    source         TEXT,
    verdict_source TEXT NOT NULL,
    present        INTEGER NOT NULL,
    size           INTEGER,
    modified       TEXT
);

CREATE TABLE IF NOT EXISTS acts (
    seq    INTEGER PRIMARY KEY,
    path   TEXT NOT NULL,
    kind   TEXT NOT NULL,
    method TEXT
);
";

/// Open (creating if absent) and migrate the working index at `path`.
pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).map_err(db_err)?;
    conn.execute_batch(SCHEMA).map_err(db_err)?;
    // Only an absent row means "pre-versioning index, assume current". A row
    // that cannot be read as a number is not coercible to the current version
    // — that would disable the gate exactly when the file is suspect.
    let v: i64 = match conn.query_row(
        "SELECT value FROM meta WHERE key = 'schema_version'",
        [],
        |r| r.get(0),
    ) {
        Ok(v) => v,
        Err(rusqlite::Error::QueryReturnedNoRows) => SCHEMA_VERSION,
        Err(e) => {
            return Err(Error::Catalog(format!(
                "index schema version is unreadable ({e}); rebuild with `chive scan`"
            )));
        }
    };
    if v != SCHEMA_VERSION {
        return Err(Error::Catalog(format!(
            "index schema version {v} is not supported (wanted {SCHEMA_VERSION}); rebuild with `chive scan`"
        )));
    }
    Ok(conn)
}

/// A raw connection for the writer. The version gate guards *readers*; a
/// writer must always be able to open the index so it can restamp it wholesale
/// (otherwise a wrongly-versioned index could never be repaired by scanning).
pub fn open_for_write(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).map_err(db_err)?;
    conn.execute_batch(SCHEMA).map_err(db_err)?;
    Ok(conn)
}

/// Wipe and rebuild the index from a catalog. Runs in a transaction so a crash
/// mid-rebuild cannot leave a half-new index.
pub fn replace(conn: &mut Connection, catalog: &Catalog) -> Result<()> {
    let tx = conn.transaction().map_err(db_err)?;
    tx.execute_batch("DELETE FROM files; DELETE FROM acts;")
        .map_err(db_err)?;
    tx.execute("DELETE FROM meta WHERE key <> 'schema_version'", [])
        .map_err(db_err)?;

    // Stamp the version this index was written with: open() refuses an index
    // whose version it cannot read, which is only possible if replace() wrote
    // one (issue #31 — the gate existed but nothing ever set it).
    set(&tx, "schema_version", &SCHEMA_VERSION.to_string())?;
    set(&tx, "root", &catalog.root)?;
    set(&tx, "scanned_at", &catalog.scanned_at)?;
    set(&tx, "host", &catalog.host)?;
    set(&tx, "next_seq", &catalog.acts().next_seq().to_string())?;

    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO files
                   (path, verdict, category, restore_method, source,
                    verdict_source, present, size, modified)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .map_err(db_err)?;
        for e in catalog.files() {
            stmt.execute(rusqlite::params![
                e.path,
                e.verdict.as_str(),
                e.category.map(Category::as_str),
                e.restore_method,
                e.source.map(Source::as_str),
                e.verdict_source.as_str(),
                e.present as i64,
                e.size,
                e.modified,
            ])
            .map_err(db_err)?;
        }
    }
    {
        let mut stmt = tx
            .prepare("INSERT INTO acts (seq, path, kind, method) VALUES (?1, ?2, ?3, ?4)")
            .map_err(db_err)?;
        for a in catalog.acts().acts() {
            stmt.execute(rusqlite::params![
                a.seq as i64,
                a.path,
                a.kind.as_str(),
                a.method,
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
        .prepare("SELECT path, verdict, category, restore_method, source, verdict_source, present, size, modified FROM files")
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| {
            let verdict: String = r.get(1)?;
            let category: Option<String> = r.get(2)?;
            let source: Option<String> = r.get(4)?;
            let verdict_source: String = r.get(5)?;
            Ok((
                r.get::<_, String>(0)?,
                verdict,
                category,
                r.get::<_, Option<String>>(3)?,
                source,
                verdict_source,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, Option<String>>(8)?,
            ))
        })
        .map_err(db_err)?;

    let mut files = Vec::new();
    for row in rows {
        let (path, verdict, category, method, source, verdict_source, present, size, modified) =
            row.map_err(db_err)?;
        files.push(FileEntry {
            path,
            verdict: parse::<Verdict>(&verdict)?,
            category: category.map(|c| parse::<Category>(&c)).transpose()?,
            restore_method: method,
            source: source.map(|s| parse::<Source>(&s)).transpose()?,
            verdict_source: parse::<Origin>(&verdict_source)?,
            present: present != 0,
            size,
            modified,
        });
    }

    let mut acts = Vec::new();
    let mut stmt = conn
        .prepare("SELECT seq, path, kind, method FROM acts ORDER BY seq")
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(db_err)?;
    for row in rows {
        let (seq, path, kind, method) = row.map_err(db_err)?;
        acts.push(Act {
            seq: seq as u64,
            path,
            kind: parse::<ActKind>(&kind)?,
            method,
        });
    }
    let next_seq = get(conn, "next_seq")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    Catalog::new(root, scanned_at, host, files, ActLog::new(acts, next_seq))
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
            Origin::Chive,
            100,
            None,
        );
        let o = FileEntry::new_unknown("tmp/x".into(), None, 3, None);
        let mut acts = ActLog::default();
        acts.append(Act::teach(0, "future/x", "echo built"));
        Catalog::new("/home/u".into(), "t".into(), "h".into(), vec![e, o], acts).unwrap()
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
    fn replace_stamps_the_schema_version() {
        // Issue #31: schema_version was read on open but never written, so the
        // version gate could never fire.
        let mut conn = temp_conn();
        replace(&mut conn, &sample()).unwrap();
        assert_eq!(get(&conn, "schema_version").as_deref(), Some("2"));
    }

    #[test]
    fn an_index_from_an_unknown_future_version_is_refused() {
        let mut conn = temp_conn();
        replace(&mut conn, &sample()).unwrap();
        conn.execute(
            "UPDATE meta SET value = '999' WHERE key = 'schema_version'",
            [],
        )
        .unwrap();
        let path = std::path::PathBuf::from(conn.path().unwrap());
        let err = open(&path).unwrap_err();
        assert!(
            err.to_string().contains("schema version"),
            "open must refuse an index it cannot read: {err}"
        );
    }

    #[test]
    fn verdict_from_str_covers_all_spellings() {
        assert_eq!(Verdict::from_str("restorable"), Ok(Verdict::Restorable));
        assert_eq!(Verdict::from_str("unknown"), Ok(Verdict::Unknown));
        assert_eq!(Verdict::from_str("disposable"), Ok(Verdict::Disposable));
        assert!(Verdict::from_str("bogus").is_err());
    }

    #[test]
    fn the_act_log_round_trips_through_the_index() {
        // D20: the derived index carries the owner's decisions too, so a
        // rebuild cannot silently lose them.
        let mut conn = temp_conn();
        let c = sample();
        replace(&mut conn, &c).unwrap();
        let back = load(&conn).unwrap();
        assert_eq!(back.acts().acts(), c.acts().acts());
        assert_eq!(back.acts().next_seq(), c.acts().next_seq());
    }
}
