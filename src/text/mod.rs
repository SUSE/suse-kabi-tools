// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::{Error, MapIOErr, PathFile, debug};
use std::cmp;
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::fs;
use std::io;
use std::io::{BufReader, BufWriter, prelude::*};
use std::ops::{Index, IndexMut};
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests_diff;
#[cfg(test)]
mod tests_filter;
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
        cmp::max(2 * max + 1, 3)
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
    mut writer: W,
) -> Result<(), Error> {
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
    mut writer: W,
) -> Result<(), Error> {
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
                        writer.by_ref(),
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
            writer.by_ref(),
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
    NamedBuffer(PathBuf, Vec<u8>),
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

    /// Creates a new [`Writer`] that writes to a named internal buffer.
    pub fn new_named_buffer<P: AsRef<Path>>(path: P) -> Self {
        Self::NamedBuffer(path.as_ref().to_path_buf(), Vec::new())
    }

    /// Obtains the internal buffer if the writer is of the [`Writer::Buffer`] type.
    pub fn into_inner_vec(self) -> Vec<u8> {
        match self {
            Self::Buffer(vec) => vec,
            _ => panic!("The writer is not of type Writer::Buffer"),
        }
    }

    /// Obtains the path and internal buffer if the writer is of the [`Writer::NamedBuffer`] type.
    pub fn into_inner_path_vec(self) -> (PathBuf, Vec<u8>) {
        match self {
            Self::NamedBuffer(path, vec) => (path, vec),
            _ => panic!("The writer is not of type Writer::NamedBuffer"),
        }
    }
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Stdout(stdout) => stdout.write(buf),
            Self::File(file) => file.write(buf),
            Self::Buffer(vec) => vec.write(buf),
            Self::NamedBuffer(_, vec) => vec.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Stdout(stdout) => stdout.flush(),
            Self::File(file) => file.flush(),
            Self::Buffer(vec) => vec.flush(),
            Self::NamedBuffer(_, vec) => vec.flush(),
        }
    }
}

/// A factory trait for [`Write`] objects, allowing writing to multiple files/streams.
pub trait WriteGenerator<W: Write> {
    /// Opens a new writer to the specified path.
    fn create<P: AsRef<Path>>(&mut self, sub_path: P) -> Result<W, Error>;

    /// Closes a writer previously provided by the `create()` method.
    fn close(&mut self, writer: W);
}

/// A factory for writing multiple files in a specific directory. The output can be written directly
/// to on-disk files, or stored in a set of internal buffers.
pub enum DirectoryWriter {
    File(PathBuf),
    Buffer(PathBuf, HashMap<PathBuf, Vec<u8>>),
}

impl DirectoryWriter {
    /// Creates a new [`DirectoryWriter`] that writes to on-disk files in a specified directory.
    pub fn new_file<P: AsRef<Path>>(root: P) -> Self {
        Self::File(root.as_ref().to_path_buf())
    }

    /// Creates a new [`DirectoryWriter`] that writes to a set of internal buffers.
    pub fn new_buffer<P: AsRef<Path>>(root: P) -> Self {
        Self::Buffer(root.as_ref().to_path_buf(), HashMap::new())
    }

    /// Obtains the internal buffers if the writer is of the [`DirectoryWriter::Buffer`] type.
    pub fn into_inner_map(self) -> HashMap<PathBuf, Vec<u8>> {
        match self {
            Self::Buffer(_, files) => files,
            _ => panic!("The writer is not of type DirectoryWriter::Buffer"),
        }
    }
}

impl WriteGenerator<Writer> for &mut DirectoryWriter {
    fn create<P: AsRef<Path>>(&mut self, sub_path: P) -> Result<Writer, Error> {
        match self {
            DirectoryWriter::File(root) => {
                let path = root.join(sub_path);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|err| {
                        Error::new_io(
                            format!("Failed to create directory '{}'", parent.display()),
                            err,
                        )
                    })?;
                }
                Writer::new_file(path)
            }
            DirectoryWriter::Buffer(root, _) => Ok(Writer::new_named_buffer(root.join(sub_path))),
        }
    }

    fn close(&mut self, writer: Writer) {
        if let DirectoryWriter::Buffer(_, files) = self {
            let (path, vec) = writer.into_inner_path_vec();
            files.insert(path, vec);
        }
    }
}

/// A collection of shell wildcard patterns used to filter symbol or file names.
#[derive(Debug, Default, PartialEq)]
pub struct Filter {
    // Literal patterns.
    literals: HashSet<String>,
    // Wildcard patterns.
    wildcards: Vec<String>,
}

impl Filter {
    /// Creates a new empty `Filter` object.
    pub fn new() -> Self {
        Self {
            literals: HashSet::new(),
            wildcards: Vec::new(),
        }
    }

    /// Loads filter data from a specified file.
    ///
    /// New patterns are appended to the already present ones.
    pub fn load<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        let path = path.as_ref();

        let file = PathFile::open(path).map_err(|err| {
            Error::new_io(format!("Failed to open file '{}'", path.display()), err)
        })?;

        self.load_buffer(path, file)
    }

    /// Loads filter data from a specified reader.
    ///
    /// The `path` should point to the filter file name, indicating the origin of the data. New
    /// patterns are appended to the already present ones.
    pub fn load_buffer<P: AsRef<Path>, R: Read>(
        &mut self,
        path: P,
        reader: R,
    ) -> Result<(), Error> {
        let path = path.as_ref();
        debug!("Loading filter data from '{}'", path.display());

        // Read all content from the file.
        let lines = match read_lines(reader) {
            Ok(lines) => lines,
            Err(err) => return Err(Error::new_io("Failed to read filter data", err)),
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
            if line
                .chars()
                .any(|x| x == '\\' || x == '?' || x == '*' || x == '[')
            {
                self.wildcards.push(line);
            } else {
                self.literals.insert(line);
            }
        }

        Ok(())
    }

    /// Checks whether the given text matches any of the filter patterns.
    pub fn matches(&self, name: &str) -> bool {
        if self.literals.contains(name) {
            return true;
        }

        for pattern in &self.wildcards {
            if matches_wildcard(name, pattern) {
                return true;
            }
        }

        false
    }
}
