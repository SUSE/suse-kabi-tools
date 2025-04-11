// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;
use crate::{assert_ok, assert_parse_err};

#[test]
fn read_basic_single() {
    // Check basic reading of a single file.
    let mut syms = SymtypesCorpus::new();
    let result = syms.load_buffer(
        "test.symtypes",
        concat!(
            "s#foo struct foo { }\n",
            "bar void bar ( s#foo )\n",
            "baz int baz ( )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_eq!(
        syms,
        SymtypesCorpus {
            types: HashMap::from([
                (
                    "s#foo".to_string(),
                    vec![vec![
                        Token::new_atom("struct"),
                        Token::new_atom("foo"),
                        Token::new_atom("{"),
                        Token::new_atom("}"),
                    ]]
                ),
                (
                    "bar".to_string(),
                    vec![vec![
                        Token::new_atom("void"),
                        Token::new_atom("bar"),
                        Token::new_atom("("),
                        Token::new_typeref("s#foo"),
                        Token::new_atom(")"),
                    ]]
                ),
                (
                    "baz".to_string(),
                    vec![vec![
                        Token::new_atom("int"),
                        Token::new_atom("baz"),
                        Token::new_atom("("),
                        Token::new_atom(")"),
                    ]]
                ),
            ]),
            exports: HashMap::from([("bar".to_string(), 0), ("baz".to_string(), 0)]),
            files: vec![SymtypesFile {
                path: PathBuf::from("test.symtypes"),
                records: HashMap::from([
                    ("s#foo".to_string(), 0),
                    ("bar".to_string(), 0),
                    ("baz".to_string(), 0),
                ])
            }],
        }
    );
}

#[test]
fn read_basic_consolidated() {
    // Check basic reading of a consolidated file.
    let mut syms = SymtypesCorpus::new();
    let result = syms.load_buffer(
        "test_consolidated.symtypes",
        concat!(
            "/* test.symtypes */\n",
            "s#foo struct foo { }\n",
            "bar void bar ( s#foo )\n",
            "/* test2.symtypes */\n",
            "baz int baz ( )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_eq!(
        syms,
        SymtypesCorpus {
            types: HashMap::from([
                (
                    "s#foo".to_string(),
                    vec![vec![
                        Token::new_atom("struct"),
                        Token::new_atom("foo"),
                        Token::new_atom("{"),
                        Token::new_atom("}"),
                    ]]
                ),
                (
                    "bar".to_string(),
                    vec![vec![
                        Token::new_atom("void"),
                        Token::new_atom("bar"),
                        Token::new_atom("("),
                        Token::new_typeref("s#foo"),
                        Token::new_atom(")"),
                    ]]
                ),
                (
                    "baz".to_string(),
                    vec![vec![
                        Token::new_atom("int"),
                        Token::new_atom("baz"),
                        Token::new_atom("("),
                        Token::new_atom(")"),
                    ]]
                ),
            ]),
            exports: HashMap::from([("bar".to_string(), 0), ("baz".to_string(), 1)]),
            files: vec![
                SymtypesFile {
                    path: PathBuf::from("test.symtypes"),
                    records: HashMap::from([("s#foo".to_string(), 0), ("bar".to_string(), 0)])
                },
                SymtypesFile {
                    path: PathBuf::from("test2.symtypes"),
                    records: HashMap::from([("baz".to_string(), 0)])
                },
            ],
        }
    );
}

#[test]
fn read_empty_record_single() {
    // Check that empty records are rejected when reading a single file.
    let mut syms = SymtypesCorpus::new();
    let result = syms.load_buffer(
        "test.symtypes",
        concat!(
            "s#foo struct foo { }\n",
            "\n",
            "bar void bar ( s#foo )\n",
            "baz int baz ( )\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(result, "test.symtypes:2: Expected a record name");
}

#[test]
fn read_empty_record_consolidated() {
    // Check that empty records are skipped when reading a consolidated file.
    let mut syms = SymtypesCorpus::new();
    let result = syms.load_buffer(
        "test_consolidated.symtypes",
        concat!(
            "/* test.symtypes */\n",
            "\n",
            "s#foo struct foo { }\n",
            "\n",
            "bar void bar ( s#foo )\n",
            "\n",
            "/* test2.symtypes */\n",
            "\n",
            "baz int baz ( )\n",
            "\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    assert_ne!(syms, SymtypesCorpus::new());
}

#[test]
fn read_duplicate_type_record() {
    // Check that type records with duplicate names are rejected when reading a file.
    let mut syms = SymtypesCorpus::new();
    let result = syms.load_buffer(
        "test.symtypes",
        concat!(
            "s#foo struct foo { int a ; }\n",
            "s#foo struct foo { int b ; }\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(result, "test.symtypes:2: Duplicate record 's#foo'");
}

/*
TODO FIXME
#[test]
fn read_duplicate_file_record() {
    // Check that file records with duplicate names are rejected when reading a consolidated file.
    let mut syms = SymtypesCorpus::new();
    let result = syms.load_buffer(
        "test_consolidated.symtypes",
        concat!(
            "/* test.symtypes */
\n",
" /* test.symtypes */
\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(
        result,
        "test.symtypes:2: Duplicate record 'F#test.symtypes'"
    );
}
*/

#[test]
fn read_invalid_reference() {
    // Check that a record referencing a symbol with a missing declaration is rejected.
    let mut syms = SymtypesCorpus::new();
    let result = syms.load_buffer(
        "test.symtypes",
        concat!(
            "bar void bar ( s#foo )\n", //
        )
        .as_bytes(),
    );
    assert_parse_err!(result, "test.symtypes:1: Type 's#foo' is not known");
}

#[test]
fn read_duplicate_type_export() {
    // Check that two exports with the same name in different files get rejected.
    let mut syms = SymtypesCorpus::new();
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
    let mut syms = SymtypesCorpus::new();
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
            "/* test.symtypes */\n",
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n", //
        )
    );
}

#[test]
fn read_write_shared_struct() {
    // Check that a structure declaration shared by two files appears only once in the consolidated
    // output.
    let mut syms = SymtypesCorpus::new();
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
            "/* test.symtypes */\n",
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n",
            "\n",
            "/* test2.symtypes */\n",
            "baz int baz ( s#foo )\n", //
        )
    );
}

#[test]
fn read_write_differing_struct() {
    // Check that a structure declaration different in two files appears in all variants in the
    // consolidated output and they are correctly referenced by the file entries.
    let mut syms = SymtypesCorpus::new();
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
            "s#foo struct foo { long a ; }\n",
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
            "/* test.symtypes */\n",
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n",
            "\n",
            "/* test2.symtypes */\n",
            "s#foo struct foo { long a ; }\n",
            "baz int baz ( s#foo )\n", //
        )
    );
}

#[test]
fn compare_identical() {
    // Check that the comparison of two identical corpuses shows no differences.
    let mut syms = SymtypesCorpus::new();
    let result = syms.load_buffer(
        "a/test.symtypes",
        concat!(
            "bar int bar ( )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut syms2 = SymtypesCorpus::new();
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
    let mut syms = SymtypesCorpus::new();
    let result = syms.load_buffer(
        "a/test.symtypes",
        concat!(
            "bar int bar ( )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut syms2 = SymtypesCorpus::new();
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
    let mut syms = SymtypesCorpus::new();
    let result = syms.load_buffer(
        "a/test.symtypes",
        concat!(
            "bar int bar ( )\n",
            "baz int baz ( )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut syms2 = SymtypesCorpus::new();
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
    let mut syms = SymtypesCorpus::new();
    let result = syms.load_buffer(
        "a/test.symtypes",
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut syms2 = SymtypesCorpus::new();
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
    let mut syms = SymtypesCorpus::new();
    let result = syms.load_buffer(
        "a/test.symtypes",
        concat!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( int a , s#foo )\n", //
        )
        .as_bytes(),
    );
    assert_ok!(result);
    let mut syms2 = SymtypesCorpus::new();
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
