// Copyright (C) 2025 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;
use crate::text::Writer;
use crate::{assert_inexact_parse_err, assert_ok, assert_ok_eq, assert_parse_err, bytes};

#[test]
fn read_export_basic() {
    // Check that basic parsing works correctly.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "test.symvers",
        bytes!(
            "0x12345678\tfoo\tvmlinux\tEXPORT_SYMBOL\n", //
        ),
    );
    assert_ok!(result);
    assert_eq!(
        symvers,
        SymversCorpus {
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
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "\n",
            "0x9abcdef0 bar vmlinux EXPORT_SYMBOL_GPL BAR_NS\n", //
        ),
    );
    assert_parse_err!(result, "test.symvers:2: The export does not specify a CRC");
    assert_eq!(symvers, SymversCorpus::new());
}

#[test]
fn read_duplicate_symbol_record() {
    // Check that symbol records with duplicate names are rejected when reading a symvers file.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "0x12345678 foo vmlinux EXPORT_SYMBOL_GPL\n", //
        ),
    );
    assert_parse_err!(result, "test.symvers:2: Duplicate record 'foo'");
}

#[test]
fn read_invalid_crc() {
    // Check that a CRC value not starting with 0x/0X is rejected.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "test.symvers",
        bytes!(
            "0 foo vmlinux EXPORT_SYMBOL\n", //
        ),
    );
    assert_parse_err!(
        result,
        "test.symvers:1: Failed to parse the CRC value '0': string does not start with 0x or 0X"
    );
    assert_eq!(symvers, SymversCorpus::new());
}

#[test]
fn read_invalid_crc2() {
    // Check that a CRC value containing non-hexadecimal digits is rejected.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "test.symvers",
        bytes!(
            "0xabcdefgh foo vmlinux EXPORT_SYMBOL\n", //
        ),
    );
    assert_inexact_parse_err!(
        result,
        "test.symvers:1: Failed to parse the CRC value '0xabcdefgh': *"
    );
    assert_eq!(symvers, SymversCorpus::new());
}

#[test]
fn read_no_name() {
    // Check that records without a name are rejected.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "test.symvers",
        bytes!(
            "0x12345678\n", //
        ),
    );
    assert_parse_err!(result, "test.symvers:1: The export does not specify a name");
    assert_eq!(symvers, SymversCorpus::new());
}

#[test]
fn read_no_module() {
    // Check that records without a module are rejected.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "test.symvers",
        bytes!(
            "0x12345678 foo\n", //
        ),
    );
    assert_parse_err!(
        result,
        "test.symvers:1: The export does not specify a module"
    );
    assert_eq!(symvers, SymversCorpus::new());
}

#[test]
fn read_type() {
    // Check that the EXPORT_SYMBOL and EXPORT_SYMBOL_GPL types are correctly recognized.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "0x9abcdef0 bar vmlinux EXPORT_SYMBOL_GPL\n", //
        ),
    );
    assert_ok!(result);
    assert_eq!(
        symvers,
        SymversCorpus {
            exports: HashMap::from([
                (
                    "foo".to_string(),
                    ExportInfo::new(0x12345678, "vmlinux", false, None::<&str>)
                ),
                (
                    "bar".to_string(),
                    ExportInfo::new(0x9abcdef0, "vmlinux", true, None::<&str>)
                ),
            ])
        }
    );
}

#[test]
fn read_no_type() {
    // Check that records without a type are rejected.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "test.symvers",
        bytes!(
            "0x12345678 foo vmlinux\n", //
        ),
    );
    assert_parse_err!(result, "test.symvers:1: The export does not specify a type");
    assert_eq!(symvers, SymversCorpus::new());
}

#[test]
fn read_invalid_type() {
    // Check that an invalid type is rejected.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_UNUSED_SYMBOL\n", //
        ),
    );
    assert_parse_err!(
        result,
        "test.symvers:1: Invalid export type 'EXPORT_UNUSED_SYMBOL', must be either EXPORT_SYMBOL or EXPORT_SYMBOL_GPL"
    );
    assert_eq!(symvers, SymversCorpus::new());
}

#[test]
fn read_namespace() {
    // Check that an optional namespace is correctly accepted.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL_GPL FOO_NS\n", //
        ),
    );
    assert_ok!(result);
    assert_eq!(
        symvers,
        SymversCorpus {
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
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL_GPL FOO_NS garbage\n", //
        ),
    );
    assert_parse_err!(
        result,
        "test.symvers:1: Unexpected string 'garbage' found at the end of the export record"
    );
    assert_eq!(symvers, SymversCorpus::new());
}

#[test]
fn compare_identical() {
    // Check that the comparison of two identical symvers shows no differences.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n", //
        ),
    );
    assert_ok!(result);
    let mut symvers2 = SymversCorpus::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        bytes!(
            "0x12345678\tfoo\tvmlinux\tEXPORT_SYMBOL\n", //
        ),
    );
    assert_ok!(result);
    let mut writer = Writer::new_buffer();
    let result =
        symvers.compare_with_buffer(&symvers2, None, &mut [(CompareFormat::Pretty, &mut writer)]);
    let out = writer.into_inner_vec();
    assert_ok_eq!(result, true);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "", //
        )
    );
}

#[test]
fn compare_added_export() {
    // Check that the comparison of two symvers reports any newly added export.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n", //
        ),
    );
    assert_ok!(result);
    let mut symvers2 = SymversCorpus::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "0x9abcdef0 bar vmlinux EXPORT_SYMBOL_GPL BAR_NS\n", //
        ),
    );
    assert_ok!(result);
    let mut writer = Writer::new_buffer();
    let result =
        symvers.compare_with_buffer(&symvers2, None, &mut [(CompareFormat::Pretty, &mut writer)]);
    let out = writer.into_inner_vec();
    assert_ok_eq!(result, true);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "Export 'bar' has been added (implicitly tolerated)\n", //
        )
    );
}

#[test]
fn compare_removed_export() {
    // Check that the comparison of two symvers reports any removed export.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "0x9abcdef0 bar vmlinux EXPORT_SYMBOL_GPL BAR_NS\n", //
        ),
    );
    assert_ok!(result);
    let mut symvers2 = SymversCorpus::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        bytes!(
            "0x9abcdef0 bar vmlinux EXPORT_SYMBOL_GPL BAR_NS\n", //
        ),
    );
    assert_ok!(result);
    let mut writer = Writer::new_buffer();
    let result =
        symvers.compare_with_buffer(&symvers2, None, &mut [(CompareFormat::Pretty, &mut writer)]);
    let out = writer.into_inner_vec();
    assert_ok_eq!(result, false);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "Export 'foo' has been removed\n", //
        )
    );
}

#[test]
fn compare_changed_crc() {
    // Check that the comparison of two symvers reports exports with changed CRCs.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n", //
        ),
    );
    assert_ok!(result);
    let mut symvers2 = SymversCorpus::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        bytes!(
            "0x9abcdef0 foo vmlinux EXPORT_SYMBOL\n", //
        ),
    );
    assert_ok!(result);
    let mut writer = Writer::new_buffer();
    let result =
        symvers.compare_with_buffer(&symvers2, None, &mut [(CompareFormat::Pretty, &mut writer)]);
    let out = writer.into_inner_vec();
    assert_ok_eq!(result, false);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "Export 'foo' changed CRC from '0x12345678' to '0x9abcdef0'\n", //
        )
    );
}

#[test]
fn compare_changed_type() {
    // Check that the comparison of two symvers reports exports with changed types.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "0x23456789 bar vmlinux EXPORT_SYMBOL\n",
            "0x3456789a baz vmlinux EXPORT_SYMBOL_GPL\n",
            "0x456789ab qux vmlinux EXPORT_SYMBOL_GPL\n", //
        ),
    );
    assert_ok!(result);
    let mut symvers2 = SymversCorpus::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "0x23456789 bar vmlinux EXPORT_SYMBOL_GPL\n",
            "0x3456789a baz vmlinux EXPORT_SYMBOL\n",
            "0x456789ab qux vmlinux EXPORT_SYMBOL_GPL\n", //
        ),
    );
    assert_ok!(result);
    let mut writer = Writer::new_buffer();
    let result =
        symvers.compare_with_buffer(&symvers2, None, &mut [(CompareFormat::Pretty, &mut writer)]);
    let out = writer.into_inner_vec();
    assert_ok_eq!(result, false);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "Export 'bar' changed type from 'EXPORT_SYMBOL' to 'EXPORT_SYMBOL_GPL'\n",
            "Export 'baz' changed type from 'EXPORT_SYMBOL_GPL' to 'EXPORT_SYMBOL' (implicitly tolerated)\n", //
        )
    );
}

#[test]
fn compare_ignored_changes() {
    // Check that severity rules can be used to tolerate changes.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n", //
        ),
    );
    assert_ok!(result);
    let mut symvers2 = SymversCorpus::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        bytes!(
            "0x9abcdef0 foo vmlinux EXPORT_SYMBOL\n", //
        ),
    );
    assert_ok!(result);
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "vmlinux PASS\n", //
        ),
    );
    assert_ok!(result);
    let mut writer = Writer::new_buffer();
    let result = symvers.compare_with_buffer(
        &symvers2,
        Some(&rules),
        &mut [(CompareFormat::Pretty, &mut writer)],
    );
    let out = writer.into_inner_vec();
    assert_ok_eq!(result, true);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "Export 'foo' changed CRC from '0x12345678' to '0x9abcdef0' (tolerated by rules)\n", //
        )
    );
}

#[test]
fn compare_format_null() {
    // Check that when using the null format, the comparison output is empty and only the return
    // code indicates any changes.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n", //
        ),
    );
    assert_ok!(result);
    let mut symvers2 = SymversCorpus::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        bytes!(
            "0x9abcdef0 foo vmlinux EXPORT_SYMBOL\n", //
        ),
    );
    assert_ok!(result);
    let mut writer = Writer::new_buffer();
    let result =
        symvers.compare_with_buffer(&symvers2, None, &mut [(CompareFormat::Null, &mut writer)]);
    let out = writer.into_inner_vec();
    assert_ok_eq!(result, false);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "", //
        )
    );
}

#[test]
fn compare_format_symbols() {
    // Check that when using the symbols format, the comparison output is in alphabetical order,
    // doesn't contain tolerated changes, and lists each symbol only once, even if it has multiple
    // changes.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "0x23456789 bar vmlinux EXPORT_SYMBOL\n",
            "0x3456789a baz vmlinux EXPORT_SYMBOL_GPL\n", //
        ),
    );
    assert_ok!(result);
    let mut symvers2 = SymversCorpus::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        bytes!(
            "0x9abcdef0 foo vmlinux EXPORT_SYMBOL_GPL\n",
            "0x23456789 bar vmlinux EXPORT_SYMBOL_GPL\n",
            "0x456789ab qux vmlinux EXPORT_SYMBOL_GPL\n", //
        ),
    );
    assert_ok!(result);
    let mut writer = Writer::new_buffer();
    let result = symvers.compare_with_buffer(
        &symvers2,
        None,
        &mut [(CompareFormat::Symbols, &mut writer)],
    );
    let out = writer.into_inner_vec();
    assert_ok_eq!(result, false);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "bar\n", "baz\n", "foo\n", //
        )
    );
}

#[test]
fn compare_format_mod_symbols() {
    // Check that when using the mod-symbols format, the comparison output lists only modified
    // symbols and exludes any additions and removals.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        bytes!(
            "0x12345678 foo vmlinux EXPORT_SYMBOL\n",
            "0x23456789 bar vmlinux EXPORT_SYMBOL\n",
            "0x3456789a baz vmlinux EXPORT_SYMBOL_GPL\n", //
        ),
    );
    assert_ok!(result);
    let mut symvers2 = SymversCorpus::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        bytes!(
            "0x9abcdef0 foo vmlinux EXPORT_SYMBOL_GPL\n",
            "0x23456789 bar vmlinux EXPORT_SYMBOL_GPL\n",
            "0x456789ab qux vmlinux EXPORT_SYMBOL_GPL\n", //
        ),
    );
    assert_ok!(result);
    let mut writer = Writer::new_buffer();
    let result = symvers.compare_with_buffer(
        &symvers2,
        None,
        &mut [(CompareFormat::ModSymbols, &mut writer)],
    );
    let out = writer.into_inner_vec();
    assert_ok_eq!(result, false);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "bar\n", "foo\n", //
        )
    );
}

#[test]
fn compare_format_short() {
    // Check that when using the short format, the comparison output details all breaking and
    // implicitly-tolerated changes, followed by a summary of changes tolerated by the rules.
    //
    // Cases:
    // aaa: Added -> separately reported as implicitly tolerated.
    // bbb: Removed -> reported separately.
    // ccc: Changes its CRC and type from regular to GPL -> reported twice separately.
    // ddd: Changes its type from regular to GPL -> reported separately.
    // eee: Changes its type from GPL to regular -> reported separately as implicitly tolerated.
    //
    // .. and the same tests but with the symbols matching the rules:
    // fff: Added -> included in the tolerated-by-rules summary.
    // ggg: Removed -> included in the tolerated-by-rules summary.
    // hhh: Changes its CRC and type from regular to GPL -> included once in the tolerated-by-rules
    //      summary.
    // iii: Changes its type from regular to GPL -> included in the tolerated-by-rules summary.
    // jjj: Changes its type from GPL to regular -> included in the tolerated-by-rules summary.
    let mut symvers = SymversCorpus::new();
    let result = symvers.load_buffer(
        "a/test.symvers",
        bytes!(
            "0x12345678 bbb vmlinux EXPORT_SYMBOL\n",
            "0x12345678 ccc vmlinux EXPORT_SYMBOL\n",
            "0x23456789 ddd vmlinux EXPORT_SYMBOL\n",
            "0x23456789 eee vmlinux EXPORT_SYMBOL_GPL\n",
            "0x12345678 ggg vmlinux EXPORT_SYMBOL\n",
            "0x12345678 hhh vmlinux EXPORT_SYMBOL\n",
            "0x23456789 iii vmlinux EXPORT_SYMBOL\n",
            "0x23456789 jjj vmlinux EXPORT_SYMBOL_GPL\n", //
        ),
    );
    assert_ok!(result);
    let mut symvers2 = SymversCorpus::new();
    let result = symvers2.load_buffer(
        "b/test.symvers",
        bytes!(
            "0x12345678 aaa vmlinux EXPORT_SYMBOL\n",
            "0x9abcdef0 ccc vmlinux EXPORT_SYMBOL_GPL\n",
            "0x23456789 ddd vmlinux EXPORT_SYMBOL_GPL\n",
            "0x23456789 eee vmlinux EXPORT_SYMBOL\n",
            "0x12345678 fff vmlinux EXPORT_SYMBOL\n",
            "0x9abcdef0 hhh vmlinux EXPORT_SYMBOL_GPL\n",
            "0x23456789 iii vmlinux EXPORT_SYMBOL_GPL\n",
            "0x23456789 jjj vmlinux EXPORT_SYMBOL\n", //
        ),
    );
    assert_ok!(result);
    let mut rules = Rules::new();
    let result = rules.load_buffer(
        "test.severities",
        bytes!(
            "fff PASS\n",
            "ggg PASS\n",
            "hhh PASS\n",
            "iii PASS\n",
            "jjj PASS\n", //
        ),
    );
    assert_ok!(result);
    let mut writer = Writer::new_buffer();
    let result = symvers.compare_with_buffer(
        &symvers2,
        Some(&rules),
        &mut [(CompareFormat::Short, &mut writer)],
    );
    let out = writer.into_inner_vec();
    assert_ok_eq!(result, false);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "Export 'aaa' has been added (implicitly tolerated)\n",
            "Export 'bbb' has been removed\n",
            "Export 'ccc' changed CRC from '0x12345678' to '0x9abcdef0'\n",
            "Export 'ccc' changed type from 'EXPORT_SYMBOL' to 'EXPORT_SYMBOL_GPL'\n",
            "Export 'ddd' changed type from 'EXPORT_SYMBOL' to 'EXPORT_SYMBOL_GPL'\n",
            "Export 'eee' changed type from 'EXPORT_SYMBOL_GPL' to 'EXPORT_SYMBOL' (implicitly tolerated)\n",
            "Changes tolerated by rules: '1' additions, '1' removals, '3' modifications\n", //
        )
    );
}
