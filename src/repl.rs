use crate::{JSError, runtime::JsRuntime};

/// A small persistent REPL environment backed by [`JsRuntime`].
pub struct Repl {
    runtime: JsRuntime,
}

impl Default for Repl {
    fn default() -> Self {
        Self::new()
    }
}

impl Repl {
    /// Create a REPL backed by a new [`JsRuntime`].
    pub fn new() -> Self {
        Repl { runtime: JsRuntime::new() }
    }

    /// Evaluate a script in the persistent runtime.
    /// Returns the evaluation result as a string or an error.
    pub fn eval<T: AsRef<str>>(&mut self, script: T) -> Result<String, JSError> {
        self.runtime.eval_repl(script)
    }

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
}
