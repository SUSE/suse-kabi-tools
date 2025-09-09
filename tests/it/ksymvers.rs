// Copyright (C) 2025 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::common::*;

#[test]
fn ksymvers_compare_identical() {
    // Check that the comparison of two identical symvers files shows no differences.
    let result = ksymvers_run([
        "compare",
        "tests/it/ksymvers/compare/a.symvers",
        "tests/it/ksymvers/compare/a.symvers",
    ]);
    assert_eq!(result.status.code().unwrap(), 0);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "");
}

#[test]
fn ksymvers_compare_changed() {
    // Check that the comparison of two different symvers files shows relevant differences and
    // results in the command exiting with a status of 1.
    let result = ksymvers_run([
        "compare",
        "tests/it/ksymvers/compare/a.symvers",
        "tests/it/ksymvers/compare/b.symvers",
    ]);
    assert_eq!(result.status.code().unwrap(), 1);
    assert_eq!(
        result.stdout,
        concat!(
            "Export 'foo' changed CRC from '0x12345678' to '0x09abcdef'\n", //
        )
    );
    assert_eq!(result.stderr, "");
}

#[test]
fn ksymvers_compare_filter_symbol_list() {
    // Check that the comparison of two symvers files can be restricted to specific exports.

    // Check that the result is sensible without a filter.
    let result = ksymvers_run([
        "compare",
        "tests/it/ksymvers/compare_filter_symbol_list/a.symvers",
        "tests/it/ksymvers/compare_filter_symbol_list/b.symvers",
    ]);
    assert_eq!(result.status.code().unwrap(), 1);
    assert_eq!(
        result.stdout,
        concat!(
            "Export 'bar' changed CRC from '0x23456789' to '0xabcdef01'\n",
            "Export 'baz' changed CRC from '0x3456789a' to '0xbcdef012'\n",
            "Export 'foo' changed CRC from '0x12345678' to '0x9abcdef0'\n", //
        )
    );
    assert_eq!(result.stderr, "");

    // Check the result when using a filter.
    let result = ksymvers_run([
        "compare",
        "--filter-symbol-list=tests/it/ksymvers/compare_filter_symbol_list/filter-symbol-list.txt",
        "tests/it/ksymvers/compare_filter_symbol_list/a.symvers",
        "tests/it/ksymvers/compare_filter_symbol_list/b.symvers",
    ]);
    assert_eq!(result.status.code().unwrap(), 1);
    assert_eq!(
        result.stdout,
        concat!(
            "Export 'bar' changed CRC from '0x23456789' to '0xabcdef01'\n",
            "Export 'baz' changed CRC from '0x3456789a' to '0xbcdef012'\n", //
        )
    );
    assert_eq!(result.stderr, "");
}
