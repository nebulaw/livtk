//! Reading shell commands for livtk invocations.
//!
//! This is text analysis, a guard rail that keeps Claude from taking a human-only step by
//! accident or by habit. It is not a security boundary: a determined shell trick (an alias, a
//! variable holding the verb) gets past it, and the human stays the root of trust.

use std::path::Path;

/// Subcommands only a human may run.
const HUMAN_VERBS: [&str; 5] = ["ack", "ratify", "reject", "retire", "accept"];
/// States only a human may move a node into.
const HUMAN_STATES: [&str; 5] = ["acked", "ratified", "rejected", "retired", "accepted"];

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
    let code = code_mask(cmd);
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
            runs: code.get(at).copied().unwrap_or(false)
                && runs_at(&code_prefix(cmd, &code, start)),
            verb,
            args,
        });
    }
    out
}

/// For each byte of `cmd`, whether the shell reads it as code: not inside quotes, and not in a
/// heredoc's body (its closing word included). A mention of livtk outside code is text the
/// command passes on, never a command it runs.
fn code_mask(cmd: &str) -> Vec<bool> {
    let b = cmd.as_bytes();
    let mut mask = vec![true; b.len()];
    // Heredocs opened on the current line, their bodies starting at its end: (word, strip tabs).
    let mut pending: Vec<(String, bool)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            q @ (b'\'' | b'"') => {
                let from = i;
                i += 1;
                while i < b.len() && b[i] != q {
                    if q == b'"' && b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                for m in &mut mask[from..b.len().min(i + 1)] {
                    *m = false;
                }
                i += 1;
            }
            b'<' if b.get(i + 1) == Some(&b'<') && b.get(i + 2) != Some(&b'<') => {
                i += 2;
                let strip = b.get(i) == Some(&b'-');
                if strip {
                    i += 1;
                }
                while b.get(i).is_some_and(|c| *c == b' ' || *c == b'\t') {
                    i += 1;
                }
                let quote = b.get(i).copied().filter(|c| *c == b'\'' || *c == b'"');
                if quote.is_some() {
                    i += 1;
                }
                let from = i;
                while b
                    .get(i)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
                {
                    i += 1;
                }
                if i > from {
                    pending.push((cmd[from..i].to_owned(), strip));
                }
                if quote.is_some() && b.get(i) == quote.as_ref() {
                    i += 1;
                }
            }
            b'\n' if !pending.is_empty() => {
                i += 1;
                // Each pending heredoc's body, one after the other, through its closing word.
                for (word, strip) in std::mem::take(&mut pending) {
                    loop {
                        let end = cmd[i..].find('\n').map_or(b.len(), |n| i + n);
                        let line = &cmd[i..end];
                        let line = if strip {
                            line.trim_start_matches('\t')
                        } else {
                            line
                        };
                        for m in &mut mask[i..end] {
                            *m = false;
                        }
                        let last = end >= b.len();
                        i = (end + 1).min(b.len());
                        if line == word || last {
                            break;
                        }
                        // The newline inside the body is the body's too.
                        if let Some(m) = mask.get_mut(end) {
                            *m = false;
                        }
                    }
                }
            }
            _ => i += 1,
        }
    }
    mask
}

/// `cmd` up to `end`, each newline the shell reads as code turned into the `;` it means there.
fn code_prefix(cmd: &str, code: &[bool], end: usize) -> String {
    cmd[..end]
        .char_indices()
        .map(|(i, c)| {
            if c == '\n' && code.get(i).copied().unwrap_or(false) {
                ';'
            } else {
                c
            }
        })
        .collect()
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
                "Only the human acknowledges plans and decides what binds: acknowledging, \
                 ratifying, rejecting, retiring and accepting are theirs. Ask them to run the \
                 livtk command or to decide in the app. (If you were only searching for this \
                 text, use the Grep tool.)"
                    .into(),
            );
        }
    }
    None
}

/// Why a file tool must not write `path`, if it must not: anything in `.livtk/`, which changes
/// only through livtk's commands. A build's own worktree sits in `.livtk/` too
/// (`.livtk/runs/worktrees/<KEY>/`); a file in it is judged by its place in that worktree, so its
/// code is the build's to edit and its own `.livtk/` is refused as any checkout's is. A path that
/// steps back (`..`) is never taken for a worktree's.
pub fn refuse_path(path: &str) -> Option<String> {
    use std::path::Component;
    let parts: Vec<Component> = Path::new(path).components().collect();
    let stepped_back = parts.iter().any(|c| matches!(c, Component::ParentDir));
    let in_livtk = |parts: &[Component]| parts.iter().position(|c| c.as_os_str() == ".livtk");
    let mut rest: &[Component] = &parts;
    while let Some(at) = in_livtk(rest) {
        // `.livtk/runs/worktrees/<KEY>/<a file>`: on to the file's place in the worktree.
        let worktree = rest
            .get(at + 1)
            .is_some_and(|c| c.as_os_str() == livtk_core::run::RUNS)
            && rest
                .get(at + 2)
                .is_some_and(|c| c.as_os_str() == livtk_core::run::WORKTREES)
            && rest
                .get(at + 3)
                .is_some_and(|c| matches!(c, Component::Normal(_)))
            && rest.len() > at + 4;
        if !worktree || stepped_back {
            return Some(
                "Files in .livtk/ change only through livtk commands, so every change stays an \
                 event in the log."
                    .into(),
            );
        }
        rest = &rest[at + 4..];
    }
    None
}

/// livtk's own flags that say who runs a command: the Claude Code session, and the subagent
/// when one runs it. Each a safe shell word.
pub fn signature(session: Option<&str>, agent: Option<&str>) -> String {
    let mut flags = Vec::new();
    if let Some(s) = session.filter(|s| !s.is_empty()) {
        flags.push(format!("--session {}", safe_word(s)));
    }
    if let Some(a) = agent.filter(|a| !a.is_empty()) {
        flags.push(format!("--agent {}", agent_name(a)));
    }
    flags.join(" ")
}

/// `cmd` with every livtk it runs signed with `flags` (see `signature`), or `None` if it runs
/// none or there is nothing to sign with.
///
/// The flags go right after the program word, so the command still starts with `livtk` and the
/// user's permission rules for it still match.
pub fn sign(cmd: &str, flags: &str) -> Option<String> {
    if flags.is_empty() {
        return None;
    }
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
        out.insert_str(p, &format!(" {flags}"));
    }
    Some(out)
}

/// An agent type as a safe shell word: our own plugin's prefix dropped, odd characters replaced.
pub fn agent_name(agent_type: &str) -> String {
    safe_word(agent_type.strip_prefix("livtk:").unwrap_or(agent_type))
}

/// `s` as a word the shell passes as it is: odd characters replaced.
fn safe_word(s: &str) -> String {
    s.chars()
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
            "-C" | "--dir" | "--agent" | "--session" => i += 2,
            t if t.starts_with("--dir=")
                || t.starts_with("--agent=")
                || t.starts_with("--session=")
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
            "livtk ratify LES-1",
            "livtk ratify LES-1 --always",
            "livtk reject DEC-2",
            "livtk retire LES-3",
            "livtk accept DEC-1",
            "livtk move LES-1 ratified",
            "livtk --json move DEC-1 accepted",
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
            "livtk lesson \"never ratify by habit\" --why -",
            "livtk list --kind lesson --state ratified",
            "livtk recall \"accept the decision\"",
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
        // A build's own worktree: its code is the build's, its own `.livtk/` is not.
        let wt = "/p/.livtk/runs/worktrees/TAS-93";
        assert_eq!(refuse_path(&format!("{wt}/crates/app/src/main.rs")), None);
        assert_eq!(refuse_path(".livtk/runs/worktrees/TAS-93/README.md"), None);
        assert!(refuse_path(&format!("{wt}/.livtk/events.jsonl")).is_some());
        assert!(refuse_path(&format!("{wt}/.livtk/runs/worktrees/X/a.rs")).is_none());
        // Only that shape: the worktrees folder itself, other run files, steps back.
        assert!(refuse_path("/p/.livtk/runs/worktrees/TAS-93").is_some());
        assert!(refuse_path("/p/.livtk/runs/worktrees").is_some());
        assert!(refuse_path("/p/.livtk/runs/TAS-93.json").is_some());
        assert!(refuse_path("/p/.livtk/runs/worktrees/TAS-93/../../config.toml").is_some());
        assert!(refuse_path("/p/.livtk/runs/worktrees/../events.jsonl").is_some());
    }

    #[test]
    fn signs_each_run_right_after_the_program_word() {
        assert_eq!(
            sign("livtk list", "--agent refuter").as_deref(),
            Some("livtk --agent refuter list")
        );
        assert_eq!(
            sign(
                "cd x && livtk note 1 hi; ~/.cargo/bin/livtk --json next",
                "--agent a"
            )
            .as_deref(),
            Some("cd x && livtk --agent a note 1 hi; ~/.cargo/bin/livtk --agent a --json next")
        );
        assert_eq!(
            sign("FOO=1 livtk list", "--agent a").as_deref(),
            Some("FOO=1 livtk --agent a list")
        );
        assert_eq!(
            sign("env livtk list", "--agent a").as_deref(),
            Some("env livtk --agent a list")
        );
        // Where the words after it are not livtk's own arguments, leave the command alone.
        assert_eq!(sign("echo livtk list", "--agent a"), None);
        assert_eq!(sign("nohup livtk list", "--agent a"), None);
        assert_eq!(sign("cargo run --bin livtk -- list", "--agent a"), None);
        assert_eq!(sign("cd livtk", "--agent a"), None);
        assert_eq!(sign("git status", "--agent a"), None);
    }

    #[test]
    fn a_session_signs_the_main_agent_too() {
        assert_eq!(
            sign("livtk note 1 hi", &signature(Some("c1c6d321-b6de"), None)).as_deref(),
            Some("livtk --session c1c6d321-b6de note 1 hi")
        );
        assert_eq!(
            signature(Some("s1"), Some("livtk:refuter")),
            "--session s1 --agent refuter"
        );
        assert_eq!(
            sign("livtk list", &signature(None, None)),
            None,
            "nothing to sign with"
        );
        assert_eq!(
            verbs("livtk --session s1 --agent a move 1 acked"),
            [Some("move".into())]
        );
    }

    #[test]
    fn a_livtk_on_a_new_line_runs_and_one_in_text_does_not() {
        let runs = |cmd: &str| -> Vec<bool> { invocations(cmd).iter().map(|i| i.runs).collect() };
        // A new line is a new command, as `;` is.
        assert_eq!(runs("livtk note A x\nlivtk move A planned"), [true, true]);
        // After a heredoc: its body is text; the command after it runs.
        assert_eq!(
            runs("cat > f <<'EOF'\nlivtk move A planned\nEOF\nlivtk note A x"),
            [false, true]
        );
        assert_eq!(
            runs("python3 - <<EOF\nprint(1)\nEOF\nlivtk note A x; livtk list"),
            [true, true]
        );
        // Text inside quotes never runs, across lines too.
        assert_eq!(runs("echo \"x\nlivtk list\""), [false]);
        assert_eq!(runs("echo 'livtk list'\nlivtk list"), [false, true]);
        // Signing touches only what runs: the heredoc's text is left as written.
        assert_eq!(
            sign("cat > f <<EOF\nlivtk list\nEOF\nlivtk next", "--session s").as_deref(),
            Some("cat > f <<EOF\nlivtk list\nEOF\nlivtk --session s next")
        );
    }

    #[test]
    fn a_signed_human_step_is_still_refused() {
        let signed = sign("livtk ack 1", "--agent refuter").unwrap();
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
