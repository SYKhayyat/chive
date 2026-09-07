//! The core promise, end to end: move to a new machine and chive re-creates
//! what is missing. Departs from the other harness tests by using *two* store
//! dirs and *two* trees — machine A (source) and machine B (target) — wiring
//! the offline path exactly as the README documents:
//!
//!   scan on A -> export the catalog -> import it on B -> restore --all on B.

use crate::harness::Env;

#[test]
fn a_catalog_survives_moving_to_a_new_machine() {
    // --- machine A: a source home with a taught recipe + an orphan + a symlink
    let a = Env::new("migrate_a");
    a.put("seed/settings.toml", "a=1");
    a.put("conf/settings.toml", "ignoreme"); // scan sees the present file
    a.ok(&["scan", a.home.to_str().unwrap()]);
    // teach a recipe that re-derives conf/settings.toml from ~/seed. NOTE: the
    // recipe must mkdir the parent — chive's restore does not create it (open
    // issue: nested {dest} fails on a fresh machine).
    a.ok(&[
        "teach",
        "conf/settings.toml",
        "--method",
        "mkdir -p \"$(dirname \"{dest}\")\" && cp ~/seed/settings.toml \"{dest}\"",
    ]);
    // an orphan that must NOT be restored (no recipe)
    a.put("Pictures/photo.nef", "bytes");
    a.ok(&["scan", a.home.to_str().unwrap()]);

    let (out, code) = a.run(&["status", "--restorable"]);
    assert!(code == 0, "{out}");
    assert!(
        out.contains("conf/settings.toml"),
        "taught file is restorable:\n{out}"
    );

    // --- export the catalog to a portable file
    let catalog = a.root.join("catalog.toml");
    a.ok(&["export", "--to", catalog.to_str().unwrap()]);
    assert!(catalog.exists(), "catalog exported");

    // --- machine B: a fresh store + a fresh tree carrying only the seed
    let b = Env::new("migrate_b");
    b.put("seed/settings.toml", "a=1");

    // import the source catalog (which claims root = machine A's home)
    b.ok(&["import", "--from", catalog.to_str().unwrap()]);

    // restore --all onto machine B. The taught recipe writes conf/settings.toml.
    let restore = b.ok(&["restore", "--root", b.home.to_str().unwrap()]);
    assert!(
        b.home.join("conf/settings.toml").exists(),
        "restored the migrated file:\n{restore}"
    );
    assert_eq!(
        std::fs::read_to_string(b.home.join("conf/settings.toml")).unwrap(),
        "a=1"
    );
    // the orphan must NOT appear on B
    assert!(
        !b.home.join("Pictures/photo.nef").exists(),
        "orphan has no recipe and must not be restored:\n{restore}"
    );
}

#[test]
fn restore_refuses_to_clobber_an_existing_file() {
    // The no-clobber contract (D15): a file already present on the target is
    // never overwritten.
    let a = Env::new("noclobber_a");
    a.put("seed/n.txt", "new");
    a.put("conf/n.txt", "present-on-a");
    a.ok(&["scan", a.home.to_str().unwrap()]);
    a.ok(&[
        "teach",
        "conf/n.txt",
        "--method",
        "mkdir -p \"$(dirname \"{dest}\")\" && cp ~/seed/n.txt \"{dest}\"",
    ]);
    a.ok(&["scan", a.home.to_str().unwrap()]);
    let catalog = a.root.join("catalog.toml");
    a.ok(&["export", "--to", catalog.to_str().unwrap()]);

    let b = Env::new("noclobber_b");
    b.put("seed/n.txt", "new");
    b.ok(&["import", "--from", catalog.to_str().unwrap()]);
    // B already has the destination with different content
    b.put("conf/n.txt", "precious");
    let restore = b.ok(&["restore", "--root", b.home.to_str().unwrap()]);
    assert!(
        restore.contains("skipped (exists)"),
        "existing file must be skipped, not overwritten:\n{restore}"
    );
    assert_eq!(
        std::fs::read_to_string(b.home.join("conf/n.txt")).unwrap(),
        "precious",
        "no-clobber: existing content preserved"
    );
}
