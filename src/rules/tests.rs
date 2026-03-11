// Copyright (C) 2025 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;
use crate::{assert_ok, assert_parse_err, bytes};

#[test]
fn read_classic_module_rule() {
    // Check that a pattern containing '/' or equal to "vmlinux" is considered as a module.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "lib/test_module.ko PASS\n",
            "vmlinux PASS\n", //
        ),
    );
    assert_ok!(result);
    assert_eq!(
        rules,
        Rules {
            data: vec![
                Rule::new(RuleType::Module, "lib/test_module.ko", Verdict::Pass, 0, 0),
                Rule::new(RuleType::Module, "vmlinux", Verdict::Pass, 0, 1),
            ],
            files: vec![PathBuf::from("test.severities")]
        }
    );
}

#[test]
fn read_classic_namespace_rule() {
    // Check that a pattern consisting of only uppercase letter is considered as a namespace.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "TEST_NAMESPACE PASS\n", //
        ),
    );
    assert_ok!(result);
    assert_eq!(
        rules,
        Rules {
            data: vec![Rule::new(
                RuleType::Namespace,
                "TEST_NAMESPACE",
                Verdict::Pass,
                0,
                0
            )],
            files: vec![PathBuf::from("test.severities")]
        }
    );
}

#[test]
fn read_classic_symbol_rule() {
    // Check that a pattern which isn't recognized as a module or a namespace is considered as
    // a symbol.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "symbol_name PASS\n",
            "test_module.ko PASS\n",
            "vmlinux2 PASS\n",
            "test_namespace PASS\n", //
        ),
    );
    assert_ok!(result);
    assert_eq!(
        rules,
        Rules {
            data: vec![
                Rule::new(RuleType::Symbol, "symbol_name", Verdict::Pass, 0, 0),
                Rule::new(RuleType::Symbol, "test_module.ko", Verdict::Pass, 0, 1),
                Rule::new(RuleType::Symbol, "vmlinux2", Verdict::Pass, 0, 2),
                Rule::new(RuleType::Symbol, "test_namespace", Verdict::Pass, 0, 3),
            ],
            files: vec![PathBuf::from("test.severities")]
        }
    );
}

#[test]
fn read_typed_module_rule() {
    // Check that explicitly typed MODULE rules are parsed as such.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "MODULE lib/test_module.ko PASS\n", //
        ),
    );
    assert_ok!(result);
    assert_eq!(
        rules,
        Rules {
            data: vec![Rule::new(
                RuleType::Module,
                "lib/test_module.ko",
                Verdict::Pass,
                0,
                0
            )],
            files: vec![PathBuf::from("test.severities")]
        }
    );
}

#[test]
fn read_typed_namespace_rule() {
    // Check that explicitly typed NAMESPACE rules are parsed as such.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "NAMESPACE TEST_NAMESPACE PASS\n", //
        ),
    );
    assert_ok!(result);
    assert_eq!(
        rules,
        Rules {
            data: vec![Rule::new(
                RuleType::Namespace,
                "TEST_NAMESPACE",
                Verdict::Pass,
                0,
                0
            )],
            files: vec![PathBuf::from("test.severities")]
        }
    );
}

#[test]
fn read_typed_symbol_rule() {
    // Check that explicitly typed NAMESPACE rules are parsed as such.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "SYMBOL symbol_name PASS\n", //
        ),
    );
    assert_ok!(result);
    assert_eq!(
        rules,
        Rules {
            data: vec![Rule::new(
                RuleType::Symbol,
                "symbol_name",
                Verdict::Pass,
                0,
                0
            )],
            files: vec![PathBuf::from("test.severities")]
        }
    );
}

#[test]
fn read_typed_invalid_type() {
    // Check that an explicitly typed rule with an invalid type is rejected.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "MOD lib/test_module.ko PASS\n", //
        ),
    );
    assert_parse_err!(
        result,
        concat!(
            "Invalid rule type 'MOD', must be either MODULE, NAMESPACE or SYMBOL\n",
            " test.severities:1\n",
            " | MOD lib/test_module.ko PASS", //
        ),
    );
    assert_eq!(
        rules,
        Rules {
            data: vec![],
            files: vec![]
        }
    );
}

#[test]
fn read_pass_fail_rule() {
    // Check that the PASS and FAIL verdicts are correctly recognized.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "symbol_name PASS\n",
            "symbol_name2 FAIL\n", //
        ),
    );
    assert_ok!(result);
    assert_eq!(
        rules,
        Rules {
            data: vec![
                Rule::new(RuleType::Symbol, "symbol_name", Verdict::Pass, 0, 0),
                Rule::new(RuleType::Symbol, "symbol_name2", Verdict::Fail, 0, 1),
            ],
            files: vec![PathBuf::from("test.severities")]
        }
    );
}

#[test]
fn read_incomplete_rule() {
    // Check that an incomplete rule is rejected.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "symbol_name\n", //
        ),
    );
    assert_parse_err!(
        result,
        concat!(
            "The rule is incomplete, must be in the form '[type] <pattern> <verdict>'\n",
            " test.severities:1\n",
            " | symbol_name", //
        ),
    );
    assert_eq!(
        rules,
        Rules {
            data: vec![],
            files: vec![]
        }
    );
}

#[test]
fn read_invalid_verdict() {
    // Check that an invalid verdict is rejected.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "symbol_name OK\n", //
        ),
    );
    assert_parse_err!(
        result,
        concat!(
            "Invalid verdict 'OK', must be either PASS or FAIL\n",
            " test.severities:1\n",
            " | symbol_name OK", //
        ),
    );
    assert_eq!(
        rules,
        Rules {
            data: vec![],
            files: vec![]
        }
    );
}

#[test]
fn read_extra_data() {
    // Check that any extra data after the verdict is rejected.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "SYMBOL symbol_name PASS garbage\n", //
        ),
    );
    assert_parse_err!(
        result,
        concat!(
            "Unexpected string found after the verdict\n",
            " test.severities:1\n",
            " | SYMBOL symbol_name PASS garbage", //
        ),
    );
    assert_eq!(
        rules,
        Rules {
            data: vec![],
            files: vec![]
        }
    );
}

#[test]
fn read_empty_record() {
    // Check that empty records are skipped when reading a rules file.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "\n", "\n", //
        ),
    );
    assert_ok!(result);
    assert_eq!(
        rules,
        Rules {
            data: vec![],
            files: vec![PathBuf::from("test.severities")]
        }
    );
}

#[test]
fn read_comments() {
    // Check that comments in various positions are correctly skipped.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "# comment 1\n",
            "lib/test_module.ko PASS # comment 2\n",
            "lib/test_module2.ko FAIL# comment 3\n", //
        ),
    );
    assert_ok!(result);
    assert_eq!(
        rules,
        Rules {
            data: vec![
                Rule::new(RuleType::Module, "lib/test_module.ko", Verdict::Pass, 0, 1),
                Rule::new(RuleType::Module, "lib/test_module2.ko", Verdict::Fail, 0, 2),
            ],
            files: vec![PathBuf::from("test.severities")]
        }
    );
}

#[test]
fn tolerate_symbol() {
    // Check whether a symbol name match in a rules file correctly determines if changes should be
    // tolerated/ignored.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "foo PASS\n",
            "bar FAIL\n",
            "baz* PASS\n", //
        ),
    );
    assert_ok!(result);
    assert!(rules.is_tolerated("foo", "lib/test_module.ko", None));
    assert!(!rules.is_tolerated("bar", "lib/test_module.ko", None));
    assert!(rules.is_tolerated("bazi", "lib/test_module.ko", None));
    assert!(!rules.is_tolerated("qux", "lib/test_module.ko", None));
}

#[test]
fn tolerate_module() {
    // Check whether a module name match in a rules file correctly determines if changes should be
    // tolerated/ignored.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "lib/foo.ko PASS\n",
            "lib/bar.ko FAIL\n",
            "lib/baz*.ko PASS\n", //
        ),
    );
    assert_ok!(result);
    assert!(rules.is_tolerated("symbol_name", "lib/foo.ko", None));
    assert!(!rules.is_tolerated("symbol_name", "lib/bar.ko", None));
    assert!(rules.is_tolerated("symbol_name", "lib/bazi.ko", None));
    assert!(!rules.is_tolerated("symbol_name", "lib/qux.ko", None));
}

#[test]
fn tolerate_namespace() {
    // Check whether a namespace match in a rules file correctly determines if changes should be
    // tolerated/ignored.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "FOO_NS PASS\n",
            "BAR_NS FAIL\n",
            "BAZ*_NS PASS\n", //
        ),
    );
    assert_ok!(result);
    assert!(rules.is_tolerated("symbol_name", "lib/test_module.ko", Some("FOO_NS")));
    assert!(!rules.is_tolerated("symbol_name", "lib/test_module.ko", Some("BAR_NS")));
    assert!(rules.is_tolerated("symbol_name", "lib/test_module.ko", Some("BAZI_NS")));
    assert!(!rules.is_tolerated("symbol_name", "lib/test_module.ko", Some("QUX_NS")));
}

#[test]
fn tolerate_order() {
    // Check that whether a rules file determines if changes should be tolerated/ignored is based on
    // the first match, and not the most specific one.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "foo* PASS\n",
            "foobar FAIL\n", //
        ),
    );
    assert_ok!(result);
    assert!(rules.is_tolerated("foobar", "lib/test_module.ko", None));
}

#[test]
fn mark_used_rules() {
    // Check that used rules are properly marked.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "foo* PASS\n",
            "foobar FAIL\n",
            "MODULE lib/qux.ko FAIL\n",
            "NAMESPACE BAZ_NS FAIL\n", //
        ),
    );
    assert_ok!(result);
    let mut used_rules = UsedRules::new();
    rules.mark_used_rule("foobar", "lib/test_module.ko", None, &mut used_rules);
    assert_eq!(used_rules, UsedRules::from([0]));
    rules.mark_used_rule("baz", "lib/test_module.ko", Some("BAZ_NS"), &mut used_rules);
    assert_eq!(used_rules, UsedRules::from([0, 3]));
}

#[test]
fn write_unused_rules() {
    // Check that unused rules are reported as such.
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "foo* PASS\n",
            "foobar FAIL\n",
            "MODULE lib/qux.ko FAIL\n",
            "NAMESPACE BAZ_NS FAIL\n", //
        ),
    );
    assert_ok!(result);
    let used_rules = UsedRules::from([0, 3]);
    let mut out = Vec::new();
    let result = rules.write_unused_rules_buffer(&used_rules, &mut out);
    assert_ok!(result);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "test.severities:2: WARNING: Severity rule 'SYMBOL foobar FAIL' is unused\n",
            "test.severities:3: WARNING: Severity rule 'MODULE lib/qux.ko FAIL' is unused\n", //
        )
    );
}
