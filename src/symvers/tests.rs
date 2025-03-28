// Copyright (C) 2025 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;
use crate::{assert_inexact_parse_err, assert_ok, assert_parse_err};

#[test]
fn read_export_basic() {
    // Check that basic parsing works correctly.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "test.symvers",
        concat!(
            "0x12345678\tfoo\tvmlinux\tEXPORT_SYMBOL\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_eq!(
        symvers,
        Symvers {
            exports: HashMap::from([(
                "foo".to_string(),
                ExportInfo::new(0x12345678, "vmlinux", false, None::<&str>)
            )])
        }
    );
}

#[test]
fn read_empty_record() {
    // Check that empty records are rejected when reading a symvers file.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "test.symvers",
        concat!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "\n",
            "0x90abcdef bar vmlinux EXPORT_SYMBOL_GPL BAR_NS\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(result, "test.symvers:2: The export does not specify a CRC");
    assert_eq!(symvers, Symvers::new());
}

#[test]
fn read_invalid_crc() {
    // Check that a CRC value not starting with 0x/0X is rejected.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "test.symvers",
        concat!(
            "0 foo vmlinux EXPORT_SYMBOL\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(
        result,
        "test.symvers:1: Failed to parse the CRC value '0': string does not start with 0x or 0X"
    );
    assert_eq!(symvers, Symvers::new());
}

#[test]
fn read_invalid_crc2() {
    // Check that a CRC value containing non-hexadecimal digits is rejected.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "test.symvers",
        concat!(
            "0xabcdefgh foo vmlinux EXPORT_SYMBOL\n", //
        )
        .as_bytes(),
    );
    assert_inexact_parse_err!(
        result,
        "test.symvers:1: Failed to parse the CRC value '0xabcdefgh': *"
    );
    assert_eq!(symvers, Symvers::new());
}

#[test]
fn read_no_name() {
    // Check that records without a name are rejected.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "test.symvers",
        concat!(
            "0x12345678\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(result, "test.symvers:1: The export does not specify a name");
    assert_eq!(symvers, Symvers::new());
}

#[test]
fn read_no_module() {
    // Check that records without a module are rejected.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "test.symvers",
        concat!(
            "0x12345678 foo\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(
        result,
        "test.symvers:1: The export does not specify a module"
    );
    assert_eq!(symvers, Symvers::new());
}

#[test]
fn read_type() {
    // Check that the EXPORT_SYMBOL and EXPORT_SYMBOL_GPL types are correctly recognized.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "test.symvers",
        concat!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "0x90abcdef bar vmlinux EXPORT_SYMBOL_GPL\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_eq!(
        symvers,
        Symvers {
            exports: HashMap::from([
                (
                    "foo".to_string(),
                    ExportInfo::new(0x12345678, "vmlinux", false, None::<&str>)
                ),
                (
                    "bar".to_string(),
                    ExportInfo::new(0x90abcdef, "vmlinux", true, None::<&str>)
                ),
            ])
        }
    );
}

#[test]
fn read_no_type() {
    // Check that records without a type are rejected.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "test.symvers",
        concat!(
            "0x12345678 foo vmlinux\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(result, "test.symvers:1: The export does not specify a type");
    assert_eq!(symvers, Symvers::new());
}

#[test]
fn read_invalid_type() {
    // Check that an invalid type is rejected.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "test.symvers",
        concat!(
            "0x12345678 foo vmlinux EXPORT_UNUSED_SYMBOL\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(result, "test.symvers:1: Invalid export type 'EXPORT_UNUSED_SYMBOL', must be either EXPORT_SYMBOL or EXPORT_SYMBOL_GPL");
    assert_eq!(symvers, Symvers::new());
}

#[test]
fn read_namespace() {
    // Check that an optional namespace is correctly accepted.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "test.symvers",
        concat!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL_GPL FOO_NS\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_eq!(
        symvers,
        Symvers {
            exports: HashMap::from([(
                "foo".to_string(),
                ExportInfo::new(0x12345678, "vmlinux", true, Some("FOO_NS"))
            )])
        }
    );
}

#[test]
fn read_extra_data() {
    // Check that any extra data after the namespace is rejected.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "test.symvers",
        concat!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL_GPL FOO_NS garbage\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(
        result,
        "test.symvers:1: Unexpected string 'garbage' found at the end of the export record"
    );
    assert_eq!(symvers, Symvers::new());
}

#[test]
fn compare_identical() {
    // Check that the comparison of two identical symvers shows no differences.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        concat!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut symvers2 = Symvers::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        concat!(
            "0x12345678\tfoo\tvmlinux\tEXPORT_SYMBOL\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = symvers.compare_with(&symvers2, None, &mut out);
    assert_ok!(result);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "", //
        )
    );
}

#[test]
fn compare_added_export() {
    // Check that the comparison of two symvers reports any newly added export.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        concat!("0x12345678 foo vmlinux EXPORT_SYMBOL\n",).as_bytes(),
    );
    assert_ok!(result);
    let mut symvers2 = Symvers::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        concat!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "0x90abcdef bar vmlinux EXPORT_SYMBOL_GPL BAR_NS\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = symvers.compare_with(&symvers2, None, &mut out);
    assert_ok!(result);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "Export 'bar' has been added (tolerated)\n", //
        )
    );
}

#[test]
fn compare_removed_export() {
    // Check that the comparison of two symvers reports any removed export.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        concat!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "0x90abcdef bar vmlinux EXPORT_SYMBOL_GPL BAR_NS\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut symvers2 = Symvers::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        concat!(
            "0x90abcdef bar vmlinux EXPORT_SYMBOL_GPL BAR_NS\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = symvers.compare_with(&symvers2, None, &mut out);
    assert_ok!(result);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "Export 'foo' has been removed\n", //
        )
    );
}

#[test]
fn compare_changed_crc() {
    // Check that the comparison of two symvers reports exports with changed CRCs.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        concat!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut symvers2 = Symvers::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        concat!(
            "0x09abcdef foo vmlinux EXPORT_SYMBOL\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = symvers.compare_with(&symvers2, None, &mut out);
    assert_ok!(result);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "Export 'foo' changed CRC from '0x12345678' to '0x09abcdef'\n", //
        )
    );
}

#[test]
fn compare_changed_type() {
    // Check that the comparison of two symvers reports exports with changed types.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        concat!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "0x23456789 bar vmlinux EXPORT_SYMBOL\n",
            "0x34567890 baz vmlinux EXPORT_SYMBOL_GPL\n",
            "0x4567890a qux vmlinux EXPORT_SYMBOL_GPL\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut symvers2 = Symvers::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        concat!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "0x23456789 bar vmlinux EXPORT_SYMBOL_GPL\n",
            "0x34567890 baz vmlinux EXPORT_SYMBOL\n",
            "0x4567890a qux vmlinux EXPORT_SYMBOL_GPL\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = symvers.compare_with(&symvers2, None, &mut out);
    assert_ok!(result);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "Export 'bar' changed type from 'EXPORT_SYMBOL' to 'EXPORT_SYMBOL_GPL'\n",
            "Export 'baz' changed type from 'EXPORT_SYMBOL_GPL' to 'EXPORT_SYMBOL' (tolerated)\n", //
        )
    );
}

#[test]
fn compare_ignored_changes() {
    // Check that severity rules can be used to tolerate changes.
    let mut symvers = Symvers::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        concat!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut symvers2 = Symvers::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        concat!(
            "0x90abcdef foo vmlinux EXPORT_SYMBOL\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        concat!(
            "vmlinux PASS\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = symvers.compare_with(&symvers2, Some(&rules), &mut out);
    assert_ok!(result);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!("Export 'foo' changed CRC from '0x12345678' to '0x90abcdef' (tolerated)\n",)
    );
}
