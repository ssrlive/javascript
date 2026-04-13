use crate::JSError;
use crate::core::statement::{
    CatchParamPattern, ExportSpecifier, ForOfPattern, ForStatement, IfStatement, ImportSpecifier, Statement, StatementKind,
    SwitchStatement, TryCatchStatement,
};
use crate::core::{BinaryOp, ClassMember, DestructuringElement, Expr, ObjectDestructuringElement, TemplatePart, Token, TokenData};
use std::sync::atomic::{AtomicU64, Ordering};
static TEMPLATE_SITE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);
fn next_template_site_id() -> u64 {
    TEMPLATE_SITE_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}
use crate::{raise_parse_error, raise_parse_error_at, raise_parse_error_with_token, raise_syntax_error, unicode::utf16_to_utf8};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
};
fn is_lexical_declaration(stmt: &Statement) -> bool {
    matches!(
        &*stmt.kind,
        StatementKind::Let(..)
            | StatementKind::Const(..)
            | StatementKind::LetDestructuringArray(..)
            | StatementKind::LetDestructuringObject(..)
            | StatementKind::ConstDestructuringArray(..)
            | StatementKind::ConstDestructuringObject(..)
            | StatementKind::Class(..)
            | StatementKind::Using(..)
            | StatementKind::AwaitUsing(..)
    )
}
fn reject_lexical_in_single_statement(stmt: &Statement, context: &str) -> Result<(), JSError> {
    if is_lexical_declaration(stmt) {
        return Err(raise_parse_error!(
            &format!("Lexical declaration not allowed in {} body", context),
            stmt.line,
            stmt.column
        ));
    }
    Ok(())
}
pub fn parse_statements(t: &[TokenData], index: &mut usize) -> Result<Vec<Statement>, JSError> {
    push_statement_depth();
    let is_module_top = in_module_context() && statement_depth() == 1;
    let mut statements = Vec::new();
    while *index < t.len() && t[*index].token != Token::EOF && t[*index].token != Token::RBrace {
        if matches!(t[*index].token, Token::Semicolon | Token::LineTerminator) {
            *index += 1;
            continue;
        }
        let stmt = parse_statement_item(t, index)?;
        if is_module_top {
            track_module_level_names(&stmt)?;
        }
        statements.push(stmt);
    }
    pop_statement_depth();
    Ok(statements)
}
/// Parse a single statement in a nested context (loop body, if body, etc.)
/// where import/export declarations are not allowed.
fn parse_nested_statement_item(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    push_statement_depth();
    let result = parse_statement_item(t, index);
    pop_statement_depth();
    result
}
fn parse_statement_item(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    log::trace!("parse_statement_item: starting at index {} token={:?}", *index, t.get(*index));
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
        return Ok(Statement {
            kind: Box::new(StatementKind::Expr(crate::core::Expr::ValuePlaceholder)),
            line: t[*index - 1].line,
            column: t[*index - 1].column,
        });
    }
    if *index >= t.len() {
        return Err(raise_parse_error_with_token!(t.last().unwrap(), "Unexpected end of input"));
    }
    let start_token = &t[*index];
    let line = start_token.line;
    let column = start_token.column;
    match start_token.token {
        Token::Import if !matches!(t.get(*index + 1).map(|d| &d.token), Some(Token::LParen) | Some(Token::Dot)) => {
            if !in_module_context() {
                return Err(raise_parse_error_with_token!(
                    t[*index],
                    "Cannot use import statement outside a module"
                ));
            }
            if statement_depth() > 1 {
                return Err(raise_parse_error_with_token!(
                    t[*index],
                    "import declarations may only appear at top level of a module"
                ));
            }
            parse_import_statement(t, index)
        }
        Token::Export => {
            if !in_module_context() {
                return Err(raise_parse_error_with_token!(
                    t[*index],
                    "Cannot use export statement outside a module"
                ));
            }
            if statement_depth() > 1 {
                return Err(raise_parse_error_with_token!(
                    t[*index],
                    "export declarations may only appear at top level of a module"
                ));
            }
            parse_export_statement(t, index)
        }
        Token::Function | Token::FunctionStar => parse_function_declaration(t, index),
        Token::Class => parse_class_declaration(t, index),
        Token::If => parse_if_statement(t, index),
        Token::Return => parse_return_statement(t, index),
        Token::Throw => parse_throw_statement(t, index),
        Token::Break => parse_break_statement(t, index),
        Token::Continue => parse_continue_statement(t, index),
        Token::Try => parse_try_statement(t, index),
        Token::LBrace => parse_block_statement(t, index),
        Token::Var => parse_var_statement(t, index),
        Token::Let => parse_let_statement(t, index),
        Token::Const => parse_const_statement(t, index),
        Token::For => parse_for_statement(t, index),
        Token::While => parse_while_statement(t, index),
        Token::Do => parse_do_while_statement(t, index),
        Token::Switch => parse_switch_statement(t, index),
        Token::Async => {
            if raw_identifier_source_has_escape(start_token) {
                let expr = parse_expression(t, index)?;
                finish_statement_without_semicolon(t, index)?;
                Ok(Statement {
                    kind: Box::new(StatementKind::Expr(expr)),
                    line,
                    column,
                })
            } else if *index + 1 < t.len() && matches!(t[*index + 1].token, Token::Function | Token::FunctionStar) {
                parse_function_declaration(t, index)
            } else {
                let expr = parse_expression(t, index)?;
                finish_statement_without_semicolon(t, index)?;
                Ok(Statement {
                    kind: Box::new(StatementKind::Expr(expr)),
                    line,
                    column,
                })
            }
        }
        Token::With => parse_with_statement(t, index),
        Token::Debugger => {
            *index += 1;
            if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
                *index += 1;
            }
            Ok(Statement {
                kind: Box::new(StatementKind::Debugger),
                line,
                column,
            })
        }
        _ => {
            if let Token::Identifier(ref name) = start_token.token
                && name == "using"
                && *index + 1 < t.len()
                && (matches!(t[*index + 1].token, Token::Identifier(_))
                    || (matches!(t[*index + 1].token, Token::Await) && !in_await_context() && !forbid_await_identifier())
                    || (matches!(t[*index + 1].token, Token::Yield) && !in_generator_context()))
            {
                return parse_using_statement(t, index);
            }
            if matches!(start_token.token, Token::Await)
                && *index + 1 < t.len()
                && matches!(& t[* index + 1].token, Token::Identifier(n) if n == "using")
                && *index + 2 < t.len()
                && matches!(t[*index + 2].token, Token::Identifier(_))
            {
                return parse_await_using_statement(t, index);
            }
            let label_name_opt = match &start_token.token {
                Token::Identifier(name) => Some(name.clone()),
                Token::Await => Some("await".to_string()),
                Token::Yield => Some("yield".to_string()),
                _ => None,
            };
            if let Some(label_name) = label_name_opt
                && *index + 1 < t.len()
                && matches!(t[*index + 1].token, Token::Colon)
            {
                // Escaped reserved words cannot be used as labels
                // e.g. nul\u006c: , f\u0061lse: , tru\u0065:
                if is_always_reserved_word(&label_name) {
                    return Err(raise_parse_error_with_token!(
                        t[*index],
                        format!("SyntaxError: '{}' is not allowed as a label", label_name)
                    ));
                }
                // await cannot be a label in module code or static blocks
                if label_name == "await" && forbid_await_identifier() {
                    return Err(raise_parse_error_with_token!(
                        t[*index],
                        "'await' cannot be used as a label in this context"
                    ));
                }
                // yield cannot be a label in generator context or strict mode
                if label_name == "yield" {
                    return Err(raise_parse_error_with_token!(
                        t[*index],
                        "'yield' cannot be used as a label in strict mode"
                    ));
                }
                *index += 2;
                // Duplicate label check
                if has_active_label(&label_name) {
                    return Err(raise_parse_error_with_token!(
                        t[*index - 2],
                        format!("Label '{}' has already been declared", label_name)
                    ));
                }
                push_label(&label_name);
                let stmt = parse_nested_statement_item(t, index);
                pop_label();
                let stmt = stmt?;
                // Labeled statements: only FunctionDeclaration is allowed as
                // a labeled item, not lexical (let/const/class) declarations
                if is_lexical_declaration(&stmt) {
                    return Err(raise_parse_error!(
                        "Lexical declaration (let/const/class) not allowed as labeled statement",
                        stmt.line,
                        stmt.column
                    ));
                }
                return Ok(Statement {
                    kind: Box::new(StatementKind::Label(label_name, Box::new(stmt))),
                    line,
                    column,
                });
            }
            let expr = parse_expression(t, index)?;
            finish_statement_without_semicolon(t, index)?;
            Ok(Statement {
                kind: Box::new(StatementKind::Expr(expr)),
                line,
                column,
            })
        }
    }
}
thread_local! {
    static AWAIT_CONTEXT : RefCell < usize > = const { RefCell::new(0) };
    /// When true (default), the parser rejects `eval` / `arguments` as binding
    /// names and assignment targets (strict-mode restriction).  Cleared for
    /// indirect-eval and `Function()` constructor code that runs in sloppy mode.
    static STRICT_BINDING_CHECKS : Cell < bool > = const { Cell::new(true) };
    static FORBID_AWAIT_IDENTIFIER: RefCell<usize> = const { RefCell::new(0) };
    static PARSE_SOURCE_STACK: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static FUNCTION_CONTEXT: RefCell<usize> = const { RefCell::new(0) };
    static FORBID_IN: RefCell<usize> = const { RefCell::new(0) };
    static GENERATOR_CONTEXT: RefCell<usize> = const { RefCell::new(0) };
    static GENERATOR_CONTEXT_STACK: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    /// >0 when parsing inside a method body (class/object method, getter, setter).
    /// super.x / super[x] are allowed when this is >0.
    static METHOD_CONTEXT: RefCell<usize> = const { RefCell::new(0) };
    static METHOD_CONTEXT_STACK: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    /// >0 when parsing inside a constructor body (derived class).
    /// super() is allowed when this is >0.
    static CONSTRUCTOR_CONTEXT: RefCell<usize> = const { RefCell::new(0) };
    static CONSTRUCTOR_CONTEXT_STACK: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    /// Whether the current class being parsed has an extends clause (heritage).
    /// Stacked to support nested classes.
    static CLASS_HAS_HERITAGE: RefCell<bool> = const { RefCell::new(false) };
    static CLASS_HAS_HERITAGE_STACK: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
    /// >0 when inside a non-arrow function (where new.target is valid).
    static NEW_TARGET_CONTEXT: RefCell<usize> = const { RefCell::new(0) };
    /// >0 when inside a class static block (where `arguments` is forbidden).
    static STATIC_BLOCK_CONTEXT: RefCell<usize> = const { RefCell::new(0) };
    static STATIC_BLOCK_CONTEXT_STACK: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    /// Whether parsing in module mode (export/import declarations allowed).
    static MODULE_CONTEXT: Cell<bool> = const { Cell::new(false) };
    /// Nesting depth for statements. 0 = top-level.
    /// import/export declarations are only valid at depth 0 in module mode.
    static STATEMENT_DEPTH: Cell<usize> = const { Cell::new(0) };
    /// Stack of active label names for duplicate label detection.
    static LABEL_STACK: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Exported names in module code, for duplicate export detection.
    static EXPORTED_NAMES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Module-level lexical names (function declarations, class declarations, let/const).
    static MODULE_LEXICAL_NAMES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Module-level var-declared names.
    static MODULE_VAR_NAMES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// When true, arrow-function detection in the LParen branch of parse_primary
    /// is suppressed (for ClassHeritage: extends LeftHandSideExpression).
    static NO_ARROW_IN_PAREN: Cell<bool> = const { Cell::new(false) };
}
fn forbid_in() -> bool {
    FORBID_IN.with(|c| *c.borrow() > 0)
}
fn with_forbidden_in<T, F: FnOnce() -> T>(f: F) -> T {
    FORBID_IN.with(|c| *c.borrow_mut() += 1);
    let out = f();
    FORBID_IN.with(|c| *c.borrow_mut() -= 1);
    out
}
fn with_allowed_in<T, F: FnOnce() -> T>(f: F) -> T {
    let saved = FORBID_IN.with(|c| {
        let prev = *c.borrow();
        *c.borrow_mut() = 0;
        prev
    });
    let out = f();
    FORBID_IN.with(|c| *c.borrow_mut() = saved);
    out
}
fn in_generator_context() -> bool {
    GENERATOR_CONTEXT.with(|c| *c.borrow() > 0)
}
fn push_generator_context() {
    GENERATOR_CONTEXT.with(|c| *c.borrow_mut() += 1);
}
fn pop_generator_context() {
    GENERATOR_CONTEXT.with(|c| *c.borrow_mut() -= 1);
}
fn in_method_context() -> bool {
    METHOD_CONTEXT.with(|c| *c.borrow() > 0)
}
fn in_constructor_context() -> bool {
    CONSTRUCTOR_CONTEXT.with(|c| *c.borrow() > 0)
}
fn push_method_context() {
    METHOD_CONTEXT.with(|c| *c.borrow_mut() += 1);
}
fn pop_method_context() {
    METHOD_CONTEXT.with(|c| *c.borrow_mut() -= 1);
}
fn push_constructor_context() {
    CONSTRUCTOR_CONTEXT.with(|c| *c.borrow_mut() += 1);
}
fn pop_constructor_context() {
    CONSTRUCTOR_CONTEXT.with(|c| *c.borrow_mut() -= 1);
}
fn class_has_heritage() -> bool {
    CLASS_HAS_HERITAGE.with(|c| *c.borrow())
}
fn push_class_heritage(has: bool) {
    CLASS_HAS_HERITAGE_STACK.with(|s| {
        s.borrow_mut().push(CLASS_HAS_HERITAGE.with(|c| *c.borrow()));
    });
    CLASS_HAS_HERITAGE.with(|c| *c.borrow_mut() = has);
}
fn pop_class_heritage() {
    if let Some(prev) = CLASS_HAS_HERITAGE_STACK.with(|s| s.borrow_mut().pop()) {
        CLASS_HAS_HERITAGE.with(|c| *c.borrow_mut() = prev);
    }
}
fn in_function_context() -> bool {
    FUNCTION_CONTEXT.with(|c| *c.borrow() > 0)
}
/// Every function boundary saves+clears generator/method/constructor context automatically.
fn push_function_context() {
    FUNCTION_CONTEXT.with(|c| *c.borrow_mut() += 1);
    NEW_TARGET_CONTEXT.with(|c| *c.borrow_mut() += 1);
    let saved_gen = GENERATOR_CONTEXT.with(|c| {
        let prev = *c.borrow();
        *c.borrow_mut() = 0;
        prev
    });
    GENERATOR_CONTEXT_STACK.with(|s| s.borrow_mut().push(saved_gen));
    let saved_method = METHOD_CONTEXT.with(|c| {
        let prev = *c.borrow();
        *c.borrow_mut() = 0;
        prev
    });
    METHOD_CONTEXT_STACK.with(|s| s.borrow_mut().push(saved_method));
    let saved_ctor = CONSTRUCTOR_CONTEXT.with(|c| {
        let prev = *c.borrow();
        *c.borrow_mut() = 0;
        prev
    });
    CONSTRUCTOR_CONTEXT_STACK.with(|s| s.borrow_mut().push(saved_ctor));
    // Functions have their own `arguments`, so clear static block context
    let saved_sb = STATIC_BLOCK_CONTEXT.with(|c| {
        let prev = *c.borrow();
        *c.borrow_mut() = 0;
        prev
    });
    STATIC_BLOCK_CONTEXT_STACK.with(|s| s.borrow_mut().push(saved_sb));
}
fn pop_function_context() {
    FUNCTION_CONTEXT.with(|c| *c.borrow_mut() -= 1);
    NEW_TARGET_CONTEXT.with(|c| *c.borrow_mut() -= 1);
    let saved_gen = GENERATOR_CONTEXT_STACK.with(|s| s.borrow_mut().pop().unwrap_or(0));
    GENERATOR_CONTEXT.with(|c| *c.borrow_mut() = saved_gen);
    let saved_method = METHOD_CONTEXT_STACK.with(|s| s.borrow_mut().pop().unwrap_or(0));
    METHOD_CONTEXT.with(|c| *c.borrow_mut() = saved_method);
    let saved_ctor = CONSTRUCTOR_CONTEXT_STACK.with(|s| s.borrow_mut().pop().unwrap_or(0));
    CONSTRUCTOR_CONTEXT.with(|c| *c.borrow_mut() = saved_ctor);
    let saved_sb = STATIC_BLOCK_CONTEXT_STACK.with(|s| s.borrow_mut().pop().unwrap_or(0));
    STATIC_BLOCK_CONTEXT.with(|c| *c.borrow_mut() = saved_sb);
}
/// Arrow functions inherit super/method/constructor context but NOT generator context
fn push_arrow_function_context() {
    FUNCTION_CONTEXT.with(|c| *c.borrow_mut() += 1);
    let saved_gen = GENERATOR_CONTEXT.with(|c| {
        let prev = *c.borrow();
        *c.borrow_mut() = 0;
        prev
    });
    GENERATOR_CONTEXT_STACK.with(|s| s.borrow_mut().push(saved_gen));
}
fn pop_arrow_function_context() {
    FUNCTION_CONTEXT.with(|c| *c.borrow_mut() -= 1);
    let saved_gen = GENERATOR_CONTEXT_STACK.with(|s| s.borrow_mut().pop().unwrap_or(0));
    GENERATOR_CONTEXT.with(|c| *c.borrow_mut() = saved_gen);
}
pub(crate) fn in_await_context() -> bool {
    AWAIT_CONTEXT.with(|c| *c.borrow() > 0)
}
pub(crate) fn push_await_context() {
    AWAIT_CONTEXT.with(|c| *c.borrow_mut() += 1);
}
pub(crate) fn pop_await_context() {
    AWAIT_CONTEXT.with(|c| *c.borrow_mut() -= 1);
}
pub(crate) fn push_method_context_for_eval() {
    METHOD_CONTEXT.with(|c| *c.borrow_mut() += 1);
}
pub(crate) fn pop_method_context_for_eval() {
    METHOD_CONTEXT.with(|c| *c.borrow_mut() -= 1);
}
pub(crate) fn push_constructor_context_for_eval() {
    CONSTRUCTOR_CONTEXT.with(|c| *c.borrow_mut() += 1);
}
pub(crate) fn pop_constructor_context_for_eval() {
    CONSTRUCTOR_CONTEXT.with(|c| *c.borrow_mut() -= 1);
}
pub(crate) fn push_new_target_context_for_eval() {
    NEW_TARGET_CONTEXT.with(|c| *c.borrow_mut() += 1);
}
pub(crate) fn pop_new_target_context_for_eval() {
    NEW_TARGET_CONTEXT.with(|c| *c.borrow_mut() -= 1);
}
fn strict_binding_checks() -> bool {
    STRICT_BINDING_CHECKS.with(|c| c.get())
}

/// Returns true if `name` is a reserved keyword or strict-mode future reserved word
/// that must not be used as a binding identifier.
/// This catches escaped reserved words (e.g. `\u{62}reak` → "break") and
/// strict-mode-only future reserved words (implements, interface, etc.).
fn is_reserved_identifier(name: &str) -> bool {
    if is_always_reserved_word(name) {
        return true;
    }
    // Strict-mode future reserved words — only reject when strict binding checks are active
    if strict_binding_checks() && is_strict_reserved_word(name) {
        return true;
    }
    false
}

fn forbid_await_identifier() -> bool {
    FORBID_AWAIT_IDENTIFIER.with(|c| *c.borrow() > 0)
}

pub(crate) fn in_module_context() -> bool {
    MODULE_CONTEXT.with(|c| c.get())
}
pub(crate) fn set_module_context(v: bool) {
    MODULE_CONTEXT.with(|c| c.set(v));
}
fn statement_depth() -> usize {
    STATEMENT_DEPTH.with(|c| c.get())
}
fn push_statement_depth() {
    STATEMENT_DEPTH.with(|c| c.set(c.get() + 1));
}
fn pop_statement_depth() {
    STATEMENT_DEPTH.with(|c| c.set(c.get() - 1));
}
fn has_active_label(name: &str) -> bool {
    LABEL_STACK.with(|c| c.borrow().iter().any(|l| l == name))
}
fn push_label(name: &str) {
    LABEL_STACK.with(|c| c.borrow_mut().push(name.to_string()));
}
fn pop_label() {
    LABEL_STACK.with(|c| c.borrow_mut().pop());
}
pub(crate) fn reset_module_tracking() {
    EXPORTED_NAMES.with(|c| c.borrow_mut().clear());
    MODULE_LEXICAL_NAMES.with(|c| c.borrow_mut().clear());
    MODULE_VAR_NAMES.with(|c| c.borrow_mut().clear());
}
fn add_exported_name(name: &str) -> Result<(), JSError> {
    EXPORTED_NAMES.with(|c| {
        let mut names = c.borrow_mut();
        if names.iter().any(|n| n == name) {
            Err(raise_syntax_error!(format!("Duplicate export name '{}'", name)))
        } else {
            names.push(name.to_string());
            Ok(())
        }
    })
}
fn add_module_lexical_name(name: &str) -> Result<(), JSError> {
    MODULE_LEXICAL_NAMES.with(|c| {
        let mut lex = c.borrow_mut();
        if lex.iter().any(|n| n == name) {
            return Err(raise_syntax_error!(format!("Identifier '{}' has already been declared", name)));
        }
        // Also check against var names
        let conflict = MODULE_VAR_NAMES.with(|v| v.borrow().iter().any(|n| n == name));
        if conflict {
            return Err(raise_syntax_error!(format!("Identifier '{}' has already been declared", name)));
        }
        lex.push(name.to_string());
        Ok(())
    })
}
fn add_module_var_name(name: &str) -> Result<(), JSError> {
    MODULE_VAR_NAMES.with(|c| {
        // Check against lexical names
        let conflict = MODULE_LEXICAL_NAMES.with(|l| l.borrow().iter().any(|n| n == name));
        if conflict {
            return Err(raise_syntax_error!(format!("Identifier '{}' has already been declared", name)));
        }
        let mut vars = c.borrow_mut();
        if !vars.iter().any(|n| n == name) {
            vars.push(name.to_string());
        }
        Ok(())
    })
}

fn with_forbidden_await_identifier<T, F: FnOnce() -> T>(f: F) -> T {
    FORBID_AWAIT_IDENTIFIER.with(|c| *c.borrow_mut() += 1);
    let out = f();
    FORBID_AWAIT_IDENTIFIER.with(|c| *c.borrow_mut() -= 1);
    out
}
pub fn with_forbidden_await_identifier_pub<T, F: FnOnce() -> T>(f: F) -> T {
    with_forbidden_await_identifier(f)
}
/// Clear FORBID_AWAIT_IDENTIFIER when crossing a function boundary
/// (function expressions, method bodies) so that `await` is valid as identifier inside.
fn with_cleared_forbidden_await_identifier<T, F: FnOnce() -> T>(f: F) -> T {
    // In module code, `await` is always a reserved word, so never clear the restriction.
    if in_module_context() {
        return f();
    }
    let saved = FORBID_AWAIT_IDENTIFIER.with(|c| {
        let prev = *c.borrow();
        *c.borrow_mut() = 0;
        prev
    });
    let out = f();
    FORBID_AWAIT_IDENTIFIER.with(|c| *c.borrow_mut() = saved);
    out
}
/// Temporarily disable strict-mode binding checks (for indirect eval / Function ctor).
pub(crate) fn parse_without_strict_binding_checks<T, F: FnOnce() -> T>(f: F) -> T {
    STRICT_BINDING_CHECKS.with(|c| {
        let prev = c.get();
        c.set(false);
        let out = f();
        c.set(prev);
        out
    })
}
pub(crate) fn with_parse_source<T, F: FnOnce() -> T>(source: &str, f: F) -> T {
    PARSE_SOURCE_STACK.with(|stack| stack.borrow_mut().push(source.to_string()));
    let out = f();
    PARSE_SOURCE_STACK.with(|stack| {
        stack.borrow_mut().pop();
    });
    out
}
/// Convert any token to its IdentifierName string, or empty string if not an identifier-like token.
fn token_to_identifier_name(td: &TokenData) -> String {
    match &td.token {
        Token::Identifier(s) => s.clone(),
        Token::Async => "async".into(),
        Token::Await => "await".into(),
        Token::As => "as".into(),
        Token::Break => "break".into(),
        Token::Case => "case".into(),
        Token::Catch => "catch".into(),
        Token::Class => "class".into(),
        Token::Const => "const".into(),
        Token::Continue => "continue".into(),
        Token::Debugger => "debugger".into(),
        Token::Default => "default".into(),
        Token::Delete => "delete".into(),
        Token::Do => "do".into(),
        Token::Else => "else".into(),
        Token::Export => "export".into(),
        Token::Extends => "extends".into(),
        Token::False => "false".into(),
        Token::Finally => "finally".into(),
        Token::For => "for".into(),
        Token::Function | Token::FunctionStar => "function".into(),
        Token::If => "if".into(),
        Token::Import => "import".into(),
        Token::In => "in".into(),
        Token::InstanceOf => "instanceof".into(),
        Token::Let => "let".into(),
        Token::New => "new".into(),
        Token::Null => "null".into(),
        Token::Return => "return".into(),
        Token::Static => "static".into(),
        Token::Super => "super".into(),
        Token::Switch => "switch".into(),
        Token::This => "this".into(),
        Token::Throw => "throw".into(),
        Token::True => "true".into(),
        Token::Try => "try".into(),
        Token::TypeOf => "typeof".into(),
        Token::Var => "var".into(),
        Token::Void => "void".into(),
        Token::While => "while".into(),
        Token::With => "with".into(),
        Token::Yield | Token::YieldStar => "yield".into(),
        _ => String::new(),
    }
}
fn raw_identifier_source_has_escape(token: &TokenData) -> bool {
    PARSE_SOURCE_STACK.with(|stack| {
        let stack = stack.borrow();
        let Some(source) = stack.last() else {
            return false;
        };
        let Some(rest) = source.get(token.byte_offset..) else {
            return false;
        };
        let mut saw_start = false;
        for ch in rest.chars() {
            if !saw_start {
                saw_start = true;
                if ch == '\\' {
                    return true;
                }
                continue;
            }
            if ch == '\\' {
                return true;
            }
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$' | '#' | '{' | '}') {
                continue;
            }
            break;
        }
        false
    })
}
fn token_matches_unescaped_identifier_name(token: &TokenData, expected: &str) -> bool {
    token.token.as_identifier_string().as_deref() == Some(expected) && !raw_identifier_source_has_escape(token)
}
fn token_is_escaped_identifier_name(token: &TokenData, expected: &str) -> bool {
    token.token.as_identifier_string().as_deref() == Some(expected) && raw_identifier_source_has_escape(token)
}
/// Returns true if the given name is a reserved word that can NEVER be an identifier.
/// These correspond to keywords that normally have their own Token variants;
/// if they appear as Token::Identifier it means they were Unicode-escaped.
fn is_always_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "null"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
    )
}
/// Returns true if the given name is a strict-mode reserved word.
fn is_strict_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "implements" | "interface" | "let" | "package" | "private" | "protected" | "public" | "static" | "yield"
    )
}
fn finish_statement_without_semicolon(t: &[TokenData], index: &mut usize) -> Result<(), JSError> {
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
        return Ok(());
    }
    if *index < t.len() && !matches!(t[*index].token, Token::LineTerminator | Token::RBrace | Token::EOF) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    Ok(())
}
fn with_cleared_await_context<T, F: FnOnce() -> T>(f: F) -> T {
    AWAIT_CONTEXT.with(|c| {
        let prev = *c.borrow();
        *c.borrow_mut() = 0;
        let out = f();
        *c.borrow_mut() = prev;
        out
    })
}
fn parse_class_declaration(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    let name = if *index < t.len() {
        match &t[*index].token {
            Token::Identifier(name) => {
                // All parts of a ClassDeclaration are strict mode code.
                // Strict reserved words cannot be used as class names.
                if is_strict_reserved_word(name) || is_always_reserved_word(name) {
                    let msg = format!("'{}' is not allowed as a class name in strict mode", name);
                    return Err(raise_parse_error_with_token!(t[*index], msg));
                }
                if name == "await" && forbid_await_identifier() {
                    return Err(raise_parse_error_with_token!(t[*index], "Cannot use 'await' as class name"));
                }
                let n = name.clone();
                *index += 1;
                n
            }
            Token::Await => {
                if forbid_await_identifier() {
                    return Err(raise_parse_error!("SyntaxError: Cannot use 'await' as class name"));
                }
                *index += 1;
                "await".to_string()
            }
            Token::Async => {
                *index += 1;
                "async".to_string()
            }
            _ => return Err(raise_parse_error_at!(t.get(*index))),
        }
    } else {
        return Err(raise_parse_error_at!(t.get(*index)));
    };
    let extends = if *index < t.len() && matches!(t[*index].token, Token::Extends) {
        *index += 1;
        NO_ARROW_IN_PAREN.with(|c| c.set(true));
        let heritage = parse_assignment(t, index);
        NO_ARROW_IN_PAREN.with(|c| c.set(false));
        Some(heritage?)
    } else {
        None
    };
    push_class_heritage(extends.is_some());
    let members = parse_class_body(t, index)?;
    pop_class_heritage();
    let class_def = crate::core::ClassDefinition { name, extends, members };
    Ok(Statement {
        kind: Box::new(StatementKind::Class(Box::new(class_def))),
        line: t[start].line,
        column: t[start].column,
    })
}
/// Collect names from var declarations inside a statement list (non-recursive into functions/classes).
fn collect_var_declared_names(stmts: &[Statement], out: &mut Vec<String>) {
    for s in stmts {
        match &*s.kind {
            StatementKind::Var(decls) => {
                for (name, _) in decls {
                    out.push(name.clone());
                }
            }
            StatementKind::Block(inner) => collect_var_declared_names(inner, out),
            StatementKind::If(if_stmt) => {
                collect_var_declared_names(&if_stmt.then_body, out);
                if let Some(ref else_body) = if_stmt.else_body {
                    collect_var_declared_names(else_body, out);
                }
            }
            StatementKind::For(f) => {
                collect_var_declared_names(&f.body, out);
            }
            StatementKind::ForIn(_, _, _, body)
            | StatementKind::ForOf(_, _, _, body)
            | StatementKind::ForAwaitOf(_, _, _, body)
            | StatementKind::ForInExpr(_, _, body)
            | StatementKind::ForOfExpr(_, _, body)
            | StatementKind::ForAwaitOfExpr(_, _, body)
            | StatementKind::ForInDestructuringObject(_, _, _, body)
            | StatementKind::ForInDestructuringArray(_, _, _, body)
            | StatementKind::ForOfDestructuringObject(_, _, _, body)
            | StatementKind::ForOfDestructuringArray(_, _, _, body)
            | StatementKind::ForAwaitOfDestructuringObject(_, _, _, body)
            | StatementKind::ForAwaitOfDestructuringArray(_, _, _, body) => {
                collect_var_declared_names(body, out);
            }
            StatementKind::While(_, body) => {
                collect_var_declared_names(body, out);
            }
            StatementKind::DoWhile(body, _) => {
                collect_var_declared_names(body, out);
            }
            StatementKind::Switch(sw) => {
                for case in &sw.cases {
                    match case {
                        crate::core::SwitchCase::Case(_, body) | crate::core::SwitchCase::Default(body) => {
                            collect_var_declared_names(body, out);
                        }
                    }
                }
            }
            StatementKind::TryCatch(tc) => {
                collect_var_declared_names(&tc.try_body, out);
                if let Some(ref cb) = tc.catch_body {
                    collect_var_declared_names(cb, out);
                }
                if let Some(ref fb) = tc.finally_body {
                    collect_var_declared_names(fb, out);
                }
            }
            StatementKind::Label(_, inner) => {
                collect_var_declared_names(std::slice::from_ref(inner.as_ref()), out);
            }
            StatementKind::With(_, body) => {
                collect_var_declared_names(body, out);
            }
            // Function/class declarations do NOT contribute var names to the enclosing scope
            _ => {}
        }
    }
}

/// Check if for-head lexical bindings conflict with var declarations in the body.
fn check_for_head_body_var_conflict(init: &Option<Box<Statement>>, body: &[Statement], line: usize, col: usize) -> Result<(), JSError> {
    if let Some(init_stmt) = init {
        let head_names: Vec<String> = match &*init_stmt.kind {
            StatementKind::Let(decls) => decls.iter().map(|(name, _)| name.clone()).collect(),
            StatementKind::Const(decls) => decls.iter().map(|(name, _)| name.clone()).collect(),
            _ => return Ok(()),
        };
        if head_names.is_empty() {
            return Ok(());
        }
        let mut var_names = Vec::new();
        collect_var_declared_names(body, &mut var_names);
        for vn in &var_names {
            if head_names.contains(vn) {
                return Err(raise_parse_error!(
                    format!("SyntaxError: Identifier '{}' has already been declared", vn),
                    line,
                    col
                ));
            }
        }
    }
    Ok(())
}

/// Collect all BoundNames from a destructuring pattern.
fn collect_destructuring_bound_names(pattern: &[DestructuringElement], out: &mut Vec<String>) {
    for elem in pattern {
        match elem {
            DestructuringElement::Variable(name, _) => out.push(name.clone()),
            DestructuringElement::Rest(name) => out.push(name.clone()),
            DestructuringElement::Property(_, inner) => collect_destructuring_bound_names(std::slice::from_ref(inner.as_ref()), out),
            DestructuringElement::ComputedProperty(_, inner) => {
                collect_destructuring_bound_names(std::slice::from_ref(inner.as_ref()), out)
            }
            DestructuringElement::NestedArray(arr, _) => collect_destructuring_bound_names(arr, out),
            DestructuringElement::NestedObject(obj, _) => collect_destructuring_bound_names(obj, out),
            _ => {}
        }
    }
}

/// Check for duplicate BoundNames in a destructuring pattern used in for-in/for-of head.
fn check_for_head_dup_bound_names(pattern: &[DestructuringElement], line: usize, col: usize) -> Result<(), JSError> {
    let mut names = Vec::new();
    collect_destructuring_bound_names(pattern, &mut names);
    for (i, name) in names.iter().enumerate() {
        if names[..i].contains(name) {
            return Err(raise_parse_error!(
                format!("SyntaxError: Duplicate binding '{}' in for-in/for-of head", name),
                line,
                col
            ));
        }
    }
    Ok(())
}

/// Check for-in/for-of head let/const names vs var declarations in body.
fn check_forinof_head_body_var_conflict(head_names: &[String], body: &[Statement], line: usize, col: usize) -> Result<(), JSError> {
    if head_names.is_empty() {
        return Ok(());
    }
    let mut var_names = Vec::new();
    collect_var_declared_names(body, &mut var_names);
    for vn in &var_names {
        if head_names.contains(vn) {
            return Err(raise_parse_error!(
                format!("SyntaxError: Identifier '{}' has already been declared", vn),
                line,
                col
            ));
        }
    }
    Ok(())
}

fn parse_for_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    let line = t[start].line;
    let column = t[start].column;
    *index += 1;
    let mut is_for_await = false;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < t.len() && matches!(t[*index].token, Token::Await) {
        is_for_await = true;
        *index += 1;
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
    }
    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    {
        let is_await_using_in_parens = matches!(t[*index].token, Token::Await) && {
            let mut pk = *index + 1;
            while pk < t.len() && matches!(t[pk].token, Token::LineTerminator) {
                pk += 1;
            }
            matches!(& t[pk].token, Token::Identifier(n) if n == "using")
        };
        if is_await_using_in_parens {
            *index += 1;
            while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                *index += 1;
            }
            is_for_await = true;
        }
        let is_using_kw = matches!(
            & t[* index].token, Token::Identifier(n) if n == "using"
        );
        if is_using_kw {
            let mut peek = *index + 1;
            while peek < t.len() && matches!(t[peek].token, Token::LineTerminator) {
                peek += 1;
            }
            let next_is_ident = peek < t.len() && matches!(&t[peek].token, Token::Identifier(_));
            let is_using_of = next_is_ident && matches!(& t[peek].token, Token::Identifier(n) if n == "of");
            let using_of_is_decl = if is_using_of {
                let mut peek2 = peek + 1;
                while peek2 < t.len() && matches!(t[peek2].token, Token::LineTerminator) {
                    peek2 += 1;
                }
                peek2 < t.len() && matches!(t[peek2].token, Token::Assign)
            } else {
                false
            };
            let enter_using_path = next_is_ident && (is_await_using_in_parens || !is_using_of || using_of_is_decl);
            if enter_using_path {
                *index += 1;
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                let first_name = match &t[*index].token {
                    Token::Identifier(n) => n.clone(),
                    _ => return Err(raise_parse_error_at!(t.get(*index))),
                };
                *index += 1;
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                if *index < t.len() && matches!(& t[* index].token, Token::Identifier(n) if n == "of") {
                    if raw_identifier_source_has_escape(&t[*index]) {
                        return Err(raise_parse_error_with_token!(
                            t[*index],
                            "'of' keyword must not contain Unicode escape sequences"
                        ));
                    }
                    *index += 1;
                    let iterable = parse_assignment(t, index)?;
                    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                        *index += 1;
                    }
                    if !matches!(t[*index].token, Token::RParen) {
                        return Err(raise_parse_error_at!(t.get(*index)));
                    }
                    *index += 1;
                    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                        *index += 1;
                    }
                    let body = parse_nested_statement_item(t, index)?;
                    reject_lexical_in_single_statement(&body, "for")?;
                    let body_stmts = vec![body];
                    // using/await using head names conflict with var in body
                    let head_names = vec![first_name.clone()];
                    check_forinof_head_body_var_conflict(&head_names, &body_stmts, line, column)?;
                    let kind = if is_for_await {
                        StatementKind::ForAwaitOf(Some(crate::core::VarDeclKind::AwaitUsing), first_name, iterable, body_stmts)
                    } else {
                        StatementKind::ForOf(Some(crate::core::VarDeclKind::Using), first_name, iterable, body_stmts)
                    };
                    return Ok(Statement {
                        kind: Box::new(kind),
                        line,
                        column,
                    });
                }
                if !matches!(t[*index].token, Token::Assign) {
                    return Err(raise_parse_error!("using declarations must have an initializer", line, column));
                }
                *index += 1;
                let first_init = parse_assignment(t, index)?;
                let mut using_decls = vec![(first_name, first_init)];
                while *index < t.len() && matches!(t[*index].token, Token::Comma) {
                    *index += 1;
                    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                        *index += 1;
                    }
                    let next_name = match &t[*index].token {
                        Token::Identifier(n) => n.clone(),
                        _ => return Err(raise_parse_error_at!(t.get(*index))),
                    };
                    *index += 1;
                    if !matches!(t[*index].token, Token::Assign) {
                        return Err(raise_parse_error!("using declarations must have an initializer", line, column));
                    }
                    *index += 1;
                    let next_init = parse_assignment(t, index)?;
                    using_decls.push((next_name, next_init));
                }
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                if !matches!(t[*index].token, Token::Semicolon) {
                    return Err(raise_parse_error_at!(t.get(*index)));
                }
                *index += 1;
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                let test = if !matches!(t[*index].token, Token::Semicolon) {
                    Some(parse_expression(t, index)?)
                } else {
                    None
                };
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                if !matches!(t[*index].token, Token::Semicolon) {
                    return Err(raise_parse_error_at!(t.get(*index)));
                }
                *index += 1;
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                let update = if !matches!(t[*index].token, Token::RParen) {
                    Some(parse_expression(t, index)?)
                } else {
                    None
                };
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                if !matches!(t[*index].token, Token::RParen) {
                    return Err(raise_parse_error_at!(t.get(*index)));
                }
                *index += 1;
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                let body = parse_nested_statement_item(t, index)?;
                reject_lexical_in_single_statement(&body, "for")?;
                let body_stmts = vec![body];
                let init_stmt = Some(Box::new(Statement {
                    kind: Box::new(if is_for_await {
                        StatementKind::AwaitUsing(using_decls)
                    } else {
                        StatementKind::Using(using_decls)
                    }),
                    line,
                    column,
                }));
                let update_stmt = update.map(|e| {
                    Box::new(Statement {
                        kind: Box::new(StatementKind::Expr(e)),
                        line,
                        column,
                    })
                });
                check_for_head_body_var_conflict(&init_stmt, &body_stmts, line, column)?;
                return Ok(Statement {
                    kind: Box::new(StatementKind::For(Box::new(ForStatement {
                        init: init_stmt,
                        test,
                        update: update_stmt,
                        body: body_stmts,
                    }))),
                    line,
                    column,
                });
            }
        }
    }
    let is_decl = matches!(t[*index].token, Token::Var | Token::Let | Token::Const);
    log::trace!("parse_for_statement: is_decl={} token={:?}", is_decl, t.get(*index));
    let mut init_expr: Option<Expr> = None;
    let mut init_decls: Option<Vec<(String, Option<Expr>)>> = None;
    let mut decl_kind = None;
    let mut for_of_pattern: Option<ForOfPattern> = None;
    let mut for_pattern_init: Option<Expr> = None;
    let mut init_was_bare_async = false;
    if is_decl {
        decl_kind = Some(t[*index].token.clone());
        *index += 1;
        if matches!(t[*index].token, Token::LBrace) {
            let pattern = parse_object_destructuring_pattern(t, index)?;
            if !matches!(decl_kind, Some(Token::Var)) {
                check_for_head_dup_bound_names(&pattern, line, column)?;
            }
            log::trace!(
                "parse_for_statement: parsed object destructuring pattern, index {} token={:?}",
                *index,
                t.get(*index)
            );
            for_of_pattern = Some(ForOfPattern::Object(pattern));
            if *index < t.len() && matches!(t[*index].token, Token::Assign) {
                *index += 1;
                for_pattern_init = Some(parse_assignment(t, index)?);
            }
        } else if matches!(t[*index].token, Token::LBracket) {
            let pattern = parse_array_destructuring_pattern(t, index)?;
            if !matches!(decl_kind, Some(Token::Var)) {
                check_for_head_dup_bound_names(&pattern, line, column)?;
            }
            log::trace!(
                "parse_for_statement: parsed array destructuring pattern, index {} token={:?}",
                *index,
                t.get(*index)
            );
            for_of_pattern = Some(ForOfPattern::Array(pattern));
            if *index < t.len() && matches!(t[*index].token, Token::Assign) {
                *index += 1;
                for_pattern_init = Some(parse_assignment(t, index)?);
            }
        } else {
            let decls = parse_variable_declaration_list(t, index)?;
            log::trace!(
                "parse_for_statement: parsed var declaration list, index {} token={:?}",
                *index,
                t.get(*index)
            );
            init_decls = Some(decls);
        }
    } else if !matches!(t[*index].token, Token::Semicolon) {
        if matches!(t[*index].token, Token::LBracket) {
            let pattern = parse_array_assignment_pattern(t, index)?;
            init_expr = Some(Expr::Array(pattern));
        } else if matches!(t[*index].token, Token::LBrace) {
            let pattern = parse_object_assignment_pattern(t, index)?;
            init_expr = Some(Expr::Object(pattern));
        } else {
            let init_start = *index;
            init_expr = Some(with_forbidden_in(|| parse_expression(t, index))?);
            if *index == init_start + 1 && matches!(t[init_start].token, Token::Async) {
                init_was_bare_async = true;
            }
        }
    }
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < t.len() && matches!(t[* index].token, Token::Identifier(ref s) if s == "of") {
        if raw_identifier_source_has_escape(&t[*index]) {
            return Err(raise_parse_error_with_token!(
                t[*index],
                "'of' keyword must not contain Unicode escape sequences"
            ));
        }
        *index += 1;
        let iterable = parse_assignment(t, index)?;
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if !matches!(t[*index].token, Token::RParen) {
            return Err(raise_parse_error_at!(t.get(*index)));
        }
        *index += 1;
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        let body = parse_nested_statement_item(t, index)?;
        reject_lexical_in_single_statement(&body, "for")?;
        // Keep Block as-is so its block-scope (alias mechanism at scope_depth 0)
        // is preserved — unwrapping loses let/const scoping in the for body.
        let body_stmts = vec![body];
        let decl_kind_mapped: Option<crate::core::VarDeclKind> = decl_kind.and_then(|tk| match tk {
            crate::Token::Var => Some(crate::core::VarDeclKind::Var),
            crate::Token::Let => Some(crate::core::VarDeclKind::Let),
            crate::Token::Const => Some(crate::core::VarDeclKind::Const),
            _ => None,
        });
        // Check for-of head let/const names vs var declarations in body
        if matches!(
            decl_kind_mapped,
            Some(crate::core::VarDeclKind::Let) | Some(crate::core::VarDeclKind::Const)
        ) {
            let mut head_names = Vec::new();
            if let Some(ref pattern) = for_of_pattern {
                match pattern {
                    ForOfPattern::Object(p) => collect_destructuring_bound_names(p, &mut head_names),
                    ForOfPattern::Array(p) => collect_destructuring_bound_names(p, &mut head_names),
                }
            } else if let Some(ref decls) = init_decls {
                for (name, _) in decls {
                    head_names.push(name.clone());
                }
            }
            check_forinof_head_body_var_conflict(&head_names, &body_stmts, line, column)?;
        }
        let kind = if let Some(pattern) = for_of_pattern {
            if for_pattern_init.is_some() {
                return Err(raise_parse_error!(
                    "for-of destructuring declaration cannot have initializer",
                    line,
                    column
                ));
            }
            match pattern {
                ForOfPattern::Object(destr_pattern) => {
                    let mut obj_pattern: Vec<ObjectDestructuringElement> = Vec::new();
                    for elem in destr_pattern.into_iter() {
                        match elem {
                            DestructuringElement::Property(key, boxed) => {
                                obj_pattern.push(ObjectDestructuringElement::Property { key, value: *boxed });
                            }
                            DestructuringElement::ComputedProperty(expr, boxed) => {
                                obj_pattern.push(ObjectDestructuringElement::ComputedProperty { key: expr, value: *boxed });
                            }
                            DestructuringElement::Rest(name) => {
                                obj_pattern.push(ObjectDestructuringElement::Rest(name));
                            }
                            _ => {
                                return Err(raise_parse_error!("Invalid element in object destructuring pattern", line, column));
                            }
                        }
                    }
                    if is_for_await {
                        StatementKind::ForAwaitOfDestructuringObject(decl_kind_mapped, obj_pattern, iterable, body_stmts)
                    } else {
                        StatementKind::ForOfDestructuringObject(decl_kind_mapped, obj_pattern, iterable, body_stmts)
                    }
                }
                ForOfPattern::Array(arr_pattern) => {
                    if is_for_await {
                        StatementKind::ForAwaitOfDestructuringArray(decl_kind_mapped, arr_pattern, iterable, body_stmts)
                    } else {
                        StatementKind::ForOfDestructuringArray(decl_kind_mapped, arr_pattern, iterable, body_stmts)
                    }
                }
            }
        } else {
            if let Some(decls) = init_decls {
                if decls.len() != 1 {
                    return Err(raise_parse_error!("Invalid for-of statement", line, column));
                }
                // for-of/for-await-of: initializers are never allowed
                if decls[0].1.is_some() {
                    return Err(raise_parse_error!(
                        "SyntaxError: for-of loop variable declaration may not have an initializer",
                        line,
                        column
                    ));
                }
                let var_name = decls[0].0.clone();
                if is_for_await {
                    StatementKind::ForAwaitOf(decl_kind_mapped, var_name, iterable, body_stmts)
                } else {
                    StatementKind::ForOf(decl_kind_mapped, var_name, iterable, body_stmts)
                }
            } else if let Some(Expr::Var(s, _, _)) = init_expr {
                // `for (async of ...)` is always a SyntaxError (spec: it's ambiguous with async arrow)
                // But `for ((async) of ...)` and `for (\u0061sync of ...)` are allowed
                if s == "async" && !is_for_await && init_was_bare_async {
                    return Err(raise_parse_error!(
                        "SyntaxError: The left-hand side of a for-of loop may not be 'async'",
                        line,
                        column
                    ));
                }
                if is_for_await {
                    StatementKind::ForAwaitOf(decl_kind_mapped, s, iterable, body_stmts)
                } else {
                    StatementKind::ForOf(decl_kind_mapped, s, iterable, body_stmts)
                }
            } else if let Some(expr) = init_expr {
                match expr {
                    Expr::Property(_, _) | Expr::Index(_, _) | Expr::PrivateMember(_, _) | Expr::Array(_) | Expr::Object(_) => {
                        check_destructuring_expr_strict(&expr)?;
                        if is_for_await {
                            StatementKind::ForAwaitOfExpr(expr, iterable, body_stmts)
                        } else {
                            StatementKind::ForOfExpr(expr, iterable, body_stmts)
                        }
                    }
                    _ => {
                        return Err(raise_parse_error!("Invalid for-of left-hand side", line, column));
                    }
                }
            } else {
                return Err(raise_parse_error!("Invalid for-of left-hand side", line, column));
            }
        };
        return Ok(Statement {
            kind: Box::new(kind),
            line,
            column,
        });
    }
    let mut is_for_in = false;
    let mut for_in_rhs = None;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    log::trace!("parse_for_statement: token before 'in' check={:?}", t.get(*index));
    if *index < t.len() && matches!(t[*index].token, Token::In) {
        is_for_in = true;
        *index += 1;
        for_in_rhs = Some(parse_expression(t, index)?);
    } else if !is_decl && init_expr.is_some() && matches!(t[*index].token, Token::RParen) {
        fn extract_in(expr: Expr) -> Option<(Box<Expr>, Expr)> {
            match expr {
                Expr::Binary(left, BinaryOp::In, right) => Some((left, *right)),
                Expr::Comma(left, right) => {
                    if let Some((inner_left, inner_right)) = extract_in(*left) {
                        Some((inner_left, Expr::Comma(Box::new(inner_right), right)))
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
        if let Some(init) = init_expr.clone()
            && let Some((left, right_expr)) = extract_in(init)
        {
            match *left {
                Expr::Var(name, _, _) => {
                    *index += 1;
                    let body = parse_nested_statement_item(t, index)?;
                    reject_lexical_in_single_statement(&body, "for")?;
                    let body_stmts = vec![body];
                    return Ok(Statement {
                        kind: Box::new(StatementKind::ForIn(None, name, right_expr, body_stmts)),
                        line,
                        column,
                    });
                }
                Expr::Property(_, _) | Expr::Index(_, _) | Expr::PrivateMember(_, _) => {
                    *index += 1;
                    let body = parse_nested_statement_item(t, index)?;
                    reject_lexical_in_single_statement(&body, "for")?;
                    let body_stmts = vec![body];
                    return Ok(Statement {
                        kind: Box::new(StatementKind::ForInExpr(*left, right_expr, body_stmts)),
                        line,
                        column,
                    });
                }
                _ => {}
            }
        }
    }
    if is_for_in {
        let rhs = for_in_rhs.unwrap();
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if !matches!(t[*index].token, Token::RParen) {
            return Err(raise_parse_error_at!(t.get(*index)));
        }
        *index += 1;
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        let body = parse_nested_statement_item(t, index)?;
        reject_lexical_in_single_statement(&body, "for")?;
        let body_stmts = vec![body];
        // Check for-in head let/const names vs var declarations in body
        let forin_decl_kind_mapped: Option<crate::core::VarDeclKind> = decl_kind.as_ref().and_then(|tk| match tk {
            Token::Let => Some(crate::core::VarDeclKind::Let),
            Token::Const => Some(crate::core::VarDeclKind::Const),
            _ => None,
        });
        if forin_decl_kind_mapped.is_some() {
            let mut head_names = Vec::new();
            if let Some(ref pattern) = for_of_pattern {
                match pattern {
                    ForOfPattern::Object(p) => collect_destructuring_bound_names(p, &mut head_names),
                    ForOfPattern::Array(p) => collect_destructuring_bound_names(p, &mut head_names),
                }
            } else if let Some(ref decls) = init_decls {
                for (name, _) in decls {
                    head_names.push(name.clone());
                }
            }
            check_forinof_head_body_var_conflict(&head_names, &body_stmts, line, column)?;
        }
        if let Some(pattern) = for_of_pattern {
            if for_pattern_init.is_some() {
                return Err(raise_parse_error!(
                    "for-in destructuring declaration cannot have initializer",
                    line,
                    column
                ));
            }
            match pattern {
                ForOfPattern::Object(destr_pattern) => {
                    let mut obj_pattern: Vec<ObjectDestructuringElement> = Vec::new();
                    for elem in destr_pattern.into_iter() {
                        match elem {
                            DestructuringElement::Property(key, boxed) => {
                                obj_pattern.push(ObjectDestructuringElement::Property { key, value: *boxed });
                            }
                            DestructuringElement::ComputedProperty(expr, boxed) => {
                                obj_pattern.push(ObjectDestructuringElement::ComputedProperty { key: expr, value: *boxed });
                            }
                            DestructuringElement::Rest(name) => {
                                obj_pattern.push(ObjectDestructuringElement::Rest(name));
                            }
                            _ => {
                                return Err(raise_parse_error!("Invalid element in object destructuring pattern", line, column));
                            }
                        }
                    }
                    return Ok(Statement {
                        kind: Box::new(StatementKind::ForInDestructuringObject(
                            match decl_kind {
                                Some(Token::Var) => Some(crate::core::VarDeclKind::Var),
                                Some(Token::Let) => Some(crate::core::VarDeclKind::Let),
                                Some(Token::Const) => Some(crate::core::VarDeclKind::Const),
                                Some(_) => {
                                    return Err(raise_parse_error!("Invalid declaration kind for for-in", line, column));
                                }
                                None => {
                                    return Err(raise_parse_error!("Missing declaration kind for for-in", line, column));
                                }
                            },
                            obj_pattern,
                            rhs,
                            body_stmts,
                        )),
                        line,
                        column,
                    });
                }
                ForOfPattern::Array(arr_pattern) => {
                    return Ok(Statement {
                        kind: Box::new(StatementKind::ForInDestructuringArray(
                            match decl_kind {
                                Some(Token::Var) => Some(crate::core::VarDeclKind::Var),
                                Some(Token::Let) => Some(crate::core::VarDeclKind::Let),
                                Some(Token::Const) => Some(crate::core::VarDeclKind::Const),
                                Some(_) => {
                                    return Err(raise_parse_error!("Invalid declaration kind for for-in", line, column));
                                }
                                None => {
                                    return Err(raise_parse_error!("Missing declaration kind for for-in", line, column));
                                }
                            },
                            arr_pattern,
                            rhs,
                            body_stmts,
                        )),
                        line,
                        column,
                    });
                }
            }
        }
        if init_decls.is_none()
            && let Some(expr) = init_expr
        {
            match expr {
                Expr::Property(_, _)
                | Expr::Index(_, _)
                | Expr::PrivateMember(_, _)
                | Expr::Var(_, _, _)
                | Expr::Array(_)
                | Expr::Object(_) => {
                    check_destructuring_expr_strict(&expr)?;
                    return Ok(Statement {
                        kind: Box::new(StatementKind::ForInExpr(expr, rhs, body_stmts)),
                        line,
                        column,
                    });
                }
                _ => {}
            }
        }
        let var_name = if let Some(decls) = init_decls {
            if decls.len() != 1 {
                return Err(raise_parse_error!("Invalid for-in", line, column));
            }
            decls[0].0.clone()
        } else {
            return Err(raise_parse_error!("Invalid codepath for for-in", line, column));
        };
        return Ok(Statement {
            kind: Box::new(StatementKind::ForIn(
                match decl_kind {
                    Some(Token::Var) => Some(crate::core::VarDeclKind::Var),
                    Some(Token::Let) => Some(crate::core::VarDeclKind::Let),
                    Some(Token::Const) => Some(crate::core::VarDeclKind::Const),
                    Some(_) => {
                        return Err(raise_parse_error!("Invalid declaration kind for for-in", line, column));
                    }
                    None => {
                        return Err(raise_parse_error!("Missing declaration kind for for-in", line, column));
                    }
                },
                var_name,
                rhs,
                body_stmts,
            )),
            line,
            column,
        });
    }
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::Semicolon) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let test = if !matches!(t[*index].token, Token::Semicolon) {
        Some(parse_expression(t, index)?)
    } else {
        None
    };
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::Semicolon) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let update = if !matches!(t[*index].token, Token::RParen) {
        Some(parse_expression(t, index)?)
    } else {
        None
    };
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let body = parse_nested_statement_item(t, index)?;
    reject_lexical_in_single_statement(&body, "for")?;
    let body_stmts = vec![body];
    let init_stmt = if is_decl {
        let k = if let Some(d) = init_decls {
            let decls = d;
            match decl_kind {
                Some(Token::Var) => StatementKind::Var(decls),
                Some(Token::Let) => StatementKind::Let(decls),
                Some(Token::Const) => {
                    let mut c_decls = Vec::new();
                    for (n, e) in decls {
                        if let Some(init) = e {
                            c_decls.push((n, init));
                        } else {
                            return Err(raise_parse_error!("Missing initializer in const", line, column));
                        }
                    }
                    StatementKind::Const(c_decls)
                }
                _ => unreachable!(),
            }
        } else if let Some(pattern) = for_of_pattern {
            let init = match for_pattern_init {
                Some(expr) => expr,
                None => {
                    return Err(raise_parse_error!("Missing initializer in destructuring declaration", line, column));
                }
            };
            match (decl_kind, pattern) {
                (Some(Token::Var), ForOfPattern::Array(arr)) => StatementKind::VarDestructuringArray(arr, init),
                (Some(Token::Let), ForOfPattern::Array(arr)) => StatementKind::LetDestructuringArray(arr, init),
                (Some(Token::Const), ForOfPattern::Array(arr)) => StatementKind::ConstDestructuringArray(arr, init),
                (Some(Token::Var), ForOfPattern::Object(destr_pattern)) => {
                    let mut obj_pattern: Vec<ObjectDestructuringElement> = Vec::new();
                    for elem in destr_pattern.into_iter() {
                        match elem {
                            DestructuringElement::Property(key, boxed) => {
                                obj_pattern.push(ObjectDestructuringElement::Property { key, value: *boxed });
                            }
                            DestructuringElement::ComputedProperty(expr, boxed) => {
                                obj_pattern.push(ObjectDestructuringElement::ComputedProperty { key: expr, value: *boxed });
                            }
                            DestructuringElement::Rest(name) => {
                                obj_pattern.push(ObjectDestructuringElement::Rest(name));
                            }
                            _ => {
                                return Err(raise_parse_error!("Invalid element in object destructuring pattern", line, column));
                            }
                        }
                    }
                    StatementKind::VarDestructuringObject(obj_pattern, init)
                }
                (Some(Token::Let), ForOfPattern::Object(destr_pattern)) => {
                    let mut obj_pattern: Vec<ObjectDestructuringElement> = Vec::new();
                    for elem in destr_pattern.into_iter() {
                        match elem {
                            DestructuringElement::Property(key, boxed) => {
                                obj_pattern.push(ObjectDestructuringElement::Property { key, value: *boxed });
                            }
                            DestructuringElement::ComputedProperty(expr, boxed) => {
                                obj_pattern.push(ObjectDestructuringElement::ComputedProperty { key: expr, value: *boxed });
                            }
                            DestructuringElement::Rest(name) => {
                                obj_pattern.push(ObjectDestructuringElement::Rest(name));
                            }
                            _ => {
                                return Err(raise_parse_error!("Invalid element in object destructuring pattern", line, column));
                            }
                        }
                    }
                    StatementKind::LetDestructuringObject(obj_pattern, init)
                }
                (Some(Token::Const), ForOfPattern::Object(destr_pattern)) => {
                    let mut obj_pattern: Vec<ObjectDestructuringElement> = Vec::new();
                    for elem in destr_pattern.into_iter() {
                        match elem {
                            DestructuringElement::Property(key, boxed) => {
                                obj_pattern.push(ObjectDestructuringElement::Property { key, value: *boxed });
                            }
                            DestructuringElement::ComputedProperty(expr, boxed) => {
                                obj_pattern.push(ObjectDestructuringElement::ComputedProperty { key: expr, value: *boxed });
                            }
                            DestructuringElement::Rest(name) => {
                                obj_pattern.push(ObjectDestructuringElement::Rest(name));
                            }
                            _ => {
                                return Err(raise_parse_error!("Invalid element in object destructuring pattern", line, column));
                            }
                        }
                    }
                    StatementKind::ConstDestructuringObject(obj_pattern, init)
                }
                _ => {
                    return Err(raise_parse_error!("Missing declarations in for-init", line, column));
                }
            }
        } else {
            return Err(raise_parse_error!("Missing declarations in for-init", line, column));
        };
        Some(Box::new(Statement {
            kind: Box::new(k),
            line,
            column,
        }))
    } else {
        init_expr.map(|e| {
            Box::new(Statement {
                kind: Box::new(StatementKind::Expr(e)),
                line,
                column,
            })
        })
    };
    let update_stmt = update.map(|e| {
        Box::new(Statement {
            kind: Box::new(StatementKind::Expr(e)),
            line,
            column,
        })
    });
    check_for_head_body_var_conflict(&init_stmt, &body_stmts, line, column)?;
    Ok(Statement {
        kind: Box::new(StatementKind::For(Box::new(ForStatement {
            init: init_stmt,
            test,
            update: update_stmt,
            body: body_stmts,
        }))),
        line,
        column,
    })
}
fn parse_function_declaration(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    let mut is_async = false;
    if matches!(t[*index].token, Token::Async) {
        is_async = true;
        *index += 1;
    }
    let mut is_generator = matches!(t[*index].token, Token::FunctionStar);
    if !is_generator && !matches!(t[*index].token, Token::Function) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    if matches!(t[*index].token, Token::Function) {
        if *index + 1 < t.len() && matches!(t[*index + 1].token, Token::Multiply) {
            is_generator = true;
            *index += 2;
        } else {
            *index += 1;
        }
    } else {
        *index += 1;
    }
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let name = if let Token::Identifier(name) = &t[*index].token {
        if name == "await" && is_async && forbid_await_identifier() && !in_await_context() {
            return Err(raise_parse_error!("SyntaxError: Cannot use 'await' as identifier in static block"));
        }
        name.clone()
    } else if matches!(t[*index].token, Token::Await) {
        // The function name uses the enclosing scope's [Await] parameter (BindingIdentifier[?Yield, ?Await]),
        // NOT the function's own +Await. So `async function await(){}` is valid in script scope.
        if in_module_context() || forbid_await_identifier() || in_await_context() {
            return Err(raise_parse_error!("SyntaxError: Cannot use 'await' as identifier"));
        }
        "await".to_string()
    } else {
        return Err(raise_parse_error_at!(t.get(*index)));
    };
    *index += 1;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    // Functions have their own `arguments`, clear static block restriction for params
    let saved_sb_fd = STATIC_BLOCK_CONTEXT.with(|c| {
        let p = *c.borrow();
        *c.borrow_mut() = 0;
        p
    });
    let params = if is_generator {
        push_generator_context();
        let p = with_cleared_forbidden_await_identifier(|| parse_parameters(t, index))?;
        pop_generator_context();
        p
    } else {
        // Non-generator function params must not see enclosing generator context
        let saved = GENERATOR_CONTEXT.with(|c| {
            let old = *c.borrow();
            *c.borrow_mut() = 0;
            old
        });
        let p = with_cleared_await_context(|| with_cleared_forbidden_await_identifier(|| parse_parameters(t, index)))?;
        GENERATOR_CONTEXT.with(|c| *c.borrow_mut() = saved);
        p
    };
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::LBrace) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    let body = if is_async {
        push_await_context();
        push_function_context();
        if is_generator {
            push_generator_context();
        }
        let b = with_cleared_forbidden_await_identifier(|| parse_statement_block(t, index))?;
        if is_generator {
            pop_generator_context();
        }
        pop_function_context();
        pop_await_context();
        b
    } else {
        push_function_context();
        if is_generator {
            push_generator_context();
        }
        let b = with_cleared_forbidden_await_identifier(|| with_cleared_await_context(|| parse_statement_block(t, index)))?;
        if is_generator {
            pop_generator_context();
        }
        pop_function_context();
        b
    };
    STATIC_BLOCK_CONTEXT.with(|c| *c.borrow_mut() = saved_sb_fd);
    Ok(Statement {
        kind: Box::new(StatementKind::FunctionDeclaration(name, params, body, is_generator, is_async)),
        line: t[start].line,
        column: t[start].column,
    })
}
fn parse_if_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    let condition = parse_expression(t, index)?;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let then_stmt = parse_nested_statement_item(t, index)?;
    reject_lexical_in_single_statement(&then_stmt, "if")?;
    let then_block = vec![then_stmt];
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let else_block = if *index < t.len() && matches!(t[*index].token, Token::Else) {
        *index += 1;
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        let else_stmt = parse_nested_statement_item(t, index)?;
        reject_lexical_in_single_statement(&else_stmt, "else")?;
        Some(vec![else_stmt])
    } else {
        None
    };
    Ok(Statement {
        kind: Box::new(StatementKind::If(Box::new(IfStatement {
            condition,
            then_body: then_block,
            else_body: else_block,
        }))),
        line: t[start].line,
        column: t[start].column,
    })
}
fn parse_return_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    if !in_function_context() {
        return Err(raise_parse_error!("Illegal return statement", t[start].line, t[start].column));
    }
    *index += 1;
    let expr = if *index < t.len() && !matches!(t[*index].token, Token::Semicolon | Token::LineTerminator | Token::RBrace) {
        Some(parse_expression(t, index)?)
    } else {
        None
    };
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
    }
    Ok(Statement {
        kind: Box::new(StatementKind::Return(expr)),
        line: t[start].line,
        column: t[start].column,
    })
}
fn parse_while_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    let condition = parse_expression(t, index)?;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let body_stmts = if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
        vec![]
    } else {
        let s = parse_nested_statement_item(t, index)?;
        reject_lexical_in_single_statement(&s, "while")?;
        vec![s]
    };
    Ok(Statement {
        kind: Box::new(StatementKind::While(condition, body_stmts)),
        line: t[start].line,
        column: t[start].column,
    })
}
fn parse_do_while_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    log::trace!("parse_do_while: at index {} token={:?}", *index, t.get(*index));
    let body_stmts = if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        log::trace!("parse_do_while: found semicolon empty body at index {}", *index);
        *index += 1;
        vec![]
    } else {
        log::trace!("parse_do_while: parsing body statement at index {}", *index);
        let body = parse_nested_statement_item(t, index)?;
        reject_lexical_in_single_statement(&body, "do-while")?;
        log::trace!(
            "parse_do_while: after parsing body index {}, next token={:?}",
            *index,
            t.get(*index)
        );
        vec![body]
    };
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::While) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    let condition = parse_expression(t, index)?;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
    }
    Ok(Statement {
        kind: Box::new(StatementKind::DoWhile(body_stmts, condition)),
        line: t[start].line,
        column: t[start].column,
    })
}
fn parse_switch_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    let expr = parse_expression(t, index)?;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    if !matches!(t[*index].token, Token::LBrace) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    let mut cases: Vec<crate::core::SwitchCase> = Vec::new();
    let mut has_default = false;
    while *index < t.len() && !matches!(t[*index].token, Token::RBrace) {
        if matches!(t[*index].token, Token::Case) {
            *index += 1;
            let case_expr = parse_expression(t, index)?;
            if !matches!(t[*index].token, Token::Colon) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1;
            let mut stmts: Vec<Statement> = Vec::new();
            loop {
                while *index < t.len() && matches!(t[*index].token, Token::Semicolon | Token::LineTerminator) {
                    *index += 1;
                }
                if *index >= t.len() || matches!(t[*index].token, Token::Case | Token::Default | Token::RBrace) {
                    break;
                }
                stmts.push(parse_nested_statement_item(t, index)?);
                if let Some(last) = stmts.last()
                    && matches!(&*last.kind, StatementKind::Using(..) | StatementKind::AwaitUsing(..))
                {
                    return Err(raise_parse_error!(
                        "using declarations are not allowed in switch case clauses",
                        last.line,
                        last.column
                    ));
                }
            }
            cases.push(crate::core::SwitchCase::Case(case_expr, stmts));
        } else if matches!(t[*index].token, Token::Default) {
            if has_default {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            has_default = true;
            *index += 1;
            if !matches!(t[*index].token, Token::Colon) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1;
            let mut stmts: Vec<Statement> = Vec::new();
            loop {
                while *index < t.len() && matches!(t[*index].token, Token::Semicolon | Token::LineTerminator) {
                    *index += 1;
                }
                if *index >= t.len() || matches!(t[*index].token, Token::Case | Token::Default | Token::RBrace) {
                    break;
                }
                stmts.push(parse_nested_statement_item(t, index)?);
                if let Some(last) = stmts.last()
                    && matches!(&*last.kind, StatementKind::Using(..) | StatementKind::AwaitUsing(..))
                {
                    return Err(raise_parse_error!(
                        "using declarations are not allowed in switch case clauses",
                        last.line,
                        last.column
                    ));
                }
            }
            cases.push(crate::core::SwitchCase::Default(stmts));
        } else if matches!(t[*index].token, Token::Semicolon | Token::LineTerminator) {
            *index += 1;
        } else {
            return Err(raise_parse_error_at!(t.get(*index)));
        }
    }
    if !matches!(t[*index].token, Token::RBrace) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    Ok(Statement {
        kind: Box::new(StatementKind::Switch(Box::new(SwitchStatement { expr, cases }))),
        line: t[start].line,
        column: t[start].column,
    })
}
fn parse_break_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    let mut label = None;
    if *index < t.len()
        && !matches!(t[*index].token, Token::Semicolon | Token::LineTerminator | Token::RBrace)
        && let Token::Identifier(name) = &t[*index].token
    {
        label = Some(name.clone());
        *index += 1;
    }
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
    }
    Ok(Statement {
        kind: Box::new(StatementKind::Break(label)),
        line: t[start].line,
        column: t[start].column,
    })
}
fn parse_continue_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    let mut label = None;
    if *index < t.len()
        && !matches!(t[*index].token, Token::Semicolon | Token::LineTerminator | Token::RBrace)
        && let Token::Identifier(name) = &t[*index].token
    {
        label = Some(name.clone());
        *index += 1;
    }
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
    }
    Ok(Statement {
        kind: Box::new(StatementKind::Continue(label)),
        line: t[start].line,
        column: t[start].column,
    })
}
fn parse_with_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    let line = t[start].line;
    let column = t[start].column;
    if strict_binding_checks() {
        return Err(raise_parse_error!(
            "Strict mode code may not include a with statement",
            line,
            column
        ));
    }
    *index += 1;
    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    let obj_expr = parse_expression(t, index)?;
    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let stmt = parse_nested_statement_item(t, index)?;
    let body_stmts = match *stmt.kind {
        StatementKind::Block(stmts) => stmts,
        _ => vec![stmt],
    };
    Ok(Statement {
        kind: Box::new(StatementKind::With(Box::new(obj_expr), body_stmts)),
        line,
        column,
    })
}
fn parse_throw_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    if matches!(t[*index].token, Token::LineTerminator) {
        return Err(raise_parse_error_with_token!(t.get(*index).unwrap(), "Illegal newline after throw"));
    }
    let expr = parse_expression(t, index)?;
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
    }
    Ok(Statement {
        kind: Box::new(StatementKind::Throw(expr)),
        line: t[start].line,
        column: t[start].column,
    })
}
fn parse_try_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let try_block = parse_block_statement(t, index)?;
    let try_body = if let StatementKind::Block(stmts) = *try_block.kind {
        stmts
    } else {
        return Err(raise_parse_error!("Expected block after try", t[start].line, t[start].column));
    };
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let mut catch_param = None;
    let mut catch_body = None;
    if *index < t.len() && matches!(t[*index].token, Token::Catch) {
        *index += 1;
        if *index < t.len() && matches!(t[*index].token, Token::LParen) {
            *index += 1;
            if *index < t.len() {
                match &t[*index].token {
                    Token::Identifier(name) => {
                        if strict_binding_checks() && (name == "eval" || name == "arguments") {
                            return Err(raise_parse_error_with_token!(
                                t.get(*index).unwrap(),
                                format!("Binding '{}' in strict mode", name)
                            ));
                        }
                        catch_param = Some(CatchParamPattern::Identifier(name.clone()));
                        *index += 1;
                    }
                    Token::Await if !in_await_context() && !forbid_await_identifier() => {
                        catch_param = Some(CatchParamPattern::Identifier("await".to_string()));
                        *index += 1;
                    }
                    Token::LBracket => {
                        let pattern = parse_array_destructuring_pattern(t, index)?;
                        catch_param = Some(CatchParamPattern::Array(pattern));
                    }
                    Token::LBrace => {
                        let pattern = parse_object_destructuring_pattern(t, index)?;
                        catch_param = Some(CatchParamPattern::Object(pattern));
                    }
                    _ => {
                        let msg = "Expected catch binding pattern";
                        return Err(raise_parse_error_with_token!(t.get(*index).unwrap(), msg));
                    }
                }
            } else {
                let msg = "Expected identifier in catch binding";
                return Err(raise_parse_error_with_token!(t.get(*index).unwrap(), msg));
            }
            if *index >= t.len() || !matches!(t[*index].token, Token::RParen) {
                let msg = "Expected ) after catch binding";
                return Err(raise_parse_error_with_token!(t.get(*index).unwrap(), msg));
            }
            *index += 1;
        }
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        let catch_block = parse_block_statement(t, index)?;
        if let StatementKind::Block(stmts) = *catch_block.kind {
            catch_body = Some(stmts);
        } else {
            return Err(raise_parse_error_with_token!(t.get(*index).unwrap(), "Expected block after catch"));
        }
    }
    let mut finally_body = None;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < t.len() && matches!(t[*index].token, Token::Finally) {
        *index += 1;
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        let finally_block = parse_block_statement(t, index)?;
        if let StatementKind::Block(stmts) = *finally_block.kind {
            finally_body = Some(stmts);
        } else {
            let msg = "Expected block after finally";
            return Err(raise_parse_error_with_token!(t.get(*index).unwrap(), msg));
        }
    }
    if catch_body.is_none() && finally_body.is_none() {
        let msg = "Missing catch or finally after try";
        return Err(raise_parse_error!(msg, t[start].line, t[start].column));
    }
    Ok(Statement {
        kind: Box::new(StatementKind::TryCatch(Box::new(TryCatchStatement {
            try_body,
            catch_param,
            catch_body,
            finally_body,
        }))),
        line: t[start].line,
        column: t[start].column,
    })
}
fn parse_block_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    let body = parse_statements(t, index)?;
    if *index >= t.len() || !matches!(t[*index].token, Token::RBrace) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    Ok(Statement {
        kind: Box::new(StatementKind::Block(body)),
        line: t[start].line,
        column: t[start].column,
    })
}
fn parse_var_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    if *index < t.len() && matches!(t[*index].token, Token::LBracket) {
        let mut idx = *index;
        let pattern = parse_array_destructuring_pattern(t, &mut idx)?;
        *index = idx;
        if *index < t.len() && matches!(t[*index].token, Token::Assign) {
            *index += 1;
            log::trace!(
                "parse_var_statement: parsing initializer at index={} token={:?}",
                *index,
                t.get(*index)
            );
            let init = parse_assignment(t, index)?;
            if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
                *index += 1;
            }
            return Ok(Statement {
                kind: Box::new(StatementKind::VarDestructuringArray(pattern, init)),
                line: t[start].line,
                column: t[start].column,
            });
        } else {
            let msg = "Missing initializer in destructuring declaration";
            return Err(raise_parse_error_with_token!(t.get(*index).unwrap(), msg));
        }
    }
    if *index < t.len() && matches!(t[*index].token, Token::LBrace) {
        let mut idx = *index;
        let pattern = parse_object_destructuring_pattern(t, &mut idx)?;
        *index = idx;
        if *index < t.len() && matches!(t[*index].token, Token::Assign) {
            *index += 1;
            let init = parse_assignment(t, index)?;
            if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
                *index += 1;
            }
            let mut obj_pattern: Vec<ObjectDestructuringElement> = Vec::new();
            for elem in pattern.into_iter() {
                match elem {
                    DestructuringElement::Property(key, boxed) => {
                        obj_pattern.push(ObjectDestructuringElement::Property { key, value: *boxed });
                    }
                    DestructuringElement::ComputedProperty(expr, boxed) => {
                        obj_pattern.push(ObjectDestructuringElement::ComputedProperty { key: expr, value: *boxed });
                    }
                    DestructuringElement::Rest(name) => {
                        obj_pattern.push(ObjectDestructuringElement::Rest(name));
                    }
                    _ => {
                        let msg = "Invalid element in object destructuring pattern";
                        return Err(raise_parse_error!(msg, t[start].line, t[start].column));
                    }
                }
            }
            return Ok(Statement {
                kind: Box::new(StatementKind::VarDestructuringObject(obj_pattern, init)),
                line: t[start].line,
                column: t[start].column,
            });
        } else {
            let msg = "Missing initializer in destructuring declaration";
            return Err(raise_parse_error_with_token!(t.get(*index).unwrap(), msg));
        }
    }
    let decls = parse_variable_declaration_list(t, index)?;
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
    }
    Ok(Statement {
        kind: Box::new(StatementKind::Var(decls)),
        line: t[start].line,
        column: t[start].column,
    })
}
fn parse_let_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    if *index < t.len() && matches!(t[*index].token, Token::LBracket) {
        let mut idx = *index;
        let pattern = parse_array_destructuring_pattern(t, &mut idx)?;
        *index = idx;
        if *index < t.len() && matches!(t[*index].token, Token::Assign) {
            *index += 1;
            let init = parse_assignment(t, index)?;
            if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
                *index += 1;
            }
            return Ok(Statement {
                kind: Box::new(StatementKind::LetDestructuringArray(pattern, init)),
                line: t[start].line,
                column: t[start].column,
            });
        } else {
            let msg = "Missing initializer in destructuring declaration";
            return Err(raise_parse_error_with_token!(t.get(*index).unwrap(), msg));
        }
    }
    if *index < t.len() && matches!(t[*index].token, Token::LBrace) {
        let mut idx = *index;
        let pattern = parse_object_destructuring_pattern(t, &mut idx)?;
        *index = idx;
        if *index < t.len() && matches!(t[*index].token, Token::Assign) {
            *index += 1;
            let init = parse_assignment(t, index)?;
            if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
                *index += 1;
            }
            let mut obj_pattern: Vec<ObjectDestructuringElement> = Vec::new();
            for elem in pattern.into_iter() {
                match elem {
                    DestructuringElement::Property(key, boxed) => {
                        obj_pattern.push(ObjectDestructuringElement::Property { key, value: *boxed });
                    }
                    DestructuringElement::ComputedProperty(expr, boxed) => {
                        obj_pattern.push(ObjectDestructuringElement::ComputedProperty { key: expr, value: *boxed });
                    }
                    DestructuringElement::Rest(name) => {
                        obj_pattern.push(ObjectDestructuringElement::Rest(name));
                    }
                    _ => {
                        let msg = "Invalid element in object destructuring pattern";
                        return Err(raise_parse_error!(msg, t[start].line, t[start].column));
                    }
                }
            }
            return Ok(Statement {
                kind: Box::new(StatementKind::LetDestructuringObject(obj_pattern, init)),
                line: t[start].line,
                column: t[start].column,
            });
        } else {
            let msg = "Missing initializer in destructuring declaration";
            return Err(raise_parse_error_with_token!(t.get(*index).unwrap(), msg));
        }
    }
    let decls = parse_variable_declaration_list(t, index)?;
    finish_statement_without_semicolon(t, index)?;
    Ok(Statement {
        kind: Box::new(StatementKind::Let(decls)),
        line: t[start].line,
        column: t[start].column,
    })
}
fn parse_const_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    if *index < t.len() && matches!(t[*index].token, Token::LBracket) {
        let mut idx = *index;
        let pattern = parse_array_destructuring_pattern(t, &mut idx)?;
        *index = idx;
        if *index < t.len() && matches!(t[*index].token, Token::Assign) {
            *index += 1;
            let init = parse_assignment(t, index)?;
            if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
                *index += 1;
            }
            return Ok(Statement {
                kind: Box::new(StatementKind::ConstDestructuringArray(pattern, init)),
                line: t[start].line,
                column: t[start].column,
            });
        } else {
            let msg = "Missing initializer in const destructuring declaration";
            return Err(raise_parse_error_with_token!(t.get(*index).unwrap(), msg));
        }
    }
    if *index < t.len() && matches!(t[*index].token, Token::LBrace) {
        let mut idx = *index;
        let pattern = parse_object_destructuring_pattern(t, &mut idx)?;
        *index = idx;
        if *index < t.len() && matches!(t[*index].token, Token::Assign) {
            *index += 1;
            let init = parse_assignment(t, index)?;
            if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
                *index += 1;
            }
            let mut obj_pattern: Vec<ObjectDestructuringElement> = Vec::new();
            for elem in pattern.into_iter() {
                match elem {
                    DestructuringElement::Property(key, boxed) => {
                        obj_pattern.push(ObjectDestructuringElement::Property { key, value: *boxed });
                    }
                    DestructuringElement::ComputedProperty(expr, boxed) => {
                        obj_pattern.push(ObjectDestructuringElement::ComputedProperty { key: expr, value: *boxed });
                    }
                    DestructuringElement::Rest(name) => {
                        obj_pattern.push(ObjectDestructuringElement::Rest(name));
                    }
                    _ => {
                        let msg = "Invalid element in object destructuring pattern";
                        return Err(raise_parse_error_with_token!(t.get(start).unwrap(), msg));
                    }
                }
            }
            return Ok(Statement {
                kind: Box::new(StatementKind::ConstDestructuringObject(obj_pattern, init)),
                line: t[start].line,
                column: t[start].column,
            });
        } else {
            let msg = "Missing initializer in const destructuring declaration";
            return Err(raise_parse_error_with_token!(t.get(*index).unwrap(), msg));
        }
    }
    let decls = parse_variable_declaration_list(t, index)?;
    let mut const_decls = Vec::new();
    for (name, init) in decls {
        if let Some(expr) = init {
            const_decls.push((name, expr));
        } else {
            return Err(raise_parse_error!("Missing initializer in const declaration"));
        }
    }
    finish_statement_without_semicolon(t, index)?;
    Ok(Statement {
        kind: Box::new(StatementKind::Const(const_decls)),
        line: t[start].line,
        column: t[start].column,
    })
}
/// Check that a StringLiteral used as a ModuleExportName is well-formed Unicode
/// (no unpaired surrogates). Spec: "It is a Syntax Error if IsStringWellFormedUnicode
/// of the StringValue of StringLiteral is false."
fn check_module_export_name_well_formed(s: &[u16]) -> Result<(), JSError> {
    let len = s.len();
    let mut i = 0;
    while i < len {
        let c = s[i];
        if (0xD800..=0xDBFF).contains(&c) {
            if i + 1 >= len || !(0xDC00..=0xDFFF).contains(&s[i + 1]) {
                return Err(raise_syntax_error!("Module export name must not contain an unpaired surrogate"));
            }
            i += 2;
        } else if (0xDC00..=0xDFFF).contains(&c) {
            return Err(raise_syntax_error!("Module export name must not contain an unpaired surrogate"));
        } else {
            i += 1;
        }
    }
    Ok(())
}
fn parse_import_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    let mut specifiers = Vec::new();
    let mut source = String::new();
    if let Token::StringLit(s) = &t[*index].token {
        source = utf16_to_utf8(s);
        *index += 1;
    } else {
        let is_deferred_namespace = matches!(&t[*index].token, Token::Identifier(s) if s == "defer")
            && *index + 1 < t.len()
            && matches!(t[*index + 1].token, Token::Multiply);
        if is_deferred_namespace {
            *index += 1; // defer
            *index += 1; // *
            if *index < t.len() {
                let is_as = match &t[*index].token {
                    _ if token_is_escaped_identifier_name(&t[*index], "as") => {
                        return Err(raise_parse_error!("Keyword 'as' must not contain Unicode escape sequences"));
                    }
                    _ if token_matches_unescaped_identifier_name(&t[*index], "as") => true,
                    _ => false,
                };
                if is_as {
                    *index += 1;
                    if let Some(name) = t[*index].token.as_identifier_string() {
                        specifiers.push(ImportSpecifier::DeferredNamespace(name));
                        *index += 1;
                    } else {
                        return Err(raise_parse_error!("Expected identifier after 'import defer * as'"));
                    }
                } else {
                    return Err(raise_parse_error!("Expected 'as' after 'import defer *'"));
                }
            }
        } else if matches!(&t[*index].token, Token::Identifier(s) if s == "defer")
            && *index + 1 < t.len()
            && matches!(t[*index + 1].token, Token::LBrace)
        {
            // `import defer { ... }` is not valid - defer only works with `* as name`
            return Err(raise_parse_error!(
                "SyntaxError: 'import defer' must use namespace form 'import defer * as name'"
            ));
        } else if let Some(name) = t[*index].token.as_identifier_string() {
            specifiers.push(ImportSpecifier::Default(name));
            *index += 1;
            if *index < t.len() && matches!(t[*index].token, Token::Comma) {
                *index += 1;
            }
        }
        if *index < t.len() && matches!(t[*index].token, Token::Multiply) {
            *index += 1;
            if *index < t.len() {
                let is_as = match &t[*index].token {
                    _ if token_is_escaped_identifier_name(&t[*index], "as") => {
                        return Err(raise_parse_error!("Keyword 'as' must not contain Unicode escape sequences"));
                    }
                    _ if token_matches_unescaped_identifier_name(&t[*index], "as") => true,
                    _ => false,
                };
                if is_as {
                    *index += 1;
                    if let Some(name) = t[*index].token.as_identifier_string() {
                        specifiers.push(ImportSpecifier::Namespace(name));
                        *index += 1;
                    } else {
                        return Err(raise_parse_error!("Expected identifier after '* as'"));
                    }
                } else {
                    return Err(raise_parse_error!("Expected 'as' after '*'"));
                }
            }
        }
        if *index < t.len() && matches!(t[*index].token, Token::LBrace) {
            *index += 1;
            loop {
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                if *index < t.len() && matches!(t[*index].token, Token::RBrace) {
                    *index += 1;
                    break;
                }
                let (imported_name, imported_is_string) = if let Some(id_name) = t[*index].token.as_identifier_string() {
                    (id_name, false)
                } else if let Token::StringLit(s) = &t[*index].token {
                    check_module_export_name_well_formed(s)?;
                    (utf16_to_utf8(s), true)
                } else {
                    return Err(raise_parse_error!("Expected identifier or string literal in named import"));
                };
                *index += 1;
                let mut local_name = None;
                if *index < t.len() {
                    let is_as = match &t[*index].token {
                        _ if token_is_escaped_identifier_name(&t[*index], "as") => {
                            return Err(raise_parse_error!("Keyword 'as' must not contain Unicode escape sequences"));
                        }
                        _ if token_matches_unescaped_identifier_name(&t[*index], "as") => true,
                        _ => false,
                    };
                    if is_as {
                        *index += 1;
                        if let Some(alias) = t[*index].token.as_identifier_string() {
                            local_name = Some(alias);
                            *index += 1;
                        } else {
                            return Err(raise_parse_error!("Expected identifier after 'as'"));
                        }
                    } else if imported_is_string {
                        return Err(raise_syntax_error!(
                            "A string literal import name requires 'as' followed by an identifier"
                        ));
                    }
                }
                specifiers.push(ImportSpecifier::Named(imported_name, local_name));
                if *index < t.len() && matches!(t[*index].token, Token::Comma) {
                    *index += 1;
                }
            }
        }
        if *index < t.len() {
            let is_from = if let Token::Identifier(ref from_kw) = t[*index].token {
                from_kw == "from" && !raw_identifier_source_has_escape(&t[*index])
            } else {
                false
            };
            if is_from {
                *index += 1;
            } else {
                return Err(raise_parse_error!("Expected 'from'"));
            }
        }
        if *index < t.len() {
            if let Token::StringLit(s) = &t[*index].token {
                source = utf16_to_utf8(s);
                *index += 1;
            } else {
                return Err(raise_parse_error!("Expected module specifier"));
            }
        }
    }
    consume_import_attributes_clause(t, index)?;
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
    }
    // Check for duplicate bound names and restricted identifiers in import specifiers
    {
        let mut bound_names = Vec::new();
        for spec in &specifiers {
            let local = match spec {
                ImportSpecifier::Default(n) => n.clone(),
                ImportSpecifier::Namespace(n) => n.clone(),
                ImportSpecifier::DeferredNamespace(n) => n.clone(),
                ImportSpecifier::Named(imported, alias) => alias.as_ref().cloned().unwrap_or_else(|| imported.clone()),
            };
            // Modules are strict mode: eval/arguments cannot be import bindings
            if local == "eval" || local == "arguments" {
                return Err(raise_parse_error!(
                    &format!("SyntaxError: '{}' cannot be used as an imported binding name", local),
                    t[start].line,
                    t[start].column
                ));
            }
            if bound_names.contains(&local) {
                return Err(raise_parse_error!(
                    &format!("SyntaxError: Duplicate import bound name '{}'", local),
                    t[start].line,
                    t[start].column
                ));
            }
            bound_names.push(local);
        }
    }
    Ok(Statement {
        kind: Box::new(StatementKind::Import(specifiers, source)),
        line: t[start].line,
        column: t[start].column,
    })
}
fn consume_import_attributes_clause(t: &[TokenData], index: &mut usize) -> Result<(), JSError> {
    if *index >= t.len() {
        return Ok(());
    }
    let is_with_clause = (matches!(t[*index].token, Token::With) || matches!(&t[*index].token, Token::Identifier(s) if s == "with"))
        && !raw_identifier_source_has_escape(&t[*index]);
    if !is_with_clause {
        return Ok(());
    }
    *index += 1;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index >= t.len() || !matches!(t[*index].token, Token::LBrace) {
        return Err(raise_parse_error!("Expected '{' after import attributes 'with'"));
    }
    *index += 1; // skip '{'
    let mut seen_keys: Vec<String> = Vec::new();
    loop {
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index >= t.len() {
            return Err(raise_parse_error!("Unterminated import attributes clause"));
        }
        if matches!(t[*index].token, Token::RBrace) {
            *index += 1;
            return Ok(());
        }
        // Parse attribute key: IdentifierName or StringLiteral
        let key = match &t[*index].token {
            Token::StringLit(s) => {
                let k = utf16_to_utf8(s);
                *index += 1;
                k
            }
            _ => {
                // Any IdentifierName (including keywords) is valid as attribute key
                let k = token_to_identifier_name(&t[*index]);
                if k.is_empty() {
                    return Err(raise_parse_error_with_token!(
                        t[*index],
                        "Expected attribute key (identifier or string)"
                    ));
                }
                *index += 1;
                k
            }
        };
        // Duplicate key check
        if seen_keys.contains(&key) {
            return Err(raise_syntax_error!(format!("Duplicate import attribute key '{}'", key)));
        }
        seen_keys.push(key);
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        // Expect ':'
        if *index >= t.len() || !matches!(t[*index].token, Token::Colon) {
            return Err(raise_parse_error!("Expected ':' in import attribute"));
        }
        *index += 1;
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        // Expect string value
        if *index >= t.len() || !matches!(t[*index].token, Token::StringLit(_)) {
            return Err(raise_parse_error!("Import attribute value must be a string"));
        }
        *index += 1;
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        // Optional comma
        if *index < t.len() && matches!(t[*index].token, Token::Comma) {
            *index += 1;
        }
    }
}
/// In module code, function declarations are lexically scoped.
/// Track names for duplicate detection and var/lexical conflicts.
fn track_module_level_names(stmt: &Statement) -> Result<(), JSError> {
    match stmt.kind.as_ref() {
        StatementKind::FunctionDeclaration(name, ..) => {
            add_module_lexical_name(name)?;
        }
        StatementKind::Class(def) => {
            if !def.name.is_empty() {
                add_module_lexical_name(&def.name)?;
            }
        }
        StatementKind::Let(decls) => {
            for decl in decls {
                add_module_lexical_name(&decl.0)?;
            }
        }
        StatementKind::Const(decls) => {
            for decl in decls {
                add_module_lexical_name(&decl.0)?;
            }
        }
        StatementKind::Var(decls) => {
            for decl in decls {
                add_module_var_name(&decl.0)?;
            }
        }
        StatementKind::Export(_specs, Some(inner), _) => {
            track_module_level_names(inner)?;
        }
        StatementKind::Export(specs, None, _) => {
            for spec in specs {
                if let ExportSpecifier::Default(expr) = spec {
                    match expr {
                        Expr::Function(Some(name), ..)
                        | Expr::GeneratorFunction(Some(name), ..)
                        | Expr::AsyncFunction(Some(name), ..)
                        | Expr::AsyncGeneratorFunction(Some(name), ..) => {
                            if name != "default" {
                                add_module_lexical_name(name)?;
                            }
                        }
                        Expr::Class(def) => {
                            if !def.name.is_empty() {
                                add_module_lexical_name(&def.name)?;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}
fn parse_export_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let mut specifiers = Vec::new();
    let mut inner_stmt = None;
    let mut source = None;
    if *index < t.len() && matches!(t[*index].token, Token::Default) {
        if raw_identifier_source_has_escape(&t[*index]) {
            return Err(raise_parse_error!("Keyword 'default' must not contain Unicode escape sequences"));
        }
        *index += 1;
        let should_normalize_default_function_name =
            *index < t.len() && matches!(t[*index].token, Token::Function | Token::FunctionStar | Token::Async);
        let mut expr = parse_assignment(t, index)?;
        if should_normalize_default_function_name {
            // export default HoistableDeclaration[Default] — the function/generator
            // is a declaration, not an expression, so it must not be called/accessed.
            match &expr {
                Expr::Function(..) | Expr::GeneratorFunction(..) | Expr::AsyncFunction(..) | Expr::AsyncGeneratorFunction(..) => {}
                _ => {
                    return Err(raise_syntax_error!("Unexpected token after export default declaration"));
                }
            }
            expr = match expr {
                Expr::Function(None, params, body, st) => Expr::Function(Some("default".to_string()), params, body, st),
                Expr::GeneratorFunction(None, params, body, st) => Expr::GeneratorFunction(Some("default".to_string()), params, body, st),
                Expr::AsyncFunction(None, params, body, st) => Expr::AsyncFunction(Some("default".to_string()), params, body, st),
                Expr::AsyncGeneratorFunction(None, params, body, st) => {
                    Expr::AsyncGeneratorFunction(Some("default".to_string()), params, body, st)
                }
                other => other,
            };
        }
        specifiers.push(ExportSpecifier::Default(expr));
        if !should_normalize_default_function_name && !matches!(specifiers.last(), Some(ExportSpecifier::Default(Expr::Class(..)))) {
            finish_statement_without_semicolon(t, index)?;
        }
    } else if *index < t.len() && matches!(t[*index].token, Token::Multiply) {
        *index += 1;
        let is_as = if *index < t.len() {
            match &t[*index].token {
                _ if token_is_escaped_identifier_name(&t[*index], "as") => {
                    return Err(raise_parse_error!("Keyword 'as' must not contain Unicode escape sequences"));
                }
                _ if token_matches_unescaped_identifier_name(&t[*index], "as") => true,
                _ => false,
            }
        } else {
            false
        };
        if is_as {
            *index += 1;
            let name = if *index < t.len() {
                if let Some(id_name) = t[*index].token.as_identifier_string() {
                    *index += 1;
                    id_name
                } else if let Token::StringLit(s) = &t[*index].token {
                    check_module_export_name_well_formed(s)?;
                    let name = utf16_to_utf8(s);
                    *index += 1;
                    name
                } else {
                    return Err(raise_parse_error!(
                        "Expected identifier or string literal after 'as' in export statement"
                    ));
                }
            } else {
                return Err(raise_parse_error!(
                    "Expected identifier or string literal after 'as' in export statement"
                ));
            };
            specifiers.push(ExportSpecifier::Namespace(name));
        } else {
            specifiers.push(ExportSpecifier::Star);
        }
        if *index < t.len() {
            let is_from = if let Token::Identifier(from_kw) = &t[*index].token {
                *from_kw == "from" && !raw_identifier_source_has_escape(&t[*index])
            } else {
                false
            };
            if !is_from {
                return Err(raise_parse_error!("Expected 'from' after export '*'"));
            }
            *index += 1;
            if *index < t.len() {
                if let Token::StringLit(s) = &t[*index].token {
                    source = Some(utf16_to_utf8(s));
                    *index += 1;
                } else {
                    return Err(raise_parse_error!("Expected module specifier"));
                }
            }
        }
        consume_import_attributes_clause(t, index)?;
        finish_statement_without_semicolon(t, index)?;
    } else if *index < t.len() && matches!(t[*index].token, Token::LBrace) {
        *index += 1;
        let mut has_string_source_name = false;
        loop {
            while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                *index += 1;
            }
            if *index < t.len() && matches!(t[*index].token, Token::RBrace) {
                *index += 1;
                break;
            }
            let (name, name_is_string) = if let Some(id_name) = t[*index].token.as_identifier_string() {
                (id_name, false)
            } else if let Token::StringLit(s) = &t[*index].token {
                check_module_export_name_well_formed(s)?;
                (utf16_to_utf8(s), true)
            } else {
                return Err(raise_parse_error!("Expected identifier or string literal in export specifier"));
            };
            *index += 1;
            let mut alias = None;
            let mut alias_is_string = false;
            if *index < t.len() {
                let is_as = match &t[*index].token {
                    _ if token_is_escaped_identifier_name(&t[*index], "as") => {
                        return Err(raise_parse_error!("Keyword 'as' must not contain Unicode escape sequences"));
                    }
                    _ if token_matches_unescaped_identifier_name(&t[*index], "as") => true,
                    _ => false,
                };
                if is_as {
                    *index += 1;
                    if *index < t.len() {
                        if let Some(id_name) = t[*index].token.as_identifier_string() {
                            alias = Some(id_name);
                            *index += 1;
                        } else if let Token::StringLit(s) = &t[*index].token {
                            check_module_export_name_well_formed(s)?;
                            alias = Some(utf16_to_utf8(s));
                            alias_is_string = true;
                            *index += 1;
                        } else {
                            return Err(raise_parse_error!("Expected identifier or string literal after as"));
                        }
                    } else {
                        return Err(raise_parse_error!("Expected identifier or string literal after as"));
                    }
                }
            }
            if name_is_string {
                has_string_source_name = true;
            }
            let _ = alias_is_string;
            specifiers.push(ExportSpecifier::Named(name, alias));
            if *index < t.len() && matches!(t[*index].token, Token::Comma) {
                *index += 1;
            }
        }
        if *index < t.len() {
            let is_from = if let Token::Identifier(from_kw) = &t[*index].token {
                *from_kw == "from" && !raw_identifier_source_has_escape(&t[*index])
            } else {
                false
            };
            if is_from {
                *index += 1;
                if *index < t.len() {
                    if let Token::StringLit(s) = &t[*index].token {
                        source = Some(utf16_to_utf8(s));
                        *index += 1;
                    } else {
                        return Err(raise_parse_error!("Expected module specifier"));
                    }
                }
            }
        }
        if source.is_none() && has_string_source_name {
            return Err(raise_syntax_error!(
                "A string literal cannot be used as an exported binding without `from`"
            ));
        }
        consume_import_attributes_clause(t, index)?;
        finish_statement_without_semicolon(t, index)?;
    } else {
        let stmt = match t[*index].token {
            Token::Var => parse_var_statement(t, index)?,
            Token::Let => parse_let_statement(t, index)?,
            Token::Const => parse_const_statement(t, index)?,
            Token::Function | Token::FunctionStar | Token::Async => parse_function_declaration(t, index)?,
            Token::Class => parse_class_declaration(t, index)?,
            _ => return Err(raise_parse_error!("Unexpected token in export statement")),
        };
        inner_stmt = Some(Box::new(stmt));
    }
    // Track exported names for duplicate detection in module code
    if in_module_context() {
        for spec in &specifiers {
            match spec {
                ExportSpecifier::Named(name, alias) => {
                    let exported = alias.as_deref().unwrap_or(name);
                    add_exported_name(exported)?;
                }
                ExportSpecifier::Namespace(name) => {
                    add_exported_name(name)?;
                }
                ExportSpecifier::Default(_) => {
                    add_exported_name("default")?;
                }
                ExportSpecifier::Star => {} // re-exports all, no specific name to track
            }
        }
        // Track names declared by `export var/let/const/function/class`
        if let Some(ref stmt) = inner_stmt {
            match stmt.kind.as_ref() {
                StatementKind::Var(decls) | StatementKind::Let(decls) => {
                    for decl in decls {
                        add_exported_name(&decl.0)?;
                    }
                }
                StatementKind::Const(decls) => {
                    for decl in decls {
                        add_exported_name(&decl.0)?;
                    }
                }
                StatementKind::FunctionDeclaration(name, ..) => {
                    add_exported_name(name)?;
                }
                StatementKind::Class(def) => {
                    if !def.name.is_empty() {
                        add_exported_name(&def.name)?;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(Statement {
        kind: Box::new(StatementKind::Export(specifiers, inner_stmt, source)),
        line: t[start].line,
        column: t[start].column,
    })
}
/// Parse `using x = expr, y = expr;` declaration
fn parse_using_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    // using declarations are not allowed at the top level of a Script
    if statement_depth() == 1 && !in_module_context() {
        return Err(raise_parse_error!(
            "using declarations are not allowed at the top level of a script",
            t[start].line,
            t[start].column
        ));
    }
    *index += 1;
    let mut decls = Vec::new();
    loop {
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index >= t.len() {
            return Err(raise_parse_error!(
                "Expected identifier in using declaration",
                t[start].line,
                t[start].column
            ));
        }
        let name = match &t[*index].token {
            Token::Identifier(n) => n.clone(),
            Token::Await if !in_await_context() && !forbid_await_identifier() => "await".to_string(),
            Token::Yield if !in_generator_context() => "yield".to_string(),
            _ => {
                return Err(raise_parse_error_with_token!(
                    t.get(*index).unwrap(),
                    "Expected identifier in using declaration"
                ));
            }
        };
        *index += 1;
        if *index >= t.len() || !matches!(t[*index].token, Token::Assign) {
            return Err(raise_parse_error!(
                "using declarations must have an initializer",
                t[start].line,
                t[start].column
            ));
        }
        *index += 1;
        let init = parse_assignment(t, index)?;
        decls.push((name, init));
        if *index < t.len() && matches!(t[*index].token, Token::Comma) {
            *index += 1;
        } else {
            break;
        }
    }
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
    }
    Ok(Statement {
        kind: Box::new(StatementKind::Using(decls)),
        line: t[start].line,
        column: t[start].column,
    })
}
/// Parse `await using x = expr;` declaration
fn parse_await_using_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    // await using declarations are not allowed at the top level of a Script
    if statement_depth() == 1 && !in_module_context() {
        return Err(raise_parse_error!(
            "await using declarations are not allowed at the top level of a script",
            t[start].line,
            t[start].column
        ));
    }
    *index += 2;
    let mut decls = Vec::new();
    loop {
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index >= t.len() {
            return Err(raise_parse_error!(
                "Expected identifier in await using declaration",
                t[start].line,
                t[start].column
            ));
        }
        let name = match &t[*index].token {
            Token::Identifier(n) => n.clone(),
            Token::Yield if !in_generator_context() => "yield".to_string(),
            _ => {
                return Err(raise_parse_error_with_token!(
                    t.get(*index).unwrap(),
                    "Expected identifier in await using declaration"
                ));
            }
        };
        *index += 1;
        if *index >= t.len() || !matches!(t[*index].token, Token::Assign) {
            return Err(raise_parse_error!(
                "await using declarations must have an initializer",
                t[start].line,
                t[start].column
            ));
        }
        *index += 1;
        let init = parse_assignment(t, index)?;
        decls.push((name, init));
        if *index < t.len() && matches!(t[*index].token, Token::Comma) {
            *index += 1;
        } else {
            break;
        }
    }
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
    }
    Ok(Statement {
        kind: Box::new(StatementKind::AwaitUsing(decls)),
        line: t[start].line,
        column: t[start].column,
    })
}
fn parse_variable_declaration_list(t: &[TokenData], index: &mut usize) -> Result<Vec<(String, Option<Expr>)>, JSError> {
    let mut decls = Vec::new();
    loop {
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        #[allow(unused_assignments)]
        let mut saw_lt_after_init = false;
        match &t[*index].token {
            Token::Identifier(name) => {
                let name = name.clone();
                if is_reserved_identifier(&name) {
                    return Err(raise_parse_error_with_token!(
                        t[*index],
                        format!("'{}' is a reserved word and cannot be used as an identifier", name)
                    ));
                }
                // Strict mode: 'eval' and 'arguments' cannot be used as binding names
                if strict_binding_checks() && (name == "eval" || name == "arguments") {
                    return Err(raise_parse_error_with_token!(
                        t[*index],
                        format!("'{}' can't be defined or assigned to in strict mode code", name)
                    ));
                }
                if name == "await" && forbid_await_identifier() {
                    return Err(raise_parse_error_with_token!(
                        t[*index],
                        "'await' cannot be used as an identifier here"
                    ));
                }
                *index += 1;
                let had_lt = *index < t.len() && matches!(t[*index].token, Token::LineTerminator);
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                let init = if *index < t.len() && matches!(t[*index].token, Token::Assign) {
                    *index += 1;
                    Some(parse_assignment(t, index)?)
                } else {
                    None
                };
                let has_init = init.is_some();
                decls.push((name, init));
                saw_lt_after_init = if has_init {
                    *index < t.len() && matches!(t[*index].token, Token::LineTerminator)
                } else {
                    had_lt
                };
            }
            Token::Await => {
                if forbid_await_identifier() {
                    return Err(raise_parse_error_with_token!(
                        t[*index],
                        "'await' cannot be used as an identifier here"
                    ));
                }
                let name = "await".to_string();
                *index += 1;
                let had_lt = *index < t.len() && matches!(t[*index].token, Token::LineTerminator);
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                let init = if *index < t.len() && matches!(t[*index].token, Token::Assign) {
                    *index += 1;
                    Some(parse_assignment(t, index)?)
                } else {
                    None
                };
                let has_init = init.is_some();
                decls.push((name, init));
                saw_lt_after_init = if has_init {
                    *index < t.len() && matches!(t[*index].token, Token::LineTerminator)
                } else {
                    had_lt
                };
            }
            Token::Async => {
                let name = "async".to_string();
                *index += 1;
                let had_lt = *index < t.len() && matches!(t[*index].token, Token::LineTerminator);
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                let init = if *index < t.len() && matches!(t[*index].token, Token::Assign) {
                    *index += 1;
                    Some(parse_assignment(t, index)?)
                } else {
                    None
                };
                let has_init = init.is_some();
                decls.push((name, init));
                saw_lt_after_init = if has_init {
                    *index < t.len() && matches!(t[*index].token, Token::LineTerminator)
                } else {
                    had_lt
                };
            }
            Token::As => {
                let name = "as".to_string();
                *index += 1;
                let had_lt = *index < t.len() && matches!(t[*index].token, Token::LineTerminator);
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                let init = if *index < t.len() && matches!(t[*index].token, Token::Assign) {
                    *index += 1;
                    Some(parse_assignment(t, index)?)
                } else {
                    None
                };
                let has_init = init.is_some();
                decls.push((name, init));
                saw_lt_after_init = if has_init {
                    *index < t.len() && matches!(t[*index].token, Token::LineTerminator)
                } else {
                    had_lt
                };
            }
            _ if matches!(t[*index].token, Token::Static) => {
                if strict_binding_checks() {
                    return Err(raise_parse_error_with_token!(
                        t[*index],
                        "'static' is a reserved word and cannot be used as an identifier"
                    ));
                }
                // In sloppy mode, `static` is a valid identifier
                let name = "static".to_string();
                *index += 1;
                let had_lt = *index < t.len() && matches!(t[*index].token, Token::LineTerminator);
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                let init = if *index < t.len() && matches!(t[*index].token, Token::Assign) {
                    *index += 1;
                    Some(parse_assignment(t, index)?)
                } else {
                    None
                };
                let has_init = init.is_some();
                decls.push((name, init));
                saw_lt_after_init = if has_init {
                    *index < t.len() && matches!(t[*index].token, Token::LineTerminator)
                } else {
                    had_lt
                };
            }
            _ => break,
        }
        // Save index before consuming line terminators so we can restore for ASI
        let saved_idx = *index;
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index < t.len() && matches!(t[*index].token, Token::Comma) {
            *index += 1;
        } else {
            // No comma: check ASI validity. If no line terminator was seen after
            // the initializer, the next token must be ; or } or EOF or a token
            // that legitimately follows in for-in/for-of context.
            if !saw_lt_after_init
                && *index < t.len()
                && !matches!(
                    t[*index].token,
                    Token::Semicolon | Token::RBrace | Token::EOF | Token::In | Token::RParen
                )
                && !matches!(&t[*index].token, Token::Identifier(n) if n == "of")
            {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            // Restore index so caller can see LineTerminators for ASI
            *index = saved_idx;
            break;
        }
    }
    if decls.is_empty() {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    Ok(decls)
}
pub fn parse_simple_expression(t: &[crate::core::TokenData], i: usize) -> Result<(Expr, usize), JSError> {
    let mut index = i;
    let expr = parse_expression(t, &mut index)?;
    Ok((expr, index))
}
pub fn parse_statement(t: &mut [TokenData]) -> Result<Statement, JSError> {
    if t.is_empty() {
        return Err(raise_parse_error!("No tokens to parse"));
    }
    let mut index = 0;
    parse_statement_item(t, &mut index)
}
pub fn parse_full_expression(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    log::trace!(
        "parse_full_expression: tokens after initial skip (first 8): {:?}",
        tokens.iter().take(8).collect::<Vec<_>>()
    );
    if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
        let mut depth = 1usize;
        let mut j = *index + 1;
        while j < tokens.len() && depth > 0 {
            match tokens[j].token {
                Token::LParen => depth += 1,
                Token::RParen => depth -= 1,
                _ => {}
            }
            if depth > 0 {
                j += 1;
            }
        }
        if depth == 0 {
            let next = j + 1;
            if next < tokens.len() && matches!(tokens[next].token, Token::Arrow) {
                log::trace!(
                    "parse_full_expr paren-scan: index={}, j={} token_j={:?} next={} token_next={:?}",
                    *index,
                    j,
                    tokens.get(j),
                    next,
                    tokens.get(next)
                );
                let mut t = *index + 1;
                log::trace!(
                    "parse_full_expr: calling parse_parameters with t={} token_at_t={:?}",
                    t,
                    tokens.get(t)
                );
                match parse_parameters(tokens, &mut t) {
                    Ok(params) => {
                        log::trace!("parse_full_expr: parse_parameters returned params={:?} t_after={}", params, t);
                        if t == j + 1 {
                            *index = next + 1;
                            let body = parse_arrow_body(tokens, index)?;
                            log::trace!("constructing arrow (full-expression precheck) params={:?}", params);
                            return Ok(Expr::ArrowFunction(params, body));
                        } else {
                            log::trace!(
                                "parse_full_expr: t_after ({}) != j+1 ({}), not treating as arrow parameter list",
                                t,
                                j + 1
                            );
                        }
                    }
                    Err(e) => {
                        log::trace!("parse_full_expr: parse_parameters failed at t={} err={:?}", t, e);
                    }
                }
            }
        }
    }
    let left = parse_assignment(tokens, index)?;
    Ok(left)
}
fn parse_binary_op<F, M>(tokens: &[TokenData], index: &mut usize, parse_next_level: F, op_mapper: M) -> Result<Expr, JSError>
where
    F: Fn(&[TokenData], &mut usize) -> Result<Expr, JSError>,
    M: Fn(&Token) -> Option<BinaryOp>,
{
    let mut left = parse_next_level(tokens, index)?;
    loop {
        let mut look = *index;
        while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
            look += 1;
        }
        if look >= tokens.len() {
            break;
        }
        if let Some(op) = op_mapper(&tokens[look].token) {
            *index = look + 1;
            let right = parse_next_level(tokens, index)?;
            left = Expr::Binary(Box::new(left), op, Box::new(right));
        } else {
            break;
        }
    }
    Ok(left)
}
fn flatten_commas(expr: Expr) -> Vec<Expr> {
    match expr {
        Expr::Comma(l, r) => {
            let mut out = flatten_commas(*l);
            out.extend(flatten_commas(*r));
            out
        }
        other => vec![other],
    }
}
fn contains_import_meta_expr(e: &Expr) -> bool {
    match e {
        Expr::Property(boxed, prop) => {
            if let Expr::Var(name, _, _) = &**boxed
                && name == "import"
                && prop == "meta"
            {
                return true;
            }
            contains_import_meta_expr(boxed)
        }
        Expr::Assign(left, right) => contains_import_meta_expr(left) || contains_import_meta_expr(right),
        Expr::Binary(left, _, right) => contains_import_meta_expr(left) || contains_import_meta_expr(right),
        Expr::Conditional(c, t, f) => contains_import_meta_expr(c) || contains_import_meta_expr(t) || contains_import_meta_expr(f),
        Expr::Call(f, args) => {
            if contains_import_meta_expr(f) {
                return true;
            }
            for a in args {
                if contains_import_meta_expr(a) {
                    return true;
                }
            }
            false
        }
        Expr::TaggedTemplate(f, ..) => contains_import_meta_expr(f),
        Expr::Index(obj, key) => contains_import_meta_expr(obj) || contains_import_meta_expr(key),
        Expr::UnaryNeg(inner) | Expr::UnaryPlus(inner) | Expr::TypeOf(inner) | Expr::Void(inner) => contains_import_meta_expr(inner),
        _ => false,
    }
}
pub fn parse_parameters(tokens: &[TokenData], index: &mut usize) -> Result<Vec<DestructuringElement>, JSError> {
    let mut params = Vec::new();
    log::trace!("parse_parameters called with index={}", *index);
    log::trace!(
        "parse_parameters: starting tokens (first 16): {:?}",
        tokens.iter().take(16).collect::<Vec<_>>()
    );
    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < tokens.len() && !matches!(tokens[*index].token, Token::RParen) {
        loop {
            if matches!(tokens[*index].token, Token::Spread) {
                *index += 1;
                if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
                    *index += 1;
                    log::trace!("parse_parameters: found rest parameter name={}", name);
                    params.push(DestructuringElement::Rest(name));
                    if *index >= tokens.len() {
                        return Err(raise_parse_error!("Unexpected end of parameters after rest"));
                    }
                    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                        *index += 1;
                    }
                    if !matches!(tokens[*index].token, Token::RParen) {
                        let msg = "Rest parameter must be last formal parameter";
                        return Err(raise_parse_error_with_token!(tokens[*index], msg));
                    }
                    break;
                } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                    let pattern = parse_array_destructuring_pattern(tokens, index)?;
                    let inner = DestructuringElement::NestedArray(pattern, None);
                    params.push(DestructuringElement::RestPattern(Box::new(inner)));
                    if *index >= tokens.len() {
                        return Err(raise_parse_error!("Unexpected end of parameters after rest"));
                    }
                    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                        *index += 1;
                    }
                    if !matches!(tokens[*index].token, Token::RParen) {
                        let msg = "Rest parameter must be last formal parameter";
                        return Err(raise_parse_error_with_token!(tokens[*index], msg));
                    }
                    break;
                } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
                    let pattern = parse_object_destructuring_pattern(tokens, index)?;
                    let inner = DestructuringElement::NestedObject(pattern, None);
                    params.push(DestructuringElement::RestPattern(Box::new(inner)));
                    if *index >= tokens.len() {
                        return Err(raise_parse_error!("Unexpected end of parameters after rest"));
                    }
                    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                        *index += 1;
                    }
                    if !matches!(tokens[*index].token, Token::RParen) {
                        let msg = "Rest parameter must be last formal parameter";
                        return Err(raise_parse_error_with_token!(tokens[*index], msg));
                    }
                    break;
                } else {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
            } else if matches!(tokens[*index].token, Token::LBrace) {
                let pattern = parse_object_destructuring_pattern(tokens, index)?;
                let mut default_expr: Option<Box<Expr>> = None;
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                    *index += 1;
                    let expr = parse_assignment(tokens, index)?;
                    if contains_import_meta_expr(&expr) {
                        return Err(raise_parse_error_with_token!(
                            tokens.get(*index - 1).unwrap(),
                            "import.meta is not allowed in parameter initializers"
                        ));
                    }
                    default_expr = Some(Box::new(expr));
                }
                params.push(DestructuringElement::NestedObject(pattern, default_expr));
            } else if matches!(tokens[*index].token, Token::LBracket) {
                let pattern = parse_array_destructuring_pattern(tokens, index)?;
                let mut default_expr: Option<Box<Expr>> = None;
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                    *index += 1;
                    let expr = parse_assignment(tokens, index)?;
                    if contains_import_meta_expr(&expr) {
                        let token = tokens.get(*index - 1).unwrap();
                        return Err(raise_parse_error_with_token!(
                            token,
                            "import.meta is not allowed in parameter initializers"
                        ));
                    }
                    default_expr = Some(Box::new(expr));
                }
                params.push(DestructuringElement::NestedArray(pattern, default_expr));
            } else if let Some(Token::Identifier(param)) = tokens.get(*index).map(|t| &t.token).cloned() {
                // Strict mode: 'eval' and 'arguments' cannot be used as parameter names
                if strict_binding_checks() && (param == "eval" || param == "arguments") {
                    return Err(raise_parse_error_with_token!(
                        tokens[*index],
                        format!("'{}' can't be defined or assigned to in strict mode code", param)
                    ));
                }
                *index += 1;
                let mut default_expr: Option<Box<Expr>> = None;
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                    *index += 1;
                    let expr = parse_assignment(tokens, index)?;
                    if contains_import_meta_expr(&expr) {
                        return Err(raise_parse_error_with_token!(
                            tokens.get(*index - 1).unwrap(),
                            "import.meta is not allowed in parameter initializers"
                        ));
                    }
                    default_expr = Some(Box::new(expr));
                }
                params.push(DestructuringElement::Variable(param, default_expr));
            } else if matches!(tokens[*index].token, Token::Await) && !forbid_await_identifier() {
                *index += 1;
                let param = "await".to_string();
                let mut default_expr: Option<Box<Expr>> = None;
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                    *index += 1;
                    let expr = parse_assignment(tokens, index)?;
                    if contains_import_meta_expr(&expr) {
                        return Err(raise_parse_error_with_token!(
                            tokens.get(*index - 1).unwrap(),
                            "import.meta is not allowed in parameter initializers"
                        ));
                    }
                    default_expr = Some(Box::new(expr));
                }
                params.push(DestructuringElement::Variable(param, default_expr));
            } else if matches!(tokens[*index].token, Token::Async) {
                *index += 1;
                let param = "async".to_string();
                let mut default_expr: Option<Box<Expr>> = None;
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                    *index += 1;
                    let expr = parse_assignment(tokens, index)?;
                    if contains_import_meta_expr(&expr) {
                        return Err(raise_parse_error_with_token!(
                            tokens.get(*index - 1).unwrap(),
                            "import.meta is not allowed in parameter initializers"
                        ));
                    }
                    default_expr = Some(Box::new(expr));
                }
                params.push(DestructuringElement::Variable(param, default_expr));
            } else {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            if *index >= tokens.len() {
                return Err(raise_parse_error!("Unexpected end of parameters"));
            }
            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                *index += 1;
            }
            if matches!(tokens[*index].token, Token::RParen) {
                break;
            }
            if !matches!(tokens[*index].token, Token::Comma) {
                return Err(raise_parse_error_with_token!(tokens[*index], "Expected ',' in parameter list"));
            }
            *index += 1;
            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                *index += 1;
            }
            if *index < tokens.len() && matches!(tokens[*index].token, Token::RParen) {
                break;
            }
        }
    }
    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(tokens.get(*index)));
    }
    *index += 1;
    log::trace!(
        "parse_parameters: consumed ')', remaining tokens (first 16): {:?}",
        tokens.iter().take(16).collect::<Vec<_>>()
    );
    log::trace!("parse_parameters: final params={:?}", params);
    Ok(params)
}
pub fn parse_statement_block(tokens: &[TokenData], index: &mut usize) -> Result<Vec<Statement>, JSError> {
    let body = parse_statements(tokens, index)?;
    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
        return Err(raise_parse_error_at!(tokens.get(*index)));
    }
    *index += 1;
    Ok(body)
}
pub fn parse_expression(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    log::trace!("parse_expression: entry index={} token_at_index={:?}", *index, tokens.get(*index));
    let mut left = parse_full_expression(tokens, index)?;
    loop {
        let mut look = *index;
        while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
            look += 1;
        }
        if look >= tokens.len() || !matches!(tokens[look].token, Token::Comma) {
            break;
        }
        *index = look + 1;
        let right = parse_full_expression(tokens, index)?;
        left = Expr::Comma(Box::new(left), Box::new(right));
    }
    Ok(left)
}
pub fn parse_conditional(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    let condition = parse_nullish(tokens, index)?;
    let mut look = *index;
    while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
        look += 1;
    }
    if look >= tokens.len() {
        return Ok(condition);
    }
    if matches!(tokens[look].token, Token::QuestionMark) {
        *index = look + 1;
        let true_expr = with_allowed_in(|| parse_assignment(tokens, index))?;
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::Colon) {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
        *index += 1;
        let false_expr = parse_assignment(tokens, index)?;
        Ok(Expr::Conditional(Box::new(condition), Box::new(true_expr), Box::new(false_expr)))
    } else {
        Ok(condition)
    }
}

#[allow(clippy::type_complexity)]
fn get_assignment_ctor(token: &Token) -> Option<fn(Box<Expr>, Box<Expr>) -> Expr> {
    match token {
        Token::Assign => Some(Expr::Assign),
        Token::LogicalAndAssign => Some(Expr::LogicalAndAssign),
        Token::LogicalOrAssign => Some(Expr::LogicalOrAssign),
        Token::NullishAssign => Some(Expr::NullishAssign),
        Token::AddAssign => Some(Expr::AddAssign),
        Token::SubAssign => Some(Expr::SubAssign),
        Token::PowAssign => Some(Expr::PowAssign),
        Token::MulAssign => Some(Expr::MulAssign),
        Token::DivAssign => Some(Expr::DivAssign),
        Token::ModAssign => Some(Expr::ModAssign),
        Token::BitXorAssign => Some(Expr::BitXorAssign),
        Token::BitAndAssign => Some(Expr::BitAndAssign),
        Token::BitOrAssign => Some(Expr::BitOrAssign),
        Token::LeftShiftAssign => Some(Expr::LeftShiftAssign),
        Token::RightShiftAssign => Some(Expr::RightShiftAssign),
        Token::UnsignedRightShiftAssign => Some(Expr::UnsignedRightShiftAssign),
        _ => None,
    }
}
fn contains_optional_chain(e: &Expr) -> bool {
    match e {
        Expr::OptionalProperty(_, _) | Expr::OptionalPrivateMember(_, _) | Expr::OptionalIndex(_, _) | Expr::OptionalCall(_, _) => true,
        Expr::Property(obj, _) => contains_optional_chain(obj.as_ref()),
        Expr::Index(obj, idx) => contains_optional_chain(obj.as_ref()) || contains_optional_chain(idx.as_ref()),
        Expr::Call(obj, _) => contains_optional_chain(obj.as_ref()),
        _ => false,
    }
}
fn parse_array_assignment_pattern(tokens: &[TokenData], index: &mut usize) -> Result<Vec<Option<Expr>>, JSError> {
    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBracket) {
        return Err(raise_parse_error_at!(tokens.get(*index)));
    }
    *index += 1;
    let mut elements: Vec<Option<Expr>> = Vec::new();
    if *index < tokens.len() && matches!(tokens[*index].token, Token::RBracket) {
        *index += 1;
        return Ok(elements);
    }
    loop {
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index >= tokens.len() {
            return Err(raise_parse_error_at!(tokens.last()));
        }
        if matches!(tokens[*index].token, Token::RBracket) {
            *index += 1;
            break;
        }
        if matches!(tokens[*index].token, Token::Comma) {
            elements.push(None);
            *index += 1;
            continue;
        }
        if matches!(tokens[*index].token, Token::Spread) {
            *index += 1;
            let rest_expr = if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                let saved = *index;
                match parse_array_assignment_pattern(tokens, index) {
                    Ok(inner) => {
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket | Token::Dot) {
                            *index = saved;
                            parse_assignment(tokens, index)?
                        } else {
                            Expr::Array(inner)
                        }
                    }
                    Err(_) => {
                        *index = saved;
                        parse_assignment(tokens, index)?
                    }
                }
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
                let saved = *index;
                match parse_object_assignment_pattern(tokens, index) {
                    Ok(inner) => {
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket | Token::Dot) {
                            *index = saved;
                            parse_assignment(tokens, index)?
                        } else {
                            Expr::Object(inner)
                        }
                    }
                    Err(_) => {
                        *index = saved;
                        parse_assignment(tokens, index)?
                    }
                }
            } else {
                parse_assignment(tokens, index)?
            };
            if matches!(rest_expr, Expr::Assign(..)) {
                return Err(raise_parse_error!("SyntaxError: Rest element may not have a default initializer"));
            }
            elements.push(Some(Expr::Spread(Box::new(rest_expr))));
            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            *index += 1;
            break;
        }
        let mut elem_expr = if matches!(tokens[*index].token, Token::LBracket) {
            let saved = *index;
            match parse_array_assignment_pattern(tokens, index) {
                Ok(inner) => {
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket | Token::Dot) {
                        *index = saved;
                        parse_assignment(tokens, index)?
                    } else {
                        Expr::Array(inner)
                    }
                }
                Err(_) => {
                    *index = saved;
                    parse_assignment(tokens, index)?
                }
            }
        } else if matches!(tokens[*index].token, Token::LBrace) {
            let saved = *index;
            match parse_object_assignment_pattern(tokens, index) {
                Ok(inner) => {
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket | Token::Dot) {
                        *index = saved;
                        parse_assignment(tokens, index)?
                    } else {
                        Expr::Object(inner)
                    }
                }
                Err(_) => {
                    *index = saved;
                    parse_assignment(tokens, index)?
                }
            }
        } else {
            parse_assignment(tokens, index)?
        };
        if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
            *index += 1;
            let default_expr = parse_assignment(tokens, index)?;
            elem_expr = Expr::Assign(Box::new(elem_expr), Box::new(default_expr));
        }
        elements.push(Some(elem_expr));
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index >= tokens.len() {
            return Err(raise_parse_error_at!(tokens.last()));
        }
        if matches!(tokens[*index].token, Token::Comma) {
            *index += 1;
            continue;
        }
        if matches!(tokens[*index].token, Token::RBracket) {
            *index += 1;
            break;
        }
        return Err(raise_parse_error_at!(tokens.get(*index)));
    }
    Ok(elements)
}
fn parse_object_assignment_pattern(tokens: &[TokenData], index: &mut usize) -> Result<Vec<(Expr, Expr, bool, bool)>, JSError> {
    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
        return Err(raise_parse_error_at!(tokens.get(*index)));
    }
    *index += 1;
    let mut properties: Vec<(Expr, Expr, bool, bool)> = Vec::new();
    if *index < tokens.len() && matches!(tokens[*index].token, Token::RBrace) {
        *index += 1;
        return Ok(properties);
    }
    loop {
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index >= tokens.len() {
            return Err(raise_parse_error_at!(tokens.last()));
        }
        if matches!(tokens[*index].token, Token::RBrace) {
            *index += 1;
            break;
        }
        if matches!(tokens[*index].token, Token::Spread) {
            *index += 1;
            let rest_expr = if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                let saved = *index;
                match parse_array_assignment_pattern(tokens, index) {
                    Ok(inner) => {
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket | Token::Dot) {
                            *index = saved;
                            parse_assignment(tokens, index)?
                        } else {
                            Expr::Array(inner)
                        }
                    }
                    Err(_) => {
                        *index = saved;
                        parse_assignment(tokens, index)?
                    }
                }
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
                let saved = *index;
                match parse_object_assignment_pattern(tokens, index) {
                    Ok(inner) => {
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket | Token::Dot) {
                            *index = saved;
                            parse_assignment(tokens, index)?
                        } else {
                            Expr::Object(inner)
                        }
                    }
                    Err(_) => {
                        *index = saved;
                        parse_assignment(tokens, index)?
                    }
                }
            } else {
                parse_assignment(tokens, index)?
            };
            properties.push((Expr::StringLit(Vec::new()), rest_expr, true, false));
            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            *index += 1;
            break;
        }
        let mut key_name: Option<String> = None;
        let mut key_expr: Option<Expr> = None;
        let mut is_identifier_key = false;
        if matches!(tokens[*index].token, Token::LBracket) {
            *index += 1;
            let expr = parse_assignment(tokens, index)?;
            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            *index += 1;
            key_expr = Some(expr);
        } else if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
            *index += 1;
            key_name = Some(name);
            is_identifier_key = true;
        } else if let Some(Token::Number(n)) = tokens.get(*index).map(|t| t.token.clone()) {
            *index += 1;
            key_name = Some(n.to_string());
        } else if let Some(Token::BigInt(s)) = tokens.get(*index).map(|t| t.token.clone()) {
            *index += 1;
            key_name = Some(s);
        } else if let Some(Token::StringLit(s)) = tokens.get(*index).map(|t| t.token.clone()) {
            *index += 1;
            key_name = Some(utf16_to_utf8(&s));
        } else if let Some(tok) = tokens.get(*index).map(|t| t.token.clone()) {
            if let Some(id) = tok.as_identifier_string() {
                *index += 1;
                key_name = Some(id);
                is_identifier_key = true;
            } else if let Some(Token::Default) = tokens.get(*index).map(|t| t.token.clone()) {
                *index += 1;
                key_name = Some("default".to_string());
            } else {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
        } else {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
        let key_expr_final = if let Some(expr) = key_expr {
            expr
        } else if let Some(name) = key_name.clone() {
            Expr::StringLit(crate::unicode::utf8_to_utf16(&name))
        } else {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        };
        let target_expr = if *index < tokens.len() && matches!(tokens[*index].token, Token::Colon) {
            *index += 1;
            let mut value_expr = if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                let saved = *index;
                match parse_array_assignment_pattern(tokens, index) {
                    Ok(inner) => {
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket | Token::Dot) {
                            *index = saved;
                            parse_assignment(tokens, index)?
                        } else {
                            Expr::Array(inner)
                        }
                    }
                    Err(_) => {
                        *index = saved;
                        parse_assignment(tokens, index)?
                    }
                }
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
                let saved = *index;
                match parse_object_assignment_pattern(tokens, index) {
                    Ok(inner) => {
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket | Token::Dot) {
                            *index = saved;
                            parse_assignment(tokens, index)?
                        } else {
                            Expr::Object(inner)
                        }
                    }
                    Err(_) => {
                        *index = saved;
                        parse_assignment(tokens, index)?
                    }
                }
            } else {
                parse_assignment(tokens, index)?
            };
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                *index += 1;
                let default_expr = parse_assignment(tokens, index)?;
                value_expr = Expr::Assign(Box::new(value_expr), Box::new(default_expr));
            }
            value_expr
        } else {
            if !is_identifier_key {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            let name = key_name.unwrap_or_default();
            let mut expr = Expr::Var(name.clone(), None, None);
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                *index += 1;
                let default_expr = parse_assignment(tokens, index)?;
                expr = Expr::Assign(Box::new(expr), Box::new(default_expr));
            }
            expr
        };
        properties.push((key_expr_final, target_expr, false, false));
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index >= tokens.len() {
            return Err(raise_parse_error_at!(tokens.last()));
        }
        if matches!(tokens[*index].token, Token::Comma) {
            *index += 1;
            continue;
        }
        if matches!(tokens[*index].token, Token::RBrace) {
            *index += 1;
            break;
        }
        return Err(raise_parse_error_at!(tokens.get(*index)));
    }
    Ok(properties)
}

fn find_pattern_end(tokens: &[TokenData], start: usize) -> Option<usize> {
    let mut stack = vec![match tokens.get(start)?.token {
        Token::LBracket => '[',
        Token::LBrace => '{',
        _ => return None,
    }];
    let mut index = start + 1;
    while index < tokens.len() {
        match tokens[index].token {
            Token::LParen => stack.push('('),
            Token::LBracket => stack.push('['),
            Token::LBrace => stack.push('{'),
            Token::RParen => {
                if matches!(stack.last(), Some('(')) {
                    stack.pop();
                }
            }
            Token::RBracket => {
                if matches!(stack.last(), Some('[')) {
                    stack.pop();
                    if stack.is_empty() {
                        return Some(index);
                    }
                }
            }
            Token::RBrace => {
                if matches!(stack.last(), Some('{')) {
                    stack.pop();
                    if stack.is_empty() {
                        return Some(index);
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

pub fn parse_assignment(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    log::trace!("parse_assignment: entry index={} token={:?}", *index, tokens.get(*index));
    // YieldExpression is at the AssignmentExpression level (not primary)
    if *index < tokens.len() && matches!(tokens[*index].token, Token::Yield | Token::YieldStar) {
        if !in_generator_context() {
            // In strict mode, yield is always reserved and cannot be an identifier
            return Err(raise_parse_error_with_token!(tokens[*index], "Unexpected yield"));
        }
        let is_star = matches!(tokens[*index].token, Token::YieldStar);
        *index += 1;
        if is_star {
            let inner = parse_assignment(tokens, index)?;
            return Ok(Expr::YieldStar(Box::new(inner)));
        }
        // yield * (separate Multiply token)
        if *index < tokens.len() && matches!(tokens[*index].token, Token::Multiply) {
            *index += 1;
            let inner = parse_assignment(tokens, index)?;
            return Ok(Expr::YieldStar(Box::new(inner)));
        }
        // yield [no LineTerminator] AssignmentExpression
        if *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            let mut look = *index;
            while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
                look += 1;
            }
            if look < tokens.len() && matches!(tokens[look].token, Token::Multiply) {
                return Err(raise_parse_error_with_token!(tokens[look], "Unexpected * after line terminator"));
            }
            return Ok(Expr::Yield(None));
        }
        if *index >= tokens.len()
            || matches!(
                tokens[*index].token,
                Token::Semicolon | Token::Comma | Token::RParen | Token::RBracket | Token::RBrace | Token::Colon
            )
        {
            return Ok(Expr::Yield(None));
        }
        let inner = parse_assignment(tokens, index)?;
        return Ok(Expr::Yield(Some(Box::new(inner))));
    }
    if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace | Token::LBracket) {
        let mut idx = *index;
        let pattern_expr_res = if matches!(tokens[idx].token, Token::LBracket) {
            parse_array_assignment_pattern(tokens, &mut idx).map(Expr::Array)
        } else {
            parse_object_assignment_pattern(tokens, &mut idx).map(Expr::Object)
        };
        match pattern_expr_res {
            Ok(pattern_expr) => {
                let mut idx2 = idx;
                while idx2 < tokens.len() && matches!(tokens[idx2].token, Token::LineTerminator) {
                    idx2 += 1;
                }
                if idx2 < tokens.len() && matches!(tokens[idx2].token, Token::Assign) {
                    *index = idx2 + 1;
                    let right = parse_assignment(tokens, index)?;
                    return Ok(Expr::Assign(Box::new(pattern_expr), Box::new(right)));
                }
            }
            Err(pattern_err) => {
                if let Some(pattern_end) = find_pattern_end(tokens, *index) {
                    let mut idx2 = pattern_end + 1;
                    while idx2 < tokens.len() && matches!(tokens[idx2].token, Token::LineTerminator) {
                        idx2 += 1;
                    }
                    if idx2 < tokens.len() && matches!(tokens[idx2].token, Token::Assign) {
                        return Err(pattern_err);
                    }
                }
            }
        }
    }
    let left = parse_conditional(tokens, index)?;
    let mut look = *index;
    while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
        look += 1;
    }
    if look >= tokens.len() {
        return Ok(left);
    }
    if let Some(ctor) = get_assignment_ctor(&tokens[look].token) {
        if contains_optional_chain(&left) {
            return Err(raise_parse_error_at!(tokens.get(look)));
        }
        // Strict mode: cannot assign to 'eval' or 'arguments'
        if strict_binding_checks()
            && let Expr::Var(ref name, _, _) = left
            && (name == "eval" || name == "arguments")
        {
            return Err(raise_parse_error_with_token!(
                tokens[look.saturating_sub(1)],
                format!("'{}' can't be defined or assigned to in strict mode code", name)
            ));
        }
        *index = look + 1;
        let right = parse_assignment(tokens, index)?;
        return Ok(ctor(Box::new(left), Box::new(right)));
    }
    Ok(left)
}
fn parse_shift(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    parse_binary_op(tokens, index, parse_additive, |token| match token {
        Token::LeftShift => Some(BinaryOp::LeftShift),
        Token::RightShift => Some(BinaryOp::RightShift),
        Token::UnsignedRightShift => Some(BinaryOp::UnsignedRightShift),
        _ => None,
    })
}
fn parse_relational(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    let mut left = parse_shift(tokens, index)?;
    loop {
        let mut look = *index;
        while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
            look += 1;
        }
        if look >= tokens.len() {
            break;
        }
        let op = match &tokens[look].token {
            Token::LessThan => Some(BinaryOp::LessThan),
            Token::GreaterThan => Some(BinaryOp::GreaterThan),
            Token::LessEqual => Some(BinaryOp::LessEqual),
            Token::GreaterEqual => Some(BinaryOp::GreaterEqual),
            Token::InstanceOf => Some(BinaryOp::InstanceOf),
            Token::In if !forbid_in() => Some(BinaryOp::In),
            _ => None,
        };
        if let Some(op) = op {
            // Validate PrivateIdentifier `in` constraints per spec
            if op == BinaryOp::In {
                if let Expr::PrivateName(_) = &left {
                    // PrivateIdentifier in ShiftExpression: validate RHS is not arrow/PrivateName
                    *index = look + 1;
                    let right = parse_shift(tokens, index)?;
                    if matches!(
                        &right,
                        Expr::ArrowFunction(..) | Expr::AsyncArrowFunction(..) | Expr::PrivateName(..)
                    ) {
                        return Err(raise_parse_error!("Invalid right-hand side in private field 'in' expression"));
                    }
                    left = Expr::Binary(Box::new(left), op, Box::new(right));
                    // After PrivateIdentifier `in` expr, the result cannot be the LHS of another `in`
                    // (it's a RelationalExpression, not a PrivateIdentifier)
                    continue;
                }
                // If the LHS already contains a PrivateIdentifier `in` pattern, disallow nested `in`
                if contains_private_in(&left) {
                    return Err(raise_parse_error!(
                        "Cannot nest 'in' expressions when left-hand side is PrivateIdentifier"
                    ));
                }
            }
            *index = look + 1;
            let right = parse_shift(tokens, index)?;
            left = Expr::Binary(Box::new(left), op, Box::new(right));
        } else {
            break;
        }
    }
    Ok(left)
}
fn contains_private_in(expr: &Expr) -> bool {
    match expr {
        Expr::Binary(left, BinaryOp::In, _) => matches!(&**left, Expr::PrivateName(..)),
        _ => false,
    }
}
fn parse_equality(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    parse_binary_op(tokens, index, parse_relational, |token| match token {
        Token::Equal => Some(BinaryOp::Equal),
        Token::StrictEqual => Some(BinaryOp::StrictEqual),
        Token::NotEqual => Some(BinaryOp::NotEqual),
        Token::StrictNotEqual => Some(BinaryOp::StrictNotEqual),
        _ => None,
    })
}
fn parse_bitwise_and(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    parse_binary_op(tokens, index, parse_equality, |token| match token {
        Token::BitAnd => Some(BinaryOp::BitAnd),
        _ => None,
    })
}
fn parse_bitwise_xor_chain(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    parse_binary_op(tokens, index, parse_bitwise_and, |token| match token {
        Token::BitXor => Some(BinaryOp::BitXor),
        _ => None,
    })
}
fn parse_bitwise_or(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    parse_binary_op(tokens, index, parse_bitwise_xor_chain, |token| match token {
        Token::BitOr => Some(BinaryOp::BitOr),
        _ => None,
    })
}
fn parse_logical_and(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    let left = parse_bitwise_or(tokens, index)?;
    let mut look = *index;
    while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
        look += 1;
    }
    if look >= tokens.len() {
        return Ok(left);
    }
    if matches!(tokens[look].token, Token::LogicalAnd) {
        *index = look + 1;
        let right = parse_logical_and(tokens, index)?;
        Ok(Expr::LogicalAnd(Box::new(left), Box::new(right)))
    } else {
        Ok(left)
    }
}
/// Check if an expression used as a for-of/for-in destructuring target contains
/// `eval` or `arguments` as simple assignment targets (strict mode restriction).
fn check_destructuring_expr_strict(expr: &Expr) -> Result<(), JSError> {
    if !strict_binding_checks() {
        return Ok(());
    }
    match expr {
        Expr::Var(name, _, _) if name == "eval" || name == "arguments" || name == "yield" => Err(raise_parse_error!(&format!(
            "'{}' can't be defined or assigned to in strict mode code",
            name
        ))),
        Expr::Array(elements) => {
            for inner in elements.iter().flatten() {
                match inner {
                    Expr::Spread(s) => check_destructuring_expr_strict(s)?,
                    Expr::Assign(lhs, _) => check_destructuring_expr_strict(lhs)?,
                    other => check_destructuring_expr_strict(other)?,
                }
            }
            Ok(())
        }
        Expr::Object(pairs) => {
            for (_, val, _, _) in pairs {
                check_destructuring_expr_strict(val)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
fn parse_logical_or(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    let left = parse_logical_and(tokens, index)?;
    let mut look = *index;
    while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
        look += 1;
    }
    if look >= tokens.len() {
        return Ok(left);
    }
    if matches!(tokens[look].token, Token::LogicalOr) {
        *index = look + 1;
        let right = parse_logical_or(tokens, index)?;
        Ok(Expr::LogicalOr(Box::new(left), Box::new(right)))
    } else {
        Ok(left)
    }
}
/// Check if tokens in range [start..end) contain || or && at paren depth 0.
fn has_bare_logical_in_range(tokens: &[TokenData], start: usize, end: usize) -> bool {
    let mut depth = 0usize;
    for td in tokens.iter().take(end).skip(start) {
        match &td.token {
            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
            Token::RParen | Token::RBracket | Token::RBrace => {
                depth = depth.saturating_sub(1);
            }
            Token::LogicalOr | Token::LogicalAnd if depth == 0 => return true,
            _ => {}
        }
    }
    false
}
fn parse_nullish(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    let start = *index;
    let left = parse_logical_or(tokens, index)?;
    let left_end = *index;
    let mut look = *index;
    while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
        look += 1;
    }
    if look >= tokens.len() {
        return Ok(left);
    }
    if matches!(tokens[look].token, Token::NullishCoalescing) {
        if has_bare_logical_in_range(tokens, start, left_end) {
            return Err(raise_parse_error!(
                "Nullish coalescing operator(??) requires parens when mixed with logical OR/AND",
                tokens[look].line,
                tokens[look].column
            ));
        }
        *index = look + 1;
        let right_start = *index;
        let right = parse_nullish(tokens, index)?;
        let right_end = *index;
        if has_bare_logical_in_range(tokens, right_start, right_end) {
            return Err(raise_parse_error!(
                "Nullish coalescing operator(??) requires parens when mixed with logical OR/AND"
            ));
        }
        Ok(Expr::NullishCoalescing(Box::new(left), Box::new(right)))
    } else {
        Ok(left)
    }
}
fn parse_additive(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    parse_binary_op(tokens, index, parse_multiplicative, |token| match token {
        Token::Plus => Some(BinaryOp::Add),
        Token::Minus => Some(BinaryOp::Sub),
        _ => None,
    })
}
fn parse_multiplicative(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    parse_binary_op(tokens, index, parse_exponentiation, |token| match token {
        Token::Multiply => Some(BinaryOp::Mul),
        Token::Divide => Some(BinaryOp::Div),
        Token::Mod => Some(BinaryOp::Mod),
        _ => None,
    })
}
fn parse_exponentiation(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    let start = *index;
    let left = parse_primary(tokens, index, true)?;
    if *index >= tokens.len() {
        return Ok(left);
    }
    if matches!(tokens[*index].token, Token::Exponent) {
        // Unary operators cannot be the base of exponentiation (spec: UpdateExpression ** ExponentiationExpression)
        // But parenthesized unary expressions are fine: (-1n) ** -1n is valid
        let was_parenthesized = matches!(tokens[start].token, Token::LParen);
        if !was_parenthesized {
            match &left {
                Expr::Delete(_)
                | Expr::Void(_)
                | Expr::TypeOf(_)
                | Expr::UnaryPlus(_)
                | Expr::UnaryNeg(_)
                | Expr::BitNot(_)
                | Expr::LogicalNot(_) => {
                    return Err(crate::raise_syntax_error!(
                        "Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence"
                    ));
                }
                _ => {}
            }
        }
        *index += 1;
        let right = parse_exponentiation(tokens, index)?;
        Ok(Expr::Binary(Box::new(left), BinaryOp::Pow, Box::new(right)))
    } else {
        Ok(left)
    }
}
thread_local! {
    static PARSING_CLASS_DEPTH : Cell < usize > = const { Cell::new(0) }; static
    PRIVATE_NAME_STACK : RefCell < Vec < Rc < RefCell < HashSet < String >>>>> = const {
    RefCell::new(Vec::new()) };
}
struct ClassContextGuard;
impl ClassContextGuard {
    fn new() -> ClassContextGuard {
        PARSING_CLASS_DEPTH.with(|c| c.set(c.get() + 1));
        ClassContextGuard
    }
}
impl Drop for ClassContextGuard {
    fn drop(&mut self) {
        PARSING_CLASS_DEPTH.with(|c| c.set(c.get() - 1));
    }
}
struct ClassPrivateNamesGuard {
    _marker: std::rc::Rc<std::cell::RefCell<std::collections::HashSet<String>>>,
}
impl ClassPrivateNamesGuard {
    fn new(set: std::rc::Rc<std::cell::RefCell<std::collections::HashSet<String>>>) -> ClassPrivateNamesGuard {
        PRIVATE_NAME_STACK.with(|s| s.borrow_mut().push(set.clone()));
        ClassPrivateNamesGuard { _marker: set }
    }
}
impl Drop for ClassPrivateNamesGuard {
    fn drop(&mut self) {
        PRIVATE_NAME_STACK.with(|s| {
            s.borrow_mut().pop();
        });
    }
}
pub fn parse_class_body(t: &[TokenData], index: &mut usize) -> Result<Vec<ClassMember>, JSError> {
    let _guard = ClassContextGuard::new();
    if *index >= t.len() || !matches!(t[*index].token, Token::LBrace) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    let mut members = Vec::new();
    let mut declared_private_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    let current_private_names = std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashSet::new()));
    let _private_guard = ClassPrivateNamesGuard::new(current_private_names.clone());
    {
        let mut pos: usize = *index;
        while pos < t.len() {
            if matches!(t[pos].token, Token::RBrace) {
                break;
            }
            if matches!(t[pos].token, Token::Semicolon | Token::LineTerminator) {
                pos += 1;
                continue;
            }
            // Skip computed property brackets — names inside are usages, not declarations
            if matches!(t[pos].token, Token::LBracket) {
                let mut depth = 1usize;
                pos += 1;
                while pos < t.len() && depth > 0 {
                    if matches!(t[pos].token, Token::LBracket) {
                        depth += 1;
                    } else if matches!(t[pos].token, Token::RBracket) {
                        depth -= 1;
                    }
                    pos += 1;
                }
                continue;
            }
            if matches!(t[pos].token, Token::Static) {
                pos += 1;
                if pos < t.len() && matches!(t[pos].token, Token::LBrace) {
                    let mut depth: usize = 1;
                    pos += 1;
                    while pos < t.len() && depth > 0 {
                        if matches!(t[pos].token, Token::LBrace) {
                            depth += 1;
                        } else if matches!(t[pos].token, Token::RBrace) {
                            depth -= 1;
                        }
                        pos += 1;
                    }
                    continue;
                }
            }
            if let Some(Token::Identifier(id)) = t.get(pos).map(|tok| &tok.token)
                && (id == "get" || id == "set")
            {
                if let Some(Token::PrivateIdentifier(name)) = t.get(pos + 1).map(|tok| &tok.token) {
                    current_private_names.borrow_mut().insert(name.clone());
                }
                pos += 1;
                if pos < t.len() && (matches!(t[pos].token, Token::Identifier(_)) || matches!(t[pos].token, Token::PrivateIdentifier(_))) {
                    pos += 1;
                }
                if pos < t.len() && matches!(t[pos].token, Token::LParen) {
                    let mut depth = 1usize;
                    pos += 1;
                    while pos < t.len() && depth > 0 {
                        if matches!(t[pos].token, Token::LParen) {
                            depth += 1;
                        } else if matches!(t[pos].token, Token::RParen) {
                            depth -= 1;
                        }
                        pos += 1;
                    }
                }
                if pos < t.len() && matches!(t[pos].token, Token::LBrace) {
                    let mut depth = 1usize;
                    pos += 1;
                    while pos < t.len() && depth > 0 {
                        if matches!(t[pos].token, Token::LBrace) {
                            depth += 1;
                        } else if matches!(t[pos].token, Token::RBrace) {
                            depth -= 1;
                        }
                        pos += 1;
                    }
                }
                continue;
            }
            if let Some(Token::PrivateIdentifier(name)) = t.get(pos).map(|tok| &tok.token) {
                current_private_names.borrow_mut().insert(name.clone());
                pos += 1;
                if pos < t.len() && matches!(t[pos].token, Token::LParen) {
                    let mut depth = 1usize;
                    pos += 1;
                    while pos < t.len() && depth > 0 {
                        if matches!(t[pos].token, Token::LParen) {
                            depth += 1;
                        } else if matches!(t[pos].token, Token::RParen) {
                            depth -= 1;
                        }
                        pos += 1;
                    }
                    if pos < t.len() && matches!(t[pos].token, Token::LBrace) {
                        let mut depth = 1usize;
                        pos += 1;
                        while pos < t.len() && depth > 0 {
                            if matches!(t[pos].token, Token::LBrace) {
                                depth += 1;
                            } else if matches!(t[pos].token, Token::RBrace) {
                                depth -= 1;
                            }
                            pos += 1;
                        }
                    }
                    continue;
                }
                if pos < t.len() && matches!(t[pos].token, Token::Assign) {
                    pos += 1;
                    while pos < t.len() && !matches!(t[pos].token, Token::Semicolon | Token::LineTerminator) {
                        pos += 1;
                    }
                    if pos < t.len() && matches!(t[pos].token, Token::Semicolon | Token::LineTerminator) {
                        pos += 1;
                    }
                    continue;
                }
                if pos < t.len() && matches!(t[pos].token, Token::Semicolon | Token::LineTerminator) {
                    pos += 1;
                    continue;
                }
            }
            if let Some(Token::Identifier(_)) = t.get(pos).map(|tok| &tok.token) {
                pos += 1;
                if pos < t.len() && matches!(t[pos].token, Token::LParen) {
                    let mut depth = 1usize;
                    pos += 1;
                    while pos < t.len() && depth > 0 {
                        if matches!(t[pos].token, Token::LParen) {
                            depth += 1;
                        } else if matches!(t[pos].token, Token::RParen) {
                            depth -= 1;
                        }
                        pos += 1;
                    }
                    if pos < t.len() && matches!(t[pos].token, Token::LBrace) {
                        let mut depth = 1usize;
                        pos += 1;
                        while pos < t.len() && depth > 0 {
                            if matches!(t[pos].token, Token::LBrace) {
                                depth += 1;
                            } else if matches!(t[pos].token, Token::RBrace) {
                                depth -= 1;
                            }
                            pos += 1;
                        }
                    }
                    continue;
                }
                if pos < t.len() && matches!(t[pos].token, Token::Assign) {
                    pos += 1;
                    while pos < t.len() && !matches!(t[pos].token, Token::Semicolon | Token::LineTerminator) {
                        pos += 1;
                    }
                    if pos < t.len() && matches!(t[pos].token, Token::Semicolon | Token::LineTerminator) {
                        pos += 1;
                    }
                    continue;
                }
                if pos < t.len() && matches!(t[pos].token, Token::Semicolon | Token::LineTerminator) {
                    pos += 1;
                    continue;
                }
            }
            pos += 1;
        }
    }
    while *index < t.len() && !matches!(t[*index].token, Token::RBrace) {
        while *index < t.len() && matches!(t[*index].token, Token::Semicolon | Token::LineTerminator) {
            *index += 1;
        }
        if *index >= t.len() || matches!(t[*index].token, Token::RBrace) {
            break;
        }
        let is_static = if *index < t.len() && matches!(t[*index].token, Token::Static) {
            if let Some(next) = t.get(*index + 1) {
                #[allow(clippy::if_same_then_else)]
                if matches!(next.token, Token::LBrace) {
                    *index += 1;
                    true
                } else if matches!(next.token, Token::LParen) {
                    false
                } else if matches!(next.token, Token::Assign) {
                    false
                } else if matches!(next.token, Token::Semicolon | Token::LineTerminator) {
                    false
                } else {
                    *index += 1;
                    true
                }
            } else {
                false
            }
        } else {
            false
        };
        if is_static && *index < t.len() && matches!(t[*index].token, Token::LBrace) {
            *index += 1;
            // Per spec §15.7.1, static blocks: StatementList[~Yield, +Await, ~Return]
            // Save and clear generator context (~Yield)
            let saved_gen = GENERATOR_CONTEXT.with(|c| {
                let prev = *c.borrow();
                *c.borrow_mut() = 0;
                prev
            });
            // Save and clear function context (~Return: return is not allowed)
            let saved_fn = FUNCTION_CONTEXT.with(|c| {
                let prev = *c.borrow();
                *c.borrow_mut() = 0;
                prev
            });
            push_method_context(); // static blocks have a HomeObject (super property access)
            // new.target is valid inside static blocks (evaluates to undefined)
            NEW_TARGET_CONTEXT.with(|c| *c.borrow_mut() += 1);
            // `arguments` is forbidden inside static blocks (ContainsArguments early error)
            STATIC_BLOCK_CONTEXT.with(|c| *c.borrow_mut() += 1);
            let body = with_forbidden_await_identifier(|| with_cleared_await_context(|| parse_statement_block(t, index)))?;
            STATIC_BLOCK_CONTEXT.with(|c| *c.borrow_mut() -= 1);
            NEW_TARGET_CONTEXT.with(|c| *c.borrow_mut() -= 1);
            pop_method_context();
            FUNCTION_CONTEXT.with(|c| *c.borrow_mut() = saved_fn);
            GENERATOR_CONTEXT.with(|c| *c.borrow_mut() = saved_gen);
            members.push(ClassMember::StaticBlock(body));
            continue;
        }
        let mut is_accessor = false;
        let mut is_getter = false;
        if let Some(Token::Identifier(kw)) = t.get(*index).map(|d| &d.token)
            && (kw == "get" || kw == "set")
        {
            if let Some(next_tok) = t.get(*index + 1) {
                log::trace!(
                    "parse_primary: accessor candidate at idx={} kw={:?} next={:?} next.as_ident={:?}",
                    *index,
                    kw,
                    next_tok.token,
                    next_tok.token.as_identifier_string()
                );
            } else {
                log::trace!("parse_primary: accessor candidate at idx={} kw={:?} but no next token", *index, kw);
            }
            if let Some(next) = t.get(*index + 1) {
                if matches!(next.token, Token::Identifier(_))
                    || matches!(next.token, Token::PrivateIdentifier(_))
                    || matches!(next.token, Token::LBracket)
                    || matches!(next.token, Token::StringLit(_))
                    || matches!(next.token, Token::Number(_))
                {
                    if raw_identifier_source_has_escape(&t[*index]) {
                        return Err(raise_parse_error_with_token!(
                            t[*index],
                            format!("'{}' keyword in accessor must not contain escaped characters", kw)
                        ));
                    }
                    is_accessor = true;
                    is_getter = kw == "get";
                    log::trace!("parse_primary: accessor recognized (kw={}) at idx={}", kw, *index);
                } else {
                    if !matches!(next.token, Token::LParen) && next.token.as_identifier_string().is_some() {
                        if raw_identifier_source_has_escape(&t[*index]) {
                            return Err(raise_parse_error_with_token!(
                                t[*index],
                                format!("'{}' keyword in accessor must not contain escaped characters", kw)
                            ));
                        }
                        is_accessor = true;
                        is_getter = kw == "get";
                        log::trace!("parse_primary: accessor recognized for keyword-name (kw={}) at idx={}", kw, *index);
                    }
                }
            }
        }
        if is_accessor {
            *index += 1;
            let mut is_private = false;
            let mut prop_expr_opt: Option<Expr> = None;
            let mut prop_name_str: Option<String> = None;
            match &t[*index].token {
                Token::Identifier(name) => {
                    prop_name_str = Some(name.clone());
                    *index += 1;
                }
                Token::StringLit(raw_s) => {
                    prop_name_str = Some(utf16_to_utf8(raw_s));
                    *index += 1;
                }
                Token::Number(n) => {
                    let s = crate::core::value_to_string(&crate::core::Value::Number(*n));
                    prop_name_str = Some(s);
                    *index += 1;
                }
                Token::BigInt(s) => {
                    prop_name_str = Some(s.clone());
                    *index += 1;
                }
                Token::PrivateIdentifier(name) => {
                    prop_name_str = Some(name.clone());
                    is_private = true;
                    *index += 1;
                }
                Token::LBracket => {
                    *index += 1;
                    let expr = with_allowed_in(|| parse_assignment(t, index))?;
                    if *index >= t.len() || !matches!(t[*index].token, Token::RBracket) {
                        return Err(raise_parse_error_at!(t.get(*index)));
                    }
                    *index += 1;
                    prop_expr_opt = Some(expr);
                }
                _ => {
                    if let Some(name) = t[*index].token.as_identifier_string() {
                        prop_name_str = Some(name);
                        *index += 1;
                    } else {
                        return Err(raise_parse_error_at!(t.get(*index)));
                    }
                }
            }
            if *index >= t.len() || !matches!(t[*index].token, Token::LParen) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1;
            let saved_sb_gs = STATIC_BLOCK_CONTEXT.with(|c| {
                let p = *c.borrow();
                *c.borrow_mut() = 0;
                p
            });
            let params = with_cleared_forbidden_await_identifier(|| parse_parameters(t, index))?;
            if is_getter && !params.is_empty() {
                return Err(raise_parse_error!("SyntaxError: Getter must not have any formal parameters"));
            }
            if *index >= t.len() || !matches!(t[*index].token, Token::LBrace) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1;
            push_function_context();
            push_method_context();
            let body = with_cleared_forbidden_await_identifier(|| parse_statement_block(t, index))?;
            pop_method_context();
            pop_function_context();
            STATIC_BLOCK_CONTEXT.with(|c| *c.borrow_mut() = saved_sb_gs);
            if is_getter {
                if let Some(prop_expr) = prop_expr_opt {
                    if is_static {
                        members.push(ClassMember::StaticGetterComputed(prop_expr, body));
                    } else {
                        members.push(ClassMember::GetterComputed(prop_expr, body));
                    }
                } else if let Some(prop_name) = prop_name_str {
                    if is_static {
                        if is_private {
                            members.push(ClassMember::PrivateStaticGetter(prop_name, body));
                        } else {
                            members.push(ClassMember::StaticGetter(prop_name, body));
                        }
                    } else if is_private {
                        members.push(ClassMember::PrivateGetter(prop_name, body));
                    } else {
                        members.push(ClassMember::Getter(prop_name, body));
                    }
                }
            } else {
                if let Some(prop_expr) = prop_expr_opt {
                    if is_static {
                        members.push(ClassMember::StaticSetterComputed(prop_expr, params, body));
                    } else {
                        members.push(ClassMember::SetterComputed(prop_expr, params, body));
                    }
                } else if let Some(prop_name) = prop_name_str {
                    if is_static {
                        if is_private {
                            members.push(ClassMember::PrivateStaticSetter(prop_name, params, body));
                        } else {
                            members.push(ClassMember::StaticSetter(prop_name, params, body));
                        }
                    } else if is_private {
                        members.push(ClassMember::PrivateSetter(prop_name, params, body));
                    } else {
                        members.push(ClassMember::Setter(prop_name, params, body));
                    }
                }
            }
            continue;
        }
        let mut is_async_member = false;
        if *index < t.len() && matches!(t[*index].token, Token::Async) {
            is_async_member = true;
            *index += 1;
        }
        let mut is_generator = false;
        if *index < t.len() && matches!(t[*index].token, Token::Multiply) {
            is_generator = true;
            log::debug!("parse_class_member: saw '*' token at index {}", *index);
            *index += 1;
        }
        let mut name_str_opt: Option<String> = None;
        let mut is_private = false;
        let mut computed_key_expr: Option<Expr> = None;
        match &t[*index].token {
            Token::Identifier(name) => {
                name_str_opt = Some(name.clone());
            }
            Token::PrivateIdentifier(name) => {
                name_str_opt = Some(name.clone());
                is_private = true;
            }
            Token::StringLit(raw) => {
                name_str_opt = Some(utf16_to_utf8(raw));
            }
            Token::Number(n) => {
                let s = crate::core::value_to_string(&crate::core::Value::Number(*n));
                name_str_opt = Some(s);
            }
            Token::BigInt(s) => {
                name_str_opt = Some(s.clone());
            }
            Token::LBracket => {
                *index += 1;
                let expr = with_allowed_in(|| parse_assignment(t, index))?;
                if *index >= t.len() || !matches!(t[*index].token, Token::RBracket) {
                    return Err(raise_parse_error_at!(t.get(*index)));
                }
                *index += 1;
                computed_key_expr = Some(expr);
            }
            _ => {
                if let Some(name) = t[*index].token.as_identifier_string() {
                    name_str_opt = Some(name);
                } else {
                    return Err(raise_parse_error_at!(t.get(*index)));
                }
            }
        }
        if let Some(ref name) = name_str_opt {
            if is_private {
                if declared_private_names.contains(name) {
                    let msg = format!("Duplicate private name: #{}", name);
                    return Err(raise_parse_error_with_token!(&t[*index], msg));
                }
                declared_private_names.insert(name.clone());
                current_private_names.borrow_mut().insert(name.clone());
            }
            *index += 1;
        }
        // It is a Syntax Error if PropName of MethodDefinition is "constructor" and SpecialMethod is true.
        if computed_key_expr.is_none()
            && !is_static
            && !is_private
            && name_str_opt.as_deref() == Some("constructor")
            && (is_generator || is_async_member)
        {
            return Err(raise_parse_error!(
                "SyntaxError: Class constructor may not be an async method or a generator"
            ));
        }
        if computed_key_expr.is_none()
            && !is_static
            && !is_private
            && name_str_opt.as_deref() == Some("constructor")
            && matches!(t.get(*index).map(|d| &d.token), Some(Token::LParen))
        {
            *index += 1;
            push_method_context(); // for super.x in default params
            push_constructor_context(); // for super() in default params
            // Functions have their own `arguments`, clear static block restriction for params
            let saved_sb = STATIC_BLOCK_CONTEXT.with(|c| {
                let p = *c.borrow();
                *c.borrow_mut() = 0;
                p
            });
            let params = with_cleared_forbidden_await_identifier(|| parse_parameters(t, index))?;
            if *index >= t.len() || !matches!(t[*index].token, Token::LBrace) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1;
            push_function_context();
            push_method_context(); // re-push for body
            push_constructor_context(); // re-push for body
            let body = with_cleared_forbidden_await_identifier(|| parse_statement_block(t, index))?;
            pop_constructor_context();
            pop_method_context();
            pop_function_context();
            pop_constructor_context(); // pop pre-params
            pop_method_context(); // pop pre-params
            STATIC_BLOCK_CONTEXT.with(|c| *c.borrow_mut() = saved_sb);
            members.push(ClassMember::Constructor(params, body));
            continue;
        }
        if *index < t.len() && matches!(t[*index].token, Token::LParen) {
            *index += 1;
            // Push method context before params so super.x works in default parameters
            push_method_context();
            // Functions have their own `arguments`, clear static block restriction for params
            let saved_sb_m = STATIC_BLOCK_CONTEXT.with(|c| {
                let p = *c.borrow();
                *c.borrow_mut() = 0;
                p
            });
            let params = if is_generator {
                push_generator_context();
                let p = with_cleared_forbidden_await_identifier(|| parse_parameters(t, index))?;
                pop_generator_context();
                p
            } else {
                let saved = GENERATOR_CONTEXT.with(|c| {
                    let old = *c.borrow();
                    *c.borrow_mut() = 0;
                    old
                });
                let p = with_cleared_forbidden_await_identifier(|| parse_parameters(t, index))?;
                GENERATOR_CONTEXT.with(|c| *c.borrow_mut() = saved);
                p
            };
            if *index >= t.len() || !matches!(t[*index].token, Token::LBrace) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1;
            push_function_context();
            push_method_context(); // re-push for body (push_function_context cleared it)
            let body = if is_generator {
                push_generator_context();
                let b = with_cleared_forbidden_await_identifier(|| parse_statement_block(t, index))?;
                pop_generator_context();
                b
            } else {
                with_cleared_forbidden_await_identifier(|| parse_statement_block(t, index))?
            };
            pop_method_context(); // pop body method context
            pop_function_context();
            pop_method_context(); // pop pre-params method context
            STATIC_BLOCK_CONTEXT.with(|c| *c.borrow_mut() = saved_sb_m);
            if is_generator {
                if let Some(expr) = computed_key_expr {
                    if is_static {
                        if is_async_member {
                            members.push(ClassMember::StaticMethodComputedAsyncGenerator(expr, params, body));
                        } else {
                            members.push(ClassMember::StaticMethodComputedGenerator(expr, params, body));
                        }
                    } else if is_async_member {
                        members.push(ClassMember::MethodComputedAsyncGenerator(expr, params, body));
                    } else {
                        members.push(ClassMember::MethodComputedGenerator(expr, params, body));
                    }
                } else if let Some(name) = name_str_opt {
                    if is_static {
                        if is_private {
                            if is_async_member {
                                members.push(ClassMember::PrivateStaticMethodAsyncGenerator(name, params, body));
                            } else {
                                members.push(ClassMember::PrivateStaticMethodGenerator(name, params, body));
                            }
                        } else if is_async_member {
                            members.push(ClassMember::StaticMethodAsyncGenerator(name, params, body));
                        } else {
                            members.push(ClassMember::StaticMethodGenerator(name, params, body));
                        }
                    } else if is_private {
                        if is_async_member {
                            members.push(ClassMember::PrivateMethodAsyncGenerator(name, params, body));
                        } else {
                            members.push(ClassMember::PrivateMethodGenerator(name, params, body));
                        }
                    } else if is_async_member {
                        members.push(ClassMember::MethodAsyncGenerator(name, params, body));
                    } else {
                        members.push(ClassMember::MethodGenerator(name, params, body));
                    }
                }
            } else if let Some(expr) = computed_key_expr {
                if is_static {
                    if is_async_member {
                        members.push(ClassMember::StaticMethodComputedAsync(expr, params, body));
                    } else {
                        members.push(ClassMember::StaticMethodComputed(expr, params, body));
                    }
                } else if is_async_member {
                    members.push(ClassMember::MethodComputedAsync(expr, params, body));
                } else {
                    members.push(ClassMember::MethodComputed(expr, params, body));
                }
            } else if let Some(name) = name_str_opt {
                if is_static {
                    if is_private {
                        if is_async_member {
                            members.push(ClassMember::PrivateStaticMethodAsync(name, params, body));
                        } else {
                            members.push(ClassMember::PrivateStaticMethod(name, params, body));
                        }
                    } else if is_async_member {
                        members.push(ClassMember::StaticMethodAsync(name, params, body));
                    } else {
                        members.push(ClassMember::StaticMethod(name, params, body));
                    }
                } else if is_private {
                    if is_async_member {
                        members.push(ClassMember::PrivateMethodAsync(name, params, body));
                    } else {
                        members.push(ClassMember::PrivateMethod(name, params, body));
                    }
                } else if is_async_member {
                    members.push(ClassMember::MethodAsync(name, params, body));
                } else {
                    members.push(ClassMember::Method(name, params, body));
                }
            }
        } else if *index < t.len() && matches!(t[*index].token, Token::Assign) {
            *index += 1;
            // Field initializers have an implicit [[HomeObject]], so super property
            // access is valid inside arrow functions in field initializers.
            push_method_context();
            let value = parse_expression(t, index)?;
            pop_method_context();
            if *index < t.len() {
                match t[*index].token {
                    Token::Semicolon | Token::LineTerminator => *index += 1,
                    Token::RBrace => {}
                    _ => return Err(raise_parse_error_at!(t.get(*index))),
                }
            }
            if let Some(expr) = computed_key_expr {
                if is_static {
                    members.push(ClassMember::StaticPropertyComputed(expr, value));
                } else {
                    members.push(ClassMember::PropertyComputed(expr, value));
                }
            } else if let Some(name) = name_str_opt {
                if is_static {
                    if is_private {
                        members.push(ClassMember::PrivateStaticProperty(name, value));
                    } else {
                        members.push(ClassMember::StaticProperty(name, value));
                    }
                } else if is_private {
                    members.push(ClassMember::PrivateProperty(name, value));
                } else {
                    members.push(ClassMember::Property(name, value));
                }
            }
        } else {
            if *index < t.len() {
                match t[*index].token {
                    Token::Semicolon | Token::LineTerminator => *index += 1,
                    Token::RBrace => {}
                    _ => return Err(raise_parse_error_at!(t.get(*index))),
                }
            }
            if let Some(expr) = computed_key_expr {
                if is_static {
                    members.push(ClassMember::StaticPropertyComputed(expr, Expr::Undefined));
                } else {
                    members.push(ClassMember::PropertyComputed(expr, Expr::Undefined));
                }
            } else if let Some(name) = name_str_opt {
                if is_static {
                    if is_private {
                        members.push(ClassMember::PrivateStaticProperty(name, Expr::Undefined));
                    } else {
                        members.push(ClassMember::StaticProperty(name, Expr::Undefined));
                    }
                } else if is_private {
                    members.push(ClassMember::PrivateProperty(name, Expr::Undefined));
                } else {
                    members.push(ClassMember::Property(name, Expr::Undefined));
                }
            }
        }
    }
    *index += 1;
    Ok(members)
}
fn parse_primary(tokens: &[TokenData], index: &mut usize, allow_call: bool) -> Result<Expr, JSError> {
    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index >= tokens.len() {
        return Err(raise_parse_error_at!(tokens.get(*index)));
    }
    // Consume the heritage arrow-suppression flag once at the top of parse_primary.
    // This prevents arrows at the outermost level of ClassHeritage while still
    // allowing arrows inside nested grouping expressions.
    let suppress_arrow = NO_ARROW_IN_PAREN.with(|c| c.replace(false));
    let token_data = &tokens[*index];
    *index += 1;
    let current = &token_data.token;
    let mut expr = match current {
        Token::Number(n) => Expr::Number(*n),
        Token::BigInt(s) => Expr::BigInt(crate::unicode::utf8_to_utf16(s)),
        Token::StringLit(s) => Expr::StringLit(s.to_vec()),
        Token::True => Expr::Boolean(true),
        Token::False => Expr::Boolean(false),
        Token::Null => Expr::Null,
        Token::TypeOf => {
            let inner = parse_primary(tokens, index, true)?;
            Expr::TypeOf(Box::new(inner))
        }
        Token::Delete => {
            let inner = parse_primary(tokens, index, true)?;
            if let Expr::Property(_, prop_name) = &inner
                && prop_name.starts_with('#')
            {
                let msg = format!("Private field '{prop_name}' cannot be deleted");
                return Err(raise_parse_error_with_token!(token_data, msg));
            }
            if let Expr::PrivateMember(_, prop_name) = &inner {
                let msg = format!("Private field '{prop_name}' cannot be deleted");
                return Err(raise_parse_error_with_token!(token_data, msg));
            }
            if let Expr::Var(..) = &inner
                && strict_binding_checks()
            {
                return Err(raise_parse_error_with_token!(
                    token_data,
                    "Delete of an unqualified identifier in strict mode"
                ));
            }
            Expr::Delete(Box::new(inner))
        }
        Token::Void => {
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Yield | Token::YieldStar) {
                return Err(raise_parse_error_with_token!(tokens[*index], "Unexpected yield"));
            }
            let inner = parse_primary(tokens, index, true)?;
            Expr::Void(Box::new(inner))
        }
        Token::Await => {
            if forbid_await_identifier() && !in_await_context() {
                return Err(raise_parse_error_with_token!(token_data, "Unexpected await"));
            }
            if *index < tokens.len() {
                let next_can_start_expr = matches!(
                    tokens[*index].token,
                    Token::Number(_)
                        | Token::BigInt(_)
                        | Token::StringLit(_)
                        | Token::True
                        | Token::False
                        | Token::Null
                        | Token::TypeOf
                        | Token::Delete
                        | Token::Void
                        | Token::Await
                        | Token::Yield
                        | Token::YieldStar
                        | Token::LogicalNot
                        | Token::Class
                        | Token::Function
                        | Token::FunctionStar
                        | Token::Async
                        | Token::LBracket
                        | Token::LBrace
                        | Token::Identifier(_)
                        | Token::PrivateIdentifier(_)
                        | Token::LParen
                        | Token::New
                        | Token::This
                        | Token::Super
                        | Token::Import
                        | Token::TemplateString(_)
                        | Token::Regex(_, _)
                );
                if matches!(tokens[*index].token, Token::Assign) {
                    Expr::Var("await".to_string(), Some(token_data.line), Some(token_data.column))
                } else if in_await_context() {
                    if next_can_start_expr {
                        let inner = parse_primary(tokens, index, true)?;
                        Expr::Await(Box::new(inner))
                    } else if forbid_await_identifier() {
                        return Err(raise_parse_error_with_token!(token_data, "'await' requires an operand"));
                    } else {
                        Expr::Var("await".to_string(), Some(token_data.line), Some(token_data.column))
                    }
                } else {
                    if next_can_start_expr && !matches!(tokens[*index].token, Token::LParen) {
                        let inner = parse_primary(tokens, index, true)?;
                        Expr::Await(Box::new(inner))
                    } else {
                        Expr::Var("await".to_string(), Some(token_data.line), Some(token_data.column))
                    }
                }
            } else {
                Expr::Var("await".to_string(), Some(token_data.line), Some(token_data.column))
            }
        }
        Token::Yield | Token::YieldStar => {
            // yield is an AssignmentExpression, not a primary expression.
            // If we reach here, yield appears in a position where only
            // primary expressions are valid (e.g., operand of binary +).
            return Err(raise_parse_error_with_token!(token_data, "Unexpected yield"));
        }
        Token::LogicalNot => {
            let inner = parse_primary(tokens, index, true)?;
            Expr::LogicalNot(Box::new(inner))
        }
        Token::Class => {
            let name = if *index < tokens.len() {
                match &tokens[*index].token {
                    Token::Identifier(n) => {
                        if is_strict_reserved_word(n) || is_always_reserved_word(n) {
                            let msg = format!("'{}' is not allowed as a class name in strict mode", n);
                            return Err(raise_parse_error_with_token!(tokens[*index], msg));
                        }
                        if n == "await" && forbid_await_identifier() {
                            return Err(raise_parse_error_with_token!(tokens[*index], "Cannot use 'await' as class name"));
                        }
                        let n = n.clone();
                        *index += 1;
                        n
                    }
                    Token::Await => {
                        if forbid_await_identifier() {
                            return Err(raise_parse_error_with_token!(tokens[*index], "Cannot use 'await' as class name"));
                        }
                        *index += 1;
                        "await".to_string()
                    }
                    Token::Async => {
                        *index += 1;
                        "async".to_string()
                    }
                    _ => "".to_string(),
                }
            } else {
                "".to_string()
            };
            let extends = if *index < tokens.len() && matches!(tokens[*index].token, Token::Extends) {
                *index += 1;
                NO_ARROW_IN_PAREN.with(|c| c.set(true));
                let heritage = parse_assignment(tokens, index);
                NO_ARROW_IN_PAREN.with(|c| c.set(false));
                Some(heritage?)
            } else {
                None
            };
            push_class_heritage(extends.is_some());
            let members = parse_class_body(tokens, index)?;
            pop_class_heritage();
            let class_def = crate::core::ClassDefinition { name, extends, members };
            Expr::Class(Box::new(class_def))
        }
        Token::New => {
            {
                let mut s = String::new();
                for i in 0..5 {
                    if *index + i < tokens.len() {
                        s.push_str(&format!("{:?} ", tokens[*index + i].token));
                    }
                }
                log::trace!("DEBUG-PARSER-New-lookahead: {}", s);
            }
            let mut look = *index;
            while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
                look += 1;
            }
            let is_new_target = if look < tokens.len() && matches!(tokens[look].token, Token::Dot) {
                look += 1;
                while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
                    look += 1;
                }
                if look < tokens.len()
                    && let Token::Identifier(id) = &tokens[look].token
                    && id == "target"
                {
                    if raw_identifier_source_has_escape(&tokens[look]) {
                        return Err(raise_parse_error!(
                            "'target' in new.target must not contain Unicode escape sequences"
                        ));
                    }
                    *index = look + 1;
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if is_new_target {
                let in_new_target_context = NEW_TARGET_CONTEXT.with(|c| *c.borrow() > 0);
                if !in_new_target_context {
                    return Err(raise_parse_error!("SyntaxError: new.target expression is not allowed here"));
                }
                Expr::NewTarget
            } else {
                // `new import(...)` is a SyntaxError, but `new (import(...))` is valid
                let bare_import = *index < tokens.len() && matches!(tokens[*index].token, Token::Import);
                let constructor = parse_primary(tokens, index, false)?;
                if bare_import && matches!(constructor, Expr::DynamicImport(..)) {
                    return Err(raise_parse_error!("Cannot use 'new' with import()"));
                }
                let args = if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                    *index += 1;
                    let mut args = Vec::new();
                    if *index < tokens.len() && !matches!(tokens[*index].token, Token::RParen) {
                        loop {
                            let arg = parse_assignment(tokens, index)?;
                            args.push(arg);
                            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                                *index += 1;
                            }
                            if *index >= tokens.len() {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            if matches!(tokens[*index].token, Token::RParen) {
                                break;
                            }
                            if !matches!(tokens[*index].token, Token::Comma) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                                *index += 1;
                            }
                            if *index >= tokens.len() {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            if matches!(tokens[*index].token, Token::RParen) {
                                break;
                            }
                        }
                    }
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                    *index += 1;
                    if args.len() == 1
                        && let Expr::Comma(_, _) = &args[0]
                    {
                        let first = args.remove(0);
                        let new_args = flatten_commas(first);
                        args.extend(new_args);
                    }
                    args
                } else {
                    Vec::new()
                };
                Expr::New(Box::new(constructor), args)
            }
        }
        Token::Minus => {
            let inner = parse_primary(tokens, index, true)?;
            Expr::UnaryNeg(Box::new(inner))
        }
        Token::Plus => {
            let inner = parse_primary(tokens, index, true)?;
            Expr::UnaryPlus(Box::new(inner))
        }
        Token::BitNot => {
            let inner = parse_primary(tokens, index, true)?;
            Expr::BitNot(Box::new(inner))
        }
        Token::Increment => {
            let inner = parse_primary(tokens, index, true)?;
            Expr::Increment(Box::new(inner))
        }
        Token::Decrement => {
            let inner = parse_primary(tokens, index, true)?;
            Expr::Decrement(Box::new(inner))
        }
        Token::Spread => {
            let inner = parse_assignment(tokens, index)?;
            Expr::Spread(Box::new(inner))
        }
        Token::TemplateString(parts) => {
            if parts.is_empty() {
                Expr::StringLit(Vec::new())
            } else if parts.len() == 1 {
                match &parts[0] {
                    TemplatePart::String(cooked_opt, _raw) => {
                        let cooked = cooked_opt.clone().ok_or_else(|| raise_parse_error_at!(tokens.get(*index - 1)))?;
                        Expr::StringLit(cooked)
                    }
                    TemplatePart::Expr(expr_tokens) => {
                        let expr_tokens = expr_tokens.clone();
                        let e = parse_expression(&expr_tokens, &mut 0)?;
                        Expr::Call(Box::new(Expr::Var("String".to_string(), None, None)), vec![e])
                    }
                }
            } else {
                let mut expr = match &parts[0] {
                    TemplatePart::String(cooked_opt, _raw) => {
                        let cooked = cooked_opt.clone().ok_or_else(|| raise_parse_error_at!(tokens.get(*index - 1)))?;
                        Expr::StringLit(cooked)
                    }
                    TemplatePart::Expr(expr_tokens) => {
                        let expr_tokens = expr_tokens.clone();
                        let e = parse_expression(&expr_tokens, &mut 0)?;
                        Expr::Binary(Box::new(Expr::StringLit(Vec::new())), BinaryOp::Add, Box::new(e))
                    }
                };
                for part in &parts[1..] {
                    let right = match part {
                        TemplatePart::String(cooked_opt, _raw) => {
                            let cooked = cooked_opt.clone().ok_or_else(|| raise_parse_error_at!(tokens.get(*index - 1)))?;
                            Expr::StringLit(cooked)
                        }
                        TemplatePart::Expr(expr_tokens) => {
                            let expr_tokens = expr_tokens.clone();
                            let e = parse_expression(&expr_tokens, &mut 0)?;
                            Expr::Call(Box::new(Expr::Var("String".to_string(), None, None)), vec![e])
                        }
                    };
                    expr = Expr::Binary(Box::new(expr), BinaryOp::Add, Box::new(right));
                }
                expr
            }
        }
        Token::Identifier(name) => {
            let line = token_data.line;
            let column = token_data.column;
            if is_always_reserved_word(name) || is_strict_reserved_word(name) {
                return Err(raise_parse_error!(
                    &format!("Keyword '{}' cannot contain escaped characters", name),
                    line,
                    column
                ));
            }
            // ContainsArguments early error: `arguments` is forbidden in class static blocks
            if name == "arguments" && STATIC_BLOCK_CONTEXT.with(|c| *c.borrow() > 0) {
                return Err(raise_parse_error!(
                    "SyntaxError: 'arguments' is not allowed in class static initialization blocks",
                    line,
                    column
                ));
            }
            let mut expr = Expr::Var(name.clone(), Some(line), Some(column));
            if !suppress_arrow && *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                *index += 1;
                let body = parse_arrow_body(tokens, index)?;
                expr = Expr::ArrowFunction(vec![DestructuringElement::Variable(name.clone(), None)], body);
            }
            expr
        }
        Token::As => {
            let line = token_data.line;
            let column = token_data.column;
            let mut expr = Expr::Var("as".to_string(), Some(line), Some(column));
            if !suppress_arrow && *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                *index += 1;
                let body = parse_arrow_body(tokens, index)?;
                expr = Expr::ArrowFunction(vec![DestructuringElement::Variable("as".to_string(), None)], body);
            }
            expr
        }
        Token::PrivateIdentifier(name) => {
            let invalid = PRIVATE_NAME_STACK.with(|s| {
                let stack = s.borrow();
                !stack.iter().rev().any(|rc| rc.borrow().contains(name))
            });
            if invalid {
                let msg = format!("Private field '#{name}' must be declared in an enclosing class");
                return Err(raise_parse_error_with_token!(tokens[*index - 1], msg));
            }
            Expr::PrivateName(name.clone())
        }
        Token::Import => {
            if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                *index += 1;
                // `in` is allowed inside import() arguments even in for-loop context
                let arg = with_allowed_in(|| parse_assignment(tokens, index))?;
                let mut options_arg: Option<Box<Expr>> = None;
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
                    *index += 1;
                    if !(*index < tokens.len() && matches!(tokens[*index].token, Token::RParen)) {
                        let opt = with_allowed_in(|| parse_assignment(tokens, index))?;
                        options_arg = Some(Box::new(opt));
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
                            *index += 1;
                        }
                    }
                }
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                    return Err(raise_parse_error!("Expected ')' after import(...)"));
                }
                *index += 1;
                Expr::DynamicImport(Box::new(arg), options_arg)
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Dot) {
                *index += 1;
                while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                if *index < tokens.len()
                    && let Token::Identifier(id) = &tokens[*index].token
                    && id == "meta"
                {
                    *index += 1;
                    Expr::Property(
                        Box::new(Expr::Var("import".to_string(), Some(token_data.line), Some(token_data.column))),
                        "meta".to_string(),
                    )
                } else {
                    return Err(raise_parse_error!(
                        "Only 'import.meta' is valid; 'import.' followed by other identifiers is not supported"
                    ));
                }
            } else {
                return Err(raise_parse_error!(
                    "'import' keyword cannot be used as an expression; use import() or import.meta"
                ));
            }
        }
        Token::Regex(pattern, flags) => Expr::Regex(pattern.clone(), flags.clone()),
        Token::This => Expr::This,
        Token::Super => {
            if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                if !in_constructor_context() {
                    return Err(raise_parse_error_with_token!(
                        tokens[*index - 1],
                        "'super()' is only valid inside a class constructor"
                    ));
                }
                if !class_has_heritage() {
                    return Err(raise_parse_error_with_token!(
                        tokens[*index - 1],
                        "'super()' is only valid in a derived class constructor"
                    ));
                }
                *index += 1;
                let mut args = Vec::new();
                if *index < tokens.len() && !matches!(tokens[*index].token, Token::RParen) {
                    loop {
                        let arg = parse_assignment(tokens, index)?;
                        args.push(arg);
                        if *index >= tokens.len() {
                            return Err(raise_parse_error_at!(tokens.get(*index)));
                        }
                        if matches!(tokens[*index].token, Token::RParen) {
                            break;
                        }
                        if !matches!(tokens[*index].token, Token::Comma) {
                            return Err(raise_parse_error_at!(tokens.get(*index)));
                        }
                        *index += 1;
                    }
                }
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                Expr::SuperCall(args)
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Dot) {
                if !in_method_context() && !in_constructor_context() {
                    return Err(raise_parse_error_with_token!(
                        tokens[*index - 1],
                        "'super' property access is only valid inside a method"
                    ));
                }
                *index += 1;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::Identifier(_)) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                let prop = if let Token::Identifier(name) = &tokens[*index - 1].token {
                    name.clone()
                } else {
                    return Err(raise_parse_error_at!(tokens.get(*index - 1)));
                };
                if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                    *index += 1;
                    let mut args = Vec::new();
                    if *index < tokens.len() && !matches!(tokens[*index].token, Token::RParen) {
                        loop {
                            let arg = parse_assignment(tokens, index)?;
                            args.push(arg);
                            if *index >= tokens.len() {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            if matches!(tokens[*index].token, Token::RParen) {
                                break;
                            }
                            if !matches!(tokens[*index].token, Token::Comma) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                                *index += 1;
                            }
                            if *index >= tokens.len() {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            if matches!(tokens[*index].token, Token::RParen) {
                                break;
                            }
                        }
                    }
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                    *index += 1;
                    if args.len() == 1
                        && let Expr::Comma(_, _) = &args[0]
                    {
                        let first = args.remove(0);
                        let new_args = flatten_commas(first);
                        args.extend(new_args);
                    }
                    Expr::SuperMethod(prop, args)
                } else {
                    Expr::SuperProperty(prop)
                }
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                // super[expr] — computed super property access
                if !in_method_context() && !in_constructor_context() {
                    return Err(raise_parse_error_with_token!(
                        tokens[*index - 1],
                        "'super' property access is only valid inside a method"
                    ));
                }
                *index += 1;
                let key_expr = parse_assignment(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                    // super[expr](args) — computed super method call
                    *index += 1;
                    let mut args = Vec::new();
                    if *index < tokens.len() && !matches!(tokens[*index].token, Token::RParen) {
                        loop {
                            let arg = parse_assignment(tokens, index)?;
                            args.push(arg);
                            if *index >= tokens.len() {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            if matches!(tokens[*index].token, Token::RParen) {
                                break;
                            }
                            if !matches!(tokens[*index].token, Token::Comma) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                                *index += 1;
                            }
                            if *index >= tokens.len() {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            if matches!(tokens[*index].token, Token::RParen) {
                                break;
                            }
                        }
                    }
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                    *index += 1;
                    if args.len() == 1
                        && let Expr::Comma(_, _) = &args[0]
                    {
                        let first = args.remove(0);
                        let new_args = flatten_commas(first);
                        args.extend(new_args);
                    }
                    Expr::SuperComputedMethod(Box::new(key_expr), args)
                } else {
                    Expr::SuperComputedProperty(Box::new(key_expr))
                }
            } else {
                return Err(raise_parse_error_with_token!(tokens[*index - 1], "'super' keyword unexpected here"));
            }
        }
        Token::LBrace => {
            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                *index += 1;
            }
            let mut properties = Vec::new();
            let mut has_proto = false;
            if *index < tokens.len() && matches!(tokens[*index].token, Token::RBrace) {
                *index += 1;
            } else {
                loop {
                    log::trace!(
                        "parse_primary: object literal loop; next tokens (first 8): {:?}",
                        tokens.iter().take(8).collect::<Vec<_>>()
                    );
                    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                        *index += 1;
                    }
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::RBrace) {
                        *index += 1;
                        break;
                    }
                    if *index >= tokens.len() {
                        return Err(raise_parse_error_at!(tokens.last()));
                    }
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Spread) {
                        log::trace!(
                            "parse_primary: object property is spread; next tokens (first 8): {:?}",
                            tokens.iter().take(8).collect::<Vec<_>>()
                        );
                        *index += 1;
                        let expr = parse_assignment(tokens, index)?;
                        properties.push((Expr::StringLit(Vec::new()), Expr::Spread(Box::new(expr)), false, false));
                    } else {
                        let method_start_byte = tokens[*index].byte_offset;
                        log::trace!(
                            "parse_primary: object literal accessor check at idx {} tok={:?} next={:?}",
                            *index,
                            tokens.get(*index).map(|t| &t.token),
                            tokens.get(*index + 1).map(|t| &t.token)
                        );
                        let is_getter =
                            if tokens.len() > *index + 1 && tokens[*index].token.as_identifier_string().as_deref() == Some("get") {
                                if matches!(
                                    tokens[*index + 1].token,
                                    Token::Identifier(_) | Token::StringLit(_) | Token::Number(_) | Token::BigInt(_)
                                ) || tokens[*index + 1].token.as_identifier_string().is_some()
                                {
                                    tokens.len() > *index + 2 && matches!(tokens[*index + 2].token, Token::LParen)
                                } else if matches!(tokens[*index + 1].token, Token::LBracket) {
                                    let mut depth = 0i32;
                                    let mut idx_after = None;
                                    for (i, t) in tokens.iter().enumerate().skip(*index + 1) {
                                        match &t.token {
                                            Token::LBracket => depth += 1,
                                            Token::RBracket => {
                                                depth -= 1;
                                                if depth == 0 {
                                                    idx_after = Some(i + 1);
                                                    break;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    if let Some(next_i) = idx_after {
                                        next_i < tokens.len() && matches!(tokens[next_i].token, Token::LParen)
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                        let is_setter =
                            if tokens.len() > *index + 1 && tokens[*index].token.as_identifier_string().as_deref() == Some("set") {
                                if matches!(
                                    tokens[*index + 1].token,
                                    Token::Identifier(_) | Token::StringLit(_) | Token::Number(_) | Token::BigInt(_)
                                ) || tokens[*index + 1].token.as_identifier_string().is_some()
                                {
                                    tokens.len() > *index + 2 && matches!(tokens[*index + 2].token, Token::LParen)
                                } else if matches!(tokens[*index + 1].token, Token::LBracket) {
                                    let mut depth = 0i32;
                                    let mut idx_after = None;
                                    for (i, t) in tokens.iter().enumerate().skip(*index + 1) {
                                        match &t.token {
                                            Token::LBracket => depth += 1,
                                            Token::RBracket => {
                                                depth -= 1;
                                                if depth == 0 {
                                                    idx_after = Some(i + 1);
                                                    break;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    if let Some(next_i) = idx_after {
                                        next_i < tokens.len() && matches!(tokens[next_i].token, Token::LParen)
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                        if is_getter || is_setter {
                            if raw_identifier_source_has_escape(&tokens[*index]) {
                                let kw = if is_getter { "get" } else { "set" };
                                return Err(raise_parse_error_with_token!(
                                    tokens[*index],
                                    format!("'{}' keyword in accessor must not contain escaped characters", kw)
                                ));
                            }
                            log::trace!(
                                "parse_primary: object property is getter/setter; next tokens (first 8): {:?}",
                                tokens.iter().take(8).collect::<Vec<_>>()
                            );
                            *index += 1;
                        }
                        let mut is_shorthand_candidate = false;
                        let mut key_is_computed = false;
                        let mut is_async_member = false;
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::Async) {
                            let mut peek = *index + 1;
                            while peek < tokens.len() && matches!(tokens[peek].token, Token::LineTerminator) {
                                peek += 1;
                            }
                            let next_starts_method_name = peek < tokens.len()
                                && (matches!(tokens[peek].token, Token::Identifier(_) | Token::LBracket | Token::Multiply)
                                    || tokens[peek].token.as_identifier_string().is_some());
                            if next_starts_method_name {
                                is_async_member = true;
                                *index += 1;
                            }
                        } else if *index < tokens.len()
                            && matches!(tokens[*index].token, Token::Identifier(ref s) if s == "async")
                            && raw_identifier_source_has_escape(&tokens[*index])
                        {
                            // Escaped "async" — check if next tokens look like a method def
                            let mut peek = *index + 1;
                            while peek < tokens.len() && matches!(tokens[peek].token, Token::LineTerminator) {
                                peek += 1;
                            }
                            let next_starts_method_name = peek < tokens.len()
                                && (matches!(tokens[peek].token, Token::Identifier(_) | Token::LBracket | Token::Multiply)
                                    || tokens[peek].token.as_identifier_string().is_some());
                            if next_starts_method_name {
                                return Err(raise_parse_error_with_token!(
                                    tokens[*index],
                                    "'async' keyword must not contain escaped characters"
                                ));
                            }
                        }
                        let mut is_generator = false;
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::Multiply) {
                            is_generator = true;
                            *index += 1;
                        }
                        if is_generator
                            && *index < tokens.len()
                            && matches!(tokens[*index].token, Token::Yield)
                            && !is_getter
                            && !is_setter
                            && tokens.len() > *index + 1
                            && matches!(tokens[*index + 1].token, Token::LParen)
                        {
                            *index += 1;
                            *index += 1;
                            push_method_context();
                            let params = with_cleared_forbidden_await_identifier(|| parse_parameters(tokens, index))?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                pop_method_context();
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            push_function_context();
                            push_method_context();
                            if is_generator {
                                push_generator_context();
                            }
                            let body = with_cleared_forbidden_await_identifier(|| parse_statements(tokens, index))?;
                            if is_generator {
                                pop_generator_context();
                            }
                            pop_method_context();
                            pop_function_context();
                            pop_method_context();
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            if is_generator {
                                if is_async_member {
                                    properties.push((
                                        Expr::StringLit(crate::unicode::utf8_to_utf16("yield")),
                                        Expr::AsyncGeneratorFunction(
                                            None,
                                            params,
                                            body,
                                            Some((method_start_byte, tokens[*index - 1].byte_offset + 1)),
                                        ),
                                        false,
                                        false,
                                    ));
                                } else {
                                    properties.push((
                                        Expr::StringLit(crate::unicode::utf8_to_utf16("yield")),
                                        Expr::GeneratorFunction(
                                            None,
                                            params,
                                            body,
                                            Some((method_start_byte, tokens[*index - 1].byte_offset + 1)),
                                        ),
                                        false,
                                        false,
                                    ));
                                }
                            } else if is_async_member {
                                properties.push((
                                    Expr::StringLit(crate::unicode::utf8_to_utf16("yield")),
                                    Expr::AsyncFunction(None, params, body, Some((method_start_byte, tokens[*index - 1].byte_offset + 1))),
                                    false,
                                    false,
                                ));
                            } else {
                                properties.push((
                                    Expr::StringLit(crate::unicode::utf8_to_utf16("yield")),
                                    Expr::Function(None, params, body, Some((method_start_byte, tokens[*index - 1].byte_offset + 1))),
                                    false,
                                    false,
                                ));
                            }
                            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator | Token::Semicolon) {
                                *index += 1;
                            }
                            if *index >= tokens.len() {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            if matches!(tokens[*index].token, Token::RBrace) {
                                *index += 1;
                                break;
                            }
                            if matches!(tokens[*index].token, Token::Comma) {
                                *index += 1;
                                continue;
                            }
                            continue;
                        }
                        let key_expr = if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
                            if !is_getter && !is_setter && tokens.len() > *index + 1 && matches!(tokens[*index + 1].token, Token::LParen) {
                                *index += 1;
                                *index += 1;
                                push_method_context();
                                let params = if is_async_member {
                                    parse_parameters(tokens, index)?
                                } else {
                                    with_cleared_forbidden_await_identifier(|| parse_parameters(tokens, index))?
                                };
                                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                    pop_method_context();
                                    return Err(raise_parse_error_at!(tokens.get(*index)));
                                }
                                *index += 1;
                                push_function_context();
                                push_method_context();
                                if is_generator {
                                    push_generator_context();
                                }
                                let body = if is_async_member {
                                    parse_statements(tokens, index)?
                                } else {
                                    with_cleared_forbidden_await_identifier(|| parse_statements(tokens, index))?
                                };
                                if is_generator {
                                    pop_generator_context();
                                }
                                pop_method_context();
                                pop_function_context();
                                pop_method_context();
                                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                    return Err(raise_parse_error_at!(tokens.get(*index)));
                                }
                                *index += 1;
                                if is_generator {
                                    if is_async_member {
                                        properties.push((
                                            Expr::StringLit(crate::unicode::utf8_to_utf16(&name)),
                                            Expr::AsyncGeneratorFunction(
                                                None,
                                                params,
                                                body,
                                                Some((method_start_byte, tokens[*index - 1].byte_offset + 1)),
                                            ),
                                            false,
                                            false,
                                        ));
                                    } else {
                                        properties.push((
                                            Expr::StringLit(crate::unicode::utf8_to_utf16(&name)),
                                            Expr::GeneratorFunction(
                                                None,
                                                params,
                                                body,
                                                Some((method_start_byte, tokens[*index - 1].byte_offset + 1)),
                                            ),
                                            false,
                                            false,
                                        ));
                                    }
                                } else if is_async_member {
                                    properties.push((
                                        Expr::StringLit(crate::unicode::utf8_to_utf16(&name)),
                                        Expr::AsyncFunction(
                                            None,
                                            params,
                                            body,
                                            Some((method_start_byte, tokens[*index - 1].byte_offset + 1)),
                                        ),
                                        false,
                                        false,
                                    ));
                                } else {
                                    properties.push((
                                        Expr::StringLit(crate::unicode::utf8_to_utf16(&name)),
                                        Expr::Function(None, params, body, Some((method_start_byte, tokens[*index - 1].byte_offset + 1))),
                                        false,
                                        false,
                                    ));
                                }
                                while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator | Token::Semicolon) {
                                    *index += 1;
                                }
                                if *index >= tokens.len() {
                                    return Err(raise_parse_error_at!(tokens.get(*index)));
                                }
                                if matches!(tokens[*index].token, Token::RBrace) {
                                    *index += 1;
                                    break;
                                }
                                if matches!(tokens[*index].token, Token::Comma) {
                                    *index += 1;
                                    continue;
                                }
                                continue;
                            }
                            is_shorthand_candidate = true;
                            *index += 1;
                            Expr::StringLit(crate::unicode::utf8_to_utf16(&name))
                        } else if let Some(Token::Number(n)) = tokens.get(*index).map(|t| t.token.clone()) {
                            *index += 1;
                            let s = crate::core::value_to_string(&crate::core::Value::Number(n));
                            Expr::StringLit(crate::unicode::utf8_to_utf16(&s))
                        } else if let Some(Token::BigInt(snum)) = tokens.get(*index).map(|t| t.token.clone()) {
                            *index += 1;
                            Expr::StringLit(crate::unicode::utf8_to_utf16(&snum))
                        } else if let Some(Token::StringLit(s)) = tokens.get(*index).map(|t| t.token.clone()) {
                            *index += 1;
                            Expr::StringLit(s)
                        } else if let Some(tok) = tokens.get(*index).map(|t| t.token.clone()) {
                            if let Some(id) = tok.as_identifier_string() {
                                if !is_getter
                                    && !is_setter
                                    && tokens.len() > *index + 1
                                    && matches!(tokens[*index + 1].token, Token::LParen)
                                {
                                    *index += 1;
                                    *index += 1;
                                    push_method_context();
                                    let params = if is_async_member {
                                        parse_parameters(tokens, index)?
                                    } else {
                                        with_cleared_forbidden_await_identifier(|| parse_parameters(tokens, index))?
                                    };
                                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                        pop_method_context();
                                        return Err(raise_parse_error_at!(tokens.get(*index)));
                                    }
                                    *index += 1;
                                    push_function_context();
                                    push_method_context();
                                    if is_generator {
                                        push_generator_context();
                                    }
                                    let body = if is_async_member {
                                        parse_statements(tokens, index)?
                                    } else {
                                        with_cleared_forbidden_await_identifier(|| parse_statements(tokens, index))?
                                    };
                                    if is_generator {
                                        pop_generator_context();
                                    }
                                    pop_method_context();
                                    pop_function_context();
                                    pop_method_context();
                                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                        return Err(raise_parse_error_at!(tokens.get(*index)));
                                    }
                                    *index += 1;
                                    if is_generator {
                                        if is_async_member {
                                            properties.push((
                                                Expr::StringLit(crate::unicode::utf8_to_utf16(&id)),
                                                Expr::AsyncGeneratorFunction(
                                                    None,
                                                    params,
                                                    body,
                                                    Some((method_start_byte, tokens[*index - 1].byte_offset + 1)),
                                                ),
                                                false,
                                                false,
                                            ));
                                        } else {
                                            properties.push((
                                                Expr::StringLit(crate::unicode::utf8_to_utf16(&id)),
                                                Expr::GeneratorFunction(
                                                    None,
                                                    params,
                                                    body,
                                                    Some((method_start_byte, tokens[*index - 1].byte_offset + 1)),
                                                ),
                                                false,
                                                false,
                                            ));
                                        }
                                    } else if is_async_member {
                                        properties.push((
                                            Expr::StringLit(crate::unicode::utf8_to_utf16(&id)),
                                            Expr::AsyncFunction(
                                                None,
                                                params,
                                                body,
                                                Some((method_start_byte, tokens[*index - 1].byte_offset + 1)),
                                            ),
                                            false,
                                            false,
                                        ));
                                    } else {
                                        properties.push((
                                            Expr::StringLit(crate::unicode::utf8_to_utf16(&id)),
                                            Expr::Function(
                                                None,
                                                params,
                                                body,
                                                Some((method_start_byte, tokens[*index - 1].byte_offset + 1)),
                                            ),
                                            false,
                                            false,
                                        ));
                                    }
                                    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator | Token::Semicolon)
                                    {
                                        *index += 1;
                                    }
                                    if *index >= tokens.len() {
                                        return Err(raise_parse_error_at!(tokens.get(*index)));
                                    }
                                    if matches!(tokens[*index].token, Token::RBrace) {
                                        *index += 1;
                                        break;
                                    }
                                    if matches!(tokens[*index].token, Token::Comma) {
                                        *index += 1;
                                        continue;
                                    }
                                    continue;
                                }
                                is_shorthand_candidate = true;
                                *index += 1;
                                Expr::StringLit(crate::unicode::utf8_to_utf16(&id))
                            } else if let Some(Token::Default) = tokens.get(*index).map(|t| t.token.clone()) {
                                *index += 1;
                                Expr::StringLit(crate::unicode::utf8_to_utf16("default"))
                            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                                key_is_computed = true;
                                *index += 1;
                                let expr = with_allowed_in(|| parse_assignment(tokens, index))?;
                                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                                    return Err(raise_parse_error_at!(tokens.get(*index)));
                                }
                                *index += 1;
                                expr
                            } else {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                        } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                            key_is_computed = true;
                            *index += 1;
                            let expr = with_allowed_in(|| parse_assignment(tokens, index))?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            expr
                        } else {
                            return Err(raise_parse_error_at!(tokens.get(*index)));
                        };
                        if !is_generator && *index < tokens.len() && matches!(tokens[*index].token, Token::Multiply) {
                            is_generator = true;
                            *index += 1;
                        }
                        if !is_getter && !is_setter && *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                            *index += 1;
                            push_method_context();
                            let params = if is_async_member {
                                parse_parameters(tokens, index)?
                            } else {
                                with_cleared_forbidden_await_identifier(|| parse_parameters(tokens, index))?
                            };
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                pop_method_context();
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            push_function_context();
                            push_method_context();
                            if is_generator {
                                push_generator_context();
                            }
                            let body = if is_async_member {
                                parse_statements(tokens, index)?
                            } else {
                                with_cleared_forbidden_await_identifier(|| parse_statements(tokens, index))?
                            };
                            if is_generator {
                                pop_generator_context();
                            }
                            pop_method_context();
                            pop_function_context();
                            pop_method_context();
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            if is_generator {
                                properties.push((
                                    key_expr,
                                    Expr::GeneratorFunction(
                                        None,
                                        params,
                                        body,
                                        Some((method_start_byte, tokens[*index - 1].byte_offset + 1)),
                                    ),
                                    key_is_computed,
                                    false,
                                ));
                            } else {
                                properties.push((
                                    key_expr,
                                    Expr::Function(None, params, body, Some((method_start_byte, tokens[*index - 1].byte_offset + 1))),
                                    key_is_computed,
                                    false,
                                ));
                            }
                            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator | Token::Semicolon) {
                                *index += 1;
                            }
                            if *index >= tokens.len() {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            if matches!(tokens[*index].token, Token::RBrace) {
                                *index += 1;
                                break;
                            }
                            if matches!(tokens[*index].token, Token::Comma) {
                                *index += 1;
                                continue;
                            }
                            continue;
                        }
                        if is_getter {
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LParen) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            push_function_context();
                            push_method_context();
                            let body = with_cleared_forbidden_await_identifier(|| parse_statements(tokens, index))?;
                            pop_method_context();
                            pop_function_context();
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            properties.push((
                                key_expr,
                                Expr::Getter(Box::new(Expr::Function(
                                    None,
                                    Vec::new(),
                                    body,
                                    Some((method_start_byte, tokens[*index - 1].byte_offset + 1)),
                                ))),
                                false,
                                false,
                            ));
                        } else if is_setter {
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LParen) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            push_method_context();
                            let params = with_cleared_forbidden_await_identifier(|| parse_parameters(tokens, index))?;
                            if params.len() != 1 {
                                pop_method_context();
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                pop_method_context();
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            push_function_context();
                            push_method_context();
                            let body = with_cleared_forbidden_await_identifier(|| parse_statements(tokens, index))?;
                            pop_method_context();
                            pop_function_context();
                            pop_method_context();
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            properties.push((
                                key_expr,
                                Expr::Setter(Box::new(Expr::Function(
                                    None,
                                    params,
                                    body,
                                    Some((method_start_byte, tokens[*index - 1].byte_offset + 1)),
                                ))),
                                false,
                                false,
                            ));
                        } else {
                            // Reject * and async prefix without a method body
                            if is_generator || is_async_member {
                                return Err(raise_parse_error!("SyntaxError: Expected method definition after '*' or 'async'"));
                            }
                            if *index < tokens.len() && matches!(tokens[*index].token, Token::Colon) {
                                // Check for duplicate __proto__ (B.3.1)
                                if !key_is_computed && let Expr::StringLit(ref s) = key_expr {
                                    let name = utf16_to_utf8(s);
                                    if name == "__proto__" {
                                        if has_proto {
                                            return Err(raise_parse_error!(
                                                "SyntaxError: Duplicate __proto__ fields are not allowed in object literals"
                                            ));
                                        }
                                        has_proto = true;
                                    }
                                }
                                *index += 1;
                                let value = parse_assignment(tokens, index)?;
                                properties.push((key_expr, value, key_is_computed, true));
                            } else {
                                if is_shorthand_candidate {
                                    if let Expr::StringLit(s) = &key_expr {
                                        let name = utf16_to_utf8(s);
                                        if name == "await" && forbid_await_identifier() {
                                            return Err(raise_parse_error!(
                                                "SyntaxError: 'await' is not allowed as an identifier in this context"
                                            ));
                                        }
                                        // Keywords cannot be used as shorthand properties
                                        // ({this}), ({null}), ({true}), ({false}) etc. are SyntaxErrors
                                        if is_reserved_identifier(&name) || matches!(name.as_str(), "this" | "null" | "true" | "false") {
                                            return Err(raise_parse_error!(format!(
                                                "SyntaxError: Unexpected reserved word '{}' in shorthand property",
                                                name
                                            )));
                                        }
                                        properties.push((key_expr, Expr::Var(name, None, None), key_is_computed, false));
                                    } else {
                                        return Err(raise_parse_error_at!(tokens.get(*index)));
                                    }
                                } else {
                                    return Err(raise_parse_error_at!(tokens.get(*index)));
                                }
                            }
                        }
                    }
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
                        *index += 1;
                    }
                }
            }
            Expr::Object(properties)
        }
        Token::LBracket => {
            log::trace!("parse_primary: entering LBracket at index {}", *index);
            log::trace!(
                "parse_primary: tokens at idx-1 {:?}, idx {:?}, idx+1 {:?}",
                tokens.get(*index).map(|t| &t.token),
                tokens.get(*index).map(|t| &t.token),
                tokens.get(*index + 1).map(|t| &t.token)
            );
            if *index < tokens.len()
                && matches!(tokens[*index].token, Token::RBracket)
                && *index > 0
                && matches!(tokens[*index - 1].token, Token::LBracket)
            {
                *index += 1;
                log::trace!("parse_primary: detected empty array (case: idx at ']') -> new idx {}", *index);
                Expr::Array(Vec::new())
            } else {
                log::trace!(
                    "parse_primary: starting array literal; next tokens (first 12): {:?}",
                    tokens.iter().take(12).collect::<Vec<_>>()
                );
                log::trace!("parse_primary: after '[' token at index {} -> {:?}", *index, tokens.get(*index));
                let mut elements = Vec::new();
                loop {
                    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator | Token::Semicolon) {
                        *index += 1;
                    }
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::RBracket) {
                        *index += 1;
                        log::trace!(
                            "parse_primary: completed array literal with {} elements; remaining tokens (first 12): {:?}",
                            elements.len(),
                            tokens.iter().take(12).collect::<Vec<_>>()
                        );
                        break;
                    }
                    log::trace!("parse_primary: array element next token: {:?}", tokens.get(*index));
                    if matches!(tokens[*index].token, Token::Comma) {
                        elements.push(None);
                        *index += 1;
                        continue;
                    }
                    let elem = parse_assignment(tokens, index)?;
                    elements.push(Some(elem));
                    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator | Token::Semicolon) {
                        *index += 1;
                    }
                    if *index >= tokens.len() {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                    if matches!(tokens[*index].token, Token::RBracket) {
                        *index += 1;
                        break;
                    } else if matches!(tokens[*index].token, Token::Comma) {
                        *index += 1;
                    } else {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                }
                Expr::Array(elements)
            }
        }
        Token::Function | Token::FunctionStar => {
            let func_start_byte = token_data.byte_offset;
            let mut is_generator = matches!(current, Token::FunctionStar);
            if !is_generator && *index < tokens.len() && matches!(tokens[*index].token, Token::Multiply) {
                is_generator = true;
                log::trace!("parse_primary: saw separate '*' token after 'function' - treating as generator");
                *index += 1;
            }
            log::trace!(
                "parse_primary: function expression, next tokens (first 8): {:?}",
                tokens.iter().take(8).collect::<Vec<_>>()
            );
            let name = if *index < tokens.len() {
                match &tokens[*index].token {
                    Token::Identifier(n) => {
                        let mut lookahead = *index + 1;
                        while lookahead < tokens.len() && matches!(tokens[lookahead].token, Token::LineTerminator) {
                            lookahead += 1;
                        }
                        if lookahead < tokens.len() && matches!(tokens[lookahead].token, Token::LParen) {
                            let name = n.clone();
                            log::trace!("parse_primary: treating '{}' as function name", name);
                            *index += 1;
                            Some(name)
                        } else {
                            None
                        }
                    }
                    Token::Await => {
                        let mut lookahead = *index + 1;
                        while lookahead < tokens.len() && matches!(tokens[lookahead].token, Token::LineTerminator) {
                            lookahead += 1;
                        }
                        if lookahead < tokens.len() && matches!(tokens[lookahead].token, Token::LParen) {
                            let name = "await".to_string();
                            log::trace!("parse_primary: treating 'await' as function name");
                            *index += 1;
                            Some(name)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            } else {
                None
            };
            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                *index += 1;
            }
            if *index < tokens.len()
                && (matches!(tokens[*index].token, Token::LParen) || matches!(tokens[*index].token, Token::Identifier(_)))
            {
                if matches!(tokens[*index].token, Token::LParen) {
                    *index += 1;
                }
                log::trace!(
                    "parse_primary: about to call parse_parameters; tokens (first 8): {:?}",
                    tokens.iter().take(8).collect::<Vec<_>>()
                );
                // Functions have their own `arguments`, clear static block restriction for params
                let saved_sb_fe = STATIC_BLOCK_CONTEXT.with(|c| {
                    let p = *c.borrow();
                    *c.borrow_mut() = 0;
                    p
                });
                let params = if is_generator {
                    push_generator_context();
                    let p = with_cleared_forbidden_await_identifier(|| parse_parameters(tokens, index))?;
                    pop_generator_context();
                    p
                } else {
                    let saved = GENERATOR_CONTEXT.with(|c| {
                        let old = *c.borrow();
                        *c.borrow_mut() = 0;
                        old
                    });
                    let p = with_cleared_await_context(|| with_cleared_forbidden_await_identifier(|| parse_parameters(tokens, index)))?;
                    GENERATOR_CONTEXT.with(|c| *c.borrow_mut() = saved);
                    p
                };
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                push_function_context();
                let body = if is_generator {
                    push_generator_context();
                    let b = with_cleared_forbidden_await_identifier(|| parse_statements(tokens, index))?;
                    pop_generator_context();
                    b
                } else {
                    with_cleared_await_context(|| with_cleared_forbidden_await_identifier(|| parse_statements(tokens, index)))?
                };
                pop_function_context();
                STATIC_BLOCK_CONTEXT.with(|c| *c.borrow_mut() = saved_sb_fe);
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                if is_generator {
                    log::trace!("parse_primary: constructed GeneratorFunction name={:?} params={:?}", name, params);
                    Expr::GeneratorFunction(name, params, body, Some((func_start_byte, tokens[*index - 1].byte_offset + 1)))
                } else {
                    log::trace!("parse_primary: constructed Function name={:?} params={:?}", name, params);
                    Expr::Function(name, params, body, Some((func_start_byte, tokens[*index - 1].byte_offset + 1)))
                }
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::RParen) {
                *index += 1;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                push_function_context();
                let body = if is_generator {
                    push_generator_context();
                    let b = with_cleared_forbidden_await_identifier(|| parse_statements(tokens, index))?;
                    pop_generator_context();
                    b
                } else {
                    with_cleared_await_context(|| with_cleared_forbidden_await_identifier(|| parse_statements(tokens, index)))?
                };
                pop_function_context();
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                if is_generator {
                    log::trace!("parse_primary: constructed GeneratorFunction name={:?} params=Vec::new()", name);
                    Expr::GeneratorFunction(name, Vec::new(), body, Some((func_start_byte, tokens[*index - 1].byte_offset + 1)))
                } else {
                    log::trace!("parse_primary: constructed Function name={:?} params=Vec::new()", name);
                    Expr::Function(name, Vec::new(), body, Some((func_start_byte, tokens[*index - 1].byte_offset + 1)))
                }
            } else {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
        }
        Token::Async => {
            if raw_identifier_source_has_escape(token_data) {
                let line = token_data.line;
                let column = token_data.column;
                Expr::Var("async".to_string(), Some(line), Some(column))
            } else {
                let start = *index - 1;
                let next = *index;
                log::trace!(
                    "parse_primary: Token::Async start={} *index={} tokens_slice={:?}",
                    start,
                    *index,
                    tokens.iter().skip(start).take(4).collect::<Vec<_>>()
                );
                let mut is_generator = false;
                if next < tokens.len()
                    && (matches!(tokens[next].token, Token::Function) || matches!(tokens[next].token, Token::FunctionStar))
                {
                    log::trace!("parse_primary (async): detected 'async function' at start={} next={}", start, next);
                    if matches!(tokens[next].token, Token::FunctionStar) {
                        is_generator = true;
                        *index = next + 1;
                    } else {
                        *index = next + 1;
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::Multiply) {
                            is_generator = true;
                            *index += 1;
                        }
                    }
                    let name = if *index < tokens.len() {
                        if let Token::Identifier(n) = &tokens[*index].token {
                            let mut idx = *index + 1;
                            while idx < tokens.len() && matches!(tokens[idx].token, Token::LineTerminator) {
                                idx += 1;
                            }
                            log::trace!(
                                "parse_primary (async): potential name='{}' idx={} token_after_name={:?}",
                                n,
                                *index,
                                tokens.get(idx)
                            );
                            if idx < tokens.len() && matches!(tokens[idx].token, Token::LParen) {
                                let name = n.clone();
                                log::trace!("parse_primary: treating '{}' as async function name", name);
                                *index += 1;
                                Some(name)
                            } else {
                                log::trace!("parse_primary (async): identifier not a name (no '(' after) at idx {}", idx);
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                        log::trace!("parse_primary (async): parsing parameters at idx {}", *index);
                        *index += 1;
                        let params = {
                            let saved = GENERATOR_CONTEXT.with(|c| {
                                let old = *c.borrow();
                                if !is_generator {
                                    *c.borrow_mut() = 0;
                                }
                                old
                            });
                            let saved_sb_af = STATIC_BLOCK_CONTEXT.with(|c| {
                                let p = *c.borrow();
                                *c.borrow_mut() = 0;
                                p
                            });
                            let p = parse_parameters(tokens, index)?;
                            STATIC_BLOCK_CONTEXT.with(|c| *c.borrow_mut() = saved_sb_af);
                            GENERATOR_CONTEXT.with(|c| *c.borrow_mut() = saved);
                            p
                        };
                        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                            log::trace!(
                                "parse_primary (async): expected '{{' after params but found {:?} at idx {}",
                                tokens.get(*index),
                                *index
                            );
                            return Err(raise_parse_error_at!(tokens.get(*index)));
                        }
                        *index += 1;
                        push_await_context();
                        push_function_context();
                        if is_generator {
                            push_generator_context();
                        }
                        let body = parse_statements(tokens, index)?;
                        if is_generator {
                            pop_generator_context();
                        }
                        pop_function_context();
                        pop_await_context();
                        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                            return Err(raise_parse_error_at!(tokens.get(*index)));
                        }
                        *index += 1;
                        if is_generator {
                            log::trace!("parse_primary: constructed AsyncGeneratorFunction name={name:?} params={params:?}");
                            Expr::AsyncGeneratorFunction(
                                name,
                                params,
                                body,
                                Some((tokens[start].byte_offset, tokens[*index - 1].byte_offset + 1)),
                            )
                        } else {
                            log::trace!("parse_primary: constructed AsyncFunction name={name:?} params={params:?}");
                            Expr::AsyncFunction(
                                name,
                                params,
                                body,
                                Some((tokens[start].byte_offset, tokens[*index - 1].byte_offset + 1)),
                            )
                        }
                    } else {
                        log::trace!(
                            "parse_primary (async): missing '(' after 'function' at idx {} token={:?}",
                            *index,
                            tokens.get(*index)
                        );
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                } else if !suppress_arrow && *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                    log::trace!("parse_primary (async): detected '(' => possible async arrow at idx {}", *index);
                    *index += 1;
                    let saved_idx = *index;
                    if let Ok(p) = parse_parameters(tokens, index) {
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                            *index += 1;
                            if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
                                *index += 1;
                                push_await_context();
                                push_function_context();
                                let body = parse_statement_block(tokens, index)?;
                                pop_function_context();
                                pop_await_context();
                                return Ok(Expr::AsyncArrowFunction(p, body));
                            } else {
                                push_await_context();
                                let body_expr = parse_assignment(tokens, index);
                                pop_await_context();
                                let body_expr = body_expr?;
                                return Ok(Expr::AsyncArrowFunction(
                                    p,
                                    vec![Statement::from(StatementKind::Return(Some(body_expr)))],
                                ));
                            }
                        } else {
                            *index = saved_idx;
                        }
                    }
                    let mut params: Vec<DestructuringElement> = Vec::new();
                    let mut is_arrow = false;
                    if matches!(tokens.get(*index).map(|t| &t.token), Some(&Token::RParen)) {
                        *index += 1;
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                            *index += 1;
                            is_arrow = true;
                        } else {
                            return Err(raise_parse_error_at!(tokens.get(*index)));
                        }
                    } else {
                        let mut param_names: Vec<DestructuringElement> = Vec::new();
                        let mut valid = true;
                        loop {
                            if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
                                *index += 1;
                                param_names.push(DestructuringElement::Variable(name, None));
                                if *index >= tokens.len() {
                                    valid = false;
                                    break;
                                }
                                if matches!(tokens[*index].token, Token::RParen) {
                                    *index += 1;
                                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                                        *index += 1;
                                        is_arrow = true;
                                    } else {
                                        valid = false;
                                    }
                                    break;
                                } else if matches!(tokens[*index].token, Token::Comma) {
                                    *index += 1;
                                } else {
                                    valid = false;
                                    break;
                                }
                            } else {
                                valid = false;
                                break;
                            }
                        }
                        if !valid || !is_arrow {
                            return Err(raise_parse_error_at!(tokens.get(*index)));
                        }
                        params = param_names;
                    }
                    if is_arrow {
                        Expr::AsyncArrowFunction(params, parse_async_arrow_body(tokens, index)?)
                    } else {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                } else if !suppress_arrow && *index < tokens.len() && matches!(tokens[*index].token, Token::Identifier(_)) {
                    if let Token::Identifier(name) = &tokens[*index].token {
                        let ident_name = name.clone();
                        let j = *index + 1;
                        if j < tokens.len() && matches!(tokens[j].token, Token::Arrow) {
                            *index = j + 1;
                            return Ok(Expr::AsyncArrowFunction(
                                vec![DestructuringElement::Variable(ident_name, None)],
                                parse_async_arrow_body(tokens, index)?,
                            ));
                        }
                    }
                    let line = token_data.line;
                    let column = token_data.column;
                    let mut expr = Expr::Var("async".to_string(), Some(line), Some(column));
                    if !suppress_arrow && *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                        *index += 1;
                        let body = parse_arrow_body(tokens, index)?;
                        expr = Expr::ArrowFunction(vec![DestructuringElement::Variable("async".to_string(), None)], body);
                    }
                    expr
                } else {
                    let line = token_data.line;
                    let column = token_data.column;
                    let mut expr = Expr::Var("async".to_string(), Some(line), Some(column));
                    if !suppress_arrow && *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                        *index += 1;
                        let body = parse_arrow_body(tokens, index)?;
                        expr = Expr::ArrowFunction(vec![DestructuringElement::Variable("async".to_string(), None)], body);
                    }
                    expr
                }
            }
        }
        Token::LParen => {
            log::trace!(
                "parse_primary: entered LParen branch at idx {} tokens={:?}",
                *index,
                tokens.iter().skip(*index).take(8).collect::<Vec<_>>()
            );
            // ClassHeritage: extends LeftHandSideExpression — arrows not allowed
            // at the outermost paren level.
            if suppress_arrow {
                // Parse as grouping expression only — no arrow detection
                let expr_inner = parse_expression(tokens, index)?;
                while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                expr_inner
            } else {
                if *index < tokens.len() && matches!(tokens[*index].token, Token::RParen) {
                    let prev = if *index >= 1 { Some(&tokens[*index - 1]) } else { None };
                    log::trace!("paren-rcase: idx={} prev={:?} token_at_idx={:?}", *index, prev, tokens.get(*index));
                    if let Some(prev_td) = prev
                        && matches!(prev_td.token, Token::LParen)
                    {
                        let next = *index + 1;
                        log::trace!("paren-rcase: next={} token_next={:?}", next, tokens.get(next));
                        if next < tokens.len() && matches!(tokens[next].token, Token::Arrow) {
                            *index = next + 1;
                            let body = parse_arrow_body(tokens, index)?;
                            log::trace!("constructing arrow (empty paren via rcase) params=Vec::new()");
                            return Ok(Expr::ArrowFunction(Vec::new(), body));
                        } else {
                            log::trace!("paren-rcase: not arrow; token_next={:?}", tokens.get(next));
                        }
                    }
                }
                {
                    if *index < tokens.len() && !matches!(tokens[*index].token, Token::Spread) {
                        let mut j = *index + 1;
                        while j < tokens.len() && matches!(tokens[j].token, Token::LineTerminator) {
                            j += 1;
                        }
                        if j < tokens.len() && matches!(tokens[j].token, Token::Identifier(_)) {
                            let mut k = j + 1;
                            while k < tokens.len() && matches!(tokens[k].token, Token::LineTerminator) {
                                k += 1;
                            }
                            if k < tokens.len() && matches!(tokens[k].token, Token::RParen) {
                                let m = k + 1;
                                if m < tokens.len()
                                    && matches!(tokens[m].token, Token::Arrow)
                                    && let Token::Identifier(name) = &tokens[j].token
                                {
                                    *index = m + 1;
                                    let body = parse_arrow_body(tokens, index)?;
                                    log::trace!(
                                        "constructing arrow (single-id fast-path) params={:?}",
                                        vec![DestructuringElement::Variable(name.clone(), None)]
                                    );
                                    return Ok(Expr::ArrowFunction(vec![DestructuringElement::Variable(name.clone(), None)], body));
                                }
                            }
                        }
                    }
                }
                {
                    {
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::RParen) {
                            let next = *index + 1;
                            log::trace!(
                                "empty-paren-fastpath: index={} token_at_index={:?} next={} token_next={:?}",
                                *index,
                                tokens.get(*index),
                                next,
                                tokens.get(next)
                            );
                            if next < tokens.len() && matches!(tokens[next].token, Token::Arrow) {
                                *index = next + 1;
                                let body = parse_arrow_body(tokens, index)?;
                                log::trace!("constructing arrow (empty paren) params=Vec::new()");
                                return Ok(Expr::ArrowFunction(Vec::new(), body));
                            } else {
                                log::trace!("empty-paren-fastpath: not arrow, skipped (token_next={:?})", tokens.get(next));
                            }
                        } else {
                            log::trace!(
                                "empty-paren-fastpath: index did not point to RParen (token={:?})",
                                tokens.get(*index)
                            );
                        }
                    }
                    let mut depth = 1usize;
                    let mut j = *index + 1;
                    while j < tokens.len() && depth > 0 {
                        match tokens[j].token {
                            Token::LParen => depth += 1,
                            Token::RParen => depth -= 1,
                            _ => {}
                        }
                        if depth > 0 {
                            j += 1;
                        }
                    }
                    if depth == 0 {
                        let next = j + 1;
                        if next < tokens.len() && matches!(tokens[next].token, Token::Arrow) {
                            log::trace!(
                                "paren-arrow-check: index={}, j={}, next={} token_at_index={:?} token_at_j={:?}",
                                *index,
                                j,
                                next,
                                tokens.get(*index),
                                tokens.get(j)
                            );
                            let mut t = *index;
                            log::trace!(
                                "paren-arrow: index={} t={} token_at_t={:?} token_at_j_plus_one={:?}",
                                *index,
                                t,
                                tokens.get(t),
                                tokens.get(j + 1)
                            );
                            if let Ok(params) = parse_parameters(tokens, &mut t)
                                && t == j + 1
                            {
                                *index = next + 1;
                                let body = parse_arrow_body(tokens, index)?;
                                log::trace!("constructing arrow (paren params) params={:?}", params);
                                return Ok(Expr::ArrowFunction(params, body));
                            }
                        }
                    }
                }
                let expr_inner = parse_expression(tokens, index)?;
                while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                if *index < tokens.len() && get_assignment_ctor(&tokens[*index].token).is_some() && matches!(expr_inner, Expr::Object(_)) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                match expr_inner {
                    // Parenthesized identifiers are not IdentifierReference in assignment
                    // named-evaluation rules; clear marker metadata used by compiler.
                    Expr::Var(name, _, _) => Expr::Var(name, None, None),
                    other => other,
                }
            } // end else (non-heritage arrow suppression)
        }
        _ => {
            if *index < tokens.len() {
                log::debug!(
                    "parse_expression unexpected token: {:?}; remaining tokens: {:?}",
                    tokens[*index].token,
                    tokens
                );
            } else {
                log::debug!("parse_expression unexpected end of tokens; tokens empty");
            }
            return Err(raise_parse_error_at!(tokens.get(*index - 1)));
        }
    };
    while *index < tokens.len() {
        log::trace!("parse_primary: postfix loop at idx {} -> {:?}", *index, tokens.get(*index));
        if matches!(tokens[*index].token, Token::LineTerminator) {
            let mut look = *index + 1;
            while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
                look += 1;
            }
            if look < tokens.len() {
                match &tokens[look].token {
                    Token::Dot | Token::LBracket | Token::LParen | Token::OptionalChain | Token::TemplateString(_) => {
                        *index = look;
                        continue;
                    }
                    Token::Increment | Token::Decrement => break,
                    _ => break,
                }
            }
            break;
        }
        if *index >= tokens.len() {
            break;
        }
        match &tokens[*index].token {
            Token::LBracket => {
                *index += 1;
                let index_expr = parse_expression(tokens, index)?;
                while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                if contains_optional_chain(&expr) {
                    expr = Expr::OptionalIndex(Box::new(expr), Box::new(index_expr));
                } else {
                    expr = Expr::Index(Box::new(expr), Box::new(index_expr));
                }
            }
            Token::Dot => {
                *index += 1;
                while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                if *index >= tokens.len() {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                if let Some(prop) = tokens[*index].token.as_identifier_string() {
                    *index += 1;
                    if contains_optional_chain(&expr) {
                        expr = Expr::OptionalProperty(Box::new(expr), prop);
                    } else {
                        expr = Expr::Property(Box::new(expr), prop);
                    }
                } else if let Token::PrivateIdentifier(prop) = &tokens[*index].token {
                    let invalid = PRIVATE_NAME_STACK.with(|s| {
                        let stack = s.borrow();
                        !stack.iter().rev().any(|rc| rc.borrow().contains(prop))
                    });
                    if invalid {
                        let msg = format!("Private field '#{}' must be declared in an enclosing class", prop);
                        return Err(raise_parse_error_with_token!(tokens[*index], msg));
                    }
                    let prop = super::make_private_key(prop);
                    *index += 1;
                    if contains_optional_chain(&expr) {
                        expr = Expr::OptionalPrivateMember(Box::new(expr), prop);
                    } else {
                        expr = Expr::PrivateMember(Box::new(expr), prop);
                    }
                } else {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
            }
            Token::OptionalChain => {
                *index += 1;
                if *index >= tokens.len() {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                if matches!(tokens[*index].token, Token::LParen) {
                    *index += 1;
                    let mut args = Vec::new();
                    if *index < tokens.len() && !matches!(tokens[*index].token, Token::RParen) {
                        loop {
                            let arg = parse_assignment(tokens, index)?;
                            args.push(arg);
                            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                                *index += 1;
                            }
                            if *index >= tokens.len() {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            if matches!(tokens[*index].token, Token::RParen) {
                                break;
                            }
                            if !matches!(tokens[*index].token, Token::Comma) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                        }
                    }
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                    *index += 1;
                    expr = Expr::OptionalCall(Box::new(expr), args);
                } else if matches!(tokens[*index].token, Token::Identifier(_)) {
                    if let Some(prop) = tokens[*index].token.as_identifier_string() {
                        *index += 1;
                        expr = Expr::OptionalProperty(Box::new(expr), prop);
                    } else {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                } else if let Token::PrivateIdentifier(prop) = &tokens[*index].token {
                    let invalid = PRIVATE_NAME_STACK.with(|s| {
                        let stack = s.borrow();
                        !stack.iter().rev().any(|rc| rc.borrow().contains(prop))
                    });
                    if invalid {
                        let msg = format!("Private field '#{prop}' must be declared in an enclosing class");
                        return Err(raise_parse_error_with_token!(tokens[*index], msg));
                    }
                    let prop = super::make_private_key(prop);
                    *index += 1;
                    expr = Expr::OptionalPrivateMember(Box::new(expr), prop);
                } else if matches!(tokens[*index].token, Token::LBracket) {
                    *index += 1;
                    let index_expr = parse_expression(tokens, index)?;
                    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                        *index += 1;
                    }
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                    *index += 1;
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                        *index += 1;
                        let mut args = Vec::new();
                        if *index < tokens.len() && !matches!(tokens[*index].token, Token::RParen) {
                            loop {
                                let arg = parse_assignment(tokens, index)?;
                                args.push(arg);
                                while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                                    *index += 1;
                                }
                                if *index >= tokens.len() {
                                    return Err(raise_parse_error_at!(tokens.get(*index)));
                                }
                                if matches!(tokens[*index].token, Token::RParen) {
                                    break;
                                }
                                if !matches!(tokens[*index].token, Token::Comma) {
                                    return Err(raise_parse_error_at!(tokens.get(*index)));
                                }
                                *index += 1;
                            }
                        }
                        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                            return Err(raise_parse_error_at!(tokens.get(*index)));
                        }
                        *index += 1;
                        if args.len() == 1
                            && let Expr::Comma(_, _) = &args[0]
                        {
                            let first = args.remove(0);
                            let new_args = flatten_commas(first);
                            args.extend(new_args);
                        }
                        expr = Expr::OptionalCall(Box::new(Expr::Index(Box::new(expr), Box::new(index_expr))), args);
                    } else {
                        expr = Expr::OptionalIndex(Box::new(expr), Box::new(index_expr));
                    }
                } else {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
            }
            Token::LParen => {
                if !allow_call {
                    break;
                }
                *index += 1;
                let mut args = Vec::new();
                if *index < tokens.len() && !matches!(tokens[*index].token, Token::RParen) {
                    loop {
                        let arg = parse_assignment(tokens, index)?;
                        args.push(arg);
                        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                            *index += 1;
                        }
                        if *index >= tokens.len() {
                            return Err(raise_parse_error_at!(tokens.get(*index)));
                        }
                        if matches!(tokens[*index].token, Token::RParen) {
                            break;
                        }
                        if !matches!(tokens[*index].token, Token::Comma) {
                            return Err(raise_parse_error_at!(tokens.get(*index)));
                        }
                        *index += 1;
                        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                            *index += 1;
                        }
                        if *index >= tokens.len() {
                            return Err(raise_parse_error_at!(tokens.get(*index)));
                        }
                        if matches!(tokens[*index].token, Token::RParen) {
                            break;
                        }
                    }
                }
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                if args.len() == 1
                    && let Expr::Comma(_, _) = &args[0]
                {
                    let first = args.remove(0);
                    let new_args = flatten_commas(first);
                    args.extend(new_args);
                }
                if contains_optional_chain(&expr) {
                    expr = Expr::OptionalCall(Box::new(expr), args);
                } else {
                    expr = Expr::Call(Box::new(expr), args);
                }
            }
            Token::Increment => {
                *index += 1;
                expr = Expr::PostIncrement(Box::new(expr));
            }
            Token::Decrement => {
                *index += 1;
                expr = Expr::PostDecrement(Box::new(expr));
            }
            Token::TemplateString(parts) => {
                if contains_optional_chain(&expr) {
                    return Err(raise_parse_error!("Tagged template cannot be used in an optional chain"));
                }
                let parts = parts.clone();
                *index += 1;
                let site_id = next_template_site_id();
                let mut cooked_strings: Vec<Option<Vec<u16>>> = Vec::new();
                let mut raw_strings: Vec<Vec<u16>> = Vec::new();
                let mut exprs = Vec::new();
                for part in parts {
                    match part {
                        TemplatePart::String(cooked_opt, raw) => {
                            cooked_strings.push(cooked_opt.clone());
                            raw_strings.push(raw.clone());
                        }
                        TemplatePart::Expr(expr_tokens) => {
                            let expr_tokens = expr_tokens.clone();
                            let e = parse_expression(&expr_tokens, &mut 0)?;
                            exprs.push(e);
                        }
                    }
                }
                expr = Expr::TaggedTemplate(Box::new(expr), site_id, cooked_strings, raw_strings, exprs);
            }
            _ => break,
        }
    }
    Ok(expr)
}
fn parse_arrow_body(tokens: &[TokenData], index: &mut usize) -> Result<Vec<Statement>, JSError> {
    parse_arrow_body_inner(tokens, index, false)
}
fn parse_async_arrow_body(tokens: &[TokenData], index: &mut usize) -> Result<Vec<Statement>, JSError> {
    parse_arrow_body_inner(tokens, index, true)
}
fn parse_arrow_body_inner(tokens: &[TokenData], index: &mut usize, is_async: bool) -> Result<Vec<Statement>, JSError> {
    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
        *index += 1;
        push_arrow_function_context();
        let body = if is_async {
            push_await_context();
            let r = with_cleared_forbidden_await_identifier(|| parse_statements(tokens, index));
            pop_await_context();
            r?
        } else {
            with_cleared_await_context(|| with_cleared_forbidden_await_identifier(|| parse_statements(tokens, index)))?
        };
        pop_arrow_function_context();
        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
        *index += 1;
        Ok(body)
    } else {
        // Concise arrow body — return is implicit, no need for function context
        let expr = if is_async {
            push_await_context();
            let r = with_cleared_forbidden_await_identifier(|| parse_assignment(tokens, index));
            pop_await_context();
            r?
        } else {
            with_cleared_await_context(|| with_cleared_forbidden_await_identifier(|| parse_assignment(tokens, index)))?
        };
        Ok(vec![Statement::from(StatementKind::Return(Some(expr)))])
    }
}
pub fn parse_array_destructuring_pattern(tokens: &[TokenData], index: &mut usize) -> Result<Vec<DestructuringElement>, JSError> {
    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBracket) {
        return Err(raise_parse_error_at!(tokens.get(*index)));
    }
    log::trace!(
        "parse_array_destructuring_pattern start tokens (first 20): {:?}",
        tokens.iter().skip(*index).take(20).collect::<Vec<_>>()
    );
    *index += 1;
    let mut pattern = Vec::new();
    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < tokens.len() && matches!(tokens[*index].token, Token::RBracket) {
        *index += 1;
        return Ok(pattern);
    }
    loop {
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index < tokens.len() && matches!(tokens[*index].token, Token::Spread) {
            *index += 1;
            if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
                if strict_binding_checks() && (name == "eval" || name == "arguments") {
                    return Err(raise_parse_error_with_token!(
                        tokens[*index],
                        format!("'{}' can't be defined or assigned to in strict mode code", name)
                    ));
                }
                *index += 1;
                pattern.push(DestructuringElement::Rest(name));
            } else if *index < tokens.len()
                && matches!(tokens[*index].token, Token::Await)
                && !in_await_context()
                && !forbid_await_identifier()
            {
                *index += 1;
                pattern.push(DestructuringElement::Rest("await".to_string()));
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                let nested_pattern = parse_array_destructuring_pattern(tokens, index)?;
                let inner = DestructuringElement::NestedArray(nested_pattern, None);
                pattern.push(DestructuringElement::RestPattern(Box::new(inner)));
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
                let nested_pattern = parse_object_destructuring_pattern(tokens, index)?;
                let inner = DestructuringElement::NestedObject(nested_pattern, None);
                pattern.push(DestructuringElement::RestPattern(Box::new(inner)));
            } else {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            *index += 1;
            break;
        } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
            pattern.push(DestructuringElement::Empty);
        } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
            let nested_pattern = parse_array_destructuring_pattern(tokens, index)?;
            let mut default_expr: Option<Box<Expr>> = None;
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                *index += 1;
                let mut depth: i32 = 0;
                let mut init_tokens: Vec<TokenData> = Vec::new();
                while *index < tokens.len() {
                    if depth == 0 && (matches!(tokens[*index].token, Token::Comma) || matches!(tokens[*index].token, Token::RBracket)) {
                        break;
                    }
                    match tokens[*index].token {
                        Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                        Token::RParen | Token::RBracket | Token::RBrace => depth -= 1,
                        _ => {}
                    }
                    init_tokens.push(tokens[*index].clone());
                    *index += 1;
                }
                if !init_tokens.is_empty() {
                    let expr = parse_expression(&init_tokens, &mut 0)?;
                    default_expr = Some(Box::new(expr));
                }
            }
            pattern.push(DestructuringElement::NestedArray(nested_pattern, default_expr));
        } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
            let nested_pattern = parse_object_destructuring_pattern(tokens, index)?;
            let mut default_expr: Option<Box<Expr>> = None;
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                *index += 1;
                let mut depth: i32 = 0;
                let mut init_tokens: Vec<TokenData> = Vec::new();
                while *index < tokens.len() {
                    if depth == 0 && (matches!(tokens[*index].token, Token::Comma) || matches!(tokens[*index].token, Token::RBracket)) {
                        break;
                    }
                    match tokens[*index].token {
                        Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                        Token::RParen | Token::RBracket | Token::RBrace => depth -= 1,
                        _ => {}
                    }
                    init_tokens.push(tokens[*index].clone());
                    *index += 1;
                }
                if !init_tokens.is_empty() {
                    let expr = parse_expression(&init_tokens, &mut 0)?;
                    default_expr = Some(Box::new(expr));
                }
            }
            pattern.push(DestructuringElement::NestedObject(nested_pattern, default_expr));
        } else if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
            if strict_binding_checks() && (name == "eval" || name == "arguments") {
                return Err(raise_parse_error_with_token!(
                    tokens[*index],
                    format!("'{}' can't be defined or assigned to in strict mode code", name)
                ));
            }
            *index += 1;
            let mut default_expr: Option<Box<Expr>> = None;
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                *index += 1;
                let mut depth: i32 = 0;
                let mut init_tokens: Vec<TokenData> = Vec::new();
                while *index < tokens.len() {
                    if depth == 0 && (matches!(tokens[*index].token, Token::Comma) || matches!(tokens[*index].token, Token::RBracket)) {
                        break;
                    }
                    match tokens[*index].token {
                        Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                        Token::RParen | Token::RBracket | Token::RBrace => depth -= 1,
                        _ => {}
                    }
                    init_tokens.push(tokens[*index].clone());
                    *index += 1;
                }
                if !init_tokens.is_empty() {
                    let tmp = init_tokens.clone();
                    log::trace!("parse_array_destructuring_pattern: default init tokens (tokens): {:?}", tmp);
                    log::trace!(
                        "parse_array_destructuring_pattern: default init tokens (tokens.tokens): {:?}",
                        tmp.iter().map(|t| &t.token).collect::<Vec<_>>()
                    );
                    let expr = parse_expression(&tmp, &mut 0)?;
                    default_expr = Some(Box::new(expr));
                }
            }
            pattern.push(DestructuringElement::Variable(name, default_expr));
        } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Await) && !in_await_context() && !forbid_await_identifier()
        {
            *index += 1;
            let mut default_expr: Option<Box<Expr>> = None;
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                *index += 1;
                let mut depth: i32 = 0;
                let mut init_tokens: Vec<TokenData> = Vec::new();
                while *index < tokens.len() {
                    if depth == 0 && (matches!(tokens[*index].token, Token::Comma) || matches!(tokens[*index].token, Token::RBracket)) {
                        break;
                    }
                    match tokens[*index].token {
                        Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                        Token::RParen | Token::RBracket | Token::RBrace => depth -= 1,
                        _ => {}
                    }
                    init_tokens.push(tokens[*index].clone());
                    *index += 1;
                }
                if !init_tokens.is_empty() {
                    let expr = parse_expression(&init_tokens, &mut 0)?;
                    default_expr = Some(Box::new(expr));
                }
            }
            pattern.push(DestructuringElement::Variable("await".to_string(), default_expr));
        } else {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index >= tokens.len() {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
        if matches!(tokens[*index].token, Token::RBracket) {
            *index += 1;
            break;
        } else if matches!(tokens[*index].token, Token::Comma) {
            *index += 1;
            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                *index += 1;
            }
            if *index < tokens.len() && matches!(tokens[*index].token, Token::RBracket) {
                *index += 1;
                break;
            }
        } else {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
    }
    Ok(pattern)
}
pub fn parse_object_destructuring_pattern(tokens: &[TokenData], index: &mut usize) -> Result<Vec<DestructuringElement>, JSError> {
    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
        return Err(raise_parse_error_at!(tokens.get(*index)));
    }
    *index += 1;
    let mut pattern = Vec::new();
    log::trace!(
        "parse_object_destructuring_pattern: tokens immediately after '{{' (first 8): {:?}",
        tokens.iter().take(8).collect::<Vec<_>>()
    );
    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < tokens.len() && matches!(tokens[*index].token, Token::RBrace) {
        *index += 1;
        return Ok(pattern);
    }
    loop {
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index < tokens.len() && matches!(tokens[*index].token, Token::RBrace) {
            *index += 1;
            break;
        }
        if *index < tokens.len() && matches!(tokens[*index].token, Token::Spread) {
            *index += 1;
            if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
                if strict_binding_checks() && (name == "eval" || name == "arguments") {
                    return Err(raise_parse_error_with_token!(
                        tokens[*index],
                        format!("'{}' can't be defined or assigned to in strict mode code", name)
                    ));
                }
                *index += 1;
                pattern.push(DestructuringElement::Rest(name));
            } else {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            *index += 1;
            break;
        } else {
            let mut key_name: Option<String> = None;
            let mut computed_key: Option<Expr> = None;
            let mut is_identifier_key = false;
            if matches!(tokens[*index].token, Token::LBracket) {
                *index += 1;
                let mut depth: i32 = 1;
                let mut expr_tokens: Vec<TokenData> = Vec::new();
                while *index < tokens.len() {
                    match tokens[*index].token {
                        Token::LBracket => {
                            depth += 1;
                            expr_tokens.push(tokens[*index].clone());
                        }
                        Token::RBracket => {
                            depth -= 1;
                            if depth == 0 {
                                *index += 1;
                                break;
                            }
                            expr_tokens.push(tokens[*index].clone());
                        }
                        _ => expr_tokens.push(tokens[*index].clone()),
                    }
                    *index += 1;
                }
                if depth != 0 {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                let expr = parse_expression(&expr_tokens, &mut 0)?;
                computed_key = Some(expr);
            } else if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
                *index += 1;
                key_name = Some(name);
                is_identifier_key = true;
            } else if *index < tokens.len()
                && matches!(tokens[*index].token, Token::Await)
                && !in_await_context()
                && !forbid_await_identifier()
            {
                *index += 1;
                key_name = Some("await".to_string());
                is_identifier_key = true;
            } else if let Some(Token::Number(n)) = tokens.get(*index).map(|t| t.token.clone()) {
                *index += 1;
                key_name = Some(n.to_string());
            } else if let Some(Token::BigInt(s)) = tokens.get(*index).map(|t| t.token.clone()) {
                *index += 1;
                key_name = Some(s);
            } else if let Some(Token::StringLit(s)) = tokens.get(*index).map(|t| t.token.clone()) {
                *index += 1;
                key_name = Some(utf16_to_utf8(&s));
            } else if let Some(name) = tokens.get(*index).and_then(|t| t.token.as_identifier_string()) {
                *index += 1;
                key_name = Some(name);
                is_identifier_key = true;
            } else {
                log::trace!("expected property key but got {:?}", tokens.get(*index));
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            let value = if *index < tokens.len() && matches!(tokens[*index].token, Token::Colon) {
                *index += 1;
                if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                    let nested = parse_array_destructuring_pattern(tokens, index)?;
                    let mut nested_default: Option<Box<Expr>> = None;
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                        *index += 1;
                        let mut depth: i32 = 0;
                        let mut init_tokens: Vec<TokenData> = Vec::new();
                        while *index < tokens.len() {
                            if depth == 0 && (matches!(tokens[*index].token, Token::Comma) || matches!(tokens[*index].token, Token::RBrace))
                            {
                                break;
                            }
                            match tokens[*index].token {
                                Token::LParen | Token::LBracket | Token::LBrace => {
                                    depth += 1;
                                }
                                Token::RParen | Token::RBracket | Token::RBrace => {
                                    depth -= 1;
                                }
                                _ => {}
                            }
                            init_tokens.push(tokens[*index].clone());
                            *index += 1;
                        }
                        if !init_tokens.is_empty() {
                            let expr = parse_expression(&init_tokens, &mut 0)?;
                            nested_default = Some(Box::new(expr));
                        }
                    }
                    DestructuringElement::NestedArray(nested, nested_default)
                } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
                    let nested = parse_object_destructuring_pattern(tokens, index)?;
                    let mut nested_default: Option<Box<Expr>> = None;
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                        *index += 1;
                        let mut depth: i32 = 0;
                        let mut init_tokens: Vec<TokenData> = Vec::new();
                        while *index < tokens.len() {
                            if depth == 0 && (matches!(tokens[*index].token, Token::Comma) || matches!(tokens[*index].token, Token::RBrace))
                            {
                                break;
                            }
                            match tokens[*index].token {
                                Token::LParen | Token::LBracket | Token::LBrace => {
                                    depth += 1;
                                }
                                Token::RParen | Token::RBracket | Token::RBrace => {
                                    depth -= 1;
                                }
                                _ => {}
                            }
                            init_tokens.push(tokens[*index].clone());
                            *index += 1;
                        }
                        if !init_tokens.is_empty() {
                            let expr = parse_expression(&init_tokens, &mut 0)?;
                            nested_default = Some(Box::new(expr));
                        }
                    }
                    DestructuringElement::NestedObject(nested, nested_default)
                } else if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
                    if strict_binding_checks() && (name == "eval" || name == "arguments") {
                        return Err(raise_parse_error_with_token!(
                            tokens[*index],
                            format!("'{}' can't be defined or assigned to in strict mode code", name)
                        ));
                    }
                    *index += 1;
                    let mut default_expr: Option<Box<Expr>> = None;
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                        *index += 1;
                        let mut depth: i32 = 0;
                        let mut init_tokens: Vec<TokenData> = Vec::new();
                        while *index < tokens.len() {
                            if depth == 0 && (matches!(tokens[*index].token, Token::Comma) || matches!(tokens[*index].token, Token::RBrace))
                            {
                                break;
                            }
                            match tokens[*index].token {
                                Token::LParen | Token::LBracket | Token::LBrace => {
                                    depth += 1;
                                }
                                Token::RParen | Token::RBracket | Token::RBrace => {
                                    depth -= 1;
                                }
                                _ => {}
                            }
                            init_tokens.push(tokens[*index].clone());
                            *index += 1;
                        }
                        if !init_tokens.is_empty() {
                            let tmp = init_tokens.clone();
                            let expr = parse_expression(&tmp, &mut 0)?;
                            default_expr = Some(Box::new(expr));
                        }
                    }
                    DestructuringElement::Variable(name, default_expr)
                } else if *index < tokens.len()
                    && matches!(tokens[*index].token, Token::Await)
                    && !in_await_context()
                    && !forbid_await_identifier()
                {
                    *index += 1;
                    let mut default_expr: Option<Box<Expr>> = None;
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                        *index += 1;
                        let mut depth: i32 = 0;
                        let mut init_tokens: Vec<TokenData> = Vec::new();
                        while *index < tokens.len() {
                            if depth == 0 && (matches!(tokens[*index].token, Token::Comma) || matches!(tokens[*index].token, Token::RBrace))
                            {
                                break;
                            }
                            match tokens[*index].token {
                                Token::LParen | Token::LBracket | Token::LBrace => {
                                    depth += 1;
                                }
                                Token::RParen | Token::RBracket | Token::RBrace => {
                                    depth -= 1;
                                }
                                _ => {}
                            }
                            init_tokens.push(tokens[*index].clone());
                            *index += 1;
                        }
                        if !init_tokens.is_empty() {
                            let expr = parse_expression(&init_tokens, &mut 0)?;
                            default_expr = Some(Box::new(expr));
                        }
                    }
                    DestructuringElement::Variable("await".to_string(), default_expr)
                } else {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
            } else {
                if !is_identifier_key {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                let key = key_name.clone().unwrap_or_default();
                if strict_binding_checks() && (key == "eval" || key == "arguments") {
                    return Err(raise_parse_error!(&format!(
                        "'{}' can't be defined or assigned to in strict mode code",
                        key
                    )));
                }
                if key == "await" && forbid_await_identifier() {
                    return Err(raise_parse_error!("'await' is not allowed as a binding identifier in this context"));
                }
                let mut init_tokens: Vec<TokenData> = Vec::new();
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                    *index += 1;
                    let mut depth: i32 = 0;
                    while *index < tokens.len() {
                        if depth == 0 && (matches!(tokens[*index].token, Token::Comma) || matches!(tokens[*index].token, Token::RBrace)) {
                            break;
                        }
                        match tokens[*index].token {
                            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                            Token::RParen | Token::RBracket | Token::RBrace => depth -= 1,
                            _ => {}
                        }
                        init_tokens.push(tokens[*index].clone());
                        *index += 1;
                    }
                }
                let mut default_expr: Option<Box<Expr>> = None;
                if !init_tokens.is_empty() {
                    let tmp = init_tokens.clone();
                    let expr = parse_expression(&tmp, &mut 0)?;
                    default_expr = Some(Box::new(expr));
                }
                let key = key_name.clone().unwrap_or_default();
                DestructuringElement::Variable(key, default_expr)
            };
            if let Some(expr) = computed_key {
                pattern.push(DestructuringElement::ComputedProperty(expr, Box::new(value)));
            } else {
                let key = key_name.unwrap_or_default();
                pattern.push(DestructuringElement::Property(key, Box::new(value)));
            }
        }
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index >= tokens.len() {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
        if matches!(tokens[*index].token, Token::RBrace) {
            *index += 1;
            break;
        } else if matches!(tokens[*index].token, Token::Comma) {
            *index += 1;
        } else {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
    }
    Ok(pattern)
}

/// Push a set of private names onto the parser's PRIVATE_NAME_STACK.
/// Used by the VM to make private fields visible during direct eval inside class bodies.
/// Returns a guard that pops the entry when dropped.
pub fn push_private_names_for_eval(names: std::collections::HashSet<String>) -> EvalPrivateNamesGuard {
    let rc = std::rc::Rc::new(std::cell::RefCell::new(names));
    PRIVATE_NAME_STACK.with(|s| s.borrow_mut().push(rc));
    EvalPrivateNamesGuard
}

/// RAII guard that pops the PRIVATE_NAME_STACK entry pushed by `push_private_names_for_eval`.
pub struct EvalPrivateNamesGuard;
impl Drop for EvalPrivateNamesGuard {
    fn drop(&mut self) {
        PRIVATE_NAME_STACK.with(|s| {
            s.borrow_mut().pop();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{core::BinaryOp, tokenize};
    #[test]
    fn test_comments_and_empty_lines_not_parsed_as_number_zero() {
        let src = "// comment\n\n3 + 8\n";
        let mut tokens = tokenize(src).unwrap();
        if tokens.last().map(|td| td.token == Token::EOF).unwrap_or(false) {
            tokens.pop();
        }
        let mut index = 0;
        let stmts = parse_statements(&tokens, &mut index).unwrap();
        assert_eq!(stmts.len(), 1, "expected only one statement (the binary expression)");
        match &*stmts[0].kind {
            StatementKind::Expr(expr) => match expr {
                Expr::Binary(left, op, right) => {
                    assert!(matches!(op, BinaryOp::Add));
                    if let Expr::Number(l) = **left {
                        assert_eq!(l, 3.0);
                    } else {
                        panic!("left is not a number")
                    }
                    if let Expr::Number(r) = **right {
                        assert_eq!(r, 8.0);
                    } else {
                        panic!("right is not a number")
                    }
                }
                _ => panic!("expected binary add expression"),
            },
            _ => panic!("expected expression statement"),
        }
    }
    #[test]
    fn test_async_function_expression_is_primary() {
        let src = "(async function foo() { }.prototype)";
        let mut tokens = tokenize(src).unwrap();
        if tokens.last().map(|td| td.token == Token::EOF).unwrap_or(false) {
            tokens.pop();
        }
        let mut index = 0usize;
        let stmts = parse_statements(&tokens, &mut index).unwrap();
        assert!(!stmts.is_empty(), "expected at least one statement");
        match &*stmts[0].kind {
            StatementKind::Expr(expr) => {
                if let Expr::Property(base, prop) = expr {
                    assert_eq!(prop, "prototype");
                    match &**base {
                        Expr::AsyncFunction(Some(name), _params, _body, _) | Expr::Function(Some(name), _params, _body, _) => {
                            assert_eq!(name, "foo");
                        }
                        other => {
                            panic!("expected async or function expression as base, got: {:?}", other)
                        }
                    }
                } else {
                    panic!("expected property expression");
                }
            }
            _ => panic!("expected expression statement"),
        }
    }
}
