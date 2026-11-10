use super::*;
use proptest::prelude::*;

proptest! {
    #[test]
    fn validate_doesnt_panic(s in r"(\PC*\$?){0,5}") {
        let _ = validate::validate_replace(&s);
    }

    // $ followed by a digit and a non-ident char or an ident char
    #[test]
    fn validate_ok(s in r"([^\$]*(\$([0-9][^a-zA-Z_0-9\$]|a-zA-Z_))?){0,5}") {
        validate::validate_replace(&s).unwrap();
    }

    // Force at least one $ followed by a digit and an ident char
    #[test]
    fn validate_err(s in r"[^\$]*?\$[0-9][a-zA-Z_]\PC*") {
        validate::validate_replace(&s).unwrap_err();
    }
}

#[derive(Default)]
struct Replace {
    look_for: &'static str,
    replace_with: &'static str,
    literal: bool,
    flags: Option<&'static str>,
    src: &'static str,
    expected: &'static str,
}

impl Replace {
    fn test(&self) {
        const UNLIMITED_REPLACEMENTS: usize = 0;
        let replacer = Replacer::new(
            self.look_for.into(),
            self.replace_with.into(),
            self.literal,
            self.flags.map(ToOwned::to_owned),
            UNLIMITED_REPLACEMENTS,
        )
        .unwrap();

        let binding = replacer.replace(self.src.as_bytes());
        let actual = std::str::from_utf8(&binding).unwrap();

        assert_eq!(self.expected, actual);
    }
}

#[test]
fn default_global() {
    Replace {
        look_for: "a",
        replace_with: "b",
        src: "aaa",
        expected: "bbb",
        ..Default::default()
    }
    .test();
}

#[test]
fn escaped_char_preservation() {
    Replace {
        look_for: "a",
        replace_with: "b",
        src: r#"a\n"#,
        expected: r#"b\n"#,
        ..Default::default()
    }
    .test();
}

#[test]
fn case_sensitive_default() {
    Replace {
        look_for: "abc",
        replace_with: "x",
        src: "abcABC",
        expected: "xABC",
        ..Default::default()
    }
    .test();

    Replace {
        look_for: "abc",
        replace_with: "x",
        literal: true,
        src: "abcABC",
        expected: "xABC",
        ..Default::default()
    }
    .test();
}

#[test]
fn sanity_check_literal_replacements() {
    Replace {
        look_for: "((special[]))",
        replace_with: "x",
        literal: true,
        src: "((special[]))y",
        expected: "xy",
        ..Default::default()
    }
    .test();
}

#[test]
fn unescape_regex_replacements() {
    Replace {
        look_for: "test",
        replace_with: r"\n",
        src: "testtest",
        expected: "\n\n",
        ..Default::default()
    }
    .test();
}

#[test]
fn no_unescape_literal_replacements() {
    Replace {
        look_for: "test",
        replace_with: r"\n",
        literal: true,
        src: "testtest",
        expected: r"\n\n",
        ..Default::default()
    }
    .test();
}

#[test]
fn full_word_replace() {
    Replace {
        look_for: "abc",
        replace_with: "def",
        flags: Some("w"),
        src: "abcd abc",
        expected: "abcd def",
        ..Default::default()
    }
    .test();
}

#[test]
fn escaping_unnecessarily() {
    // https://github.com/chmln/sd/issues/313
    Replace {
        look_for: "abc",
        replace_with: r#"\n{"#,
        src: "abc",
        expected: "\n{",
        ..Default::default()
    }
    .test();

    Replace {
        look_for: "abc",
        replace_with: r#"\n\{"#,
        src: "abc",
        expected: "\n\\{",
        ..Default::default()
    }
    .test();
}

#[test]
fn newline_in_find_spans_lines() {
    assert!(spans_lines(r"a\nb", false), "regex \\n");
    assert!(spans_lines("a\nb", false), "regex line feed");
    assert!(spans_lines("a\nb", true), "fixed-string line feed");
    assert!(spans_lines("a\\\nb", false), "escaped line feed");
    assert!(spans_lines(r"a\\\nb", false), "\\n after escaped backslash");
    assert!(spans_lines(r"[\n,]", false), "\\n in a class");
    assert!(spans_lines(r"[^a]\n", false), "\\n after a negated class");
}

#[test]
fn find_without_explicit_newline_stays_line_by_line() {
    assert!(!spans_lines(r"a\nb", true), "fixed-string backslash n");
    assert!(!spans_lines(r"a\\nb", false), "escaped backslash then n");
    assert!(!spans_lines(r"[^\n]+", false), "\\n in a negated class");
    assert!(!spans_lines(r"[a[^\n]]", false), "nested negated class");
    assert!(!spans_lines(r"a\sb", false), "\\s");
    assert!(!spans_lines(r"[^x]", false), "negated class");
    assert!(!spans_lines(r"(?s)a.b", false), "dot matching newline");
}

/// Whether `look_for` makes the CLI read each input whole instead of by line.
fn spans_lines(look_for: &str, literal: bool) -> bool {
    Replacer::new(look_for.into(), String::new(), literal, None, 0)
        .unwrap()
        .spans_lines()
}

#[test]
fn replacement_naming_a_missing_group_is_rejected() {
    for (look_for, replace_with, reference) in [
        ("path=.*", "path=$SOA_URL/import_map.json", "$SOA_URL"),
        ("x", "${n0} ${vis}", "${n0}"),
        ("x", "`${end.action} ${id}`", "${end.action}"),
        ("(a)", "$1$2", "$2"),
    ] {
        assert_eq!(
            unknown_capture(look_for, replace_with).as_deref(),
            Some(reference),
            "{replace_with}"
        );
    }
}

#[test]
fn replacement_naming_existing_groups_is_accepted() {
    assert_eq!(unknown_capture("(a)(?P<b>b)", "$0 $1 ${2} $b ${b}"), None);
    assert_eq!(unknown_capture("x", "$$HOME"), None, "escaped dollar");
    assert!(
        Replacer::new("x".into(), "$HOME".into(), true, None, 0).is_ok(),
        "fixed strings"
    );
}

/// The capture reference `Replacer::new` rejects, if any.
fn unknown_capture(look_for: &str, replace_with: &str) -> Option<String> {
    match Replacer::new(look_for.into(), replace_with.into(), false, None, 0) {
        Err(crate::Error::UnknownCapture(reference)) => Some(reference),
        Err(e) => panic!("unexpected error: {e}"),
        Ok(_) => None,
    }
}
