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
pub fn parse_statements(t: &[TokenData], index: &mut usize) -> Result<Vec<Statement>, JSError> {
    let mut statements = Vec::new();
    while *index < t.len() && t[*index].token != Token::EOF && t[*index].token != Token::RBrace {
        if matches!(t[*index].token, Token::Semicolon | Token::LineTerminator) {
            *index += 1;
            continue;
        }
        statements.push(parse_statement_item(t, index)?);
    }
    Ok(statements)
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
            parse_import_statement(t, index)
        }
        Token::Export => parse_export_statement(t, index),
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
            if *index + 1 < t.len() && matches!(t[*index + 1].token, Token::Function | Token::FunctionStar) {
                parse_function_declaration(t, index)
            } else {
                let expr = parse_expression(t, index)?;
                if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
                    *index += 1;
                }
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
                && matches!(t[*index + 1].token, Token::Identifier(_))
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
                _ => None,
            };
            if let Some(label_name) = label_name_opt
                && *index + 1 < t.len()
                && matches!(t[*index + 1].token, Token::Colon)
            {
                *index += 2;
                let stmt = parse_statement_item(t, index)?;
                return Ok(Statement {
                    kind: Box::new(StatementKind::Label(label_name, Box::new(stmt))),
                    line,
                    column,
                });
            }
            let expr = parse_expression(t, index)?;
            if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
                *index += 1;
            }
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
                let n = name.clone();
                *index += 1;
                n
            }
            Token::Await => {
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
        Some(parse_expression(t, index)?)
    } else {
        None
    };
    let members = parse_class_body(t, index)?;
    let class_def = crate::core::ClassDefinition { name, extends, members };
    Ok(Statement {
        kind: Box::new(StatementKind::Class(Box::new(class_def))),
        line: t[start].line,
        column: t[start].column,
    })
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
                    let body = parse_statement_item(t, index)?;
                    let body_stmts = match *body.kind {
                        StatementKind::Block(b) => b,
                        _ => vec![body],
                    };
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
                let body = parse_statement_item(t, index)?;
                let body_stmts = match *body.kind {
                    StatementKind::Block(b) => b,
                    _ => vec![body],
                };
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
    if is_decl {
        decl_kind = Some(t[*index].token.clone());
        *index += 1;
        if matches!(t[*index].token, Token::LBrace) {
            let pattern = parse_object_destructuring_pattern(t, index)?;
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
            init_expr = Some(parse_expression(t, index)?);
        }
    }
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < t.len() && matches!(t[* index].token, Token::Identifier(ref s) if s == "of") {
        *index += 1;
        let iterable = parse_assignment(t, index)?;
        if !matches!(t[*index].token, Token::RParen) {
            return Err(raise_parse_error_at!(t.get(*index)));
        }
        *index += 1;
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        let body = parse_statement_item(t, index)?;
        let body_stmts = match *body.kind {
            StatementKind::Block(stmts) => stmts,
            _ => vec![body],
        };
        let decl_kind_mapped: Option<crate::core::VarDeclKind> = decl_kind.and_then(|tk| match tk {
            crate::Token::Var => Some(crate::core::VarDeclKind::Var),
            crate::Token::Let => Some(crate::core::VarDeclKind::Let),
            crate::Token::Const => Some(crate::core::VarDeclKind::Const),
            _ => None,
        });
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
                let var_name = decls[0].0.clone();
                if is_for_await {
                    StatementKind::ForAwaitOf(decl_kind_mapped, var_name, iterable, body_stmts)
                } else {
                    StatementKind::ForOf(decl_kind_mapped, var_name, iterable, body_stmts)
                }
            } else if let Some(Expr::Var(s, _, _)) = init_expr {
                if is_for_await {
                    StatementKind::ForAwaitOf(decl_kind_mapped, s, iterable, body_stmts)
                } else {
                    StatementKind::ForOf(decl_kind_mapped, s, iterable, body_stmts)
                }
            } else if let Some(expr) = init_expr {
                match expr {
                    Expr::Property(_, _) | Expr::Index(_, _) | Expr::PrivateMember(_, _) | Expr::Array(_) | Expr::Object(_) => {
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
                    let body = parse_statement_item(t, index)?;
                    let body_stmts = match *body.kind {
                        StatementKind::Block(b) => b,
                        _ => vec![body],
                    };
                    return Ok(Statement {
                        kind: Box::new(StatementKind::ForIn(None, name, right_expr, body_stmts)),
                        line,
                        column,
                    });
                }
                Expr::Property(_, _) | Expr::Index(_, _) | Expr::PrivateMember(_, _) => {
                    *index += 1;
                    let body = parse_statement_item(t, index)?;
                    let body_stmts = match *body.kind {
                        StatementKind::Block(b) => b,
                        _ => vec![body],
                    };
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
        if !matches!(t[*index].token, Token::RParen) {
            return Err(raise_parse_error_at!(t.get(*index)));
        }
        *index += 1;
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        let body = parse_statement_item(t, index)?;
        let body_stmts = match *body.kind {
            StatementKind::Block(b) => b,
            _ => vec![body],
        };
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
    let body = parse_statement_item(t, index)?;
    let body_stmts = match *body.kind {
        StatementKind::Block(b) => b,
        _ => vec![body],
    };
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
        name.clone()
    } else if matches!(t[*index].token, Token::Await) {
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
    let params = parse_parameters(t, index)?;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::LBrace) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    let body = if is_async {
        push_await_context();
        let b = parse_statement_block(t, index)?;
        pop_await_context();
        b
    } else {
        with_cleared_await_context(|| parse_statement_block(t, index))?
    };
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
    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let then_stmt = parse_statement_item(t, index)?;
    let then_block = match *then_stmt.kind {
        StatementKind::Block(stmts) => stmts,
        _ => vec![then_stmt],
    };
    while *index < t.len() && matches!(t[*index].token, Token::Semicolon | Token::LineTerminator) {
        *index += 1;
    }
    let else_block = if *index < t.len() && matches!(t[*index].token, Token::Else) {
        *index += 1;
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        let else_stmt = parse_statement_item(t, index)?;
        match *else_stmt.kind {
            StatementKind::Block(stmts) => Some(stmts),
            _ => Some(vec![else_stmt]),
        }
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
        let body = parse_statement_item(t, index)?;
        match *body.kind {
            StatementKind::Block(stmts) => stmts,
            _ => vec![body],
        }
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
        let body = parse_statement_item(t, index)?;
        log::trace!(
            "parse_do_while: after parsing body index {}, next token={:?}",
            *index,
            t.get(*index)
        );
        match *body.kind {
            StatementKind::Block(stmts) => stmts,
            _ => vec![body],
        }
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
    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    if !matches!(t[*index].token, Token::LBrace) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1;
    let mut cases: Vec<crate::core::SwitchCase> = Vec::new();
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
                stmts.push(parse_statement_item(t, index)?);
            }
            cases.push(crate::core::SwitchCase::Case(case_expr, stmts));
        } else if matches!(t[*index].token, Token::Default) {
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
                stmts.push(parse_statement_item(t, index)?);
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
    let stmt = parse_statement_item(t, index)?;
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
                        catch_param = Some(CatchParamPattern::Identifier(name.clone()));
                        *index += 1;
                    }
                    Token::Await if !in_await_context() => {
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
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
    }
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
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
    }
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
        if let Some(name) = t[*index].token.as_identifier_string() {
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
                    Token::Identifier(s) if s == "as" => true,
                    Token::As => true,
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
                        Token::Identifier(s) if s == "as" => true,
                        Token::As => true,
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
                from_kw == "from"
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
    let is_with_clause = matches!(t[*index].token, Token::With) || matches!(& t[* index].token, Token::Identifier(s) if s == "with");
    if !is_with_clause {
        return Ok(());
    }
    *index += 1;
    if *index >= t.len() || !matches!(t[*index].token, Token::LBrace) {
        return Err(raise_parse_error!("Expected '{' after import attributes 'with'"));
    }
    let mut depth = 0_i32;
    while *index < t.len() {
        match t[*index].token {
            Token::LBrace => depth += 1,
            Token::RBrace => {
                depth -= 1;
                if depth == 0 {
                    *index += 1;
                    return Ok(());
                }
            }
            Token::EOF => {
                return Err(raise_parse_error!("Unterminated import attributes clause"));
            }
            _ => {}
        }
        *index += 1;
    }
    Err(raise_parse_error!("Unterminated import attributes clause"))
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
        *index += 1;
        let should_normalize_default_function_name =
            *index < t.len() && matches!(t[*index].token, Token::Function | Token::FunctionStar | Token::Async);
        let mut expr = parse_assignment(t, index)?;
        if should_normalize_default_function_name {
            expr = match expr {
                Expr::Function(None, params, body) => Expr::Function(Some("default".to_string()), params, body),
                Expr::GeneratorFunction(None, params, body) => Expr::GeneratorFunction(Some("default".to_string()), params, body),
                Expr::AsyncFunction(None, params, body) => Expr::AsyncFunction(Some("default".to_string()), params, body),
                Expr::AsyncGeneratorFunction(None, params, body) => Expr::AsyncGeneratorFunction(Some("default".to_string()), params, body),
                other => other,
            };
        }
        specifiers.push(ExportSpecifier::Default(expr));
        if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
            *index += 1;
        }
    } else if *index < t.len() && matches!(t[*index].token, Token::Multiply) {
        *index += 1;
        let is_as = if *index < t.len() {
            match &t[*index].token {
                Token::Identifier(s) if s == "as" => true,
                Token::As => true,
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
                from_kw == "from"
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
        if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
            *index += 1;
        }
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
                    Token::Identifier(s) if s == "as" => true,
                    Token::As => true,
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
                from_kw == "from"
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
        if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
            *index += 1;
        }
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
    Ok(Statement {
        kind: Box::new(StatementKind::Export(specifiers, inner_stmt, source)),
        line: t[start].line,
        column: t[start].column,
    })
}
/// Parse `using x = expr, y = expr;` declaration
fn parse_using_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
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
        match &t[*index].token {
            Token::Identifier(name) => {
                let name = name.clone();
                *index += 1;
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                let init = if *index < t.len() && matches!(t[*index].token, Token::Assign) {
                    *index += 1;
                    Some(parse_assignment(t, index)?)
                } else {
                    None
                };
                decls.push((name, init));
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
            }
            Token::Await => {
                let name = "await".to_string();
                *index += 1;
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                let init = if *index < t.len() && matches!(t[*index].token, Token::Assign) {
                    *index += 1;
                    Some(parse_assignment(t, index)?)
                } else {
                    None
                };
                decls.push((name, init));
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
            }
            Token::Async => {
                let name = "async".to_string();
                *index += 1;
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                let init = if *index < t.len() && matches!(t[*index].token, Token::Assign) {
                    *index += 1;
                    Some(parse_assignment(t, index)?)
                } else {
                    None
                };
                decls.push((name, init));
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
            }
            Token::As => {
                let name = "as".to_string();
                *index += 1;
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                let init = if *index < t.len() && matches!(t[*index].token, Token::Assign) {
                    *index += 1;
                    Some(parse_assignment(t, index)?)
                } else {
                    None
                };
                decls.push((name, init));
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
            }
            _ if matches!(t[*index].token, Token::Static) => {
                let name = "static".to_string();
                *index += 1;
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
                let init = if *index < t.len() && matches!(t[*index].token, Token::Assign) {
                    *index += 1;
                    Some(parse_assignment(t, index)?)
                } else {
                    None
                };
                decls.push((name, init));
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
            }
            _ => break,
        }
        if *index < t.len() && matches!(t[*index].token, Token::Comma) {
            *index += 1;
        } else {
            break;
        }
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
            let mut next = j + 1;
            while next < tokens.len() && matches!(tokens[next].token, Token::LineTerminator) {
                next += 1;
            }
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
        if *index >= tokens.len() {
            break;
        }
        if let Some(op) = op_mapper(&tokens[*index].token) {
            *index += 1;
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
            } else if matches!(tokens[*index].token, Token::Await) {
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
    while *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
        *index += 1;
        let right = parse_full_expression(tokens, index)?;
        left = Expr::Comma(Box::new(left), Box::new(right));
    }
    Ok(left)
}
pub fn parse_conditional(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    let condition = parse_nullish(tokens, index)?;
    if *index >= tokens.len() {
        return Ok(condition);
    }
    if matches!(tokens[*index].token, Token::QuestionMark) {
        *index += 1;
        let true_expr = parse_conditional(tokens, index)?;
        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::Colon) {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
        *index += 1;
        let false_expr = parse_conditional(tokens, index)?;
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
pub fn parse_assignment(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    log::trace!("parse_assignment: entry index={} token={:?}", *index, tokens.get(*index));
    if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace | Token::LBracket) {
        let mut idx = *index;
        let pattern_expr_res = if matches!(tokens[idx].token, Token::LBracket) {
            parse_array_assignment_pattern(tokens, &mut idx).map(Expr::Array)
        } else {
            parse_object_assignment_pattern(tokens, &mut idx).map(Expr::Object)
        };
        if let Ok(pattern_expr) = pattern_expr_res {
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
    }
    let left = parse_conditional(tokens, index)?;
    if *index >= tokens.len() {
        return Ok(left);
    }
    if let Some(ctor) = get_assignment_ctor(&tokens[*index].token) {
        if contains_optional_chain(&left) {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
        *index += 1;
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
    parse_binary_op(tokens, index, parse_shift, |token| match token {
        Token::LessThan => Some(BinaryOp::LessThan),
        Token::GreaterThan => Some(BinaryOp::GreaterThan),
        Token::LessEqual => Some(BinaryOp::LessEqual),
        Token::GreaterEqual => Some(BinaryOp::GreaterEqual),
        Token::InstanceOf => Some(BinaryOp::InstanceOf),
        Token::In => Some(BinaryOp::In),
        _ => None,
    })
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
    if *index >= tokens.len() {
        return Ok(left);
    }
    if matches!(tokens[*index].token, Token::LogicalAnd) {
        *index += 1;
        let right = parse_logical_and(tokens, index)?;
        Ok(Expr::LogicalAnd(Box::new(left), Box::new(right)))
    } else {
        Ok(left)
    }
}
fn parse_logical_or(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    let left = parse_logical_and(tokens, index)?;
    if *index >= tokens.len() {
        return Ok(left);
    }
    if matches!(tokens[*index].token, Token::LogicalOr) {
        *index += 1;
        let right = parse_logical_or(tokens, index)?;
        Ok(Expr::LogicalOr(Box::new(left), Box::new(right)))
    } else {
        Ok(left)
    }
}
fn parse_nullish(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    let left = parse_logical_or(tokens, index)?;
    if *index >= tokens.len() {
        return Ok(left);
    }
    if matches!(tokens[*index].token, Token::NullishCoalescing) {
        *index += 1;
        let right = parse_nullish(tokens, index)?;
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
    let left = parse_primary(tokens, index, true)?;
    if *index >= tokens.len() {
        return Ok(left);
    }
    if matches!(tokens[*index].token, Token::Exponent) {
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
            let body = parse_statement_block(t, index)?;
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
                    is_accessor = true;
                    is_getter = kw == "get";
                    log::trace!("parse_primary: accessor recognized (kw={}) at idx={}", kw, *index);
                } else {
                    if !matches!(next.token, Token::LParen) && next.token.as_identifier_string().is_some() {
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
                    let expr = parse_assignment(t, index)?;
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
            let params = parse_parameters(t, index)?;
            if *index >= t.len() || !matches!(t[*index].token, Token::LBrace) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1;
            let body = parse_statement_block(t, index)?;
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
                let expr = parse_assignment(t, index)?;
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
        if computed_key_expr.is_none()
            && !is_static
            && !is_private
            && name_str_opt.as_deref() == Some("constructor")
            && matches!(t.get(*index).map(|d| &d.token), Some(Token::LParen))
        {
            *index += 1;
            let params = parse_parameters(t, index)?;
            if *index >= t.len() || !matches!(t[*index].token, Token::LBrace) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1;
            let body = parse_statement_block(t, index)?;
            members.push(ClassMember::Constructor(params, body));
            continue;
        }
        if *index < t.len() && matches!(t[*index].token, Token::LParen) {
            *index += 1;
            let params = parse_parameters(t, index)?;
            if *index >= t.len() || !matches!(t[*index].token, Token::LBrace) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1;
            let body = parse_statement_block(t, index)?;
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
            let value = parse_expression(t, index)?;
            if *index < t.len() && matches!(t[*index].token, Token::Semicolon | Token::LineTerminator) {
                *index += 1;
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
            if *index < t.len() && matches!(t[*index].token, Token::Semicolon | Token::LineTerminator) {
                *index += 1;
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
            Expr::Delete(Box::new(inner))
        }
        Token::Void => {
            let inner = parse_primary(tokens, index, true)?;
            Expr::Void(Box::new(inner))
        }
        Token::Await => {
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
        Token::Yield => {
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Multiply) {
                *index += 1;
                let inner = parse_assignment(tokens, index)?;
                Expr::YieldStar(Box::new(inner))
            } else if *index >= tokens.len()
                || matches!(
                    tokens[*index].token,
                    Token::Semicolon
                        | Token::Comma
                        | Token::RParen
                        | Token::RBracket
                        | Token::RBrace
                        | Token::Colon
                        | Token::LineTerminator
                )
            {
                Expr::Yield(None)
            } else {
                let inner = parse_assignment(tokens, index)?;
                Expr::Yield(Some(Box::new(inner)))
            }
        }
        Token::YieldStar => {
            let inner = parse_assignment(tokens, index)?;
            Expr::YieldStar(Box::new(inner))
        }
        Token::LogicalNot => {
            let inner = parse_primary(tokens, index, true)?;
            Expr::LogicalNot(Box::new(inner))
        }
        Token::Class => {
            let name = if *index < tokens.len() {
                match &tokens[*index].token {
                    Token::Identifier(n) => {
                        let n = n.clone();
                        *index += 1;
                        n
                    }
                    Token::Await => {
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
                Some(parse_expression(tokens, index)?)
            } else {
                None
            };
            let members = parse_class_body(tokens, index)?;
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
                    *index = look + 1;
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if is_new_target {
                Expr::NewTarget
            } else {
                let constructor = parse_primary(tokens, index, false)?;
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
            let mut expr = Expr::Var(name.clone(), Some(line), Some(column));
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
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
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                *index += 1;
                let body = parse_arrow_body(tokens, index)?;
                expr = Expr::ArrowFunction(vec![DestructuringElement::Variable("as".to_string(), None)], body);
            }
            expr
        }
        Token::PrivateIdentifier(name) => Expr::PrivateName(name.clone()),
        Token::Import => {
            if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                *index += 1;
                let arg = parse_assignment(tokens, index)?;
                let mut options_arg: Option<Box<Expr>> = None;
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
                    *index += 1;
                    if !(*index < tokens.len() && matches!(tokens[*index].token, Token::RParen)) {
                        let opt = parse_assignment(tokens, index)?;
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
            } else {
                Expr::Var("import".to_string(), Some(token_data.line), Some(token_data.column))
            }
        }
        Token::Regex(pattern, flags) => Expr::Regex(pattern.clone(), flags.clone()),
        Token::This => Expr::This,
        Token::Super => {
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
                    }
                }
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                Expr::SuperCall(args)
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Dot) {
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
                Expr::Super
            }
        }
        Token::LBrace => {
            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                *index += 1;
            }
            let mut properties = Vec::new();
            if *index < tokens.len() && matches!(tokens[*index].token, Token::RBrace) {
                *index += 1;
            } else {
                loop {
                    log::trace!(
                        "parse_primary: object literal loop; next tokens (first 8): {:?}",
                        tokens.iter().take(8).collect::<Vec<_>>()
                    );
                    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator | Token::Semicolon) {
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
                                    || (tokens[peek].token.as_identifier_string().is_some()
                                        && !matches!(tokens[peek].token, Token::Async)));
                            if next_starts_method_name {
                                is_async_member = true;
                                *index += 1;
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
                            let params = parse_parameters(tokens, index)?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            let body = parse_statements(tokens, index)?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            if is_generator {
                                if is_async_member {
                                    properties.push((
                                        Expr::StringLit(crate::unicode::utf8_to_utf16("yield")),
                                        Expr::AsyncGeneratorFunction(None, params, body),
                                        false,
                                        false,
                                    ));
                                } else {
                                    properties.push((
                                        Expr::StringLit(crate::unicode::utf8_to_utf16("yield")),
                                        Expr::GeneratorFunction(None, params, body),
                                        false,
                                        false,
                                    ));
                                }
                            } else if is_async_member {
                                properties.push((
                                    Expr::StringLit(crate::unicode::utf8_to_utf16("yield")),
                                    Expr::AsyncFunction(None, params, body),
                                    false,
                                    false,
                                ));
                            } else {
                                properties.push((
                                    Expr::StringLit(crate::unicode::utf8_to_utf16("yield")),
                                    Expr::Function(None, params, body),
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
                                let params = parse_parameters(tokens, index)?;
                                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                    return Err(raise_parse_error_at!(tokens.get(*index)));
                                }
                                *index += 1;
                                let body = parse_statements(tokens, index)?;
                                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                    return Err(raise_parse_error_at!(tokens.get(*index)));
                                }
                                *index += 1;
                                if is_generator {
                                    if is_async_member {
                                        properties.push((
                                            Expr::StringLit(crate::unicode::utf8_to_utf16(&name)),
                                            Expr::AsyncGeneratorFunction(None, params, body),
                                            false,
                                            false,
                                        ));
                                    } else {
                                        properties.push((
                                            Expr::StringLit(crate::unicode::utf8_to_utf16(&name)),
                                            Expr::GeneratorFunction(None, params, body),
                                            false,
                                            false,
                                        ));
                                    }
                                } else if is_async_member {
                                    properties.push((
                                        Expr::StringLit(crate::unicode::utf8_to_utf16(&name)),
                                        Expr::AsyncFunction(None, params, body),
                                        false,
                                        false,
                                    ));
                                } else {
                                    properties.push((
                                        Expr::StringLit(crate::unicode::utf8_to_utf16(&name)),
                                        Expr::Function(None, params, body),
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
                                    let params = parse_parameters(tokens, index)?;
                                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                        return Err(raise_parse_error_at!(tokens.get(*index)));
                                    }
                                    *index += 1;
                                    let body = parse_statements(tokens, index)?;
                                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                        return Err(raise_parse_error_at!(tokens.get(*index)));
                                    }
                                    *index += 1;
                                    if is_generator {
                                        if is_async_member {
                                            properties.push((
                                                Expr::StringLit(crate::unicode::utf8_to_utf16(&id)),
                                                Expr::AsyncGeneratorFunction(None, params, body),
                                                false,
                                                false,
                                            ));
                                        } else {
                                            properties.push((
                                                Expr::StringLit(crate::unicode::utf8_to_utf16(&id)),
                                                Expr::GeneratorFunction(None, params, body),
                                                false,
                                                false,
                                            ));
                                        }
                                    } else if is_async_member {
                                        properties.push((
                                            Expr::StringLit(crate::unicode::utf8_to_utf16(&id)),
                                            Expr::AsyncFunction(None, params, body),
                                            false,
                                            false,
                                        ));
                                    } else {
                                        properties.push((
                                            Expr::StringLit(crate::unicode::utf8_to_utf16(&id)),
                                            Expr::Function(None, params, body),
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
                                let expr = parse_assignment(tokens, index)?;
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
                            let expr = parse_assignment(tokens, index)?;
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
                            let params = parse_parameters(tokens, index)?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            let body = parse_statements(tokens, index)?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            if is_generator {
                                properties.push((key_expr, Expr::GeneratorFunction(None, params, body), key_is_computed, false));
                            } else {
                                properties.push((key_expr, Expr::Function(None, params, body), key_is_computed, false));
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
                            let body = parse_statements(tokens, index)?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            properties.push((
                                key_expr,
                                Expr::Getter(Box::new(Expr::Function(None, Vec::new(), body))),
                                false,
                                false,
                            ));
                        } else if is_setter {
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LParen) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            let params = parse_parameters(tokens, index)?;
                            if params.len() != 1 {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            let body = parse_statements(tokens, index)?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1;
                            properties.push((key_expr, Expr::Setter(Box::new(Expr::Function(None, params, body))), false, false));
                        } else {
                            if *index < tokens.len() && matches!(tokens[*index].token, Token::Colon) {
                                *index += 1;
                                let value = parse_assignment(tokens, index)?;
                                properties.push((key_expr, value, key_is_computed, true));
                            } else {
                                if is_shorthand_candidate {
                                    if let Expr::StringLit(s) = &key_expr {
                                        let name = utf16_to_utf8(s);
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
                let params = parse_parameters(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                let body = parse_statements(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                if is_generator {
                    log::trace!("parse_primary: constructed GeneratorFunction name={:?} params={:?}", name, params);
                    Expr::GeneratorFunction(name, params, body)
                } else {
                    log::trace!("parse_primary: constructed Function name={:?} params={:?}", name, params);
                    Expr::Function(name, params, body)
                }
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::RParen) {
                *index += 1;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                let body = parse_statements(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                if is_generator {
                    log::trace!("parse_primary: constructed GeneratorFunction name={:?} params=Vec::new()", name);
                    Expr::GeneratorFunction(name, Vec::new(), body)
                } else {
                    log::trace!("parse_primary: constructed Function name={:?} params=Vec::new()", name);
                    Expr::Function(name, Vec::new(), body)
                }
            } else {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
        }
        Token::Async => {
            let start = *index - 1;
            let next = *index;
            log::trace!(
                "parse_primary: Token::Async start={} *index={} tokens_slice={:?}",
                start,
                *index,
                tokens.iter().skip(start).take(4).collect::<Vec<_>>()
            );
            let mut is_generator = false;
            if next < tokens.len() && (matches!(tokens[next].token, Token::Function) || matches!(tokens[next].token, Token::FunctionStar)) {
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
                    let params = parse_parameters(tokens, index)?;
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
                    let body = parse_statements(tokens, index)?;
                    pop_await_context();
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                    *index += 1;
                    if is_generator {
                        log::trace!("parse_primary: constructed AsyncGeneratorFunction name={name:?} params={params:?}");
                        Expr::AsyncGeneratorFunction(name, params, body)
                    } else {
                        log::trace!("parse_primary: constructed AsyncFunction name={name:?} params={params:?}");
                        Expr::AsyncFunction(name, params, body)
                    }
                } else {
                    log::trace!(
                        "parse_primary (async): missing '(' after 'function' at idx {} token={:?}",
                        *index,
                        tokens.get(*index)
                    );
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                log::trace!("parse_primary (async): detected '(' => possible async arrow at idx {}", *index);
                *index += 1;
                let saved_idx = *index;
                if let Ok(p) = parse_parameters(tokens, index) {
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                        *index += 1;
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
                            *index += 1;
                            push_await_context();
                            let body = parse_statement_block(tokens, index)?;
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
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Identifier(_)) {
                if let Token::Identifier(name) = &tokens[*index].token {
                    let ident_name = name.clone();
                    let mut j = *index + 1;
                    while j < tokens.len() && matches!(tokens[j].token, Token::LineTerminator) {
                        j += 1;
                    }
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
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                    *index += 1;
                    let body = parse_arrow_body(tokens, index)?;
                    expr = Expr::ArrowFunction(vec![DestructuringElement::Variable("async".to_string(), None)], body);
                }
                expr
            } else {
                let line = token_data.line;
                let column = token_data.column;
                let mut expr = Expr::Var("async".to_string(), Some(line), Some(column));
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                    *index += 1;
                    let body = parse_arrow_body(tokens, index)?;
                    expr = Expr::ArrowFunction(vec![DestructuringElement::Variable("async".to_string(), None)], body);
                }
                expr
            }
        }
        Token::LParen => {
            log::trace!(
                "parse_primary: entered LParen branch at idx {} tokens={:?}",
                *index,
                tokens.iter().skip(*index).take(8).collect::<Vec<_>>()
            );
            if *index < tokens.len() && matches!(tokens[*index].token, Token::RParen) {
                let prev = if *index >= 1 { Some(&tokens[*index - 1]) } else { None };
                log::trace!("paren-rcase: idx={} prev={:?} token_at_idx={:?}", *index, prev, tokens.get(*index));
                if let Some(prev_td) = prev
                    && matches!(prev_td.token, Token::LParen)
                {
                    let mut next = *index + 1;
                    while next < tokens.len() && matches!(tokens[next].token, Token::LineTerminator) {
                        next += 1;
                    }
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
                            let mut m = k + 1;
                            while m < tokens.len() && matches!(tokens[m].token, Token::LineTerminator) {
                                m += 1;
                            }
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
                        let mut next = *index + 1;
                        while next < tokens.len() && matches!(tokens[next].token, Token::LineTerminator) {
                            next += 1;
                        }
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
                    let mut next = j + 1;
                    while next < tokens.len() && matches!(tokens[next].token, Token::LineTerminator) {
                        next += 1;
                    }
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
            expr_inner
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
            if look < tokens.len() && matches!(tokens[look].token, Token::Increment | Token::Decrement) {
                break;
            }
            *index = look;
        }
        if *index >= tokens.len() {
            break;
        }
        match &tokens[*index].token {
            Token::LBracket => {
                *index += 1;
                let index_expr = parse_expression(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                expr = Expr::Index(Box::new(expr), Box::new(index_expr));
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
                    expr = Expr::Property(Box::new(expr), prop);
                } else if let Token::PrivateIdentifier(prop) = &tokens[*index].token {
                    let invalid = PRIVATE_NAME_STACK.with(|s| {
                        let stack = s.borrow();
                        !stack.iter().rev().any(|rc| rc.borrow().contains(prop))
                    });
                    if invalid {
                        let msg = format!("Private field '#{}' must be declared in an enclosing class", prop);
                        return Err(raise_parse_error_with_token!(tokens[*index], msg));
                    }
                    let prop = format!("#{}", prop);
                    *index += 1;
                    expr = Expr::PrivateMember(Box::new(expr), prop);
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
                    let prop = format!("#{prop}");
                    *index += 1;
                    expr = Expr::OptionalPrivateMember(Box::new(expr), prop);
                } else if matches!(tokens[*index].token, Token::LBracket) {
                    *index += 1;
                    let index_expr = parse_expression(tokens, index)?;
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
                expr = Expr::Call(Box::new(expr), args);
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
        let body = if is_async {
            push_await_context();
            let r = parse_statements(tokens, index);
            pop_await_context();
            r?
        } else {
            with_cleared_await_context(|| parse_statements(tokens, index))?
        };
        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
        *index += 1;
        Ok(body)
    } else {
        let expr = if is_async {
            push_await_context();
            let r = parse_assignment(tokens, index);
            pop_await_context();
            r?
        } else {
            with_cleared_await_context(|| parse_assignment(tokens, index))?
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
                *index += 1;
                pattern.push(DestructuringElement::Rest(name));
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Await) && !in_await_context() {
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
        } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Await) && !in_await_context() {
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
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Await) && !in_await_context() {
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
                } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Await) && !in_await_context() {
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
                        Expr::AsyncFunction(Some(name), _params, _body) | Expr::Function(Some(name), _params, _body) => {
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
