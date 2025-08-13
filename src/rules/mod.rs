// Copyright (C) 2025 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::text::{matches_wildcard, read_lines};
use crate::{Error, PathFile, debug};
use std::io::prelude::*;
use std::iter::Peekable;
use std::path::Path;

#[cfg(test)]
mod tests;

/// A pattern used in the specification of a severity rule.
#[derive(Debug, PartialEq)]
enum Pattern {
    Module(String),
    Namespace(String),
    Symbol(String),
}

impl Pattern {
    /// Creates a new `Pattern::Module`.
    pub fn new_module<S: Into<String>>(name: S) -> Self {
        Pattern::Module(name.into())
    }

    /// Creates a new `Pattern::Namespace`.
    pub fn new_namespace<S: Into<String>>(name: S) -> Self {
        Pattern::Namespace(name.into())
    }

    /// Creates a new `Pattern::Symbol`.
    pub fn new_symbol<S: Into<String>>(name: S) -> Self {
        Pattern::Symbol(name.into())
    }
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
    pattern: Pattern,
    verdict: Verdict,
}

impl Rule {
    /// Creates a new severity rule.
    pub fn new(pattern: Pattern, verdict: Verdict) -> Self {
        Rule { pattern, verdict }
    }
}

/// A collection of severity rules.
#[derive(Debug, Default, PartialEq)]
pub struct Rules {
    data: Vec<Rule>,
}

impl Rules {
    /// Creates a new empty `Rules` object.
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Loads rules data from a specified file.
    ///
    /// New rules are appended to the already present ones.
    pub fn load<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        let path = path.as_ref();

        let file = PathFile::open(path).map_err(|err| {
            Error::new_io(format!("Failed to open the file '{}'", path.display()), err)
        })?;

        self.load_buffer(path, file)
    }

    /// Loads rules data from a specified reader.
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
        let mut new_rules = Vec::new();
        for (line_idx, line) in lines.iter().enumerate() {
            if let Some(rule) = parse_rule(path, line_idx, line)? {
                new_rules.push(rule);
            }
        }

        // Add the new rules.
        self.data.append(&mut new_rules);

        Ok(())
    }

    /// Looks for the first rule that matches the specified symbol and, if found, returns its
    /// verdict on whether changes to the symbol should be tolerated. Otherwise, returns false.
    pub fn is_tolerated(&self, symbol: &str, module: &str, maybe_namespace: Option<&str>) -> bool {
        for rule in &self.data {
            match &rule.pattern {
                Pattern::Module(rule_module) => {
                    if matches_wildcard(module, rule_module) {
                        return rule.verdict == Verdict::Pass;
                    }
                }
                Pattern::Namespace(rule_namespace) => {
                    if let Some(namespace) = maybe_namespace {
                        if matches_wildcard(namespace, rule_namespace) {
                            return rule.verdict == Verdict::Pass;
                        }
                    }
                }
                Pattern::Symbol(rule_symbol) => {
                    if matches_wildcard(symbol, rule_symbol) {
                        return rule.verdict == Verdict::Pass;
                    }
                }
            }
        }
        false
    }
}

/// Parses the next word from the `chars` iterator, taking into account comments starting with '#'.
fn get_next_word<I: Iterator<Item = char>>(chars: &mut Peekable<I>) -> Option<String> {
    // Skip over any whitespace.
    while let Some(&c) = chars.peek() {
        if !c.is_ascii_whitespace() {
            break;
        }
        chars.next();
    }

    // Terminate when a comment starting with '#' is found.
    if let Some(&c) = chars.peek() {
        if c == '#' {
            return None;
        }
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
fn parse_rule(path: &Path, line_idx: usize, line: &str) -> Result<Option<Rule>, Error> {
    let mut chars = line.chars().peekable();

    // Parse the pattern.
    let pattern = match get_next_word(&mut chars) {
        Some(pattern) => {
            if pattern.contains('/') || pattern == "vmlinux" {
                Pattern::new_module(pattern)
            } else if pattern == pattern.to_uppercase() {
                Pattern::new_namespace(pattern)
            } else {
                Pattern::new_symbol(pattern)
            }
        }
        None => {
            // The line doesn't contain any rule.
            return Ok(None);
        }
    };

    // Parse the verdict.
    let verdict = match get_next_word(&mut chars) {
        Some(verdict) => match verdict.as_str() {
            "PASS" => Verdict::Pass,
            "FAIL" => Verdict::Fail,
            _ => {
                return Err(Error::new_parse(format!(
                    "{}:{}: Invalid verdict '{}', must be either PASS or FAIL",
                    path.display(),
                    line_idx + 1,
                    verdict
                )));
            }
        },
        None => {
            return Err(Error::new_parse(format!(
                "{}:{}: The rule does not specify a verdict",
                path.display(),
                line_idx + 1
            )));
        }
    };

    // Check that nothing else is left on the line.
    if let Some(word) = get_next_word(&mut chars) {
        return Err(Error::new_parse(format!(
            "{}:{}: Unexpected string '{}' found after the verdict",
            path.display(),
            line_idx + 1,
            word
        )));
    }

    Ok(Some(Rule::new(pattern, verdict)))
}
