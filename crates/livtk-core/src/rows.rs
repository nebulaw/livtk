//! A model2vec model read rather than loaded, for the prompt hook.
//!
//! Loading the library's model reads and converts the whole embedding matrix (128 MB for
//! `potion-retrieval-32M`) and parses its 1.5 MB tokenizer: 80 ms, on every prompt, to embed a
//! prompt that needs about fifteen rows. This reads only the rows a text needs, from
//! `model.safetensors`, and tokenizes with its own WordPiece over a sorted vocabulary cached
//! beside the model (`livtk-vocab.txt`), rebuilt whenever `tokenizer.json` changes.
//!
//! It covers the tokenizers model2vec's potion models ship: BERT's normalizer and
//! pre-tokenizer, WordPiece, special tokens matched as written, a single float32 `embeddings`
//! tensor. Any other model is left to the library ([`Rows::open`] says `None`). Within what it
//! covers it reproduces the library step by step, to the bit: the same Unicode tables (the same
//! crates, pinned), the same truncation, the same sums in the same order. Tests hold both the
//! tokens and the vectors to the library's.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::Value;
use unicode_categories::UnicodeCategories;
use unicode_normalization_alignments::UnicodeNormalization;

/// The library embeds at most this many tokens of a text.
const MAX_TOKENS: usize = 512;
/// The cache's format; a different one is rebuilt.
const CACHE_VERSION: u32 = 1;
const CACHE_NAME: &str = "livtk-vocab.txt";

/// A model read from its directory: the tokenizer, and where its rows lie.
pub struct Rows {
    vocab: Vocab,
    matrix: Matrix,
}

impl Rows {
    /// The model in `dir`, when livtk can read it; `None` leaves it to the library (another
    /// tokenizer, a quantized matrix, a file it cannot make sense of).
    pub fn open(dir: &Path) -> Option<Rows> {
        let matrix = Matrix::open(&dir.join("model.safetensors"))?;
        let vocab = Vocab::open(dir)?;
        Some(Rows { vocab, matrix })
    }

    /// The token ids the library embeds for `text`: tokenized, unknown tokens dropped, at most
    /// 512.
    pub fn token_ids(&self, text: &str) -> Vec<u32> {
        let v = &self.vocab;
        let text = truncate(text, MAX_TOKENS.saturating_mul(v.median));
        let mut ids = v.tokenize(text);
        ids.retain(|&id| id != v.unk);
        ids.truncate(MAX_TOKENS);
        ids
    }

    /// Each text's vector: the mean of its tokens' rows, normalized, as the library computes it.
    pub fn embed(&self, texts: &[String]) -> std::io::Result<Vec<Vec<f32>>> {
        let ids: Vec<Vec<u32>> = texts.iter().map(|t| self.token_ids(t)).collect();
        let rows = self.matrix.fetch(ids.iter().flatten().copied())?;
        let cols = self.matrix.cols;
        Ok(ids
            .iter()
            .map(|ids| {
                let mut sum = vec![0.0_f32; cols];
                for &id in ids {
                    // The library scales each row by a weight of 1.0 when the model has none,
                    // which changes no bit.
                    for (s, &v) in sum.iter_mut().zip(rows.row(id)) {
                        *s += v;
                    }
                }
                let denom = ids.len().max(1) as f32;
                for x in &mut sum {
                    *x /= denom;
                }
                let norm = sum.iter().map(|&v| v * v).sum::<f32>().sqrt().max(1e-12);
                for x in &mut sum {
                    *x /= norm;
                }
                sum
            })
            .collect())
    }
}

/// The first `chars` characters of `text`, as the library cuts a text before tokenizing it.
fn truncate(text: &str, chars: usize) -> &str {
    text.char_indices()
        .nth(chars)
        .map_or(text, |(at, _)| &text[..at])
}

// ---- the matrix ---------------------------------------------------------------------------------

/// `model.safetensors`: where its one float32 `embeddings` tensor starts, and its shape.
struct Matrix {
    file: File,
    start: u64,
    rows: usize,
    cols: usize,
}

/// The rows one batch of texts needs.
enum Fetched {
    /// Read one by one, by token id.
    Some(HashMap<u32, Vec<f32>>),
    /// The whole matrix, read in one go.
    All { data: Vec<f32>, cols: usize },
}

impl Fetched {
    fn row(&self, id: u32) -> &[f32] {
        match self {
            Fetched::Some(rows) => rows.get(&id).map_or(&[], Vec::as_slice),
            Fetched::All { data, cols } => {
                let at = id as usize * cols;
                data.get(at..at + cols).unwrap_or(&[])
            }
        }
    }
}

impl Matrix {
    fn open(path: &Path) -> Option<Matrix> {
        let mut file = File::open(path).ok()?;
        let mut len = [0u8; 8];
        file.read_exact(&mut len).ok()?;
        let len = usize::try_from(u64::from_le_bytes(len)).ok()?;
        if len > 1 << 20 {
            return None;
        }
        let mut header = vec![0u8; len];
        file.read_exact(&mut header).ok()?;
        let header: Value = serde_json::from_slice(&header).ok()?;
        let tensors: Vec<&String> = header
            .as_object()?
            .keys()
            .filter(|k| *k != "__metadata__")
            .collect();
        // Weights or a token mapping (a quantized model) are the library's to apply.
        if tensors != ["embeddings"] {
            return None;
        }
        let e = &header["embeddings"];
        if e["dtype"] != "F32" {
            return None;
        }
        let shape = e["shape"].as_array()?;
        let [rows, cols] = shape.as_slice() else {
            return None;
        };
        let (rows, cols) = (
            usize::try_from(rows.as_u64()?).ok()?,
            usize::try_from(cols.as_u64()?).ok()?,
        );
        let offsets = e["data_offsets"].as_array()?;
        let (from, to) = (offsets.first()?.as_u64()?, offsets.get(1)?.as_u64()?);
        if to - from != (rows * cols * 4) as u64 {
            return None;
        }
        Some(Matrix {
            file,
            start: 8 + len as u64 + from,
            rows,
            cols,
        })
    }

    /// The rows of `ids`, each read once. A batch that needs a large part of the matrix (the
    /// first search of a big project) reads it whole, as the library would.
    fn fetch(&self, ids: impl Iterator<Item = u32>) -> std::io::Result<Fetched> {
        let mut want: Vec<u32> = ids.collect();
        want.sort_unstable();
        want.dedup();
        if let Some(&bad) = want.iter().find(|&&id| id as usize >= self.rows) {
            return Err(std::io::Error::other(format!(
                "token {bad} past the model's {} rows",
                self.rows
            )));
        }
        let row_bytes = self.cols * 4;
        let mut file = &self.file;
        if want.len() > self.rows / 8 {
            let mut all = vec![0u8; self.rows * row_bytes];
            file.seek(SeekFrom::Start(self.start))?;
            file.read_exact(&mut all)?;
            return Ok(Fetched::All {
                data: floats(&all),
                cols: self.cols,
            });
        }
        let mut rows = HashMap::with_capacity(want.len());
        let mut bytes = vec![0u8; row_bytes];
        for id in want {
            file.seek(SeekFrom::Start(
                self.start + u64::from(id) * row_bytes as u64,
            ))?;
            file.read_exact(&mut bytes)?;
            rows.insert(id, floats(&bytes));
        }
        Ok(Fetched::Some(rows))
    }
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    // As the library decodes them.
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| f32::from_le_bytes(b))
        .collect()
}

// ---- the tokenizer ------------------------------------------------------------------------------

/// BERT's normalizer, as configured in `tokenizer.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Normalizer {
    clean_text: bool,
    chinese_chars: bool,
    strip_accents: bool,
    lowercase: bool,
}

/// A WordPiece tokenizer over a vocabulary sorted by token, looked up by binary search: loading
/// it is reading a file and indexing its lines, with no table to build.
struct Vocab {
    text: String,
    /// Each token's span in `text` and its id, sorted by token.
    entries: Vec<(u32, u32, u32)>,
    unk: u32,
    prefix: String,
    max_chars: usize,
    /// The median token length in bytes, by which the library cuts long texts.
    median: usize,
    normalizer: Normalizer,
    /// Special tokens, matched in the text as written.
    added: Vec<(String, u32)>,
}

impl Vocab {
    /// The cached vocabulary beside the model, made again from `tokenizer.json` when that
    /// changed. A tokenizer this cannot reproduce gives `None`, and the cache remembers it.
    fn open(dir: &Path) -> Option<Vocab> {
        let source = dir.join("tokenizer.json");
        let stamp = stamp(&source)?;
        let cache = dir.join(CACHE_NAME);
        if let Ok(text) = fs::read_to_string(&cache)
            && let Some(cached) = Cached::parse(text, &stamp)
        {
            return cached.0;
        }
        let made = make_cache(&source, &stamp);
        // A models directory that cannot be written costs speed, not correctness.
        let tmp = cache.with_extension("txt.tmp");
        if fs::write(&tmp, &made).is_ok() {
            let _ = fs::rename(&tmp, &cache);
        }
        Cached::parse(made, &stamp)?.0
    }

    fn get(&self, token: &str) -> Option<u32> {
        self.entries
            .binary_search_by(|&(s, e, _)| self.text[s as usize..e as usize].cmp(token))
            .ok()
            .map(|i| self.entries[i].2)
    }

    /// The ids of `text`: special tokens as written, the rest normalized, split into words
    /// and pieced by WordPiece.
    fn tokenize(&self, text: &str) -> Vec<u32> {
        let mut out = Vec::new();
        let mut rest = text;
        while !rest.is_empty() {
            match self.next_added(rest) {
                Some((at, len, id)) => {
                    self.tokenize_plain(&rest[..at], &mut out);
                    out.push(id);
                    rest = &rest[at + len..];
                }
                None => {
                    self.tokenize_plain(rest, &mut out);
                    break;
                }
            }
        }
        out
    }

    /// The leftmost special token in `text`, the longest where several start there: where it
    /// starts, how long it is, its id.
    fn next_added(&self, text: &str) -> Option<(usize, usize, u32)> {
        if self.added.is_empty() {
            return None;
        }
        for (at, _) in text.char_indices() {
            let here = &text[at..];
            let best = self
                .added
                .iter()
                .filter(|(t, _)| here.starts_with(t.as_str()))
                .max_by_key(|(t, _)| t.len());
            if let Some((t, id)) = best {
                return Some((at, t.len(), *id));
            }
        }
        None
    }

    fn tokenize_plain(&self, text: &str, out: &mut Vec<u32>) {
        if text.is_empty() {
            return;
        }
        let normal = self.normalizer.apply(text);
        for word in words(&normal) {
            self.wordpiece(word, out);
        }
    }

    /// One word in pieces, longest first; a word with no way through is unknown.
    fn wordpiece(&self, word: &str, out: &mut Vec<u32>) {
        if word.chars().count() > self.max_chars {
            out.push(self.unk);
            return;
        }
        let mark = out.len();
        let mut start = 0;
        while start < word.len() {
            let mut end = word.len();
            let mut found = None;
            while start < end {
                let piece = &word[start..end];
                let id = if start > 0 {
                    self.get(&format!("{}{piece}", self.prefix))
                } else {
                    self.get(piece)
                };
                if let Some(id) = id {
                    found = Some(id);
                    break;
                }
                end -= piece.chars().last().map_or(1, char::len_utf8);
            }
            match found {
                Some(id) => {
                    out.push(id);
                    start = end;
                }
                None => {
                    out.truncate(mark);
                    out.push(self.unk);
                    return;
                }
            }
        }
    }
}

impl Normalizer {
    /// The text as BERT's normalizer leaves it, step by step as it runs: clean, space out
    /// Chinese characters, strip accents, lowercase.
    fn apply(&self, text: &str) -> String {
        let mut s: String = if self.clean_text {
            text.chars()
                .filter(|&c| !(c == '\0' || c == '\u{fffd}' || is_control(c)))
                .map(|c| if is_whitespace(c) { ' ' } else { c })
                .collect()
        } else {
            text.to_owned()
        };
        if self.chinese_chars {
            let mut spaced = String::with_capacity(s.len());
            for c in s.chars() {
                if is_chinese_char(c) {
                    spaced.push(' ');
                    spaced.push(c);
                    spaced.push(' ');
                } else {
                    spaced.push(c);
                }
            }
            s = spaced;
        }
        if self.strip_accents {
            s = s
                .as_str()
                .nfd()
                .map(|(c, _)| c)
                .filter(|c| !c.is_mark_nonspacing())
                .collect();
        }
        if self.lowercase {
            s = s.chars().flat_map(char::to_lowercase).collect();
        }
        s
    }
}

/// BERT's pre-tokenizer: words split at whitespace, each punctuation mark a word of its own.
fn words(text: &str) -> impl Iterator<Item = &str> {
    let mut out = Vec::new();
    let mut start = None;
    for (at, c) in text.char_indices() {
        if c.is_whitespace() || is_bert_punc(c) {
            if let Some(s) = start.take() {
                out.push(&text[s..at]);
            }
            if !c.is_whitespace() {
                out.push(&text[at..at + c.len_utf8()]);
            }
        } else if start.is_none() {
            start = Some(at);
        }
    }
    if let Some(s) = start {
        out.push(&text[s..]);
    }
    out.into_iter()
}

fn is_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r') || c.is_whitespace()
}

fn is_control(c: char) -> bool {
    !matches!(c, '\t' | '\n' | '\r') && c.is_other()
}

fn is_chinese_char(c: char) -> bool {
    matches!(
        c as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B920..=0x2CEAF
            | 0xF900..=0xFAFF
            | 0x2F800..=0x2FA1F
    )
}

fn is_bert_punc(c: char) -> bool {
    c.is_ascii_punctuation() || c.is_punctuation()
}

// ---- the cache ----------------------------------------------------------------------------------

/// What identifies a `tokenizer.json`: its length and modification time.
fn stamp(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!("{} {}", meta.len(), mtime.as_nanos()))
}

/// A cache file, read: the vocabulary, or `None` when it records a tokenizer livtk does not
/// reproduce.
struct Cached(Option<Vocab>);

impl Cached {
    /// The cache's text, if it is this version's and made from the tokenizer as `stamp` finds
    /// it now.
    ///
    /// ```text
    /// livtk-vocab 1
    /// source <len> <mtime>
    /// normalizer <clean> <chinese> <strip> <lower>     (each 0 or 1)
    /// wordpiece <unk id> <max chars> <prefix>
    /// median <bytes>
    /// added <n>
    /// <id>\t<token>                                    (n lines)
    /// vocab <n>
    /// <token>\t<id>                                    (n lines, sorted by token)
    /// ```
    ///
    /// or, for a tokenizer left to the library, `unsupported <why>` after the source line.
    fn parse(text: String, stamp: &str) -> Option<Cached> {
        let mut at = 0;
        // The next line and where it starts, moving past it.
        let mut line = || -> Option<(usize, &str)> {
            let rest = text.get(at..)?;
            let len = rest.find('\n')?;
            let start = at;
            at += len + 1;
            Some((start, &rest[..len]))
        };
        if line()?.1 != format!("livtk-vocab {CACHE_VERSION}") {
            return None;
        }
        if line()?.1 != format!("source {stamp}") {
            return None;
        }
        let first = line()?.1;
        if first.starts_with("unsupported") {
            return Some(Cached(None));
        }
        let flags: Vec<bool> = first
            .strip_prefix("normalizer ")?
            .split(' ')
            .map(|f| f == "1")
            .collect();
        let [clean_text, chinese_chars, strip_accents, lowercase] = flags[..] else {
            return None;
        };
        let mut wp = line()?.1.strip_prefix("wordpiece ")?.splitn(3, ' ');
        let unk: u32 = wp.next()?.parse().ok()?;
        let max_chars: usize = wp.next()?.parse().ok()?;
        let prefix = wp.next()?.to_owned();
        let median: usize = line()?.1.strip_prefix("median ")?.parse().ok()?;
        let n: usize = line()?.1.strip_prefix("added ")?.parse().ok()?;
        let mut added = Vec::with_capacity(n);
        for _ in 0..n {
            let (id, token) = line()?.1.split_once('\t')?;
            added.push((token.to_owned(), id.parse().ok()?));
        }
        let n: usize = line()?.1.strip_prefix("vocab ")?.parse().ok()?;
        // Each token is looked up where it lies in the text: only its span is kept.
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            let (start, l) = line()?;
            let (token, id) = l.split_once('\t')?;
            let s = u32::try_from(start).ok()?;
            let e = s + u32::try_from(token.len()).ok()?;
            entries.push((s, e, id.parse().ok()?));
        }
        Some(Cached(Some(Vocab {
            text,
            entries,
            unk,
            prefix,
            max_chars,
            median,
            normalizer: Normalizer {
                clean_text,
                chinese_chars,
                strip_accents,
                lowercase,
            },
            added,
        })))
    }
}

/// The cache's text for the tokenizer at `source`, or a line saying why livtk leaves it to the
/// library.
fn make_cache(source: &Path, stamp: &str) -> String {
    let head = format!("livtk-vocab {CACHE_VERSION}\nsource {stamp}\n");
    match describe(source) {
        Ok(body) => head + &body,
        Err(why) => format!("{head}unsupported {why}\n"),
    }
}

fn describe(source: &Path) -> Result<String, String> {
    let text = fs::read_to_string(source).map_err(|e| e.to_string())?;
    let json: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    for setting in ["truncation", "padding"] {
        if !json[setting].is_null() {
            return Err(format!("{setting} is set"));
        }
    }
    if json["pre_tokenizer"]["type"] != "BertPreTokenizer" {
        return Err("not BERT's pre-tokenizer".into());
    }
    let n = &json["normalizer"];
    if n["type"] != "BertNormalizer" {
        return Err("not BERT's normalizer".into());
    }
    let flag = |k: &str| n[k].as_bool();
    let lowercase = flag("lowercase").ok_or("no lowercase setting")?;
    let normalizer = Normalizer {
        clean_text: flag("clean_text").ok_or("no clean_text setting")?,
        chinese_chars: flag("handle_chinese_chars").ok_or("no handle_chinese_chars setting")?,
        strip_accents: flag("strip_accents").unwrap_or(lowercase),
        lowercase,
    };
    let m = &json["model"];
    if m["type"] != "WordPiece" {
        return Err("not WordPiece".into());
    }
    let vocab: BTreeMap<&str, u32> = m["vocab"]
        .as_object()
        .ok_or("no vocabulary")?
        .iter()
        .map(|(t, id)| Some((t.as_str(), u32::try_from(id.as_u64()?).ok()?)))
        .collect::<Option<_>>()
        .ok_or("a vocabulary id is not a number")?;
    let unk_token = m["unk_token"].as_str().ok_or("no unknown token")?;
    let unk = *vocab
        .get(unk_token)
        .ok_or("the unknown token is not in the vocabulary")?;
    let prefix = m["continuing_subword_prefix"]
        .as_str()
        .ok_or("no subword prefix")?;
    let max_chars = m["max_input_chars_per_word"]
        .as_u64()
        .ok_or("no word length limit")?;
    let mut added = Vec::new();
    for a in json["added_tokens"].as_array().into_iter().flatten() {
        let plain = a["normalized"] == false
            && a["single_word"] == false
            && a["lstrip"] == false
            && a["rstrip"] == false;
        if !plain {
            return Err("a special token is matched other than as written".into());
        }
        let content = a["content"].as_str().ok_or("a special token has no text")?;
        let id = a["id"].as_u64().ok_or("a special token has no id")?;
        added.push((id, content));
    }
    let lines = vocab.keys().chain(added.iter().map(|(_, t)| t));
    if lines
        .into_iter()
        .any(|t| t.contains(['\t', '\n']) || t.is_empty())
    {
        return Err("a token holds a tab or a line break".into());
    }
    if prefix.contains('\n') {
        return Err("the subword prefix holds a line break".into());
    }
    let mut lens: Vec<usize> = vocab.keys().map(|t| t.len()).collect();
    lens.sort_unstable();
    let median = lens.get(lens.len() / 2).copied().unwrap_or(1);
    let bit = |b: bool| if b { "1" } else { "0" };
    let mut out = format!(
        "normalizer {} {} {} {}\nwordpiece {unk} {max_chars} {prefix}\nmedian {median}\nadded {}\n",
        bit(normalizer.clean_text),
        bit(normalizer.chinese_chars),
        bit(normalizer.strip_accents),
        bit(normalizer.lowercase),
        added.len()
    );
    for (id, t) in &added {
        out.push_str(&format!("{id}\t{t}\n"));
    }
    out.push_str(&format!("vocab {}\n", vocab.len()));
    for (t, id) in &vocab {
        out.push_str(&format!("{t}\t{id}\n"));
    }
    Ok(out)
}

/// Where the cache of the model in `dir` lives.
pub fn cache_path(dir: &Path) -> PathBuf {
    dir.join(CACHE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bert_normalizing_cleans_spaces_out_strips_and_lowers() {
        let n = Normalizer {
            clean_text: true,
            chinese_chars: true,
            strip_accents: true,
            lowercase: true,
        };
        assert_eq!(n.apply("Café\tNAÏVE\u{0}x\u{fffd}"), "cafe naivex");
        assert_eq!(n.apply("中文"), " 中  文 ");
    }

    #[test]
    fn words_split_at_spaces_and_around_punctuation() {
        let w: Vec<&str> = words("hey friend!   how are you?!?").collect();
        assert_eq!(
            w,
            ["hey", "friend", "!", "how", "are", "you", "?", "!", "?"]
        );
    }

    #[test]
    fn a_truncated_text_keeps_its_first_characters() {
        assert_eq!(truncate("héllo", 2), "hé");
        assert_eq!(truncate("hi", 10), "hi");
    }
}
