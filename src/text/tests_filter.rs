// Copyright (C) 2025 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;
use crate::{assert_ok, assert_parse_err, bytes, string_vec};

#[test]
fn read_literal_pattern() {
    // Check that patterns containing regular characters are considered as literals.
    let mut filter = Filter::new();
    let result = filter.load_buffer(
        "test.filter",
        bytes!(
            "abc\n", "ABC\n", "_09\n", //
        ),
    );
    assert_ok!(result);
    assert_eq!(
        filter,
        Filter {
            literals: HashSet::from(["abc".to_string(), "ABC".to_string(), "_09".to_string()]),
            wildcards: vec![],
        }
    );
}

#[test]
fn read_wildcard_pattern() {
    // Check that patterns containing specific special characters are considered as wildcards.
    let mut filter = Filter::new();
    let result = filter.load_buffer(
        "test.filter",
        bytes!(
            "\\abc\n", "a?bc\n", "ab*c\n", "abc[\n", //
        ),
    );
    assert_ok!(result);
    assert_eq!(
        filter,
        Filter {
            literals: HashSet::new(),
            wildcards: string_vec!["\\abc", "a?bc", "ab*c", "abc["],
        }
    );
}

#[test]
fn read_empty_record() {
    // Check that empty records are rejected when reading a filter file.
    let mut filter = Filter::new();
    let result = filter.load_buffer(
        "test.filter",
        bytes!(
            "foo\n", "\n", "bar\n", //
        ),
    );
    assert_parse_err!(result, "test.filter:2: Expected a pattern");
    assert_eq!(filter, Filter::new());
}

#[test]
fn matches_literal_pattern() {
    // Check that a filter can match a literal pattern.
    let mut filter = Filter::new();
    let result = filter.load_buffer(
        "test.filter",
        bytes!(
            "abc\n" //
        ),
    );
    assert_ok!(result);
    assert!(filter.matches("abc"));
    assert!(!filter.matches("Xbc"));
}

#[test]
fn matches_wildcard_pattern() {
    // Check that a filter can match a wildcard pattern.
    let mut filter = Filter::new();
    let result = filter.load_buffer(
        "test.filter",
        bytes!(
            "a*\n" //
        ),
    );
    assert_ok!(result);
    assert!(filter.matches("abc"));
    assert!(!filter.matches("Xbc"));
}
