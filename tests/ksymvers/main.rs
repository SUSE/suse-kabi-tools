// Copyright (C) 2025 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use std::ffi::OsStr;

#[path = "../common/mod.rs"]
mod common;
use common::*;

fn ksymvers_run<I: IntoIterator<Item = S>, S: AsRef<OsStr>>(args: I) -> RunResult {
    tool_run(env!("CARGO_BIN_EXE_ksymvers"), args)
}

#[test]
fn compare_cmd_identical() {
    // Check that the comparison of two identical symvers files shows no differences.
    let result = ksymvers_run([
        "compare",
        "tests/ksymvers/compare_cmd/a.symvers",
        "tests/ksymvers/compare_cmd/a.symvers",
    ]);
    assert_eq!(result.status.code().unwrap(), 0);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "");
}

#[test]
fn compare_cmd_changed() {
    // Check that the comparison of two different symvers files shows relevant differences and
    // results in the command exiting with a status of 1.
    let result = ksymvers_run([
        "compare",
        "tests/ksymvers/compare_cmd/a.symvers",
        "tests/ksymvers/compare_cmd/b.symvers",
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
