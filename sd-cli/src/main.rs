mod cli;

use clap::Parser;
use std::{io::stdout, process};

use sd::{Replacer, Result, Source, process_sources};

/// Exit status when the pattern matched no input.
///
/// Distinct from the status used for errors, so that a script can tell "your
/// pattern found nothing" apart from "sd could not do its job".
const NO_MATCH_EXIT_CODE: i32 = 2;

fn main() {
    let options = cli::Options::parse();

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

    fn message_for(args: &[&str]) -> String {
        let args = ["sd"].iter().chain(args);
        no_match_message(&cli::Options::parse_from(args))
    }
}
