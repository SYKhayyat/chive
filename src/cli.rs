//! The command line: parsing and the thin dispatch to the operations in `app`
//! and `action`. Keeping parsing and effect separate means every command's body
//! is a plain function over [`App`], testable without a terminal.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::act::Act;
use crate::action;
use crate::app::App;
use crate::catalog::Catalog;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::model::{FileEntry, Origin, Source, Verdict};
use crate::store::Store;

/// chive — the `home.nix` you never wrote. Keep a recipe for every meaningful
/// file, and re-derive what is missing on a new or damaged machine.
#[derive(Debug, Parser)]
#[command(name = "chive", version, about)]
pub struct Cli {
    /// Override the config directory (default: $CHIVE_CONFIG_DIR, else ~/.config/chive).
    #[arg(global = true, long)]
    config_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Scan a machine and record a recipe for every file.
    Scan {
        /// The path to scan (usually `~/`).
        path: PathBuf,
        /// Extra directory basenames to skip, beyond the config ignore list.
        #[arg(long)]
        ignore: Vec<String>,
        /// Write the catalog TOML here as well as to the store, so the archive
        /// is updated by the scan rather than by a separate manual export.
        #[arg(long)]
        to: Option<PathBuf>,
    },
    /// List catalog entries, optionally filtered by verdict.
    Status {
        /// Show only restorable entries.
        #[arg(long, conflicts_with_all = ["unknown", "disposable"])]
        restorable: bool,
        /// Show only holes (entries chive cannot rebuild).
        #[arg(long, conflicts_with_all = ["restorable", "disposable"])]
        unknown: bool,
        /// Show only entries judged disposable.
        #[arg(long, conflicts_with_all = ["restorable", "unknown"])]
        disposable: bool,
    },
    /// List what chive cannot rebuild, largest first. The work list.
    Holes {
        /// Show at most this many rows.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Show catalog statistics.
    Stats,
    /// Preview a restore without executing anything.
    Plan {
        #[command(subcommand)]
        sub: PlanCmd,
    },
    /// Re-derive files by running their recipes. Default: every restorable file.
    Restore {
        /// Explicit spelling of the default: restore every restorable file.
        /// Paths and --all are the same operation; `chive restore --all` is
        /// the documented form.
        #[arg(long, conflicts_with_all = ["paths", "exclude"])]
        all: bool,
        /// The target root used to resolve `{dest}`.
        #[arg(long)]
        root: Option<PathBuf>,
        /// Restore only these relative paths.
        paths: Vec<String>,
        /// Omit these relative paths.
        #[arg(long)]
        exclude: Vec<String>,
    },
    /// Teach a recipe for a file, making it restorable.
    Teach {
        /// The relative path to teach. May not exist on this machine yet.
        path: String,
        /// The shell recipe. `{dest}` expands at restore time.
        #[arg(long)]
        method: String,
    },
    /// Judge a file disposable: a known gap that `clean` may remove.
    Dispose {
        /// The relative path to judge disposable.
        path: String,
    },
    /// Take back your judgement, so the path re-derives from evidence.
    Withdraw {
        /// The relative path to withdraw.
        path: String,
    },
    /// Remove disposable files from disk.
    Clean {
        /// Preview what would be removed without removing anything.
        #[arg(long)]
        dry_run: bool,
        /// Skip the confirmation prompt.
        #[arg(long)]
        force: bool,
    },
    /// Load a catalog TOML (from a file or the default location) into the store.
    Import {
        /// The catalog TOML to load. Defaults to the store's default location.
        #[arg(long)]
        from: Option<PathBuf>,
    },
    /// Export the current catalog as the versionable TOML.
    Export {
        /// Where to write the catalog TOML. Defaults to the store's location.
        #[arg(long)]
        to: Option<PathBuf>,
    },
}

/// Return SIGPIPE to its default behaviour so writing to a closed pipe (e.g.
/// `chive ... | head`) terminates quietly instead of panicking in the std
/// printer. This is the usual, correct convention for Unix CLI tools.
fn restore_sigpipe() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

/// The action to preview under `plan`.
#[derive(Debug, Subcommand)]
enum PlanCmd {
    /// Preview restoring every restorable file (or a chosen subset).
    Restore {
        /// Explicit spelling of the default (parity with `restore --all`).
        #[arg(long, conflicts_with_all = ["plan_paths", "plan_exclude"])]
        all: bool,
        #[arg(long)]
        root: Option<PathBuf>,
        #[arg(id = "plan_paths")]
        paths: Vec<String>,
        #[arg(long, id = "plan_exclude")]
        exclude: Vec<String>,
    },
}

/// Parse the full `argv` (the first element is the program name, as clap
/// expects) and run the chosen command. Returns the process exit code.
pub fn run(argv: impl IntoIterator<Item = String>) -> Result<i32> {
    restore_sigpipe();

    let cli = match Cli::try_parse_from(argv) {
        Ok(c) => c,
        Err(e) => {
            use clap::error::ErrorKind;
            return match e.kind() {
                // Help and version are normal output, not errors.
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                    print!("{e}");
                    Ok(0)
                }
                _ => {
                    eprint!("{e}");
                    Ok(2)
                }
            };
        }
    };

    let store = Store::resolve(cli.config_dir);
    // --dry-run is read from the environment so Callers can flip it uniformly.
    let dry_run = std::env::var_os("CHIVE_DRY_RUN").is_some();
    let app = App::new(store, dry_run)?;

    let code = match cli.command {
        Command::Scan { path, ignore, to } => cmd_scan(&app, &path, &ignore, to)?,
        Command::Status {
            restorable,
            unknown,
            disposable,
        } => cmd_status(
            &app,
            if restorable {
                Some(Verdict::Restorable)
            } else if unknown {
                Some(Verdict::Unknown)
            } else if disposable {
                Some(Verdict::Disposable)
            } else {
                None
            },
        )?,
        Command::Holes { limit } => cmd_holes(&app, limit)?,
        Command::Stats => cmd_stats(&app)?,
        Command::Plan { sub } => match sub {
            PlanCmd::Restore {
                all: _,
                root,
                paths,
                exclude,
            } => cmd_plan(&app, root, &paths, &exclude)?,
        },
        Command::Restore {
            all: _,
            root,
            paths,
            exclude,
        } => cmd_restore(&app, root, &paths, &exclude)?,
        Command::Teach { path, method } => cmd_teach(&app, &path, &method)?,
        Command::Dispose { path } => cmd_dispose(&app, &path)?,
        Command::Withdraw { path } => cmd_withdraw(&app, &path)?,
        Command::Clean { dry_run, force } => cmd_clean(&app, dry_run, force)?,
        Command::Import { from } => cmd_import(&app, from)?,
        Command::Export { to } => cmd_export(&app, to)?,
    };
    Ok(code)
}

fn cmd_scan(app: &App, root: &Path, ignore: &[String], to: Option<PathBuf>) -> Result<i32> {
    let catalog = app.scan(root, ignore)?;
    app.save_catalog(&catalog)?;
    // Issue #44: the archive was only as current as the last manual `export`, so
    // every verdict recorded since went unrecorded off-box. `--to` lets the scan
    // write the versionable catalog directly.
    if let Some(dest) = to {
        crate::catalog::toml::write(&catalog, &dest)?;
        println!("wrote catalog to {}", dest.display());
    }
    print_scan_summary(&catalog);
    Ok(0)
}

fn print_scan_summary(c: &Catalog) {
    fn count(c: &Catalog, f: impl Fn(&Verdict) -> bool) -> usize {
        c.files().iter().filter(|e| f(&e.verdict)).count()
    }
    let total = c.files().len();
    let restorable = count(c, |v| *v == Verdict::Restorable);
    let verified = c
        .files()
        .iter()
        .filter(|e| e.source == Some(Source::Verified))
        .count();
    let user = c
        .files()
        .iter()
        .filter(|e| e.source == Some(Source::UserSupplied))
        .count();
    let unknown = count(c, |v| *v == Verdict::Unknown);
    let disposable = count(c, |v| *v == Verdict::Disposable);
    // Break the disposals down by who decided them. Naming only "provably dead"
    // would misreport a rule- or owner-disposed file as something chive proved,
    // which is the exact claim D19 exists to keep honest.
    let disposed_by = |origin: Origin| {
        c.files()
            .iter()
            .filter(|e| e.verdict == Verdict::Disposable && e.verdict_source == origin)
            .count()
    };
    let by_owner = disposed_by(Origin::Owner);
    let by_rule = disposed_by(Origin::Rule);
    let by_chive = disposed_by(Origin::Chive);
    println!("Scanned {total} files:");
    println!("  {restorable:>8} restorable ({verified:>6} verified, {user:>6} user-supplied)");
    println!("  {unknown:>8} unknown (holes)");
    println!(
        "  {disposable:>8} disposable ({by_owner} owner, {by_rule} by rule, {by_chive} provably dead)"
    );
}

/// Human-readable size, so the work list reads in the units the owner cares
/// about rather than raw byte counts.
fn human_bytes(n: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

fn cmd_status(app: &App, filter: Option<Verdict>) -> Result<i32> {
    let catalog = app.load_catalog()?;
    for entry in catalog.files() {
        let wanted = filter.is_none_or(|f| entry.verdict == f);
        if !wanted {
            continue;
        }
        let method = entry.restore_method.as_deref().unwrap_or("-");
        let origin = entry.verdict_source.as_str();
        let category = entry.category.map(|c| c.as_str()).unwrap_or("-");
        let absent = if entry.present {
            ""
        } else {
            "  (not on this machine)"
        };
        println!(
            "{:<11} {:<6} {:<8} {}{absent}",
            entry.verdict, origin, category, entry.path
        );
        if filter.is_none() {
            println!("  method: {method}");
        }
    }
    Ok(0)
}

fn cmd_holes(app: &App, limit: usize) -> Result<i32> {
    // D23: the loop the product exists to support -- see what has no recipe,
    // teach it -- had no verb. Largest first, because the biggest hole is the
    // one worth closing.
    let catalog = app.load_catalog()?;
    let mut holes: Vec<_> = catalog
        .files()
        .iter()
        .filter(|e| e.verdict == Verdict::Unknown)
        .collect();
    holes.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    let total: i64 = holes.iter().map(|e| e.size).sum();
    println!(
        "{} holes ({}) — nothing chive can rebuild yet",
        thousands(holes.len()),
        human_bytes(total)
    );
    println!();
    for e in holes.iter().take(limit) {
        println!(
            "  {:>9}  {:<52} teach a recipe, or dispose",
            human_bytes(e.size),
            e.path
        );
    }
    if holes.len() > limit {
        println!(
            "  ... and {} more (`chive holes --limit N`)",
            thousands(holes.len() - limit)
        );
    }
    Ok(0)
}

/// Thousands separators, so a five-figure hole count is readable at a glance.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn cmd_stats(app: &App) -> Result<i32> {
    let c = app.load_catalog()?;
    let total = c.files().len();
    let restorable = c.files().iter().filter(|e| e.is_restorable()).count() as u64;
    let verified = c
        .files()
        .iter()
        .filter(|e| e.source == Some(Source::Verified))
        .count();
    let user = c
        .files()
        .iter()
        .filter(|e| e.source == Some(Source::UserSupplied))
        .count();
    let unknown = c
        .files()
        .iter()
        .filter(|e| e.verdict == Verdict::Unknown)
        .count();
    let disposable = c
        .files()
        .iter()
        .filter(|e| e.verdict == Verdict::Disposable)
        .count();
    let pct = |n: usize| {
        if total == 0 {
            0.0
        } else {
            100.0 * n as f64 / total as f64
        }
    };
    println!("Catalog: {total} files");
    // D18: the hole count leads, because it is the only number here the owner
    // can act on. Percentages demote below it.
    println!(
        "  holes (unknown):{unknown:>7} ({:>5.1}%)  <- nothing chive can rebuild yet",
        pct(unknown)
    );
    println!(
        "  restorable:     {restorable:>7} ({:>5.1}%)",
        pct(restorable as usize)
    );
    println!(
        "  disposable:     {disposable:>7} ({:>5.1}%)",
        pct(disposable)
    );
    if restorable > 0 {
        let v = 100.0 * verified as f64 / restorable as f64;
        let u = 100.0 * user as f64 / restorable as f64;
        println!(
            "  verified:       {verified:>7} ({:>5.1}% of restorable)",
            v
        );
        println!("  user-supplied:  {user:>7} ({:>5.1}% of restorable)", u);
    }
    Ok(0)
}

fn cmd_plan(app: &App, root: Option<PathBuf>, paths: &[String], exclude: &[String]) -> Result<i32> {
    let catalog = app.load_catalog()?;
    let root = root.unwrap_or_else(|| PathBuf::from(&catalog.root));
    let plan = action::build_plan(&catalog, &root, paths, exclude);
    println!("Plan: {} file(s) to restore", plan.len());
    for item in &plan {
        println!("\n  {}", item.command);
        println!("  -> {}", item.dest.display());
    }
    Ok(0)
}

fn cmd_restore(
    app: &App,
    root: Option<PathBuf>,
    paths: &[String],
    exclude: &[String],
) -> Result<i32> {
    let catalog = app.load_catalog()?;
    let root = root.unwrap_or_else(|| PathBuf::from(&catalog.root));
    let config = Config::load(&app.store.config_file())?;
    let plan = action::build_plan(&catalog, &root, paths, exclude);
    let outcomes = action::restore(
        app.runner() as &dyn crate::runner::Runner,
        &plan,
        config.overwrite(),
    );
    for o in &outcomes {
        match o {
            action::RestoreOutcome::Restored(p) => println!("restored: {p}"),
            action::RestoreOutcome::SkippedExists(p) => println!("skipped (exists): {p}"),
            action::RestoreOutcome::Failed(p, why) => println!("failed:   {p} — {why}"),
        }
    }
    // Every recipe ran (failures do not stop the run), but the exit status
    // must not claim success when anything failed.
    let failed = outcomes
        .iter()
        .any(|o| matches!(o, action::RestoreOutcome::Failed(_, _)));
    Ok(if failed { 1 } else { 0 })
}

/// Record an owner act, refresh the entry it governs, and save.
///
/// The act is the durable part; the entry is the derived view a scan would
/// produce for that path. Recording the act first is what makes `teach` work on
/// a path this machine does not have (issue #45): there is an entry to refresh,
/// but the log entry is what a later scan and a new machine both read.
fn record_act(
    app: &App,
    act: Act,
    apply: impl FnOnce(&mut Catalog, &Act) -> Result<()>,
) -> Result<Catalog> {
    let mut catalog = app.load_catalog()?;
    let recorded = catalog.record(act)?;
    apply(&mut catalog, &recorded)?;
    app.save_catalog(&catalog)?;
    Ok(catalog)
}

fn cmd_teach(app: &App, path: &str, method: &str) -> Result<i32> {
    // The act is the durable part; the entry is the view a scan would produce
    // for that path, and it is refreshed here too — otherwise `teach` followed
    // by `restore` would do nothing until the next scan, which is the gap the
    // old sidecar-file model hid.
    record_act(app, Act::teach(0, path, method), |c, act| {
        match c.by_path(&act.path).cloned() {
            Some(mut e) => {
                e.verdict = Verdict::Restorable;
                e.verdict_source = Origin::Owner;
                e.restore_method = act.method.clone();
                e.source = Some(Source::UserSupplied);
                c.upsert(e)?;
            }
            // Teaching for a path this machine does not have is legitimate —
            // planning a new machine is the core workflow — so the entry is
            // created and marked absent rather than dropped (issue #45).
            None => {
                c.upsert(FileEntry::new_absent_restorable(
                    act.path.clone(),
                    crate::model::classify(&act.path),
                    act.method.clone().unwrap_or_default(),
                ))?;
            }
        }
        Ok(())
    })?;
    println!("taught: {path}");
    println!("  method: {method}");
    Ok(0)
}

fn cmd_dispose(app: &App, path: &str) -> Result<i32> {
    // The only verb that makes a path cleanable.
    record_act(app, Act::dispose(0, path), |c, act| {
        if let Some(entry) = c.by_path(&act.path) {
            let mut e = entry.clone();
            e.verdict = Verdict::Disposable;
            e.verdict_source = Origin::Owner;
            e.restore_method = None;
            e.source = None;
            c.upsert(e)?;
        }
        Ok(())
    })?;
    println!("disposed: {path}");
    Ok(0)
}

fn cmd_withdraw(app: &App, path: &str) -> Result<i32> {
    // Take back the judgement, then let chive re-examine the path: a recipe it
    // can still prove stays, and one it cannot is a hole. This needs no rule of
    // its own -- "recipe present => restorable, absent => unknown" -- but it does
    // need the evidence, which `dispose` cleared when it took the verdict away.
    // So the entry is re-derived here rather than left stale until the next scan.
    let catalog = record_act(app, Act::withdraw(0, path), |c, act| {
        if let Some(entry) = c.by_path(&act.path).cloned() {
            c.upsert(entry)?;
        }
        Ok(())
    })?;
    let catalog = app.rederive(&catalog, path)?;
    let after = catalog
        .by_path(path)
        .map(|e| e.verdict)
        .unwrap_or(Verdict::Unknown);
    app.save_catalog(&catalog)?;
    println!("withdrew: {path} -> {after}");
    Ok(0)
}

fn cmd_clean(app: &App, dry_run: bool, force: bool) -> Result<i32> {
    let mut catalog = app.load_catalog()?;
    let root = PathBuf::from(&catalog.root);
    let (rows, refused) = action::clean_resolved(&catalog, &root);
    if !refused.is_empty() {
        // Refuse rather than skip. A containment failure that silently drops
        // entries, or worse removes something else and exits 0, is the failure
        // mode issue #48 describes (issue #48).
        eprintln!(
            "refused {} path(s) that resolve outside the scan root:",
            refused.len()
        );
        for rel in &refused {
            eprintln!("  {rel}");
        }
        return Err(Error::Refused(format!(
            "{} path(s) resolve outside {}; nothing was removed",
            refused.len(),
            root.display()
        )));
    }
    if dry_run {
        println!("Would remove {} file(s):", rows.len());
        for (rel, _) in rows {
            println!("  {rel}");
        }
        return Ok(0);
    }
    let expected = rows.len();
    if !force && !confirm(&format!("Remove {expected} file(s)? [y/N] ")) {
        println!("nothing removed");
        return Ok(0);
    }
    let (next, removed) =
        action::clean_execute(app.runner() as &dyn crate::runner::Runner, &catalog, &root)?;
    catalog = next;
    app.save_catalog(&catalog)?;
    println!("removed {} file(s)", removed.len());
    // Same honesty rule as restore: a removal that did not happen is a
    // failure the exit status must carry.
    Ok(if removed.len() < expected { 1 } else { 0 })
}

fn confirm(prompt_liter: &str) -> bool {
    use std::io::Write;
    eprint!("{prompt_liter}");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    let read = std::io::stdin()
        .read_line(&mut line)
        .map(|_| line.trim().to_ascii_lowercase());
    matches!(read, Ok(ref s) if s == "y" || s == "yes")
}

fn cmd_import(app: &App, from: Option<PathBuf>) -> Result<i32> {
    let path = from.unwrap_or_else(|| app.store.default_catalog_file());
    let catalog = crate::catalog::toml::read(&path)?;
    // D25: a root outside your home is the owner's judgement call, so it is
    // answered in config rather than assumed. `home-only` (the default) refuses
    // -- a catalog from another machine may name a path you would not want
    // `clean` to delete.
    let config = Config::load(&app.store.config_file())?;
    if let Some(note) = crate::catalog::root_in_scope(&catalog.root, config.root_scope())? {
        eprintln!("note: {note}");
    }
    app.save_catalog(&catalog)?;
    // The act log arrives with the catalog: these are the owner's decisions from
    // the machine the archive came from, and a verdict kept in config would have
    // stayed behind with it (D20).
    println!(
        "imported {} file(s) and {} owner act(s) from {}",
        catalog.files().len(),
        catalog.acts().len(),
        path.display()
    );
    Ok(0)
}

fn cmd_export(app: &App, to: Option<PathBuf>) -> Result<i32> {
    let catalog = app.load_catalog()?;
    let path = to.unwrap_or_else(|| app.store.default_catalog_file());
    crate::catalog::toml::write(&catalog, &path)?;
    println!(
        "exported {} file(s) to {}",
        catalog.files().len(),
        path.display()
    );
    Ok(0)
}
