// Copyright (C) 2025 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use std::ffi::OsStr;
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

pub fn tmp_path<P: AsRef<Path>>(path: P) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(path)
}
