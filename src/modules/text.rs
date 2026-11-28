//! Higher-level text processing helpers that complement the `string` module.
//!
//! Where `string` offers primitive operations on strings, `text` works on
//! whole pieces of text: word splitting, identifier case conversion,
//! whitespace normalisation and a few small string algorithms. All widths and
//! indices are counted in Unicode scalar values (characters), not bytes.

use crate::modules::support::as_usize;
#[cfg(test)]
use crate::modules::support::call_native;
use crate::runtime::{expect_string, native};
use crate::value::Value;
use std::collections::BTreeMap;

/// Every exported name is a native function so registration and the shared
/// backend checks in `modules/mod.rs` stay simple.
pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "words".to_string(),
        native("text.words", |args| {
            let text = first_string(args, "words")?;
            Ok(Value::new_array(
                text.split_whitespace()
                    .map(|word| Value::String(word.to_string()))
                    .collect(),
            ))
        }),
    );

    fields.insert(
        "title".to_string(),
        native("text.title", |args| {
            let text = first_string(args, "title")?;
            Ok(Value::String(
                text.split_whitespace()
                    .map(capitalize_word)
                    .collect::<Vec<_>>()
                    .join(" "),
            ))
        }),
    );

    fields.insert(
        "capitalize".to_string(),
        native("text.capitalize", |args| {
            let text = first_string(args, "capitalize")?;
            Ok(Value::String(capitalize_word(&text)))
        }),
    );

    fields.insert(
        "swapCase".to_string(),
        native("text.swapCase", |args| {
            let text = first_string(args, "swapCase")?;
            let mut swapped = String::new();
            for ch in text.chars() {
                if ch.is_uppercase() {
                    swapped.extend(ch.to_lowercase());
                } else {
                    swapped.extend(ch.to_uppercase());
                }
            }
            Ok(Value::String(swapped))
        }),
    );

    fields.insert(
        "snake".to_string(),
        native("text.snake", |args| {
            let text = first_string(args, "snake")?;
            Ok(Value::String(identifier_words(&text).join("_")))
        }),
    );

    fields.insert(
        "kebab".to_string(),
        native("text.kebab", |args| {
            let text = first_string(args, "kebab")?;
            Ok(Value::String(identifier_words(&text).join("-")))
        }),
    );

    fields.insert(
        "camel".to_string(),
        native("text.camel", |args| {
            let text = first_string(args, "camel")?;
            let words = identifier_words(&text);
            let mut result = words.first().cloned().unwrap_or_default();
            for word in words.iter().skip(1) {
                result.push_str(&capitalize_word(word));
            }
            Ok(Value::String(result))
        }),
    );

    fields.insert(
        "pascal".to_string(),
        native("text.pascal", |args| {
            let text = first_string(args, "pascal")?;
            Ok(Value::String(
                identifier_words(&text)
                    .iter()
                    .map(|word| capitalize_word(word))
                    .collect::<Vec<_>>()
                    .join(""),
            ))
        }),
    );

    fields.insert(
        "slug".to_string(),
        native("text.slug", |args| {
            let text = first_string(args, "slug")?;
            let mut result = String::new();
            for ch in text.chars() {
                if ch.is_alphanumeric() {
                    result.extend(ch.to_lowercase());
                } else if !result.ends_with('-') {
                    result.push('-');
                }
            }
            Ok(Value::String(result.trim_matches('-').to_string()))
        }),
    );

    fields.insert(
        "collapseWhitespace".to_string(),
        native("text.collapseWhitespace", |args| {
            let text = first_string(args, "collapseWhitespace")?;
            Ok(Value::String(
                text.split_whitespace().collect::<Vec<_>>().join(" "),
            ))
        }),
    );

    fields.insert(
        "count".to_string(),
        native("text.count", |args| {
            let text = first_string(args, "count")?;
            let needle = expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the substring for text.count",
            )?;
            if needle.is_empty() {
                return Err("text.count needs a non-empty substring".to_string());
            }
            Ok(Value::Number(text.match_indices(&needle).count() as f64))
        }),
    );

    fields.insert(
        "truncate".to_string(),
        native("text.truncate", |args| {
            let text = first_string(args, "truncate")?;
            let width = as_usize(
                args.get(1).unwrap_or(&Value::Null),
                "as the width for text.truncate",
            )?;
            let suffix = args
                .get(2)
                .map(|value| expect_string(value, "as the suffix for text.truncate"))
                .transpose()?
                .unwrap_or_else(|| "...".to_string());
            let chars: Vec<char> = text.chars().collect();
            if chars.len() <= width {
                return Ok(Value::String(text));
            }
            let suffix_chars: Vec<char> = suffix.chars().collect();
            if suffix_chars.len() >= width {
                return Ok(Value::String(chars[..width].iter().collect()));
            }
            let mut result: String = chars[..width - suffix_chars.len()].iter().collect();
            result.extend(suffix_chars);
            Ok(Value::String(result))
        }),
    );

    fields.insert(
        "wrap".to_string(),
        native("text.wrap", |args| {
            let text = first_string(args, "wrap")?;
            let width = as_usize(
                args.get(1).unwrap_or(&Value::Null),
                "as the width for text.wrap",
            )?;
            if width == 0 {
                return Err("text.wrap width must be greater than zero".to_string());
            }
            let mut lines: Vec<String> = Vec::new();
            let mut current = String::new();
            for word in text.split_whitespace() {
                if current.is_empty() {
                    current.push_str(word);
                } else if current.chars().count() + 1 + word.chars().count() <= width {
                    current.push(' ');
                    current.push_str(word);
                } else {
                    lines.push(std::mem::take(&mut current));
                    current.push_str(word);
                }
            }
            if !current.is_empty() {
                lines.push(current);
            }
            Ok(Value::new_array(
                lines.into_iter().map(Value::String).collect(),
            ))
        }),
    );

    fields.insert(
        "indent".to_string(),
        native("text.indent", |args| {
            let text = first_string(args, "indent")?;
            let prefix = expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the prefix for text.indent",
            )?;
            // `split_inclusive` keeps trailing newlines and CRLF terminators;
            // `str::lines()` would silently normalise both away.
            let mut result = String::new();
            for segment in text.split_inclusive('\n') {
                result.push_str(&prefix);
                result.push_str(segment);
            }
            Ok(Value::String(result))
        }),
    );

    fields.insert(
        "dedent".to_string(),
        native("text.dedent", |args| {
            let text = first_string(args, "dedent")?;
            let segments: Vec<&str> = text.split_inclusive('\n').collect();
            let common = segments
                .iter()
                .map(|segment| split_line_ending(segment).0)
                .filter(|body| !body.trim().is_empty())
                .map(|body| body.chars().take_while(|c| *c == ' ' || *c == '\t').count())
                .min()
                .unwrap_or(0);
            let mut result = String::new();
            for segment in segments {
                let (body, ending) = split_line_ending(segment);
                result.extend(body.chars().skip(common));
                result.push_str(ending);
            }
            Ok(Value::String(result))
        }),
    );

    fields.insert(
        "reverseWords".to_string(),
        native("text.reverseWords", |args| {
            let text = first_string(args, "reverseWords")?;
            Ok(Value::String(
                text.split_whitespace().rev().collect::<Vec<_>>().join(" "),
            ))
        }),
    );

    fields.insert(
        "initials".to_string(),
        native("text.initials", |args| {
            let text = first_string(args, "initials")?;
            let mut result = String::new();
            for word in text.split_whitespace() {
                let Some(first) = word.chars().next() else {
                    continue;
                };
                result.extend(first.to_uppercase());
            }
            Ok(Value::String(result))
        }),
    );

    fields.insert(
        "isPalindrome".to_string(),
        native("text.isPalindrome", |args| {
            let text = first_string(args, "isPalindrome")?;
            // Compare case-folded alphanumeric characters so punctuation,
            // spacing and case are ignored. Full folding can expand a single
            // character, so build a flat vec of characters first.
            let mut folded = String::new();
            for ch in text.chars().filter(|ch| ch.is_alphanumeric()) {
                folded.push_str(&casefold_char(ch));
            }
            let normalized: Vec<char> = folded.chars().collect();
            Ok(Value::Bool(normalized.iter().eq(normalized.iter().rev())))
        }),
    );

    fields.insert(
        "levenshtein".to_string(),
        native("text.levenshtein", |args| {
            let left = first_string(args, "levenshtein")?;
            let right = expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the second string for text.levenshtein",
            )?;
            // An empty side needs only the other side's length, so return
            // before allocating a table that could be arbitrarily wide.
            if left.is_empty() {
                return Ok(Value::Number(right.chars().count() as f64));
            }
            if right.is_empty() {
                return Ok(Value::Number(left.chars().count() as f64));
            }
            let a: Vec<char> = left.chars().collect();
            let b: Vec<char> = right.chars().collect();
            // Bound the dynamic-programming table so a typo cannot allocate
            // an unreasonable amount of memory.
            if a.len().saturating_mul(b.len()) > 1_000_000 {
                return Err("text.levenshtein inputs are too long".to_string());
            }
            let mut previous: Vec<usize> = (0..=b.len()).collect();
            let mut current = vec![0usize; b.len() + 1];
            for (i, ca) in a.iter().enumerate() {
                current[0] = i + 1;
                for (j, cb) in b.iter().enumerate() {
                    let cost = usize::from(ca != cb);
                    current[j + 1] = (previous[j + 1] + 1)
                        .min(current[j] + 1)
                        .min(previous[j] + cost);
                }
                std::mem::swap(&mut previous, &mut current);
            }
            Ok(Value::Number(previous[b.len()] as f64))
        }),
    );

    Value::new_object(fields)
}

/// Read the first argument as text, with an error message naming the function.
fn first_string(args: &[Value], function: &str) -> Result<String, String> {
    expect_string(
        args.get(0).unwrap_or(&Value::Null),
        &format!("for text.{function}"),
    )
}

/// Split a `split_inclusive('\n')` segment into its body and line ending,
/// keeping CRLF as a single terminator.
fn split_line_ending(segment: &str) -> (&str, &str) {
    let Some(body) = segment.strip_suffix('\n') else {
        return (segment, "");
    };
    match body.strip_suffix('\r') {
        Some(body) => (body, "\r\n"),
        None => (body, "\n"),
    }
}

/// Apply Unicode full case folding for the common multi-character folds that
/// `char::to_lowercase` does not cover. Returned strings are always non-empty
/// for alphanumeric inputs.
fn casefold_char(ch: char) -> String {
    match ch {
        'ß' => "ss".to_string(),
        'ẛ' => "ṡ".to_string(),
        'ﬀ' => "ff".to_string(),
        'ﬁ' => "fi".to_string(),
        'ﬂ' => "fl".to_string(),
        'ﬃ' => "ffi".to_string(),
        'ﬄ' => "ffl".to_string(),
        'ﬅ' | 'ﬆ' => "st".to_string(),
        'ς' => "σ".to_string(),
        other => other.to_lowercase().collect(),
    }
}

/// Uppercase the first character and lowercase the rest.
fn capitalize_word(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let mut result: String = first.to_uppercase().collect();
            result.extend(chars.flat_map(char::to_lowercase));
            result
        }
    }
}

/// Split a mixed-case identifier into lowercase words.
///
/// Separators (whitespace, `_`, `-`, `.`, ...) start a new word, as do camel
/// case boundaries and acronym boundaries: `HTTPServer` becomes
/// `["http", "server"]`.
fn identifier_words(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut words = Vec::new();
    let mut current = String::new();

    for (index, &ch) in chars.iter().enumerate() {
        if !ch.is_alphanumeric() {
            if !current.is_empty() {
                words.push(current.to_lowercase());
                current.clear();
            }
            continue;
        }
        if ch.is_uppercase() && !current.is_empty() {
            let previous = chars[index - 1];
            let next_is_lowercase = chars.get(index + 1).is_some_and(|ch| ch.is_lowercase());
            let boundary = previous.is_lowercase()
                || previous.is_numeric()
                || (previous.is_uppercase() && next_is_lowercase);
            if boundary {
                words.push(std::mem::take(&mut current).to_lowercase());
            }
        }
        current.push(ch);
    }
    if !current.is_empty() {
        words.push(current.to_lowercase());
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(value: &str) -> Value {
        Value::String(value.to_string())
    }

    fn call(name: &str, args: &[Value]) -> Result<Value, String> {
        call_native(&module(), name, args)
    }

    fn apply(name: &str, value: &str) -> String {
        call(name, &[string(value)]).unwrap().display()
    }

    fn strings(value: Value) -> Vec<String> {
        let Value::Array(items) = value else {
            panic!("expected an array result");
        };
        let result = items
            .borrow()
            .iter()
            .map(|item| match item {
                Value::String(text) => text.clone(),
                other => panic!("expected a string item, found {}", other.type_name()),
            })
            .collect();
        result
    }

    #[test]
    fn exports_exactly_the_expected_native_functions() {
        let expected = [
            "words",
            "title",
            "capitalize",
            "swapCase",
            "snake",
            "kebab",
            "camel",
            "pascal",
            "slug",
            "collapseWhitespace",
            "count",
            "truncate",
            "wrap",
            "indent",
            "dedent",
            "reverseWords",
            "initials",
            "isPalindrome",
            "levenshtein",
        ];
        let Value::Object(fields) = module() else {
            panic!("text module is not an object");
        };
        let fields = fields.borrow();
        assert_eq!(fields.len(), expected.len(), "unexpected export count");
        for name in expected {
            assert!(fields.contains_key(name), "missing text.{name} export");
        }
        for (name, value) in fields.iter() {
            assert!(
                matches!(value, Value::Native(_)),
                "text.{name} is not native"
            );
        }
    }

    #[test]
    fn words_and_reverse_words_ignore_extra_whitespace() {
        assert_eq!(
            strings(call("words", &[string("  hello\twide\nworld ")]).unwrap()),
            ["hello", "wide", "world"]
        );
        assert_eq!(apply("reverseWords", "one  two three"), "three two one");
        assert_eq!(apply("reverseWords", "   "), "");
        assert!(call("words", &[Value::Number(3.0)]).is_err());
    }

    #[test]
    fn title_capitalize_and_swap_case() {
        assert_eq!(apply("title", "hello   WORLD"), "Hello World");
        assert_eq!(apply("capitalize", "hELLO world"), "Hello world");
        assert_eq!(apply("capitalize", ""), "");
        assert_eq!(apply("swapCase", "Hello, World!"), "hELLO, wORLD!");
        assert_eq!(apply("swapCase", "héLLO"), "HÉllo");
    }

    #[test]
    fn identifier_case_conversions() {
        assert_eq!(
            apply("snake", "HTTP Server-response"),
            "http_server_response"
        );
        assert_eq!(apply("kebab", "userID"), "user-id");
        assert_eq!(apply("camel", "hello_world"), "helloWorld");
        assert_eq!(apply("camel", "XMLHttpRequest"), "xmlHttpRequest");
        assert_eq!(apply("pascal", "hello-world"), "HelloWorld");
        assert_eq!(apply("pascal", "hello"), "Hello");
        assert_eq!(
            apply("snake", "a١B"),
            "a١_b",
            "Unicode digits are boundaries too"
        );
        assert_eq!(apply("snake", ""), "");
    }

    #[test]
    fn slug_and_whitespace_normalisation() {
        assert_eq!(apply("slug", "  Hello, World!  "), "hello-world");
        assert_eq!(apply("slug", "a___b---c"), "a-b-c");
        assert_eq!(apply("collapseWhitespace", "a \t\n b   c"), "a b c");
        assert_eq!(apply("collapseWhitespace", "   "), "");
    }

    #[test]
    fn count_is_non_overlapping() {
        assert!(matches!(
            call("count", &[string("aaaa"), string("aa")]).unwrap(),
            Value::Number(n) if n == 2.0
        ));
        assert!(matches!(
            call("count", &[string("banana"), string("ana")]).unwrap(),
            Value::Number(n) if n == 1.0
        ));
        assert!(matches!(
            call("count", &[string("abc"), string("z")]).unwrap(),
            Value::Number(n) if n == 0.0
        ));
        assert!(call("count", &[string("abc"), string("")]).is_err());
    }

    #[test]
    fn truncate_counts_characters_and_handles_suffixes() {
        assert_eq!(
            call("truncate", &[string("hello world"), Value::Number(20.0)])
                .unwrap()
                .display(),
            "hello world"
        );
        assert_eq!(
            call("truncate", &[string("héllo world"), Value::Number(8.0)])
                .unwrap()
                .display(),
            "héllo..."
        );
        assert_eq!(
            call(
                "truncate",
                &[string("hello world"), Value::Number(4.0), string("~~"),]
            )
            .unwrap()
            .display(),
            "he~~"
        );
        assert_eq!(
            call(
                "truncate",
                &[string("hello"), Value::Number(2.0), string("..."),]
            )
            .unwrap()
            .display(),
            "he"
        );
        assert!(call("truncate", &[string("hello"), Value::Number(-1.0)]).is_err());
    }

    #[test]
    fn wrap_produces_lines_no_wider_than_requested() {
        let lines =
            strings(call("wrap", &[string("the quick brown fox"), Value::Number(9.0)]).unwrap());
        assert_eq!(lines, ["the quick", "brown fox"]);
        let long_word = strings(
            call(
                "wrap",
                &[string("supercalifragilistic"), Value::Number(4.0)],
            )
            .unwrap(),
        );
        assert_eq!(long_word, ["supercalifragilistic"]);
        assert!(call("wrap", &[string("a b"), Value::Number(0.0)]).is_err());
    }

    #[test]
    fn indent_and_dedent_handle_multiline_text() {
        assert_eq!(
            call("indent", &[string("a\n\nb"), string("> ")])
                .unwrap()
                .display(),
            "> a\n> \n> b"
        );
        assert_eq!(apply("dedent", "    a\n      b\n    c"), "a\n  b\nc");
        assert_eq!(apply("dedent", "a\nb"), "a\nb");
        assert!(call("indent", &[string("a")]).is_err());
    }

    #[test]
    fn indent_and_dedent_preserve_line_endings() {
        assert_eq!(
            call("indent", &[string("a\n"), string("> ")])
                .unwrap()
                .display(),
            "> a\n",
            "a trailing newline must survive indentation"
        );
        assert_eq!(
            call("indent", &[string("a\r\nb"), string("> ")])
                .unwrap()
                .display(),
            "> a\r\n> b",
            "CRLF must not be normalised to LF"
        );
        assert_eq!(
            apply("dedent", "  a\r\n  b\r\n"),
            "a\r\nb\r\n",
            "dedent must preserve CRLF and the final terminator"
        );
    }

    #[test]
    fn initials_and_palindrome_checks() {
        assert_eq!(apply("initials", "ada lovelace"), "AL");
        assert_eq!(apply("initials", " john   doe "), "JD");
        assert_eq!(apply("initials", ""), "");
        assert!(matches!(
            call("isPalindrome", &[string("A man, a plan, a canal: Panama!")]).unwrap(),
            Value::Bool(true)
        ));
        assert!(matches!(
            call("isPalindrome", &[string("héllo")]).unwrap(),
            Value::Bool(false)
        ));
        assert!(
            matches!(
                call("isPalindrome", &[string("ßSS")]).unwrap(),
                Value::Bool(true)
            ),
            "full case folding must expand ß to ss"
        );
    }

    #[test]
    fn module_is_importable_in_both_backends() {
        // Every export is called here so a name that resolves in one backend
        // but not the other cannot hide behind a two-function smoke test.
        let source = r#"
import text
r01 = text.words("a b")
r02 = text.title("hello world")
r03 = text.capitalize("hELLO")
r04 = text.swapCase("aB")
r05 = text.snake("HTTP Server")
r06 = text.kebab("HTTP Server")
r07 = text.camel("http server")
r08 = text.pascal("http server")
r09 = text.slug("Hi, There!")
r10 = text.collapseWhitespace("a   b")
r11 = text.count("aaaa", "aa")
r12 = text.truncate("abcdef", 4)
r13 = text.wrap("a b c", 2)
r14 = text.indent("a\n", "> ")
r15 = text.dedent("  a\n  b")
r16 = text.reverseWords("a b")
r17 = text.initials("ada lovelace")
r18 = text.isPalindrome("ßSS")
r19 = text.levenshtein("kitten", "sitting")
"#;
        let program = crate::parser::parse(crate::lexer::tokenize(source).unwrap()).unwrap();

        let mut interpreter = crate::interpreter::Interpreter::new();
        interpreter.run(&program).unwrap();

        let chunk = crate::compiler::compile(&program).unwrap();
        let mut vm = crate::vm::Vm::new();
        vm.run(chunk).unwrap();

        let expectations = [
            ("r01", "[\"a\", \"b\"]"),
            ("r02", "Hello World"),
            ("r03", "Hello"),
            ("r04", "Ab"),
            ("r05", "http_server"),
            ("r06", "http-server"),
            ("r07", "httpServer"),
            ("r08", "HttpServer"),
            ("r09", "hi-there"),
            ("r10", "a b"),
            ("r11", "2"),
            ("r12", "a..."),
            ("r13", "[\"a\", \"b\", \"c\"]"),
            ("r14", "> a\n"),
            ("r15", "a\nb"),
            ("r16", "b a"),
            ("r17", "AL"),
            ("r18", "true"),
            ("r19", "3"),
        ];
        for (name, expected) in expectations {
            assert_eq!(interpreter.globals.get(name).unwrap().display(), expected);
            assert_eq!(vm.globals.get(name).unwrap().display(), expected);
        }
    }

    #[test]
    fn levenshtein_measures_edit_distance_in_characters() {
        assert!(matches!(
            call("levenshtein", &[string("kitten"), string("sitting")]).unwrap(),
            Value::Number(n) if n == 3.0
        ));
        assert!(matches!(
            call("levenshtein", &[string(""), string("abc")]).unwrap(),
            Value::Number(n) if n == 3.0
        ));
        assert!(matches!(
            call("levenshtein", &[string("café"), string("cafe")]).unwrap(),
            Value::Number(n) if n == 1.0
        ));
        assert!(call("levenshtein", &[string("only one")]).is_err());
        let huge = "x".repeat(1_000_001);
        assert!(matches!(
            call("levenshtein", &[string(""), string(&huge)]).unwrap(),
            Value::Number(n) if n == 1_000_001.0
        ));
        assert!(
            call("levenshtein", &[string("a"), string(&huge)]).is_err(),
            "a wide table with a non-empty left side must be rejected"
        );
    }
}
