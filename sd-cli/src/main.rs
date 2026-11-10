mod cli;

use clap::{CommandFactory, FromArgMatches, Parser, error::ErrorKind};
use std::{ffi::OsString, io::stdout, process};

use sd::{Replacer, Result, Source, process_sources};

/// Exit status when the pattern matched no input.
///
/// Distinct from the status used for errors, so that a script can tell "your
/// pattern found nothing" apart from "sd could not do its job".
const NO_MATCH_EXIT_CODE: i32 = 2;

fn main() {
    let options = parse_options(std::env::args_os().collect())
        .unwrap_or_else(|e| e.exit());

    // Any replacements have already been written by the time this reports
    // no match. It describes the run, it does not make the run conditional.
    match run(&options) {
        Ok(true) => {}
        Ok(false) if options.allow_no_match => {}
        Ok(false) => {
            eprintln!("{}", no_match_message(&options));
            process::exit(NO_MATCH_EXIT_CODE);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

/// Parses like clap, except that a `-`-led FIND or REPLACE_WITH that clap
/// rejects as an unknown option, such as `- item`, is taken as a value.
///
/// Allowing hyphen values from the start would also take attached option
/// values such as `-n1` or `-fi` as FIND, so the lenient parse only runs once
/// the strict one has failed, leaving every input that parsed before as is.
fn parse_options(args: Vec<OsString>) -> Result<cli::Options, clap::Error> {
    let error = match cli::Options::try_parse_from(&args) {
        Err(error) if error.kind() == ErrorKind::UnknownArgument => error,
        strict => return strict,
    };
    cli::Options::command()
        // `mut_arg` would move the argument to the end, reordering positionals.
        .mut_args(|arg| match arg.get_id().as_str() {
            "find" | "replace_with" => arg.allow_hyphen_values(true),
            _ => arg,
        })
        .try_get_matches_from(&args)
        .and_then(|matches| cli::Options::from_arg_matches(&matches))
        // The strict error describes the typo better, if the input was one.
        .map_err(|_| error)
}

/// Returns whether the pattern matched any input.
fn run(options: &cli::Options) -> Result<bool> {
    let replacer = Replacer::new(
        options.find.clone(),
        options.replace_with.clone(),
        options.literal_mode,
        options.flags.clone(),
        options.replacements,
    )?;

    let sources = if !options.files.is_empty() {
        Source::from_paths(options.files.clone())
    } else {
        Ok(Source::from_stdin())
    };
    let sources = sources?;

    // A pattern that names a line feed can never match a single line, so it
    // implies --across.
    let line_by_line = !options.across && !replacer.spans_lines();

    let mut handle = stdout().lock();
    process_sources(
        &replacer,
        &sources,
        options.preview,
        line_by_line,
        &mut handle,
    )
}

fn no_match_message(options: &cli::Options) -> String {
    let place = match options.files.as_slice() {
        [] => "stdin".to_string(),
        [path] => path.display().to_string(),
        paths => format!("any of {} files", paths.len()),
    };
    // `$` and `(` are the metacharacters most likely meant literally, as in
    // prices, shell variables and function calls.
    let hint = if !options.literal_mode && options.find.contains(['$', '(']) {
        " (FIND is a regex; use -F to match $ and ( literally)"
    } else {
        ""
    };
    format!("sd: no match in {place}{hint}")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;

    /// Markdown list items and option-like text are common things to edit.
    #[test]
    fn hyphen_led_find_and_replacement_need_no_double_dash() {
        for (find, replace_with) in [
            ("- item", "- new item"),
            ("--import-map", "x"),
            ("-Command", "x"),
            ("- [bar width]", "- x"),
            ("x", "- y"),
        ] {
            let options = parse(&["-F", find, replace_with, "a.md"]);
            assert_eq!(options.find, find);
            assert_eq!(options.replace_with, replace_with);
            assert_eq!(options.files, [Path::new("a.md")], "{find}");
        }
    }

    /// Spellings that parsed before hyphen-led values were allowed keep their
    /// meaning, wherever they sit among the positionals.
    #[test]
    fn attached_option_values_and_flag_clusters_parse_as_before() {
        let positionals =
            |options: cli::Options| (options.find, options.replace_with);
        let expected = ("a".to_owned(), "b".to_owned());

        for args in [["-n1", "a", "b"], ["a", "-n1", "b"], ["-n=1", "a", "b"]] {
            let options = parse(&args);
            assert_eq!(options.replacements, 1, "{args:?}");
            assert_eq!(positionals(options), expected);
        }
        for args in [["-fi", "a", "b"], ["a", "-fi", "b"]] {
            let options = parse(&args);
            assert_eq!(options.flags.as_deref(), Some("i"), "{args:?}");
            assert_eq!(positionals(options), expected);
        }
        for args in [["-FA", "a", "b"], ["a", "-Ap", "b"]] {
            let options = parse(&args);
            assert!(options.across, "{args:?}");
            assert!(options.literal_mode || options.preview, "{args:?}");
            assert_eq!(positionals(options), expected);
        }
    }

    #[test]
    fn options_after_positionals_still_apply() {
        let options = parse(&["a", "b", "file", "--across"]);
        assert!(options.across);
        assert_eq!(options.files, [Path::new("file")]);

        let options = parse(&["- item", "b", "-p"]);
        assert!(options.preview);
        assert_eq!(options.find, "- item");
        assert!(options.files.is_empty());
    }

    #[test]
    fn double_dash_passes_option_names_as_values() {
        let options = parse(&["--", "-F", "x", "f"]);
        assert!(!options.literal_mode);
        assert_eq!(options.find, "-F");
        assert_eq!(options.files, [Path::new("f")]);
    }

    #[test]
    fn regex_with_group_suggests_fixed_strings() {
        assert_eq!(
            message_for(&["f(x)", "g(x)", "a.ts"]),
            "sd: no match in a.ts \
             (FIND is a regex; use -F to match $ and ( literally)",
        );
    }

    #[test]
    fn fixed_strings_get_no_hint() {
        assert_eq!(
            message_for(&["-F", "f($x)", "g", "a.ts"]),
            "sd: no match in a.ts",
        );
    }

    fn parse(args: &[&str]) -> cli::Options {
        let args = ["sd"].iter().chain(args).map(OsString::from).collect();
        parse_options(args).unwrap()
    }

    fn message_for(args: &[&str]) -> String {
        let args = ["sd"].iter().chain(args);
        no_match_message(&cli::Options::parse_from(args))
    }
}
