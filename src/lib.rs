//! chive — a reconstruction engine.
//!
//! chive scans a machine, decides what matters, and records a recipe for each
//! meaningful file. On a new or damaged machine it re-derives what is missing
//! by running those recipes. It never touches a file that already exists.
//!
//! The library is organised into small focused modules:
//!
//! - [`model`] — the domain types (status, source, category, entry).
//! - [`error`] — typed errors mapped to process exit codes.
//! - [`runner`] — the one seam through which external commands run.
//! - [`catalog`] — the catalog: TOML as truth, SQLite as a derived index.
//! - [`config`] — user-editable settings (ignore list, defaults).
//! - [`store`] — where chive keeps its state.
//! - [`provenance`] — how chive infers a recipe for a file.

pub mod catalog;
pub mod config;
pub mod error;
pub mod model;
pub mod provenance;
pub mod runner;
pub mod store;
