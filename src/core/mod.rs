use crate::error::JSError;
use crate::raise_eval_error;
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
    let mut tokens = tokenize(script)?;
    if tokens.last().map(|td| td.token == Token::EOF).unwrap_or(false) {
        tokens.pop();
    }

    let mut index = 0;
    if !run_as_module {
        let enable_top_level_await = !script_declares_await_identifier(script);
        if enable_top_level_await {
            crate::core::parser::push_await_context();
            let res = parse_statements(&tokens, &mut index);
            crate::core::parser::pop_await_context();
            res
        } else {
            parse_statements(&tokens, &mut index)
        }
    } else {
        crate::core::parser::push_await_context();
        let res = parse_statements(&tokens, &mut index);
        crate::core::parser::pop_await_context();
        res
    }
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
                    StatementKind::FunctionDeclaration(name, _, _, _, _) => {
                        if !name.is_empty() && !export_names.contains(name) {
                            export_names.push(name.clone());
                            export_name_to_local.insert(name.clone(), name.clone());
                        }
                    }
                    StatementKind::Class(cd) => {
                        if !cd.name.is_empty() {
                            if !export_names.contains(&cd.name) {
                                export_names.push(cd.name.clone());
                            }
                            export_name_to_local.insert(cd.name.clone(), cd.name.clone());
                        }
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
            StatementKind::Import(_, source) => {
                let import_base = source.strip_prefix("./").unwrap_or(source);
                if import_base == self_basename {
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
            StatementKind::Import(specifiers, source) => {
                let phase = if specifiers.iter().any(|spec| matches!(spec, ImportSpecifier::DeferredNamespace(_))) {
                    ModuleRequestPhase::Defer
                } else {
                    ModuleRequestPhase::Evaluation
                };
                Some((source, phase))
            }
            StatementKind::Export(_specs, _, Some(source)) => Some((source, ModuleRequestPhase::Evaluation)),
            _ => None,
        };

        let Some((source, phase)) = maybe_request else {
            continue;
        };
        let import_base = source.strip_prefix("./").unwrap_or(source);
        if import_base == self_basename || known_builtins.contains(&source.as_str()) {
            continue;
        }
        if seen.insert((source.clone(), phase)) {
            requests.push(ModuleRequest {
                specifier: source.clone(),
                phase,
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
                // Fixup circular re-exports
                vm.fixup_circular_reexports();
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
