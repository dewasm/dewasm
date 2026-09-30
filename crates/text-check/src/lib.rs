//! Checks the AGENTS.md writing style rules that a program can check.
//! The text is every tracked Markdown file and every comment line in a tracked source file.
//! Each line of text holds one sentence.
//! A Markdown sentence has at most 100 read characters: the text and code a reader sees.
//! A comment line has at most 100 columns, counting its indentation and comment marker.
//! Table rows, code blocks, and HTML are not text in this sense.

use std::path::Path;
use std::process::Command;

pub mod vocabulary;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

const MAX_LENGTH: usize = 100;

/// One line of text: the length the bound applies to, and the text the sentence check reads.
/// That text masks each code span to `` `x` ``, so a period inside code never ends a sentence.
#[derive(Debug, Default, PartialEq)]
struct TextLine {
    number: usize,
    length: usize,
    text: String,
}

/// The comment markers of a source file, by extension; `None` for a file the check does not read.
pub(crate) fn comment_markers(path: &str) -> Option<&'static [&'static str]> {
    let ext = path.rsplit_once('.')?.1;
    Some(match ext {
        "rs" | "go" => &["//"],
        "java" => &["//", "/*", "*"],
        "rb" | "py" | "sh" | "pl" | "codon" | "toml" | "yml" => &["#"],
        "wat" => &[";;"],
        _ => return None,
    })
}

/// Each line of Markdown `text` that holds text a reader sees.
/// Table rows are read only with `read_tables`.
/// The length bound exempts them, and the vocabulary does not.
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
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => skipped_depth += 1,
            Event::End(TagEnd::CodeBlock | TagEnd::MetadataBlock(_)) => skipped_depth -= 1,
            Event::Start(Tag::Table(_)) if !read_tables => skipped_depth += 1,
            Event::End(TagEnd::Table) if !read_tables => skipped_depth -= 1,
            _ if skipped_depth > 0 => {}
            // Cells are separate, so a word never runs across a cell border.
            Event::End(TagEnd::TableCell) => append(&mut lines[line_of(range.start)], "", " "),
            Event::Start(Tag::MetadataBlock(_)) => {
                // A metadata block is YAML, so each of its lines is read as written.
                let block = &text[range.clone()];
                let first = line_of(range.start);
                for (index, raw) in block.lines().enumerate().skip(1) {
                    let raw = raw.trim();
                    if raw != "---" {
                        let line = &mut lines[first + index];
                        line.length = raw.chars().count();
                        line.text = raw.to_owned();
                    }
                }
                skipped_depth += 1;
            }
            Event::Text(read) => {
                let mut start = range.start;
                for piece in read.split('\n') {
                    append(&mut lines[line_of(start)], piece, piece);
                    start = text[start..].find('\n').map_or(start, |i| start + i + 1);
                }
            }
            Event::Code(code) => {
                let masked = format!("`{}`", "x".repeat(code.chars().count()));
                append(&mut lines[line_of(range.start)], &code, &masked);
            }
            _ => {}
        }
    }
    lines.retain(|line| !line.text.is_empty());
    lines
}

fn append(line: &mut TextLine, read: &str, masked: &str) {
    line.length += read.chars().count();
    line.text.push_str(masked);
}

/// Directives are comments a program reads, not a person, so they are not text.
/// A unit's `# requires:` header is one, as are the lint, encoding, and build directives.
fn is_directive(content: &str) -> bool {
    const DIRECTIVES: &[&str] = &[
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
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#!") || trimmed.starts_with("#[") {
            continue;
        }
        let Some(rest) = markers.iter().find_map(|m| trimmed.strip_prefix(m)) else {
            continue;
        };
        let content = rest
            .trim_start_matches(['/', '!', '*', '#', ';'])
            .trim_start();
        if is_directive(content) {
            continue;
        }
        if content.starts_with("```") || content.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || content.starts_with('|') {
            continue;
        }
        out.push(TextLine {
            number: index + 1,
            length: line.chars().count(),
            text: mask_code_spans(content),
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
/// An abbreviation such as "e.g." and a leading ordered-list number do not end a sentence.
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
pub fn tracked_files(root: &Path) -> Vec<String> {
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

/// Whether the checks read `path`.
pub fn is_checked_text(path: &str) -> bool {
    path.ends_with(".md") || comment_markers(path).is_some()
}

/// Each defect of the text file `path` under `root`, as `path:line: what`.
pub fn file_defects(root: &Path, path: &str) -> Vec<String> {
    let text = std::fs::read_to_string(root.join(path))
        .unwrap_or_else(|e| panic!("{path} is tracked text and must read as UTF-8: {e}"));
    let (lines, unit) = match comment_markers(path) {
        Some(markers) => (comment_lines(markers, &text), "columns"),
        None => (markdown_lines(&text, false), "read characters"),
    };
    let mut report = Vec::new();
    for line in lines {
        if line.length > MAX_LENGTH {
            report.push(format!("{path}:{}: {} {unit}", line.number, line.length));
        }
        if holds_two_sentences(&line.text) {
            report.push(format!("{path}:{}: two sentences", line.number));
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

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

    /// Files whose comments predate the vocabulary rules, which the check skips until rewritten.
    /// A listed file that already passes fails the check, so the list only shrinks.
    const VOCABULARY_UNCHECKED: &str = include_str!("vocabulary_unchecked.txt");

    #[test]
    fn text_uses_the_vocabulary() {
        let root = repo_root();
        let vocabulary = vocabulary::Vocabulary::load(&root).unwrap_or_else(|e| panic!("{e}"));
        let unchecked: Vec<&str> = VOCABULARY_UNCHECKED
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        let mut report = Vec::new();
        let mut stale = Vec::new();
        for path in tracked_files(&root) {
            let found = vocabulary.file_defects(&root, &path);
            if !unchecked.contains(&path.as_str()) {
                report.extend(found);
            } else if found.is_empty() {
                stale.push(path);
            }
        }
        assert!(
            stale.is_empty(),
            "these files now pass; remove them from vocabulary_unchecked.txt:\n{}",
            stale.join("\n")
        );
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
