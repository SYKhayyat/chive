//! `config.toml` — user-editable settings.
//!
//! It holds the ignore list, the scan defaults, and the owner's verdict rules.
//! Unknown keys are a hard error (`deny_unknown_fields`), the same policy Shall
//! uses, so a typo cannot silently change how chive scans — which matters most
//! for a rule, since a mistyped script would quietly change verdicts.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::rules::{Rule, Rules};

/// The directories never scanned. Matching is on the exact directory basename,
/// so `.git` matches any `.git` dir at any depth. This is the default; the
/// file overrides it wholesale.
pub const DEFAULT_IGNORE: &[&str] = &[
    ".git",
    ".svn",
    "node_modules",
    "target",
    "__pycache__",
    ".cache",
    "dist",
    "build",
    ".next",
    ".nuxt",
];

/// What `restore` does when `{dest}` already exists (D24).
///
/// Three levels rather than a bool, in Shall's `ExecTrust` shape: the safe
/// answer is the default, and the escape hatch is named so reading the config
/// says what it is doing. A bool could not name the middle setting, and the
/// default is the whole point — a policy that has to be opted *out* of is not a
/// safety property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Overwrite {
    /// Never write over an existing file. D15 as written.
    #[default]
    Refuse,
    /// Copy the existing file to `<dest>.chive-backup`, then replace it.
    Backup,
    /// Replace without asking. The escape hatch.
    Overwrite,
}

impl Overwrite {
    /// The suffix a displaced file is copied to. Mirrors Shall's
    /// `<target>.shall-backup` (`decisions.md`, II.13).
    pub const BACKUP_SUFFIX: &'static str = ".chive-backup";
}

/// How much an imported catalog's root is trusted (D25).
///
/// The default is `warn`, not `home-only`, and the reason is the product: a
/// catalog written on another machine names *that* machine's root, which by
/// construction is not your home. `home-only` as a default would refuse the
/// migration chive exists for. So the default accepts the root and **names it**,
/// which is the moment you can notice it is wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RootScope {
    /// An imported root must live inside the current user's home.
    HomeOnly,
    /// Accept any absolute root, naming it on import when it is not your home.
    #[default]
    Warn,
    /// Accept anything, silently.
    Any,
}

/// `config.toml`'s two judgement-call surfaces (D24, D25). Both default to the
/// strict answer, and both are three-level so the escape hatch is named rather
/// than implied by a boolean's absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[serde(default)]
    pub restore: Restore,
    #[serde(default)]
    pub catalog: CatalogPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Restore {
    /// What to do about an existing `{dest}`.
    #[serde(default)]
    pub overwrite: Overwrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogPolicy {
    /// How far outside the home an imported root may point.
    #[serde(default)]
    pub root_scope: RootScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Directory basenames to skip during scan.
    #[serde(default = "default_ignore_list")]
    pub ignore: Vec<String>,
    /// The owner's verdict policy (D21). Compiled once per scan, not per file.
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// D24 and D25. One place the two judgement calls live.
    #[serde(default)]
    pub policy: Policy,
}

fn default_ignore_list() -> Vec<String> {
    DEFAULT_IGNORE.iter().map(|s| (*s).to_string()).collect()
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        if !path.exists() {
            // No config yet: defaults apply.
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(path).map_err(Error::Io)?;
        toml::from_str(&text).map_err(|e| Error::parse(path.to_path_buf(), e.to_string()))
    }

    /// Whether a directory with the given basename should be skipped.
    pub fn is_ignored(&self, basename: &str) -> bool {
        self.ignore.iter().any(|i| i == basename)
    }

    /// Compile the configured rules. A script that does not compile is refused
    /// here, at load, rather than half way through a scan — a rule that silently
    /// stops matching would quietly change every verdict after it.
    pub fn compile_rules(&self) -> Result<Rules> {
        Rules::compile(&self.rules)
    }

    /// What `restore` does about an existing `{dest}` (D24).
    pub fn overwrite(&self) -> Overwrite {
        self.policy.restore.overwrite
    }

    /// How far outside the home an imported root may point (D25).
    pub fn root_scope(&self) -> RootScope {
        self.policy.catalog.root_scope
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            ignore: default_ignore_list(),
            rules: Vec::new(),
            policy: Policy::default(),
        }
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    fn tmp(p: &str) -> std::path::PathBuf {
        tempfile::NamedTempFile::new()
            .unwrap()
            .into_temp_path()
            .parent()
            .unwrap()
            .join(p)
    }

    #[test]
    fn missing_config_yields_defaults() {
        let c = Config::load(Path::new("/definitely/absent/config.toml")).unwrap();
        assert!(c.is_ignored(".git"));
        assert!(c.is_ignored("node_modules"));
        assert!(!c.is_ignored("src"));
    }

    #[test]
    fn parses_ignore_list() {
        let path = tmp("chive-config-a.toml");
        std::fs::write(&path, "ignore = [\".git\", \"vendor\"]\n").unwrap();
        let c = Config::load(&path).unwrap();
        assert!(c.is_ignored(".git"));
        assert!(c.is_ignored("vendor"));
        assert!(!c.is_ignored("node_modules"));
    }

    #[test]
    fn empty_file_uses_default_ignore() {
        // `ignore = []` is stored; a totally empty file means "no key" -> default.
        let c = Config::default();
        assert!(c.is_ignored("target"));
    }

    #[test]
    fn unknown_key_is_rejected() {
        let path = tmp("chive-config-unknown.toml");
        std::fs::write(&path, "ignore = []\nwrong_key = 1\n").unwrap();
        let err = Config::load(&path).unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
    }

    #[test]
    fn no_rules_by_default() {
        assert!(Config::default().compile_rules().unwrap().is_empty());
    }

    #[test]
    fn a_rule_is_loaded_and_compiled() {
        let path = tmp("chive-config-rules.toml");
        std::fs::write(
            &path,
            "[[rules]]\nname = \"apk\"\nscript = 'if path.ends_with(\".apk\") { \"disposable\" } else { () }'\n",
        )
        .unwrap();
        let c = Config::load(&path).unwrap();
        assert_eq!(c.rules.len(), 1);
        assert_eq!(c.rules[0].name, "apk");
        assert!(!c.compile_rules().unwrap().is_empty());
    }

    #[test]
    fn both_policies_default_to_the_strict_answer() {
        // The safety properties are D15 (never overwrite) and D25 (an imported
        // root stays inside the home). Neither may require opting out.
        let c = Config::default();
        assert_eq!(c.overwrite(), Overwrite::Refuse);
        // `warn`, not `home-only`: a catalog from another machine names that
        // machine's root, so refusing outside-home by default would break the
        // migration chive exists for (D25).
        assert_eq!(c.root_scope(), RootScope::Warn);
    }

    #[test]
    fn policies_parse_from_toml() {
        let path = tmp("chive-config-policy.toml");
        std::fs::write(
            &path,
            "[policy.restore]\noverwrite = \"backup\"\n\
             [policy.catalog]\nroot_scope = \"any\"\n",
        )
        .unwrap();
        let c = Config::load(&path).unwrap();
        assert_eq!(c.overwrite(), Overwrite::Backup);
        assert_eq!(c.root_scope(), RootScope::Any);
    }

    #[test]
    fn an_unknown_overwrite_level_is_refused_rather_than_defaulted() {
        // `deny_unknown_fields` does not catch a bad enum *value*, so the
        // serde path is what refuses it -- and it must refuse rather than fall
        // back to `refuse`, which would silently ignore the owner's intent.
        let path = tmp("chive-config-bad-level.toml");
        std::fs::write(&path, "[policy.restore]\noverwrite = \"yolo\"\n").unwrap();
        assert!(Config::load(&path).is_err());
    }

    #[test]
    fn a_partial_policy_block_keeps_the_other_default() {
        let path = tmp("chive-config-partial.toml");
        std::fs::write(&path, "[policy.restore]\noverwrite = \"overwrite\"\n").unwrap();
        let c = Config::load(&path).unwrap();
        assert_eq!(c.overwrite(), Overwrite::Overwrite);
        assert_eq!(c.root_scope(), RootScope::Warn);
    }

    #[test]
    fn a_rule_with_a_typo_is_refused_at_load_not_mid_scan() {
        let path = tmp("chive-config-bad-rule.toml");
        std::fs::write(&path, "[[rules]]\nscript = \"if { \"\n").unwrap();
        let c = Config::load(&path).unwrap();
        assert!(
            c.compile_rules().is_err(),
            "a broken script must fail loudly, not silently stop matching"
        );
    }
}
