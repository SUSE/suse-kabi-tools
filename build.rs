// Copyright (C) 2025 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use std::fs;
use std::process::Command;

fn run(name: &str, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to execute {}: {}", name, err));
    if !output.status.success() {
        panic!("{} exited with error: {}", name, output.status);
    }
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("output from {} is not valid UTF-8: {}", name, err))
}

fn main() {
    // Check if the version is explicitly set, for instance, by a distribution package recipe.
    if let Ok(raw_version) = fs::read_to_string("VERSION") {
        let version = raw_version.trim();
        println!("cargo:rustc-env=SUSE_KABI_TOOLS_VERSION={}", version);
        // Note that the following statement would ideally be moved outside of the if block to allow
        // detecting when the VERSION file is added. Unfortunately, this is not possible because if
        // the file is missing, it would always trigger a rerun of build.rs.
        println!("cargo:rerun-if-changed=VERSION");
        return;
    }

    // Execute git-describe to retrieve version information.
    let raw_version = run("git", &["describe", "--dirty"]);
    let version = raw_version.trim().strip_prefix('v').unwrap_or(&raw_version);
    println!("cargo:rustc-env=SUSE_KABI_TOOLS_VERSION={}", version);

    // List items that the `git describe --dirty` command depends on.
    let ls_files = run("git", &["ls-files"]);
    for file in ls_files.lines() {
        println!("cargo:rerun-if-changed={}", file);
    }
    println!("cargo:rerun-if-changed=.git/HEAD");
    let raw_head = fs::read_to_string(".git/HEAD").expect("file .git/HEAD should be readable");
    if let Some(head) = raw_head.trim().strip_prefix("ref: ") {
        println!("cargo:rerun-if-changed=.git/{}", head);
    }
    println!("cargo:rerun-if-changed=.git/index");
}
