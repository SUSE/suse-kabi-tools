// Copyright (C) 2025 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use std::ffi::OsStr;
use std::fs;
use suse_kabi_tools::assert_inexact;

#[path = "../common/mod.rs"]
mod common;
use common::*;

fn ksymtypes_run<I: IntoIterator<Item = S>, S: AsRef<OsStr>>(args: I) -> RunResult {
    tool_run(env!("CARGO_BIN_EXE_ksymtypes"), args)
}

#[test]
fn consolidate_cmd() {
    // Check that the consolidate command trivially works.
    let output_path = tmp_path("consolidate_cmd.symtypes");
    fs::remove_file(&output_path).ok();
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("consolidate"),
        "--output".as_ref(),
        &output_path.as_ref(),
        "tests/ksymtypes/consolidate_cmd".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 0);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "");
    let output_data = fs::read_to_string(output_path).expect("Unable to read the output file");
    assert_eq!(
        output_data,
        concat!(
            "/* a.symtypes */\n",
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n",
            "\n",
            "/* b.symtypes */\n",
            "baz int baz ( s#foo )\n", //
        )
    );
}

#[test]
fn consolidate_cmd_missing_output() {
    // Check that the consolidate command fails if no --output is specified.
    let result = ksymtypes_run(["consolidate", "tests/ksymtypes/consolidate_cmd"]);
    assert_eq!(result.status.code().unwrap(), 2);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "The consolidate output is missing\n");
}

#[test]
fn consolidate_cmd_invalid_input() {
    // Check that the consolidate command correctly propagates inner errors and writes them on the
    // standard error output.
    let output_path = tmp_path("consolidate_cmd_invalid_input.symtypes");
    fs::remove_file(&output_path).ok();
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("consolidate"),
        "--output".as_ref(),
        &output_path.as_ref(),
        "tests/missing".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 2);
    assert_eq!(result.stdout, "");
    assert_inexact!(
        result.stderr,
        "Failed to read symtypes from 'tests/missing': Failed to query path 'tests/missing': *\n"
    );
}

#[test]
fn split_cmd() {
    // Check that the split command trivially works.
    let output_path = tmp_path("split_cmd_output");
    fs::remove_dir_all(&output_path).ok();
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("split"),
        "--output".as_ref(),
        &output_path.as_ref(),
        "tests/ksymtypes/split_cmd/consolidated.symtypes".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 0);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "");
    assert_eq!(
        fs::read_to_string(output_path.join("a.symtypes")).unwrap(),
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n", //
        )
    );
    assert_eq!(
        fs::read_to_string(output_path.join("b.symtypes")).unwrap(),
        concat!(
            "s#foo struct foo { int a ; }\n",
            "baz int baz ( s#foo )\n", //
        )
    );
}

#[test]
fn split_cmd_missing_output() {
    // Check that the split command fails if no --output is specified.
    let result = ksymtypes_run(["split", "tests/ksymtypes/split_cmd/consolidated.symtypes"]);
    assert_eq!(result.status.code().unwrap(), 2);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "The split output is missing\n");
}

#[test]
fn compare_cmd() {
    // Check that the comparison of two different symtypes files shows relevant differences and
    // results in the command exiting with a status of 1.
    let result = ksymtypes_run([
        "compare",
        "tests/ksymtypes/compare_cmd/a.symtypes",
        "tests/ksymtypes/compare_cmd/b.symtypes",
    ]);
    assert_eq!(result.status.code().unwrap(), 1);
    assert_eq!(
        result.stdout,
        concat!(
            "The following '1' exports are different:\n",
            " foo\n",
            "\n",
            "because of a changed 'foo':\n",
            "@@ -1,1 +1,1 @@\n",
            "-void foo ( int a )\n",
            "+void foo ( long a )\n", //
        )
    );
    assert_eq!(result.stderr, "");
}

#[test]
fn compare_cmd_dash_dash() {
    // Check that operands of the compare command can be specified after '--'.
    let result = ksymtypes_run([
        "compare",
        "--",
        "tests/ksymtypes/compare_cmd/a.symtypes",
        "tests/ksymtypes/compare_cmd/b.symtypes",
    ]);
    assert_eq!(result.status.code().unwrap(), 1);
    assert_eq!(
        result.stdout,
        concat!(
            "The following '1' exports are different:\n",
            " foo\n",
            "\n",
            "because of a changed 'foo':\n",
            "@@ -1,1 +1,1 @@\n",
            "-void foo ( int a )\n",
            "+void foo ( long a )\n", //
        )
    );
    assert_eq!(result.stderr, "");
}
