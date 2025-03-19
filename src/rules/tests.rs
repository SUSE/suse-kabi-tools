// Copyright (C) 2025 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;
use crate::{assert_ok, assert_parse_err};

#[test]
fn read_module_rule() {
    // Check that a pattern containing '/' or equal to "vmlinux" is considered as a module.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        concat!(
            "lib/test_module.ko PASS\n",
            "vmlinux PASS\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_eq!(
        rules,
        Rules {
            data: vec![
                Rule::new(Pattern::new_module("lib/test_module.ko"), Verdict::Pass),
                Rule::new(Pattern::new_module("vmlinux"), Verdict::Pass),
            ]
        }
    );
}

#[test]
fn read_namespace_rule() {
    // Check that a pattern consisting of only uppercase letter is considered as a namespace.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        concat!(
            "TEST_NAMESPACE PASS\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_eq!(
        rules,
        Rules {
            data: vec![Rule::new(
                Pattern::new_namespace("TEST_NAMESPACE"),
                Verdict::Pass
            ),]
        }
    );
}

#[test]
fn read_symbol_rule() {
    // Check that a pattern which isn't recognized as a module or a namespace is considered as
    // a symbol.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        concat!(
            "symbol_name PASS\n",
            "test_module.ko PASS\n",
            "vmlinux2 PASS\n",
            "test_namespace PASS\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_eq!(
        rules,
        Rules {
            data: vec![
                Rule::new(Pattern::new_symbol("symbol_name"), Verdict::Pass),
                Rule::new(Pattern::new_symbol("test_module.ko"), Verdict::Pass),
                Rule::new(Pattern::new_symbol("vmlinux2"), Verdict::Pass),
                Rule::new(Pattern::new_symbol("test_namespace"), Verdict::Pass),
            ]
        }
    );
}

#[test]
fn read_pass_fail_rule() {
    // Check that the PASS and FAIL verdicts are correctly recognized.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        concat!(
            "symbol_name PASS\n",
            "symbol_name2 FAIL\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_eq!(
        rules,
        Rules {
            data: vec![
                Rule::new(Pattern::new_symbol("symbol_name"), Verdict::Pass),
                Rule::new(Pattern::new_symbol("symbol_name2"), Verdict::Fail),
            ]
        }
    );
}

#[test]
fn read_no_verdict() {
    // Check that a rule without a verdict is rejected.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        concat!(
            "symbol_name\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(
        result,
        "test.severities:1: The rule does not specify a verdict"
    );
    assert_eq!(rules, Rules { data: vec![] });
}

#[test]
fn read_invalid_verdict() {
    // Check that an invalid verdict is rejected.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        concat!(
            "symbol_name OK\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(
        result,
        "test.severities:1: Invalid verdict 'OK', must be either PASS or FAIL"
    );
    assert_eq!(rules, Rules { data: vec![] });
}

#[test]
fn read_extra_data() {
    // Check that any extra data after the verdict is rejected.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        concat!(
            "symbol_name PASS garbage\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(
        result,
        "test.severities:1: Unexpected string 'garbage' found after the verdict"
    );
    assert_eq!(rules, Rules { data: vec![] });
}

#[test]
fn read_empty_record() {
    // Check that empty records are skipped when reading a rules file.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        concat!(
            "\n",
            "\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_eq!(rules, Rules { data: vec![] });
}

#[test]
fn read_comments() {
    // Check that comments in various positions are correctly skipped.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        concat!(
            "# comment 1\n",
            "lib/test_module.ko PASS # comment 2\n",
            "lib/test_module2.ko FAIL# comment 3\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_eq!(
        rules,
        Rules {
            data: vec![
                Rule::new(Pattern::new_module("lib/test_module.ko"), Verdict::Pass),
                Rule::new(Pattern::new_module("lib/test_module2.ko"), Verdict::Fail),
            ]
        }
    );
}
