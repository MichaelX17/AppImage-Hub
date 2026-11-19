//! Shell-like tokenizer for launch arguments.
//!
//! Arguments are passed literally to `execve` — no shell runs — but users still
//! need to write arguments containing spaces or quotes in the UI. This module
//! implements a small, dependency-free subset of POSIX quoting:
//!
//! - whitespace separates arguments outside quotes;
//! - single quotes preserve their content literally (no escapes);
//! - double quotes group content and allow `\` escapes;
//! - `\` escapes the next character outside quotes;
//! - `""` produces one empty argument;
//! - variables, globbing, and `$(...)` are **not** interpreted.
//!
//! Quoting is only part of a token (`--profile="My Profile"`), so that prefix
//! stays attached to the quoted value: `["--profile=My Profile"]`.

use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quote {
    None,
    Single,
    Double,
}

/// Splits a launch-argument display string into literal arguments.
pub fn split_args(input: &str) -> Result<Vec<String>> {
    let mut args: Vec<String> = Vec::new();
    let mut current = String::new();
    // Distinguishes `""` (one empty argument) from no token at all.
    let mut in_token = false;
    let mut quote = Quote::None;
    let mut chars = input.chars();

    while let Some(character) = chars.next() {
        match quote {
            Quote::None => match character {
                c if c.is_whitespace() => {
                    if in_token {
                        args.push(std::mem::take(&mut current));
                        in_token = false;
                    }
                }
                '\'' => {
                    quote = Quote::Single;
                    in_token = true;
                }
                '"' => {
                    quote = Quote::Double;
                    in_token = true;
                }
                '\\' => {
                    in_token = true;
                    match chars.next() {
                        Some(escaped) => current.push(escaped),
                        None => bail!("unterminated escape in launch arguments"),
                    }
                }
                _ => {
                    in_token = true;
                    current.push(character);
                }
            },
            Quote::Single => {
                if character == '\'' {
                    quote = Quote::None;
                } else {
                    current.push(character);
                }
            }
            Quote::Double => match character {
                '"' => quote = Quote::None,
                '\\' => match chars.next() {
                    Some(escaped) => current.push(escaped),
                    None => bail!("unterminated escape in launch arguments"),
                },
                _ => current.push(character),
            },
        }
    }

    if quote != Quote::None {
        bail!("unterminated quote in launch arguments");
    }
    if in_token {
        args.push(current);
    }
    Ok(args)
}

/// Renders `args` so that [`split_args`] returns exactly the same vector.
pub fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| quote_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_arg(arg: &str) -> String {
    let needs_quoting = arg.is_empty()
        || arg
            .chars()
            .any(|c| c.is_whitespace() || c == '"' || c == '\'' || c == '\\');
    if !needs_quoting {
        return arg.to_string();
    }

    let mut quoted = String::with_capacity(arg.len() + 2);
    quoted.push('"');
    for character in arg.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_args_plain() {
        assert_eq!(
            split_args("--fullscreen --foo=bar").unwrap(),
            vec!["--fullscreen", "--foo=bar"]
        );
        assert_eq!(split_args("").unwrap(), Vec::<String>::new());
        assert_eq!(split_args("   ").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn split_args_respects_single_and_double_quotes() {
        assert_eq!(
            split_args("a \"b c\" 'd e'").unwrap(),
            vec!["a", "b c", "d e"]
        );
        // Quoting part of a token keeps the prefix attached.
        assert_eq!(
            split_args("--profile=\"My Profile\" --flag").unwrap(),
            vec!["--profile=My Profile", "--flag"]
        );
    }

    #[test]
    fn split_args_handles_escaped_spaces() {
        assert_eq!(
            split_args("one\\ two three").unwrap(),
            vec!["one two", "three"]
        );
        assert_eq!(
            split_args("\"say \\\"hi\\\"\"").unwrap(),
            vec!["say \"hi\""]
        );
        assert_eq!(split_args("c:\\\\temp").unwrap(), vec!["c:\\temp"]);
    }

    #[test]
    fn split_args_keeps_empty_quoted_argument() {
        assert_eq!(split_args("a \"\" b ''").unwrap(), vec!["a", "", "b", ""]);
    }

    #[test]
    fn split_args_rejects_unterminated_quote() {
        assert!(split_args("a \"b c").is_err());
        assert!(split_args("a 'b c").is_err());
        assert!(split_args("trailing\\").is_err());
    }

    #[test]
    fn join_then_split_round_trips() {
        let cases: Vec<Vec<&str>> = vec![
            vec![],
            vec!["--fullscreen"],
            vec!["", "a", "--profile=My Profile", "b c"],
            vec!["say \"hi\"", "it's", "c:\\temp", "a\\b"],
            vec!["'", "\"", "\\", "$HOME", "`cmd`"],
            vec!["two  spaces", "\ttab", "line\nbreak"],
        ];
        for case in cases {
            let args: Vec<String> = case.into_iter().map(str::to_string).collect();
            let rendered = join_args(&args);
            assert_eq!(
                split_args(&rendered).unwrap(),
                args,
                "round trip failed for {rendered:?}"
            );
        }
    }
}
