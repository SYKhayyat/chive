pub mod toml;

use std::collections::BTreeMap;
use std::path::{Component, Path};

use crate::act::{self, Act, ActLog};
use crate::config::RootScope;
use crate::error::{Error, Result};
use crate::model::FileEntry;

/// The containment rule every catalog path must satisfy. A path is relative to
/// the scan root, `/`-separated, and names nothing outside the root: no `..`,
/// no `.` segments, no empty segments, no absolute form, no drive prefix, no
/// backslash (a backslash is a legal filename character on Unix, but it is how
/// a Windows-authored catalog would smuggle a second separator past a `join`).
///
/// `Catalog` only ever holds validated paths, so `restore` and `clean` can
/// join a path onto a root without re-proving containment each time. This is
/// the boundary behind issue #17: a catalog is a file a stranger may hand you,
/// and `root.join("..")` is how it would write outside everything chive owns.
pub fn validate_path(path: &str) -> Result<()> {
    let refuse = |why: &str| {
        Err(Error::Catalog(format!(
            "entry path {path:?} escapes the scan root: {why}"
        )))
    };
    if path.is_empty() {
        return refuse("paths are non-empty");
    }
    if path.contains('\\') {
        return refuse("catalog paths are /-separated");
    }
    if path.contains('\0') {
        return refuse("paths never contain NUL");
    }
    // Textual segment rule: `.` and `..` are refused even where `Path`
    // normalization would quietly swallow them.
    for seg in path.split('/') {
        match seg {
            "" => return refuse("segments are non-empty (no `//`)"),
            "." => return refuse("no `.` segments"),
            ".." => return refuse("`..` cannot be joined safely"),
            _ => {}
        }
    }
    let first = path.split('/').next().unwrap_or("");
    let bytes = first.as_bytes();
    let drive_prefix = bytes.len() == 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic();
    if drive_prefix {
        return refuse("no drive-letter prefixes");
    }
    for c in std::path::Path::new(path).components() {
        if !matches!(c, Component::Normal(_)) {
            return refuse("paths are relative to the scan root");
        }
    }
    Ok(())
}

/// The root a catalog claims, checked for the properties that make it usable as
/// the anchor every containment check is relative to.
///
/// Two separate refusals, because they are two different things (D25). A root
/// that is empty, relative, or `/` is a *malformed* catalog and no setting can
/// make it acceptable — it would mean "every absolute path is inside the root".
/// A root that is absolute but outside your home is a *judgement call*, and
/// `RootScope` is where the owner answers it.
pub fn validate_root(root: &str) -> Result<()> {
    let refuse = |why: &str| {
        Err(Error::Catalog(format!(
            "catalog root {root:?} is not usable: {why}"
        )))
    };
    if root.trim().is_empty() {
        return refuse("the root is empty");
    }
    let p = Path::new(root);
    if !p.is_absolute() {
        return refuse(
            "the root must be absolute, or a later command resolves it against whatever cwd it runs from",
        );
    }
    // `/` as a root makes every absolute path "inside" it, which is exactly the
    // claim containment exists to prevent.
    if p.components().count() <= 1 {
        return refuse("`/` contains everything, so nothing is inside the root");
    }
    Ok(())
}

/// Whether an absolute `root` may be accepted under `scope`, and what to say
/// about it when it may (D25). Returns a note to print rather than swallowing the
/// observation: the moment a root is announced is the only moment the owner can
/// notice it is the wrong one.
pub fn root_in_scope(root: &str, scope: RootScope) -> Result<Option<String>> {
    if scope == RootScope::Any {
        return Ok(None);
    }
    let Some(home) = home_dir() else {
        return Ok(Some(format!(
            "catalog root is {root:?}; HOME and USERPROFILE are unset, so chive cannot tell whether it is inside your home"
        )));
    };
    // Normalise textually rather than canonicalising, because a catalog from
    // another machine legitimately names a root that does not exist here, and a
    // check that silently falls back to a lexical comparison when
    // `canonicalize` fails is the check that let #47 through:
    // `/home/u/../etc` starts with `/home/u` as a *spelling*.
    if normalize_lexical(Path::new(root)).starts_with(normalize_lexical(&home)) {
        return Ok(None);
    }
    match scope {
        RootScope::Any | RootScope::Warn => Ok(Some(format!(
            "catalog root is {root:?}, which is outside your home ({}); chive will write and delete only inside it",
            home.display()
        ))),
        RootScope::HomeOnly => Err(Error::Refused(format!(
            "catalog root {root:?} is outside your home ({}); a catalog from another machine may name a path you do not want cleaned. Scan it yourself, or set policy.catalog.root_scope",
            home.display()
        ))),
    }
}

/// Join `rel` onto the root chive is about to act against, and prove the result
/// is inside it.
///
/// The root is a parameter rather than `self.root` because restore runs against
/// the *target* machine's `--root`, which on a fresh machine is not the root the
/// catalog was written against. Checking against `self.root` would validate the
/// wrong path and prove nothing about where the write lands (issue #48).
pub fn resolve_under(root: &Path, rel: &str) -> Result<std::path::PathBuf> {
    validate_path(rel)?;
    let joined = root.join(rel);
    // A path whose *parent* escapes is already caught by `validate_path`. What
    // is left is a symlink somewhere along the way, so canonicalise the deepest
    // existing ancestor and re-append the rest: the file itself may not exist
    // yet (a restore creates it), but its parent must be real.
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    let mut probe = joined.as_path();
    let base = loop {
        match probe.canonicalize() {
            Ok(real) => break real,
            Err(_) => match (probe.file_name(), probe.parent()) {
                (Some(name), Some(parent)) => {
                    tail.push(name);
                    probe = parent;
                }
                _ => break probe.to_path_buf(),
            },
        }
    };
    let mut out = base;
    for name in tail.iter().rev() {
        out.push(name);
    }
    let root_real = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if !out.starts_with(&root_real) {
        return Err(Error::Refused(format!(
            "{rel:?} resolves to {}, which is outside the root {}",
            out.display(),
            root_real.display()
        )));
    }
    Ok(out)
}

/// Resolve `.` and `..` in a path *textually`, leaving a leading `/` and any
/// prefix before the first `..` that cannot be resolved.
///
/// Deliberately not `canonicalize`: this runs on paths from a catalog that
/// describes a different machine, so the path usually does not exist here, and a
/// filesystem-dependent answer would be both unavailable and racy.
pub fn normalize_lexical(p: &Path) -> std::path::PathBuf {
    let mut out: Vec<std::ffi::OsString> = Vec::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                // Only a `..` that can consume a real segment is consumed; a
                // leading one has nowhere to go and is kept, so the result stays
                // absolute and the comparison cannot be satisfied by it.
                match out.last() {
                    Some(last) if last != ".." => {
                        out.pop();
                    }
                    _ => out.push("..".into()),
                }
            }
            other => out.push(other.as_os_str().to_os_string()),
        }
    }
    let mut s = std::path::PathBuf::from("/");
    for seg in out {
        s.push(seg);
    }
    s
}

/// The user's home, or `None` when the environment does not say.
pub fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
}

/// A catalog: metadata, every recorded path, and the owner act log.
///
/// The present state of a machine, addressed by path relative to the scan root
/// so the same catalog describes any machine whose home lives at a different
/// absolute path.
///
/// The act log travels *inside* the catalog (D20). It is what makes an owner's
/// decision durable — the scanner reads it rather than overwriting it — and
/// what makes it portable, since the catalog is the file a new machine imports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    /// The absolute scan root on the machine that produced this catalog.
    pub root: String,
    /// The scan's canonical timestamp (ISO 8601 UTC).
    pub scanned_at: String,
    /// The hostname that produced the catalog.
    pub host: String,
    /// Entries kept sorted by `path` and addressed via [`Catalog::by_path`].
    files: Vec<FileEntry>,
    acts: ActLog,
}

impl Catalog {
    /// Every path is validated here, so a `Catalog` in memory never holds a
    /// path that could address a file outside the scan root. The same rule
    /// applies to the act log: a hand-edited catalog may name any path it likes,
    /// and a `teach` act naming one would otherwise smuggle it into a recipe.
    pub fn new(
        root: String,
        scanned_at: String,
        host: String,
        files: Vec<FileEntry>,
        acts: ActLog,
    ) -> Result<Self> {
        validate_root(&root)?;
        for e in &files {
            validate_path(&e.path)?;
        }
        for a in acts.acts() {
            validate_path(&a.path)?;
        }
        act::check_unique_seqs(acts.acts())?;
        let mut files = files;
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Catalog {
            root,
            scanned_at,
            host,
            files,
            acts,
        })
    }

    /// The owner act log. Read by the scanner, appended to by the verbs.
    pub fn acts(&self) -> &ActLog {
        &self.acts
    }

    /// Resolve a catalog path to an absolute path under the root, refusing
    /// anything that does not land there.
    ///
    /// The join is *fallible on purpose*. The previous shape -- `root.join(rel)`
    /// filtered by a lexical `starts_with` -- failed open in two ways: it could
    /// not notice that `root` itself was untrusted, and a string prefix says
    /// nothing about a symlink under the root, which every later filesystem call
    /// follows (issue #48). Canonicalising the parent and re-checking is what
    /// makes the answer about the filesystem rather than about the spelling.
    pub fn resolve(&self, rel: &str) -> Result<std::path::PathBuf> {
        resolve_under(Path::new(&self.root), rel)
    }

    /// Record an owner act, stamping it with the next sequence number.
    ///
    /// Validates the path here for the same reason the constructor does: an act
    /// is as much a path a `restore` or `clean` will act on as an entry is, and
    /// a verb must not be able to write one the next load would refuse (#17).
    pub fn record(&mut self, act: Act) -> Result<Act> {
        validate_path(&act.path)?;
        Ok(self.acts.append(act))
    }

    pub fn files(&self) -> &[FileEntry] {
        &self.files
    }

    pub fn by_path(&self, path: &str) -> Option<&FileEntry> {
        // Binary search over the sorted entries.
        self.files
            .binary_search_by(|e| e.path.as_str().cmp(path))
            .ok()
            .map(|i| &self.files[i])
    }

    /// Replace one entry by path, keeping sorted order. The same containment
    /// rule as the constructor applies; an invalid path is refused.
    pub fn upsert(&mut self, entry: FileEntry) -> Result<()> {
        validate_path(&entry.path)?;
        match self.files.binary_search_by(|e| e.path.cmp(&entry.path)) {
            Ok(i) => self.files[i] = entry,
            Err(i) => self.files.insert(i, entry),
        }
        Ok(())
    }

    /// Index of every path, efficient for bulk lookups.
    pub fn index(&self) -> BTreeMap<&str, &FileEntry> {
        self.files.iter().map(|e| (e.path.as_str(), e)).collect()
    }
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
    use crate::act::ActLog;
    use crate::model::{Category, Origin, Source, Verdict};

    fn cat(root: &str, files: Vec<FileEntry>) -> Result<Catalog> {
        Catalog::new(
            root.into(),
            "t".into(),
            "h".into(),
            files,
            ActLog::default(),
        )
    }

    fn entry(path: &str) -> FileEntry {
        FileEntry::new_unknown(path.into(), Some(Category::Document), 1, None)
    }

    #[test]
    fn by_path_finds_and_misses() {
        let mut c = cat("/home/u", vec![entry("b"), entry("a")]).unwrap();
        c.upsert(entry("c")).unwrap();
        assert!(c.by_path("a").is_some());
        assert!(c.by_path("b").is_some());
        assert!(c.by_path("c").is_some());
        assert!(c.by_path("missing").is_none());
    }

    #[test]
    fn constructor_sorts_by_path() {
        let c = cat("/r", vec![entry("zeta"), entry("alpha")]).unwrap();
        let paths: Vec<_> = c.files().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["alpha", "zeta"]);
    }

    #[test]
    fn reinserting_existing_path_updates_in_place() {
        let mut c = cat("/r", vec![entry("a")]).unwrap();
        let upgraded = FileEntry::new_restorable(
            "a".into(),
            None,
            "git checkout -- A".into(),
            Source::Verified,
            Origin::Chive,
            3,
            None,
        );
        c.upsert(upgraded).unwrap();
        assert_eq!(c.files().len(), 1);
        assert_eq!(c.by_path("a").unwrap().verdict, Verdict::Restorable);
    }

    #[test]
    fn escapes_are_refused_at_construction() {
        for evil in [
            "../outside.txt",
            "a/../../outside.txt",
            "./hidden",
            "a/./b",
            "/etc/passwd",
            "a\\win",
            "C:/Users/x",
            "",
            "a/../b",
            "a//b",
        ] {
            let err = cat("/r", vec![entry(evil)]);
            assert!(err.is_err(), "{evil:?} must be refused");
        }
    }

    #[test]
    fn escape_refusal_names_the_reason() {
        let err = cat("/r", vec![entry("../outside.txt")]).unwrap_err();
        assert!(err.to_string().contains("escapes the scan root"));
    }

    #[test]
    fn ordinary_nested_paths_are_accepted() {
        cat(
            "/r",
            vec![
                entry("conf/emacs.d/init.el"),
                entry("my docs/file name.txt"),
                entry("dot.file"),
            ],
        )
        .unwrap();
    }

    #[test]
    fn upsert_validates_like_the_constructor() {
        let mut c = cat("/r", vec![]).unwrap();
        assert!(c.upsert(entry("../evil")).is_err());
        assert!(c.files().is_empty(), "a refused entry is never stored");
    }

    #[test]
    fn an_escaping_act_path_is_refused_like_any_entry_path() {
        // A hand-edited act log is as much untrusted input as an entry list: a
        // `teach` naming `../escape` would smuggle the path into a recipe.
        let acts = ActLog::new(vec![Act::dispose(0, "../escape")], 1);
        assert!(Catalog::new("/r".into(), "t".into(), "h".into(), vec![], acts).is_err());
    }

    #[test]
    fn the_act_log_travels_with_the_catalog() {
        let acts = ActLog::new(vec![Act::dispose(0, "a")], 1);
        let c = Catalog::new("/r".into(), "t".into(), "h".into(), vec![], acts).unwrap();
        assert_eq!(
            c.acts().latest("a").unwrap().kind,
            crate::act::ActKind::Dispose
        );
    }

    #[test]
    fn record_appends_and_stamps_a_sequence_number() {
        let mut c = cat("/r", vec![]).unwrap();
        let recorded = c.record(Act::teach(0, "a", "echo a")).unwrap();
        assert_eq!(recorded.seq, 0);
        assert_eq!(c.acts().len(), 1);
        assert_eq!(c.acts().next_seq(), 1);
    }

    #[test]
    fn a_root_that_is_empty_relative_or_slash_is_refused() {
        // D25's unconditional half: no setting can make `/` an acceptable root,
        // because it would make every absolute path "inside" the root.
        for bad in ["", "   ", ".", "relative/path", "/"] {
            assert!(validate_root(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn an_ordinary_absolute_root_is_accepted() {
        assert!(validate_root("/home/u").is_ok());
        assert!(validate_root("/").is_err());
        assert!(validate_root("/etc").is_ok());
    }

    #[test]
    fn a_root_outside_home_is_refused_by_default_and_allowed_on_request() {
        let home = home_dir().expect("a home dir");
        let base = tempfile::tempdir().unwrap();
        let outside = base.path().join("elsewhere").to_string_lossy().to_string();
        assert!(root_in_scope(&outside, RootScope::HomeOnly).is_err());
        // A `..` spelling must not buy a pass: `/home/u/../etc` starts with
        // `/home/u` lexically, which is the check that let #47 through.
        let dotted = format!("{}/../etc", home.display());
        assert!(
            root_in_scope(&dotted, RootScope::HomeOnly).is_err(),
            "`..` must not buy a pass, even though the path does not exist here"
        );
        assert!(
            root_in_scope(&home.to_string_lossy(), RootScope::HomeOnly).is_ok(),
            "the home itself is in scope"
        );
        assert!(root_in_scope(&outside, RootScope::Warn).is_ok());
        assert!(root_in_scope(&outside, RootScope::Any).is_ok());
        let inside = home.join("Documents").to_string_lossy().to_string();
        assert!(root_in_scope(&inside, RootScope::HomeOnly).is_ok());
        assert_eq!(
            root_in_scope(&inside, RootScope::Warn).unwrap(),
            None,
            "a root inside home needs no announcement"
        );
    }

    #[test]
    fn resolve_refuses_a_path_that_escapes_through_a_symlink() {
        // Issue #48: `starts_with` is a spelling check and cannot see a symlink.
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("root");
        let outside = base.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("victim.txt"), "x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();

        let c = cat(root.to_str().unwrap(), vec![]).unwrap();
        let escaped = c.resolve("link/victim.txt");
        #[cfg(unix)]
        assert!(
            escaped.is_err(),
            "a symlinked directory must not carry a path out of the root"
        );
        assert!(c.resolve("inside.txt").is_ok());
    }

    #[test]
    fn record_refuses_an_escaping_path() {
        let mut c = cat("/r", vec![]).unwrap();
        assert!(c.record(Act::teach(0, "../escape", "echo x")).is_err());
        assert!(
            c.acts().is_empty(),
            "a refused act is never stored, exactly like a refused entry"
        );
    }
}
