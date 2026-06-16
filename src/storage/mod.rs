//! Persistence: loading the problem bank and exporting solved problems into
//! the local Markdown vault.

pub mod problem;
pub mod progress;
pub mod vault;

use std::path::PathBuf;

use anyhow::Context;

/// Environment variable that overrides the default vault location.
pub const VAULT_ENV_VAR: &str = "MAMBA_VAULT";

/// Environment variable that overrides the problem-bank location.
pub const PROBLEMS_ENV_VAR: &str = "MAMBA_PROBLEMS";

/// Resolves the directory holding the JSON problem bank, so the installed
/// binary works from any working directory.
///
/// Resolution order:
/// 1. `$MAMBA_PROBLEMS`, taken verbatim, if set and non-empty.
/// 2. `./problems`, if it exists — the development-checkout case.
/// 3. The platform data dir, e.g. `~/.local/share/mamba/problems` on Linux —
///    the installed-binary case.
pub fn default_problems_path() -> anyhow::Result<PathBuf> {
    if let Ok(custom) = std::env::var(PROBLEMS_ENV_VAR)
        && !custom.trim().is_empty()
    {
        return Ok(PathBuf::from(custom));
    }
    let local = PathBuf::from("problems");
    if local.is_dir() {
        return Ok(local);
    }
    let data = dirs::data_dir().context("could not determine the platform data directory")?;
    Ok(data.join("mamba").join("problems"))
}

/// Resolves the Markdown vault directory that solved problems are exported
/// into.
///
/// Resolution order:
/// 1. `$MAMBA_VAULT`, taken verbatim, if set and non-empty.
/// 2. `~/OnyxVault/Mamba` — a dedicated subfolder so Mamba never clutters the
///    root of the user's vault (and sits beside `~/OnyxVault/Kata`).
///
/// The directory is *not* created here; [`vault::export`] creates it lazily
/// on the first successful solve.
pub fn default_vault_path() -> anyhow::Result<PathBuf> {
    if let Ok(custom) = std::env::var(VAULT_ENV_VAR)
        && !custom.trim().is_empty()
    {
        return Ok(PathBuf::from(custom));
    }
    let home = dirs::home_dir().context("could not determine the home directory")?;
    Ok(home.join("OnyxVault").join("Mamba"))
}

/// Resolves the on-disk cache of problem embeddings for semantic search, e.g.
/// `~/.local/share/mamba/embeddings.json` on Linux. The cache is recomputed
/// lazily when the bank text or embedding model changes; deleting it is safe.
pub fn default_embeddings_path() -> anyhow::Result<PathBuf> {
    let data = dirs::data_dir().context("could not determine the platform data directory")?;
    Ok(data.join("mamba").join("embeddings.json"))
}

/// Resolves the append-only FSRS review-event log, e.g.
/// `~/.local/share/mamba/reviews.jsonl`. One line per graded review (solve or
/// lapse); the vault remains the source of truth, this is the corpus `mamba
/// optimize` fits the scheduler weights to. Deleting it is safe.
pub fn default_reviews_path() -> anyhow::Result<PathBuf> {
    let data = dirs::data_dir().context("could not determine the platform data directory")?;
    Ok(data.join("mamba").join("reviews.jsonl"))
}
