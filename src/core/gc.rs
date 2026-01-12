use crate::core::{Collect, GcTrace};
use crate::core::{DestructuringElement, Expr, Statement, StatementKind};

// Manual implementation of trace to avoid cycles in derive
pub fn trace_expr<'gc, T: GcTrace<'gc>>(context: &mut T, expr: &Expr) {
    // helper to find and trace any Expr defaults nested in destructuring elements
    fn trace_destructuring<'gc, T: GcTrace<'gc>>(cc: &mut T, d: &DestructuringElement) {
        match d {
            DestructuringElement::Variable(_, Some(e)) => trace_expr(cc, e),
            DestructuringElement::Property(_, inner) => trace_destructuring(cc, inner),
            DestructuringElement::Variable(_, None) => {}
            DestructuringElement::Rest(_) => {}
            DestructuringElement::Empty => {}
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
        Expr::DynamicImport(a) => {
            trace_expr(context, a);
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

pub fn trace_stmt<'gc, T: GcTrace<'gc>>(context: &mut T, stmt: &Statement) {
    match &*stmt.kind {
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
        StatementKind::If(if_stmt) => {
            let if_stmt = if_stmt.as_ref();
            trace_expr(context, &if_stmt.condition);
            for s in &if_stmt.then_body {
                trace_stmt(context, s);
            }
            if let Some(e) = &if_stmt.else_body {
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
        StatementKind::TryCatch(tc_stmt) => {
            let tc_stmt = tc_stmt.as_ref();
            for s in &tc_stmt.try_body {
                trace_stmt(context, s);
            }
            if let Some(stmts) = &tc_stmt.catch_body {
                for s in stmts {
                    trace_stmt(context, s);
                }
            }
            if let Some(stmts) = &tc_stmt.finally_body {
                for s in stmts {
                    trace_stmt(context, s);
                }
            }
        }
        StatementKind::For(for_stmt) => {
            let for_stmt = for_stmt.as_ref();
            if let Some(init) = &for_stmt.init {
                trace_stmt(context, init);
            }
            if let Some(test) = &for_stmt.test {
                trace_expr(context, test);
            }
            if let Some(update) = &for_stmt.update {
                trace_stmt(context, update);
            }
            for s in &for_stmt.body {
                trace_stmt(context, s);
            }
        }
        StatementKind::Switch(sw_stmt) => {
            let sw_stmt = sw_stmt.as_ref();
            trace_expr(context, &sw_stmt.expr);
            for case in &sw_stmt.cases {
                match case {
                    crate::core::SwitchCase::Case(e, stmts) => {
                        trace_expr(context, e);
                        for s in stmts {
                            trace_stmt(context, s);
                        }
                    }
                    crate::core::SwitchCase::Default(stmts) => {
                        for s in stmts {
                            trace_stmt(context, s);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}
