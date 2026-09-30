//! The vocabulary rules of `agents/vocabulary.md`, checked over Markdown.
//! A word in a sentence comes from an allowed source, and no excluded word is used.
//! The tables of `agents/vocabulary.md` and the fetched base lists are read as they are.
//! The one list of words here is the number words, which the "Number" source allows.

use std::collections::HashSet;
use std::path::Path;

use crate::markdown_lines;

/// The file that holds the vocabulary tables, relative to the repository root.
pub const VOCABULARY: &str = "agents/vocabulary.md";

/// The base lists that `crates/text-check/setup.sh` fetches, relative to the repository root.
const BASE_LISTS: &[&str] = &[
    "crates/text-check/cache/NGSL_12_lemmatized_for_research.csv",
    "crates/text-check/cache/NAWL_12_lemmatized_for_research.csv",
];

/// The "Number" source of `agents/vocabulary.md`: a number written as a word.
/// The base lists leave numbers out, so the words are listed here.
const NUMBER_WORDS: &[&str] = &[
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "hundred",
    "thousand",
    "million",
    "billion",
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "sixth",
    "seventh",
    "eighth",
    "ninth",
    "tenth",
    "twelfth",
    "twentieth",
    "hundredth",
    "thousandth",
    "half",
    "halves",
    "twice",
    "once",
    "dozen",
];

/// One row of an excluded-word table.
#[derive(Debug)]
struct Exclusion {
    forms: Vec<String>,
    exceptions: Vec<String>,
}

pub struct Vocabulary {
    base: HashSet<String>,
    /// The documents about writing, which the "Terms for writing" section names in code spans.
    writing_documents: HashSet<String>,
    /// Each capitalized word that some tracked Markdown uses inside a sentence, where it is a name.
    names: HashSet<String>,
    terms: HashSet<String>,
    writing_terms: HashSet<String>,
    exclusions: Vec<Exclusion>,
}

impl Vocabulary {
    /// Reads `agents/vocabulary.md` and the fetched base lists under `root`.
    pub fn load(root: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(root.join(VOCABULARY))
            .map_err(|e| format!("{VOCABULARY} cannot be read: {e}"))?;
        let mut base = HashSet::new();
        for list in BASE_LISTS {
            let bytes = std::fs::read(root.join(list)).map_err(|e| {
                format!("{list} is not cached ({e}): run crates/text-check/setup.sh")
            })?;
            // NAWL 1.2 is Latin-1; each byte is its own code point.
            let text: String = bytes.iter().map(|&b| b as char).collect();
            for line in text.lines().filter(|l| !l.starts_with("##")) {
                base.extend(line.split(',').map(|f| f.trim().to_lowercase()));
            }
        }
        base.remove("");
        let sections = sections(&text);
        let spans_of = |title: &str| -> HashSet<String> {
            sections
                .iter()
                .filter(|(t, _)| t == title)
                .flat_map(|(_, body)| table_rows(body))
                .flat_map(|row| row.into_iter().flat_map(|cell| code_spans(&cell)))
                .map(|s| s.to_lowercase())
                .collect()
        };
        let mut terms = spans_of("Terms of the field");
        terms.extend(spans_of("Names"));
        let one_meaning: HashSet<String> = sections
            .iter()
            .filter(|(t, _)| t == "Terms with one meaning")
            .flat_map(|(_, body)| table_rows(body))
            .filter_map(|row| row.first().cloned())
            .flat_map(|cell| code_spans(&cell))
            .map(|s| s.to_lowercase())
            .collect();
        terms.extend(one_meaning);
        let writing_terms = spans_of("Terms for writing");
        let writing_documents: HashSet<String> = sections
            .iter()
            .filter(|(t, _)| t == "Terms for writing")
            .flat_map(|(_, body)| body.lines().filter(|l| !l.starts_with('|')))
            .flat_map(code_spans)
            .filter(|span| span.ends_with(".md"))
            .collect();
        if writing_documents.is_empty() {
            return Err(format!("{VOCABULARY} names no document about writing"));
        }
        let mut names = HashSet::new();
        for path in crate::tracked_files(root)
            .iter()
            .filter(|p| p.ends_with(".md"))
        {
            let Ok(text) = std::fs::read_to_string(root.join(path)) else {
                continue;
            };
            for line in markdown_lines(&text, true) {
                let sentence = unquoted(&line.text);
                names.extend(
                    words(&sentence)
                        .into_iter()
                        .filter(|w| !w.first && w.capitalized)
                        .map(|w| w.text),
                );
            }
        }
        let mut exclusions = Vec::new();
        let mut in_excluded = false;
        for (title, body) in &sections {
            if title == "Excluded words" {
                in_excluded = true;
            }
            if !in_excluded {
                continue;
            }
            for row in table_rows(body) {
                let [forms, _, exceptions] = row.as_slice() else {
                    continue;
                };
                exclusions.push(Exclusion {
                    forms: code_spans(forms).iter().map(|s| s.to_lowercase()).collect(),
                    exceptions: code_spans(exceptions)
                        .iter()
                        .map(|s| s.to_lowercase())
                        .collect(),
                });
            }
        }
        if base.is_empty() || terms.is_empty() || exclusions.is_empty() {
            return Err(format!("{VOCABULARY} or a base list has no entries"));
        }
        Ok(Vocabulary {
            base,
            writing_documents,
            names,
            terms,
            writing_terms,
            exclusions,
        })
    }

    /// Each vocabulary defect of the Markdown file `path` under `root`, as `path:line: what`.
    pub fn file_defects(&self, root: &Path, path: &str) -> Vec<String> {
        if !path.ends_with(".md") || path == VOCABULARY {
            return Vec::new();
        }
        let text = std::fs::read_to_string(root.join(path))
            .unwrap_or_else(|e| panic!("{path} is tracked text and must read as UTF-8: {e}"));
        let writing = self.writing_documents.contains(path);
        let mut report = Vec::new();
        for line in markdown_lines(&text, true) {
            let sentence = unquoted(&line.text);
            let excluded = self.excluded_uses(&sentence);
            for form in &excluded {
                report.push(format!("{path}:{}: excluded \"{form}\"", line.number));
            }
            let mut outside: Vec<String> = words(&sentence)
                .into_iter()
                .filter(|w| !self.is_name(w))
                .map(|w| w.text.to_lowercase())
                .filter(|w| !self.allowed(w, writing))
                .filter(|w| {
                    !excluded
                        .iter()
                        .any(|form| form.split([' ', '-']).any(|f| f == w))
                })
                .collect();
            outside.dedup();
            for word in outside {
                report.push(format!(
                    "{path}:{}: outside the lists \"{word}\"",
                    line.number
                ));
            }
        }
        report
    }

    /// Whether `word` is a name, which no list governs.
    /// A word in capitals is one, and so is a capitalized word inside a sentence.
    /// A capitalized first word is one only when some tracked text uses it inside a sentence.
    /// So is a capitalized first word that the next word continues, as in "Oxford Guide".
    /// Otherwise it is an ordinary word that starts the sentence.
    fn is_name(&self, word: &Word) -> bool {
        let all_capitals = word.text.chars().filter(|c| c.is_alphabetic()).count() > 1
            && !word.text.chars().any(|c| c.is_lowercase());
        let known = !word.first || word.next_capitalized || self.names.contains(&word.text);
        all_capitals || (word.capitalized && known)
    }

    fn excluded_uses(&self, sentence: &str) -> Vec<String> {
        let mut lower = sentence.to_lowercase();
        let mut found = Vec::new();
        for exclusion in &self.exclusions {
            for exception in &exclusion.exceptions {
                lower = lower.replace(exception.as_str(), &" ".repeat(exception.len()));
            }
        }
        for exclusion in &self.exclusions {
            for form in &exclusion.forms {
                if contains_word(&lower, form) {
                    found.push(form.clone());
                }
            }
        }
        found
    }

    fn known(&self, word: &str, writing: bool) -> bool {
        self.base.contains(word)
            || self.terms.contains(word)
            || NUMBER_WORDS.contains(&word)
            || (writing && self.writing_terms.contains(word))
    }

    /// Whether `word`, lower case, is allowed.
    /// A hyphenated word is allowed whole, or when each of its parts is.
    /// A part may be a prefix of the "Derived form" row, as in "non-trivial".
    fn allowed(&self, word: &str, writing: bool) -> bool {
        if self.allowed_word(word, writing) {
            return true;
        }
        word.contains('-')
            && word.split('-').all(|part| {
                part.is_empty() || PREFIXES.contains(&part) || self.allowed_word(part, writing)
            })
    }

    /// Whether `word` is a known word, one of its inflections, or a derived form.
    fn allowed_word(&self, word: &str, writing: bool) -> bool {
        inflections(word).any(|w| {
            self.known(&w, writing)
                || derivations(&w).any(|b| {
                    inflections(&b).any(|b1| {
                        self.known(&b1, writing)
                            || derivations(&b1).any(|b2| self.known(&b2, writing))
                    })
                })
        })
    }
}

/// `(title, body)` for each heading of `markdown`, at any level.
fn sections(markdown: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim_start_matches('#');
        if trimmed.len() < line.len() && trimmed.starts_with(' ') {
            out.push((trimmed.trim().to_owned(), String::new()));
        } else if let Some((_, body)) = out.last_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    out
}

/// The cells of each body row of the tables in `body`, without the header and separator rows.
fn table_rows(body: &str) -> Vec<Vec<String>> {
    let rows: Vec<&str> = body.lines().filter(|l| l.starts_with('|')).collect();
    rows.iter()
        .enumerate()
        .filter(|(i, row)| {
            let is_separator = row.chars().all(|c| "|-: ".contains(c));
            let is_header = rows.get(i + 1).is_some_and(|next| next.contains("---"));
            !is_separator && !is_header
        })
        .map(|(_, row)| {
            row.trim()
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_owned())
                .collect()
        })
        .collect()
}

fn code_spans(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// `sentence` without its code spans and its double-quoted mentions.
/// A quote that never closes is not a mention, as in `5" wide`, so its text stays.
fn unquoted(sentence: &str) -> String {
    let mut out = String::with_capacity(sentence.len());
    let mut open: Option<(char, usize)> = None;
    for c in sentence.chars() {
        match open {
            None if "`\"“".contains(c) => {
                let close = if c == '“' { '”' } else { c };
                open = Some((close, out.len()));
                out.push(c);
            }
            Some((close, start)) if c == close => {
                out.truncate(start);
                // A capital letter keeps a suffix after a code span, as in "`foo`ed", off the list.
                out.push('X');
                open = None;
            }
            _ => out.push(c),
        }
    }
    out
}

fn contains_word(text: &str, form: &str) -> bool {
    text.match_indices(form).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + form.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric()) && !after.is_some_and(|c| c.is_alphanumeric())
    })
}

/// A word of a sentence, as written.
#[derive(Debug, PartialEq)]
struct Word {
    text: String,
    /// The word starts the sentence, where every word is capitalized.
    first: bool,
    capitalized: bool,
    /// The next word of the sentence is capitalized too.
    next_capitalized: bool,
}

/// The words of `sentence` that the vocabulary governs, as written.
/// A token with a digit, a one-letter token, or a token with an inner period is not a word.
/// Those are a list marker such as "(b)", a file name, or "e.g.".
/// A hyphenated word stays whole, so a hyphenated name can be listed.
/// A word with `_` stays whole too: it is an identifier outside a code span.
fn words(sentence: &str) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::new();
    let mut first = true;
    for token in sentence.split(|c: char| !(c.is_alphanumeric() || ".'’-_".contains(c))) {
        let token = token.trim_matches(['.', '\'', '’', '-', '_']);
        if token.is_empty() {
            continue;
        }
        let was_first = std::mem::replace(&mut first, false);
        if token.contains('.') || token.chars().count() < 2 {
            continue;
        }
        let parts: Vec<String> = token.split('-').map(without_contraction).collect();
        let text = parts.join("-");
        if !text
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == '-' || c == '_')
        {
            continue;
        }
        let capitalized = text.starts_with(|c: char| c.is_ascii_uppercase());
        if let Some(previous) = out.last_mut() {
            previous.next_capitalized = capitalized;
        }
        out.push(Word {
            text,
            first: was_first,
            capitalized,
            next_capitalized: false,
        });
    }
    out
}

/// `token` without an English contraction: "don't" is "do", and "it's" is "it".
fn without_contraction(token: &str) -> String {
    let normalized = token.replace('’', "'");
    let lower = normalized.to_lowercase();
    match lower.as_str() {
        "can't" => return "can".to_owned(),
        "won't" => return "will".to_owned(),
        _ => {}
    }
    if let Some(base) = normalized.strip_suffix("n't") {
        return base.to_owned();
    }
    normalized.split('\'').next().unwrap_or("").to_owned()
}

/// `word` and the stems its inflectional endings could have come from.
fn inflections(word: &str) -> impl Iterator<Item = String> + '_ {
    const ENDINGS: &[(&str, &[&str])] = &[
        ("ies", &["y"]),
        ("es", &["", "e"]),
        ("s", &[""]),
        ("ied", &["y"]),
        ("ed", &["", "e"]),
        ("ing", &["", "e"]),
    ];
    std::iter::once(word.to_owned()).chain(ENDINGS.iter().flat_map(move |(ending, stems)| {
        let base = word
            .strip_suffix(ending)
            .filter(|b| b.len() >= 3)
            .map(str::to_owned);
        stems.iter().flat_map(move |stem| {
            base.iter()
                .flat_map(|b| {
                    let full = format!("{b}{stem}");
                    let mut last_two = full.chars().rev().take(2);
                    let doubled = last_two.next().is_some() && {
                        let c = full.chars().next_back();
                        c == last_two.next()
                    };
                    let undoubled = doubled.then(|| full[..full.len() - 1].to_owned());
                    std::iter::once(full).chain(undoubled)
                })
                .collect::<Vec<_>>()
        })
    }))
}

/// The prefixes of the "Derived form" row of `agents/vocabulary.md`.
const PREFIXES: &[&str] = &["un", "re", "mis", "non"];

/// The words `word` could be derived from under the "Derived form" row of the vocabulary.
fn derivations(word: &str) -> impl Iterator<Item = String> + '_ {
    const SUFFIXES: &[(&str, &[&str])] = &[
        ("ly", &["", "e"]),
        ("ily", &["y"]),
        ("bly", &["ble"]),
        ("ally", &[""]),
        ("ness", &[""]),
        ("iness", &["y"]),
        ("er", &["", "e"]),
        ("ier", &["y"]),
        ("able", &["", "e"]),
        ("iable", &["y"]),
        ("ion", &["", "e"]),
        ("sion", &["t", "d", "de"]),
        ("ation", &["", "e"]),
        ("ication", &["y"]),
        ("ity", &["", "e"]),
        ("ibility", &["ible"]),
        ("ability", &["able", "", "e"]),
        ("ment", &["", "e"]),
        ("al", &["", "e"]),
        ("en", &["", "e"]),
    ];
    let prefixed = PREFIXES.iter().filter_map(move |p| {
        word.strip_prefix(p)
            .filter(|rest| rest.len() >= 3)
            .map(str::to_owned)
    });
    let suffixed = SUFFIXES.iter().flat_map(move |(suffix, stems)| {
        let base = word
            .strip_suffix(suffix)
            .filter(|b| b.len() >= 2)
            .map(str::to_owned);
        // A doubled final consonant comes from the suffix: "runner" is "run" + "er".
        let undoubled = base.as_ref().and_then(|b| {
            let mut last = b.chars().rev();
            let (c, d) = (last.next()?, last.next()?);
            (c == d && !"aeiou".contains(c)).then(|| b[..b.len() - 1].to_owned())
        });
        stems
            .iter()
            .filter_map(move |stem| base.as_ref().map(|b| format!("{b}{stem}")))
            .chain(undoubled)
    });
    prefixed.chain(suffixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocabulary() -> Vocabulary {
        let set = |words: &[&str]| words.iter().map(|w| w.to_string()).collect();
        Vocabulary {
            base: set(&[
                "run", "check", "fast", "use", "happy", "a", "the", "is", "gate", "wrap", "wide",
                "rely", "verify", "trivial", "do", "list",
            ]),
            writing_documents: set(&["AGENTS.md"]),
            names: set(&["Wasmtime"]),
            terms: set(&["backend", "wasi-sdk"]),
            writing_terms: set(&["idiom"]),
            exclusions: vec![
                Exclusion {
                    forms: vec!["gate".to_owned(), "gates".to_owned()],
                    exceptions: vec![],
                },
                Exclusion {
                    forms: vec!["sweep".to_owned()],
                    exceptions: vec!["mark and sweep".to_owned()],
                },
            ],
        }
    }

    fn texts(sentence: &str) -> Vec<String> {
        words(&unquoted(sentence))
            .into_iter()
            .map(|w| w.text)
            .collect()
    }

    fn outside(v: &Vocabulary, sentence: &str) -> Vec<String> {
        words(&unquoted(sentence))
            .into_iter()
            .filter(|w| !v.is_name(w))
            .map(|w| w.text.to_lowercase())
            .filter(|w| !v.allowed(w, false))
            .collect()
    }

    #[test]
    fn inflected_and_derived_forms_are_allowed() {
        let v = vocabulary();
        for word in [
            "runs",
            "running",
            "checked",
            "backends",
            "unhappiness",
            "rerun",
            "usable",
        ] {
            assert!(v.allowed(word, false), "{word}");
        }
        for word in ["gizmo", "idiom", "fd_read", "gizmo-sdk"] {
            assert!(!v.allowed(word, false), "{word}");
        }
        assert!(v.allowed("idiom", true));
    }

    #[test]
    fn numbers_code_and_mentions_are_not_words() {
        let sentence =
            "Wasmtime runs `gizmo`ed 3 times, e.g. AGENTS.md says \"gizmo\" in (b) WASI; don't.";
        assert_eq!(
            texts(sentence),
            ["Wasmtime", "runs", "Xed", "times", "says", "in", "WASI", "do"]
        );
        assert_eq!(
            texts("Calls fd_read and non-trivial don't-care work."),
            ["Calls", "fd_read", "and", "non-trivial", "do-care", "work"]
        );
    }

    #[test]
    fn a_first_word_is_checked_unless_it_is_a_known_name() {
        let v = vocabulary();
        assert_eq!(outside(&v, "Curate the list."), ["curate"]);
        assert_eq!(outside(&v, "Gizmos run."), ["gizmos"]);
        assert!(outside(&v, "Wasmtime runs the Gizmo check fast.").is_empty());
        assert!(outside(&v, "Run the WASI check.").is_empty());
        assert!(outside(&v, "Oxford Guide is run.").is_empty());
    }

    #[test]
    fn an_unclosed_quote_hides_nothing() {
        let v = vocabulary();
        assert_eq!(
            outside(&v, "The board is 5\" wide and we curate it."),
            ["board", "and", "we", "curate", "it"]
        );
    }

    #[test]
    fn table_cells_are_read() {
        let v = vocabulary();
        let lines = markdown_lines("| a | b |\n| --- | --- |\n| run | a gizmo |\n", true);
        let found: Vec<String> = lines.iter().flat_map(|l| outside(&v, &l.text)).collect();
        assert_eq!(found, ["gizmo"]);
    }

    #[test]
    fn excluded_forms_match_whole_words_outside_exceptions() {
        let v = vocabulary();
        assert_eq!(v.excluded_uses("A gate is a check."), ["gate"]);
        assert!(v.excluded_uses("Propagates run.").is_empty());
        assert!(v.excluded_uses("A mark and sweep collector.").is_empty());
        assert!(v
            .excluded_uses(&unquoted("The `gate` flag and \"gates\"."))
            .is_empty());
    }

    #[test]
    fn tables_are_read_by_section() {
        let md = "## Excluded words\n\n### Metaphors\n\n| Do not write | Write | Except |\n\
                  | --- | --- | --- |\n| `wire`, `wires` | connect | `wire format` |\n";
        let sections = sections(md);
        let rows: Vec<_> = sections.iter().flat_map(|(_, b)| table_rows(b)).collect();
        assert_eq!(rows, [["`wire`, `wires`", "connect", "`wire format`"]]);
    }
}
