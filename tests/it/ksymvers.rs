// Copyright (C) 2025 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::common::*;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

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

#[test]
fn ksymvers_compare_rules() {
    // Check that severity rules can be used to tolerate changes.
    let result = ksymvers_run([
        "compare",
        "--rules=tests/it/ksymvers/compare_rules/severities.txt",
        "tests/it/ksymvers/compare_rules/a.symvers",
        "tests/it/ksymvers/compare_rules/b.symvers",
    ]);
    assert_eq!(result.status.code().unwrap(), 0);
    assert_eq!(
        result.stdout,
        concat!(
            "Export 'foo' changed CRC from '0x12345678' to '0x9abcdef0' (tolerated by rules)\n", //
        )
    );
    assert_eq!(result.stderr, "");
}

#[test]
fn ksymvers_compare_format() {
    // Check that the comparison allows specifying the output format.
    //
    // NOTE: Keep this test synchronized with the compare_format_short symvers unit test.
    fn expected_path(file: &str) -> PathBuf {
        Path::new("tests/it/ksymvers/compare_format/").join(file)
    }

    fn tmp_path(file: &str) -> PathBuf {
        crate::common::tmp_path(Path::new("tests/it/ksymvers/compare_format/").join(file))
    }

    fs::remove_dir_all(tmp_path("")).ok();

    let pretty_out_path = tmp_path("pretty.out");
    let short_out_path = tmp_path("short.out");
    let symbols_out_path = tmp_path("symbols.out");
    let mod_symbols_out_path = tmp_path("mod_symbols.out");
    let result = ksymvers_run([
        AsRef::<OsStr>::as_ref("compare"),
        "--rules=tests/it/ksymvers/compare_format/severities.txt".as_ref(),
        "--format=null".as_ref(),
        &concat_os("--format=pretty:", &pretty_out_path),
        &concat_os("--format=short:", &short_out_path),
        &concat_os("--format=symbols:", &symbols_out_path),
        &concat_os("--format=mod-symbols:", &mod_symbols_out_path),
        "tests/it/ksymvers/compare_format/a.symvers".as_ref(),
        "tests/it/ksymvers/compare_format/b.symvers".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 1);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "");

    let pretty_out = fs::read_to_string(&pretty_out_path).unwrap();
    let pretty_exp = fs::read_to_string(expected_path("pretty.exp")).unwrap();
    assert_eq!(pretty_out, pretty_exp);

    let short_out = fs::read_to_string(&short_out_path).unwrap();
    let short_exp = fs::read_to_string(expected_path("short.exp")).unwrap();
    assert_eq!(short_out, short_exp);

    let symbols_out = fs::read_to_string(&symbols_out_path).unwrap();
    let symbols_exp = fs::read_to_string(expected_path("symbols.exp")).unwrap();
    assert_eq!(symbols_out, symbols_exp);

    let mod_symbols_out = fs::read_to_string(&mod_symbols_out_path).unwrap();
    let mod_symbols_exp = fs::read_to_string(expected_path("mod_symbols.exp")).unwrap();
    assert_eq!(mod_symbols_out, mod_symbols_exp);
}
