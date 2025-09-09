// Copyright (C) 2025 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

pub struct RunResult {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

pub fn tool_run<P: AsRef<OsStr>, I: IntoIterator<Item = S>, S: AsRef<OsStr>>(
    program: P,
    args: I,
) -> RunResult {
    let program = program.as_ref();
    let output = Command::new(program)
        .args(args)
        .output()
        .expect(&format!("failed to execute {:?}", program));
    RunResult {
        status: output.status,
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

pub fn ksymtypes_run<I: IntoIterator<Item = S>, S: AsRef<OsStr>>(args: I) -> RunResult {
    tool_run(env!("CARGO_BIN_EXE_ksymtypes"), args)
}

pub fn ksymvers_run<I: IntoIterator<Item = S>, S: AsRef<OsStr>>(args: I) -> RunResult {
    tool_run(env!("CARGO_BIN_EXE_ksymvers"), args)
}

pub fn tmp_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let res = Path::new(env!("CARGO_TARGET_TMPDIR")).join(path);
    if let Some(parent) = res.parent() {
        fs::create_dir_all(parent).unwrap()
    }
    res
}

pub fn concat_os<S: AsRef<OsStr>, S2: AsRef<OsStr>>(s: S, s2: S2) -> OsString {
    let s = s.as_ref();
    let s2 = s2.as_ref();

    let mut res = OsString::with_capacity(s.len() + s2.len());
    res.push(s);
    res.push(s2);
    res
}
