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
            "0x4dfa8d4b\tmutex_lock\tvmlinux\tEXPORT_SYMBOL\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_eq!(
        symvers,
        Symvers {
            exports: HashMap::from([(
                "mutex_lock".to_string(),
                ExportInfo::new(0x4dfa8d4b, "vmlinux", false, None::<&str>)
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
            "0x4dfa8d4b mutex_lock vmlinux EXPORT_SYMBOL\n",
            "\n",
            "0x2303b915 efivar_lock vmlinux EXPORT_SYMBOL_GPL EFIVAR\n", //
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
            "0 mutex_lock vmlinux EXPORT_SYMBOL\n", //
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
            "0xabcdefgh mutex_lock vmlinux EXPORT_SYMBOL\n", //
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
            "0x4dfa8d4b\n", //
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
            "0x4dfa8d4b mutex_lock\n", //
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
            "0x4dfa8d4b mutex_lock vmlinux EXPORT_SYMBOL\n",
            "0xa04f945a cpus_read_lock vmlinux EXPORT_SYMBOL_GPL\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_eq!(
        symvers,
        Symvers {
            exports: HashMap::from([
                (
                    "mutex_lock".to_string(),
                    ExportInfo::new(0x4dfa8d4b, "vmlinux", false, None::<&str>)
                ),
                (
                    "cpus_read_lock".to_string(),
                    ExportInfo::new(0xa04f945a, "vmlinux", true, None::<&str>)
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
            "0x4dfa8d4b mutex_lock vmlinux\n", //
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
            "0x4dfa8d4b mutex_lock vmlinux EXPORT_UNUSED_SYMBOL\n", //
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
            "0x2303b915 efivar_lock vmlinux EXPORT_SYMBOL_GPL EFIVAR\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_eq!(
        symvers,
        Symvers {
            exports: HashMap::from([(
                "efivar_lock".to_string(),
                ExportInfo::new(0x2303b915, "vmlinux", true, Some("EFIVAR"))
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
            "0x2303b915 efivar_lock vmlinux EXPORT_SYMBOL_GPL EFIVAR garbage\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(
        result,
        "test.symvers:1: Unexpected string 'garbage' found at the end of the export record"
    );
    assert_eq!(symvers, Symvers::new());
}
