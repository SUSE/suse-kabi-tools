// Copyright (C) 2025 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

//! A representation of kABI severity rules and tools for working with the data.

use crate::text::{matches_wildcard, read_lines};
use crate::{Error, MapIOErr, PathFile, debug};
use std::collections::HashSet;
use std::fmt::{self, Display, Formatter};
use std::io::prelude::*;
use std::iter::Peekable;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

/// A type used in the specification of a severity rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuleType {
    Module,
    Namespace,
    Symbol,
}

impl Display for RuleType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Module => write!(f, "MODULE"),
            Self::Namespace => write!(f, "NAMESPACE"),
            Self::Symbol => write!(f, "SYMBOL"),
        }
    }
}

/// A verdict used in the specification of a severity rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Verdict {
    Pass,
    Fail,
}

impl Display for Verdict {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Pass => write!(f, "PASS"),
            Self::Fail => write!(f, "FAIL"),
        }
    }
}

/// A severity rule.
#[derive(Debug, Eq, PartialEq)]
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
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Rules {
    data: Vec<Rule>,
    files: Vec<PathBuf>,
}

/// Indexes of all rules in [`Rules`] that were matched by any symvers record.
pub type UsedRules = HashSet<usize>;

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
        let file_idx = self.files.len();
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

    /// Searches for the first rule that matches the specified symbol. If a match is found, it
    /// returns the index of the rule. Otherwise, returns None.
    fn find_matching_rule(
        &self,
        symbol: &str,
        module: &str,
        maybe_namespace: Option<&str>,
    ) -> Option<usize> {
        for (rule_idx, rule) in self.data.iter().enumerate() {
            match rule.rule_type {
                RuleType::Module => {
                    if matches_wildcard(module, &rule.pattern) {
                        return Some(rule_idx);
                    }
                }
                RuleType::Namespace => {
                    if let Some(namespace) = maybe_namespace
                        && matches_wildcard(namespace, &rule.pattern)
                    {
                        return Some(rule_idx);
                    }
                }
                RuleType::Symbol => {
                    if matches_wildcard(symbol, &rule.pattern) {
                        return Some(rule_idx);
                    }
                }
            }
        }
        None
    }

    /// Searches for the first rule that matches the specified symbol. If a match is found, it
    /// returns its verdict on whether changes to the symbol should be tolerated. Otherwise, returns
    /// false.
    pub fn is_tolerated(&self, symbol: &str, module: &str, maybe_namespace: Option<&str>) -> bool {
        if let Some(rule_idx) = self.find_matching_rule(symbol, module, maybe_namespace) {
            self.data[rule_idx].verdict == Verdict::Pass
        } else {
            false
        }
    }

    /// Searches for the first rule that matches the specified symbol. If a match is found, the
    /// index of the rule is added to `used_rules`.
    pub fn mark_used_rule(
        &self,
        symbol: &str,
        module: &str,
        maybe_namespace: Option<&str>,
        used_rules: &mut UsedRules,
    ) {
        if let Some(rule_idx) = self.find_matching_rule(symbol, module, maybe_namespace) {
            used_rules.insert(rule_idx);
        }
    }

    /// Writes information about all unused rules to the provided output stream.
    pub fn write_unused_rules_buffer<W: Write>(
        &self,
        used_rules: &UsedRules,
        mut writer: W,
    ) -> Result<(), Error> {
        let err_desc = "Failed to write information about an unused rule";

        for (rule_idx, rule) in self.data.iter().enumerate() {
            if !used_rules.contains(&rule_idx) {
                writeln!(
                    writer,
                    "{}:{}: WARNING: Severity rule '{} {} {}' is unused",
                    self.files[rule.source_file_idx].display(),
                    rule.source_line_idx + 1,
                    rule.rule_type,
                    rule.pattern,
                    rule.verdict
                )
                .map_io_err(err_desc)?;
            }
        }

        writer.flush().map_io_err(err_desc)?;

        Ok(())
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

    // Parse the type, pattern and partially verdict.
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
