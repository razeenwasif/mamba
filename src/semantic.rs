//! Semantic problem search via local text embeddings.
//!
//! The fuzzy finder ([`crate::app`]'s `/`) matches *characters*; this matches
//! *meaning*. Each problem is embedded once with a local [Ollama] embedding
//! model (default `nomic-embed-text`), the vectors are cached to disk, and a
//! natural-language query is ranked against them by cosine similarity — so
//! "rate of change of a curve" finds the derivative problems even with no shared
//! words.
//!
//! Everything runs against the same local Ollama server as the tutor
//! ([`crate::agent`]); no data leaves the machine. The search runs on a worker
//! thread and reports back over an [`mpsc`] channel — the same shape as the
//! verification and tutor workers — so indexing the bank never blocks the UI.
//!
//! `nomic-embed-text` is trained with task prefixes; we prepend
//! `search_document:` to each problem and `search_query:` to the query, which
//! materially improves retrieval.
//!
//! [Ollama]: https://ollama.com

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use serde::{Deserialize, Serialize};

/// The embedding model tag, from `MAMBA_EMBED_MODEL` or the default. Must be an
/// embedding model pulled in Ollama (`ollama pull nomic-embed-text`).
pub fn default_model() -> String {
    std::env::var("MAMBA_EMBED_MODEL").unwrap_or_else(|_| "nomic-embed-text".to_string())
}

/// One problem's cached embedding, tagged with a hash of the text it was built
/// from so a stale entry (the problem's prompt changed) is recomputed.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    hash: u64,
    vec: Vec<f32>,
}

/// The persisted embedding cache: every problem's vector, keyed by problem id,
/// plus the model they were built with (so switching models rebuilds cleanly).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Store {
    model: String,
    entries: HashMap<String, Entry>,
}

impl Store {
    fn load(path: &PathBuf, model: &str) -> Store {
        let parsed = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Store>(&s).ok());
        match parsed {
            // A different model invalidates every vector — start fresh.
            Some(store) if store.model == model => store,
            _ => Store {
                model: model.to_string(),
                entries: HashMap::new(),
            },
        }
    }

    fn save(&self, path: &PathBuf) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

/// What a problem looks like to the indexer: a stable id plus the text to embed.
#[derive(Debug, Clone)]
pub struct Doc {
    pub id: String,
    pub text: String,
}

/// Progress and results streamed from a running search.
#[derive(Debug)]
pub enum SearchEvent {
    /// Building the index: `done` of `total` problems embedded so far.
    Indexing { done: usize, total: usize },
    /// Final ranking: problem indices (into the `docs` slice) paired with their
    /// cosine similarity, best first. `store` is `Some` only when the index was
    /// (re)built, so the caller can keep it in memory for the next query.
    Ranked {
        store: Option<Store>,
        results: Vec<(usize, f32)>,
    },
    /// The search failed (server down, model not pulled, …).
    Error(String),
}

/// FNV-1a hash of the embed text, used to detect when a cached vector is stale.
/// Hand-rolled so it's stable across runs (unlike the default hasher's keys).
fn text_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Cosine similarity of two equal-length vectors; `0.0` if either is degenerate.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// Calls Ollama's embeddings endpoint for one text, returning the vector.
fn embed(url: &str, model: &str, text: &str) -> anyhow::Result<Vec<f32>> {
    let endpoint = format!("{}/api/embeddings", url.trim_end_matches('/'));
    let body = serde_json::json!({ "model": model, "prompt": text });
    let resp: serde_json::Value = ureq::post(&endpoint)
        .send_json(body)
        .map_err(|e| {
            anyhow::anyhow!(
                "Ollama embeddings request failed: {e}. Is `ollama serve` running and \
                 is the embedding model pulled (`ollama pull {model}`)?"
            )
        })?
        .into_json()?;
    let vec: Vec<f32> = resp
        .get("embedding")
        .and_then(|e| e.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect()
        })
        .unwrap_or_default();
    if vec.is_empty() {
        anyhow::bail!("Ollama returned an empty embedding for model '{model}'");
    }
    Ok(vec)
}

/// Ranks `docs` against `query` by semantic similarity on a worker thread,
/// returning the receiving end of its progress/result channel.
///
/// `existing` is the in-memory index from a previous query, if any: when present
/// the bank is not re-embedded (only the query is), so repeat searches are fast.
/// On a cold start the worker builds the index — reusing `cache_path` for any
/// unchanged problems and embedding only the new/changed ones — emits
/// [`SearchEvent::Indexing`] progress, and returns the rebuilt [`Store`].
pub fn search(
    url: String,
    model: String,
    cache_path: PathBuf,
    docs: Vec<Doc>,
    existing: Option<Store>,
    query: String,
) -> Receiver<SearchEvent> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        // 1. Build (or reuse) the index of problem vectors.
        let (store, rebuilt) = match existing {
            Some(store) => (store, false),
            None => {
                let mut store = Store::load(&cache_path, &model);
                let total = docs.len();
                let mut changed = false;
                for (done, doc) in docs.iter().enumerate() {
                    let hash = text_hash(&doc.text);
                    let fresh = store
                        .entries
                        .get(&doc.id)
                        .is_some_and(|e| e.hash == hash && !e.vec.is_empty());
                    if !fresh {
                        match embed(&url, &model, &format!("search_document: {}", doc.text)) {
                            Ok(vec) => {
                                store.entries.insert(doc.id.clone(), Entry { hash, vec });
                                changed = true;
                            }
                            Err(e) => {
                                let _ = tx.send(SearchEvent::Error(e.to_string()));
                                return;
                            }
                        }
                        let _ = tx.send(SearchEvent::Indexing {
                            done: done + 1,
                            total,
                        });
                    }
                }
                if changed {
                    store.save(&cache_path);
                }
                (store, true)
            }
        };

        // 2. Embed the query and rank every problem against it.
        let query_vec = match embed(&url, &model, &format!("search_query: {query}")) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(SearchEvent::Error(e.to_string()));
                return;
            }
        };
        let mut results: Vec<(usize, f32)> = docs
            .iter()
            .enumerate()
            .filter_map(|(i, doc)| {
                store
                    .entries
                    .get(&doc.id)
                    .map(|e| (i, cosine(&query_vec, &e.vec)))
            })
            .collect();
        // Best (highest similarity) first; stable tiebreak by index.
        results.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });

        let _ = tx.send(SearchEvent::Ranked {
            store: rebuilt.then_some(store),
            results,
        });
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_is_one_for_identical_and_zero_for_orthogonal() {
        let a = [1.0, 2.0, 3.0];
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        // Mismatched lengths and zero vectors are defined as 0.
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn text_hash_is_stable_and_discriminating() {
        assert_eq!(
            text_hash("derivative of x^2"),
            text_hash("derivative of x^2")
        );
        assert_ne!(
            text_hash("derivative of x^2"),
            text_hash("derivative of x^3")
        );
    }

    /// Hits a *live* Ollama server, so it's ignored by default. Run with:
    /// `cargo test -- --ignored ranks_relevant`.
    #[test]
    #[ignore = "requires a running ollama server with nomic-embed-text pulled"]
    fn ranks_relevant_docs_first_live() {
        use std::time::Duration;

        let docs = vec![
            Doc {
                id: "deriv".into(),
                text: "Derivative — Calculus\nFind d/dx of x^2".into(),
            },
            Doc {
                id: "prime".into(),
                text: "Primality test — Number Theory\nIs 97 a prime number?".into(),
            },
            Doc {
                id: "det".into(),
                text: "Determinant — Linear Algebra\nCompute the determinant of a 2x2 matrix"
                    .into(),
            },
        ];
        let cache = std::env::temp_dir().join(format!("mamba-embed-live-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&cache);

        let rx = search(
            crate::agent::default_url(),
            default_model(),
            cache.clone(),
            docs,
            None,
            "the instantaneous rate of change of a function".into(),
        );
        let mut results = None;
        while let Ok(ev) = rx.recv_timeout(Duration::from_secs(120)) {
            match ev {
                SearchEvent::Ranked { results: r, .. } => {
                    results = Some(r);
                    break;
                }
                SearchEvent::Error(e) => panic!("semantic search error: {e}"),
                SearchEvent::Indexing { .. } => {}
            }
        }
        let results = results.expect("a ranked result");
        eprintln!("ranked: {results:?}");
        assert_eq!(
            results[0].0, 0,
            "the derivative doc should rank first for a rate-of-change query"
        );
        let _ = std::fs::remove_file(&cache);
    }

    #[test]
    fn a_different_model_invalidates_the_cache() {
        let path = std::env::temp_dir().join(format!("mamba-embed-{}.json", std::process::id()));
        let mut store = Store {
            model: "old-model".into(),
            entries: HashMap::new(),
        };
        store.entries.insert(
            "p1".into(),
            Entry {
                hash: 1,
                vec: vec![0.1, 0.2],
            },
        );
        store.save(&path);

        // Same model → loads the cached entry; different model → empty.
        assert_eq!(Store::load(&path, "old-model").entries.len(), 1);
        assert_eq!(Store::load(&path, "new-model").entries.len(), 0);
        let _ = std::fs::remove_file(&path);
    }
}
