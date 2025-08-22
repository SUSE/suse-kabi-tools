// Copyright (C) 2024 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;

#[test]
fn myers_trivial_empty() {
    // Check a situation when no operation is needed because both inputs are empty.
    let a: [&str; 0] = [];
    let b = [];
    let edit_script = myers(&a, &b);
    assert_eq!(edit_script, []);
}

#[test]
fn myers_trivial_replace() {
    // Check a situation when a complete replacement is needed.
    let a = ["X"];
    let b = ["Y"];
    let edit_script = myers(&a, &b);
    assert_eq!(edit_script, [Edit::RemoveA(0), Edit::InsertB(0)]);
}

#[test]
fn myers_trivial_insert() {
    // Check a situation when an insert operation from `b` is the only step needed.
    let a = [];
    let b = ["X"];
    let edit_script = myers(&a, &b);
    assert_eq!(edit_script, [Edit::InsertB(0)]);
}

#[test]
fn myers_trivial_remove() {
    // Check a situation when a remove operation from `a` is the only step needed.
    let a = ["X"];
    let b = [];
    let edit_script = myers(&a, &b);
    assert_eq!(edit_script, [Edit::RemoveA(0)]);
}

#[test]
fn myers_trivial_keep() {
    // Check a situation when a keep operation from `a` is the only step needed.
    let a = ["X"];
    let b = ["X"];
    let edit_script = myers(&a, &b);
    assert_eq!(edit_script, [Edit::KeepA(0)]);
}

#[test]
fn myers_insert_front() {
    // Check a situation when an insert operation at the front of `a` is needed.
    let a = ["X", "Y"];
    let b = ["W", "X", "Y"];
    let edit_script = myers(&a, &b);
    assert_eq!(
        edit_script,
        [Edit::InsertB(0), Edit::KeepA(0), Edit::KeepA(1)]
    );
}

#[test]
fn myers_insert_middle() {
    // Check a situation when an insert operation in the middle of `a` is needed.
    let a = ["X", "Z"];
    let b = ["X", "Y", "Z"];
    let edit_script = myers(&a, &b);
    assert_eq!(
        edit_script,
        [Edit::KeepA(0), Edit::InsertB(1), Edit::KeepA(1)]
    );
}

#[test]
fn myers_insert_end() {
    // Check a situation when an insert operation at the end of `a` is needed.
    let a = ["X", "Y"];
    let b = ["X", "Y", "Z"];
    let edit_script = myers(&a, &b);
    assert_eq!(
        edit_script,
        [Edit::KeepA(0), Edit::KeepA(1), Edit::InsertB(2)]
    );
}

#[test]
fn myers_insert_subsequent() {
    // Check a situation when subsequent insert operations in `a` are needed.
    let a = [];
    let b = ["X", "Y", "Z"];
    let edit_script = myers(&a, &b);
    assert_eq!(
        edit_script,
        [Edit::InsertB(0), Edit::InsertB(1), Edit::InsertB(2)]
    );
}

#[test]
fn myers_remove_front() {
    // Check a situation when a remove operation from the front of `a` is needed.
    let a = ["W", "X", "Y"];
    let b = ["X", "Y"];
    let edit_script = myers(&a, &b);
    assert_eq!(
        edit_script,
        [Edit::RemoveA(0), Edit::KeepA(1), Edit::KeepA(2)]
    );
}

#[test]
fn myers_remove_middle() {
    // Check a situation when a remove operation from the middle of `a` is needed.
    let a = ["X", "Y", "Z"];
    let b = ["X", "Z"];
    let edit_script = myers(&a, &b);
    assert_eq!(
        edit_script,
        [Edit::KeepA(0), Edit::RemoveA(1), Edit::KeepA(2)]
    );
}

#[test]
fn myers_remove_end() {
    // Check a situation when a remove operation from the end of `a` is needed.
    let a = ["X", "Y", "Z"];
    let b = ["X", "Y"];
    let edit_script = myers(&a, &b);
    assert_eq!(
        edit_script,
        [Edit::KeepA(0), Edit::KeepA(1), Edit::RemoveA(2)]
    );
}

#[test]
fn myers_remove_subsequent() {
    // Check a situation when subsequent remove operations from `a` are needed.
    let a = ["X", "Y", "Z"];
    let b = [];
    let edit_script = myers(&a, &b);
    assert_eq!(
        edit_script,
        [Edit::RemoveA(0), Edit::RemoveA(1), Edit::RemoveA(2)]
    );
}

#[test]
fn myers_keep_subsequent() {
    // Check a situation when subsequent keep operations from `a` are needed.
    let a = ["X", "Y", "Z"];
    let b = ["W", "X", "Y"];
    let edit_script = myers(&a, &b);
    assert_eq!(
        edit_script,
        [
            Edit::InsertB(0),
            Edit::KeepA(0),
            Edit::KeepA(1),
            Edit::RemoveA(2)
        ]
    );
}
