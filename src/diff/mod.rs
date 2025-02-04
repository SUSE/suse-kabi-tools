// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

// Implementation of the Myers diff alrogithm:
// Myers, E.W. An O(ND) difference algorithm and its variations. Algorithmica 1, 251--266 (1986).
// https://doi.org/10.1007/BF01840446

use crate::MapIOErr;
use std::fmt::Display;
use std::io::{prelude::*, BufWriter};
use std::ops::{Index, IndexMut};
use std::path::Path;

#[cfg(test)]
mod tests;

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
fn myers<T>(a: &[T], b: &[T]) -> EditScript
where
    T: AsRef<str> + PartialEq,
{
    let max = a.len() + b.len();
    let mut v = IVec(vec![
        DiagonalState {
            x: usize::MAX,
            edit_index: usize::MAX,
        };
        // Minium of 3 diagonals to allow accessing `v[1].x` when the inputs are empty.
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

fn write_hunk<W>(
    hunk_pos_a: usize,
    hunk_len_a: usize,
    hunk_pos_b: usize,
    hunk_len_b: usize,
    hunk_data: &[String],
    path: &Path,
    writer: &mut BufWriter<W>,
) -> Result<(), crate::Error>
where
    W: Write,
{
    writeln!(
        writer,
        "@@ -{},{} +{},{} @@",
        hunk_pos_a, hunk_len_a, hunk_pos_b, hunk_len_b
    )
    .map_io_err(path)?;
    for hunk_str in hunk_data {
        writeln!(writer, "{}", hunk_str).map_io_err(path)?;
    }
    Ok(())
}

pub fn unified<T, W>(a: &[T], b: &[T], path: &Path, writer: W) -> Result<(), crate::Error>
where
    T: AsRef<str> + PartialEq + Display,
    W: Write,
{
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
                    for i in context_begin..context_begin + CONTEXT_SIZE {
                        hunk_data.push(format!(" {}", a[i]));
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
                        path,
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
                for i in context_begin..context_end {
                    hunk_data.push(format!(" {}", a[i]));
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
        for i in context_begin..context_end {
            hunk_data.push(format!(" {}", a[i]));
        }
        hunk_len_a += context_end - context_begin;
        hunk_len_b += context_end - context_begin;
        write_hunk(
            hunk_pos_a,
            hunk_len_a,
            hunk_pos_b,
            hunk_len_b,
            &hunk_data,
            path,
            &mut writer,
        )?;
    }

    Ok(())
}
