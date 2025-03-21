// Copyright (C) 2024 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;

#[test]
fn matches_plain() {
    // Check wildcard matching when the pattern contains only simple characters.
    assert!(matches_wildcard("", ""));
    assert!(matches_wildcard("abc", "abc"));
    assert!(!matches_wildcard("abc", "Xbc"));
}

#[test]
fn matches_asterisk() {
    // Check wildcard matching with an asterisk as a multi-character wildcard.
    assert!(matches_wildcard("abc", "*"));
    assert!(matches_wildcard("abc", "a*"));
    assert!(matches_wildcard("abc", "*bc"));
    assert!(matches_wildcard("abc", "a*c"));
    assert!(matches_wildcard("abc", "ab*"));
    assert!(matches_wildcard("abc", "a****c"));
    assert!(matches_wildcard("a-b-c", "a*b*c"));
    assert!(!matches_wildcard("abcd", "*bc"));
}

#[test]
fn matches_question_mark() {
    // Check wildcard matching with a question mark as a single-character wildcard.
    assert!(!matches_wildcard("", "?"));
    assert!(matches_wildcard("a", "?"));
    assert!(!matches_wildcard("ab", "?"));
    assert!(matches_wildcard("abc", "?bc"));
    assert!(matches_wildcard("abc", "a?c"));
    assert!(matches_wildcard("abc", "ab?"));
    assert!(!matches_wildcard("abc", "a???c"));
    assert!(matches_wildcard("abdec", "a???c"));
    assert!(matches_wildcard("a-b-c", "a?b?c"));
    assert!(!matches_wildcard("abcd", "?bc"));
}

#[test]
fn matches_range() {
    // Check wildcard matching with a bracketed character range.
    assert!(!matches_wildcard("", "[abc]"));
    assert!(matches_wildcard("a", "[abc]"));
    assert!(matches_wildcard("b", "[abc]"));
    assert!(matches_wildcard("c", "[abc]"));
    assert!(!matches_wildcard("ab", "[abc]"));
    assert!(matches_wildcard("bde", "[abc]de"));
    assert!(matches_wildcard("dbe", "d[abc]e"));
    assert!(matches_wildcard("deb", "de[abc]"));
    assert!(matches_wildcard("dabce", "d[abc][abc][abc]e"));
    assert!(!matches_wildcard("abcd", "[abc]bc"));

    assert!(!matches_wildcard("", "[^abc]"));
    assert!(!matches_wildcard("a", "[^abc]"));
    assert!(!matches_wildcard("b", "[^abc]"));
    assert!(!matches_wildcard("c", "[^abc]"));
    assert!(matches_wildcard("d", "[^abc]"));
    assert!(!matches_wildcard("ab", "[^abc]"));

    assert!(!matches_wildcard("", "[a-c]"));
    assert!(matches_wildcard("a", "[a-c]"));
    assert!(matches_wildcard("b", "[a-c]"));
    assert!(matches_wildcard("c", "[a-c]"));
    assert!(!matches_wildcard("ab", "[a-c]"));

    assert!(matches_wildcard("a", "[a-cD-EF]"));
    assert!(matches_wildcard("b", "[a-cD-EF]"));
    assert!(matches_wildcard("c", "[a-cD-EF]"));
    assert!(matches_wildcard("D", "[a-cD-EF]"));
    assert!(matches_wildcard("E", "[a-cD-EF]"));
    assert!(matches_wildcard("F", "[a-cD-EF]"));

    assert!(matches_wildcard("]", "[]]"));
    assert!(matches_wildcard("a", "[^]]"));
    assert!(matches_wildcard("-", "[-]"));
    assert!(matches_wildcard("a", "[^-]"));
    assert!(matches_wildcard("a", "[^]-]"));
}
