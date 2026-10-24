//! Checks the AGENTS.md writing style rules that a program can check.
//! The text is every tracked Markdown file and every comment line in a tracked source file.
//! Each line of text holds one sentence, and ends where the sentence ends.
//! A Markdown sentence has at most 100 read characters: the text and code a reader sees.
//! A comment line has at most 100 columns, counting its indentation and comment marker.
//! Table rows, code blocks, and HTML are not text in this sense.

use std::path::{Path, PathBuf};
use std::process::Command;

mod vocabulary;

use anyhow::{bail, Result};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

const MAX_LENGTH: usize = 100;

/// One line of text: the length the bound applies to, and the text the sentence checks read.
/// That text masks each code span to `` `x` ``, so a period inside code never ends a sentence.
/// A `fragment` is a line that is no sentence by form: a heading, a metadata line, `[!NOTE]`.
/// An `item` line starts a list item, so the line before it was not wrapped into it.
#[derive(Debug, Default, PartialEq)]
struct TextLine {
    number: usize,
    length: usize,
    text: String,
    fragment: bool,
    item: bool,
}

impl TextLine {
    /// Whether this line is the rest of a sentence the line before it did not finish.
    fn continues(&self, previous: &TextLine) -> bool {
        self.number == previous.number + 1
            && !self.item
            && self
                .text
                .trim_start()
                .starts_with(|c: char| c.is_ascii_lowercase() || c == '(' || c == '`')
    }
}

/// The comment markers of a source file, by extension; `None` for a file the check does not read.
pub(crate) fn comment_markers(path: &str) -> Option<&'static [&'static str]> {
    let ext = path.rsplit_once('.')?.1;
    Some(match ext {
        "rs" | "go" => &["//"],
        "java" | "c" | "h" | "js" => &["//", "/*", "*"],
        "css" => &["/*", "*"],
        "rb" | "py" | "sh" | "pl" | "codon" | "toml" | "yml" => &["#"],
        "wat" => &[";;"],
        _ => return None,
    })
}

/// Each line of Markdown `text` that holds text a reader sees.
/// Table rows are read only with `read_tables`.
/// The length bound does not apply to them, and the vocabulary does.
pub(crate) fn markdown_lines(text: &str, read_tables: bool) -> Vec<TextLine> {
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let line_of = |offset: usize| line_starts.partition_point(|&start| start <= offset) - 1;
    let mut lines: Vec<TextLine> = (0..line_starts.len())
        .map(|index| TextLine {
            number: index + 1,
            ..TextLine::default()
        })
        .collect();
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let mut skipped_depth = 0;
    // A heading and the text of an image are no sentence by form.
    let mut in_heading = false;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => skipped_depth += 1,
            Event::End(TagEnd::CodeBlock | TagEnd::MetadataBlock(_)) => skipped_depth -= 1,
            Event::Start(Tag::Table(_)) if !read_tables => skipped_depth += 1,
            Event::End(TagEnd::Table) if !read_tables => skipped_depth -= 1,
            Event::Start(Tag::Heading { .. } | Tag::Image { .. }) => in_heading = true,
            Event::End(TagEnd::Heading(_) | TagEnd::Image) => in_heading = false,
            Event::Start(Tag::Item) => lines[line_of(range.start)].item = true,
            _ if skipped_depth > 0 => {}
            // Cells are separate, so a word never runs across a cell border.
            Event::End(TagEnd::TableCell) => append(&mut lines[line_of(range.start)], "", " "),
            Event::Start(Tag::MetadataBlock(_)) => {
                // A `MetadataBlock` is YAML, so each of its lines is read as written.
                let block = &text[range.clone()];
                let first = line_of(range.start);
                for (index, raw) in block.lines().enumerate().skip(1) {
                    let raw = raw.trim();
                    if raw != "---" {
                        let line = &mut lines[first + index];
                        line.length = raw.chars().count();
                        line.text = raw.to_owned();
                        line.fragment = true;
                    }
                }
                skipped_depth += 1;
            }
            Event::Text(read) => {
                let mut start = range.start;
                for piece in read.split('\n') {
                    let line = &mut lines[line_of(start)];
                    append(line, piece, piece);
                    line.fragment |= in_heading;
                    start = text[start..].find('\n').map_or(start, |i| start + i + 1);
                }
            }
            Event::Code(code) => {
                let masked = format!("`{}`", "x".repeat(code.chars().count()));
                let line = &mut lines[line_of(range.start)];
                append(line, &code, &masked);
                line.fragment |= in_heading;
            }
            _ => {}
        }
    }
    lines.retain(|line| !line.text.is_empty());
    for line in &mut lines {
        line.fragment |= is_alert_mark(line.text.trim()) || is_no_sentence(&line.text);
    }
    lines
}

/// Whether `text` is no sentence by its form.
/// That is a line with no letter, with columns two spaces apart, or with a `---` rule.
fn is_no_sentence(text: &str) -> bool {
    let text = text.trim();
    !text.contains(char::is_alphabetic)
        || text.contains("  ")
        || text.starts_with("---")
        || text.ends_with("---")
}

fn append(line: &mut TextLine, read: &str, masked: &str) {
    line.length += read.chars().count();
    line.text.push_str(masked);
}

/// Whether `text` is a GitHub note mark such as `[!NOTE]`.
fn is_alert_mark(text: &str) -> bool {
    text.starts_with("[!") && text.ends_with(']')
}

/// Whether `text` ends where a sentence does, or where an item of a list does (`;`).
/// A closing mark after it is allowed.
fn ends_sentence(text: &str) -> bool {
    text.trim_end()
        .trim_end_matches([')', ']', '"', '\'', '*', '_'])
        .ends_with(['.', ':', '?', '!', ';'])
}

/// Whether `content` is a Markdown link definition, as in "[`Backend`]: dewasm_backend::Backend".
fn is_link_definition(content: &str) -> bool {
    content.starts_with('[') && content.contains("]: ")
}

/// `text` without the target of each Markdown link, which a reader does not see.
fn without_link_targets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == ']' && chars.peek() == Some(&'(') {
            for inner in chars.by_ref() {
                if inner == ')' {
                    break;
                }
            }
        }
    }
    out
}

/// Directives are comments a program reads, not a person, so they are not text.
/// A unit's `# requires:` header is one, as are the lint, encoding, and build directives.
/// A C `#define` line inside a Ruby string starts with `#` too.
fn is_directive(content: &str) -> bool {
    const DIRECTIVES: &[&str] = &[
        "define ",
        "include ",
        "ifdef ",
        "ifndef ",
        "endif",
        "undef ",
        "requires:",
        "shellcheck ",
        "SPDX-",
        "go:",
        "frozen_string_literal:",
        "-*-",
        "noqa",
        "type: ignore",
        "pylint:",
        "rubocop:",
        "NOLINT",
    ];
    DIRECTIVES.iter().any(|d| content.starts_with(d))
        || (content.starts_with("line ") && content.contains(':'))
}

pub(crate) fn comment_lines(markers: &[&str], text: &str) -> Vec<TextLine> {
    let mut in_fence = false;
    // Inside a `/* ... */` block, where a line may start with `*` instead of a marker.
    let mut in_block = false;
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let was_in_block = in_block;
        if markers.contains(&"/*") {
            if was_in_block {
                in_block = !trimmed.contains("*/");
            } else if let Some(after) = trimmed.strip_prefix("/*") {
                in_block = !after.contains("*/");
            }
        }
        if trimmed.starts_with("#!") || trimmed.starts_with("#[") {
            continue;
        }
        // A line that starts with `*` outside a block is code, such as a pointer dereference.
        let rest = if was_in_block {
            Some(trimmed)
        } else {
            markers
                .iter()
                .filter(|m| **m != "*")
                .find_map(|m| trimmed.strip_prefix(m))
        };
        let Some(rest) = rest else {
            continue;
        };
        // A doc comment is Markdown, so `# Examples` after the marker is a heading.
        let doc_heading = rest
            .trim_start_matches(['/', '!', '*', ';'])
            .trim_start()
            .starts_with("# ");
        let content = rest
            .trim_start_matches(['/', '!', '*', '#', ';'])
            .trim_start();
        let content = content.trim_end().trim_end_matches("*/").trim_end();
        if content.is_empty() {
            continue;
        }
        if is_directive(content) {
            continue;
        }
        if content.starts_with("```") || content.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        // A doc comment is Markdown, so a link definition names a target, not a sentence.
        if in_fence || content.starts_with('|') || is_link_definition(content) {
            continue;
        }
        out.push(TextLine {
            number: index + 1,
            length: line.chars().count(),
            text: without_link_targets(&mask_code_spans(content)),
            fragment: doc_heading || is_no_sentence(content),
            item: starts_list_item(content),
        });
    }
    out
}

fn mask_code_spans(content: &str) -> String {
    let mut in_code = false;
    content
        .chars()
        .map(|c| {
            if c == '`' {
                in_code = !in_code;
                c
            } else if in_code {
                'x'
            } else {
                c
            }
        })
        .collect()
}

/// Whether a sentence ends mid-line and another starts after it.
/// A short form such as "e.g." and a leading ordered-list number do not end a sentence.
fn holds_two_sentences(text: &str) -> bool {
    let text = strip_list_number(text);
    let chars: Vec<char> = text.chars().collect();
    for i in 1..chars.len().saturating_sub(2) {
        if !matches!(chars[i], '.' | '?' | '!') || chars[i + 1] != ' ' {
            continue;
        }
        let before = chars[i - 1];
        if !(before.is_ascii_lowercase() || before.is_ascii_digit() || "\"`)]*".contains(before)) {
            continue;
        }
        let after = chars[i + 1..].iter().find(|c| **c != ' ').copied();
        let starts_sentence = after.is_some_and(|c| c.is_ascii_uppercase() || "\"`([*".contains(c));
        if starts_sentence && !ends_with_abbreviation(&chars[..=i]) {
            return true;
        }
    }
    false
}

const ABBREVIATIONS: &[&str] = &["e.g.", "i.e.", "vs.", "etc.", "cf.", "approx.", "No."];

fn ends_with_abbreviation(prefix: &[char]) -> bool {
    let prefix: String = prefix.iter().collect();
    ABBREVIATIONS.iter().any(|abbr| {
        prefix.strip_suffix(abbr).is_some_and(|head| {
            head.chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric())
        })
    })
}

/// Whether a comment line starts a Markdown list item: `- `, `* `, or `1. `.
fn starts_list_item(content: &str) -> bool {
    content.starts_with("- ") || content.starts_with("* ") || strip_list_number(content) != content
}

fn strip_list_number(text: &str) -> &str {
    let unmarked = text.trim_start_matches(['*', '#', ' ']);
    let digits = unmarked.len()
        - unmarked
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .len();
    match unmarked[digits..].strip_prefix(". ") {
        Some(rest) if digits > 0 => rest,
        _ => text,
    }
}

/// The tracked files the checks read: Markdown, and source files with known comment markers.
fn tracked_files(root: &Path) -> Vec<String> {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .expect("the text check lists tracked files with `git ls-files`");
    assert!(output.status.success(), "`git ls-files` failed");
    String::from_utf8(output.stdout)
        .expect("tracked paths are UTF-8")
        .split('\0')
        .filter(|path| is_checked_text(path))
        .map(str::to_owned)
        .collect()
}

/// Tracked files another program writes, text included.
/// Active Record rewrites this one with its own header on every schema dump.
const WRITTEN_BY_A_PROGRAM: &[&str] = &["examples/rails/db/schema.rb"];

/// Whether the checks read `path`.
fn is_checked_text(path: &str) -> bool {
    !WRITTEN_BY_A_PROGRAM.contains(&path)
        && (path.ends_with(".md") || comment_markers(path).is_some())
}

/// Each defect of the text file `path` under `root`, as `path:line: what`.
fn file_defects(root: &Path, path: &str) -> Vec<String> {
    let text = std::fs::read_to_string(root.join(path))
        .unwrap_or_else(|e| panic!("{path} is tracked text and must read as UTF-8: {e}"));
    let (lines, unit) = match comment_markers(path) {
        Some(markers) => (comment_lines(markers, &text), "columns"),
        None => (markdown_lines(&text, false), "read characters"),
    };
    let mut report = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.length > MAX_LENGTH {
            report.push(format!("{path}:{}: {} {unit}", line.number, line.length));
        }
        if holds_two_sentences(&line.text) {
            report.push(format!("{path}:{}: two sentences", line.number));
        }
        let wrapped = !line.fragment
            && !ends_sentence(&line.text)
            && lines
                .get(index + 1)
                .is_some_and(|next| next.continues(line));
        if wrapped {
            report.push(format!("{path}:{}: wrapped sentence", line.number));
        }
    }
    report
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `check-text [path]...`: prints every defect in the named files, or in every tracked text file.
pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let root = repo_root();
    let mut paths: Vec<String> = args.collect();
    let all = paths.is_empty();
    if all {
        paths = tracked_files(&root);
    }
    let vocabulary = vocabulary::Vocabulary::load(&root).map_err(anyhow::Error::msg)?;
    let mut count = 0;
    for path in &paths {
        if !is_checked_text(path) {
            bail!("{path} is neither Markdown nor a source file with known comment markers");
        }
        let mut defects = file_defects(&root, path);
        defects.extend(vocabulary.file_defects(&root, path));
        for defect in defects {
            println!("{defect}");
            count += 1;
        }
    }
    // Whether a table word is used shows only after every file was read.
    if all {
        for defect in vocabulary.table_defects() {
            println!("{defect}");
            count += 1;
        }
    }
    if count > 0 {
        bail!("{count} text defects");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_meets_the_writing_style() {
        let root = repo_root();
        let report: Vec<String> = tracked_files(&root)
            .iter()
            .flat_map(|path| file_defects(&root, path))
            .collect();
        assert!(
            report.is_empty(),
            "text breaks the AGENTS.md writing style (100 characters, one sentence per line):\n{}\n\
             Run `cargo xtask check-text <path>` to check a file again.",
            report.join("\n")
        );
    }

    #[test]
    fn text_uses_the_vocabulary() {
        let root = repo_root();
        let vocabulary = vocabulary::Vocabulary::load(&root).unwrap_or_else(|e| panic!("{e}"));
        let mut report: Vec<String> = tracked_files(&root)
            .iter()
            .flat_map(|path| vocabulary.file_defects(&root, path))
            .collect();
        report.extend(vocabulary.table_defects());
        assert!(
            report.is_empty(),
            "text and agents/vocabulary.md disagree:\n{}\n\
             Run `cargo xtask check-text <path>` to check a file again.",
            report.join("\n")
        );
    }

    #[test]
    fn two_sentences_are_found() {
        assert!(holds_two_sentences("One thing. Another thing."));
        assert!(holds_two_sentences("It ends in code `x`. Then more."));
        assert!(!holds_two_sentences("A list, e.g. Ruby and Python."));
        assert!(!holds_two_sentences("Ruby vs. Python."));
        assert!(!holds_two_sentences("2. A library-mode name is kept."));
        assert!(!holds_two_sentences("## 3. Library mode: call the exports"));
    }

    #[test]
    fn a_line_ends_where_its_sentence_does() {
        assert!(ends_sentence("One sentence."));
        assert!(ends_sentence("Two parts (decision 98)."));
        assert!(ends_sentence("The commands are:"));
        assert!(!ends_sentence("A sentence wrapped after a"));
        assert!(!ends_sentence("A sentence ending in `xxxx`"));
        let md = "# A heading\n\nA line\nwrapped.\n\n> [!NOTE]\n> One note.\n- Tab\n- `q`: exit\n";
        let lines = markdown_lines(md, false);
        let shape: Vec<(usize, bool, bool)> = lines
            .iter()
            .map(|l| (l.number, l.fragment, l.item))
            .collect();
        let expected = [
            (1, true, false),
            (3, false, false),
            (4, false, false),
            (6, true, false),
            (7, false, false),
            (8, false, true),
            (9, false, true),
        ];
        assert_eq!(shape, expected);
        assert!(lines[2].continues(&lines[1]));
        assert!(!lines[6].continues(&lines[5]));
        let rust = "/// # Examples\n/// A doc line.\n/// - an item\n";
        let shape: Vec<(bool, bool)> = comment_lines(&["//"], rust)
            .iter()
            .map(|l| (l.fragment, l.item))
            .collect();
        assert_eq!(shape, [(true, false), (false, false), (false, true)]);
    }

    fn texts(lines: Vec<TextLine>) -> Vec<(usize, usize, String)> {
        lines
            .into_iter()
            .map(|l| (l.number, l.length, l.text))
            .collect()
    }

    #[test]
    fn markdown_counts_what_a_reader_sees() {
        let md = "A [link](https://example.com/a/long/path) and `a.B` and *em*.\n\
                  - Item with with_units.\n\
                  <img alt=\"a. B\">\n\
                  \n\
                  | a. B | c |\n\
                  | --- | --- |\n\
                  \n\
                  ```\n\
                  long. Code\n\
                  ```\n";
        let expected = vec![
            (1, 22, "A link and `xxx` and em.".to_owned()),
            (2, 21, "Item with with_units.".to_owned()),
        ];
        assert_eq!(texts(markdown_lines(md, false)), expected);
    }

    #[test]
    fn doc_comment_link_targets_are_not_text() {
        let rust = "/// See [`run`](crate::run_all).\n/// [`Backend`]: dewasm_backend::Backend\n";
        let texts: Vec<String> = comment_lines(&["//"], rust)
            .into_iter()
            .map(|l| l.text)
            .collect();
        assert_eq!(texts, ["See [`xxx`]."]);
    }

    #[test]
    fn block_comments_are_read_and_code_is_not() {
        let c = "/* One sentence. */\n/*\n * Inside a block.\n */\n*--p = (char)v;\n";
        let texts: Vec<String> = comment_lines(&["//", "/*", "*"], c)
            .into_iter()
            .map(|l| l.text)
            .collect();
        assert_eq!(texts, ["One sentence.", "Inside a block."]);
    }

    #[test]
    fn a_shell_glob_opens_no_block() {
        let sh = "case $p in\n  /*) echo abs ;;\n  # A comment.\nesac\n";
        let texts: Vec<String> = comment_lines(&["#"], sh)
            .into_iter()
            .map(|l| l.text)
            .collect();
        assert_eq!(texts, ["A comment."]);
    }

    #[test]
    fn directives_are_not_text() {
        let script = "# shellcheck disable=SC2034\n# requires: rt/trap\n# A comment.\n";
        let go = "//go:build linux\n//line gen.go:3\n// A comment.\n";
        let texts = |lines: Vec<TextLine>| lines.into_iter().map(|l| l.text).collect::<Vec<_>>();
        assert_eq!(texts(comment_lines(&["#"], script)), ["A comment."]);
        assert_eq!(texts(comment_lines(&["//"], go)), ["A comment."]);
    }

    #[test]
    fn comments_count_columns() {
        let rust = "fn f() {}\n/// A `a. B` doc line.\n    // A comment.\n#[test]\n";
        let expected = vec![
            (2, 22, "A `xxxx` doc line.".to_owned()),
            (3, 17, "A comment.".to_owned()),
        ];
        assert_eq!(texts(comment_lines(&["//"], rust)), expected);
    }
}
