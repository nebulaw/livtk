//! Search: keywords (SQLite FTS5) and meaning (a local model2vec model), fused by reciprocal
//! rank fusion. Without the model, search is keywords only and nothing else changes.

use std::collections::HashMap;
#[cfg(feature = "embed")]
use std::path::Path;
use std::path::PathBuf;

use serde::Serialize;
use uuid::Uuid;

#[cfg(feature = "embed")]
use crate::Error;
use crate::Result;
use crate::model::{Kind, State};

/// The model used when the config names none: tuned for retrieval, 512 dimensions.
pub const DEFAULT_MODEL: &str = "potion-retrieval-32M";
/// The cosine at which a memory counts as relevant to a prompt by meaning alone. Measured on a
/// fixture of 12 rules and 16 prompts (2026-10-16): with `potion-retrieval-32M` true matches
/// scored 0.26 to 0.44 and unrelated prompts at most 0.17; with `potion-base-8M` true matches
/// scored 0.29 to 0.62 and unrelated prompts up to 0.29.
pub fn cosine_bar(model: &str) -> f32 {
    match model {
        "potion-base-8M" => 0.30,
        _ => 0.25,
    }
}

/// Candidates taken from each ranking before fusion.
pub const CANDIDATES: usize = 20;
/// The usual constant for reciprocal rank fusion (Cormack, Clarke and Buettcher, 2009).
const RRF_K: f64 = 60.0;
/// Words too common to say anything about relevance.
const STOP: [&str; 64] = [
    "a", "about", "after", "again", "all", "also", "an", "and", "any", "are", "as", "at", "be",
    "been", "but", "by", "can", "could", "do", "does", "for", "from", "get", "had", "has", "have",
    "how", "i", "if", "in", "into", "is", "it", "its", "just", "let", "like", "make", "me", "my",
    "need", "no", "not", "now", "of", "on", "or", "our", "please", "should", "so", "some", "that",
    "the", "then", "there", "this", "to", "us", "was", "we", "what", "with", "you",
];

/// One search result.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hit {
    pub id: Uuid,
    pub key: String,
    pub kind: Kind,
    pub state: State,
    pub title: String,
    /// Fused rank score; higher is better. Comparable only within one search.
    pub score: f64,
    /// Cosine similarity to the query, when the model ranked it.
    pub cosine: Option<f32>,
    /// How many distinct query words the node contains.
    pub terms: usize,
}

/// Turns text into vectors. The model behind it is loaded once per process.
pub trait Embed {
    /// The model's name; part of each stored vector's hash, so a new model re-embeds.
    fn name(&self) -> &str;
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// Where downloaded models live: `$LIVTK_MODELS_DIR`, else `$XDG_CACHE_HOME/livtk/models`,
/// else `~/.cache/livtk/models`.
pub fn models_dir() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    env("LIVTK_MODELS_DIR")
        .or_else(|| env("XDG_CACHE_HOME").map(|d| d.join("livtk/models")))
        .or_else(|| env("HOME").map(|d| d.join(".cache/livtk/models")))
}

/// A model2vec model loaded from a local directory.
#[cfg(feature = "embed")]
pub struct Model {
    name: String,
    inner: model2vec_rs::model::StaticModel,
}

#[cfg(feature = "embed")]
impl Model {
    /// Load `name` from `dir/name`. Fails when the files are missing.
    pub fn load(dir: &Path, name: &str) -> Result<Model> {
        let path = dir.join(name);
        if !path.join("model.safetensors").is_file() {
            return Err(Error::Invalid(format!(
                "no model at {}; run `livtk embed setup`",
                path.display()
            )));
        }
        let inner =
            model2vec_rs::model::StaticModel::from_pretrained(&path, None, Some(true), None)
                .map_err(|e| {
                    Error::Invalid(format!("cannot load the model at {}: {e}", path.display()))
                })?;
        Ok(Model {
            name: name.to_owned(),
            inner,
        })
    }
}

#[cfg(feature = "embed")]
impl Embed for Model {
    fn name(&self) -> &str {
        &self.name
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // The library panics if its tokenizer fails; a hook must never crash on that.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.inner.encode(texts)))
            .map_err(|_| Error::Invalid("the embedding model failed on this text".into()))
    }
}

/// The distinct, meaningful words of a query, lowercased, in order.
pub fn query_terms(query: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in words(query) {
        if word.len() >= 2 && !STOP.contains(&word.as_str()) && !out.contains(&word) {
            out.push(word);
        }
        if out.len() == 32 {
            break;
        }
    }
    out
}

/// An FTS5 query matching any of the terms. Each term is quoted, so no FTS syntax leaks through.
pub fn fts_query(terms: &[String]) -> Option<String> {
    (!terms.is_empty()).then(|| {
        terms
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(" OR ")
    })
}

/// As [`fts_query`], for a query still being typed: its last term, the word under the cursor,
/// also matches as a prefix (`"resi"*` finds "resize").
pub fn fts_query_typing(terms: &[String]) -> Option<String> {
    let (last, rest) = terms.split_last()?;
    let mut parts: Vec<String> = rest.iter().map(|t| format!("\"{t}\"")).collect();
    parts.push(format!("\"{last}\"*"));
    Some(parts.join(" OR "))
}

/// How many of `terms` appear in `text`, comparing rough word stems.
pub fn matched_terms(terms: &[String], text: &str) -> usize {
    let stems: Vec<String> = words(text).map(|w| stem(&w)).collect();
    terms.iter().filter(|t| stems.contains(&stem(t))).count()
}

/// Reciprocal rank fusion of ranked id lists: best first, ties by id.
pub fn fuse(lists: &[&[Uuid]]) -> Vec<(Uuid, f64)> {
    let mut scores: HashMap<Uuid, f64> = HashMap::new();
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            *scores.entry(*id).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
        }
    }
    let mut fused: Vec<(Uuid, f64)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    fused
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
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

/// The hash a stored vector is keyed by: the model and the exact text it was made from.
pub fn text_hash(model: &str, text: &str) -> String {
    let mut bytes = Vec::with_capacity(model.len() + text.len() + 1);
    bytes.extend_from_slice(model.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(text.as_bytes());
    format!("{:016x}", crate::log::fnv1a(&bytes))
}

fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
}

/// A rough English stem, enough to match "checks", "checked" and "checking" to "check", or
/// "messages" to "message" and "pushes" to "push".
fn stem(word: &str) -> String {
    let long = |b: &str| b.chars().count() >= 3;
    if let Some(b) = word.strip_suffix("ing").filter(|b| long(b)) {
        return b.to_owned();
    }
    if let Some(b) = word.strip_suffix("ed").filter(|b| long(b)) {
        return b.to_owned();
    }
    if let Some(b) = word
        .strip_suffix("es")
        .filter(|b| long(b) && ["s", "x", "z", "ch", "sh"].iter().any(|e| b.ends_with(e)))
    {
        return b.to_owned();
    }
    if let Some(b) = word
        .strip_suffix('s')
        .filter(|b| long(b) && !b.ends_with('s'))
    {
        return b.to_owned();
    }
    word.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_drop_noise_and_keep_order() {
        assert_eq!(
            query_terms("Please make sure the Compositor never drops a frame, the compositor!"),
            ["sure", "compositor", "never", "drops", "frame"]
        );
        assert!(query_terms("a the of").is_empty());
    }

    #[test]
    fn fts_queries_quote_every_term() {
        let terms = query_terms("NEAR(x) \"quoted\" OR -minus");
        let q = fts_query(&terms).unwrap();
        assert_eq!(q, "\"near\" OR \"quoted\" OR \"minus\"");
        assert_eq!(fts_query(&[]), None);
    }

    #[test]
    fn stems_join_common_word_forms() {
        for (a, b) in [
            ("checks", "check"),
            ("checked", "check"),
            ("checking", "check"),
            ("messages", "message"),
            ("pushes", "push"),
            ("passes", "pass"),
            ("tests", "test"),
            ("uses", "use"),
        ] {
            assert_eq!(stem(a), stem(b), "{a} / {b}");
        }
        assert_eq!(stem("class"), "class", "a double s is not a plural");
    }

    #[test]
    fn stems_match_simple_word_forms() {
        let terms = query_terms("checking reports");
        assert_eq!(
            matched_terms(&terms, "Always run the checks before you report"),
            2
        );
        assert_eq!(matched_terms(&terms, "nothing related here"), 0);
    }

    #[test]
    fn fusion_rewards_agreement() {
        let (a, b, c) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let fused = fuse(&[&[a, b], &[b, c]]);
        assert_eq!(fused[0].0, b, "in both lists");
        assert_eq!(fused.len(), 3);
    }

    #[test]
    fn cosine_handles_degenerate_vectors() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine(&[1.0], &[1.0, 0.0]), 0.0);
    }

    #[test]
    fn hashes_change_with_model_and_text() {
        let h = text_hash("m", "t");
        assert_ne!(h, text_hash("n", "t"));
        assert_ne!(h, text_hash("m", "u"));
        assert_eq!(h, text_hash("m", "t"));
    }
}
