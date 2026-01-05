use crate::core::{DestructuringElement, Expr, Statement, StatementKind};
use gc_arena::collect::Trace;
use gc_arena::lock::RefLock as GcCell;
use gc_arena::{Collect, Gc};

pub type GcPtr<'gc, T> = Gc<'gc, GcCell<T>>;

// Manual implementation of trace to avoid cycles in derive
pub fn trace_expr<'gc, T: Trace<'gc>>(context: &mut T, expr: &Expr) {
    // helper to find and trace any Expr defaults nested in destructuring elements
    fn trace_destructuring<'gc, T: Trace<'gc>>(cc: &mut T, d: &DestructuringElement) {
        match d {
            DestructuringElement::Variable(_, Some(e)) => trace_expr(cc, e),
            DestructuringElement::Property(_, inner) => trace_destructuring(cc, inner),
            DestructuringElement::NestedArray(arr) => {
                for a in arr {
                    trace_destructuring(cc, a);
                }
            }
            DestructuringElement::NestedObject(obj) => {
                for e in obj {
                    trace_destructuring(cc, e);
                }
            }
            _ => {}
        }
    }

    match expr {
        Expr::Assign(a, b) => {
            trace_expr(context, a);
            trace_expr(context, b);
        }
        Expr::Binary(a, _, b) => {
            trace_expr(context, a);
            trace_expr(context, b);
        }
        Expr::Call(a, args) => {
            trace_expr(context, a);
            for arg in args {
                trace_expr(context, arg);
            }
        }
        Expr::Function(_, _, body) => {
            for stmt in body {
                trace_stmt(context, stmt);
            }
        }
        Expr::TemplateString(parts) => {
            for part in parts {
                if let crate::core::TemplatePart::Expr(tokens) = part {
                    for token in tokens {
                        token.trace(context);
                    }
                }
            }
        }
        Expr::ArrowFunction(params, body) => {
            for param in params {
                trace_destructuring(context, param);
            }
            for stmt in body {
                trace_stmt(context, stmt);
            }
        }
        Expr::AsyncArrowFunction(params, body) => {
            for param in params {
                trace_destructuring(context, param);
            }
            for stmt in body {
                trace_stmt(context, stmt);
            }
        }
        _ => {}
    }
}

pub fn trace_stmt<'gc, T: Trace<'gc>>(context: &mut T, stmt: &Statement) {
    match &stmt.kind {
        StatementKind::Expr(e) => trace_expr(context, e),
        StatementKind::Let(decls) | StatementKind::Var(decls) => {
            for (_, e_opt) in decls {
                if let Some(e) = e_opt {
                    trace_expr(context, e);
                }
            }
        }
        StatementKind::Const(decls) => {
            for (_, e) in decls {
                trace_expr(context, e);
            }
        }
        StatementKind::Return(e_opt) => {
            if let Some(e) = e_opt {
                trace_expr(context, e);
            }
        }
        StatementKind::Throw(e) => {
            trace_expr(context, e);
        }
        StatementKind::Block(stmts) => {
            for s in stmts {
                trace_stmt(context, s);
            }
        }
        StatementKind::If(c, t, e_opt) => {
            trace_expr(context, c);
            for s in t {
                trace_stmt(context, s);
            }
            if let Some(e) = e_opt {
                for s in e {
                    trace_stmt(context, s);
                }
            }
        }
        StatementKind::FunctionDeclaration(_, _, body, _) => {
            for s in body {
                trace_stmt(context, s);
            }
        }
        StatementKind::TryCatch(try_body, _, catch_body, finally_body) => {
            for s in try_body {
                trace_stmt(context, s);
            }
            if let Some(stmts) = catch_body {
                for s in stmts {
                    trace_stmt(context, s);
                }
            }
            if let Some(stmts) = finally_body {
                for s in stmts {
                    trace_stmt(context, s);
                }
            }
        }
    }
}
