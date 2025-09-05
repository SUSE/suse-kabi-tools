// Copyright (C) 2025 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::common::*;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use suse_kabi_tools::assert_inexact;

#[test]
fn ksymtypes_consolidate() {
    // Check that the consolidate command trivially works.
    let output_path = tmp_path("it/ksymtypes/consolidate.symtypes");
    fs::remove_file(&output_path).ok();
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("consolidate"),
        "--output".as_ref(),
        &output_path.as_ref(),
        "tests/it/ksymtypes/consolidate".as_ref(),
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
fn ksymtypes_consolidate_missing_output() {
    // Check that the consolidate command fails if no --output is specified.
    let result = ksymtypes_run(["consolidate", "tests/it/ksymtypes/consolidate"]);
    assert_eq!(result.status.code().unwrap(), 2);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "The consolidate output is missing\n");
}

#[test]
fn ksymtypes_consolidate_invalid_input() {
    // Check that the consolidate command correctly propagates inner errors and writes them on the
    // standard error output.
    let output_path = tmp_path("it/ksymtypes/consolidate_invalid_input.symtypes");
    fs::remove_file(&output_path).ok();
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("consolidate"),
        "--output".as_ref(),
        &output_path.as_ref(),
        "tests/it/ksymtypes/missing".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 2);
    assert_eq!(result.stdout, "");
    assert_inexact!(
        result.stderr,
        "Failed to read symtypes from 'tests/it/ksymtypes/missing': Failed to read the directory 'tests/it/ksymtypes/missing/': *\n"
    );
}

#[test]
fn ksymtypes_consolidate_non_directory() {
    // Check that the consolidate command rejects an input path that is not a directory.
    let output_path = tmp_path("ksymtypes/consolidate_non_directory.symtypes");
    fs::remove_file(&output_path).ok();
    let input_path = Path::new("tests/it/ksymtypes/consolidate_non_directory/a.symtypes");
    assert!(input_path.is_file());
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("consolidate"),
        "--output".as_ref(),
        &output_path.as_ref(),
        input_path.as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 2);
    assert_eq!(result.stdout, "");
    assert_inexact!(
        result.stderr,
        "Failed to read symtypes from 'tests/it/ksymtypes/consolidate_non_directory/a.symtypes': Failed to read the directory 'tests/it/ksymtypes/consolidate_non_directory/a.symtypes/': *\n"
    );
}

#[test]
fn ksymtypes_consolidate_reject_consolidated() {
    // Check that the consolidate command rejects loading any symtypes files in the consolidated
    // format.
    let output_path = tmp_path("it/consolidate/reject_consolidated.symtypes");
    fs::remove_file(&output_path).ok();
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("consolidate"),
        "--output".as_ref(),
        &output_path.as_ref(),
        "tests/it/ksymtypes/consolidate_reject_consolidated".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 2);
    assert_eq!(result.stdout, "");
    assert_eq!(
        result.stderr,
        "Failed to read symtypes from 'tests/it/ksymtypes/consolidate_reject_consolidated': consolidated.symtypes:1: Expected a plain symtypes file, but found consolidated data\n"
    );
}

#[test]
fn ksymtypes_split() {
    // Check that the split command trivially works.
    let output_path = tmp_path("it/ksymtypes/split");
    fs::remove_dir_all(&output_path).ok();
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("split"),
        "--output".as_ref(),
        &output_path.as_ref(),
        "tests/it/ksymtypes/split/consolidated.symtypes".as_ref(),
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
fn ksymtypes_split_missing_output() {
    // Check that the split command fails if no --output is specified.
    let result = ksymtypes_run(["split", "tests/it/ksymtypes/split/consolidated.symtypes"]);
    assert_eq!(result.status.code().unwrap(), 2);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "The split output is missing\n");
}

#[test]
fn ksymtypes_split_invalid_input() {
    // Check that the split command correctly propagates inner errors and writes them on the
    // standard error output.
    let output_path = tmp_path("it/ksymtypes/split_invalid_input");
    fs::remove_file(&output_path).ok();
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("split"),
        "--output".as_ref(),
        &output_path.as_ref(),
        "tests/it/ksymtypes/missing".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 2);
    assert_eq!(result.stdout, "");
    assert_inexact!(
        result.stderr,
        "Failed to read symtypes from 'tests/it/ksymtypes/missing': Failed to open the file 'tests/it/ksymtypes/missing': *\n"
    );
}

#[test]
fn ksymtypes_split_non_file() {
    // Check that the split command rejects an input path that is not a file.
    let output_path = tmp_path("it/ksymtypes/split_non_file");
    fs::remove_file(&output_path).ok();
    let input_path = Path::new("tests/it/ksymtypes/split_non_file");
    assert!(input_path.is_dir());
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("split"),
        "--output".as_ref(),
        &output_path.as_ref(),
        input_path.as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 2);
    assert_eq!(result.stdout, "");
    assert_inexact!(
        result.stderr,
        "Failed to read symtypes from 'tests/it/ksymtypes/split_non_file': Failed to read symtypes data: Failed to read from the file 'tests/it/ksymtypes/split_non_file': *\n"
    );
}

#[test]
fn ksymtypes_split_reject_plain() {
    // Check that the split command rejects loading a symtypes file in the non-consolidated format.
    let output_path = tmp_path("it/split/reject_plain");
    fs::remove_file(&output_path).ok();
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("split"),
        "--output".as_ref(),
        &output_path.as_ref(),
        "tests/it/ksymtypes/split_reject_plain/a.symtypes".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 2);
    assert_eq!(result.stdout, "");
    assert_eq!(
        result.stderr,
        "Failed to read symtypes from 'tests/it/ksymtypes/split_reject_plain/a.symtypes': tests/it/ksymtypes/split_reject_plain/a.symtypes:1: Expected a consolidated symtypes file, but found an invalid header\n"
    );
}

#[test]
fn ksymtypes_compare() {
    // Check that the comparison of two different symtypes files shows relevant differences and
    // results in the command exiting with a status of 1.
    let result = ksymtypes_run([
        "compare",
        "tests/it/ksymtypes/compare/a.symtypes",
        "tests/it/ksymtypes/compare/b.symtypes",
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
fn ksymtypes_compare_dash_dash() {
    // Check that operands of the compare command can be specified after '--'.
    let result = ksymtypes_run([
        "compare",
        "--",
        "tests/it/ksymtypes/compare/a.symtypes",
        "tests/it/ksymtypes/compare/b.symtypes",
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
fn ksymtypes_compare_split_and_consolidated() {
    // Check that the compare command works when one input is a directory with split symtypes files
    // and the second input is a consolidated symtypes file.
    let result = ksymtypes_run([
        "compare",
        "tests/it/ksymtypes/compare_split_and_consolidated/a",
        "tests/it/ksymtypes/compare_split_and_consolidated/b.symtypes",
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
