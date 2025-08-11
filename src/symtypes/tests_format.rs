// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;
use crate::assert_ok;

#[test]
fn format_typedef() {
    // Check the pretty format of a typedef declaration.
    let pretty = pretty_format_type(
        "t#u64",
        &vec![
            Token::new_atom("typedef"),
            Token::new_atom("unsigned"),
            Token::new_atom("long"),
            Token::new_atom("long"),
            Token::new_atom("u64"),
        ],
    );
    assert_eq!(
        pretty,
        crate::string_vec!(
            "typedef unsigned long long u64", //
        )
    );
}

#[test]
fn format_enum() {
    // Check the pretty format of an enum declaration.
    let pretty = pretty_format_type(
        "e#test",
        &vec![
            Token::new_atom("enum"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("VALUE1"),
            Token::new_atom(","),
            Token::new_atom("VALUE2"),
            Token::new_atom(","),
            Token::new_atom("VALUE3"),
            Token::new_atom("}"),
        ],
    );
    assert_eq!(
        pretty,
        crate::string_vec!(
            "enum test {",
            "\tVALUE1,",
            "\tVALUE2,",
            "\tVALUE3",
            "}", //
        )
    );
}

#[test]
fn format_struct() {
    // Check the pretty format of a struct declaration.
    let pretty = pretty_format_type(
        "s#test",
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue"),
            Token::new_atom(";"),
            Token::new_atom("long"),
            Token::new_atom("lvalue"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
    );
    assert_eq!(
        pretty,
        crate::string_vec!(
            "struct test {",
            "\tint ivalue;",
            "\tlong lvalue;",
            "}", //
        )
    );
}

#[test]
fn format_union() {
    // Check the pretty format of a union declaration.
    let pretty = pretty_format_type(
        "u#test",
        &vec![
            Token::new_atom("union"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue"),
            Token::new_atom(";"),
            Token::new_atom("long"),
            Token::new_atom("lvalue"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
    );
    assert_eq!(
        pretty,
        crate::string_vec!(
            "union test {",
            "\tint ivalue;",
            "\tlong lvalue;",
            "}", //
        )
    );
}

#[test]
fn format_function() {
    // Check the pretty format of a function declaration.
    let pretty = pretty_format_type(
        "test",
        &vec![
            Token::new_atom("void"),
            Token::new_atom("test"),
            Token::new_atom("("),
            Token::new_atom("int"),
            Token::new_atom("ivalue"),
            Token::new_atom(","),
            Token::new_atom("long"),
            Token::new_atom("lvalue"),
            Token::new_atom(")"),
        ],
    );
    assert_eq!(
        pretty,
        crate::string_vec!(
            "void test ( int ivalue, long lvalue )", //
        )
    );
}

#[test]
fn format_enum_constant() {
    // Check the pretty format of an enum constant declaration.
    let pretty = pretty_format_type("E#TEST", &vec![Token::new_atom("7")]);
    assert_eq!(
        pretty,
        crate::string_vec!(
            "7", //
        )
    );
}

#[test]
fn format_nested() {
    // Check the pretty format of a nested declaration.
    let pretty = pretty_format_type(
        "u#nested",
        &vec![
            Token::new_atom("union"),
            Token::new_atom("nested"),
            Token::new_atom("{"),
            Token::new_atom("struct"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("}"),
            Token::new_atom(";"),
            Token::new_atom("long"),
            Token::new_atom("lvalue"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
    );
    assert_eq!(
        pretty,
        crate::string_vec!(
            "union nested {",
            "\tstruct {",
            "\t\tint ivalue1;",
            "\t\tint ivalue2;",
            "\t};",
            "\tlong lvalue;",
            "}", //
        )
    );
}

#[test]
fn format_imbalanced() {
    // Check the pretty format of a declaration with wrongly balanced brackets.
    let pretty = pretty_format_type(
        "s#imbalanced",
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("imbalanced"),
            Token::new_atom("{"),
            Token::new_atom("{"),
            Token::new_atom("}"),
            Token::new_atom("}"),
            Token::new_atom("}"),
            Token::new_atom(";"),
            Token::new_atom("{"),
            Token::new_atom("{"),
        ],
    );
    assert_eq!(
        pretty,
        crate::string_vec!(
            "struct imbalanced {",
            "\t{",
            "\t}",
            "}",
            "};",
            "{",
            "\t{", //
        )
    );
}

#[test]
fn format_typeref() {
    // Check the pretty format of a declaration with a reference to another type.
    let pretty = pretty_format_type(
        "s#typeref",
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("typeref"),
            Token::new_atom("{"),
            Token::new_typeref("s#other"),
            Token::new_atom("other"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
    );
    assert_eq!(
        pretty,
        crate::string_vec!(
            "struct typeref {",
            "\ts#other other;",
            "}", //
        )
    );
}

#[test]
fn format_removal() {
    // Check the diff format when a struct member is removed.
    let mut out = Vec::new();
    let result = write_type_diff(
        "s#test",
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
        &mut out,
    );
    assert_ok!(result);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "@@ -1,4 +1,3 @@\n",
            " struct test {\n",
            " \tint ivalue1;\n",
            "-\tint ivalue2;\n",
            " }\n", //
        )
    );
}

#[test]
fn format_removal_top() {
    // Check the diff format when data is removed at the top.
    let mut out = Vec::new();
    let result = write_type_diff(
        "dummy",
        &vec![
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue3"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue4"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue5"),
            Token::new_atom(";"),
        ],
        &vec![
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue3"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue4"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue5"),
            Token::new_atom(";"),
        ],
        &mut out,
    );
    assert_ok!(result);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "@@ -1,4 +1,3 @@\n",
            "-int ivalue1;\n",
            " int ivalue2;\n",
            " int ivalue3;\n",
            " int ivalue4;\n", //
        )
    );
}

#[test]
fn format_removal_end() {
    // Check the diff format when data is removed at the end.
    let mut out = Vec::new();
    let result = write_type_diff(
        "dummy",
        &vec![
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue3"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue4"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue5"),
            Token::new_atom(";"),
        ],
        &vec![
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue3"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue4"),
            Token::new_atom(";"),
        ],
        &mut out,
    );
    assert_ok!(result);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "@@ -2,4 +2,3 @@\n",
            " int ivalue2;\n",
            " int ivalue3;\n",
            " int ivalue4;\n",
            "-int ivalue5;\n", //
        )
    );
}

#[test]
fn format_max_context() {
    // Check the diff format shows changes separated by up to 6 lines of context as one hunk.
    let mut out = Vec::new();
    let result = write_type_diff(
        "dummy",
        &vec![
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue3"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue4"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue5"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue6"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue7"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue8"),
            Token::new_atom(";"),
        ],
        &vec![
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue3"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue4"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue5"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue6"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue7"),
            Token::new_atom(";"),
        ],
        &mut out,
    );
    assert_ok!(result);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "@@ -1,8 +1,6 @@\n",
            "-int ivalue1;\n",
            " int ivalue2;\n",
            " int ivalue3;\n",
            " int ivalue4;\n",
            " int ivalue5;\n",
            " int ivalue6;\n",
            " int ivalue7;\n",
            "-int ivalue8;\n", //
        )
    );
}

#[test]
fn format_max_context2() {
    // Check the diff format shows changes separated by more than 6 lines of context as two hunks.
    let mut out = Vec::new();
    let result = write_type_diff(
        "dummy",
        &vec![
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue3"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue4"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue5"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue6"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue7"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue8"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue9"),
            Token::new_atom(";"),
        ],
        &vec![
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue3"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue4"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue5"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue6"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue7"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue8"),
            Token::new_atom(";"),
        ],
        &mut out,
    );
    assert_ok!(result);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "@@ -1,4 +1,3 @@\n",
            "-int ivalue1;\n",
            " int ivalue2;\n",
            " int ivalue3;\n",
            " int ivalue4;\n",
            "@@ -6,4 +5,3 @@\n",
            " int ivalue6;\n",
            " int ivalue7;\n",
            " int ivalue8;\n",
            "-int ivalue9;\n", //
        )
    );
}

#[test]
fn format_addition() {
    // Check the diff format when a struct member is added.
    let mut out = Vec::new();
    let result = write_type_diff(
        "s#test",
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
        &mut out,
    );
    assert_ok!(result);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "@@ -1,3 +1,4 @@\n",
            " struct test {\n",
            " \tint ivalue1;\n",
            "+\tint ivalue2;\n",
            " }\n", //
        )
    );
}

#[test]
fn format_modification() {
    // Check the diff format when a struct member is modified.
    let mut out = Vec::new();
    let result = write_type_diff(
        "s#test",
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue1"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
        &vec![
            Token::new_atom("struct"),
            Token::new_atom("test"),
            Token::new_atom("{"),
            Token::new_atom("int"),
            Token::new_atom("ivalue2"),
            Token::new_atom(";"),
            Token::new_atom("}"),
        ],
        &mut out,
    );
    assert_ok!(result);
    assert_eq!(
        str::from_utf8(&out).unwrap(),
        concat!(
            "@@ -1,3 +1,3 @@\n",
            " struct test {\n",
            "-\tint ivalue1;\n",
            "+\tint ivalue2;\n",
            " }\n", //
        )
    );
}
