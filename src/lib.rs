//! chive — the `home.nix` you never wrote.
//!
//! chive scans a machine, decides what matters, and records a recipe for each
//! meaningful file. On a new or damaged machine it re-derives what is missing
//! by running those recipes. It never touches a file that already exists.
//!
//! The archive is the product; deleting is a side benefit (D18).
//!
//! The library is organised into small focused modules:
//!
//! - [`model`] — the domain types (verdict, origin, source, category, entry).
//! - [`act`] — the ordered owner-act log, the durable record of a decision.
//! - [`error`] — typed errors mapped to process exit codes.
//! - [`runner`] — the one seam through which external commands run.
//! - [`catalog`] — the catalog: TOML as truth, SQLite as a derived index.
//! - [`config`] — user-editable settings (ignore list, verdict rules).
//! - [`rules`] — the owner's verdict policy, as sandboxed Rhai scripts.
//! - [`store`] — where chive keeps its state.
//! - [`provenance`] — how chive infers a recipe for a file.

pub mod act;
pub mod action;
pub mod app;
pub mod catalog;
pub mod cli;
pub mod config;
pub mod error;
pub mod model;
pub mod provenance;
pub mod rules;
pub mod runner;
pub mod scan;
pub mod store;
