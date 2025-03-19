// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;
use crate::{assert_ok, assert_parse_err};

#[test]
fn read_empty_record() {
    // Check that empty records are rejected when reading a file.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "test.symtypes",
        concat!(
            "s#test struct test { }\n",
            "\n",
            "s#test2 struct test2 { }\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(result, "test.symtypes:2: Expected a record name");
}

#[test]
fn read_duplicate_type_record() {
    // Check that type records with duplicate names are rejected when reading a file.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "test.symtypes",
        concat!(
            "s#test struct test { int a ; }\n",
            "s#test struct test { int b ; }\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(result, "test.symtypes:2: Duplicate record 's#test'");
}

#[test]
fn read_duplicate_file_record() {
    // Check that F# records with duplicate names are rejected when reading a consolidated file.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "test.symtypes",
        concat!(
            "bar int bar ( )\n",
            "baz int baz ( )\n",
            "F#test.symtypes bar\n",
            "F#test.symtypes baz\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(
        result,
        "test.symtypes:4: Duplicate record 'F#test.symtypes'"
    );
}

#[test]
fn read_invalid_file_record_ref() {
    // Check that an F# record referencing a type in form '<base_name>' is rejected if the type is
    // not known.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "test.symtypes",
        concat!(
            "F#test.symtypes bar\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(result, "test.symtypes:1: Type 'bar' is not known");
}

#[test]
fn read_invalid_file_record_ref2() {
    // Check that an F# record referencing a type in form '<base_name>@<variant_idx>' is rejected if
    // the base name is not known.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "test.symtypes",
        concat!(
            "F#test.symtypes bar@0\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(result, "test.symtypes:1: Type 'bar@0' is not known");
}

#[test]
fn read_invalid_file_record_ref3() {
    // Check that an F# record referencing a type in form '<base_name>@<variant_idx>' is rejected if
    // the variant index is not known.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "test.symtypes",
        concat!(
            "bar@0 int bar ( )\n",
            "F#test.symtypes bar@0\n",
            "F#test2.symtypes bar@1\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(result, "test.symtypes:3: Type 'bar@1' is not known");
}

#[test]
fn read_duplicate_type_export() {
    // Check that two exports with the same name in different files get rejected.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "test.symtypes",
        concat!(
            "foo int foo ( )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let result = syms.load_buffer(
        "test2.symtypes",
        concat!(
            "foo int foo ( )", //
        )
        .as_bytes(),
    );
    assert_parse_err!(
        result,
        "test2.symtypes:1: Export 'foo' is duplicate, previous occurrence found in 'test.symtypes'"
    );
}

#[test]
fn read_write_basic() {
    // Check reading of a single file and writing the consolidated output.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "test.symtypes",
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = syms.write_consolidated_buffer(&mut out);
    assert_ok!(result);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n",
            "F#test.symtypes bar\n", //
        )
    );
}

#[test]
fn read_write_shared_struct() {
    // Check that a structure declaration shared by two files appears only once in the consolidated
    // output.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "test.symtypes",
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let result = syms.load_buffer(
        "test2.symtypes",
        concat!(
            "s#foo struct foo { int a ; }\n",
            "baz int baz ( s#foo )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = syms.write_consolidated_buffer(&mut out);
    assert_ok!(result);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n",
            "baz int baz ( s#foo )\n",
            "F#test.symtypes bar\n",
            "F#test2.symtypes baz\n", //
        )
    );
}

#[test]
fn read_write_differing_struct() {
    // Check that a structure declaration different in two files appears in all variants in the
    // consolidated output and they are correctly referenced by the F# entries.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "test.symtypes",
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let result = syms.load_buffer(
        "test2.symtypes",
        concat!(
            "s#foo struct foo { UNKNOWN }\n",
            "baz int baz ( s#foo )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = syms.write_consolidated_buffer(&mut out);
    assert_ok!(result);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "s#foo@0 struct foo { int a ; }\n",
            "s#foo@1 struct foo { UNKNOWN }\n",
            "bar int bar ( s#foo )\n",
            "baz int baz ( s#foo )\n",
            "F#test.symtypes s#foo@0 bar\n",
            "F#test2.symtypes s#foo@1 baz\n", //
        )
    );
}

#[test]
fn compare_identical() {
    // Check that the comparison of two identical corpuses shows no differences.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "a/test.symtypes",
        concat!(
            "bar int bar ( )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut syms2 = SymCorpus::new();
    let result = syms2.load_buffer(
        "b/test.symtypes",
        concat!(
            "bar int bar ( )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = syms.compare_with(&syms2, None, &mut out, 1);
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
    // Check that the comparison of two corpuses reports any newly added export.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "a/test.symtypes",
        concat!(
            "bar int bar ( )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut syms2 = SymCorpus::new();
    let result = syms2.load_buffer(
        "b/test.symtypes",
        concat!(
            "bar int bar ( )\n",
            "baz int baz ( )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = syms.compare_with(&syms2, None, &mut out, 1);
    assert_ok!(result);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "Export 'baz' has been added\n", //
        )
    );
}

#[test]
fn compare_removed_export() {
    // Check that the comparison of two corpuses reports any removed export.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "a/test.symtypes",
        concat!(
            "bar int bar ( )\n",
            "baz int baz ( )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut syms2 = SymCorpus::new();
    let result = syms2.load_buffer(
        "b/test.symtypes",
        concat!(
            "baz int baz ( )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = syms.compare_with(&syms2, None, &mut out, 1);
    assert_ok!(result);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "Export 'bar' has been removed\n", //
        )
    );
}

#[test]
fn compare_changed_type() {
    // Check that the comparison of two corpuses reports changed types and affected exports.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "a/test.symtypes",
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut syms2 = SymCorpus::new();
    let result = syms2.load_buffer(
        "b/test.symtypes",
        concat!(
            "s#foo struct foo { int a ; int b ; }\n",
            "bar int bar ( s#foo )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = syms.compare_with(&syms2, None, &mut out, 1);
    assert_ok!(result);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "The following '1' exports are different:\n",
            " bar\n",
            "\n",
            "because of a changed 's#foo':\n",
            "@@ -1,3 +1,4 @@\n",
            " struct foo {\n",
            " \tint a;\n",
            "+\tint b;\n",
            " }\n", //
        )
    );
}

#[test]
fn compare_changed_nested_type() {
    // Check that the comparison of two corpuses reports also changes in subtypes even if the parent
    // type itself is modified, as long as each subtype is referenced by the parent type in both
    // inputs.
    let mut syms = SymCorpus::new();
    let result = syms.load_buffer(
        "a/test.symtypes",
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( int a , s#foo )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut syms2 = SymCorpus::new();
    let result = syms2.load_buffer(
        "b/test.symtypes",
        concat!(
            "s#foo struct foo { int a ; int b ; }\n",
            "bar int bar ( s#foo , int a )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = syms.compare_with(&syms2, None, &mut out, 1);
    assert_ok!(result);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "The following '1' exports are different:\n",
            " bar\n",
            "\n",
            "because of a changed 'bar':\n",
            "@@ -1,4 +1,4 @@\n",
            " int bar (\n",
            "-\tint a,\n",
            "-\ts#foo\n",
            "+\ts#foo,\n",
            "+\tint a\n",
            " )\n",
            "\n",
            "The following '1' exports are different:\n",
            " bar\n",
            "\n",
            "because of a changed 's#foo':\n",
            "@@ -1,3 +1,4 @@\n",
            " struct foo {\n",
            " \tint a;\n",
            "+\tint b;\n",
            " }\n", //
        )
    );
}
