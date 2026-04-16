use crate::error::JSError;
use crate::{raise_eval_error, raise_syntax_error};
pub(crate) use gc_arena::GcWeak;
pub(crate) use gc_arena::Mutation as GcContext;
pub(crate) use gc_arena::collect::Trace as GcTrace;
pub(crate) use gc_arena::lock::RefLock as GcCell;
pub(crate) use gc_arena::{Collect, Gc};
use std::collections::HashMap;
pub(crate) type GcPtr<'gc, T> = Gc<'gc, GcCell<T>>;

#[inline]
pub fn new_gc_cell_ptr<'gc, T: 'gc + Collect<'gc>>(ctx: &GcContext<'gc>, value: T) -> GcPtr<'gc, T> {
    Gc::new(ctx, GcCell::new(value))
}

mod gc;

mod value;
pub use value::*;

pub mod property_descriptor;
#[allow(unused_imports)]
pub use property_descriptor::{PropAttrs, PropDesc};

mod statement;
pub use statement::*;

mod token;
pub use token::*;

/// Prefix for internal private field/method keys to separate them from public
/// properties that happen to start with `#`.
pub const PRIVATE_KEY_PREFIX: &str = "\x00#";

/// Create an internal property key for a private class member.
/// The `name` argument should NOT include the `#` prefix.
pub fn make_private_key(name: &str) -> String {
    format!("{}{}", PRIVATE_KEY_PREFIX, name)
}

mod parser;
pub use parser::*;

pub mod js_error;

pub mod opcode;
pub use opcode::*;

pub mod vm;
pub use vm::*;

pub mod compiler;
pub use compiler::*;

pub(crate) mod function_id;

pub type JsArenaVm = gc_arena::Arena<gc_arena::Rootable!['gc => VM<'gc>]>;

fn extract_injected_module_filepath(script: &str) -> Option<String> {
    let marker = "globalThis.__filepath = \"";
    let start = script.find(marker)? + marker.len();
    let rest = &script[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn script_declares_await_identifier(s: &str) -> bool {
    s.contains("function await")
        || s.contains("function await(")
        || s.contains("var await")
        || s.contains("let await")
        || s.contains("const await")
        || s.contains("class await")
}

pub(crate) fn parse_program_statements(script: &str, run_as_module: bool) -> Result<Vec<Statement>, JSError> {
    crate::core::parser::set_module_context(run_as_module);
    let mut tokens = tokenize(script)?;
    if tokens.last().map(|td| td.token == Token::EOF).unwrap_or(false) {
        tokens.pop();
    }

    crate::core::parser::with_parse_source(script, || {
        if run_as_module {
            crate::core::parser::reset_module_tracking();
        }
        let mut index = 0;
        let result = if !run_as_module {
            // Detect whether the script has a real "use strict" directive by checking
            // the first non-LineTerminator token (the tokenizer already strips comments
            // and hashbang lines, but emits LineTerminator tokens for newlines).
            let has_real_directive = tokens
                .iter()
                .find(|td| !matches!(td.token, crate::core::token::Token::LineTerminator))
                .map(|td| {
                    if let crate::core::token::Token::StringLit(ref s) = td.token {
                        String::from_utf16_lossy(s) == "use strict"
                    } else {
                        false
                    }
                })
                .unwrap_or(false);
            let enable_top_level_await = !script_declares_await_identifier(script);
            let parse_fn = |idx: &mut usize| {
                if enable_top_level_await {
                    crate::core::parser::push_await_context();
                    let res = parse_statements(&tokens, idx);
                    crate::core::parser::pop_await_context();
                    res
                } else {
                    parse_statements(&tokens, idx)
                }
            };
            if has_real_directive {
                parse_fn(&mut index)
            } else {
                crate::core::parser::parse_without_strict_binding_checks(|| parse_fn(&mut index))
            }
        } else {
            crate::core::parser::push_await_context();
            // In module mode, `await` is reserved and cannot be used as an identifier
            let res = crate::core::parser::with_forbidden_await_identifier_pub(|| parse_statements(&tokens, &mut index));
            crate::core::parser::pop_await_context();
            res
        }?;
        validate_early_errors(&result)?;
        if run_as_module {
            validate_module_exported_bindings(&result)?;
        }
        Ok(result)
    })
}

/// Read a script file from disk and decode it into a UTF-8 Rust `String`.
/// Supports UTF-8 (with optional BOM) and UTF-16 (LE/BE) with BOM.
pub fn read_script_file<P: AsRef<std::path::Path>>(path: P) -> Result<String, JSError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|e| raise_eval_error!(format!("Failed to read script file '{}': {e}", path.display())))?;
    if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        // UTF-8 with BOM
        let s = std::str::from_utf8(&bytes[3..]).map_err(|e| raise_eval_error!(format!("Script file contains invalid UTF-8: {e}")))?;
        return Ok(s.to_string());
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        // UTF-16LE
        if (bytes.len() - 2) % 2 != 0 {
            return Err(raise_eval_error!("Invalid UTF-16LE script file length"));
        }
        let mut u16s = Vec::with_capacity((bytes.len() - 2) / 2);
        for chunk in bytes[2..].chunks(2) {
            let lo = chunk[0] as u16;
            let hi = chunk[1] as u16;
            u16s.push((hi << 8) | lo);
        }
        return String::from_utf16(&u16s).map_err(|e| raise_eval_error!(format!("Invalid UTF-16LE script file contents: {e}")));
    }
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        // UTF-16BE
        if (bytes.len() - 2) % 2 != 0 {
            return Err(raise_eval_error!("Invalid UTF-16BE script file length"));
        }
        let mut u16s = Vec::with_capacity((bytes.len() - 2) / 2);
        for chunk in bytes[2..].chunks(2) {
            let hi = chunk[0] as u16;
            let lo = chunk[1] as u16;
            u16s.push((hi << 8) | lo);
        }
        return String::from_utf16(&u16s).map_err(|e| raise_eval_error!(format!("Invalid UTF-16BE script file contents: {e}")));
    }
    // Otherwise assume UTF-8 without BOM
    std::str::from_utf8(&bytes)
        .map(|s| s.to_string())
        .map_err(|e| raise_eval_error!(format!("Script file contains invalid UTF-8: {e}")))
}

#[derive(Clone)]
pub(crate) enum ReexportSpec {
    Named(String, Option<String>), // export { name as alias } from ...
    Star,                          // export * from ...
    Namespace(String),             // export * as name from ...
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ModuleRequestPhase {
    Evaluation,
    Defer,
}

#[derive(Clone, Debug)]
pub(crate) struct ModuleRequest {
    pub specifier: String,
    pub phase: ModuleRequestPhase,
    pub import_type: Option<String>,
}

/// Resolve a module specifier relative to a base path.
pub(crate) fn resolve_module_path(specifier: &str, base_path: &std::path::Path) -> std::path::PathBuf {
    let spec_path = std::path::Path::new(specifier);
    if spec_path.is_absolute() {
        return normalize_path(spec_path);
    }
    if specifier.starts_with("./") || specifier.starts_with("../") {
        let parent = base_path.parent().unwrap_or(std::path::Path::new("."));
        return normalize_path(&parent.join(spec_path));
    }
    spec_path.to_path_buf()
}

pub(crate) fn module_request_key_from_resolved_path(resolved_path: &std::path::Path, import_type: Option<&str>) -> String {
    let resolved = resolved_path.to_string_lossy().to_string();
    match import_type {
        Some(import_type) => format!("{resolved}\0{import_type}"),
        None => resolved,
    }
}

pub(crate) fn resolve_module_request_key(specifier: &str, base_path: &std::path::Path, import_type: Option<&str>) -> String {
    let resolved_path = resolve_module_path(specifier, base_path);
    module_request_key_from_resolved_path(&resolved_path, import_type)
}

/// Remove `.` and resolve `..` components from a path without touching the filesystem.
fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut result = std::path::PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            other => result.push(other),
        }
    }
    result
}

pub(crate) type ExportInfo = (Vec<String>, HashMap<String, String>, Vec<(String, Vec<ReexportSpec>)>);

/// Extract binding names from object destructuring pattern elements.
fn collect_object_destr_binding_names(elems: &[crate::core::statement::ObjectDestructuringElement]) -> Vec<String> {
    use crate::core::statement::ObjectDestructuringElement;
    let mut names = Vec::new();
    for elem in elems {
        match elem {
            ObjectDestructuringElement::Property { value, .. } | ObjectDestructuringElement::ComputedProperty { value, .. } => {
                collect_destr_binding_names(value, &mut names);
            }
            ObjectDestructuringElement::Rest(name) => {
                names.push(name.clone());
            }
        }
    }
    names
}

/// Extract binding names from array destructuring pattern elements.
fn collect_array_destr_binding_names(elems: &[crate::core::statement::DestructuringElement]) -> Vec<String> {
    let mut names = Vec::new();
    for elem in elems {
        collect_destr_binding_names(elem, &mut names);
    }
    names
}

/// Recursively extract binding names from a destructuring element.
fn collect_destr_binding_names(elem: &crate::core::statement::DestructuringElement, names: &mut Vec<String>) {
    use crate::core::statement::DestructuringElement;
    match elem {
        DestructuringElement::Variable(name, _) => {
            names.push(name.clone());
        }
        DestructuringElement::Property(_, inner) => {
            collect_destr_binding_names(inner, names);
        }
        DestructuringElement::ComputedProperty(_, inner) => {
            collect_destr_binding_names(inner, names);
        }
        DestructuringElement::Rest(name) => {
            names.push(name.clone());
        }
        DestructuringElement::RestPattern(inner) => {
            collect_destr_binding_names(inner, names);
        }
        DestructuringElement::NestedArray(elems, _) => {
            for e in elems {
                collect_destr_binding_names(e, names);
            }
        }
        DestructuringElement::NestedObject(elems, _) => {
            for e in elems {
                collect_destr_binding_names(e, names);
            }
        }
        DestructuringElement::Empty => {}
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StatementListKind {
    ScriptOrFunction,
    Block,
}

fn push_unique_or_throw(names: &mut std::collections::HashSet<String>, name: &str) -> Result<(), JSError> {
    if names.insert(name.to_string()) {
        Ok(())
    } else {
        Err(crate::raise_syntax_error!(format!(
            "Identifier '{}' has already been declared",
            name
        )))
    }
}

fn is_reserved_identifier_name(name: &str) -> bool {
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
            | "let"
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
            | "yield"
            | "implements"
            | "interface"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
    )
}

fn validate_pattern_identifier_name(name: &str, reject_eval_arguments: bool) -> Result<(), JSError> {
    if is_reserved_identifier_name(name) {
        return Err(crate::raise_syntax_error!(format!("Unexpected reserved word '{}'", name)));
    }
    if reject_eval_arguments && matches!(name, "eval" | "arguments") {
        return Err(crate::raise_syntax_error!(format!("Invalid assignment target '{}'", name)));
    }
    Ok(())
}

fn validate_destructuring_elements(
    elems: &[crate::core::statement::DestructuringElement],
    reject_eval_arguments: bool,
) -> Result<(), JSError> {
    use crate::core::statement::DestructuringElement;

    for (index, elem) in elems.iter().enumerate() {
        if matches!(elem, DestructuringElement::Rest(_) | DestructuringElement::RestPattern(_)) && index + 1 != elems.len() {
            return Err(crate::raise_syntax_error!("Rest element must be last"));
        }
        validate_destructuring_element(elem, reject_eval_arguments)?;
    }
    Ok(())
}

fn validate_destructuring_element(elem: &crate::core::statement::DestructuringElement, reject_eval_arguments: bool) -> Result<(), JSError> {
    use crate::core::statement::DestructuringElement;

    match elem {
        DestructuringElement::Variable(name, default_expr) => {
            validate_pattern_identifier_name(name, reject_eval_arguments)?;
            if let Some(default_expr) = default_expr {
                validate_expression(default_expr)?;
            }
        }
        DestructuringElement::Property(_, inner) => {
            validate_destructuring_element(inner, reject_eval_arguments)?;
        }
        DestructuringElement::ComputedProperty(expr, inner) => {
            validate_expression(expr)?;
            validate_destructuring_element(inner, reject_eval_arguments)?;
        }
        DestructuringElement::Rest(name) => {
            validate_pattern_identifier_name(name, reject_eval_arguments)?;
        }
        DestructuringElement::RestPattern(inner) => {
            validate_destructuring_element(inner, reject_eval_arguments)?;
        }
        DestructuringElement::NestedArray(elems, default_expr) => {
            validate_destructuring_elements(elems, reject_eval_arguments)?;
            if let Some(default_expr) = default_expr {
                validate_expression(default_expr)?;
            }
        }
        DestructuringElement::NestedObject(elems, default_expr) => {
            validate_destructuring_elements(elems, reject_eval_arguments)?;
            if let Some(default_expr) = default_expr {
                validate_expression(default_expr)?;
            }
        }
        DestructuringElement::Empty => {}
    }

    Ok(())
}

fn validate_formal_parameters(params: &[crate::core::statement::DestructuringElement]) -> Result<(), JSError> {
    let mut names = std::collections::HashSet::new();
    for param in params {
        validate_destructuring_element(param, true)?;
        let mut binding_names = Vec::new();
        collect_destr_binding_names(param, &mut binding_names);
        for binding_name in binding_names {
            validate_pattern_identifier_name(&binding_name, true)?;
            push_unique_or_throw(&mut names, &binding_name)?;
        }
    }
    Ok(())
}

fn collect_param_binding_names(params: &[crate::core::statement::DestructuringElement]) -> Vec<String> {
    let mut names = Vec::new();
    for param in params {
        collect_destr_binding_names(param, &mut names);
    }
    names
}

fn has_non_simple_parameters(params: &[crate::core::statement::DestructuringElement]) -> bool {
    use crate::core::statement::DestructuringElement;

    params.iter().any(|param| !matches!(param, DestructuringElement::Variable(_, None)))
}

fn body_contains_use_strict_directive(body: &[Statement]) -> bool {
    for statement in body {
        match &*statement.kind {
            StatementKind::Expr(Expr::StringLit(value)) => {
                if crate::unicode::utf16_to_utf8(value) == "use strict" {
                    return true;
                }
            }
            _ => break,
        }
    }
    false
}

fn expr_contains_await(expr: &Expr) -> bool {
    match expr {
        Expr::Await(_) => true,
        Expr::Assign(lhs, rhs)
        | Expr::Binary(lhs, _, rhs)
        | Expr::LogicalAnd(lhs, rhs)
        | Expr::LogicalOr(lhs, rhs)
        | Expr::NullishCoalescing(lhs, rhs)
        | Expr::Mod(lhs, rhs)
        | Expr::Pow(lhs, rhs)
        | Expr::LogicalAndAssign(lhs, rhs)
        | Expr::LogicalOrAssign(lhs, rhs)
        | Expr::NullishAssign(lhs, rhs)
        | Expr::AddAssign(lhs, rhs)
        | Expr::SubAssign(lhs, rhs)
        | Expr::PowAssign(lhs, rhs)
        | Expr::MulAssign(lhs, rhs)
        | Expr::DivAssign(lhs, rhs)
        | Expr::ModAssign(lhs, rhs)
        | Expr::BitXorAssign(lhs, rhs)
        | Expr::BitAndAssign(lhs, rhs)
        | Expr::BitOrAssign(lhs, rhs)
        | Expr::LeftShiftAssign(lhs, rhs)
        | Expr::RightShiftAssign(lhs, rhs)
        | Expr::UnsignedRightShiftAssign(lhs, rhs)
        | Expr::Index(lhs, rhs)
        | Expr::OptionalIndex(lhs, rhs)
        | Expr::Comma(lhs, rhs) => expr_contains_await(lhs) || expr_contains_await(rhs),
        Expr::Conditional(test, consequent, alternate) => {
            expr_contains_await(test) || expr_contains_await(consequent) || expr_contains_await(alternate)
        }
        Expr::Property(expr, _)
        | Expr::OptionalProperty(expr, _)
        | Expr::PrivateMember(expr, _)
        | Expr::OptionalPrivateMember(expr, _)
        | Expr::SuperComputedProperty(expr)
        | Expr::TypeOf(expr)
        | Expr::Delete(expr)
        | Expr::Void(expr)
        | Expr::LogicalNot(expr)
        | Expr::UnaryNeg(expr)
        | Expr::UnaryPlus(expr)
        | Expr::BitNot(expr)
        | Expr::Increment(expr)
        | Expr::Decrement(expr)
        | Expr::Spread(expr)
        | Expr::PostIncrement(expr)
        | Expr::PostDecrement(expr)
        | Expr::Getter(expr)
        | Expr::Setter(expr)
        | Expr::YieldStar(expr) => expr_contains_await(expr),
        Expr::Call(callee, args) | Expr::OptionalCall(callee, args) | Expr::New(callee, args) => {
            expr_contains_await(callee) || args.iter().any(expr_contains_await)
        }
        Expr::SuperCall(args) | Expr::SuperMethod(_, args) => args.iter().any(expr_contains_await),
        Expr::SuperComputedMethod(prop, args) => expr_contains_await(prop) || args.iter().any(expr_contains_await),
        Expr::Object(entries) => entries
            .iter()
            .any(|(key, value, _, _)| expr_contains_await(key) || expr_contains_await(value)),
        Expr::Array(elements) => elements.iter().flatten().any(expr_contains_await),
        Expr::TaggedTemplate(tag, _, _, _, exprs) => expr_contains_await(tag) || exprs.iter().any(expr_contains_await),
        Expr::DynamicImport(specifier, options) => {
            expr_contains_await(specifier) || options.as_ref().map(|expr| expr_contains_await(expr)).unwrap_or(false)
        }
        Expr::DeferredImport(specifier) | Expr::SourceImport(specifier) => expr_contains_await(specifier),
        Expr::ArrowFunction(_, body) | Expr::AsyncArrowFunction(_, body) => body.iter().any(statement_contains_await),
        Expr::Function(..)
        | Expr::GeneratorFunction(..)
        | Expr::AsyncFunction(..)
        | Expr::AsyncGeneratorFunction(..)
        | Expr::Class(_)
        | Expr::TemplateString(_)
        | Expr::Regex(_, _)
        | Expr::Number(_)
        | Expr::StringLit(_)
        | Expr::Boolean(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::Var(_, _, _)
        | Expr::BigInt(_)
        | Expr::PrivateName(_)
        | Expr::This
        | Expr::NewTarget
        | Expr::SuperProperty(_)
        | Expr::Super
        | Expr::Yield(_)
        | Expr::ValuePlaceholder => false,
    }
}

fn expr_contains_yield(expr: &Expr) -> bool {
    match expr {
        Expr::Yield(_) | Expr::YieldStar(_) => true,
        Expr::Assign(lhs, rhs)
        | Expr::Binary(lhs, _, rhs)
        | Expr::LogicalAnd(lhs, rhs)
        | Expr::LogicalOr(lhs, rhs)
        | Expr::NullishCoalescing(lhs, rhs)
        | Expr::Mod(lhs, rhs)
        | Expr::Pow(lhs, rhs)
        | Expr::LogicalAndAssign(lhs, rhs)
        | Expr::LogicalOrAssign(lhs, rhs)
        | Expr::NullishAssign(lhs, rhs)
        | Expr::AddAssign(lhs, rhs)
        | Expr::SubAssign(lhs, rhs)
        | Expr::PowAssign(lhs, rhs)
        | Expr::MulAssign(lhs, rhs)
        | Expr::DivAssign(lhs, rhs)
        | Expr::ModAssign(lhs, rhs)
        | Expr::BitXorAssign(lhs, rhs)
        | Expr::BitAndAssign(lhs, rhs)
        | Expr::BitOrAssign(lhs, rhs)
        | Expr::LeftShiftAssign(lhs, rhs)
        | Expr::RightShiftAssign(lhs, rhs)
        | Expr::UnsignedRightShiftAssign(lhs, rhs)
        | Expr::Index(lhs, rhs)
        | Expr::OptionalIndex(lhs, rhs)
        | Expr::Comma(lhs, rhs) => expr_contains_yield(lhs) || expr_contains_yield(rhs),
        Expr::Conditional(test, consequent, alternate) => {
            expr_contains_yield(test) || expr_contains_yield(consequent) || expr_contains_yield(alternate)
        }
        Expr::Property(expr, _)
        | Expr::OptionalProperty(expr, _)
        | Expr::PrivateMember(expr, _)
        | Expr::OptionalPrivateMember(expr, _)
        | Expr::SuperComputedProperty(expr)
        | Expr::TypeOf(expr)
        | Expr::Delete(expr)
        | Expr::Void(expr)
        | Expr::Await(expr)
        | Expr::LogicalNot(expr)
        | Expr::UnaryNeg(expr)
        | Expr::UnaryPlus(expr)
        | Expr::BitNot(expr)
        | Expr::Increment(expr)
        | Expr::Decrement(expr)
        | Expr::Spread(expr)
        | Expr::PostIncrement(expr)
        | Expr::PostDecrement(expr)
        | Expr::Getter(expr)
        | Expr::Setter(expr) => expr_contains_yield(expr),
        Expr::Call(callee, args) | Expr::OptionalCall(callee, args) | Expr::New(callee, args) => {
            expr_contains_yield(callee) || args.iter().any(expr_contains_yield)
        }
        Expr::SuperCall(args) | Expr::SuperMethod(_, args) => args.iter().any(expr_contains_yield),
        Expr::SuperComputedMethod(prop, args) => expr_contains_yield(prop) || args.iter().any(expr_contains_yield),
        Expr::Object(entries) => entries
            .iter()
            .any(|(key, value, _, _)| expr_contains_yield(key) || expr_contains_yield(value)),
        Expr::Array(elements) => elements.iter().flatten().any(expr_contains_yield),
        Expr::TaggedTemplate(tag, _, _, _, exprs) => expr_contains_yield(tag) || exprs.iter().any(expr_contains_yield),
        Expr::DynamicImport(specifier, options) => {
            expr_contains_yield(specifier) || options.as_ref().map(|expr| expr_contains_yield(expr)).unwrap_or(false)
        }
        Expr::DeferredImport(specifier) | Expr::SourceImport(specifier) => expr_contains_yield(specifier),
        Expr::ArrowFunction(_, body) | Expr::AsyncArrowFunction(_, body) => body.iter().any(statement_contains_yield),
        Expr::Function(..)
        | Expr::GeneratorFunction(..)
        | Expr::AsyncFunction(..)
        | Expr::AsyncGeneratorFunction(..)
        | Expr::Class(_)
        | Expr::TemplateString(_)
        | Expr::Regex(_, _)
        | Expr::Number(_)
        | Expr::StringLit(_)
        | Expr::Boolean(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::Var(_, _, _)
        | Expr::BigInt(_)
        | Expr::PrivateName(_)
        | Expr::This
        | Expr::NewTarget
        | Expr::SuperProperty(_)
        | Expr::Super
        | Expr::ValuePlaceholder => false,
    }
}

fn destructuring_element_contains_yield(elem: &crate::core::statement::DestructuringElement) -> bool {
    use crate::core::statement::DestructuringElement;

    match elem {
        DestructuringElement::Variable(_, default_expr) => default_expr.as_ref().map(|expr| expr_contains_yield(expr)).unwrap_or(false),
        DestructuringElement::Property(_, inner) => destructuring_element_contains_yield(inner),
        DestructuringElement::ComputedProperty(expr, inner) => expr_contains_yield(expr) || destructuring_element_contains_yield(inner),
        DestructuringElement::Rest(_) => false,
        DestructuringElement::RestPattern(inner) => destructuring_element_contains_yield(inner),
        DestructuringElement::NestedArray(elems, default_expr) | DestructuringElement::NestedObject(elems, default_expr) => {
            elems.iter().any(destructuring_element_contains_yield)
                || default_expr.as_ref().map(|expr| expr_contains_yield(expr)).unwrap_or(false)
        }
        DestructuringElement::Empty => false,
    }
}

fn destructuring_element_contains_yield_in_list(params: &[crate::core::statement::DestructuringElement]) -> bool {
    params.iter().any(destructuring_element_contains_yield)
}

fn destructuring_element_contains_await(elem: &crate::core::statement::DestructuringElement) -> bool {
    use crate::core::statement::DestructuringElement;

    match elem {
        DestructuringElement::Variable(_, default_expr) => default_expr.as_ref().map(|expr| expr_contains_await(expr)).unwrap_or(false),
        DestructuringElement::Property(_, inner) => destructuring_element_contains_await(inner),
        DestructuringElement::ComputedProperty(expr, inner) => expr_contains_await(expr) || destructuring_element_contains_await(inner),
        DestructuringElement::Rest(_) => false,
        DestructuringElement::RestPattern(inner) => destructuring_element_contains_await(inner),
        DestructuringElement::NestedArray(elems, default_expr) | DestructuringElement::NestedObject(elems, default_expr) => {
            elems.iter().any(destructuring_element_contains_await)
                || default_expr.as_ref().map(|expr| expr_contains_await(expr)).unwrap_or(false)
        }
        DestructuringElement::Empty => false,
    }
}

fn destructuring_element_uses_identifier(elem: &crate::core::statement::DestructuringElement, ident: &str) -> bool {
    use crate::core::statement::DestructuringElement;

    match elem {
        DestructuringElement::Variable(name, default_expr) => {
            name == ident || default_expr.as_ref().map(|expr| expr_uses_identifier(expr, ident)).unwrap_or(false)
        }
        DestructuringElement::Property(_, inner) => destructuring_element_uses_identifier(inner, ident),
        DestructuringElement::ComputedProperty(expr, inner) => {
            expr_uses_identifier(expr, ident) || destructuring_element_uses_identifier(inner, ident)
        }
        DestructuringElement::Rest(name) => name == ident,
        DestructuringElement::RestPattern(inner) => destructuring_element_uses_identifier(inner, ident),
        DestructuringElement::NestedArray(elems, default_expr) | DestructuringElement::NestedObject(elems, default_expr) => {
            elems.iter().any(|elem| destructuring_element_uses_identifier(elem, ident))
                || default_expr.as_ref().map(|expr| expr_uses_identifier(expr, ident)).unwrap_or(false)
        }
        DestructuringElement::Empty => false,
    }
}

fn params_use_identifier(params: &[crate::core::statement::DestructuringElement], ident: &str) -> bool {
    params.iter().any(|param| destructuring_element_uses_identifier(param, ident))
}

fn scan_expr_mask(expr: &Expr, mask: u8) -> u8 {
    let statement = Statement::from(StatementKind::Return(Some(expr.clone())));
    eval_ast_scan(&[statement], mask)
}

fn statement_contains_yield(statement: &Statement) -> bool {
    match &*statement.kind {
        StatementKind::Expr(expr) | StatementKind::Throw(expr) => expr_contains_yield(expr),
        StatementKind::Return(expr) => expr.as_ref().map(expr_contains_yield).unwrap_or(false),
        StatementKind::Let(decls) | StatementKind::Var(decls) => decls
            .iter()
            .any(|(_, init)| init.as_ref().map(expr_contains_yield).unwrap_or(false)),
        StatementKind::Const(decls) | StatementKind::Using(decls) | StatementKind::AwaitUsing(decls) => {
            decls.iter().any(|(_, init)| expr_contains_yield(init))
        }
        StatementKind::Assign(_, expr)
        | StatementKind::LetDestructuringArray(_, expr)
        | StatementKind::VarDestructuringArray(_, expr)
        | StatementKind::ConstDestructuringArray(_, expr)
        | StatementKind::LetDestructuringObject(_, expr)
        | StatementKind::VarDestructuringObject(_, expr)
        | StatementKind::ConstDestructuringObject(_, expr) => expr_contains_yield(expr),
        StatementKind::Block(statements) => statements.iter().any(statement_contains_yield),
        StatementKind::If(if_stmt) => {
            expr_contains_yield(&if_stmt.condition)
                || if_stmt.then_body.iter().any(statement_contains_yield)
                || if_stmt
                    .else_body
                    .as_ref()
                    .map(|body| body.iter().any(statement_contains_yield))
                    .unwrap_or(false)
        }
        StatementKind::For(for_stmt) => {
            for_stmt.init.as_ref().map(|stmt| statement_contains_yield(stmt)).unwrap_or(false)
                || for_stmt.test.as_ref().map(expr_contains_yield).unwrap_or(false)
                || for_stmt.update.as_ref().map(|stmt| statement_contains_yield(stmt)).unwrap_or(false)
                || for_stmt.body.iter().any(statement_contains_yield)
        }
        StatementKind::ForOf(_, _, expr, body)
        | StatementKind::ForAwaitOf(_, _, expr, body)
        | StatementKind::ForIn(_, _, expr, body)
        | StatementKind::ForInDestructuringObject(_, _, expr, body)
        | StatementKind::ForInDestructuringArray(_, _, expr, body)
        | StatementKind::ForOfDestructuringObject(_, _, expr, body)
        | StatementKind::ForOfDestructuringArray(_, _, expr, body)
        | StatementKind::ForAwaitOfDestructuringObject(_, _, expr, body)
        | StatementKind::ForAwaitOfDestructuringArray(_, _, expr, body)
        | StatementKind::While(expr, body) => expr_contains_yield(expr) || body.iter().any(statement_contains_yield),
        StatementKind::ForOfExpr(lhs, expr, body)
        | StatementKind::ForAwaitOfExpr(lhs, expr, body)
        | StatementKind::ForInExpr(lhs, expr, body) => {
            expr_contains_yield(lhs) || expr_contains_yield(expr) || body.iter().any(statement_contains_yield)
        }
        StatementKind::DoWhile(body, expr) => expr_contains_yield(expr) || body.iter().any(statement_contains_yield),
        StatementKind::With(expr, body) => expr_contains_yield(expr) || body.iter().any(statement_contains_yield),
        StatementKind::Switch(switch_stmt) => {
            expr_contains_yield(&switch_stmt.expr)
                || switch_stmt.cases.iter().any(|case| match case {
                    crate::core::SwitchCase::Case(expr, body) => expr_contains_yield(expr) || body.iter().any(statement_contains_yield),
                    crate::core::SwitchCase::Default(body) => body.iter().any(statement_contains_yield),
                })
        }
        StatementKind::TryCatch(try_stmt) => {
            try_stmt.try_body.iter().any(statement_contains_yield)
                || try_stmt
                    .catch_body
                    .as_ref()
                    .map(|body| body.iter().any(statement_contains_yield))
                    .unwrap_or(false)
                || try_stmt
                    .finally_body
                    .as_ref()
                    .map(|body| body.iter().any(statement_contains_yield))
                    .unwrap_or(false)
        }
        StatementKind::Label(_, inner) => statement_contains_yield(inner),
        StatementKind::FunctionDeclaration(..) | StatementKind::Class(..) => false,
        StatementKind::Import(..)
        | StatementKind::Export(..)
        | StatementKind::Break(_)
        | StatementKind::Continue(_)
        | StatementKind::Debugger => false,
    }
}

fn statement_contains_await(statement: &Statement) -> bool {
    match &*statement.kind {
        StatementKind::Expr(expr) | StatementKind::Throw(expr) => expr_contains_await(expr),
        StatementKind::Return(expr) => expr.as_ref().map(expr_contains_await).unwrap_or(false),
        StatementKind::Let(decls) | StatementKind::Var(decls) => decls
            .iter()
            .any(|(_, init)| init.as_ref().map(expr_contains_await).unwrap_or(false)),
        StatementKind::Const(decls) | StatementKind::Using(decls) | StatementKind::AwaitUsing(decls) => {
            decls.iter().any(|(_, init)| expr_contains_await(init))
        }
        StatementKind::Assign(_, expr)
        | StatementKind::LetDestructuringArray(_, expr)
        | StatementKind::VarDestructuringArray(_, expr)
        | StatementKind::ConstDestructuringArray(_, expr)
        | StatementKind::LetDestructuringObject(_, expr)
        | StatementKind::VarDestructuringObject(_, expr)
        | StatementKind::ConstDestructuringObject(_, expr) => expr_contains_await(expr),
        StatementKind::Block(statements) => statements.iter().any(statement_contains_await),
        StatementKind::If(if_stmt) => {
            expr_contains_await(&if_stmt.condition)
                || if_stmt.then_body.iter().any(statement_contains_await)
                || if_stmt
                    .else_body
                    .as_ref()
                    .map(|body| body.iter().any(statement_contains_await))
                    .unwrap_or(false)
        }
        StatementKind::For(for_stmt) => {
            for_stmt.init.as_ref().map(|stmt| statement_contains_await(stmt)).unwrap_or(false)
                || for_stmt.test.as_ref().map(expr_contains_await).unwrap_or(false)
                || for_stmt.update.as_ref().map(|stmt| statement_contains_await(stmt)).unwrap_or(false)
                || for_stmt.body.iter().any(statement_contains_await)
        }
        StatementKind::ForOf(_, _, expr, body)
        | StatementKind::ForAwaitOf(_, _, expr, body)
        | StatementKind::ForIn(_, _, expr, body)
        | StatementKind::ForInDestructuringObject(_, _, expr, body)
        | StatementKind::ForInDestructuringArray(_, _, expr, body)
        | StatementKind::ForOfDestructuringObject(_, _, expr, body)
        | StatementKind::ForOfDestructuringArray(_, _, expr, body)
        | StatementKind::ForAwaitOfDestructuringObject(_, _, expr, body)
        | StatementKind::ForAwaitOfDestructuringArray(_, _, expr, body)
        | StatementKind::While(expr, body) => expr_contains_await(expr) || body.iter().any(statement_contains_await),
        StatementKind::ForOfExpr(lhs, expr, body)
        | StatementKind::ForAwaitOfExpr(lhs, expr, body)
        | StatementKind::ForInExpr(lhs, expr, body) => {
            expr_contains_await(lhs) || expr_contains_await(expr) || body.iter().any(statement_contains_await)
        }
        StatementKind::DoWhile(body, expr) => expr_contains_await(expr) || body.iter().any(statement_contains_await),
        StatementKind::With(expr, body) => expr_contains_await(expr) || body.iter().any(statement_contains_await),
        StatementKind::Switch(switch_stmt) => {
            expr_contains_await(&switch_stmt.expr)
                || switch_stmt.cases.iter().any(|case| match case {
                    crate::core::SwitchCase::Case(expr, body) => expr_contains_await(expr) || body.iter().any(statement_contains_await),
                    crate::core::SwitchCase::Default(body) => body.iter().any(statement_contains_await),
                })
        }
        StatementKind::TryCatch(try_stmt) => {
            try_stmt.try_body.iter().any(statement_contains_await)
                || try_stmt
                    .catch_body
                    .as_ref()
                    .map(|body| body.iter().any(statement_contains_await))
                    .unwrap_or(false)
                || try_stmt
                    .finally_body
                    .as_ref()
                    .map(|body| body.iter().any(statement_contains_await))
                    .unwrap_or(false)
        }
        StatementKind::Label(_, inner) => statement_contains_await(inner),
        StatementKind::FunctionDeclaration(..) | StatementKind::Class(..) => false,
        StatementKind::Import(..)
        | StatementKind::Export(..)
        | StatementKind::Break(_)
        | StatementKind::Continue(_)
        | StatementKind::Debugger => false,
    }
}

fn expr_uses_identifier(expr: &Expr, ident: &str) -> bool {
    match expr {
        Expr::Var(name, _, _) => name == ident,
        Expr::Assign(lhs, rhs)
        | Expr::Binary(lhs, _, rhs)
        | Expr::LogicalAnd(lhs, rhs)
        | Expr::LogicalOr(lhs, rhs)
        | Expr::NullishCoalescing(lhs, rhs)
        | Expr::Mod(lhs, rhs)
        | Expr::Pow(lhs, rhs)
        | Expr::LogicalAndAssign(lhs, rhs)
        | Expr::LogicalOrAssign(lhs, rhs)
        | Expr::NullishAssign(lhs, rhs)
        | Expr::AddAssign(lhs, rhs)
        | Expr::SubAssign(lhs, rhs)
        | Expr::PowAssign(lhs, rhs)
        | Expr::MulAssign(lhs, rhs)
        | Expr::DivAssign(lhs, rhs)
        | Expr::ModAssign(lhs, rhs)
        | Expr::BitXorAssign(lhs, rhs)
        | Expr::BitAndAssign(lhs, rhs)
        | Expr::BitOrAssign(lhs, rhs)
        | Expr::LeftShiftAssign(lhs, rhs)
        | Expr::RightShiftAssign(lhs, rhs)
        | Expr::UnsignedRightShiftAssign(lhs, rhs)
        | Expr::Index(lhs, rhs)
        | Expr::OptionalIndex(lhs, rhs)
        | Expr::Comma(lhs, rhs) => expr_uses_identifier(lhs, ident) || expr_uses_identifier(rhs, ident),
        Expr::Conditional(test, consequent, alternate) => {
            expr_uses_identifier(test, ident) || expr_uses_identifier(consequent, ident) || expr_uses_identifier(alternate, ident)
        }
        Expr::Property(expr, _)
        | Expr::OptionalProperty(expr, _)
        | Expr::PrivateMember(expr, _)
        | Expr::OptionalPrivateMember(expr, _)
        | Expr::SuperComputedProperty(expr)
        | Expr::TypeOf(expr)
        | Expr::Delete(expr)
        | Expr::Void(expr)
        | Expr::Await(expr)
        | Expr::LogicalNot(expr)
        | Expr::UnaryNeg(expr)
        | Expr::UnaryPlus(expr)
        | Expr::BitNot(expr)
        | Expr::Increment(expr)
        | Expr::Decrement(expr)
        | Expr::Spread(expr)
        | Expr::PostIncrement(expr)
        | Expr::PostDecrement(expr)
        | Expr::Getter(expr)
        | Expr::Setter(expr)
        | Expr::Yield(Some(expr))
        | Expr::YieldStar(expr) => expr_uses_identifier(expr, ident),
        Expr::Call(callee, args) | Expr::OptionalCall(callee, args) | Expr::New(callee, args) => {
            expr_uses_identifier(callee, ident) || args.iter().any(|arg| expr_uses_identifier(arg, ident))
        }
        Expr::SuperCall(args) | Expr::SuperMethod(_, args) => args.iter().any(|arg| expr_uses_identifier(arg, ident)),
        Expr::SuperComputedMethod(prop, args) => {
            expr_uses_identifier(prop, ident) || args.iter().any(|arg| expr_uses_identifier(arg, ident))
        }
        Expr::Object(entries) => entries
            .iter()
            .any(|(key, value, _, _)| expr_uses_identifier(key, ident) || expr_uses_identifier(value, ident)),
        Expr::Array(elements) => elements.iter().flatten().any(|expr| expr_uses_identifier(expr, ident)),
        Expr::TaggedTemplate(tag, _, _, _, exprs) => {
            expr_uses_identifier(tag, ident) || exprs.iter().any(|expr| expr_uses_identifier(expr, ident))
        }
        Expr::DynamicImport(specifier, options) => {
            expr_uses_identifier(specifier, ident) || options.as_ref().map(|expr| expr_uses_identifier(expr, ident)).unwrap_or(false)
        }
        Expr::DeferredImport(specifier) | Expr::SourceImport(specifier) => expr_uses_identifier(specifier, ident),
        Expr::ArrowFunction(params, body) | Expr::AsyncArrowFunction(params, body) => {
            params_use_identifier(params, ident) || statement_list_uses_identifier(body, ident)
        }
        Expr::Function(..)
        | Expr::GeneratorFunction(..)
        | Expr::AsyncFunction(..)
        | Expr::AsyncGeneratorFunction(..)
        | Expr::Class(_)
        | Expr::TemplateString(_)
        | Expr::Regex(_, _)
        | Expr::Yield(None)
        | Expr::Number(_)
        | Expr::StringLit(_)
        | Expr::Boolean(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::BigInt(_)
        | Expr::PrivateName(_)
        | Expr::This
        | Expr::NewTarget
        | Expr::SuperProperty(_)
        | Expr::Super
        | Expr::ValuePlaceholder => false,
    }
}

fn catch_param_uses_identifier(param: &crate::core::statement::CatchParamPattern, ident: &str) -> bool {
    match param {
        crate::core::statement::CatchParamPattern::Identifier(name) => name == ident,
        crate::core::statement::CatchParamPattern::Array(params) | crate::core::statement::CatchParamPattern::Object(params) => {
            params_use_identifier(params, ident)
        }
    }
}

fn statement_uses_identifier(statement: &Statement, ident: &str) -> bool {
    match &*statement.kind {
        StatementKind::Expr(expr) | StatementKind::Throw(expr) => expr_uses_identifier(expr, ident),
        StatementKind::Return(expr) => expr.as_ref().map(|expr| expr_uses_identifier(expr, ident)).unwrap_or(false),
        StatementKind::Let(decls) | StatementKind::Var(decls) => decls
            .iter()
            .any(|(name, init)| name == ident || init.as_ref().map(|expr| expr_uses_identifier(expr, ident)).unwrap_or(false)),
        StatementKind::Const(decls) | StatementKind::Using(decls) | StatementKind::AwaitUsing(decls) => {
            decls.iter().any(|(name, init)| name == ident || expr_uses_identifier(init, ident))
        }
        StatementKind::Assign(name, expr) => name == ident || expr_uses_identifier(expr, ident),
        StatementKind::LetDestructuringArray(params, expr)
        | StatementKind::VarDestructuringArray(params, expr)
        | StatementKind::ConstDestructuringArray(params, expr) => params_use_identifier(params, ident) || expr_uses_identifier(expr, ident),
        StatementKind::LetDestructuringObject(params, expr)
        | StatementKind::VarDestructuringObject(params, expr)
        | StatementKind::ConstDestructuringObject(params, expr) => {
            params.iter().any(|param| match param {
                crate::core::statement::ObjectDestructuringElement::Property { key, value } => {
                    key == ident || destructuring_element_uses_identifier(value, ident)
                }
                crate::core::statement::ObjectDestructuringElement::ComputedProperty { key, value } => {
                    expr_uses_identifier(key, ident) || destructuring_element_uses_identifier(value, ident)
                }
                crate::core::statement::ObjectDestructuringElement::Rest(name) => name == ident,
            }) || expr_uses_identifier(expr, ident)
        }
        StatementKind::Block(statements) => statement_list_uses_identifier(statements, ident),
        StatementKind::If(if_stmt) => {
            expr_uses_identifier(&if_stmt.condition, ident)
                || statement_list_uses_identifier(&if_stmt.then_body, ident)
                || if_stmt
                    .else_body
                    .as_ref()
                    .map(|body| statement_list_uses_identifier(body, ident))
                    .unwrap_or(false)
        }
        StatementKind::For(for_stmt) => {
            for_stmt
                .init
                .as_ref()
                .map(|stmt| statement_uses_identifier(stmt, ident))
                .unwrap_or(false)
                || for_stmt
                    .test
                    .as_ref()
                    .map(|expr| expr_uses_identifier(expr, ident))
                    .unwrap_or(false)
                || for_stmt
                    .update
                    .as_ref()
                    .map(|stmt| statement_uses_identifier(stmt, ident))
                    .unwrap_or(false)
                || statement_list_uses_identifier(&for_stmt.body, ident)
        }
        StatementKind::ForOf(_, name, expr, body)
        | StatementKind::ForAwaitOf(_, name, expr, body)
        | StatementKind::ForIn(_, name, expr, body) => {
            name == ident || expr_uses_identifier(expr, ident) || statement_list_uses_identifier(body, ident)
        }
        StatementKind::ForOfExpr(lhs, expr, body)
        | StatementKind::ForAwaitOfExpr(lhs, expr, body)
        | StatementKind::ForInExpr(lhs, expr, body) => {
            expr_uses_identifier(lhs, ident) || expr_uses_identifier(expr, ident) || statement_list_uses_identifier(body, ident)
        }
        StatementKind::ForInDestructuringObject(_, params, expr, body)
        | StatementKind::ForOfDestructuringObject(_, params, expr, body)
        | StatementKind::ForAwaitOfDestructuringObject(_, params, expr, body) => {
            params.iter().any(|param| match param {
                crate::core::statement::ObjectDestructuringElement::Property { key, value } => {
                    key == ident || destructuring_element_uses_identifier(value, ident)
                }
                crate::core::statement::ObjectDestructuringElement::ComputedProperty { key, value } => {
                    expr_uses_identifier(key, ident) || destructuring_element_uses_identifier(value, ident)
                }
                crate::core::statement::ObjectDestructuringElement::Rest(name) => name == ident,
            }) || expr_uses_identifier(expr, ident)
                || statement_list_uses_identifier(body, ident)
        }
        StatementKind::ForInDestructuringArray(_, params, expr, body)
        | StatementKind::ForOfDestructuringArray(_, params, expr, body)
        | StatementKind::ForAwaitOfDestructuringArray(_, params, expr, body) => {
            params_use_identifier(params, ident) || expr_uses_identifier(expr, ident) || statement_list_uses_identifier(body, ident)
        }
        StatementKind::While(expr, body) => expr_uses_identifier(expr, ident) || statement_list_uses_identifier(body, ident),
        StatementKind::With(expr, body) => expr_uses_identifier(expr, ident) || statement_list_uses_identifier(body, ident),
        StatementKind::DoWhile(body, expr) => statement_list_uses_identifier(body, ident) || expr_uses_identifier(expr, ident),
        StatementKind::Switch(switch_stmt) => {
            expr_uses_identifier(&switch_stmt.expr, ident)
                || switch_stmt.cases.iter().any(|case| match case {
                    crate::core::SwitchCase::Case(expr, body) => {
                        expr_uses_identifier(expr, ident) || statement_list_uses_identifier(body, ident)
                    }
                    crate::core::SwitchCase::Default(body) => statement_list_uses_identifier(body, ident),
                })
        }
        StatementKind::TryCatch(try_stmt) => {
            statement_list_uses_identifier(&try_stmt.try_body, ident)
                || try_stmt
                    .catch_param
                    .as_ref()
                    .map(|param| catch_param_uses_identifier(param, ident))
                    .unwrap_or(false)
                || try_stmt
                    .catch_body
                    .as_ref()
                    .map(|body| statement_list_uses_identifier(body, ident))
                    .unwrap_or(false)
                || try_stmt
                    .finally_body
                    .as_ref()
                    .map(|body| statement_list_uses_identifier(body, ident))
                    .unwrap_or(false)
        }
        StatementKind::Label(name, inner) => name == ident || statement_uses_identifier(inner, ident),
        StatementKind::Export(_, Some(inner), _) => statement_uses_identifier(inner, ident),
        StatementKind::FunctionDeclaration(name, ..) => name == ident,
        StatementKind::Class(class_def) => class_def.name == ident,
        StatementKind::Import(..)
        | StatementKind::Export(..)
        | StatementKind::Break(_)
        | StatementKind::Continue(_)
        | StatementKind::Debugger => false,
    }
}

fn statement_list_uses_identifier(statements: &[Statement], ident: &str) -> bool {
    statements.iter().any(|statement| statement_uses_identifier(statement, ident))
}

fn expr_contains_arrow_params_with_await(expr: &Expr) -> bool {
    match expr {
        Expr::ArrowFunction(params, body) | Expr::AsyncArrowFunction(params, body) => {
            params_use_identifier(params, "await")
                || params.iter().any(destructuring_element_contains_await)
                || body.iter().any(statement_contains_arrow_params_with_await)
        }
        Expr::Assign(lhs, rhs)
        | Expr::Binary(lhs, _, rhs)
        | Expr::LogicalAnd(lhs, rhs)
        | Expr::LogicalOr(lhs, rhs)
        | Expr::NullishCoalescing(lhs, rhs)
        | Expr::Mod(lhs, rhs)
        | Expr::Pow(lhs, rhs)
        | Expr::LogicalAndAssign(lhs, rhs)
        | Expr::LogicalOrAssign(lhs, rhs)
        | Expr::NullishAssign(lhs, rhs)
        | Expr::AddAssign(lhs, rhs)
        | Expr::SubAssign(lhs, rhs)
        | Expr::PowAssign(lhs, rhs)
        | Expr::MulAssign(lhs, rhs)
        | Expr::DivAssign(lhs, rhs)
        | Expr::ModAssign(lhs, rhs)
        | Expr::BitXorAssign(lhs, rhs)
        | Expr::BitAndAssign(lhs, rhs)
        | Expr::BitOrAssign(lhs, rhs)
        | Expr::LeftShiftAssign(lhs, rhs)
        | Expr::RightShiftAssign(lhs, rhs)
        | Expr::UnsignedRightShiftAssign(lhs, rhs)
        | Expr::Index(lhs, rhs)
        | Expr::OptionalIndex(lhs, rhs)
        | Expr::Comma(lhs, rhs) => expr_contains_arrow_params_with_await(lhs) || expr_contains_arrow_params_with_await(rhs),
        Expr::Conditional(test, consequent, alternate) => {
            expr_contains_arrow_params_with_await(test)
                || expr_contains_arrow_params_with_await(consequent)
                || expr_contains_arrow_params_with_await(alternate)
        }
        Expr::Property(expr, _)
        | Expr::OptionalProperty(expr, _)
        | Expr::PrivateMember(expr, _)
        | Expr::OptionalPrivateMember(expr, _)
        | Expr::SuperComputedProperty(expr)
        | Expr::TypeOf(expr)
        | Expr::Delete(expr)
        | Expr::Void(expr)
        | Expr::Await(expr)
        | Expr::LogicalNot(expr)
        | Expr::UnaryNeg(expr)
        | Expr::UnaryPlus(expr)
        | Expr::BitNot(expr)
        | Expr::Increment(expr)
        | Expr::Decrement(expr)
        | Expr::Spread(expr)
        | Expr::PostIncrement(expr)
        | Expr::PostDecrement(expr)
        | Expr::Getter(expr)
        | Expr::Setter(expr)
        | Expr::Yield(Some(expr))
        | Expr::YieldStar(expr) => expr_contains_arrow_params_with_await(expr),
        Expr::Call(callee, args) | Expr::OptionalCall(callee, args) | Expr::New(callee, args) => {
            expr_contains_arrow_params_with_await(callee) || args.iter().any(expr_contains_arrow_params_with_await)
        }
        Expr::SuperCall(args) | Expr::SuperMethod(_, args) => args.iter().any(expr_contains_arrow_params_with_await),
        Expr::SuperComputedMethod(prop, args) => {
            expr_contains_arrow_params_with_await(prop) || args.iter().any(expr_contains_arrow_params_with_await)
        }
        Expr::Object(entries) => entries
            .iter()
            .any(|(key, value, _, _)| expr_contains_arrow_params_with_await(key) || expr_contains_arrow_params_with_await(value)),
        Expr::Array(elements) => elements.iter().flatten().any(expr_contains_arrow_params_with_await),
        Expr::TaggedTemplate(tag, _, _, _, exprs) => {
            expr_contains_arrow_params_with_await(tag) || exprs.iter().any(expr_contains_arrow_params_with_await)
        }
        Expr::DynamicImport(specifier, options) => {
            expr_contains_arrow_params_with_await(specifier)
                || options
                    .as_ref()
                    .map(|expr| expr_contains_arrow_params_with_await(expr))
                    .unwrap_or(false)
        }
        Expr::DeferredImport(specifier) | Expr::SourceImport(specifier) => expr_contains_arrow_params_with_await(specifier),
        Expr::Function(..)
        | Expr::GeneratorFunction(..)
        | Expr::AsyncFunction(..)
        | Expr::AsyncGeneratorFunction(..)
        | Expr::Class(_)
        | Expr::TemplateString(_)
        | Expr::Regex(_, _)
        | Expr::Yield(None)
        | Expr::Number(_)
        | Expr::StringLit(_)
        | Expr::Boolean(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::Var(_, _, _)
        | Expr::BigInt(_)
        | Expr::PrivateName(_)
        | Expr::This
        | Expr::NewTarget
        | Expr::SuperProperty(_)
        | Expr::Super
        | Expr::ValuePlaceholder => false,
    }
}

fn statement_contains_arrow_params_with_await(statement: &Statement) -> bool {
    match &*statement.kind {
        StatementKind::Expr(expr) | StatementKind::Throw(expr) => expr_contains_arrow_params_with_await(expr),
        StatementKind::Return(expr) => expr.as_ref().map(expr_contains_arrow_params_with_await).unwrap_or(false),
        StatementKind::Let(decls) | StatementKind::Var(decls) => decls
            .iter()
            .any(|(_, init)| init.as_ref().map(expr_contains_arrow_params_with_await).unwrap_or(false)),
        StatementKind::Const(decls) | StatementKind::Using(decls) | StatementKind::AwaitUsing(decls) => {
            decls.iter().any(|(_, init)| expr_contains_arrow_params_with_await(init))
        }
        StatementKind::Assign(_, expr)
        | StatementKind::LetDestructuringArray(_, expr)
        | StatementKind::VarDestructuringArray(_, expr)
        | StatementKind::ConstDestructuringArray(_, expr)
        | StatementKind::LetDestructuringObject(_, expr)
        | StatementKind::VarDestructuringObject(_, expr)
        | StatementKind::ConstDestructuringObject(_, expr) => expr_contains_arrow_params_with_await(expr),
        StatementKind::Block(statements) => statements.iter().any(statement_contains_arrow_params_with_await),
        StatementKind::If(if_stmt) => {
            expr_contains_arrow_params_with_await(&if_stmt.condition)
                || if_stmt.then_body.iter().any(statement_contains_arrow_params_with_await)
                || if_stmt
                    .else_body
                    .as_ref()
                    .map(|body| body.iter().any(statement_contains_arrow_params_with_await))
                    .unwrap_or(false)
        }
        StatementKind::For(for_stmt) => {
            for_stmt
                .init
                .as_ref()
                .map(|stmt| statement_contains_arrow_params_with_await(stmt))
                .unwrap_or(false)
                || for_stmt.test.as_ref().map(expr_contains_arrow_params_with_await).unwrap_or(false)
                || for_stmt
                    .update
                    .as_ref()
                    .map(|stmt| statement_contains_arrow_params_with_await(stmt))
                    .unwrap_or(false)
                || for_stmt.body.iter().any(statement_contains_arrow_params_with_await)
        }
        StatementKind::ForOf(_, _, expr, body)
        | StatementKind::ForAwaitOf(_, _, expr, body)
        | StatementKind::ForIn(_, _, expr, body)
        | StatementKind::ForInDestructuringObject(_, _, expr, body)
        | StatementKind::ForInDestructuringArray(_, _, expr, body)
        | StatementKind::ForOfDestructuringObject(_, _, expr, body)
        | StatementKind::ForOfDestructuringArray(_, _, expr, body)
        | StatementKind::ForAwaitOfDestructuringObject(_, _, expr, body)
        | StatementKind::ForAwaitOfDestructuringArray(_, _, expr, body)
        | StatementKind::While(expr, body) => {
            expr_contains_arrow_params_with_await(expr) || body.iter().any(statement_contains_arrow_params_with_await)
        }
        StatementKind::ForOfExpr(lhs, expr, body)
        | StatementKind::ForAwaitOfExpr(lhs, expr, body)
        | StatementKind::ForInExpr(lhs, expr, body) => {
            expr_contains_arrow_params_with_await(lhs)
                || expr_contains_arrow_params_with_await(expr)
                || body.iter().any(statement_contains_arrow_params_with_await)
        }
        StatementKind::DoWhile(body, expr) => {
            expr_contains_arrow_params_with_await(expr) || body.iter().any(statement_contains_arrow_params_with_await)
        }
        StatementKind::With(expr, body) => {
            expr_contains_arrow_params_with_await(expr) || body.iter().any(statement_contains_arrow_params_with_await)
        }
        StatementKind::Switch(switch_stmt) => {
            expr_contains_arrow_params_with_await(&switch_stmt.expr)
                || switch_stmt.cases.iter().any(|case| match case {
                    crate::core::SwitchCase::Case(expr, body) => {
                        expr_contains_arrow_params_with_await(expr) || body.iter().any(statement_contains_arrow_params_with_await)
                    }
                    crate::core::SwitchCase::Default(body) => body.iter().any(statement_contains_arrow_params_with_await),
                })
        }
        StatementKind::TryCatch(try_stmt) => {
            try_stmt.try_body.iter().any(statement_contains_arrow_params_with_await)
                || try_stmt
                    .catch_body
                    .as_ref()
                    .map(|body| body.iter().any(statement_contains_arrow_params_with_await))
                    .unwrap_or(false)
                || try_stmt
                    .finally_body
                    .as_ref()
                    .map(|body| body.iter().any(statement_contains_arrow_params_with_await))
                    .unwrap_or(false)
        }
        StatementKind::Label(_, inner) => statement_contains_arrow_params_with_await(inner),
        StatementKind::FunctionDeclaration(..)
        | StatementKind::Class(..)
        | StatementKind::Import(..)
        | StatementKind::Export(..)
        | StatementKind::Break(_)
        | StatementKind::Continue(_)
        | StatementKind::Debugger => false,
    }
}

fn collect_statement_list_lexical_names(statements: &[Statement]) -> Result<std::collections::HashSet<String>, JSError> {
    let mut names = std::collections::HashSet::new();
    for statement in statements {
        collect_direct_lexical_names(statement, StatementListKind::ScriptOrFunction, &mut names)?;
    }
    Ok(names)
}

fn validate_function_like(
    name: Option<&str>,
    params: &[crate::core::statement::DestructuringElement],
    body: &[Statement],
    is_async: bool,
    is_generator: bool,
) -> Result<(), JSError> {
    validate_formal_parameters(params)?;

    if has_non_simple_parameters(params) && body_contains_use_strict_directive(body) {
        return Err(crate::raise_syntax_error!(
            "Illegal 'use strict' directive in function with non-simple parameter list"
        ));
    }

    let param_names = collect_param_binding_names(params);
    let lexical_names = collect_statement_list_lexical_names(body)?;
    for param_name in &param_names {
        if lexical_names.contains(param_name) {
            return Err(crate::raise_syntax_error!(format!(
                "Identifier '{}' has already been declared",
                param_name
            )));
        }
    }

    if let Some(name) = name
        && matches!(name, "eval" | "arguments")
    {
        return Err(crate::raise_syntax_error!(format!(
            "'{}' can't be defined or assigned to in strict mode code",
            name
        )));
    }

    if is_async {
        if params_use_identifier(params, "await") || params.iter().any(destructuring_element_contains_await) {
            return Err(crate::raise_syntax_error!("Unexpected await"));
        }
        if statement_list_uses_identifier(body, "await") {
            return Err(crate::raise_syntax_error!("Unexpected await"));
        }
        if body.iter().any(statement_contains_arrow_params_with_await) {
            return Err(crate::raise_syntax_error!("Unexpected await"));
        }
    }

    if is_generator {
        if params_use_identifier(params, "yield") || destructuring_element_contains_yield_in_list(params) {
            return Err(crate::raise_syntax_error!("Unexpected yield"));
        }
        if statement_list_uses_identifier(body, "yield") {
            return Err(crate::raise_syntax_error!("Unexpected yield"));
        }
    }

    validate_statement_list(body, StatementListKind::ScriptOrFunction)
}

fn validate_class_field_initializer(value: &Expr) -> Result<(), JSError> {
    validate_expression(value)?;
    let found = scan_expr_mask(value, SCAN_ARGUMENTS);
    if found & SCAN_ARGUMENTS != 0 {
        return Err(crate::raise_syntax_error!("Class field initializer may not contain arguments"));
    }
    if field_initializer_has_direct_super_call(value) {
        return Err(crate::raise_syntax_error!("Class field initializer may not contain super()"));
    }
    Ok(())
}

/// Check if an expression contains a direct `super()` call (Expr::SuperCall).
/// Unlike SCAN_SUPER_CALL, this does NOT match `super.method()` (Expr::SuperMethod).
/// Recurses into arrow functions (which inherit super binding) but not into
/// regular functions, generators, async functions, or class bodies.
fn field_initializer_has_direct_super_call(expr: &Expr) -> bool {
    match expr {
        Expr::SuperCall(_) => true,
        // Arrow functions inherit super binding — recurse
        Expr::ArrowFunction(_, body) | Expr::AsyncArrowFunction(_, body) => body.iter().any(stmt_has_direct_super_call),
        // Regular functions/classes create new scopes — don't recurse
        Expr::Function(..) | Expr::GeneratorFunction(..) | Expr::AsyncFunction(..) | Expr::AsyncGeneratorFunction(..) | Expr::Class(_) => {
            false
        }
        // Compound expressions
        Expr::Assign(a, b)
        | Expr::Binary(a, _, b)
        | Expr::LogicalAnd(a, b)
        | Expr::LogicalOr(a, b)
        | Expr::NullishCoalescing(a, b)
        | Expr::Mod(a, b)
        | Expr::Pow(a, b)
        | Expr::LogicalAndAssign(a, b)
        | Expr::LogicalOrAssign(a, b)
        | Expr::NullishAssign(a, b)
        | Expr::AddAssign(a, b)
        | Expr::SubAssign(a, b)
        | Expr::PowAssign(a, b)
        | Expr::MulAssign(a, b)
        | Expr::DivAssign(a, b)
        | Expr::ModAssign(a, b)
        | Expr::BitXorAssign(a, b)
        | Expr::BitAndAssign(a, b)
        | Expr::BitOrAssign(a, b)
        | Expr::LeftShiftAssign(a, b)
        | Expr::RightShiftAssign(a, b)
        | Expr::UnsignedRightShiftAssign(a, b)
        | Expr::Index(a, b)
        | Expr::Comma(a, b)
        | Expr::OptionalIndex(a, b) => field_initializer_has_direct_super_call(a) || field_initializer_has_direct_super_call(b),
        Expr::Conditional(a, b, c) => {
            field_initializer_has_direct_super_call(a)
                || field_initializer_has_direct_super_call(b)
                || field_initializer_has_direct_super_call(c)
        }
        Expr::Property(e, _)
        | Expr::OptionalProperty(e, _)
        | Expr::PrivateMember(e, _)
        | Expr::OptionalPrivateMember(e, _)
        | Expr::TypeOf(e)
        | Expr::Delete(e)
        | Expr::Void(e)
        | Expr::Await(e)
        | Expr::LogicalNot(e)
        | Expr::UnaryNeg(e)
        | Expr::UnaryPlus(e)
        | Expr::BitNot(e)
        | Expr::Increment(e)
        | Expr::Decrement(e)
        | Expr::Spread(e)
        | Expr::PostIncrement(e)
        | Expr::PostDecrement(e)
        | Expr::Getter(e)
        | Expr::Setter(e)
        | Expr::YieldStar(e)
        | Expr::SuperComputedProperty(e) => field_initializer_has_direct_super_call(e),
        Expr::DynamicImport(e, opts) => {
            field_initializer_has_direct_super_call(e) || opts.as_ref().is_some_and(|o| field_initializer_has_direct_super_call(o))
        }
        Expr::DeferredImport(e) | Expr::SourceImport(e) => field_initializer_has_direct_super_call(e),
        Expr::Yield(Some(e)) => field_initializer_has_direct_super_call(e),
        Expr::Call(callee, args) | Expr::OptionalCall(callee, args) | Expr::New(callee, args) => {
            field_initializer_has_direct_super_call(callee) || args.iter().any(field_initializer_has_direct_super_call)
        }
        Expr::SuperMethod(_, args) | Expr::SuperComputedMethod(_, args) => args.iter().any(field_initializer_has_direct_super_call),
        Expr::Object(entries) => entries
            .iter()
            .any(|(k, v, _, _)| field_initializer_has_direct_super_call(k) || field_initializer_has_direct_super_call(v)),
        Expr::Array(elements) => elements
            .iter()
            .any(|e| e.as_ref().is_some_and(field_initializer_has_direct_super_call)),
        Expr::TaggedTemplate(tag, _, _, _, exprs) => {
            field_initializer_has_direct_super_call(tag) || exprs.iter().any(field_initializer_has_direct_super_call)
        }
        _ => false,
    }
}

fn stmt_has_direct_super_call(stmt: &Statement) -> bool {
    match &*stmt.kind {
        StatementKind::Expr(e) | StatementKind::Throw(e) | StatementKind::Return(Some(e)) => field_initializer_has_direct_super_call(e),
        StatementKind::Let(decls) | StatementKind::Var(decls) => decls
            .iter()
            .any(|(_, init)| init.as_ref().is_some_and(field_initializer_has_direct_super_call)),
        StatementKind::Const(decls) => decls.iter().any(|(_, e)| field_initializer_has_direct_super_call(e)),
        StatementKind::Block(stmts) => stmts.iter().any(stmt_has_direct_super_call),
        StatementKind::If(if_stmt) => {
            field_initializer_has_direct_super_call(&if_stmt.condition)
                || if_stmt.then_body.iter().any(stmt_has_direct_super_call)
                || if_stmt.else_body.as_ref().is_some_and(|b| b.iter().any(stmt_has_direct_super_call))
        }
        StatementKind::For(for_stmt) => {
            for_stmt.init.as_ref().is_some_and(|s| stmt_has_direct_super_call(s))
                || for_stmt.test.as_ref().is_some_and(field_initializer_has_direct_super_call)
                || for_stmt.update.as_ref().is_some_and(|s| stmt_has_direct_super_call(s))
                || for_stmt.body.iter().any(stmt_has_direct_super_call)
        }
        StatementKind::While(cond, body) => field_initializer_has_direct_super_call(cond) || body.iter().any(stmt_has_direct_super_call),
        StatementKind::DoWhile(body, cond) => body.iter().any(stmt_has_direct_super_call) || field_initializer_has_direct_super_call(cond),
        _ => false,
    }
}

fn validate_class_field_name(name: &str, is_static: bool) -> Result<(), JSError> {
    if is_static {
        if matches!(name, "constructor" | "prototype") {
            return Err(crate::raise_syntax_error!(format!("Invalid static field name '{}'", name)));
        }
    } else if name == "constructor" {
        return Err(crate::raise_syntax_error!("Invalid field name 'constructor'"));
    }
    Ok(())
}

fn validate_class_method_name(name: &str, is_static: bool, is_special: bool) -> Result<(), JSError> {
    if is_static && name == "prototype" {
        return Err(crate::raise_syntax_error!("Invalid static method name 'prototype'"));
    }
    if is_special && name == "constructor" {
        return Err(crate::raise_syntax_error!("Invalid special method name 'constructor'"));
    }
    Ok(())
}

fn record_private_name_kind(
    private_name_kinds: &mut std::collections::HashMap<String, (usize, usize, usize, usize, usize)>,
    name: &str,
    is_getter: bool,
    is_setter: bool,
    is_static: bool,
) -> Result<(), JSError> {
    if name == "constructor" {
        return Err(crate::raise_syntax_error!("Private names may not be '#constructor'"));
    }
    // Entry: (static_getters, instance_getters, static_setters, instance_setters, others)
    let entry = private_name_kinds.entry(name.to_string()).or_insert((0, 0, 0, 0, 0));
    match (is_getter, is_setter, is_static) {
        (true, _, true) => entry.0 += 1,  // static getter
        (true, _, false) => entry.1 += 1, // instance getter
        (_, true, true) => entry.2 += 1,  // static setter
        (_, true, false) => entry.3 += 1, // instance setter
        _ => entry.4 += 1,                // method / property
    }
    Ok(())
}

fn validate_class_definition(class_def: &ClassDefinition) -> Result<(), JSError> {
    if let Some(extends) = &class_def.extends {
        validate_expression(extends)?;
    }

    let mut constructor_count = 0usize;
    let mut private_name_kinds: HashMap<String, (usize, usize, usize, usize, usize)> = HashMap::new();

    for member in &class_def.members {
        match member {
            ClassMember::Constructor(params, body) => {
                constructor_count += 1;
                validate_function_like(None, params, body, false, false)?;
            }

            ClassMember::Method(_, params, body)
            | ClassMember::StaticMethod(_, params, body)
            | ClassMember::PrivateMethod(_, params, body)
            | ClassMember::PrivateStaticMethod(_, params, body) => {
                validate_function_like(None, params, body, false, false)?;
            }

            ClassMember::MethodGenerator(_, params, body)
            | ClassMember::StaticMethodGenerator(_, params, body)
            | ClassMember::PrivateMethodGenerator(_, params, body)
            | ClassMember::PrivateStaticMethodGenerator(_, params, body) => {
                validate_function_like(None, params, body, false, true)?;
            }

            ClassMember::MethodAsync(_, params, body)
            | ClassMember::StaticMethodAsync(_, params, body)
            | ClassMember::PrivateMethodAsync(_, params, body)
            | ClassMember::PrivateStaticMethodAsync(_, params, body) => {
                validate_function_like(None, params, body, true, false)?;
            }

            ClassMember::MethodAsyncGenerator(_, params, body)
            | ClassMember::StaticMethodAsyncGenerator(_, params, body)
            | ClassMember::PrivateMethodAsyncGenerator(_, params, body)
            | ClassMember::PrivateStaticMethodAsyncGenerator(_, params, body) => {
                validate_function_like(None, params, body, true, true)?;
            }

            ClassMember::Getter(_, body)
            | ClassMember::StaticGetter(_, body)
            | ClassMember::PrivateGetter(_, body)
            | ClassMember::PrivateStaticGetter(_, body)
            | ClassMember::StaticBlock(body) => {
                validate_statement_list(body, StatementListKind::ScriptOrFunction)?;
            }

            ClassMember::Setter(_, params, body)
            | ClassMember::StaticSetter(_, params, body)
            | ClassMember::PrivateSetter(_, params, body)
            | ClassMember::PrivateStaticSetter(_, params, body) => {
                validate_function_like(None, params, body, false, false)?;
            }

            ClassMember::Property(name, value) => {
                validate_class_field_name(name, false)?;
                validate_class_field_initializer(value)?;
            }
            ClassMember::StaticProperty(name, value) => {
                validate_class_field_name(name, true)?;
                validate_class_field_initializer(value)?;
            }
            ClassMember::PrivateProperty(_, value) | ClassMember::PrivateStaticProperty(_, value) => {
                validate_class_field_initializer(value)?;
            }

            ClassMember::PropertyComputed(key, value) | ClassMember::StaticPropertyComputed(key, value) => {
                validate_expression(key)?;
                validate_class_field_initializer(value)?;
            }

            ClassMember::GetterComputed(key, body) | ClassMember::StaticGetterComputed(key, body) => {
                validate_expression(key)?;
                validate_statement_list(body, StatementListKind::ScriptOrFunction)?;
            }
            ClassMember::SetterComputed(key, params, body) | ClassMember::StaticSetterComputed(key, params, body) => {
                validate_expression(key)?;
                validate_function_like(None, params, body, false, false)?;
            }
            ClassMember::MethodComputed(key, params, body) | ClassMember::StaticMethodComputed(key, params, body) => {
                validate_expression(key)?;
                validate_function_like(None, params, body, false, false)?;
            }
            ClassMember::MethodComputedGenerator(key, params, body) | ClassMember::StaticMethodComputedGenerator(key, params, body) => {
                validate_expression(key)?;
                validate_function_like(None, params, body, false, true)?;
            }
            ClassMember::MethodComputedAsync(key, params, body) | ClassMember::StaticMethodComputedAsync(key, params, body) => {
                validate_expression(key)?;
                validate_function_like(None, params, body, true, false)?;
            }
            ClassMember::MethodComputedAsyncGenerator(key, params, body)
            | ClassMember::StaticMethodComputedAsyncGenerator(key, params, body) => {
                validate_expression(key)?;
                validate_function_like(None, params, body, true, true)?;
            }
        }

        match member {
            ClassMember::Method(name, _, _) => {
                validate_class_method_name(name, false, false)?;
            }
            ClassMember::MethodGenerator(name, _, _)
            | ClassMember::MethodAsync(name, _, _)
            | ClassMember::MethodAsyncGenerator(name, _, _) => {
                validate_class_method_name(name, false, true)?;
            }
            ClassMember::Getter(name, _) => {
                validate_class_method_name(name, false, true)?;
            }
            ClassMember::Setter(name, _, _) => {
                validate_class_method_name(name, false, true)?;
            }
            ClassMember::StaticMethod(name, _, _)
            | ClassMember::StaticMethodGenerator(name, _, _)
            | ClassMember::StaticMethodAsync(name, _, _)
            | ClassMember::StaticMethodAsyncGenerator(name, _, _) => {
                validate_class_method_name(name, true, false)?;
            }
            ClassMember::StaticGetter(name, _) => {
                validate_class_method_name(name, true, false)?;
            }
            ClassMember::StaticSetter(name, _, _) => {
                validate_class_method_name(name, true, false)?;
            }
            ClassMember::PrivateProperty(name, _) | ClassMember::PrivateStaticProperty(name, _) => {
                record_private_name_kind(&mut private_name_kinds, name, false, false, false)?;
            }
            ClassMember::PrivateMethod(name, _, _)
            | ClassMember::PrivateMethodAsync(name, _, _)
            | ClassMember::PrivateMethodGenerator(name, _, _)
            | ClassMember::PrivateMethodAsyncGenerator(name, _, _) => {
                record_private_name_kind(&mut private_name_kinds, name, false, false, false)?;
            }
            ClassMember::PrivateStaticMethod(name, _, _)
            | ClassMember::PrivateStaticMethodAsync(name, _, _)
            | ClassMember::PrivateStaticMethodGenerator(name, _, _)
            | ClassMember::PrivateStaticMethodAsyncGenerator(name, _, _) => {
                record_private_name_kind(&mut private_name_kinds, name, false, false, true)?;
            }
            ClassMember::PrivateGetter(name, _) => {
                record_private_name_kind(&mut private_name_kinds, name, true, false, false)?;
            }
            ClassMember::PrivateStaticGetter(name, _) => {
                record_private_name_kind(&mut private_name_kinds, name, true, false, true)?;
            }
            ClassMember::PrivateSetter(name, _, _) => {
                record_private_name_kind(&mut private_name_kinds, name, false, true, false)?;
            }
            ClassMember::PrivateStaticSetter(name, _, _) => {
                record_private_name_kind(&mut private_name_kinds, name, false, true, true)?;
            }
            _ => {}
        }
    }

    if constructor_count > 1 {
        return Err(crate::raise_syntax_error!("Duplicate constructor"));
    }

    for (name, (sg, ig, ss, is, others)) in private_name_kinds {
        // Each private name slot may hold at most one method/property,
        // or exactly one accessor pair (both static or both instance).
        let total = sg + ig + ss + is + others;
        let valid = match (sg, ig, ss, is, others) {
            // single item
            (_, _, _, _, _) if total == 1 => true,
            // valid accessor pair: both static getter+setter
            (1, 0, 1, 0, 0) => true,
            // valid accessor pair: both instance getter+setter
            (0, 1, 0, 1, 0) => true,
            _ => false,
        };
        if !valid {
            return Err(crate::raise_syntax_error!(format!("Duplicate private name: #{}", name)));
        }
    }

    Ok(())
}

fn validate_assignment_target_expr(expr: &Expr, allow_pattern: bool) -> Result<(), JSError> {
    match expr {
        Expr::Assign(lhs, rhs) if allow_pattern => {
            validate_assignment_target_expr(lhs, true)?;
            validate_expression(rhs)
        }
        Expr::Var(name, _, _) => validate_pattern_identifier_name(name, true),
        Expr::Property(base, prop) if prop == "meta" && matches!(&**base, Expr::Var(name, _, _) if name == "import") => {
            Err(crate::raise_syntax_error!("Invalid assignment target"))
        }
        Expr::Property(base, _) | Expr::PrivateMember(base, _) | Expr::SuperComputedProperty(base) => validate_expression(base),
        Expr::Index(base, index) => {
            validate_expression(base)?;
            validate_expression(index)
        }
        Expr::SuperProperty(_) => Ok(()),
        Expr::Array(elems) if allow_pattern => {
            for (index, elem) in elems.iter().enumerate() {
                let Some(elem) = elem else {
                    continue;
                };
                if let Expr::Spread(rest_target) = elem {
                    if index + 1 != elems.len() {
                        return Err(crate::raise_syntax_error!("Rest element must be last"));
                    }
                    match &**rest_target {
                        Expr::Assign(_, _) => {
                            return Err(crate::raise_syntax_error!("Rest element cannot have an initializer"));
                        }
                        other => validate_assignment_target_expr(other, true)?,
                    }
                } else {
                    validate_assignment_target_expr(elem, true)?;
                }
            }
            Ok(())
        }
        Expr::Object(entries) if allow_pattern => {
            for (index, (key, value, _, _)) in entries.iter().enumerate() {
                validate_expression(key)?;
                if let Expr::Spread(rest_target) = value {
                    if index + 1 != entries.len() {
                        return Err(crate::raise_syntax_error!("Rest property must be last"));
                    }
                    match &**rest_target {
                        Expr::Assign(_, _) => {
                            return Err(crate::raise_syntax_error!("Rest property cannot have an initializer"));
                        }
                        other => validate_assignment_target_expr(other, true)?,
                    }
                } else {
                    validate_assignment_target_expr(value, true)?;
                }
            }
            Ok(())
        }
        _ => Err(crate::raise_syntax_error!("Invalid assignment target")),
    }
}

fn validate_expression(expr: &Expr) -> Result<(), JSError> {
    match expr {
        Expr::Assign(lhs, rhs) => {
            validate_assignment_target_expr(lhs, true)?;
            validate_expression(rhs)?;
        }
        Expr::LogicalAndAssign(lhs, rhs)
        | Expr::LogicalOrAssign(lhs, rhs)
        | Expr::NullishAssign(lhs, rhs)
        | Expr::AddAssign(lhs, rhs)
        | Expr::SubAssign(lhs, rhs)
        | Expr::PowAssign(lhs, rhs)
        | Expr::MulAssign(lhs, rhs)
        | Expr::DivAssign(lhs, rhs)
        | Expr::ModAssign(lhs, rhs)
        | Expr::BitXorAssign(lhs, rhs)
        | Expr::BitAndAssign(lhs, rhs)
        | Expr::BitOrAssign(lhs, rhs)
        | Expr::LeftShiftAssign(lhs, rhs)
        | Expr::RightShiftAssign(lhs, rhs)
        | Expr::UnsignedRightShiftAssign(lhs, rhs) => {
            validate_assignment_target_expr(lhs, false)?;
            validate_expression(rhs)?;
        }
        Expr::Binary(lhs, _, rhs)
        | Expr::LogicalAnd(lhs, rhs)
        | Expr::LogicalOr(lhs, rhs)
        | Expr::NullishCoalescing(lhs, rhs)
        | Expr::Mod(lhs, rhs)
        | Expr::Pow(lhs, rhs)
        | Expr::Index(lhs, rhs)
        | Expr::OptionalIndex(lhs, rhs)
        | Expr::Comma(lhs, rhs) => {
            validate_expression(lhs)?;
            validate_expression(rhs)?;
        }
        Expr::Conditional(test, consequent, alternate) => {
            validate_expression(test)?;
            validate_expression(consequent)?;
            validate_expression(alternate)?;
        }
        Expr::Property(expr, _)
        | Expr::OptionalProperty(expr, _)
        | Expr::PrivateMember(expr, _)
        | Expr::OptionalPrivateMember(expr, _)
        | Expr::SuperComputedProperty(expr)
        | Expr::TypeOf(expr)
        | Expr::Delete(expr)
        | Expr::Void(expr)
        | Expr::Await(expr)
        | Expr::LogicalNot(expr)
        | Expr::UnaryNeg(expr)
        | Expr::UnaryPlus(expr)
        | Expr::BitNot(expr)
        | Expr::Getter(expr)
        | Expr::Setter(expr)
        | Expr::YieldStar(expr) => {
            validate_expression(expr)?;
        }
        Expr::Spread(_) => {
            return Err(crate::raise_syntax_error!("Unexpected spread element"));
        }
        Expr::Increment(expr) | Expr::Decrement(expr) | Expr::PostIncrement(expr) | Expr::PostDecrement(expr) => {
            validate_assignment_target_expr(expr, false)?;
        }
        Expr::Yield(Some(expr)) => {
            validate_expression(expr)?;
        }
        Expr::Call(callee, args) | Expr::OptionalCall(callee, args) | Expr::New(callee, args) => {
            validate_expression(callee)?;
            for arg in args {
                if let Expr::Spread(inner) = arg {
                    validate_expression(inner)?;
                } else {
                    validate_expression(arg)?;
                }
            }
        }
        Expr::SuperCall(args) | Expr::SuperMethod(_, args) => {
            for arg in args {
                if let Expr::Spread(inner) = arg {
                    validate_expression(inner)?;
                } else {
                    validate_expression(arg)?;
                }
            }
        }
        Expr::SuperComputedMethod(prop, args) => {
            validate_expression(prop)?;
            for arg in args {
                if let Expr::Spread(inner) = arg {
                    validate_expression(inner)?;
                } else {
                    validate_expression(arg)?;
                }
            }
        }
        Expr::Object(entries) => {
            for (key, value, _, _) in entries {
                validate_expression(key)?;
                if let Expr::Spread(inner) = value {
                    validate_expression(inner)?;
                } else {
                    validate_expression(value)?;
                }
            }
        }
        Expr::Array(elements) => {
            for expr in elements.iter().flatten() {
                if let Expr::Spread(inner) = expr {
                    validate_expression(inner)?;
                } else {
                    validate_expression(expr)?;
                }
            }
        }
        Expr::ArrowFunction(params, body) => {
            validate_function_like(None, params, body, false, false)?;
            if destructuring_element_contains_yield_in_list(params) {
                return Err(crate::raise_syntax_error!("Arrow parameters may not contain yield expressions"));
            }
        }
        Expr::AsyncArrowFunction(params, body) => {
            validate_function_like(None, params, body, true, false)?;
            if destructuring_element_contains_yield_in_list(params) {
                return Err(crate::raise_syntax_error!("Arrow parameters may not contain yield expressions"));
            }
        }
        Expr::Function(name, params, body, _) => {
            validate_function_like(name.as_deref(), params, body, false, false)?;
        }
        Expr::GeneratorFunction(name, params, body, _) => {
            validate_function_like(name.as_deref(), params, body, false, true)?;
        }
        Expr::AsyncFunction(name, params, body, _) => {
            validate_function_like(name.as_deref(), params, body, true, false)?;
        }
        Expr::AsyncGeneratorFunction(name, params, body, _) => {
            validate_function_like(name.as_deref(), params, body, true, true)?;
        }
        Expr::TaggedTemplate(tag, _, _, _, exprs) => {
            validate_expression(tag)?;
            for expr in exprs {
                validate_expression(expr)?;
            }
        }
        Expr::DynamicImport(specifier, options) => {
            validate_expression(specifier)?;
            if let Some(options) = options {
                validate_expression(options)?;
            }
        }
        Expr::DeferredImport(specifier) | Expr::SourceImport(specifier) => {
            validate_expression(specifier)?;
        }
        Expr::Class(class_def) => {
            validate_class_definition(class_def)?;
        }
        Expr::Yield(None)
        | Expr::Number(_)
        | Expr::StringLit(_)
        | Expr::Boolean(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::Var(_, _, _)
        | Expr::BigInt(_)
        | Expr::PrivateName(_)
        | Expr::This
        | Expr::NewTarget
        | Expr::SuperProperty(_)
        | Expr::Super
        | Expr::TemplateString(_)
        | Expr::Regex(_, _)
        | Expr::ValuePlaceholder => {}
    }
    Ok(())
}

fn collect_direct_lexical_names(
    stmt: &Statement,
    list_kind: StatementListKind,
    names: &mut std::collections::HashSet<String>,
) -> Result<(), JSError> {
    match &*stmt.kind {
        StatementKind::Let(decls) => {
            for (name, _) in decls {
                push_unique_or_throw(names, name)?;
            }
        }
        StatementKind::Const(decls) => {
            for (name, _) in decls {
                push_unique_or_throw(names, name)?;
            }
        }
        StatementKind::LetDestructuringArray(elems, _) | StatementKind::ConstDestructuringArray(elems, _) => {
            for name in collect_array_destr_binding_names(elems) {
                push_unique_or_throw(names, &name)?;
            }
        }
        StatementKind::LetDestructuringObject(elems, _) | StatementKind::ConstDestructuringObject(elems, _) => {
            for name in collect_object_destr_binding_names(elems) {
                push_unique_or_throw(names, &name)?;
            }
        }
        StatementKind::Using(decls) | StatementKind::AwaitUsing(decls) => {
            for (name, _) in decls {
                push_unique_or_throw(names, name)?;
            }
        }
        StatementKind::Class(class_def) if !class_def.name.is_empty() => {
            push_unique_or_throw(names, &class_def.name)?;
        }
        StatementKind::FunctionDeclaration(name, ..) if list_kind == StatementListKind::Block => {
            push_unique_or_throw(names, name)?;
        }
        StatementKind::Export(_, Some(inner), _) => {
            collect_direct_lexical_names(inner, list_kind, names)?;
        }
        _ => {}
    }
    Ok(())
}

fn collect_var_declared_names(stmt: &Statement, list_kind: StatementListKind, names: &mut Vec<String>) {
    match &*stmt.kind {
        StatementKind::Var(decls) => {
            for (name, _) in decls {
                names.push(name.clone());
            }
        }
        StatementKind::VarDestructuringArray(elems, _) => {
            names.extend(collect_array_destr_binding_names(elems));
        }
        StatementKind::VarDestructuringObject(elems, _) => {
            names.extend(collect_object_destr_binding_names(elems));
        }
        StatementKind::FunctionDeclaration(name, ..) if list_kind == StatementListKind::ScriptOrFunction => {
            names.push(name.clone());
        }
        StatementKind::Block(statements) => {
            for statement in statements {
                collect_var_declared_names(statement, StatementListKind::Block, names);
            }
        }
        StatementKind::If(if_stmt) => {
            for statement in &if_stmt.then_body {
                collect_var_declared_names(statement, list_kind, names);
            }
            if let Some(else_body) = &if_stmt.else_body {
                for statement in else_body {
                    collect_var_declared_names(statement, list_kind, names);
                }
            }
        }
        StatementKind::For(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_var_declared_names(init, list_kind, names);
            }
            for statement in &for_stmt.body {
                collect_var_declared_names(statement, list_kind, names);
            }
        }
        StatementKind::ForOf(_, _, _, body)
        | StatementKind::ForOfExpr(_, _, body)
        | StatementKind::ForAwaitOf(_, _, _, body)
        | StatementKind::ForAwaitOfExpr(_, _, body)
        | StatementKind::ForIn(_, _, _, body)
        | StatementKind::ForInExpr(_, _, body)
        | StatementKind::ForInDestructuringObject(_, _, _, body)
        | StatementKind::ForInDestructuringArray(_, _, _, body)
        | StatementKind::ForOfDestructuringObject(_, _, _, body)
        | StatementKind::ForOfDestructuringArray(_, _, _, body)
        | StatementKind::ForAwaitOfDestructuringObject(_, _, _, body)
        | StatementKind::ForAwaitOfDestructuringArray(_, _, _, body)
        | StatementKind::While(_, body)
        | StatementKind::DoWhile(body, _) => {
            for statement in body {
                collect_var_declared_names(statement, list_kind, names);
            }
        }
        StatementKind::Switch(switch_stmt) => {
            for case in &switch_stmt.cases {
                match case {
                    crate::core::SwitchCase::Case(_, statements) | crate::core::SwitchCase::Default(statements) => {
                        for statement in statements {
                            collect_var_declared_names(statement, StatementListKind::Block, names);
                        }
                    }
                }
            }
        }
        StatementKind::TryCatch(try_stmt) => {
            for statement in &try_stmt.try_body {
                collect_var_declared_names(statement, StatementListKind::Block, names);
            }
            if let Some(catch_body) = &try_stmt.catch_body {
                for statement in catch_body {
                    collect_var_declared_names(statement, StatementListKind::Block, names);
                }
            }
            if let Some(finally_body) = &try_stmt.finally_body {
                for statement in finally_body {
                    collect_var_declared_names(statement, StatementListKind::Block, names);
                }
            }
        }
        StatementKind::Label(_, inner) => {
            collect_var_declared_names(inner, list_kind, names);
        }
        StatementKind::With(_, body) => {
            for statement in body {
                collect_var_declared_names(statement, list_kind, names);
            }
        }
        StatementKind::Export(_, Some(inner), _) => {
            collect_var_declared_names(inner, list_kind, names);
        }
        _ => {}
    }
}

fn validate_non_block_body(statement: &Statement) -> Result<(), JSError> {
    if matches!(&*statement.kind, StatementKind::FunctionDeclaration(..)) {
        return Err(crate::raise_syntax_error!(
            "Function declarations are only allowed inside blocks in strict mode"
        ));
    }
    Ok(())
}

fn validate_statement_list(statements: &[Statement], list_kind: StatementListKind) -> Result<(), JSError> {
    let mut lexical_names = std::collections::HashSet::new();
    for statement in statements {
        collect_direct_lexical_names(statement, list_kind, &mut lexical_names)?;
    }

    let mut var_names = Vec::new();
    for statement in statements {
        collect_var_declared_names(statement, list_kind, &mut var_names);
    }

    for lexical_name in &lexical_names {
        if var_names.iter().any(|var_name| var_name == lexical_name) {
            return Err(crate::raise_syntax_error!(format!(
                "Identifier '{}' has already been declared",
                lexical_name
            )));
        }
    }

    for statement in statements {
        validate_statement(statement)?;
    }
    Ok(())
}

fn validate_statement(statement: &Statement) -> Result<(), JSError> {
    match &*statement.kind {
        StatementKind::Expr(expr) | StatementKind::Throw(expr) => {
            validate_expression(expr)?;
        }
        StatementKind::Let(decls) | StatementKind::Var(decls) => {
            for (_, init) in decls {
                if let Some(init) = init {
                    validate_expression(init)?;
                }
            }
        }
        StatementKind::Const(decls) => {
            for (_, init) in decls {
                validate_expression(init)?;
            }
        }
        StatementKind::Return(Some(expr)) => {
            validate_expression(expr)?;
        }
        StatementKind::Return(None) => {}
        StatementKind::Assign(name, expr) => {
            validate_pattern_identifier_name(name, true)?;
            validate_expression(expr)?;
        }
        StatementKind::LetDestructuringArray(_, expr)
        | StatementKind::VarDestructuringArray(_, expr)
        | StatementKind::ConstDestructuringArray(_, expr)
        | StatementKind::LetDestructuringObject(_, expr)
        | StatementKind::VarDestructuringObject(_, expr)
        | StatementKind::ConstDestructuringObject(_, expr) => {
            validate_expression(expr)?;
        }
        StatementKind::Using(decls) | StatementKind::AwaitUsing(decls) => {
            for (_, expr) in decls {
                validate_expression(expr)?;
            }
        }
        StatementKind::Block(statements) => validate_statement_list(statements, StatementListKind::Block)?,
        StatementKind::FunctionDeclaration(name, params, body, is_generator, is_async) => {
            validate_function_like(Some(name), params, body, *is_async, *is_generator)?;
        }
        StatementKind::Class(class_def) => {
            validate_class_definition(class_def)?;
        }
        StatementKind::If(if_stmt) => {
            validate_expression(&if_stmt.condition)?;
            for statement in &if_stmt.then_body {
                validate_non_block_body(statement)?;
                validate_statement(statement)?;
            }
            if let Some(else_body) = &if_stmt.else_body {
                for statement in else_body {
                    validate_non_block_body(statement)?;
                    validate_statement(statement)?;
                }
            }
        }
        StatementKind::For(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                validate_statement(init)?;
            }
            if let Some(test) = &for_stmt.test {
                validate_expression(test)?;
            }
            if let Some(update) = &for_stmt.update {
                validate_statement(update)?;
            }
            for statement in &for_stmt.body {
                validate_non_block_body(statement)?;
                validate_statement(statement)?;
            }
        }
        StatementKind::ForOf(_, _, iter, body)
        | StatementKind::ForAwaitOf(_, _, iter, body)
        | StatementKind::ForIn(_, _, iter, body)
        | StatementKind::ForInDestructuringObject(_, _, iter, body)
        | StatementKind::ForInDestructuringArray(_, _, iter, body)
        | StatementKind::ForOfDestructuringObject(_, _, iter, body)
        | StatementKind::ForOfDestructuringArray(_, _, iter, body)
        | StatementKind::ForAwaitOfDestructuringObject(_, _, iter, body)
        | StatementKind::ForAwaitOfDestructuringArray(_, _, iter, body)
        | StatementKind::While(iter, body) => {
            validate_expression(iter)?;
            for statement in body {
                validate_non_block_body(statement)?;
                validate_statement(statement)?;
            }
        }
        StatementKind::With(expr, body) => {
            validate_expression(expr)?;
            for statement in body {
                validate_non_block_body(statement)?;
                validate_statement(statement)?;
            }
        }
        StatementKind::ForOfExpr(lhs, iter, body)
        | StatementKind::ForAwaitOfExpr(lhs, iter, body)
        | StatementKind::ForInExpr(lhs, iter, body) => {
            validate_assignment_target_expr(lhs, true)?;
            validate_expression(iter)?;
            for statement in body {
                validate_non_block_body(statement)?;
                validate_statement(statement)?;
            }
        }
        StatementKind::DoWhile(body, condition) => {
            validate_expression(condition)?;
            for statement in body {
                validate_non_block_body(statement)?;
                validate_statement(statement)?;
            }
        }
        StatementKind::Switch(switch_stmt) => {
            validate_expression(&switch_stmt.expr)?;
            // Per spec: "It is a Syntax Error if the LexicallyDeclaredNames of CaseBlock
            // contains any duplicate entries."
            // Also: "It is a Syntax Error if any element of the LexicallyDeclaredNames of
            // CaseBlock also occurs in the VarDeclaredNames of CaseBlock."
            let mut all_lexical_names = std::collections::HashSet::new();
            let mut all_var_names = Vec::new();
            for case in &switch_stmt.cases {
                let stmts = match case {
                    crate::core::SwitchCase::Case(_, statements) => statements,
                    crate::core::SwitchCase::Default(statements) => statements,
                };
                for stmt in stmts {
                    collect_direct_lexical_names(stmt, StatementListKind::Block, &mut all_lexical_names)?;
                    collect_var_declared_names(stmt, StatementListKind::Block, &mut all_var_names);
                }
            }
            for var_name in &all_var_names {
                if all_lexical_names.contains(var_name) {
                    return Err(crate::raise_syntax_error!(format!(
                        "Identifier '{}' has already been declared",
                        var_name
                    )));
                }
            }
            for case in &switch_stmt.cases {
                match case {
                    crate::core::SwitchCase::Case(expr, statements) => {
                        validate_expression(expr)?;
                        validate_statement_list(statements, StatementListKind::Block)?;
                    }
                    crate::core::SwitchCase::Default(statements) => {
                        validate_statement_list(statements, StatementListKind::Block)?;
                    }
                }
            }
        }
        StatementKind::TryCatch(try_stmt) => {
            validate_statement_list(&try_stmt.try_body, StatementListKind::Block)?;
            if let Some(catch_body) = &try_stmt.catch_body {
                // Check catch parameter early errors
                if let Some(catch_param) = &try_stmt.catch_param {
                    let param_names = collect_catch_param_names(catch_param);
                    // Check for duplicate names in destructuring catch parameter
                    {
                        let mut seen = std::collections::HashSet::new();
                        for name in &param_names {
                            if !seen.insert(name.clone()) {
                                return Err(crate::raise_syntax_error!(format!(
                                    "Duplicate binding '{}' in catch parameter",
                                    name
                                )));
                            }
                        }
                    }
                    // Check that catch param names don't conflict with lexical names in catch body
                    let mut lexical_names = std::collections::HashSet::new();
                    for stmt in catch_body {
                        collect_direct_lexical_names(stmt, StatementListKind::Block, &mut lexical_names)?;
                    }
                    for name in &param_names {
                        if lexical_names.contains(name) {
                            return Err(crate::raise_syntax_error!(format!(
                                "Identifier '{}' has already been declared",
                                name
                            )));
                        }
                    }
                }
                validate_statement_list(catch_body, StatementListKind::Block)?;
            }
            if let Some(finally_body) = &try_stmt.finally_body {
                validate_statement_list(finally_body, StatementListKind::Block)?;
            }
        }
        StatementKind::Label(_, inner) => {
            validate_non_block_body(inner)?;
            validate_statement(inner)?;
        }
        StatementKind::Export(_, Some(inner), _) => {
            validate_statement(inner)?;
        }
        _ => {}
    }
    Ok(())
}

fn collect_catch_param_names(param: &crate::core::statement::CatchParamPattern) -> Vec<String> {
    use crate::core::statement::CatchParamPattern;
    match param {
        CatchParamPattern::Identifier(name) => vec![name.clone()],
        CatchParamPattern::Array(elems) => collect_array_destr_binding_names(elems),
        CatchParamPattern::Object(elems) => {
            let mut names = Vec::new();
            for elem in elems {
                collect_destr_binding_names(elem, &mut names);
            }
            names
        }
    }
}

fn validate_early_errors(statements: &[Statement]) -> Result<(), JSError> {
    validate_statement_list(statements, StatementListKind::ScriptOrFunction)
}

/// In module code, every local binding referenced by `export { x }` (without
/// `from` clause) must be declared as either a var or lexical name.
fn validate_module_exported_bindings(statements: &[Statement]) -> Result<(), JSError> {
    use crate::core::statement::{ExportSpecifier as ES, StatementKind as SK};
    use std::collections::HashSet;
    let mut declared_names = HashSet::new();
    // Collect all declared names (var + lexical + function + class + import bindings)
    collect_module_declared_names(statements, &mut declared_names);
    // Check exported bindings
    for stmt in statements {
        if let SK::Export(specs, _, source) = &*stmt.kind {
            if source.is_some() {
                continue; // re-exports don't need local bindings
            }
            for spec in specs {
                if let ES::Named(name, _) = spec
                    && !declared_names.contains(name.as_str())
                {
                    return Err(raise_syntax_error!(format!("Export '{}' is not defined", name)));
                }
            }
        }
    }
    Ok(())
}

fn collect_module_declared_names(statements: &[Statement], names: &mut std::collections::HashSet<String>) {
    use crate::core::statement::{ExportSpecifier as ES, StatementKind as SK};
    for stmt in statements {
        match stmt.kind.as_ref() {
            SK::Var(decls) | SK::Let(decls) => {
                for decl in decls {
                    names.insert(decl.0.clone());
                }
            }
            SK::Const(decls) => {
                for decl in decls {
                    names.insert(decl.0.clone());
                }
            }
            SK::FunctionDeclaration(n, ..) => {
                names.insert(n.clone());
            }
            SK::Class(def) if !def.name.is_empty() => {
                names.insert(def.name.clone());
            }
            SK::Import(specs, _, _) => {
                for spec in specs {
                    match spec {
                        crate::core::statement::ImportSpecifier::Default(n) => {
                            names.insert(n.clone());
                        }
                        crate::core::statement::ImportSpecifier::Named(n, alias) => {
                            names.insert(alias.as_ref().unwrap_or(n).clone());
                        }
                        crate::core::statement::ImportSpecifier::Namespace(n)
                        | crate::core::statement::ImportSpecifier::DeferredNamespace(n) => {
                            names.insert(n.clone());
                        }
                    }
                }
            }
            SK::Export(specs, inner, _) => {
                if let Some(inner) = inner {
                    collect_module_declared_names(std::slice::from_ref(inner.as_ref()), names);
                }
                for spec in specs {
                    if let ES::Default(expr) = spec {
                        match expr {
                            crate::core::Expr::Function(Some(n), ..)
                            | crate::core::Expr::GeneratorFunction(Some(n), ..)
                            | crate::core::Expr::AsyncFunction(Some(n), ..)
                            | crate::core::Expr::AsyncGeneratorFunction(Some(n), ..) => {
                                names.insert(n.clone());
                            }
                            crate::core::Expr::Class(def) if !def.name.is_empty() => {
                                names.insert(def.name.clone());
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Collect export info from parsed AST statements.
pub(crate) fn collect_exports_from_ast(statements: &[Statement]) -> ExportInfo {
    use crate::core::statement::ExportSpecifier as ES;
    let mut export_names = Vec::new();
    let mut export_name_to_local: HashMap<String, String> = HashMap::new();
    let mut reexport_sources: Vec<(String, Vec<ReexportSpec>)> = Vec::new();

    for stmt in statements {
        if let StatementKind::Export(specs, inner, source) = &*stmt.kind {
            if let Some(src) = source {
                // Re-export: export { ... } from './other.js' or export * from './other.js'
                let mut reexport_specs = Vec::new();
                for spec in specs {
                    match spec {
                        ES::Named(name, alias) => {
                            let export_name = alias.as_deref().unwrap_or(name).to_string();
                            reexport_specs.push(ReexportSpec::Named(name.clone(), alias.clone()));
                            if !export_names.contains(&export_name) {
                                export_names.push(export_name.clone());
                            }
                        }
                        ES::Star => {
                            reexport_specs.push(ReexportSpec::Star);
                        }
                        ES::Namespace(name) => {
                            reexport_specs.push(ReexportSpec::Namespace(name.clone()));
                            if !export_names.contains(name) {
                                export_names.push(name.clone());
                            }
                        }
                        ES::Default(_) => {}
                    }
                }
                if !reexport_specs.is_empty() {
                    reexport_sources.push((src.clone(), reexport_specs));
                }
                continue;
            }

            // Local exports
            for spec in specs {
                match spec {
                    ES::Named(name, alias) => {
                        let export_name = alias.as_deref().unwrap_or(name).to_string();
                        if !export_names.contains(&export_name) {
                            export_names.push(export_name.clone());
                        }
                        export_name_to_local.insert(export_name, name.clone());
                    }
                    ES::Default(expr) => {
                        if !export_names.contains(&"default".to_string()) {
                            export_names.push("default".to_string());
                        }
                        // For named default function/class exports, use the actual
                        // local binding name so live bindings resolve correctly.
                        let local_name = match expr {
                            Expr::Function(Some(n), ..)
                            | Expr::AsyncFunction(Some(n), ..)
                            | Expr::GeneratorFunction(Some(n), ..)
                            | Expr::AsyncGeneratorFunction(Some(n), ..)
                                if !n.is_empty() =>
                            {
                                n.clone()
                            }
                            Expr::Class(cd) if !cd.name.is_empty() => cd.name.clone(),
                            _ => {
                                // Also check inner declaration (e.g. `export default class C {}`)
                                inner
                                    .as_ref()
                                    .and_then(|inner_stmt| match &*inner_stmt.kind {
                                        StatementKind::FunctionDeclaration(name, ..) if !name.is_empty() => Some(name.clone()),
                                        StatementKind::Class(cd) if !cd.name.is_empty() => Some(cd.name.clone()),
                                        _ => None,
                                    })
                                    .unwrap_or_else(|| "*default*".to_string())
                            }
                        };
                        export_name_to_local.insert("default".to_string(), local_name);
                    }
                    ES::Namespace(_) | ES::Star => {}
                }
            }

            // Exported declarations
            if let Some(inner) = inner {
                match &*inner.kind {
                    StatementKind::Var(decls) => {
                        for (n, _) in decls {
                            if !export_names.contains(n) {
                                export_names.push(n.clone());
                            }
                            export_name_to_local.insert(n.clone(), n.clone());
                        }
                    }
                    StatementKind::Const(decls) => {
                        for (n, _) in decls {
                            if !export_names.contains(n) {
                                export_names.push(n.clone());
                            }
                            export_name_to_local.insert(n.clone(), n.clone());
                        }
                    }
                    StatementKind::Let(decls) => {
                        for (n, _) in decls {
                            if !export_names.contains(n) {
                                export_names.push(n.clone());
                            }
                            export_name_to_local.insert(n.clone(), n.clone());
                        }
                    }
                    StatementKind::FunctionDeclaration(name, _, _, _, _) if !name.is_empty() && !export_names.contains(name) => {
                        export_names.push(name.clone());
                        export_name_to_local.insert(name.clone(), name.clone());
                    }
                    StatementKind::Class(cd) if !cd.name.is_empty() => {
                        if !export_names.contains(&cd.name) {
                            export_names.push(cd.name.clone());
                        }
                        export_name_to_local.insert(cd.name.clone(), cd.name.clone());
                    }
                    StatementKind::ConstDestructuringObject(elems, _)
                    | StatementKind::LetDestructuringObject(elems, _)
                    | StatementKind::VarDestructuringObject(elems, _) => {
                        for name in collect_object_destr_binding_names(elems) {
                            if !export_names.contains(&name) {
                                export_names.push(name.clone());
                            }
                            export_name_to_local.insert(name.clone(), name);
                        }
                    }
                    StatementKind::ConstDestructuringArray(elems, _) | StatementKind::LetDestructuringArray(elems, _) => {
                        for name in collect_array_destr_binding_names(elems) {
                            if !export_names.contains(&name) {
                                export_names.push(name.clone());
                            }
                            export_name_to_local.insert(name.clone(), name);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    (export_names, export_name_to_local, reexport_sources)
}

/// Extract external import/export-from sources from the AST.
#[allow(dead_code)]
pub(crate) fn collect_module_sources(statements: &[Statement], self_basename: &str) -> Vec<String> {
    let mut sources = Vec::new();
    let known_builtins = ["math", "console", "os", "std", "./es6_module_export.js"];

    for stmt in statements {
        match &*stmt.kind {
            StatementKind::Import(_, source, import_type) => {
                let import_base = source.strip_prefix("./").unwrap_or(source);
                if import_type.is_none() && import_base == self_basename {
                    continue;
                }
                if known_builtins.contains(&source.as_str()) {
                    continue;
                }
                if !sources.contains(source) {
                    sources.push(source.clone());
                }
            }
            StatementKind::Export(_specs, _, Some(source)) => {
                let import_base = source.strip_prefix("./").unwrap_or(source);
                if import_base == self_basename {
                    continue;
                }
                if !sources.contains(source) {
                    sources.push(source.clone());
                }
            }
            _ => {}
        }
    }
    sources
}

pub(crate) fn collect_module_requests(statements: &[Statement], self_basename: &str) -> Vec<ModuleRequest> {
    let mut requests = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let known_builtins = ["math", "console", "os", "std", "./es6_module_export.js"];

    for stmt in statements {
        let maybe_request = match &*stmt.kind {
            StatementKind::Import(specifiers, source, import_type) => {
                let phase = if specifiers.iter().any(|spec| matches!(spec, ImportSpecifier::DeferredNamespace(_))) {
                    ModuleRequestPhase::Defer
                } else {
                    ModuleRequestPhase::Evaluation
                };
                Some((source, phase, import_type))
            }
            StatementKind::Export(_specs, _, Some(source)) => Some((source, ModuleRequestPhase::Evaluation, &None)),
            _ => None,
        };

        let Some((source, phase, import_type)) = maybe_request else {
            continue;
        };
        let import_base = source.strip_prefix("./").unwrap_or(source);
        if (import_type.is_none() && import_base == self_basename) || known_builtins.contains(&source.as_str()) {
            continue;
        }
        if seen.insert((source.clone(), phase, import_type.clone())) {
            requests.push(ModuleRequest {
                specifier: source.clone(),
                phase,
                import_type: import_type.clone(),
            });
        }
    }

    requests
}

pub(crate) fn module_has_top_level_await(statements: &[Statement]) -> bool {
    fn expr_has_top_level_await(expr: &Expr) -> bool {
        match expr {
            Expr::Await(_) => true,
            Expr::Assign(lhs, rhs)
            | Expr::LogicalAnd(lhs, rhs)
            | Expr::LogicalOr(lhs, rhs)
            | Expr::NullishCoalescing(lhs, rhs)
            | Expr::Mod(lhs, rhs)
            | Expr::Pow(lhs, rhs)
            | Expr::LogicalAndAssign(lhs, rhs)
            | Expr::LogicalOrAssign(lhs, rhs)
            | Expr::NullishAssign(lhs, rhs)
            | Expr::AddAssign(lhs, rhs)
            | Expr::SubAssign(lhs, rhs)
            | Expr::PowAssign(lhs, rhs)
            | Expr::MulAssign(lhs, rhs)
            | Expr::DivAssign(lhs, rhs)
            | Expr::ModAssign(lhs, rhs)
            | Expr::BitXorAssign(lhs, rhs)
            | Expr::BitAndAssign(lhs, rhs)
            | Expr::BitOrAssign(lhs, rhs)
            | Expr::LeftShiftAssign(lhs, rhs)
            | Expr::RightShiftAssign(lhs, rhs)
            | Expr::UnsignedRightShiftAssign(lhs, rhs)
            | Expr::Binary(lhs, _, rhs)
            | Expr::Comma(lhs, rhs) => expr_has_top_level_await(lhs) || expr_has_top_level_await(rhs),
            Expr::Conditional(test, cons, alt) => {
                expr_has_top_level_await(test) || expr_has_top_level_await(cons) || expr_has_top_level_await(alt)
            }
            Expr::TypeOf(inner)
            | Expr::Delete(inner)
            | Expr::Void(inner)
            | Expr::Yield(Some(inner))
            | Expr::LogicalNot(inner)
            | Expr::UnaryNeg(inner)
            | Expr::UnaryPlus(inner)
            | Expr::BitNot(inner)
            | Expr::Increment(inner)
            | Expr::Decrement(inner)
            | Expr::PostIncrement(inner)
            | Expr::PostDecrement(inner)
            | Expr::Spread(inner)
            | Expr::Getter(inner)
            | Expr::Setter(inner)
            | Expr::YieldStar(inner)
            | Expr::OptionalProperty(inner, _)
            | Expr::OptionalPrivateMember(inner, _)
            | Expr::Property(inner, _)
            | Expr::PrivateMember(inner, _)
            | Expr::SuperComputedProperty(inner) => expr_has_top_level_await(inner),
            Expr::OptionalIndex(obj, idx) | Expr::Index(obj, idx) => expr_has_top_level_await(obj) || expr_has_top_level_await(idx),
            Expr::OptionalCall(callee, args)
            | Expr::Call(callee, args)
            | Expr::New(callee, args)
            | Expr::SuperComputedMethod(callee, args) => expr_has_top_level_await(callee) || args.iter().any(expr_has_top_level_await),
            Expr::SuperCall(args) | Expr::SuperMethod(_, args) => args.iter().any(expr_has_top_level_await),
            Expr::Array(items) => items.iter().flatten().any(expr_has_top_level_await),
            Expr::Object(props) => props
                .iter()
                .any(|(k, v, _, _)| expr_has_top_level_await(k) || expr_has_top_level_await(v)),
            Expr::TaggedTemplate(tag, _, _, _, exprs) => expr_has_top_level_await(tag) || exprs.iter().any(expr_has_top_level_await),
            Expr::TemplateString(_parts) => false,
            Expr::DynamicImport(spec, attrs) => {
                expr_has_top_level_await(spec) || attrs.as_ref().is_some_and(|attrs| expr_has_top_level_await(attrs))
            }
            Expr::DeferredImport(spec) | Expr::SourceImport(spec) => expr_has_top_level_await(spec),
            Expr::Class(class_def) => {
                class_def.extends.as_ref().is_some_and(expr_has_top_level_await)
                    || class_def.members.iter().any(|member| match member {
                        ClassMember::Property(_, value)
                        | ClassMember::StaticProperty(_, value)
                        | ClassMember::PrivateProperty(_, value)
                        | ClassMember::PrivateStaticProperty(_, value) => expr_has_top_level_await(value),
                        ClassMember::PropertyComputed(key, value) | ClassMember::StaticPropertyComputed(key, value) => {
                            expr_has_top_level_await(key) || expr_has_top_level_await(value)
                        }
                        ClassMember::StaticBlock(body) => module_has_top_level_await(body),
                        ClassMember::MethodComputed(key, _, _)
                        | ClassMember::MethodComputedGenerator(key, _, _)
                        | ClassMember::MethodComputedAsync(key, _, _)
                        | ClassMember::MethodComputedAsyncGenerator(key, _, _)
                        | ClassMember::StaticMethodComputed(key, _, _)
                        | ClassMember::StaticMethodComputedGenerator(key, _, _)
                        | ClassMember::StaticMethodComputedAsync(key, _, _)
                        | ClassMember::StaticMethodComputedAsyncGenerator(key, _, _)
                        | ClassMember::SetterComputed(key, _, _)
                        | ClassMember::StaticSetterComputed(key, _, _) => expr_has_top_level_await(key),
                        ClassMember::GetterComputed(key, _) | ClassMember::StaticGetterComputed(key, _) => expr_has_top_level_await(key),
                        _ => false,
                    })
            }
            Expr::Function(..)
            | Expr::GeneratorFunction(..)
            | Expr::AsyncFunction(..)
            | Expr::AsyncGeneratorFunction(..)
            | Expr::ArrowFunction(..)
            | Expr::AsyncArrowFunction(..)
            | Expr::Yield(None)
            | Expr::Var(..)
            | Expr::Number(_)
            | Expr::StringLit(_)
            | Expr::Boolean(_)
            | Expr::Null
            | Expr::Undefined
            | Expr::BigInt(_)
            | Expr::This
            | Expr::NewTarget
            | Expr::PrivateName(_)
            | Expr::Super
            | Expr::SuperProperty(_)
            | Expr::Regex(_, _)
            | Expr::ValuePlaceholder => false,
        }
    }

    fn stmt_has_top_level_await(stmt: &Statement) -> bool {
        match &*stmt.kind {
            StatementKind::Expr(expr) => expr_has_top_level_await(expr),
            StatementKind::Let(decls) | StatementKind::Var(decls) => {
                decls.iter().any(|(_, init)| init.as_ref().is_some_and(expr_has_top_level_await))
            }
            StatementKind::Const(decls) | StatementKind::Using(decls) | StatementKind::AwaitUsing(decls) => {
                decls.iter().any(|(_, init)| expr_has_top_level_await(init))
            }
            StatementKind::Return(expr) => expr.as_ref().is_some_and(expr_has_top_level_await),
            StatementKind::Throw(expr)
            | StatementKind::Assign(_, expr)
            | StatementKind::LetDestructuringArray(_, expr)
            | StatementKind::VarDestructuringArray(_, expr)
            | StatementKind::ConstDestructuringArray(_, expr)
            | StatementKind::LetDestructuringObject(_, expr)
            | StatementKind::VarDestructuringObject(_, expr)
            | StatementKind::ConstDestructuringObject(_, expr) => expr_has_top_level_await(expr),
            StatementKind::While(expr, body) | StatementKind::DoWhile(body, expr) => {
                expr_has_top_level_await(expr) || module_has_top_level_await(body)
            }
            StatementKind::Block(stmts) => module_has_top_level_await(stmts),
            StatementKind::If(if_stmt) => {
                expr_has_top_level_await(&if_stmt.condition)
                    || module_has_top_level_await(&if_stmt.then_body)
                    || if_stmt
                        .else_body
                        .as_ref()
                        .is_some_and(|else_body| module_has_top_level_await(else_body))
            }
            StatementKind::TryCatch(try_stmt) => {
                module_has_top_level_await(&try_stmt.try_body)
                    || try_stmt
                        .catch_body
                        .as_ref()
                        .is_some_and(|catch_body| module_has_top_level_await(catch_body))
                    || try_stmt
                        .finally_body
                        .as_ref()
                        .is_some_and(|finally_body| module_has_top_level_await(finally_body))
            }
            StatementKind::Class(class_def) => expr_has_top_level_await(&Expr::Class(class_def.clone())),
            StatementKind::For(for_stmt) => {
                for_stmt.init.as_ref().is_some_and(|init| stmt_has_top_level_await(init))
                    || for_stmt.test.as_ref().is_some_and(expr_has_top_level_await)
                    || for_stmt.update.as_ref().is_some_and(|update| stmt_has_top_level_await(update))
                    || module_has_top_level_await(&for_stmt.body)
            }
            StatementKind::ForOf(_, _, expr, body)
            | StatementKind::ForIn(_, _, expr, body)
            | StatementKind::ForAwaitOf(_, _, expr, body) => {
                matches!(&*stmt.kind, StatementKind::ForAwaitOf(..)) || expr_has_top_level_await(expr) || module_has_top_level_await(body)
            }
            StatementKind::ForOfExpr(lhs, rhs, body)
            | StatementKind::ForInExpr(lhs, rhs, body)
            | StatementKind::ForAwaitOfExpr(lhs, rhs, body) => {
                matches!(&*stmt.kind, StatementKind::ForAwaitOfExpr(..))
                    || expr_has_top_level_await(lhs)
                    || expr_has_top_level_await(rhs)
                    || module_has_top_level_await(body)
            }
            StatementKind::ForInDestructuringObject(_, _, expr, body)
            | StatementKind::ForInDestructuringArray(_, _, expr, body)
            | StatementKind::ForOfDestructuringObject(_, _, expr, body)
            | StatementKind::ForOfDestructuringArray(_, _, expr, body)
            | StatementKind::ForAwaitOfDestructuringObject(_, _, expr, body)
            | StatementKind::ForAwaitOfDestructuringArray(_, _, expr, body) => {
                matches!(
                    &*stmt.kind,
                    StatementKind::ForAwaitOfDestructuringObject(..) | StatementKind::ForAwaitOfDestructuringArray(..)
                ) || expr_has_top_level_await(expr)
                    || module_has_top_level_await(body)
            }
            StatementKind::Switch(switch_stmt) => {
                expr_has_top_level_await(&switch_stmt.expr)
                    || switch_stmt.cases.iter().any(|case| match case {
                        SwitchCase::Case(expr, body) => expr_has_top_level_await(expr) || module_has_top_level_await(body),
                        SwitchCase::Default(body) => module_has_top_level_await(body),
                    })
            }
            StatementKind::With(expr, body) => expr_has_top_level_await(expr) || module_has_top_level_await(body),
            StatementKind::Label(_, inner) => stmt_has_top_level_await(inner),
            StatementKind::FunctionDeclaration(..)
            | StatementKind::Import(..)
            | StatementKind::Export(..)
            | StatementKind::Break(_)
            | StatementKind::Continue(_)
            | StatementKind::Debugger => false,
        }
    }

    statements.iter().any(stmt_has_top_level_await)
}

fn collect_hoisted_function_names(statements: &[Statement]) -> Vec<String> {
    let mut out = Vec::new();
    for stmt in statements {
        match &*stmt.kind {
            StatementKind::FunctionDeclaration(name, ..) => {
                out.push(name.clone());
            }
            StatementKind::Export(_, Some(inner), _) => {
                if let StatementKind::FunctionDeclaration(name, ..) = &*inner.kind {
                    out.push(name.clone());
                }
            }
            _ => {}
        }
    }
    out
}

fn collect_hoisted_function_defs<'gc>(chunk: &crate::core::opcode::Chunk<'gc>, hoisted_locals: &[String]) -> HashMap<String, (usize, u8)> {
    let wanted: std::collections::HashSet<&str> = hoisted_locals.iter().map(String::as_str).collect();
    let mut defs = HashMap::new();
    for constant in &chunk.constants {
        if let Value::VmFunction(ip, arity) = constant
            && let Some(name) = chunk.fn_names.get(ip)
            && wanted.contains(name.as_str())
            && !defs.contains_key(name)
        {
            defs.insert(name.clone(), (*ip, *arity));
        }
    }
    defs
}

fn collect_hoisted_export_function_defs(
    hoisted_defs: &HashMap<String, (usize, u8)>,
    export_name_to_local: &HashMap<String, String>,
) -> HashMap<String, (usize, u8)> {
    let mut seeded = HashMap::new();
    for (export_name, local_name) in export_name_to_local {
        if let Some((ip, arity)) = hoisted_defs.get(local_name) {
            seeded.insert(export_name.clone(), (*ip, *arity));
        }
    }
    seeded
}

type MainModuleRecord = (
    String,
    Vec<String>,
    HashMap<String, String>,
    Vec<(String, Vec<ReexportSpec>)>,
    Vec<ModuleRequest>,
    bool,
    std::path::PathBuf,
);

pub fn evaluate_script<T: AsRef<str>, P: AsRef<std::path::Path>>(
    script: T,
    run_as_module: bool,
    script_path: Option<P>,
) -> Result<String, JSError> {
    let unwrap_top_level_promise = script_path.is_none();
    evaluate_script_with_unwrap(script, run_as_module, script_path, unwrap_top_level_promise)
}

pub fn evaluate_script_with_unwrap<T: AsRef<str>, P: AsRef<std::path::Path>>(
    script: T,
    run_as_module: bool,
    script_path: Option<P>,
    unwrap_top_level_promise: bool,
) -> Result<String, JSError> {
    let script_str = script.as_ref();
    let statements = parse_program_statements(script_str, run_as_module)?;
    let script_path_buf = script_path.as_ref().map(|p| p.as_ref().to_path_buf());

    let mut arena = JsArenaVm::new(|ctx| VM::new(Chunk::new(), ctx));

    let result = arena.mutate_root(|ctx, vm| {
        if !crate::js_agent::is_agent_thread() {
            crate::js_agent::reset_agent_state();
        }

        let script_path_buf = if let Some(p) = script_path_buf.as_ref() {
            let mut p_str = p.to_string_lossy().to_string();
            if run_as_module && let Some(injected_path) = extract_injected_module_filepath(script_str) {
                p_str = injected_path;
            }

            Some(std::path::PathBuf::from(p_str))
        } else {
            None
        };

        let mut compiler = Compiler::new();
        compiler.set_source_text(script_str.to_string());
        if run_as_module && let Some(ref p) = script_path_buf {
            compiler.set_script_filename(p.to_string_lossy().to_string());
        }
        let mut main_hoisted_local_defs: Option<HashMap<String, (usize, u8)>> = None;
        let mut main_hoisted_export_defs: Option<HashMap<String, (usize, u8)>> = None;
        let mut main_code_offset: Option<usize> = None;

        // Multi-file module loading: load dependency graphs before compiling the main
        // module so import/re-export metadata is available during compilation.
        let mut main_module_record: Option<MainModuleRecord> = None;
        if run_as_module && let Some(ref entry_path) = script_path_buf {
            let main_key = entry_path.to_string_lossy().to_string();
            let (main_export_names, main_export_name_to_local, main_reexport_sources) = collect_exports_from_ast(&statements);
            vm.pre_create_module_namespace(ctx, &main_key);
            vm.seed_module_record(&main_key, &main_export_names, &main_export_name_to_local);
            vm.seed_module_export_metadata(&main_key, &main_export_name_to_local, &main_reexport_sources, entry_path);
            let main_has_tla = module_has_top_level_await(&statements);

            let self_basename = entry_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let requests = collect_module_requests(&statements, self_basename);
            main_module_record = Some((
                main_key,
                main_export_names,
                main_export_name_to_local,
                main_reexport_sources,
                requests.clone(),
                main_has_tla,
                entry_path.clone(),
            ));
            if let Some((
                main_key,
                main_export_names,
                main_export_name_to_local,
                main_reexport_sources,
                main_requests,
                main_has_tla,
                entry_path,
            )) = &main_module_record
            {
                vm.seed_pending_module_record(
                    ctx,
                    main_key,
                    script_str,
                    entry_path,
                    &statements,
                    main_export_names,
                    main_export_name_to_local,
                    main_reexport_sources,
                    main_requests,
                    *main_has_tla,
                );
                vm.mark_module_record_evaluating(main_key);
            }
            if !requests.is_empty() {
                vm.load_module_graph(ctx, entry_path, &requests);
                // Propagate load errors (e.g. SyntaxError) for all dependencies,
                // including deferred modules whose evaluation is skipped.
                vm.check_module_load_health(ctx, entry_path, &requests)?;
                // Fixup circular re-exports
                vm.fixup_circular_reexports();
                // Validate module resolution: check that all re-exports and
                // import bindings resolve to actual exports in source modules.
                if let Some((ref mk, _, ref metl, ref mrs, _, _, ref ep)) = main_module_record {
                    vm.validate_module_resolution(mk, &statements, metl, mrs, ep)?;
                }
                // Pass loaded module info to the main compiler
                for (path, exports) in &vm.loaded_modules {
                    let mut info = HashMap::new();
                    for k in exports.keys() {
                        info.insert(k.clone(), path.clone());
                    }
                    compiler.set_loaded_module_exports(path.clone(), info);
                }
            }
        }

        let chunk = compiler.compile(&statements)?;
        if let Some((_, _, main_export_name_to_local, _, _, _, _)) = &main_module_record {
            let hoisted_locals = collect_hoisted_function_names(&statements);
            if !hoisted_locals.is_empty() {
                let hoisted_defs = collect_hoisted_function_defs(&chunk, &hoisted_locals);
                main_hoisted_export_defs = Some(collect_hoisted_export_function_defs(&hoisted_defs, main_export_name_to_local));
                main_hoisted_local_defs = Some(hoisted_defs);
            }
        }

        // If dependency code was already merged into vm.chunk, merge the main
        // module's chunk too so all code shares one unified bytecode buffer.
        if run_as_module && vm.chunk.code.is_empty() {
            main_code_offset = Some(0);
            vm.chunk = chunk;
            vm.main_module_ip_start = Some(0);
        } else if run_as_module && !vm.chunk.code.is_empty() {
            // Save module-specific metadata before merge consumes the chunk
            let main_loaded_module_vars = chunk.loaded_module_vars.clone();
            let main_self_namespace_imports = chunk.self_namespace_imports.clone();
            let main_self_deferred_namespace_imports = chunk.self_deferred_namespace_imports.clone();
            let main_self_import_aliases = chunk.self_import_aliases.clone();
            let main_const_import_bindings = chunk.const_import_bindings.clone();

            let main_ip = vm.chunk.merge_dependency_chunk(chunk);
            main_code_offset = Some(main_ip);
            vm.ip = main_ip;
            vm.main_module_ip_start = Some(main_ip);

            // Restore main module's module-specific metadata on the merged chunk
            vm.chunk.loaded_module_vars = main_loaded_module_vars;
            vm.chunk.self_namespace_imports = main_self_namespace_imports;
            vm.chunk.self_deferred_namespace_imports = main_self_deferred_namespace_imports;
            vm.chunk.self_import_aliases = main_self_import_aliases;
            vm.chunk.const_import_bindings = main_const_import_bindings;
        } else {
            vm.chunk = chunk;
        }

        // In module mode, top-level `this` is undefined (not globalThis)
        if run_as_module {
            vm.set_module_this();
        }

        if let Some((
            main_key,
            main_export_names,
            main_export_name_to_local,
            main_reexport_sources,
            main_requests,
            main_has_tla,
            entry_path,
        )) = &main_module_record
        {
            vm.register_current_module_record(
                ctx,
                main_key,
                script_str,
                entry_path,
                &statements,
                main_export_names,
                main_export_name_to_local,
                main_reexport_sources,
                main_requests,
                *main_has_tla,
            );
            vm.mark_module_record_evaluating(main_key);
            if let Some(defs) = &main_hoisted_local_defs
                && !defs.is_empty()
            {
                let code_offset = main_code_offset.unwrap_or(0);
                let seeded_locals = defs
                    .iter()
                    .map(|(local_name, (ip, arity))| (local_name.clone(), Value::VmFunction(ip + code_offset, *arity)))
                    .collect::<HashMap<_, _>>();
                vm.seed_module_locals(main_key, &seeded_locals);
            }
            if let Some(defs) = &main_hoisted_export_defs
                && !defs.is_empty()
            {
                let code_offset = main_code_offset.unwrap_or(0);
                let seeded_exports = defs
                    .iter()
                    .map(|(export_name, (ip, arity))| (export_name.clone(), Value::VmFunction(ip + code_offset, *arity)))
                    .collect::<HashMap<_, _>>();
                vm.seed_module_exports(ctx, main_key, &seeded_exports);
            }
            if !main_requests.is_empty() {
                vm.evaluate_module_requests(ctx, entry_path, main_requests);
            }
        }

        // Inject loaded module bindings into module_locals before execution.
        if run_as_module {
            vm.inject_loaded_module_bindings(ctx);
        }

        // let mut vm = VM::new(chunk, ctx);
        vm.set_source_context(script_str, script_path_buf.as_deref());
        let mut v = vm.run(ctx)?;
        if let Some((main_key, main_export_names, ..)) = &main_module_record {
            vm.finalize_active_module_record(ctx, main_key, main_export_names);
        }

        // Helper behavior for eval/unit-test entry points: if the top-level result
        // is a settled Promise, expose its payload so direct callers can assert it.
        // File execution should preserve normal script semantics and must not turn
        // a bare `import()` completion value into a process-level failure.
        if unwrap_top_level_promise {
            for _ in 0..8 {
                let step = if let Value::VmObject(obj) = &v {
                    let b = obj.borrow();
                    let is_promise = matches!(b.get("__type__"), Some(Value::String(s)) if crate::unicode::utf16_to_utf8(s) == "Promise");
                    if is_promise {
                        let rejected = matches!(b.get("__promise_rejected__"), Some(Value::Boolean(true)));
                        let next = b.get("__promise_value__").cloned();
                        Some((rejected, next))
                    } else {
                        None
                    }
                } else {
                    None
                };

                let Some((rejected, next)) = step else {
                    break;
                };
                let Some(next) = next else {
                    break;
                };

                if rejected && let Value::VmObject(obj) = &next {
                    let b = obj.borrow();
                    if let Some(Value::String(t)) = b.get("__type__") {
                        let tn = crate::unicode::utf16_to_utf8(t);
                        if tn == "Error" || tn.ends_with("Error") {
                            drop(b);
                            return Err(vm.vm_error_to_js_error(ctx, &next));
                        }
                    }
                }
                v = next;
            }
        }

        match v {
            Value::String(s) => {
                let s_utf8 = crate::unicode::utf16_to_utf8(&s);
                match serde_json::to_string(&s_utf8) {
                    Ok(quoted) => Ok(quoted),
                    Err(_) => Ok(format!("\"{}\"", s_utf8)),
                }
            }
            Value::VmArray(_) | Value::VmObject(_) => Ok(value_to_compact_result_string(&v)),
            _ => Ok(value_to_string(&v)),
        }
    });

    // Run incremental GC to reclaim unreachable objects before returning.
    arena.collect_debt();

    result
}
