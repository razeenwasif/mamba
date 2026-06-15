//! Optional user configuration at `~/.config/mamba/config.toml`.
//!
//! Every setting Mamba reads is also an environment variable (`MAMBA_*`); the
//! config file simply provides persistent defaults so they don't have to be
//! exported each shell. Precedence is **environment > config file > built-in
//! default**: [`apply`] sets an env var from the config *only when it isn't
//! already set*, after which the existing `default_*` resolvers pick the value
//! up unchanged — so no other module needs to know the config file exists.
//!
//! Call [`apply`] once at the very start of `main`, before any threads spawn
//! (setting environment variables is only sound single-threaded).
//!
//! ```toml
//! # ~/.config/mamba/config.toml — every key is optional
//! vault        = "~/Notes/Mamba"
//! theme        = "midnight"
//! ollama_model = "gemma4:12b-it-qat"
//! embed_model  = "nomic-embed-text"
//! ```

use std::path::PathBuf;

use serde::Deserialize;

/// The parsed config file. Every field is optional; a missing key leaves the
/// corresponding environment variable (and thus the built-in default) untouched.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Markdown export vault (`MAMBA_VAULT`).
    pub vault: Option<String>,
    /// Problem-bank directory (`MAMBA_PROBLEMS`).
    pub problems: Option<String>,
    /// Color theme (`MAMBA_THEME`): `venom` · `midnight` · `slate`.
    pub theme: Option<String>,
    /// Tutor model tag (`MAMBA_OLLAMA_MODEL`).
    pub ollama_model: Option<String>,
    /// Ollama server URL (`MAMBA_OLLAMA_URL`).
    pub ollama_url: Option<String>,
    /// Semantic-search embedding model (`MAMBA_EMBED_MODEL`).
    pub embed_model: Option<String>,
}

/// The config file path, `~/.config/mamba/config.toml` on Linux.
pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("mamba").join("config.toml"))
}

/// Reads and parses the config file, or `None` if it's absent. A malformed file
/// is reported to stderr and ignored rather than aborting startup.
pub fn load() -> Option<Config> {
    let path = path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    match toml::from_str(&text) {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            eprintln!("mamba: ignoring malformed {}: {e}", path.display());
            None
        }
    }
}

/// The `(env var, value)` pairs a config implies — every field that is set to a
/// non-empty string. Pure (no env or filesystem access) so it's unit-testable.
fn env_pairs(cfg: &Config) -> Vec<(&'static str, String)> {
    [
        ("MAMBA_VAULT", &cfg.vault),
        ("MAMBA_PROBLEMS", &cfg.problems),
        ("MAMBA_THEME", &cfg.theme),
        ("MAMBA_OLLAMA_MODEL", &cfg.ollama_model),
        ("MAMBA_OLLAMA_URL", &cfg.ollama_url),
        ("MAMBA_EMBED_MODEL", &cfg.embed_model),
    ]
    .into_iter()
    .filter_map(|(key, value)| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| (key, v.to_string()))
    })
    .collect()
}

/// Applies the config file to the environment: for each configured setting whose
/// `MAMBA_*` variable is unset, exports it so the existing resolvers see it.
/// Environment variables already set by the user always win.
///
/// Must be called before any threads are spawned (see the module docs).
pub fn apply() {
    let Some(cfg) = load() else {
        return;
    };
    for (key, value) in env_pairs(&cfg) {
        let already_set = std::env::var(key).is_ok_and(|v| !v.trim().is_empty());
        if !already_set {
            // SAFETY: called at the very top of `main`, before any thread is
            // spawned, so there is no concurrent access to the environment.
            unsafe {
                std::env::set_var(key, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_pairs_includes_only_non_empty_fields() {
        let cfg = Config {
            vault: Some("~/Notes/Mamba".into()),
            problems: None,
            theme: Some("  ".into()), // whitespace-only → dropped
            ollama_model: Some("gemma4:12b-it-qat".into()),
            ollama_url: None,
            embed_model: None,
        };
        let pairs = env_pairs(&cfg);
        assert!(pairs.contains(&("MAMBA_VAULT", "~/Notes/Mamba".to_string())));
        assert!(pairs.contains(&("MAMBA_OLLAMA_MODEL", "gemma4:12b-it-qat".to_string())));
        // Unset and whitespace-only fields contribute nothing.
        assert!(pairs.iter().all(|(k, _)| *k != "MAMBA_THEME"));
        assert!(pairs.iter().all(|(k, _)| *k != "MAMBA_PROBLEMS"));
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn an_empty_config_implies_no_env() {
        assert!(env_pairs(&Config::default()).is_empty());
    }

    #[test]
    fn parses_a_partial_toml_file() {
        let cfg: Config = toml::from_str("theme = \"midnight\"\nvault = \"/tmp/v\"\n").unwrap();
        assert_eq!(cfg.theme.as_deref(), Some("midnight"));
        assert_eq!(cfg.vault.as_deref(), Some("/tmp/v"));
        assert!(cfg.ollama_model.is_none());
    }
}
