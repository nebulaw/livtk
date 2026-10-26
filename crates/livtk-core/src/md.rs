//! Markdown as blocks: a source split into its top-level blocks (a paragraph, a heading, a whole
//! list, a quote, a code block, a table…), each with its kind and the exact bytes it came from,
//! and an edit of one block written back into the source with nothing else in it touched. A
//! body or a doc is read and edited block by block through this.
//!
//! Parsed by the `markdown` crate with the options the app's renderer uses (CommonMark, GitHub's
//! tables, task lists, strikethrough, autolinks and footnotes, and math), so the blocks found
//! here are the blocks drawn. The crate stays behind this module: callers see only `Block`.

use std::ops::Range;

use ::markdown::mdast::Node;
use ::markdown::{ParseOptions, to_mdast};

/// What a block is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    /// `#` to `######`, or underlined with `===` (1) or `---` (2).
    Heading {
        level: u8,
    },
    /// A whole list: its items share their markers and numbering, so they are one block.
    List {
        ordered: bool,
    },
    Quote,
    /// Fenced (``` or ~~~) or indented.
    Code {
        fenced: bool,
    },
    /// A `$$` block.
    Math,
    Table,
    /// `---`, `***` or `___` on a line of its own.
    Rule,
    Html,
    /// A link's reference definition, `[name]: url`: drawn as nothing, read by the links that
    /// name it, wherever they are.
    Definition,
    /// A footnote's text, `[^name]: …`.
    Footnote,
    /// Anything else the parser finds at the top.
    Other,
}

/// A top-level block of a source: its kind and its bytes, `source[range]`, which never start or
/// end with a line break; what lies between blocks (blank lines) belongs to none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub range: Range<usize>,
}

/// The options the source is parsed with: the app's renderer's.
fn options() -> ParseOptions {
    let mut options = ParseOptions::gfm();
    options.constructs.math_text = true;
    options.constructs.math_flow = true;
    options
}

/// The top-level blocks of `source`, in order. Markdown has no syntax errors, so every source
/// has its blocks; an empty or blank one has none.
pub fn blocks(source: &str) -> Vec<Block> {
    let Ok(Node::Root(root)) = to_mdast(source, &options()) else {
        return Vec::new();
    };
    root.children
        .iter()
        .filter_map(|node| {
            let p = node.position()?;
            let range = trim_breaks(source, p.start.offset..p.end.offset);
            (!range.is_empty()).then(|| Block {
                kind: kind_of(node, &source[range.clone()]),
                range,
            })
        })
        .collect()
}

/// `node`'s kind; `text`, its source, tells a fenced code block from an indented one.
fn kind_of(node: &Node, text: &str) -> BlockKind {
    match node {
        Node::Paragraph(_) => BlockKind::Paragraph,
        Node::Heading(h) => BlockKind::Heading { level: h.depth },
        Node::List(l) => BlockKind::List { ordered: l.ordered },
        Node::Blockquote(_) => BlockKind::Quote,
        Node::Code(_) => BlockKind::Code {
            fenced: is_fence(text),
        },
        Node::Math(_) => BlockKind::Math,
        Node::Table(_) => BlockKind::Table,
        Node::ThematicBreak(_) => BlockKind::Rule,
        Node::Html(_) => BlockKind::Html,
        Node::Definition(_) => BlockKind::Definition,
        Node::FootnoteDefinition(_) => BlockKind::Footnote,
        _ => BlockKind::Other,
    }
}

/// Whether a code block's source opens with a fence: up to three spaces, then ``` or ~~~.
fn is_fence(text: &str) -> bool {
    let line = text.trim_start_matches(' ');
    text.len() - line.len() <= 3 && (line.starts_with("```") || line.starts_with("~~~"))
}

/// `range` less any line breaks at its ends.
fn trim_breaks(source: &str, range: Range<usize>) -> Range<usize> {
    let s = &source[range.clone()];
    let start = range.start + (s.len() - s.trim_start_matches(['\n', '\r']).len());
    let end = range.end - (s.len() - s.trim_end_matches(['\n', '\r']).len());
    start..end.max(start)
}

/// `source` with `range` (a block's) replaced by `text`, everything else as it was, byte for
/// byte. `text` unchanged gives `source` back unchanged. Line breaks after `text` are dropped
/// (what separates blocks stays where it is); `text` empty removes the block and the blank lines
/// after it (before it, for the last), so no gap is left where it was.
pub fn splice(source: &str, range: Range<usize>, text: &str) -> String {
    let text = text.trim_end_matches(['\n', '\r']);
    if source.get(range.clone()) == Some(text) {
        return source.to_owned();
    }
    let range = range.start.min(source.len())..range.end.min(source.len());
    let range = if text.trim().is_empty() {
        // The blank lines after it go with it, up to the next block's line (its indent kept);
        // after the last, the blank lines before it, from the end of the line above.
        let next_line = blank_lines_after(source, range.end);
        if next_line < source.len() {
            range.start..next_line
        } else {
            blank_lines_before(source, range.start)..source.len()
        }
    } else {
        range
    };
    let text = if text.trim().is_empty() { "" } else { text };
    let mut out = String::with_capacity(source.len() + text.len());
    out.push_str(&source[..range.start]);
    out.push_str(text);
    out.push_str(&source[range.end..]);
    out
}

/// Where the first line after `at` with anything on it starts (`source`'s end if none): the
/// rest of `at`'s line and the blank lines after it end there.
fn blank_lines_after(source: &str, at: usize) -> usize {
    let mut line = at;
    for (i, c) in source[at..].char_indices() {
        if c == '\n' {
            line = at + i + 1;
        } else if !c.is_whitespace() {
            return line;
        }
    }
    source.len()
}

/// Where the line break ending the last line before `at` with anything on it is (`0` if none):
/// the blank lines before `at` start there.
fn blank_lines_before(source: &str, at: usize) -> usize {
    let head = &source[..at];
    let Some(last) = head.rfind(|c: char| !c.is_whitespace()) else {
        return 0;
    };
    head[last..].find(['\r', '\n']).map_or(at, |i| last + i)
}

/// `source` with `text` added as a new last block, a blank line between them.
pub fn append(source: &str, text: &str) -> String {
    let text = text.trim_end_matches(['\n', '\r']);
    if text.trim().is_empty() {
        return source.to_owned();
    }
    let kept = source.trim_end();
    if kept.is_empty() {
        return text.to_owned();
    }
    format!("{kept}\n\n{text}")
}

/// What a block needs besides its own text to read as it does in the whole source: the link
/// reference definitions, which a link anywhere may name.
pub fn context(source: &str, blocks: &[Block]) -> String {
    blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Definition)
        .map(|b| &source[b.range.clone()])
        .collect::<Vec<_>>()
        .join("\n")
}

/// A task list item's box, `[ ]` or `[x]`: where its mark sits (the byte between the brackets)
/// and whether it is ticked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskBox {
    pub mark: usize,
    pub checked: bool,
}

/// Every task box in `source`, in order, nested lists' too.
pub fn task_boxes(source: &str) -> Vec<TaskBox> {
    let Ok(root) = to_mdast(source, &options()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_boxes(&root, source, &mut out);
    out
}

fn collect_boxes(node: &Node, source: &str, out: &mut Vec<TaskBox>) {
    if let Node::ListItem(item) = node
        && let (Some(checked), Some(p)) = (item.checked, item.position.as_ref())
        && let Some(mark) = box_mark(&source[p.start.offset..p.end.offset])
    {
        out.push(TaskBox {
            mark: p.start.offset + mark,
            checked,
        });
    }
    for child in node.children().into_iter().flatten() {
        collect_boxes(child, source, out);
    }
}

/// Where the mark of the box at the start of an item's source is: past its marker (`-`, `*`,
/// `+`, or a number and `.` or `)`) and the spaces after it, between `[` and `]`.
fn box_mark(item: &str) -> Option<usize> {
    let marker = item
        .find(|c: char| c.is_whitespace())
        .filter(|&end| end > 0)?;
    let rest = &item[marker..];
    let open = marker + (rest.len() - rest.trim_start().len());
    (item.get(open..open + 1) == Some("[") && item.get(open + 2..open + 3) == Some("]"))
        .then_some(open + 1)
}

/// `source` with the task box whose mark is at `mark` ticked or unticked; `None` when there is
/// no box there.
pub fn toggle_task(source: &str, mark: usize) -> Option<String> {
    let b = task_boxes(source).into_iter().find(|b| b.mark == mark)?;
    let mut out = source.to_owned();
    out.replace_range(mark..mark + 1, if b.checked { " " } else { "x" });
    Some(out)
}

/// An item of a list, for drawing it by hand: its marker as written (`-`, `1.`), its task box
/// if it has one (the box's mark's byte in the list's source, and whether it is ticked), and
/// its content as markdown of its own (what follows the marker and the box, its continuation
/// lines out-dented to it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    pub marker: String,
    pub task: Option<TaskBox>,
    pub content: String,
}

/// The top-level items of the list `source` holds (a list block's text); none if it holds no
/// list.
pub fn list_items(source: &str) -> Vec<ListItem> {
    let Ok(Node::Root(root)) = to_mdast(source, &options()) else {
        return Vec::new();
    };
    let Some(Node::List(list)) = root.children.first() else {
        return Vec::new();
    };
    list.children
        .iter()
        .filter_map(|node| {
            let Node::ListItem(item) = node else {
                return None;
            };
            let p = item.position.as_ref()?;
            let text = source[p.start.offset..p.end.offset].trim_end_matches(['\n', '\r']);
            let marker_end = text.find(char::is_whitespace).unwrap_or(text.len());
            let marker = text[..marker_end].to_owned();
            let task = item.checked.and_then(|checked| {
                box_mark(text).map(|mark| TaskBox {
                    mark: p.start.offset + mark,
                    checked,
                })
            });
            // Where the content starts: past the box and a space, or past the marker's spaces.
            let start = match task {
                Some(b) => b.mark - p.start.offset + 2,
                None => marker_end,
            };
            let first_line = text.lines().next().unwrap_or("");
            let first = first_line.get(start.min(first_line.len())..).unwrap_or("");
            let pad = first.len() - first.trim_start_matches(' ').len();
            let column = start + pad;
            let mut content = first.trim_start_matches(' ').to_owned();
            for line in text.lines().skip(1) {
                let indent = line.len() - line.trim_start_matches(' ').len();
                content.push('\n');
                content.push_str(&line[indent.min(column)..]);
            }
            Some(ListItem {
                marker,
                task,
                content,
            })
        })
        .collect()
}

/// Whether the list `source` holds has a task box.
pub fn has_task_box(source: &str) -> bool {
    list_items(source).iter().any(|i| i.task.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<BlockKind> {
        blocks(source).into_iter().map(|b| b.kind).collect()
    }

    /// Each edge case of the plan's table, its blocks' kinds in order.
    const CORPUS: &[(&str, &[BlockKind])] = &[
        ("", &[]),
        ("\n\n  \n", &[]),
        ("One paragraph.", &[BlockKind::Paragraph]),
        (
            "Soft\nbreak, hard  \nbreak and hard\\\nbreak.\n",
            &[BlockKind::Paragraph],
        ),
        (
            "Inline <b>html</b>, [a link](https://x.y \"its title\"), <https://auto.link>, \\*escaped\\*.",
            &[BlockKind::Paragraph],
        ),
        (
            "# One\n\n## Two\n\n###### Six",
            &[
                BlockKind::Heading { level: 1 },
                BlockKind::Heading { level: 2 },
                BlockKind::Heading { level: 6 },
            ],
        ),
        (
            "Setext one\n===\n\nSetext two\n---",
            &[
                BlockKind::Heading { level: 1 },
                BlockKind::Heading { level: 2 },
            ],
        ),
        (
            "- a\n- b\n  - nested\n    1. deeper\n- c",
            &[BlockKind::List { ordered: false }],
        ),
        (
            "- dash\n\n* star\n\n+ plus",
            &[
                BlockKind::List { ordered: false },
                BlockKind::List { ordered: false },
                BlockKind::List { ordered: false },
            ],
        ),
        ("7. seven\n8. eight", &[BlockKind::List { ordered: true }]),
        (
            "- loose\n\n- list\n\n- items",
            &[BlockKind::List { ordered: false }],
        ),
        (
            "- [ ] open\n- [x] done\n  - [X] nested done",
            &[BlockKind::List { ordered: false }],
        ),
        (
            "> quote\n> > nested\n> - a list\n>\n> ```\n> code\n> ```",
            &[BlockKind::Quote],
        ),
        (
            "```rust\nfn main() {}\n```\n\n~~~\ntilde\n~~~\n\n````\n```\nshorter inside\n```\n````",
            &[
                BlockKind::Code { fenced: true },
                BlockKind::Code { fenced: true },
                BlockKind::Code { fenced: true },
            ],
        ),
        (
            "Text.\n\n    indented code\n    more",
            &[BlockKind::Paragraph, BlockKind::Code { fenced: false }],
        ),
        ("| a | b \\| c |\n|:--|--:|\n| 1 | 2 |", &[BlockKind::Table]),
        (
            "Above\n\n---\n\n***\n\nBelow",
            &[
                BlockKind::Paragraph,
                BlockKind::Rule,
                BlockKind::Rule,
                BlockKind::Paragraph,
            ],
        ),
        ("<div>\nan html block\n</div>", &[BlockKind::Html]),
        (
            "See [the doc][d].\n\n[d]: https://x.y",
            &[BlockKind::Paragraph, BlockKind::Definition],
        ),
        (
            "A note[^1].\n\n[^1]: The footnote.",
            &[BlockKind::Paragraph, BlockKind::Footnote],
        ),
        ("$$\nx^2\n$$", &[BlockKind::Math]),
        (
            "Ünïcödé ✓ — 日本語.\n\n## Émoji 🎉",
            &[BlockKind::Paragraph, BlockKind::Heading { level: 2 }],
        ),
        (
            "Windows\r\nlines.\r\n\r\n# Head\r\n",
            &[BlockKind::Paragraph, BlockKind::Heading { level: 1 }],
        ),
        (
            "\n\nLeading blank lines, trailing too.\n\n\n",
            &[BlockKind::Paragraph],
        ),
    ];

    #[test]
    fn each_case_has_its_blocks() {
        for (source, want) in CORPUS {
            assert_eq!(&kinds(source), want, "{source:?}");
        }
    }

    #[test]
    fn blocks_are_in_order_apart_and_trimmed_of_breaks() {
        for (source, _) in CORPUS {
            let bs = blocks(source);
            let mut at = 0;
            for b in &bs {
                assert!(b.range.start >= at, "{source:?}: overlaps");
                assert!(b.range.end <= source.len());
                let s = &source[b.range.clone()];
                assert!(
                    !s.starts_with(['\n', '\r']) && !s.ends_with(['\n', '\r']),
                    "{s:?}"
                );
                at = b.range.end;
            }
            // Between blocks only blank lines.
            let mut last = 0;
            for b in &bs {
                assert!(source[last..b.range.start].trim().is_empty(), "{source:?}");
                last = b.range.end;
            }
            assert!(source[last..].trim().is_empty(), "{source:?}");
        }
    }

    #[test]
    fn an_unchanged_block_writes_back_byte_for_byte() {
        for (source, _) in CORPUS {
            for b in blocks(source) {
                let text = source[b.range.clone()].to_owned();
                assert_eq!(splice(source, b.range.clone(), &text), *source);
                // With a line break the editor added after it, too.
                assert_eq!(splice(source, b.range, &format!("{text}\n")), *source);
            }
        }
    }

    #[test]
    fn an_edit_changes_only_its_own_block() {
        for (source, _) in CORPUS {
            let bs = blocks(source);
            for (i, b) in bs.iter().enumerate() {
                let out = splice(source, b.range.clone(), "Edited.");
                assert_eq!(&out[..b.range.start], &source[..b.range.start]);
                assert_eq!(&out[b.range.start + 7..], &source[b.range.end..]);
                let after = blocks(&out);
                assert_eq!(after.len(), bs.len(), "{source:?} -> {out:?}");
                assert_eq!(after[i].kind, BlockKind::Paragraph);
            }
        }
    }

    #[test]
    fn an_edit_that_becomes_another_kind_is_that_kind() {
        let source = "First.\n\nSecond.";
        let b = &blocks(source)[1];
        let out = splice(source, b.range.clone(), "## Second");
        assert_eq!(out, "First.\n\n## Second");
        assert_eq!(kinds(&out)[1], BlockKind::Heading { level: 2 });
        // A blank line typed in a block makes two.
        let out = splice(source, b.range.clone(), "Second.\n\nThird.");
        assert_eq!(kinds(&out).len(), 3);
    }

    #[test]
    fn an_emptied_block_leaves_no_gap() {
        let source = "One.\n\nTwo.\n\nThree.\n";
        let bs = blocks(source);
        assert_eq!(splice(source, bs[1].range.clone(), ""), "One.\n\nThree.\n");
        assert_eq!(
            splice(source, bs[0].range.clone(), "  \n"),
            "Two.\n\nThree.\n"
        );
        assert_eq!(splice(source, bs[2].range.clone(), ""), "One.\n\nTwo.");
        assert_eq!(splice("Only.", 0..5, ""), "");
    }

    #[test]
    fn an_emptied_block_takes_only_blank_lines_with_it() {
        // The next block's indent is its own: indented code stays code.
        let source = "A\n\nB\n\n    code\n    more";
        let bs = blocks(source);
        let out = splice(source, bs[1].range.clone(), "");
        assert_eq!(out, "A\n\n    code\n    more");
        assert_eq!(kinds(&out)[1], BlockKind::Code { fenced: false });
        // The last one: the line above keeps its own bytes, trailing spaces too.
        let source = "Hard  \n\nGone";
        let out = splice(source, blocks(source)[1].range.clone(), "");
        assert_eq!(out, "Hard  ");
        let source = "One\r\n\r\nTwo\r\n";
        let out = splice(source, blocks(source)[1].range.clone(), "");
        assert_eq!(out, "One");
    }

    #[test]
    fn a_block_is_added_after_the_last() {
        assert_eq!(append("", "First."), "First.");
        assert_eq!(append("One.\n\n", "Two.\n"), "One.\n\nTwo.");
        assert_eq!(append("One.", "  "), "One.");
    }

    #[test]
    fn a_block_reads_with_the_definitions_it_may_name() {
        let source = "See [the doc][d].\n\nText.\n\n[d]: https://x.y\n[e]: https://e.f";
        let bs = blocks(source);
        assert_eq!(context(source, &bs), "[d]: https://x.y\n[e]: https://e.f");
    }

    #[test]
    fn task_boxes_tick_and_untick_in_place() {
        let source = "- [ ] open\n- [x] done\n  1. [ ] nested\n* plain\n\n> - [X] quoted";
        let boxes = task_boxes(source);
        assert_eq!(
            boxes.iter().map(|b| b.checked).collect::<Vec<_>>(),
            [false, true, false, true]
        );
        for b in &boxes {
            assert!(matches!(&source[b.mark..=b.mark], " " | "x" | "X"));
        }
        let ticked = toggle_task(source, boxes[0].mark).unwrap();
        assert!(ticked.starts_with("- [x] open\n"));
        assert_eq!(ticked.len(), source.len());
        let unticked = toggle_task(source, boxes[3].mark).unwrap();
        assert!(unticked.ends_with("> - [ ] quoted"));
        assert_eq!(toggle_task(source, 0), None);
    }

    #[test]
    fn a_list_reads_item_by_item() {
        let source = "- [ ] open **one**\n  carried on\n- [x] done\n  - nested\n- plain";
        let items = list_items(source);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].marker, "-");
        assert_eq!(
            items[0].task,
            Some(TaskBox {
                mark: 3,
                checked: false
            })
        );
        assert_eq!(items[0].content, "open **one**\ncarried on");
        assert_eq!(items[1].content, "done\n- nested");
        assert!(items[1].task.unwrap().checked);
        assert_eq!(
            &source[items[1].task.unwrap().mark..=items[1].task.unwrap().mark],
            "x"
        );
        assert_eq!(items[2].task, None);
        assert_eq!(items[2].content, "plain");
        let ordered = list_items("3. [ ] three\n4. four");
        assert_eq!(ordered[0].marker, "3.");
        assert_eq!(ordered[0].content, "three");
        assert!(has_task_box("- [ ] a") && !has_task_box("- a") && !has_task_box("text"));
    }

    proptest::proptest! {
        /// Whatever the source, its blocks are in it, in order, and each writes back unchanged.
        #[test]
        fn any_source_splits_and_writes_back(source in "[-#>*`|~ \\[\\]x0-9a-z\n]{0,200}") {
            let bs = blocks(&source);
            let mut at = 0;
            for b in &bs {
                proptest::prop_assert!(b.range.start >= at && b.range.end <= source.len());
                at = b.range.end;
                let text = source[b.range.clone()].to_owned();
                proptest::prop_assert_eq!(splice(&source, b.range.clone(), &text), source.clone());
            }
        }
    }
}
