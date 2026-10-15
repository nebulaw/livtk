//! Reading shell commands for livtk invocations.
//!
//! This is text analysis, a guard rail that keeps Claude from taking a human-only step by
//! accident or by habit. It is not a security boundary: a determined shell trick (an alias, a
//! variable holding the verb) gets past it, and the human stays the root of trust.

use std::path::Path;

/// Subcommands only a human may run.
const HUMAN_VERBS: [&str; 1] = ["ack"];
/// States only a human may move a task into.
const HUMAN_STATES: [&str; 1] = ["acked"];

/// Characters that end a simple command in a shell line.
const SEPARATORS: [char; 6] = [';', '&', '|', '\n', ')', '`'];

/// One place a command line runs livtk.
#[derive(Debug, PartialEq, Eq)]
pub struct Invocation {
    /// Byte offset where the program word starts (its path, if it has one).
    pub start: usize,
    /// Byte offset just after the program word, where livtk's own flags begin.
    pub end: usize,
    /// The program word is what the shell runs here, so the words after it are livtk's own
    /// arguments.
    pub runs: bool,
    /// The livtk subcommand, with global flags skipped.
    pub verb: Option<String>,
    pub args: Vec<String>,
}

/// Every place `cmd` mentions the livtk program as a word: `livtk`, a path ending in it, or the
/// `livtk-cli` package run through cargo.
pub fn invocations(cmd: &str) -> Vec<Invocation> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(pos) = cmd[from..].find("livtk") {
        let at = from + pos;
        from = at + "livtk".len();
        let joined = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.');
        if cmd[..at].chars().next_back().is_some_and(joined) {
            continue;
        }
        let rest = &cmd[at..];
        let word = if rest.starts_with("livtk-cli") { 9 } else { 5 };
        let ends_word = |c: char| c.is_whitespace() || SEPARATORS.contains(&c) || "\"'".contains(c);
        if rest[word..].chars().next().is_some_and(|c| !ends_word(c)) {
            continue;
        }
        let start = cmd[..at]
            .char_indices()
            .rev()
            .find(|&(_, c)| c.is_whitespace() || SEPARATORS.contains(&c) || "\"'(".contains(c))
            .map_or(0, |(i, c)| i + c.len_utf8());
        let tail = &rest[word..];
        let tail = &tail[..tail.find(SEPARATORS).unwrap_or(tail.len())];
        let tokens: Vec<String> = tail.split_whitespace().map(unquote).collect();
        let (verb, args) = split_verb(&tokens);
        out.push(Invocation {
            start,
            end: at + word,
            runs: runs_at(&cmd[..start]),
            verb,
            args,
        });
    }
    out
}

/// Why a Bash command must not run, if it must not.
pub fn refuse_bash(cmd: &str) -> Option<String> {
    if cmd.contains("CLAUDECODE") {
        return Some(
            "CLAUDECODE tells livtk that Claude is acting; commands may not set or unset it."
                .into(),
        );
    }
    for inv in invocations(cmd) {
        let human_verb = inv
            .verb
            .as_deref()
            .is_some_and(|v| HUMAN_VERBS.contains(&v));
        let human_move = inv.verb.as_deref() == Some("move")
            && inv.args.iter().any(|a| HUMAN_STATES.contains(&a.as_str()));
        if human_verb || human_move {
            return Some(
                "Only the human acknowledges a plan. Ask them to run `livtk ack KEY` or to \
                 acknowledge it in the app. (If you were only searching for this text, use the \
                 Grep tool.)"
                    .into(),
            );
        }
    }
    None
}

/// Why a file tool must not write `path`, if it must not.
pub fn refuse_path(path: &str) -> Option<String> {
    Path::new(path)
        .components()
        .any(|c| c.as_os_str() == ".livtk")
        .then(|| {
            "Files in .livtk/ change only through livtk commands, so every change stays an event \
             in the log."
                .into()
        })
}

/// `cmd` with every livtk it runs signed by `agent`, or `None` if it runs none.
///
/// The signature is livtk's own `--agent` flag, placed right after the program word, so the
/// command still starts with `livtk` and the user's permission rules for it still match.
pub fn sign(cmd: &str, agent: &str) -> Option<String> {
    let spots: Vec<usize> = invocations(cmd)
        .into_iter()
        .filter(|i| i.runs && i.verb.is_some())
        .map(|i| i.end)
        .collect();
    if spots.is_empty() {
        return None;
    }
    let mut out = cmd.to_owned();
    for &p in spots.iter().rev() {
        out.insert_str(p, &format!(" --agent {agent}"));
    }
    Some(out)
}

/// An agent type as a safe shell word: our own plugin's prefix dropped, odd characters replaced.
pub fn agent_name(agent_type: &str) -> String {
    agent_type
        .strip_prefix("livtk:")
        .unwrap_or(agent_type)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_-.:".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn unquote(token: &str) -> String {
    token.chars().filter(|c| !"\"'\\".contains(*c)).collect()
}

/// Skip livtk's global flags and cargo's `--`, then split off the subcommand.
fn split_verb(tokens: &[String]) -> (Option<String>, Vec<String>) {
    let mut i = 0;
    while let Some(t) = tokens.get(i) {
        match t.as_str() {
            "--" | "--json" => i += 1,
            "-C" | "--dir" | "--agent" => i += 2,
            t if t.starts_with("--dir=")
                || t.starts_with("--agent=")
                || (t.starts_with("-C") && t.len() > 2) =>
            {
                i += 1
            }
            _ => break,
        }
    }
    (
        tokens.get(i).cloned(),
        tokens.iter().skip(i + 1).cloned().collect(),
    )
}

/// Whether a word right after `prefix` is a command the shell runs, so that an environment
/// assignment in front of it applies to it.
fn runs_at(prefix: &str) -> bool {
    for token in prefix.split_whitespace().rev() {
        let assignment = token.split_once('=').is_some_and(|(name, _)| {
            name.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        if assignment {
            continue;
        }
        return token.ends_with(SEPARATORS)
            || token.ends_with(['(', '{'])
            || matches!(token, "then" | "do" | "else" | "!" | "env");
    }
    // Nothing before it but assignments: the start of the line.
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verbs(cmd: &str) -> Vec<Option<String>> {
        invocations(cmd).into_iter().map(|i| i.verb).collect()
    }

    #[test]
    fn finds_livtk_as_a_program_word_only() {
        assert_eq!(verbs("livtk list"), [Some("list".into())]);
        assert_eq!(
            verbs("~/.cargo/bin/livtk --json -C ../x show 3"),
            [Some("show".into())]
        );
        assert_eq!(
            verbs("cargo run -p livtk-cli -- ack 1"),
            [Some("ack".into())]
        );
        assert_eq!(
            verbs("cargo build -p livtk-core"),
            Vec::<Option<String>>::new()
        );
        assert_eq!(
            verbs("cat crates/livtk-cli/src/main.rs"),
            Vec::<Option<String>>::new()
        );
        assert_eq!(verbs("echo mylivtk"), Vec::<Option<String>>::new());
        assert_eq!(verbs("cd livtk && make"), [None]);
    }

    #[test]
    fn refuses_human_steps_however_they_are_spelled() {
        for cmd in [
            "livtk ack 1",
            "livtk ack LIV-3",
            "cd x && livtk --json move 2 acked",
            "livtk move 2 'acked'",
            "livtk \"ack\" 1",
            "livtk a\\ck 1",
            "sh -c \"livtk ack 1\"",
            "bash -c 'livtk -C .. ack 4'",
            "x=$(livtk ack 1)",
            "./target/release/livtk ack 1",
            "cargo run --bin livtk -- ack 3",
            "env -u CLAUDECODE livtk list",
            "CLAUDECODE= livtk move 1 acked",
            "unset CLAUDECODE; livtk ack 1",
        ] {
            assert!(refuse_bash(cmd).is_some(), "{cmd}");
        }
    }

    #[test]
    fn allows_everything_else() {
        for cmd in [
            "livtk list",
            "livtk list --state acked",
            "livtk move 2 planned",
            "livtk show 2 | grep acked",
            "livtk new \"ack the review\"",
            "git add .livtk && git commit -m 'livtk: tasks'",
            "cargo test -p livtk-cli",
            "echo ack",
        ] {
            assert_eq!(refuse_bash(cmd), None, "{cmd}");
        }
    }

    #[test]
    fn a_mention_in_a_search_is_refused_with_a_hint() {
        let why = refuse_bash("grep -r \"livtk ack\" docs").unwrap();
        assert!(why.contains("Grep tool"), "{why}");
    }

    #[test]
    fn refuses_file_tools_inside_livtk() {
        assert!(refuse_path("/p/.livtk/events.jsonl").is_some());
        assert!(refuse_path(".livtk/config.toml").is_some());
        assert_eq!(refuse_path("/p/src/livtk.rs"), None);
        assert_eq!(refuse_path("/p/docs/.livtkrc"), None);
    }

    #[test]
    fn signs_each_run_right_after_the_program_word() {
        assert_eq!(
            sign("livtk list", "refuter").as_deref(),
            Some("livtk --agent refuter list")
        );
        assert_eq!(
            sign(
                "cd x && livtk note 1 hi; ~/.cargo/bin/livtk --json next",
                "a"
            )
            .as_deref(),
            Some("cd x && livtk --agent a note 1 hi; ~/.cargo/bin/livtk --agent a --json next")
        );
        assert_eq!(
            sign("FOO=1 livtk list", "a").as_deref(),
            Some("FOO=1 livtk --agent a list")
        );
        assert_eq!(
            sign("env livtk list", "a").as_deref(),
            Some("env livtk --agent a list")
        );
        // Where the words after it are not livtk's own arguments, leave the command alone.
        assert_eq!(sign("echo livtk list", "a"), None);
        assert_eq!(sign("nohup livtk list", "a"), None);
        assert_eq!(sign("cargo run --bin livtk -- list", "a"), None);
        assert_eq!(sign("cd livtk", "a"), None);
        assert_eq!(sign("git status", "a"), None);
    }

    #[test]
    fn a_signed_human_step_is_still_refused() {
        let signed = sign("livtk ack 1", "refuter").unwrap();
        assert!(refuse_bash(&signed).is_some());
        assert!(refuse_bash("livtk --agent=x move 1 acked").is_some());
    }

    #[test]
    fn agent_names_are_safe_shell_words() {
        assert_eq!(agent_name("livtk:refuter"), "refuter");
        assert_eq!(agent_name("Explore"), "Explore");
        assert_eq!(agent_name("other:rev iew$(x)"), "other:rev_iew__x_");
    }
}
