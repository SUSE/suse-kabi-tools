// Copyright (C) 2025 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

#[path = "../common/mod.rs"]
mod common;

use crate::common::*;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

// The SL-16.0 tests examine real-world situations using files from the respective SUSE kernel.
//
// The symvers and symtypes test data come from the repository at
// https://github.com/SUSE/kernel-source, branch SL-16.0.
//
// * base: taken at commit 0c9b3adf9bb1 ("kABI: update kABI symbols") from 2025-06-30, which froze
//   the kABI.
// * new: the data are from an actual build at commit df71eeba0a01 ("Merge branch 'SL-16.0-GA' into
//   SL-16.0") from 2025-08-22, containing 19 kABI fixups.
// * new-broken: the data from the same version, but with 4 dropped kABI fixup patches.
//
// kABI fixups dropped by new-broken:
//
// * patches.kabi/bpf-Do-not-include-stack-ptr-register-in-precision-b.patch: affects
//   bpf_jmp_history_entry.
// * patches.kabi/drm_gem-kabi-workaround.patch: a new include linux/dma-buf.h makes several
//   structures defined.
// * patches.kabi/kABI-Fix-the-module-name-type-in-audit_context.patch: affects audit_context.
// * patches.kabi/kABI-fix-for-net-vlan-fix-VLAN-0-refcount-imbalance-.patch: affects vlan_info.

fn concat_os<S: AsRef<OsStr>, S2: AsRef<OsStr>>(s: S, s2: S2) -> OsString {
    let s = s.as_ref();
    let s2 = s2.as_ref();

    let mut res = OsString::with_capacity(s.len() + s2.len());
    res.push(s);
    res.push(s2);
    res
}

#[test]
#[cfg_attr(feature = "skip_expensive_tests", ignore)]
fn sl_16_0_base_vs_new_with_rules() {
    // Check the comparison between the SL-16.0 reference and an updated kernel that preserves the
    // kABI, using the SUSE kernel severity rules. The tools should not report any non-tolerated
    // kABI differences.
    fn expected_path(file: &str) -> PathBuf {
        Path::new("tests/sl/sl_16_0/expected/base_vs_new_with_rules/").join(file)
    }

    fn tmp_path(file: &str) -> PathBuf {
        crate::common::tmp_path(Path::new("tests/sl/sl_16_0/base_vs_new_with_rules/").join(file))
    }

    fs::remove_dir_all(tmp_path("")).ok();

    // Check the ksymvers comparison with the various --format options.
    let pretty_out_path = tmp_path("ksymvers_compare_pretty.out");
    let short_out_path = tmp_path("ksymvers_compare_short.out");
    let symbols_out_path = tmp_path("ksymvers_compare_symbols.out");
    let mod_symbols_out_path = tmp_path("ksymvers_compare_mod_symbols.out");
    let result = ksymvers_run([
        AsRef::<OsStr>::as_ref("compare"),
        "--rules=tests/sl/sl_16_0/input/severities".as_ref(),
        "--format=null".as_ref(),
        &concat_os("--format=pretty:", &pretty_out_path),
        &concat_os("--format=short:", &short_out_path),
        &concat_os("--format=symbols:", &symbols_out_path),
        &concat_os("--format=mod-symbols:", &mod_symbols_out_path),
        "tests/sl/sl_16_0/input/base/symvers-default".as_ref(),
        "tests/sl/sl_16_0/input/new/symvers-default".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 0);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "");

    let pretty_out = fs::read_to_string(&pretty_out_path).unwrap();
    let pretty_exp = fs::read_to_string(expected_path("ksymvers_compare_pretty.exp")).unwrap();
    assert_eq!(pretty_out, pretty_exp);

    let short_out = fs::read_to_string(&short_out_path).unwrap();
    let short_exp = fs::read_to_string(expected_path("ksymvers_compare_short.exp")).unwrap();
    assert_eq!(short_out, short_exp);

    let symbols_out = fs::read_to_string(&symbols_out_path).unwrap();
    let symbols_exp = fs::read_to_string(expected_path("ksymvers_compare_symbols.exp")).unwrap();
    assert_eq!(symbols_out, symbols_exp);

    let mod_symbols_out = fs::read_to_string(&mod_symbols_out_path).unwrap();
    let mod_symbols_exp =
        fs::read_to_string(expected_path("ksymvers_compare_mod_symbols.exp")).unwrap();
    assert_eq!(mod_symbols_out, mod_symbols_exp);

    // Check the ksymtypes comparison with the various --format options.
    let pretty2_out_path = tmp_path("ksymtypes_compare_pretty.out");
    let short2_out_path = tmp_path("ksymtypes_compare_short.out");
    let symbols2_out_path = tmp_path("ksymtypes_compare_symbols.out");
    let mod_symbols2_out_path = tmp_path("ksymtypes_compare_mod_symbols.out");
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("compare"),
        "-j8".as_ref(),
        &concat_os("--filter-symbol-list=", &symbols_out_path),
        "--format=null".as_ref(),
        &concat_os("--format=pretty:", &pretty2_out_path),
        &concat_os("--format=short:", &short2_out_path),
        &concat_os("--format=symbols:", &symbols2_out_path),
        &concat_os("--format=mod-symbols:", &mod_symbols2_out_path),
        "tests/sl/sl_16_0/input/base/symtypes-default".as_ref(),
        "tests/sl/sl_16_0/input/new/symtypes-default".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 0);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "");

    let pretty2_out = fs::read_to_string(&pretty2_out_path).unwrap();
    let pretty2_exp = fs::read_to_string(expected_path("ksymtypes_compare_pretty.exp")).unwrap();
    assert_eq!(pretty2_out, pretty2_exp);

    let short2_out = fs::read_to_string(&short2_out_path).unwrap();
    let short2_exp = fs::read_to_string(expected_path("ksymtypes_compare_short.exp")).unwrap();
    assert_eq!(short2_out, short2_exp);

    let symbols2_out = fs::read_to_string(&symbols2_out_path).unwrap();
    let symbols2_exp = fs::read_to_string(expected_path("ksymtypes_compare_symbols.exp")).unwrap();
    assert_eq!(symbols2_out, symbols2_exp);

    let mod_symbols2_out = fs::read_to_string(&mod_symbols2_out_path).unwrap();
    let mod_symbols2_exp =
        fs::read_to_string(expected_path("ksymtypes_compare_mod_symbols.exp")).unwrap();
    assert_eq!(mod_symbols2_out, mod_symbols2_exp);
}

#[test]
#[cfg_attr(feature = "skip_expensive_tests", ignore)]
fn sl_16_0_base_vs_new_without_rules() {
    // Check the comparison between the SL-16.0 reference and an updated kernel that preserves the
    // kABI, without using the SUSE kernel severity rules. The tools should report several kABI
    // differences that would be normally tolerated, such as those affecting KVM symbols.
    fn expected_path(file: &str) -> PathBuf {
        Path::new("tests/sl/sl_16_0/expected/base_vs_new_without_rules/").join(file)
    }

    fn tmp_path(file: &str) -> PathBuf {
        crate::common::tmp_path(Path::new("tests/sl/sl_16_0/base_vs_new_without_rules/").join(file))
    }

    fs::remove_dir_all(tmp_path("")).ok();

    // Check the ksymvers comparison with the various --format options.
    let pretty_out_path = tmp_path("ksymvers_compare_pretty.out");
    let short_out_path = tmp_path("ksymvers_compare_short.out");
    let symbols_out_path = tmp_path("ksymvers_compare_symbols.out");
    let mod_symbols_out_path = tmp_path("ksymvers_compare_mod_symbols.out");
    let result = ksymvers_run([
        AsRef::<OsStr>::as_ref("compare"),
        "--format=null".as_ref(),
        &concat_os("--format=pretty:", &pretty_out_path),
        &concat_os("--format=short:", &short_out_path),
        &concat_os("--format=symbols:", &symbols_out_path),
        &concat_os("--format=mod-symbols:", &mod_symbols_out_path),
        "tests/sl/sl_16_0/input/base/symvers-default".as_ref(),
        "tests/sl/sl_16_0/input/new/symvers-default".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 1);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "");

    let pretty_out = fs::read_to_string(&pretty_out_path).unwrap();
    let pretty_exp = fs::read_to_string(expected_path("ksymvers_compare_pretty.exp")).unwrap();
    assert_eq!(pretty_out, pretty_exp);

    let short_out = fs::read_to_string(&short_out_path).unwrap();
    let short_exp = fs::read_to_string(expected_path("ksymvers_compare_short.exp")).unwrap();
    assert_eq!(short_out, short_exp);

    let symbols_out = fs::read_to_string(&symbols_out_path).unwrap();
    let symbols_exp = fs::read_to_string(expected_path("ksymvers_compare_symbols.exp")).unwrap();
    assert_eq!(symbols_out, symbols_exp);

    let mod_symbols_out = fs::read_to_string(&mod_symbols_out_path).unwrap();
    let mod_symbols_exp =
        fs::read_to_string(expected_path("ksymvers_compare_mod_symbols.exp")).unwrap();
    assert_eq!(mod_symbols_out, mod_symbols_exp);

    // Check the ksymtypes comparison with the various --format options.
    let pretty2_out_path = tmp_path("ksymtypes_compare_pretty.out");
    let short2_out_path = tmp_path("ksymtypes_compare_short.out");
    let symbols2_out_path = tmp_path("ksymtypes_compare_symbols.out");
    let mod_symbols2_out_path = tmp_path("ksymtypes_compare_mod_symbols.out");
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("compare"),
        "-j8".as_ref(),
        &concat_os("--filter-symbol-list=", &symbols_out_path),
        "--format=null".as_ref(),
        &concat_os("--format=pretty:", &pretty2_out_path),
        &concat_os("--format=short:", &short2_out_path),
        &concat_os("--format=symbols:", &symbols2_out_path),
        &concat_os("--format=mod-symbols:", &mod_symbols2_out_path),
        "tests/sl/sl_16_0/input/base/symtypes-default".as_ref(),
        "tests/sl/sl_16_0/input/new/symtypes-default".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 1);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "");

    let pretty2_out = fs::read_to_string(&pretty2_out_path).unwrap();
    let pretty2_exp = fs::read_to_string(expected_path("ksymtypes_compare_pretty.exp")).unwrap();
    assert_eq!(pretty2_out, pretty2_exp);

    let short2_out = fs::read_to_string(&short2_out_path).unwrap();
    let short2_exp = fs::read_to_string(expected_path("ksymtypes_compare_short.exp")).unwrap();
    assert_eq!(short2_out, short2_exp);

    let symbols2_out = fs::read_to_string(&symbols2_out_path).unwrap();
    let symbols2_exp = fs::read_to_string(expected_path("ksymtypes_compare_symbols.exp")).unwrap();
    assert_eq!(symbols2_out, symbols2_exp);

    let mod_symbols2_out = fs::read_to_string(&mod_symbols2_out_path).unwrap();
    let mod_symbols2_exp =
        fs::read_to_string(expected_path("ksymtypes_compare_mod_symbols.exp")).unwrap();
    assert_eq!(mod_symbols2_out, mod_symbols2_exp);
}

#[test]
#[cfg_attr(feature = "skip_expensive_tests", ignore)]
fn sl_16_0_base_vs_broken_with_rules() {
    // Check the comparison between the SL-16.0 reference and an updated kernel that breaks kABI,
    // using the SUSE kernel severity rules. The tools should report several kABI differences
    // resulting from the removal of kABI workarounds.
    fn expected_path(file: &str) -> PathBuf {
        Path::new("tests/sl/sl_16_0/expected/base_vs_broken_with_rules/").join(file)
    }

    fn tmp_path(file: &str) -> PathBuf {
        crate::common::tmp_path(Path::new("tests/sl/sl_16_0/base_vs_broken_with_rules/").join(file))
    }

    fs::remove_dir_all(tmp_path("")).ok();

    // Check the ksymvers comparison with the various --format options.
    let pretty_out_path = tmp_path("ksymvers_compare_pretty.out");
    let short_out_path = tmp_path("ksymvers_compare_short.out");
    let symbols_out_path = tmp_path("ksymvers_compare_symbols.out");
    let mod_symbols_out_path = tmp_path("ksymvers_compare_mod_symbols.out");
    let result = ksymvers_run([
        AsRef::<OsStr>::as_ref("compare"),
        "--rules=tests/sl/sl_16_0/input/severities".as_ref(),
        "--format=null".as_ref(),
        &concat_os("--format=pretty:", &pretty_out_path),
        &concat_os("--format=short:", &short_out_path),
        &concat_os("--format=symbols:", &symbols_out_path),
        &concat_os("--format=mod-symbols:", &mod_symbols_out_path),
        "tests/sl/sl_16_0/input/base/symvers-default".as_ref(),
        "tests/sl/sl_16_0/input/broken/symvers-default".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 1);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "");

    let pretty_out = fs::read_to_string(&pretty_out_path).unwrap();
    let pretty_exp = fs::read_to_string(expected_path("ksymvers_compare_pretty.exp")).unwrap();
    assert_eq!(pretty_out, pretty_exp);

    let short_out = fs::read_to_string(&short_out_path).unwrap();
    let short_exp = fs::read_to_string(expected_path("ksymvers_compare_short.exp")).unwrap();
    assert_eq!(short_out, short_exp);

    let symbols_out = fs::read_to_string(&symbols_out_path).unwrap();
    let symbols_exp = fs::read_to_string(expected_path("ksymvers_compare_symbols.exp")).unwrap();
    assert_eq!(symbols_out, symbols_exp);

    let mod_symbols_out = fs::read_to_string(&mod_symbols_out_path).unwrap();
    let mod_symbols_exp =
        fs::read_to_string(expected_path("ksymvers_compare_mod_symbols.exp")).unwrap();
    assert_eq!(mod_symbols_out, mod_symbols_exp);

    // Check the ksymtypes comparison with the various --format options.
    let pretty2_out_path = tmp_path("ksymtypes_compare_pretty.out");
    let short2_out_path = tmp_path("ksymtypes_compare_short.out");
    let symbols2_out_path = tmp_path("ksymtypes_compare_symbols.out");
    let mod_symbols2_out_path = tmp_path("ksymtypes_compare_mod_symbols.out");
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("compare"),
        "-j8".as_ref(),
        &concat_os("--filter-symbol-list=", &symbols_out_path),
        "--format=null".as_ref(),
        &concat_os("--format=pretty:", &pretty2_out_path),
        &concat_os("--format=short:", &short2_out_path),
        &concat_os("--format=symbols:", &symbols2_out_path),
        &concat_os("--format=mod-symbols:", &mod_symbols2_out_path),
        "tests/sl/sl_16_0/input/base/symtypes-default".as_ref(),
        "tests/sl/sl_16_0/input/broken/symtypes-default".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 1);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "");

    let pretty2_out = fs::read_to_string(&pretty2_out_path).unwrap();
    let pretty2_exp = fs::read_to_string(expected_path("ksymtypes_compare_pretty.exp")).unwrap();
    assert_eq!(pretty2_out, pretty2_exp);

    let short2_out = fs::read_to_string(&short2_out_path).unwrap();
    let short2_exp = fs::read_to_string(expected_path("ksymtypes_compare_short.exp")).unwrap();
    assert_eq!(short2_out, short2_exp);

    let symbols2_out = fs::read_to_string(&symbols2_out_path).unwrap();
    let symbols2_exp = fs::read_to_string(expected_path("ksymtypes_compare_symbols.exp")).unwrap();
    assert_eq!(symbols2_out, symbols2_exp);

    let mod_symbols2_out = fs::read_to_string(&mod_symbols2_out_path).unwrap();
    let mod_symbols2_exp =
        fs::read_to_string(expected_path("ksymtypes_compare_mod_symbols.exp")).unwrap();
    assert_eq!(mod_symbols2_out, mod_symbols2_exp);
}

#[test]
#[cfg_attr(feature = "skip_expensive_tests", ignore)]
fn sl_16_0_new_split_consolidate() {
    // Check that splitting the symtypes reference into individual files and then consolidating them
    // back produces a symtypes file that matches the original. Note that the test is performed on
    // the "new" SL-16.0 data, since the "base" data was produced using the old modversions script,
    // which has a different record ordering.
    fn tmp_path(file: &str) -> PathBuf {
        crate::common::tmp_path(Path::new("tests/sl/sl_16_0/new_split_consolidate/").join(file))
    }

    fs::remove_dir_all(tmp_path("")).ok();

    // Split the symtypes corpus.
    let split_out_path = tmp_path("split");
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("split"),
        "-j8".as_ref(),
        &concat_os("--output=", &split_out_path),
        "tests/sl/sl_16_0/input/new/symtypes-default".as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 0);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "");

    // Consolidate the symtypes corpus back.
    let consolidate_out_path = tmp_path("consolidate.symtypes");
    let result = ksymtypes_run([
        AsRef::<OsStr>::as_ref("consolidate"),
        "-j8".as_ref(),
        &concat_os("--output=", &consolidate_out_path),
        split_out_path.as_ref(),
    ]);
    assert_eq!(result.status.code().unwrap(), 0);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "");

    // Check that the new symtypes corpus matches the original.
    let consolidate_out = fs::read_to_string(&consolidate_out_path).unwrap();
    let consolidate_exp =
        fs::read_to_string("tests/sl/sl_16_0/input/new/symtypes-default").unwrap();
    assert_eq!(consolidate_out, consolidate_exp);
}
