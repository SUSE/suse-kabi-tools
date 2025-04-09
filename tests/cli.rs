// Copyright (C) 2025 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitStatus};
use suse_kabi_tools::assert_inexact;

struct RunResult {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn ksymtypes_run<I: IntoIterator<Item = S>, S: AsRef<OsStr>>(args: I) -> RunResult {
    let output = Command::new(env!("CARGO_BIN_EXE_ksymtypes"))
        .args(args)
        .output()
        .expect("failed to execute ksymtypes");
    RunResult {
        status: output.status,
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

#[test]
fn compare_cmd() {
    // Check that the compare command trivially works.
    let result = ksymtypes_run([
        "compare",
        "tests/compare_cmd/a.symtypes",
        "tests/compare_cmd/b.symtypes",
    ]);
    assert!(result.status.success());
    assert_eq!(
        result.stdout,
        concat!(
            "The following '1' exports are different:\n",
            " foo\n",
            "\n",
            "because of a changed 'foo':\n",
            "@@ -1,3 +1,3 @@\n",
            " void foo (\n",
            "-\tint a\n",
            "+\tlong a\n",
            " )\n", //
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
        "tests/compare_cmd/a.symtypes",
        "tests/compare_cmd/b.symtypes",
    ]);
    assert!(result.status.success());
    assert_eq!(
        result.stdout,
        concat!(
            "The following '1' exports are different:\n",
            " foo\n",
            "\n",
            "because of a changed 'foo':\n",
            "@@ -1,3 +1,3 @@\n",
            " void foo (\n",
            "-\tint a\n",
            "+\tlong a\n",
            " )\n", //
        )
    );
    assert_eq!(result.stderr, "");
}

#[test]
fn consolidate_cmd() {
    // Check that the consolidate command trivially works.
    let result = ksymtypes_run(["consolidate", "tests/consolidate_cmd"]);
    assert!(result.status.success());
    assert_eq!(
        result.stdout,
        concat!(
            "/* a.symtypes */\n",
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n",
            "\n",
            "/* b.symtypes */\n",
            "baz int baz ( s#foo )\n", //
        )
    );
    assert_eq!(result.stderr, "");
}

#[test]
fn consolidate_cmd_output() {
    // Check that the consolidate command writes its result to the file specified by --output.
    let output_path =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join("consolidate_cmd_output.symtypes");
    fs::remove_file(&output_path).ok();
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("consolidate"),
        "--output".as_ref(),
        &output_path.as_ref(),
        "tests/consolidate_cmd".as_ref(),
    ]);
    assert!(result.status.success());
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
fn consolidate_cmd_invalid_input() {
    // Check that the consolidate command correctly propagates inner errors and writes them on the
    // standard error output.
    let result = ksymtypes_run(["consolidate", "tests/missing"]);
    assert!(!result.status.success());
    assert_eq!(result.stdout, "");
    assert_inexact!(
        result.stderr,
        "Failed to read symtypes from 'tests/missing': Failed to query path 'tests/missing': *"
    );
}
