//! Utility helpers for interactive REPL input handling.
//!
//! The main exposed helper is `is_complete_input(input: &str) -> bool` which
//! applies a tolerant JavaScript-aware heuristics to determine whether the
//! provided text forms a complete top-level input (can be evaluated) or if more
//! lines should be read. The heuristics try to ignore brackets inside strings,
//! template literals, regexes and comments and correctly treat `${...}` in
//! template literals.

/// Returns true when the given `input` looks like a complete JavaScript
/// top-level expression/program piece (i.e. brackets and template expressions
/// are balanced, strings/comments/regex literals are properly closed).
///
/// This uses heuristics (not a full parser) but covers common REPL cases:
/// - ignores brackets inside single/double-quoted strings
/// - supports template literals and nested ${ ... } expressions
/// - ignores brackets inside // and /* */ comments
/// - attempts to detect regex literals using a simple context heuristic and
///   ignores brackets inside them
pub fn is_complete_input(src: &str) -> bool {
    let mut bracket_stack: Vec<char> = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_regex = false;
    let mut escape = false;

    // small helper returns whether a char is considered a token that can
    // precede a regex literal (heuristic).
    fn can_start_regex(prev: Option<char>) -> bool {
        match prev {
            None => true,
            Some(p) => matches!(
                p,
                '(' | ',' | '=' | ':' | '[' | '!' | '?' | '{' | '}' | ';' | '\n' | '\r' | '\t' | ' '
            ),
        }
    }

    let mut prev_non_space: Option<char> = None;
    let mut chars = src.chars().peekable();

    while let Some(ch) = chars.next() {
        // handle escaping inside strings/template/regex
        if escape {
            escape = false;
            // don't treat escaped characters as structure
            continue;
        }

        // start of line comment
        if in_line_comment {
            if ch == '\n' || ch == '\r' {
                in_line_comment = false;
            }
            prev_non_space = Some(ch);
            continue;
        }

        // inside block comment
        if in_block_comment {
            if ch == '*'
                && let Some('/') = chars.peek().copied()
            {
                // consume '/'
                let _ = chars.next();
                in_block_comment = false;
                prev_non_space = Some('/');
                continue;
            }
            prev_non_space = Some(ch);
            continue;
        }

        // if inside a regex, look for unescaped trailing slash
        if in_regex {
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '/' {
                // consume optional flags after regex
                // we don't need to parse flags here, just stop regex mode
                in_regex = false;
                // consume following letters (flags) without affecting structure
                while let Some(&f) = chars.peek() {
                    if f.is_ascii_alphabetic() {
                        chars.next();
                    } else {
                        break;
                    }
                }
            }
            prev_non_space = Some(ch);
            continue;
        }

        // top-level string / template handling
        if in_single {
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '\'' {
                in_single = false;
            }
            prev_non_space = Some(ch);
            continue;
        }
        if in_double {
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '"' {
                in_double = false;
            }
            prev_non_space = Some(ch);
            continue;
        }
        if in_backtick {
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '`' {
                in_backtick = false;
                prev_non_space = Some('`');
                continue;
            }
            // template expression start: ${ ... }
            if ch == '$' && chars.peek() == Some(&'{') {
                // consume the '{'
                let _ = chars.next();
                // treat it as an opening brace in the normal bracket stack
                bracket_stack.push('}');
                prev_non_space = Some('{');
                continue;
            }
            // a closing '}' may appear while still inside the template literal
            // if it corresponds to a `${ ... }` expression — pop that marker
            if ch == '}' {
                if let Some(expected) = bracket_stack.pop() {
                    if expected != '}' {
                        return true; // mismatched - treat as complete and surface parse error
                    }
                } else {
                    // unmatched closing brace inside template - treat as complete
                    return true;
                }
                prev_non_space = Some('}');
                continue;
            }
            prev_non_space = Some(ch);
            continue;
        }

        // not inside any obvious literal or comment
        match ch {
            '\'' => in_single = true,
            '"' => in_double = true,
            '`' => in_backtick = true,
            '/' => {
                // Could be line comment '//' or block comment '/*' or regex literal '/.../'
                if let Some(&next) = chars.peek() {
                    if next == '/' {
                        // consume next and enter line comment
                        let _ = chars.next();
                        in_line_comment = true;
                        prev_non_space = Some('/');
                        continue;
                    } else if next == '*' {
                        // consume next and enter block comment
                        let _ = chars.next();
                        in_block_comment = true;
                        prev_non_space = Some('/');
                        continue;
                    }
                }

                // Heuristic: start regex when previous non-space allows it
                if can_start_regex(prev_non_space) {
                    in_regex = true;
                    prev_non_space = Some('/');
                    continue;
                }

                // otherwise treat as division/operator and continue
            }
            '(' => bracket_stack.push(')'),
            '[' => bracket_stack.push(']'),
            '{' => bracket_stack.push('}'),
            ')' | ']' | '}' => {
                if let Some(expected) = bracket_stack.pop() {
                    if expected != ch {
                        // mismatched closing; we treat the input as "complete"
                        // so a syntax error will be surfaced by the evaluator.
                        return true;
                    }
                } else {
                    // extra closing bracket — still treat as complete so user gets a parse error
                    return true;
                }
            }
            _ => {}
        }

        if !ch.is_whitespace() {
            prev_non_space = Some(ch);
        }
    }

    // Input is complete if we are not inside any multiline construct and there are
    // no unmatched opening brackets remaining.
    !in_single && !in_double && !in_backtick && !in_block_comment && !in_regex && bracket_stack.is_empty()
}

#[cfg(test)]
mod tests {
    use super::is_complete_input;

    #[test]
    fn test_balanced_simple() {
        assert!(is_complete_input("1 + 1"));
        assert!(is_complete_input("let a = 10;"));
    }

    #[test]
    fn test_unbalanced_brackets() {
        assert!(!is_complete_input("(1 + 2"));
        assert!(!is_complete_input("function f() {"));
        assert!(!is_complete_input("[1, 2"));
    }

    #[test]
    fn test_strings_and_comments() {
        assert!(is_complete_input("let s = '\\'not a bracket\\'';"));
        assert!(is_complete_input("// comment with { [ ( "));
        assert!(is_complete_input("/* block comment with { [ ( */"));
        assert!(is_complete_input("'a string with } inside'"));
    }

    #[test]
    fn test_template_literals() {
        // unterminated template (missing closing backtick) -> incomplete
        assert!(!is_complete_input("`unterminated template"));
        // closed template -> complete
        assert!(is_complete_input("`unterminated template`"));
        assert!(is_complete_input("`simple`"));
        // template with expression
        assert!(is_complete_input("`a ${1 + 2} b`"));
        // incomplete template expression
        assert!(!is_complete_input("`x ${ {`"));
    }

    #[test]
    fn test_regex_handling() {
        assert!(is_complete_input("/abc/.test('x')"));
        // regex with brackets shouldn't upset brackets counting
        assert!(is_complete_input("/([a-z]{2})/g"));
        // division (not regex) combined with open paren
        assert!(!is_complete_input("(a / 1"));
    }
}
