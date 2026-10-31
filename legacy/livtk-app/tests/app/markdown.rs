//! Descriptions and notes are markdown, and their code blocks are coloured by language: each
//! language the app promises has its grammar compiled in and actually colours code.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot set up should panic

use gpui_kit::component::Rope;
use gpui_kit::component::highlighter::{HighlightTheme, LanguageRegistry, SyntaxHighlighter};

/// A fence's language, and a line of it.
const SAMPLES: &[(&str, &str)] = &[
    ("rust", "fn main() { let x = 1; }"),
    ("toml", "[package]\nname = \"livtk\""),
    ("json", "{\"key\": [1, true, null]}"),
    ("yaml", "key: [1, true]"),
    ("bash", "for f in *; do echo \"$f\"; done"),
    ("javascript", "const x = () => 1;"),
    ("typescript", "let x: number = 1;"),
    ("tsx", "const a = <div>{1}</div>;"),
    ("python", "def f(x):\n    return 1"),
    ("go", "func main() { x := 1 }"),
    ("diff", "--- a\n+++ b\n-old\n+new"),
    ("html", "<p class=\"x\">hi</p>"),
    ("css", "p { color: red; }"),
    ("markdown", "# Title\n\n- item"),
];

#[test]
fn every_promised_language_colours_its_code() {
    let theme = HighlightTheme::default_dark();
    for (lang, code) in SAMPLES {
        assert!(
            LanguageRegistry::singleton().language(lang).is_some(),
            "{lang}: its grammar is compiled in"
        );
        let mut h = SyntaxHighlighter::new(lang);
        h.update(None, &Rope::from_str(code), None);
        let styles = h.styles(&(0..code.len()), &*theme);
        assert!(!styles.is_empty(), "{lang}: {code:?} is coloured");
    }
}

/// A fence in a language without a grammar is shown as plain code, never an error.
#[test]
fn a_language_without_a_grammar_is_plain_code() {
    assert!(LanguageRegistry::singleton().language("cobol").is_none());
    let code = "DISPLAY 'HI'.";
    let mut h = SyntaxHighlighter::new("cobol");
    h.update(None, &Rope::from_str(code), None);
    let _ = h.styles(&(0..code.len()), &*HighlightTheme::default_dark());
}
