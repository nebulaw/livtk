//! Reading a model's rows instead of loading it: the same tokens and the same vectors as the
//! library, to the bit, for every model in `LIVTK_TEST_MODELS` livtk reads itself.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic
#![cfg(feature = "embed")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use livtk_core::search::{Embed, Model};

const MODELS: [&str; 2] = ["potion-retrieval-32M", "potion-base-8M"];

/// The models present in `LIVTK_TEST_MODELS`; the retrieval model must be.
fn models() -> (PathBuf, Vec<&'static str>) {
    let dir = PathBuf::from(std::env::var("LIVTK_TEST_MODELS").expect(
        "set LIVTK_TEST_MODELS to a directory holding potion-retrieval-32M (see `livtk embed setup`)",
    ));
    let present: Vec<&str> = MODELS
        .into_iter()
        .filter(|m| dir.join(m).join("model.safetensors").is_file())
        .collect();
    assert!(
        present.contains(&MODELS[0]),
        "no model in {}",
        dir.display()
    );
    (dir, present)
}

/// Texts that try the tokenizer: real prose and code, and the corners of Unicode it handles.
fn corpus() -> Vec<String> {
    let mut texts: Vec<String> = [
        "",
        " ",
        "is the task done before cargo check passes? never report it done otherwise",
        "Café, naïve, résumé, Ångström, Øre, Œuvre, façade, jalapeño",
        "İstanbul DİKKAT ıI straße STRASSE ΣΊΣΥΦΟΣ ὈΔΥΣΣΕΎΣ",
        "中文和日本語のテキスト、한국어 텍스트 합니다",
        "emoji 😀👍🏽 and flags 🇬🇪 and zwj 👩‍💻",
        "e\u{301} a\u{308} combining marks, \u{200b}zero\u{200d}width\u{feff}",
        "tabs\tand\nnewlines\r\nand\u{a0}nbsp\u{2003}em space",
        "control \u{1}\u{7}\u{1b}[0m and \u{0} nul and \u{fffd} replacement",
        "[CLS] start [SEP] middle [MASK]end[PAD][UNK] and [cls] lower",
        "##ing ## # prefix-like ##",
        "punctuation: !\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~ ¡¿ «» — – … ‘’ “” 、。",
        "https://example.com/path?q=1&r=two#frag user@mail.org",
        "fn main() { let x: Vec<u32> = (0..10).map(|i| i * 2).collect(); }",
        "numbers 3.14159 1,000,000 2026-10-16 0x1F 1e-12 ½ ² ①",
        "عربي مع حركات: مُحَمَّد ، עברית ، हिन्दी में लिखा ، ไทย",
        "ﬁ ligature ｆｕｌｌｗｉｄｔｈ ㎏ Ⅻ",
        &"a".repeat(101),
        &"word ".repeat(400),
        &"ε".repeat(150),
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for doc in [
        include_str!("../../../README.md"),
        include_str!("../../../docs/plan.md"),
    ] {
        texts.extend(doc.lines().map(str::to_owned));
        texts.push(doc.to_owned());
    }
    texts
}

fn library_ids(tok: &tokenizers::Tokenizer, text: &str) -> Vec<u32> {
    let unk = tok.token_to_id("[UNK]");
    let mut ids: Vec<u32> = tok
        .encode_fast(text, false)
        .unwrap()
        .get_ids()
        .iter()
        .copied()
        .filter(|&i| Some(i) != unk)
        .collect();
    ids.truncate(512);
    ids
}

#[test]
#[ignore = "needs the model: LIVTK_TEST_MODELS=<dir> cargo test -- --ignored"]
fn with_model_the_tokens_are_the_librarys() {
    let (dir, present) = models();
    for name in present {
        let model = Model::load(&dir, name).unwrap();
        assert!(model.reads_rows(), "{name}: livtk reads it itself");
        let tok = tokenizers::Tokenizer::from_file(dir.join(name).join("tokenizer.json")).unwrap();
        let mut checked = 0;
        for text in corpus() {
            // Long texts are cut before tokenizing, by a length the vectors test covers.
            if text.chars().count() > 3000 {
                continue;
            }
            assert_eq!(
                model.token_ids(&text).unwrap(),
                library_ids(&tok, &text),
                "{name}: {text:?}"
            );
            checked += 1;
        }
        assert!(checked > 500, "{checked} texts");
    }
}

#[test]
#[ignore = "needs the model: LIVTK_TEST_MODELS=<dir> cargo test -- --ignored"]
fn with_model_the_vectors_are_the_librarys_to_the_bit() {
    let (dir, present) = models();
    for name in present {
        let read = Model::load(&dir, name).unwrap();
        let whole = Model::load_whole_named(&dir, name).unwrap();
        assert!(read.reads_rows() && !whole.reads_rows());
        let texts = corpus();
        assert_eq!(
            read.embed(&texts).unwrap(),
            whole.embed(&texts).unwrap(),
            "{name}"
        );
        // One at a time, as the prompt hook embeds.
        for t in texts.iter().take(40) {
            let one = std::slice::from_ref(t);
            assert_eq!(
                read.embed(one).unwrap(),
                whole.embed(one).unwrap(),
                "{name}: {t:?}"
            );
        }
        // A batch that needs much of the matrix reads it whole: every token as a text.
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join(name).join("tokenizer.json")).unwrap(),
        )
        .unwrap();
        let vocab: Vec<String> = json["model"]["vocab"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            read.embed(&vocab).unwrap(),
            whole.embed(&vocab).unwrap(),
            "{name}: the whole vocabulary"
        );
    }
}

/// A copy of the retrieval model in `to`, its matrix linked, its tokenizer as given.
fn copy_model(dir: &Path, to: &Path, tokenizer: &str) {
    let from = dir.join(MODELS[0]);
    std::fs::create_dir_all(to).unwrap();
    std::fs::hard_link(from.join("model.safetensors"), to.join("model.safetensors"))
        .or_else(|_| {
            std::fs::copy(from.join("model.safetensors"), to.join("model.safetensors")).map(drop)
        })
        .unwrap();
    std::fs::copy(from.join("config.json"), to.join("config.json")).unwrap();
    std::fs::write(to.join("tokenizer.json"), tokenizer).unwrap();
}

#[test]
#[ignore = "needs the model: LIVTK_TEST_MODELS=<dir> cargo test -- --ignored"]
fn with_model_the_vocabulary_is_cached_and_follows_its_tokenizer() {
    let (dir, _) = models();
    let tmp = tempfile::tempdir().unwrap();
    let original = std::fs::read_to_string(dir.join(MODELS[0]).join("tokenizer.json")).unwrap();
    copy_model(&dir, &tmp.path().join("m"), &original);
    let cache = livtk_core::rows::cache_path(&tmp.path().join("m"));
    let model = Model::load(tmp.path(), "m").unwrap();
    assert!(model.reads_rows());
    assert!(cache.is_file(), "the first open writes the cache");
    let first = std::fs::read_to_string(&cache).unwrap();
    assert!(first.starts_with("livtk-vocab 1\n"));
    Model::load(tmp.path(), "m").unwrap();
    assert_eq!(std::fs::read_to_string(&cache).unwrap(), first, "reused");

    // A tokenizer livtk does not reproduce goes to the library, and the cache says so.
    let other = original.replacen("\"BertPreTokenizer\"", "\"Whitespace\"", 1);
    std::thread::sleep(Duration::from_millis(10));
    std::fs::write(tmp.path().join("m/tokenizer.json"), other).unwrap();
    let model = Model::load(tmp.path(), "m").unwrap();
    assert!(!model.reads_rows(), "left to the library");
    assert!(
        std::fs::read_to_string(&cache)
            .unwrap()
            .contains("unsupported not BERT's pre-tokenizer"),
        "remembered"
    );

    // Put back, it is read again.
    std::thread::sleep(Duration::from_millis(10));
    std::fs::write(tmp.path().join("m/tokenizer.json"), &original).unwrap();
    assert!(Model::load(tmp.path(), "m").unwrap().reads_rows());

    // A cache that cannot be written costs nothing but speed.
    std::fs::remove_file(&cache).unwrap();
    std::fs::create_dir(&cache).unwrap();
    assert!(Model::load(tmp.path(), "m").unwrap().reads_rows());
}

/// `LIVTK_TEST_MODELS=<dir> cargo test --release -p livtk-core --test rows -- --ignored --nocapture`
#[test]
#[ignore = "needs the model: LIVTK_TEST_MODELS=<dir>"]
fn with_model_a_prompt_is_embedded_in_milliseconds() {
    let (dir, _) = models();
    Model::load(&dir, MODELS[0]).unwrap(); // the cache, made once
    let prompt =
        vec!["is the task done before cargo check passes? never say done otherwise".to_owned()];
    let times: Vec<(Duration, Duration)> = (0..10)
        .map(|_| {
            let t = Instant::now();
            let m = Model::load(&dir, MODELS[0]).unwrap();
            let open = t.elapsed();
            m.embed(&prompt).unwrap();
            (open, t.elapsed())
        })
        .collect();
    let (open, total) = times.iter().min_by_key(|t| t.1).unwrap();
    let t = Instant::now();
    let whole = Model::load_whole_named(&dir, MODELS[0]).unwrap();
    whole.embed(&prompt).unwrap();
    let library = t.elapsed();
    eprintln!(
        "a prompt: open {open:?}, open and embed {total:?}; the library loading it {library:?}"
    );
    assert!(*total < Duration::from_millis(10), "took {total:?}");
}
