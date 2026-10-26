//! What a run of Claude may do without asking, by its stage: its whole job, so the human is
//! asked almost never. One table of defaults; a project adds its own rules (`[runs.allow]`, per
//! stage) and refuses more (`[runs.deny]`); a few things are never allowed whatever is set, and
//! are refused outright, not asked. Rules are Claude Code's own: `Bash(cargo test:*)` lets every
//! command that starts with `cargo test`, a bare tool's name (`Edit`) the tool.

use std::collections::BTreeMap;

use crate::run::Stage;

/// Reading the project, at every stage: commands that cannot write with any flag (`find`,
/// `sed`, `sort` and `git branch` can, so they are asked; the Read, Grep and Glob tools read
/// without them).
const READ: &[&str] = &[
    "Read",
    "Grep",
    "Glob",
    "Bash(livtk:*)",
    "Bash(ls:*)",
    "Bash(cat:*)",
    "Bash(head:*)",
    "Bash(tail:*)",
    "Bash(wc:*)",
    "Bash(grep:*)",
    "Bash(rg:*)",
    "Bash(diff:*)",
    "Bash(pwd:*)",
    "Bash(which:*)",
    "Bash(uniq:*)",
    "Bash(git status:*)",
    "Bash(git log:*)",
    "Bash(git show:*)",
    "Bash(git diff:*)",
    "Bash(git branch --show-current:*)",
    "Bash(git branch --list:*)",
    "Bash(cargo metadata:*)",
    "Bash(cargo tree:*)",
];

/// Checking the project: building it and running its tests and checks, never changing it.
const CHECK: &[&str] = &[
    "Bash(cargo build:*)",
    "Bash(cargo test:*)",
    "Bash(cargo clippy:*)",
    "Bash(cargo check:*)",
    "Bash(scripts/check:*)",
    "Bash(./scripts/check:*)",
];

/// Building: editing files, cargo, and committing on its own branch. A project's own scripts are
/// named one by one (`[runs.allow]`, or Allow always): a rule's prefix matches whole words only,
/// so `scripts/` does not cover `scripts/x`. Removing files is asked (`rm` can remove anything).
const BUILD: &[&str] = &[
    "Edit",
    "Write",
    "NotebookEdit",
    "TodoWrite",
    "Bash(cargo:*)",
    "Bash(git add:*)",
    "Bash(git commit:*)",
    "Bash(git restore:*)",
    "Bash(git rm:*)",
    "Bash(git mv:*)",
    "Bash(git switch:*)",
    "Bash(git checkout:*)",
    "Bash(git merge:*)",
    "Bash(git stash:*)",
    "Bash(mkdir:*)",
    "Bash(mv:*)",
    "Bash(cp:*)",
    "Bash(touch:*)",
];

/// Never, at any stage, whatever a project allows: refused outright, and said on the task.
pub const NEVER: &[&str] = &[
    "Bash(git push:*)",
    "Bash(git reset --hard:*)",
    "Bash(sudo:*)",
    "Bash(rm -rf /:*)",
    "Bash(rm -rf ~:*)",
    "Bash(rm -rf ..:*)",
    "Bash(cargo install:*)",
    "Bash(brew:*)",
    "Bash(npm install -g:*)",
    "Bash(npm i -g:*)",
    "Bash(npm install --global:*)",
    "Bash(pnpm add -g:*)",
    "Bash(pip install:*)",
    "Bash(pip3 install:*)",
];

/// Commands that run other commands (shells, interpreters, wrappers), or that remove: a rule
/// for their kind would let anything through, so allowing one always allows only that exact
/// command line.
const EXACT_ONLY: &[&str] = &[
    "bash",
    "sh",
    "zsh",
    "fish",
    "dash",
    "env",
    "xargs",
    "exec",
    "eval",
    "nohup",
    "nice",
    "time",
    "timeout",
    "command",
    "builtin",
    "sudo",
    "doas",
    "su",
    "ssh",
    "python",
    "python3",
    "node",
    "deno",
    "bun",
    "ruby",
    "perl",
    "php",
    "npx",
    "pnpx",
    "bunx",
    "uvx",
    "osascript",
    "open",
    "curl",
    "wget",
    "rm",
    "dd",
    "chmod",
    "chown",
    "kill",
    "pkill",
    "cd",
    "source",
    ".",
];

/// What a project adds and refuses, from its `[runs]`: its own rules for a stage (by the
/// stage's name), and rules refused at every stage.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rules<'a> {
    pub allow: Option<&'a BTreeMap<String, Vec<String>>>,
    pub deny: &'a [String],
}

/// What a run of `stage` may do without asking, and what it is refused outright.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Permits {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

/// The defaults for `stage`: reading always; checking for a review, research or build; and a
/// build's own work.
pub fn defaults(stage: Stage) -> Vec<String> {
    let mut out: Vec<&str> = READ.to_vec();
    if stage != Stage::Plan {
        out.extend(CHECK);
    }
    if stage == Stage::Build {
        out.extend(BUILD);
    }
    out.into_iter().map(str::to_owned).collect()
}

/// What a run of `stage` in a project with `rules` may do: the defaults and the project's own
/// for the stage, less anything refused (never-rules and the project's), which win over all.
pub fn permits(stage: Stage, rules: &Rules) -> Permits {
    let deny: Vec<String> = NEVER
        .iter()
        .map(|s| (*s).to_owned())
        .chain(rules.deny.iter().cloned())
        .collect();
    let mut allow = defaults(stage);
    if let Some(own) = rules.allow.and_then(|a| a.get(stage.as_str())) {
        for r in own {
            if !allow.contains(r) {
                allow.push(r.clone());
            }
        }
    }
    allow.retain(|r| !deny.iter().any(|d| d == r));
    Permits { allow, deny }
}

/// Whether `rule` (a refused tool, as a run's refusals name it: `Bash(git push origin main)`,
/// `WebFetch`) falls under `pattern` (`Bash(git push:*)`, `WebFetch`).
pub fn covers(pattern: &str, rule: &str) -> bool {
    match (bash(pattern), bash(rule)) {
        (Some(p), Some(r)) => match p.strip_suffix(":*") {
            Some(prefix) => {
                let prefix = prefix.trim_end();
                // As Claude Code matches: the prefix, then the end or a space.
                r == prefix
                    || r.strip_prefix(prefix)
                        .is_some_and(|rest| rest.starts_with(' '))
            }
            None => p == r,
        },
        (None, None) => pattern == rule,
        _ => false,
    }
}

/// The command inside `Bash(…)`, if `rule` is one.
fn bash(rule: &str) -> Option<&str> {
    rule.strip_prefix("Bash(")?.strip_suffix(')').map(str::trim)
}

/// The rule to allow from now on for a refused one: its kind of command as a prefix rule (its
/// first word, or two for a tool with subcommands: `cargo test`, `git log`, `npm run`), so the
/// same kind is never asked again. Only that exact command line when a kind would be too much:
/// a command that runs others or removes (`EXACT_ONLY`), one led by a flag or a variable
/// (`git -C x`, `FOO=1 make`), or a kind that would cover something never allowed. A tool as
/// itself.
pub fn always_rule(rule: &str) -> String {
    let Some(cmd) = bash(rule) else {
        return rule.to_owned();
    };
    let words: Vec<&str> = cmd.split_whitespace().collect();
    let exact = format!("Bash({cmd})");
    let Some(first) = words.first().copied() else {
        return exact;
    };
    let bin = first.rsplit('/').next().unwrap_or(first);
    if EXACT_ONLY.contains(&bin) || first.contains('=') {
        return exact;
    }
    let sub = matches!(
        bin,
        "cargo" | "git" | "npm" | "pnpm" | "yarn" | "go" | "docker" | "make" | "uv" | "just"
    );
    let take = if sub { 2 } else { 1 };
    if words.len() < take || words[..take].iter().any(|w| w.starts_with('-')) {
        return exact;
    }
    let kind = format!("Bash({}:*)", words[..take].join(" "));
    // A kind that would let through something never allowed is too much.
    let never_covered = NEVER.iter().any(|n| {
        bash(n).is_some_and(|nc| covers(&kind, &format!("Bash({})", nc.trim_end_matches(":*"))))
    });
    if never_covered {
        return exact;
    }
    kind
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_stage_may_do_its_whole_job() {
        let plan = defaults(Stage::Plan);
        assert!(plan.iter().any(|r| r == "Bash(git log:*)"));
        assert!(!plan.iter().any(|r| r == "Edit"), "a plan edits nothing");
        let review = defaults(Stage::Review);
        assert!(review.iter().any(|r| r == "Bash(cargo test:*)"));
        assert!(!review.iter().any(|r| r == "Edit"));
        let build = defaults(Stage::Build);
        for r in [
            "Edit",
            "Bash(cargo:*)",
            "Bash(scripts/check:*)",
            "Bash(git commit:*)",
        ] {
            assert!(build.iter().any(|b| b == r), "{r}");
        }
    }

    #[test]
    fn a_project_adds_and_refuses_and_never_wins() {
        let mut allow = BTreeMap::new();
        allow.insert(
            "build".to_owned(),
            vec!["Bash(npm test:*)".to_owned(), "Bash(git push:*)".to_owned()],
        );
        let deny = vec!["Bash(rm:*)".to_owned()];
        let p = permits(
            Stage::Build,
            &Rules {
                allow: Some(&allow),
                deny: &deny,
            },
        );
        assert!(p.allow.iter().any(|r| r == "Bash(npm test:*)"), "its own");
        assert!(
            !p.allow.iter().any(|r| r == "Bash(git push:*)"),
            "never, whatever is set"
        );
        assert!(
            !p.allow.iter().any(|r| r == "Bash(rm:*)"),
            "refused by the project"
        );
        assert!(p.deny.iter().any(|r| r == "Bash(git push:*)"));
        // Another stage's own rules are not this one's.
        let review = permits(
            Stage::Review,
            &Rules {
                allow: Some(&allow),
                deny: &deny,
            },
        );
        assert!(!review.allow.iter().any(|r| r == "Bash(npm test:*)"));
    }

    #[test]
    fn a_refusal_is_covered_by_its_rule() {
        assert!(covers("Bash(git push:*)", "Bash(git push origin main)"));
        assert!(covers("Bash(git push:*)", "Bash(git push)"));
        assert!(!covers("Bash(git push:*)", "Bash(git pushy)"));
        assert!(!covers("Bash(git push:*)", "Bash(git status)"));
        assert!(
            !covers("Bash(scripts/:*)", "Bash(scripts/check)"),
            "as Claude Code: words"
        );
        assert!(covers("Bash(cargo test)", "Bash(cargo test)"));
        assert!(!covers("Bash(cargo test)", "Bash(cargo test --all)"));
        assert!(covers("WebFetch", "WebFetch"));
        assert!(!covers("WebFetch", "Bash(WebFetch)"));
    }

    #[test]
    fn allowing_always_takes_the_kind_of_command() {
        assert_eq!(always_rule("Bash(npm test -- --watch)"), "Bash(npm test:*)");
        assert_eq!(always_rule("Bash(jq .name package.json)"), "Bash(jq:*)");
        assert_eq!(always_rule("Bash(make build)"), "Bash(make build:*)");
        // Only the exact line: what runs others, removes, or is led by a flag or a variable.
        assert_eq!(
            always_rule("Bash(git -C sub status)"),
            "Bash(git -C sub status)"
        );
        assert_eq!(always_rule("Bash(bash -c ls)"), "Bash(bash -c ls)");
        assert_eq!(always_rule("Bash(python3 x.py)"), "Bash(python3 x.py)");
        assert_eq!(
            always_rule("Bash(/usr/bin/env FOO=1 x)"),
            "Bash(/usr/bin/env FOO=1 x)"
        );
        assert_eq!(always_rule("Bash(rm -rf target)"), "Bash(rm -rf target)");
        assert_eq!(always_rule("Bash(FOO=1 make)"), "Bash(FOO=1 make)");
        // A kind covering something never allowed: the exact line.
        assert_eq!(always_rule("Bash(brew list)"), "Bash(brew list)");
        assert_eq!(always_rule("WebFetch"), "WebFetch");
    }
}
