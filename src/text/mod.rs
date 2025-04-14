// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::{Error, MapIOErr, PathFile, debug};
use std::collections::HashSet;
use std::fmt::Display;
use std::io;
use std::io::{BufReader, BufWriter, prelude::*};
use std::ops::{Index, IndexMut};
use std::path::Path;

#[cfg(test)]
mod tests_diff;
#[cfg(test)]
mod tests_wildcard;

// Implementation of the Myers diff algorithm:
// Myers, E.W. An O(ND) difference algorithm and its variations. Algorithmica 1, 251--266 (1986).
// https://doi.org/10.1007/BF01840446

/// A step in the edit script.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Edit {
    KeepA(usize),
    RemoveA(usize),
    InsertB(usize),
}

/// An edit script which describes how to transform `a` to `b`.
type EditScript = Vec<Edit>;

/// A limited [`Vec`] wrapper which allows indexing by `isize` in range
/// `(-self.0.len() / 2)..((self.0.len() + 1) / 2`) instead of `0..self.0.len()`.
struct IVec<T>(Vec<T>);

impl<T> Index<isize> for IVec<T> {
    type Output = T;
    fn index(&self, index: isize) -> &T {
        let real_index = (self.0.len() / 2).wrapping_add_signed(index);
        &self.0[real_index]
    }
}

impl<T> IndexMut<isize> for IVec<T> {
    fn index_mut(&mut self, index: isize) -> &mut T {
        let real_index = (self.0.len() / 2).wrapping_add_signed(index);
        &mut self.0[real_index]
    }
}

/// An edit step + an identifier of the previous steps leading to the current point during the edit
/// graph traversal.
#[derive(Clone, Copy)]
struct EditChain {
    prev: usize,
    step: Edit,
}

/// A state of a diagonal during the edit graph traversal.
#[derive(Clone)]
struct DiagonalState {
    x: usize,
    edit_index: usize,
}

/// Compares `a` with `b` and returns an edit script describing how to transform the former to the
/// latter.
fn myers<T: AsRef<str> + PartialEq>(a: &[T], b: &[T]) -> EditScript {
    let max = a.len() + b.len();
    let mut v = IVec(vec![
        DiagonalState {
            x: usize::MAX,
            edit_index: usize::MAX,
        };
        // Minimum of 3 diagonals to allow accessing `v[1].x` when the inputs are empty.
        std::cmp::max(2 * max + 1, 3)
    ]);
    v[1].x = 0;
    let mut edit_chains = Vec::new();

    for d in 0..(max as isize + 1) {
        for k in (-d..d + 1).step_by(2) {
            // Determine where to progress, insert from `b` or remove from `a`.
            let insert_b = k == -d || (k != d && v[k - 1].x < v[k + 1].x);
            let (mut x, mut edit_index) = if insert_b {
                (v[k + 1].x, v[k + 1].edit_index)
            } else {
                (v[k - 1].x + 1, v[k - 1].edit_index)
            };
            let mut y = x.wrapping_add_signed(-k);

            // Record the step in the edit script. Skip the first step in the algorithm which
            // initially brings the traversal to (0,0).
            if d != 0 {
                edit_chains.push(EditChain {
                    prev: edit_index,
                    step: if insert_b {
                        Edit::InsertB(y - 1)
                    } else {
                        Edit::RemoveA(x - 1)
                    },
                });
                edit_index = edit_chains.len() - 1;
            }

            // Look for a snake.
            while x < a.len() && y < b.len() && a[x] == b[y] {
                (x, y) = (x + 1, y + 1);
                edit_chains.push(EditChain {
                    prev: edit_index,
                    step: Edit::KeepA(x - 1),
                });
                edit_index = edit_chains.len() - 1;
            }

            // Check if the end is reached or more steps are needed.
            if x >= a.len() && y >= b.len() {
                // Traverse the edit chain and turn it into a proper edit script.
                let mut edit_script = EditScript::new();
                while edit_index != usize::MAX {
                    let edit_chain = edit_chains[edit_index];
                    edit_script.push(edit_chain.step);
                    edit_index = edit_chain.prev;
                }
                edit_script.reverse();
                return edit_script;
            }
            v[k] = DiagonalState { x, edit_index };
        }
    }
    unreachable!();
}

/// Writes a single diff hunk to the provided output stream.
fn write_hunk<W: Write>(
    hunk_pos_a: usize,
    hunk_len_a: usize,
    hunk_pos_b: usize,
    hunk_len_b: usize,
    hunk_data: &[String],
    writer: &mut BufWriter<W>,
) -> Result<(), crate::Error> {
    let err_desc = "Failed to write a diff hunk";

    writeln!(
        writer,
        "@@ -{},{} +{},{} @@",
        hunk_pos_a, hunk_len_a, hunk_pos_b, hunk_len_b
    )
    .map_io_err(err_desc)?;
    for hunk_str in hunk_data {
        writeln!(writer, "{}", hunk_str).map_io_err(err_desc)?;
    }
    Ok(())
}

/// Compares `a` with `b` and writes their unified diff to the provided output stream.
pub fn unified_diff<T: AsRef<str> + PartialEq + Display, W: Write>(
    a: &[T],
    b: &[T],
    writer: W,
) -> Result<(), crate::Error> {
    let mut writer = BufWriter::new(writer);

    // Diff the two inputs and calculate the edit script.
    let edit_script = myers(a, b);

    // Turn the edit script into hunks in the unified format.
    const CONTEXT_SIZE: usize = 3;
    let (mut context_begin, mut context_end) = (0, 0);
    let (mut pos_a, mut pos_b) = (1, 1);
    let (mut hunk_pos_a, mut hunk_len_a, mut hunk_pos_b, mut hunk_len_b) = (0, 0, 0, 0);
    let mut hunk_data = Vec::new();

    for edit in edit_script {
        match edit {
            Edit::KeepA(index_a) => {
                // Start recording a new context, or extend the current one.
                if context_begin == context_end {
                    context_begin = index_a;
                    context_end = context_begin + 1;
                } else {
                    context_end += 1;
                }

                // Update the positions.
                pos_a += 1;
                pos_b += 1;

                // If handling a hunk, check if it should be closed off.
                if !hunk_data.is_empty() && context_end - context_begin > 2 * CONTEXT_SIZE {
                    for line in a.iter().skip(context_begin).take(CONTEXT_SIZE) {
                        hunk_data.push(format!(" {}", line));
                    }
                    hunk_len_a += CONTEXT_SIZE;
                    hunk_len_b += CONTEXT_SIZE;
                    context_begin += CONTEXT_SIZE;
                    write_hunk(
                        hunk_pos_a,
                        hunk_len_a,
                        hunk_pos_b,
                        hunk_len_b,
                        &hunk_data,
                        &mut writer,
                    )?;
                    hunk_data.clear();
                }
            }

            Edit::RemoveA(_) | Edit::InsertB(_) => {
                // Open a new hunk if not already handling one.
                if hunk_data.is_empty() {
                    if context_end - context_begin > CONTEXT_SIZE {
                        context_begin = context_end - CONTEXT_SIZE;
                    }
                    hunk_pos_a = pos_a - (context_end - context_begin);
                    hunk_len_a = 0;
                    hunk_pos_b = pos_b - (context_end - context_begin);
                    hunk_len_b = 0;
                }

                // Update the positions.
                if let Edit::RemoveA(_) = edit {
                    pos_a += 1;
                } else {
                    pos_b += 1;
                }

                // Add any accumulated context.
                for line in a.iter().take(context_end).skip(context_begin) {
                    hunk_data.push(format!(" {}", line));
                }
                hunk_len_a += context_end - context_begin;
                hunk_len_b += context_end - context_begin;
                context_begin = context_end;

                // Record the removed/added string.
                if let Edit::RemoveA(index_a) = edit {
                    hunk_data.push(format!("-{}", a[index_a]));
                    hunk_len_a += 1;
                } else if let Edit::InsertB(index_b) = edit {
                    hunk_data.push(format!("+{}", b[index_b]));
                    hunk_len_b += 1;
                }
            }
        }
    }

    // Close off the last hunk, if one is open.
    if !hunk_data.is_empty() {
        if context_end - context_begin > CONTEXT_SIZE {
            context_end = context_begin + CONTEXT_SIZE;
        }
        for line in a.iter().take(context_end).skip(context_begin) {
            hunk_data.push(format!(" {}", line));
        }
        hunk_len_a += context_end - context_begin;
        hunk_len_b += context_end - context_begin;
        write_hunk(
            hunk_pos_a,
            hunk_len_a,
            hunk_pos_b,
            hunk_len_b,
            &hunk_data,
            &mut writer,
        )?;
    }

    Ok(())
}

// Rust implementation of the Salz's wildcard method:
// https://github.com/richsalz/wildmat
// Original code has been placed in the public domain.

#[derive(PartialEq)]
enum DoMatchResult {
    True,
    False,
    Abort,
}

/// Attempts to match the given text against the specified shell wildcard pattern.
fn do_match(mut text: &[char], mut p: &[char]) -> DoMatchResult {
    while p[0] != '\0' {
        if text[0] == '\0' && p[0] != '*' {
            return DoMatchResult::Abort;
        }

        match p[0] {
            '\\' => {
                // Literal match with following character.
                p = &p[1..];
                if text[0] != p[0] {
                    return DoMatchResult::False;
                }
            }
            '?' => {
                // Match anything.
            }
            '*' => {
                p = &p[1..];
                while p[0] == '*' {
                    // Consecutive stars act just like one.
                    p = &p[1..];
                }
                if p[0] == '\0' {
                    // Trailing star matches everything.
                    return DoMatchResult::True;
                }
                while text[0] != '\0' {
                    let matched = do_match(text, p);
                    if matched != DoMatchResult::False {
                        return matched;
                    }
                    text = &text[1..];
                }
                return DoMatchResult::Abort;
            }
            '[' => {
                let reverse = p[1] == '^';
                if reverse {
                    // Inverted character class.
                    p = &p[1..];
                }
                let mut matched = false;
                if p[1] == ']' || p[1] == '-' {
                    p = &p[1..];
                    if p[0] == text[0] {
                        matched = true;
                    }
                }
                let mut last = p[0];
                p = &p[1..];
                while p[0] != '\0' && p[0] != ']' {
                    // This next line requires a good C compiler.
                    if if p[0] == '-' && p[1] != ']' {
                        p = &p[1..];
                        text[0] <= p[0] && text[0] >= last
                    } else {
                        text[0] == p[0]
                    } {
                        matched = true;
                    }
                    last = p[0];
                    p = &p[1..];
                }
                if matched == reverse {
                    return DoMatchResult::False;
                }
            }
            _ => {
                if text[0] != p[0] {
                    return DoMatchResult::False;
                }
            }
        }

        text = &text[1..];
        p = &p[1..];
    }

    if text[0] == '\0' {
        DoMatchResult::True
    } else {
        DoMatchResult::False
    }
}

/// Checks whether the given text matches the specified shell wildcard pattern.
pub fn matches_wildcard(text: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let mut text = text.chars().collect::<Vec<_>>();
    text.push('\0');
    let mut pattern = pattern.chars().collect::<Vec<_>>();
    pattern.push('\0');
    do_match(&text, &pattern) == DoMatchResult::True
}

/// Reads data from a specified reader and returns its content as a [`Vec`] of [`String`] lines.
pub fn read_lines<R: Read>(reader: R) -> io::Result<Vec<String>> {
    let reader = BufReader::new(reader);
    let mut lines = Vec::new();
    for maybe_line in reader.lines() {
        match maybe_line {
            Ok(line) => lines.push(line),
            Err(err) => return Err(err),
        };
    }
    Ok(lines)
}

/// A writer to the standard output, a file or an internal buffer.
pub enum Writer {
    Stdout(BufWriter<io::Stdout>),
    File(BufWriter<PathFile>),
    Buffer(Vec<u8>),
}

impl Writer {
    /// Creates a new [`Writer`] that writes to the specified file. Treats "-" as the standard
    /// output.
    pub fn new_file<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let path = path.as_ref();

        if path == Path::new("-") {
            Ok(Self::Stdout(BufWriter::new(io::stdout())))
        } else {
            match PathFile::create(path) {
                Ok(file) => Ok(Self::File(BufWriter::new(file))),
                Err(err) => Err(Error::new_io(
                    format!("Failed to create file '{}'", path.display()),
                    err,
                )),
            }
        }
    }

    /// Creates a new [`Writer`] that writes to an internal buffer.
    pub fn new_buffer() -> Self {
        Self::Buffer(Vec::new())
    }

    /// Obtains the internal buffer when the writer is of the appropriate type.
    pub fn into_inner(self) -> Vec<u8> {
        match self {
            Self::Stdout(_) | Self::File(_) => panic!("The writer is not of type Writer::Buffer"),
            Self::Buffer(vec) => vec,
        }
    }
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Stdout(stdout) => stdout.write(buf),
            Self::File(file) => file.write(buf),
            Self::Buffer(vec) => vec.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Stdout(stdout) => stdout.flush(),
            Self::File(file) => file.flush(),
            Self::Buffer(vec) => vec.flush(),
        }
    }
}

// TODO Support wildcards.
#[derive(Default)]
pub struct Filter {
    patterns: HashSet<String>,
}

impl Filter {
    pub fn new() -> Self {
        Self {
            patterns: HashSet::new(),
        }
    }

    pub fn load<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        let path = path.as_ref();
        debug!("Loading '{}'", path.display());

        let file = PathFile::open(path).map_err(|err| {
            crate::Error::new_io(format!("Failed to open file '{}'", path.display()), err)
        })?;

        // Read all content from the file.
        let lines = match read_lines(file) {
            Ok(lines) => lines,
            Err(err) => return Err(crate::Error::new_io("Failed to read filter data", err)),
        };

        // Validate the patterns, reject empty ones.
        for (line_idx, line) in lines.iter().enumerate() {
            if line.is_empty() {
                return Err(Error::new_parse(format!(
                    "{}:{}: Expected a pattern",
                    path.display(),
                    line_idx + 1
                )));
            }
        }

        // Insert the new patterns.
        for line in lines {
            self.patterns.insert(line);
        }

        Ok(())
    }

    pub fn matches(&self, name: &str) -> bool {
        self.patterns.contains(name)
    }
}
