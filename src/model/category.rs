use std::fmt;

use serde::{Deserialize, Serialize};

/// What a file *is*, decided from its extension.
///
/// A category is flavor only: it never implies restorability. An `.md` file
/// with no recipe is an orphaned document, not something chive can rebuild.
///
/// Storage-wise the category is a string, so the taxonomy can grow without a
/// schema change (see decision D7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Document,
    Image,
    Code,
    Config,
    Program,
    Audio,
    Video,
    Archive,
    Data,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Document => "document",
            Category::Image => "image",
            Category::Code => "code",
            Category::Config => "config",
            Category::Program => "program",
            Category::Audio => "audio",
            Category::Video => "video",
            Category::Archive => "archive",
            Category::Data => "data",
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Category {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, ()> {
        match s {
            "document" => Ok(Category::Document),
            "image" => Ok(Category::Image),
            "code" => Ok(Category::Code),
            "config" => Ok(Category::Config),
            "program" => Ok(Category::Program),
            "audio" => Ok(Category::Audio),
            "video" => Ok(Category::Video),
            "archive" => Ok(Category::Archive),
            "data" => Ok(Category::Data),
            _ => Err(()),
        }
    }
}

/// Classify a path by its (case-insensitive) extension. Returns `None` for
/// dot-files, extensionless files, and unknown extensions.
///
/// The last syllable is the extension, except that a `.tar` penultimate marks a
/// compound archive (`.tar.gz`, `.tar.xz`, `.tar.bz2`, `.tar.zst`), which the
/// last-extension rule would get wrong (decision D8). `.jpeg` aliases `.jpg`.
pub fn classify(path: &str) -> Option<Category> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if name.is_empty() || name.starts_with('.') {
        return None;
    }

    let lower = name.to_ascii_lowercase();
    let syllables: Vec<&str> = lower.split('.').filter(|p| !p.is_empty()).collect();

    match syllables.last().copied()? {
        "gz" | "xz" | "bz2" | "zst" if penultimate_is_tar(&syllables) => Some(Category::Archive),
        ext => classify_single(ext),
    }
}

fn penultimate_is_tar(syllables: &[&str]) -> bool {
    syllables.len() >= 2 && syllables[syllables.len() - 2] == "tar"
}

fn classify_single(ext: &str) -> Option<Category> {
    match ext {
        "jpeg" => Some(Category::Image), // alias for jpg (D8)
        // document
        "pdf" | "doc" | "docx" | "txt" | "md" | "rtf" | "odt" | "xls" | "xlsx" | "ppt" | "pptx"
        | "tex" => Some(Category::Document),
        // image
        "png" | "jpg" | "gif" | "svg" | "webp" | "bmp" | "tiff" | "tif" => Some(Category::Image),
        // code
        "rs" | "py" | "ts" | "js" | "go" | "cpp" | "c" | "h" | "java" | "rb" | "lua" | "sh"
        | "zsh" | "bash" | "el" | "nix" => Some(Category::Code),
        // config
        "toml" | "yaml" | "yml" | "json" | "xml" | "conf" | "ini" | "env" => Some(Category::Config),
        // program
        "deb" | "rpm" | "exe" | "appimage" | "snap" | "dmg" => Some(Category::Program),
        // audio
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" => Some(Category::Audio),
        // video
        "mp4" | "mkv" | "avi" | "webm" | "mov" => Some(Category::Video),
        // archive
        "zip" | "tar" | "gz" | "xz" | "bz2" | "zst" | "rar" | "7z" => Some(Category::Archive),
        // data
        "csv" | "sql" | "db" | "bin" | "parquet" => Some(Category::Data),
        _ => None,
    }
}

#[cfg(test)]
mod category_tests {
    use super::*;

    fn is(path: &str, expected: Category) {
        assert_eq!(classify(path), Some(expected), "for {path:?}");
    }

    #[test]
    fn simple_extensions_map_to_categories() {
        is("Pictures/photo.png", Category::Image);
        is("readme.md", Category::Document);
        is("main.rs", Category::Code);
        is("config.toml", Category::Config);
        is("game.deb", Category::Program);
        is("song.mp3", Category::Audio);
        is("clip.mp4", Category::Video);
        is("bundle.zip", Category::Archive);
        is("table.csv", Category::Data);
    }

    #[test]
    fn jpeg_is_alias_for_jpg() {
        is("a.jpeg", Category::Image);
        is("b.jpg", Category::Image);
    }

    #[test]
    fn compound_tar_suffixes_are_archives() {
        is("node.tar.gz", Category::Archive);
        is("node.tar.xz", Category::Archive);
        is("node.tar.bz2", Category::Archive);
        is("node.tar.zst", Category::Archive);
    }

    #[test]
    fn bare_compression_extensions_are_archives() {
        is("node.gz", Category::Archive);
        is("node.xz", Category::Archive);
        is("node.tar", Category::Archive);
    }

    #[test]
    fn unknown_and_extensionless_are_unclassified() {
        assert_eq!(classify("README"), None);
        assert_eq!(classify(".bashrc"), None);
        assert_eq!(classify("weird.xyzzy"), None);
        assert_eq!(classify("tmp/workfile~"), None);
    }

    #[test]
    fn case_insensitive() {
        is("ReadMe.MD", Category::Document);
        is("PHOTO.PNG", Category::Image);
    }

    #[test]
    fn display_matches_serialized_spelling() {
        for c in [
            Category::Document,
            Category::Image,
            Category::Code,
            Category::Config,
            Category::Program,
            Category::Audio,
            Category::Video,
            Category::Archive,
            Category::Data,
        ] {
            assert_eq!(c.as_str(), c.to_string());
        }
        assert_eq!(Category::Archive.as_str(), "archive");
    }
}
