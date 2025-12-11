use crate::{
    JSError,
    core::{
        JSObjectDataPtr, PropertyKey, Value, evaluate_statements, filter_input_script, initialize_global_constructors, new_js_object_data,
        obj_get_value, obj_set_value, parse_statements, tokenize, value_to_string,
    },
    js_promise::{PromiseState, run_event_loop},
};

/// A small persistent REPL environment wrapper.
///
/// Notes:
/// - `Repl::new()` creates a persistent environment and initializes built-ins.
/// - `Repl::eval(&self, code)` evaluates the provided code in the persistent env
///   so variables, functions and imports persist between calls.
pub struct Repl {
    env: JSObjectDataPtr,
}

impl Default for Repl {
    fn default() -> Self {
        Self::new()
    }
}

impl Repl {
    /// Create a new persistent REPL environment (with built-ins initialized).
    pub fn new() -> Self {
        let env: JSObjectDataPtr = new_js_object_data();
        env.borrow_mut().is_function_scope = true;
        // Initialize built-in constructors once for the persistent environment
        initialize_global_constructors(&env).unwrap();
        Repl { env }
    }

    /// Evaluate a script in the persistent environment.
    /// Returns the evaluation result or an error.
    pub fn eval<T: AsRef<str>>(&self, script: T) -> Result<Value, JSError> {
        let script = script.as_ref();
        let filtered = filter_input_script(script);

        // Parse tokens and statements
        let mut tokens = tokenize(&filtered)?;
        let statements = parse_statements(&mut tokens)?;

        // Inject simple host `std` / `os` shims when importing with the pattern:
        //   import * as NAME from "std";
        for line in script.lines() {
            let l = line.trim();
            if l.starts_with("import * as")
                && l.contains("from")
                && let (Some(as_idx), Some(from_idx)) = (l.find("as"), l.find("from"))
            {
                let name_part = &l[as_idx + 2..from_idx].trim();
                let name = PropertyKey::String(name_part.trim().to_string());
                if let Some(start_quote) = l[from_idx..].find(|c: char| ['"', '\''].contains(&c)) {
                    let quote_char = l[from_idx + start_quote..].chars().next().unwrap();
                    let rest = &l[from_idx + start_quote + 1..];
                    if let Some(end_quote) = rest.find(quote_char) {
                        let module = &rest[..end_quote];
                        if module == "std" {
                            obj_set_value(&self.env, &name, Value::Object(crate::js_std::make_std_object()?))?;
                        } else if module == "os" {
                            obj_set_value(&self.env, &name, Value::Object(crate::js_os::make_os_object()?))?;
                        }
                    }
                }
            }
        }

        match evaluate_statements(&self.env, &statements) {
            Ok(v) => {
                // If the result is a Promise object (wrapped in Object with __promise property), wait for it to resolve
                if let Value::Object(obj) = &v
                    && let Some(promise_val_rc) = obj_get_value(obj, &"__promise".into())?
                    && let Value::Promise(promise) = &*promise_val_rc.borrow()
                {
                    // Run the event loop until the promise is resolved
                    loop {
                        run_event_loop()?;
                        let promise_borrow = promise.borrow();
                        match &promise_borrow.state {
                            PromiseState::Fulfilled(val) => return Ok(val.clone()),
                            PromiseState::Rejected(reason) => {
                                return Err(raise_eval_error!(format!("Promise rejected: {}", value_to_string(reason))));
                            }
                            PromiseState::Pending => {
                                // Continue running the event loop
                            }
                        }
                    }
                }
                // Run event loop once to process any queued asynchronous tasks
                run_event_loop()?;
                Ok(v)
            }
            Err(e) => Err(e),
        }
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
