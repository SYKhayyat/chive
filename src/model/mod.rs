pub mod category;
pub mod entry;
pub mod source;
pub mod verdict;

pub use category::{Category, classify};
pub use entry::FileEntry;
pub use source::Source;
pub use verdict::{Origin, Verdict};
