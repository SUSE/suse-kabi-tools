// Copyright (C) 2025 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

//! A representation of kABI severity rules and tools for working with the data.

use crate::text::{matches_wildcard, read_lines};
use crate::{Error, PathFile, debug};
use std::io::prelude::*;
use std::iter::Peekable;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

/// A type used in the specification of a severity rule.
#[derive(Debug, PartialEq)]
enum RuleType {
    Module,
    Namespace,
    Symbol,
}

/// A verdict used in the specification of a severity rule.
#[derive(Debug, PartialEq)]
enum Verdict {
    Pass,
    Fail,
}

/// A severity rule.
#[derive(Debug, PartialEq)]
struct Rule {
    rule_type: RuleType,
    pattern: String,
    verdict: Verdict,

    source_file_idx: usize, // Index into `Rules.files`.
    source_line_idx: usize,
}

impl Rule {
    /// Creates a new severity rule.
    pub fn new<S: Into<String>>(
        rule_type: RuleType,
        pattern: S,
        verdict: Verdict,
        source_file_idx: usize,
        source_line_idx: usize,
    ) -> Self {
        Rule {
            rule_type,
            pattern: pattern.into(),
            verdict,
            source_file_idx,
            source_line_idx,
        }
    }
}

/// A collection of severity rules.
#[derive(Debug, Default, PartialEq)]
pub struct Rules {
    data: Vec<Rule>,
    files: Vec<PathBuf>,
}

impl Rules {
    /// Creates a new empty `Rules` object.
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            files: Vec::new(),
        }
    }

    /// Loads rules data from the specified file.
    ///
    /// New rules are appended to the already present ones.
    pub fn load<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        let path = path.as_ref();

        let file = PathFile::open(path).map_err(|err| {
            Error::new_io(format!("Failed to open the file '{}'", path.display()), err)
        })?;

        self.load_buffer(path, file)
    }

    /// Loads rules data from the specified reader.
    ///
    /// The `path` should point to the rules file name, indicating the origin of the data. New rules
    /// are appended to the already present ones.
    pub fn load_buffer<P: AsRef<Path>, R: Read>(
        &mut self,
        path: P,
        reader: R,
    ) -> Result<(), Error> {
        let path = path.as_ref();
        debug!("Loading rules data from '{}'", path.display());

        // Read all content from the file.
        let lines = match read_lines(reader) {
            Ok(lines) => lines,
            Err(err) => return Err(Error::new_io("Failed to read rules data", err)),
        };

        // Parse all rules.
        let file_idx = self.data.len();
        let mut new_rules = Vec::new();
        for (line_idx, line) in lines.iter().enumerate() {
            if let Some(rule) = parse_rule(path, file_idx, line_idx, line)? {
                new_rules.push(rule);
            }
        }

        // Add the new rules.
        self.files.push(path.to_path_buf());
        self.data.append(&mut new_rules);

        Ok(())
    }

    /// Looks for the first rule that matches the specified symbol and, if found, returns its
    /// verdict on whether changes to the symbol should be tolerated. Otherwise, returns false.
    pub fn is_tolerated(&self, symbol: &str, module: &str, maybe_namespace: Option<&str>) -> bool {
        for rule in &self.data {
            match rule.rule_type {
                RuleType::Module => {
                    if matches_wildcard(module, &rule.pattern) {
                        return rule.verdict == Verdict::Pass;
                    }
                }
                RuleType::Namespace => {
                    if let Some(namespace) = maybe_namespace
                        && matches_wildcard(namespace, &rule.pattern)
                    {
                        return rule.verdict == Verdict::Pass;
                    }
                }
                RuleType::Symbol => {
                    if matches_wildcard(symbol, &rule.pattern) {
                        return rule.verdict == Verdict::Pass;
                    }
                }
            }
        }
        false
    }
}

/// Parses the next rule word from the given iterator, taking into account comments starting with
/// '#'.
fn get_next_rule_word<I: Iterator<Item = char>>(chars: &mut Peekable<I>) -> Option<String> {
    // Skip over any whitespace.
    while let Some(&c) = chars.peek() {
        if !c.is_ascii_whitespace() {
            break;
        }
        chars.next();
    }

    // Terminate when a comment starting with '#' is found.
    if let Some(&c) = chars.peek()
        && c == '#'
    {
        return None;
    }

    // Read one word.
    let mut word = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_whitespace() || c == '#' {
            break;
        }
        word.push(c);
        chars.next();
    }

    if word.is_empty() {
        return None;
    }
    Some(word)
}

/// Parses a single severity rule.
fn parse_rule(
    path: &Path,
    file_idx: usize,
    line_idx: usize,
    line: &str,
) -> Result<Option<Rule>, Error> {
    let mut chars = line.chars().peekable();

    // Parse the first two words blindly.
    let word0 = match get_next_rule_word(&mut chars) {
        Some(word) => word,
        None => {
            // The line doesn't contain any rule.
            return Ok(None);
        }
    };
    let word1 = match get_next_rule_word(&mut chars) {
        Some(word) => word,
        None => {
            return Err(Error::new_parse_format(
                "The rule is incomplete, must be in the form '[type] <pattern> <verdict>'",
                path,
                line_idx + 1,
                line,
            ));
        }
    };

    // Parse the type, pattern and partically verdict.
    //
    // The style of a rule is determined by the number of words. Classic rules are in the form
    // `<pattern> <verdict>`. Rules with an explicit type are in the form
    // `<type> <pattern> <verdict>`.
    let (rule_type, pattern, verdict) = match get_next_rule_word(&mut chars) {
        Some(word2) => {
            let rule_type = match word0.as_str() {
                "MODULE" => RuleType::Module,
                "NAMESPACE" => RuleType::Namespace,
                "SYMBOL" => RuleType::Symbol,
                _ => {
                    return Err(Error::new_parse_format(
                        &format!(
                            "Invalid rule type '{}', must be either MODULE, NAMESPACE or SYMBOL",
                            word0
                        ),
                        path,
                        line_idx + 1,
                        line,
                    ));
                }
            };

            // Check that nothing else is left on the line.
            if get_next_rule_word(&mut chars).is_some() {
                return Err(Error::new_parse_format(
                    "Unexpected string found after the verdict",
                    path,
                    line_idx + 1,
                    line,
                ));
            }

            (rule_type, word1, word2)
        }
        None => {
            let rule_type = if word0.contains('/') || word0 == "vmlinux" {
                RuleType::Module
            } else if word0 == word0.to_uppercase() {
                RuleType::Namespace
            } else {
                RuleType::Symbol
            };

            (rule_type, word0, word1)
        }
    };

    // Parse the verdict.
    let verdict = match verdict.as_str() {
        "PASS" => Verdict::Pass,
        "FAIL" => Verdict::Fail,
        _ => {
            return Err(Error::new_parse_format(
                &format!("Invalid verdict '{}', must be either PASS or FAIL", verdict),
                path,
                line_idx + 1,
                line,
            ));
        }
    };

    Ok(Some(Rule::new(
        rule_type, pattern, verdict, file_idx, line_idx,
    )))
}
