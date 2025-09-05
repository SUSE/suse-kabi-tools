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
