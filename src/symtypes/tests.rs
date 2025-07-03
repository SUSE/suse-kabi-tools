// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;
use crate::{assert_ok, assert_parse_err, bytes};

#[test]
fn read_basic_single() {
    // Check basic reading of a single file.
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "test.symtypes",
        bytes!(
            "s#foo struct foo { }\n",
            "bar void bar ( s#foo )\n",
            "baz int baz ( )\n", //
        ),
    );
    assert_ok!(result);
    let mut exp_symtypes = SymtypesCorpus {
        types: array::from_fn(|_| Types::new()),
        exports: HashMap::from([("bar".to_string(), 0), ("baz".to_string(), 0)]),
        files: vec![SymtypesFile {
            path: PathBuf::from("test.symtypes"),
            records: HashMap::from([
                ("s#foo".to_string(), 0),
                ("bar".to_string(), 0),
                ("baz".to_string(), 0),
            ]),
        }],
    };
    exp_symtypes.types[type_bucket_idx("s#foo")].insert(
        "s#foo".to_string(),
        vec![vec![
            Token::new_atom("struct"),
            Token::new_atom("foo"),
            Token::new_atom("{"),
            Token::new_atom("}"),
        ]],
    );
    exp_symtypes.types[type_bucket_idx("bar")].insert(
        "bar".to_string(),
        vec![vec![
            Token::new_atom("void"),
            Token::new_atom("bar"),
            Token::new_atom("("),
            Token::new_typeref("s#foo"),
            Token::new_atom(")"),
        ]],
    );
    exp_symtypes.types[type_bucket_idx("baz")].insert(
        "baz".to_string(),
        vec![vec![
            Token::new_atom("int"),
            Token::new_atom("baz"),
            Token::new_atom("("),
            Token::new_atom(")"),
        ]],
    );
    assert_eq!(symtypes, exp_symtypes);
}

#[test]
fn read_basic_consolidated() {
    // Check basic reading of a consolidated file.
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "test_consolidated.symtypes",
        bytes!(
            "/* test.symtypes */\n",
            "s#foo struct foo { }\n",
            "bar void bar ( s#foo )\n",
            "/* test2.symtypes */\n",
            "baz int baz ( )\n", //
        ),
    );
    assert_ok!(result);
    let mut exp_symtypes = SymtypesCorpus {
        types: array::from_fn(|_| Types::new()),
        exports: HashMap::from([("bar".to_string(), 0), ("baz".to_string(), 1)]),
        files: vec![
            SymtypesFile {
                path: PathBuf::from("test.symtypes"),
                records: HashMap::from([("s#foo".to_string(), 0), ("bar".to_string(), 0)]),
            },
            SymtypesFile {
                path: PathBuf::from("test2.symtypes"),
                records: HashMap::from([("baz".to_string(), 0)]),
            },
        ],
    };
    exp_symtypes.types[type_bucket_idx("s#foo")].insert(
        "s#foo".to_string(),
        vec![vec![
            Token::new_atom("struct"),
            Token::new_atom("foo"),
            Token::new_atom("{"),
            Token::new_atom("}"),
        ]],
    );
    exp_symtypes.types[type_bucket_idx("bar")].insert(
        "bar".to_string(),
        vec![vec![
            Token::new_atom("void"),
            Token::new_atom("bar"),
            Token::new_atom("("),
            Token::new_typeref("s#foo"),
            Token::new_atom(")"),
        ]],
    );
    exp_symtypes.types[type_bucket_idx("baz")].insert(
        "baz".to_string(),
        vec![vec![
            Token::new_atom("int"),
            Token::new_atom("baz"),
            Token::new_atom("("),
            Token::new_atom(")"),
        ]],
    );
    assert_eq!(symtypes, exp_symtypes);
}

#[test]
fn read_empty_record_single() {
    // Check that empty records are rejected when reading a single file.
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "test.symtypes",
        bytes!(
            "s#foo struct foo { }\n",
            "\n",
            "bar void bar ( s#foo )\n",
            "baz int baz ( )\n", //
        ),
    );
    assert_parse_err!(result, "test.symtypes:2: Expected a record name");
}

#[test]
fn read_empty_record_consolidated() {
    // Check that empty records are skipped when reading a consolidated file.
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "test_consolidated.symtypes",
        bytes!(
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
        ),
    );
    assert_ok!(result);
    assert_ne!(symtypes, SymtypesCorpus::new());
}

#[test]
fn read_duplicate_type_record() {
    // Check that type records with duplicate names are rejected when reading a symtypes file.
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "test.symtypes",
        bytes!(
            "s#foo struct foo { int a ; }\n",
            "s#foo struct foo { int b ; }\n", //
        ),
    );
    assert_parse_err!(result, "test.symtypes:2: Duplicate record 's#foo'");
}

/*
TODO FIXME
#[test]
fn read_duplicate_file_record() {
    // Check that file records with duplicate names are rejected when reading a consolidated file.
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "test_consolidated.symtypes",
        bytes!(
            "/* test.symtypes */
\n",
" /* test.symtypes */
\n", //
        ),
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
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "test.symtypes",
        bytes!(
            "bar void bar ( s#foo )\n", //
        ),
    );
    assert_parse_err!(result, "test.symtypes:1: Type 's#foo' is not known");
}

#[test]
fn read_duplicate_type_export() {
    // Check that two exports with the same name in different files get rejected.
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "test.symtypes",
        bytes!(
            "foo int foo ( )\n", //
        ),
    );
    assert_ok!(result);
    let result = symtypes.load_buffer(
        "test2.symtypes",
        bytes!(
            "foo int foo ( )", //
        ),
    );
    assert_parse_err!(
        result,
        "test2.symtypes:1: Export 'foo' is duplicate, previous occurrence found in 'test.symtypes'"
    );
}

#[test]
fn read_write_basic() {
    // Check reading of a single file and writing the consolidated output.
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "test.symtypes",
        bytes!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n", //
        ),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = symtypes.write_consolidated_buffer(&mut out, 1);
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
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "test.symtypes",
        bytes!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n", //
        ),
    );
    assert_ok!(result);
    let result = symtypes.load_buffer(
        "test2.symtypes",
        bytes!(
            "s#foo struct foo { int a ; }\n",
            "baz int baz ( s#foo )\n", //
        ),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = symtypes.write_consolidated_buffer(&mut out, 1);
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
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "test.symtypes",
        bytes!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n", //
        ),
    );
    assert_ok!(result);
    let result = symtypes.load_buffer(
        "test2.symtypes",
        bytes!(
            "s#foo struct foo { long a ; }\n",
            "baz int baz ( s#foo )\n", //
        ),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = symtypes.write_consolidated_buffer(&mut out, 1);
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
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "a/test.symtypes",
        bytes!(
            "bar int bar ( )\n", //
        ),
    );
    assert_ok!(result);
    let mut symtypes2 = SymtypesCorpus::new();
    let result = symtypes2.load_buffer(
        "b/test.symtypes",
        bytes!(
            "bar int bar ( )\n", //
        ),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = symtypes.compare_with(&symtypes2, None, &mut out, 1);
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
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "a/test.symtypes",
        bytes!(
            "bar int bar ( )\n", //
        ),
    );
    assert_ok!(result);
    let mut symtypes2 = SymtypesCorpus::new();
    let result = symtypes2.load_buffer(
        "b/test.symtypes",
        bytes!(
            "bar int bar ( )\n",
            "baz int baz ( )\n", //
        ),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = symtypes.compare_with(&symtypes2, None, &mut out, 1);
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
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "a/test.symtypes",
        bytes!(
            "bar int bar ( )\n",
            "baz int baz ( )\n", //
        ),
    );
    assert_ok!(result);
    let mut symtypes2 = SymtypesCorpus::new();
    let result = symtypes2.load_buffer(
        "b/test.symtypes",
        bytes!(
            "baz int baz ( )\n", //
        ),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = symtypes.compare_with(&symtypes2, None, &mut out, 1);
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
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "a/test.symtypes",
        bytes!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( s#foo )\n", //
        ),
    );
    assert_ok!(result);
    let mut symtypes2 = SymtypesCorpus::new();
    let result = symtypes2.load_buffer(
        "b/test.symtypes",
        bytes!(
            "s#foo struct foo { int a ; int b ; }\n",
            "bar int bar ( s#foo )\n", //
        ),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = symtypes.compare_with(&symtypes2, None, &mut out, 1);
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
    let mut symtypes = SymtypesCorpus::new();
    let result = symtypes.load_buffer(
        "a/test.symtypes",
        bytes!(
            "s#foo struct foo { int a ; }\n",
            "bar int bar ( int a , s#foo )\n", //
        ),
    );
    assert_ok!(result);
    let mut symtypes2 = SymtypesCorpus::new();
    let result = symtypes2.load_buffer(
        "b/test.symtypes",
        bytes!(
            "s#foo struct foo { int a ; int b ; }\n",
            "bar int bar ( s#foo , int a )\n", //
        ),
    );
    assert_ok!(result);
    let mut out = Vec::new();
    let result = symtypes.compare_with(&symtypes2, None, &mut out, 1);
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
