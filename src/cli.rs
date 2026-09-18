//! The command line: parsing and the thin dispatch to the operations in `app`
//! and `action`. Keeping parsing and effect separate means every command's body
//! is a plain function over [`App`], testable without a terminal.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::action::{self, CleanScope};
use crate::app::App;
use crate::catalog::Catalog;
use crate::error::{Error, Result};
use crate::model::{Source, Status};
use crate::recipes::Recipes;
use crate::store::Store;

/// chive — a reconstruction engine. Keep a recipe for every meaningful file, and
/// re-derive what is missing on a new or damaged machine.
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
    },
    /// List catalog entries, optionally filtered by status.
    Status {
        /// Show only restorable entries.
        #[arg(long, conflicts_with_all = ["not_restorable", "temporary_status", "orphaned"])]
        restorable: bool,
        /// Show only not-restorable entries.
        #[arg(long = "not-restorable", id = "not_restorable", conflicts_with_all = ["restorable", "temporary_status", "orphaned"])]
        not_restorable: bool,
        /// Show only temporary entries.
        #[arg(long, id = "temporary_status", conflicts_with_all = ["restorable", "not_restorable", "orphaned"])]
        temporary: bool,
        /// Show only orphaned entries.
        #[arg(long, conflicts_with_all = ["restorable", "not_restorable", "temporary_status"])]
        orphaned: bool,
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
    /// Teach a recipe for a file, making it restorable (overrules inference).
    Teach {
        /// The relative path to teach.
        path: String,
        /// The shell recipe. `{dest}` expands at restore time.
        #[arg(long)]
        method: String,
    },
    /// Mark a file's status (not-restorable, temporary, or orphaned).
    Mark {
        /// The relative path to mark.
        path: String,
        #[arg(long, value_enum)]
        status: MarkStatus,
    },
    /// Remove temporary and/or orphaned files from disk.
    Clean {
        /// Which cleanable statuses to remove.
        #[arg(long, value_enum, default_value_t = CleanArg::Both)]
        scope: CleanArg,
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

/// The target status for `mark`.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum MarkStatus {
    NotRestorable,
    Temporary,
    Orphaned,
}

/// The clean scope flag (reuses `action::CleanScope` semantics).
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum CleanArg {
    Temporary,
    Orphaned,
    Both,
}

impl From<CleanArg> for CleanScope {
    fn from(v: CleanArg) -> Self {
        match v {
            CleanArg::Temporary => CleanScope::Temporary,
            CleanArg::Orphaned => CleanScope::Orphaned,
            CleanArg::Both => CleanScope::Both,
        }
    }
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
        Command::Scan { path, ignore } => cmd_scan(&app, &path, &ignore)?,
        Command::Status {
            restorable,
            not_restorable,
            temporary,
            orphaned,
        } => cmd_status(
            &app,
            if restorable {
                Some(Status::Restorable)
            } else if not_restorable {
                Some(Status::NotRestorable)
            } else if temporary {
                Some(Status::Temporary)
            } else if orphaned {
                Some(Status::Orphaned)
            } else {
                None
            },
        )?,
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
        Command::Mark { path, status } => cmd_mark(&app, &path, status)?,
        Command::Clean {
            scope,
            dry_run,
            force,
        } => cmd_clean(&app, scope.into(), dry_run, force)?,
        Command::Import { from } => cmd_import(&app, from)?,
        Command::Export { to } => cmd_export(&app, to)?,
    };
    Ok(code)
}

fn cmd_scan(app: &App, root: &Path, ignore: &[String]) -> Result<i32> {
    let catalog = app.scan(root, ignore)?;
    app.save_catalog(&catalog)?;
    print_scan_summary(&catalog);
    Ok(0)
}

fn print_scan_summary(c: &Catalog) {
    fn count(c: &Catalog, f: impl Fn(&Status) -> bool) -> usize {
        c.files().iter().filter(|e| f(&e.status)).count()
    }
    let total = c.files().len();
    let restorable = count(c, |s| *s == Status::Restorable);
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
    let not_restorable = count(c, |s| *s == Status::NotRestorable);
    let temporary = count(c, |s| *s == Status::Temporary);
    let orphaned = count(c, |s| *s == Status::Orphaned);
    println!("Scanned {total} files:");
    println!("  {restorable:>8} restorable ({verified:>6} verified, {user:>6} user-supplied)");
    println!("  {not_restorable:>8} not-restorable");
    println!("  {temporary:>8} temporary");
    println!("  {orphaned:>8} orphaned");
}

fn cmd_status(app: &App, filter: Option<Status>) -> Result<i32> {
    let catalog = app.load_catalog()?;
    for entry in catalog.files() {
        let wanted = filter.is_none_or(|f| entry.status == f);
        if !wanted {
            continue;
        }
        let method = entry.restore_method.as_deref().unwrap_or("-");
        let source = entry.source.map(|s| s.as_str()).unwrap_or("-");
        let category = entry.category.map(|c| c.as_str()).unwrap_or("-");
        println!(
            "{:<15} {:<8} {:<8} {}",
            entry.status, source, category, entry.path
        );
        if filter.is_none() {
            println!("  method: {method}");
        }
    }
    Ok(0)
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
    let not_restorable = c
        .files()
        .iter()
        .filter(|e| e.status == Status::NotRestorable)
        .count();
    let temporary = c
        .files()
        .iter()
        .filter(|e| e.status == Status::Temporary)
        .count();
    let orphaned = c
        .files()
        .iter()
        .filter(|e| e.status == Status::Orphaned)
        .count();
    let pct = |n: usize| {
        if total == 0 {
            0.0
        } else {
            100.0 * n as f64 / total as f64
        }
    };
    println!("Catalog: {total} files");
    println!(
        "  restorable:     {restorable:>7} ({:>5.1}%)",
        pct(restorable as usize)
    );
    println!(
        "  not-restorable: {not_restorable:>7} ({:>5.1}%)",
        pct(not_restorable)
    );
    println!(
        "  temporary:      {temporary:>7} ({:>5.1}%)",
        pct(temporary)
    );
    println!("  orphaned:       {orphaned:>7} ({:>5.1}%)", pct(orphaned));
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
        match &item.dest {
            Some(d) => println!("  -> {}", d.display()),
            None => println!("  -> (placed by the recipe / package manager)"),
        }
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
    let plan = action::build_plan(&catalog, &root, paths, exclude);
    let outcomes = action::restore(app.runner() as &dyn crate::runner::Runner, &plan);
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

fn cmd_teach(app: &App, path: &str, method: &str) -> Result<i32> {
    // Teaching extends the recipes file, then we persist the catalog with the
    // taught entry promoted to restorable (user_supplied), overruling inference.
    let mut recipes = Recipes::load(&app.store.recipes_file())?;
    recipes.teach(path, method, &app.store.recipes_file())?;

    // Rebuild the catalog so this file is now restorable.
    let mut catalog = app.load_catalog()?;
    match catalog.by_path(path).cloned() {
        Some(mut entry) => {
            entry.status = Status::Restorable;
            entry.restore_method = Some(method.to_string());
            entry.source = Some(Source::UserSupplied);
            catalog.upsert(entry)?;
        }
        None => {
            // A taught recipe for a file not present on this machine should not
            // invent a file; recipes.toml alone holds it. Nothing to change.
        }
    }
    app.save_catalog(&catalog)?;
    println!("taught: {path}");
    println!("  method: {method}");
    Ok(0)
}

fn cmd_mark(app: &App, path: &str, status: MarkStatus) -> Result<i32> {
    let mut catalog = app.load_catalog()?;
    let target = match status {
        MarkStatus::NotRestorable => Status::NotRestorable,
        MarkStatus::Temporary => Status::Temporary,
        MarkStatus::Orphaned => Status::Orphaned,
    };
    let mut entry = catalog
        .by_path(path)
        .cloned()
        .ok_or_else(|| Error::Catalog(format!("no such file in catalog: {path}")))?;
    entry.status = target;
    // A mark clears any previous restore recipe; the entry is no longer restorable.
    entry.restore_method = None;
    entry.source = None;
    catalog.upsert(entry)?;
    app.save_catalog(&catalog)?;
    println!("marked: {path} → {target}");
    Ok(0)
}

fn cmd_clean(app: &App, scope: CleanScope, dry_run: bool, force: bool) -> Result<i32> {
    let mut catalog = app.load_catalog()?;
    let root = PathBuf::from(&catalog.root);
    if dry_run {
        let rows = action::clean_preview(&catalog, &root, scope);
        println!("Would remove {} file(s):", rows.len());
        for (rel, _) in rows {
            println!("  {rel}");
        }
        return Ok(0);
    }
    let expected = action::clean_preview(&catalog, &root, scope).len();
    if !force && !confirm(&format!("Remove {expected} file(s)? [y/N] ")) {
        println!("nothing removed");
        return Ok(0);
    }
    let (next, removed) = action::clean_execute(
        app.runner() as &dyn crate::runner::Runner,
        &catalog,
        &root,
        scope,
    )?;
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
    app.save_catalog(&catalog)?;
    println!(
        "imported {} file(s) from {}",
        catalog.files().len(),
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
