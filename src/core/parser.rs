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
use crate::{raise_parse_error, raise_parse_error_at, raise_parse_error_with_token, unicode::utf16_to_utf8};
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
    // Skip leading line terminators when parsing a single statement.
    // A leading semicolon denotes an empty statement, so consume it and return a ValuePlaceholder expression.
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    // Empty statement (single semicolon)
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
            // consume debugger statement
            *index += 1; // consume debugger
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
            if let Token::Identifier(name) = &start_token.token
                && *index + 1 < t.len()
                && matches!(t[*index + 1].token, Token::Colon)
            {
                let label_name = name.clone();
                *index += 2; // consume Identifier and Colon
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
    // Tracks whether we are currently parsing inside an async function/arrow/function-expression body.
    // When > 0, `await` should be parsed as an operator; otherwise it should be treated as an IdentifierName.
    static AWAIT_CONTEXT: RefCell<usize> = const { RefCell::new(0) };
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

fn parse_class_declaration(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1; // consume class
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
    *index += 1; // consume for

    // Optional `await` for for-await-of
    let mut is_for_await = false;
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < t.len() && matches!(t[*index].token, Token::Await) {
        is_for_await = true;
        *index += 1; // consume await
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
    }

    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume (

    // Skip any leading line terminators after the opening paren so constructs like
    // `for (\n let ...` are correctly recognized as declarations.
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }

    // Check for declaration
    let is_decl = matches!(t[*index].token, Token::Var | Token::Let | Token::Const);
    log::trace!("parse_for_statement: is_decl={} token={:?}", is_decl, t.get(*index));

    let mut init_expr: Option<Expr> = None;
    let mut init_decls: Option<Vec<(String, Option<Expr>)>> = None;
    let mut decl_kind = None;
    let mut for_of_pattern: Option<ForOfPattern> = None;

    if is_decl {
        decl_kind = Some(t[*index].token.clone());
        *index += 1; // consume var/let/const

        // Check for destructuring
        if matches!(t[*index].token, Token::LBrace) {
            let pattern = parse_object_destructuring_pattern(t, index)?;
            log::trace!(
                "parse_for_statement: parsed object destructuring pattern, index {} token={:?}",
                *index,
                t.get(*index)
            );
            for_of_pattern = Some(ForOfPattern::Object(pattern));
        } else if matches!(t[*index].token, Token::LBracket) {
            let pattern = parse_array_destructuring_pattern(t, index)?;
            log::trace!(
                "parse_for_statement: parsed array destructuring pattern, index {} token={:?}",
                *index,
                t.get(*index)
            );
            for_of_pattern = Some(ForOfPattern::Array(pattern));
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
        init_expr = Some(parse_expression(t, index)?);
    }

    // Handle 'of'
    if *index < t.len() && matches!(t[*index].token, Token::Identifier(ref s) if s == "of") {
        *index += 1; // consume of
        let iterable = parse_assignment(t, index)?;

        if !matches!(t[*index].token, Token::RParen) {
            return Err(raise_parse_error_at!(t.get(*index)));
        }
        *index += 1; // consume )

        // Skip any line terminators before for-of body (do not skip semicolons: they can be empty-statement body)
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        let body = parse_statement_item(t, index)?;
        let body_stmts = match *body.kind {
            StatementKind::Block(stmts) => stmts,
            _ => vec![body],
        };

        // Map token-based decl_kind (if present) to VarDeclKind
        let decl_kind_mapped: Option<crate::core::VarDeclKind> = decl_kind.and_then(|tk| match tk {
            crate::Token::Var => Some(crate::core::VarDeclKind::Var),
            crate::Token::Let => Some(crate::core::VarDeclKind::Let),
            crate::Token::Const => Some(crate::core::VarDeclKind::Const),
            _ => None,
        });

        let kind = if let Some(pattern) = for_of_pattern {
            match pattern {
                ForOfPattern::Object(destr_pattern) => {
                    // Convert Vec<DestructuringElement> -> Vec<ObjectDestructuringElement>
                    let mut obj_pattern: Vec<ObjectDestructuringElement> = Vec::new();
                    for elem in destr_pattern.into_iter() {
                        match elem {
                            DestructuringElement::Property(key, boxed) => {
                                obj_pattern.push(ObjectDestructuringElement::Property { key, value: *boxed });
                            }
                            DestructuringElement::Rest(name) => {
                                obj_pattern.push(ObjectDestructuringElement::Rest(name));
                            }
                            _ => return Err(raise_parse_error!("Invalid element in object destructuring pattern", line, column)),
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
            // Non-destructuring for-of: could be a variable declaration, simple identifier, or an assignment-form expression (property/index)
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
                // Allow property/index expressions (assignment form) as left-hand side
                // e.g., `for (obj.prop of iterable) ...`
                match expr {
                    Expr::Property(_, _) | Expr::Index(_, _) => {
                        if is_for_await {
                            StatementKind::ForAwaitOfExpr(expr, iterable, body_stmts)
                        } else {
                            StatementKind::ForOfExpr(expr, iterable, body_stmts)
                        }
                    }
                    _ => return Err(raise_parse_error!("Invalid for-of left-hand side", line, column)),
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

    // Handle 'in'
    let mut is_for_in = false;
    let mut for_in_rhs = None;

    // Allow line terminators between the left-hand side and the 'in' keyword
    // so constructs like `for (let [x] \n in obj)` are accepted.
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    log::trace!("parse_for_statement: token before 'in' check={:?}", t.get(*index));

    if *index < t.len() && matches!(t[*index].token, Token::In) {
        is_for_in = true;
        *index += 1;
        for_in_rhs = Some(parse_expression(t, index)?);
    } else if !is_decl && init_expr.is_some() && matches!(t[*index].token, Token::RParen) {
        // Check if init_expr contains an `in` as the left-most element of a comma expression
        // or as a top-level Binary op. If found, extract the LHS and RHS appropriately.
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
                // Simple identifier assignment-form
                Expr::Var(name, _, _) => {
                    *index += 1; // consume )
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
                // Member/index assignment-form
                Expr::Property(_, _) | Expr::Index(_, _) => {
                    *index += 1; // consume )
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
        // Skip any line terminators before for-in body (do not skip semicolons: they can be empty-statement body)
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        let body = parse_statement_item(t, index)?;
        let body_stmts = match *body.kind {
            StatementKind::Block(b) => b,
            _ => vec![body],
        };

        // If the LHS was a destructuring pattern (array/object) with a declaration
        // (e.g., `for (var [a, b] in obj) ...`), return a destructuring ForIn variant
        if let Some(pattern) = for_of_pattern {
            match pattern {
                ForOfPattern::Object(destr_pattern) => {
                    // Convert Vec<DestructuringElement> -> Vec<ObjectDestructuringElement>
                    let mut obj_pattern: Vec<ObjectDestructuringElement> = Vec::new();
                    for elem in destr_pattern.into_iter() {
                        match elem {
                            DestructuringElement::Property(key, boxed) => {
                                obj_pattern.push(ObjectDestructuringElement::Property { key, value: *boxed });
                            }
                            DestructuringElement::Rest(name) => {
                                obj_pattern.push(ObjectDestructuringElement::Rest(name));
                            }
                            _ => return Err(raise_parse_error!("Invalid element in object destructuring pattern", line, column)),
                        }
                    }

                    return Ok(Statement {
                        kind: Box::new(StatementKind::ForInDestructuringObject(
                            match decl_kind {
                                Some(Token::Var) => Some(crate::core::VarDeclKind::Var),
                                Some(Token::Let) => Some(crate::core::VarDeclKind::Let),
                                Some(Token::Const) => Some(crate::core::VarDeclKind::Const),
                                Some(_) => return Err(raise_parse_error!("Invalid declaration kind for for-in", line, column)),
                                None => return Err(raise_parse_error!("Missing declaration kind for for-in", line, column)),
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
                                Some(_) => return Err(raise_parse_error!("Invalid declaration kind for for-in", line, column)),
                                None => return Err(raise_parse_error!("Missing declaration kind for for-in", line, column)),
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

        // Assignment-form for-in: allow expressions like `for (x in obj)` or `for (a.b in obj)`
        if init_decls.is_none()
            && let Some(expr) = init_expr
        {
            match expr {
                Expr::Property(_, _) | Expr::Index(_, _) | Expr::Var(_, _, _) => {
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
                    Some(_) => return Err(raise_parse_error!("Invalid declaration kind for for-in", line, column)),
                    None => return Err(raise_parse_error!("Missing declaration kind for for-in", line, column)),
                },
                var_name,
                rhs,
                body_stmts,
            )),
            line,
            column,
        });
    }

    // Standard for loop
    // Skip line terminators between the init and the first semicolon
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::Semicolon) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume ;

    // Skip line terminators before test expression or semicolon
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let test = if !matches!(t[*index].token, Token::Semicolon) {
        Some(parse_expression(t, index)?)
    } else {
        None
    };

    // Skip line terminators before the second semicolon
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::Semicolon) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume ;

    // Skip line terminators before update expression or closing paren
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let update = if !matches!(t[*index].token, Token::RParen) {
        Some(parse_expression(t, index)?)
    } else {
        None
    };

    // Skip line terminators before the closing ')'
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume )

    // Skip any line terminators before for loop body (do not skip semicolons: they can be empty-statement body)
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let body = parse_statement_item(t, index)?;
    let body_stmts = match *body.kind {
        StatementKind::Block(b) => b,
        _ => vec![body],
    };

    let init_stmt = if is_decl {
        let decls = match init_decls {
            Some(d) => d,
            None => return Err(raise_parse_error!("Missing declarations in for-init", line, column)),
        };
        let k = match decl_kind {
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
    // Handle both `function*` (single token) and `function *` (two tokens)
    if matches!(t[*index].token, Token::Function) {
        // If next token is '*' treat as generator and consume both
        if *index + 1 < t.len() && matches!(t[*index + 1].token, Token::Multiply) {
            is_generator = true;
            *index += 2; // consume 'function' and '*'
        } else {
            *index += 1; // consume 'function'
        }
    } else {
        // consume single Token::FunctionStar
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

    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume (

    let params = parse_parameters(t, index)?;

    // Skip any line terminators before the function body opening brace
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if !matches!(t[*index].token, Token::LBrace) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume {

    let body = if is_async {
        push_await_context();
        let b = parse_statement_block(t, index)?;
        pop_await_context();
        b
    } else {
        parse_statement_block(t, index)?
    };

    Ok(Statement {
        kind: Box::new(StatementKind::FunctionDeclaration(name, params, body, is_generator, is_async)),
        line: t[start].line,
        column: t[start].column,
    })
}

fn parse_if_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1; // consume if
    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume (
    let condition = parse_expression(t, index)?;
    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume )

    // Skip any line terminators before a single-statement body
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    let then_stmt = parse_statement_item(t, index)?;
    let then_block = match *then_stmt.kind {
        StatementKind::Block(stmts) => stmts,
        _ => vec![then_stmt],
    };

    // Skip any semicolons or line terminators before an 'else' token so it binds
    // to the nearest if (handles constructs like `if (a)\nelse ...`).
    while *index < t.len() && matches!(t[*index].token, Token::Semicolon | Token::LineTerminator) {
        *index += 1;
    }

    let else_block = if *index < t.len() && matches!(t[*index].token, Token::Else) {
        *index += 1;
        // Skip any line terminators before else body
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
    *index += 1; // consume return
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
    *index += 1; // consume while
    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume (
    let condition = parse_expression(t, index)?;
    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume )

    // Skip any line terminators before while body (do not skip semicolons: they can be empty-statement body)
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    // If the next token is a semicolon, that's an empty-statement body
    let body_stmts = if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1; // consume the semicolon
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
    *index += 1; // consume do
    // Skip any line terminators before do body (do not skip semicolons: they can be empty-statement body)
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    // If the next token is a semicolon, that's an empty-statement body
    log::trace!("parse_do_while: at index {} token={:?}", *index, t.get(*index));
    let body_stmts = if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        log::trace!("parse_do_while: found semicolon empty body at index {}", *index);
        *index += 1; // consume the semicolon
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

    // Skip any line terminators between do-body and the 'while' keyword
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }

    if !matches!(t[*index].token, Token::While) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume while

    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume (

    let condition = parse_expression(t, index)?;

    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume )

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
    *index += 1; // consume switch

    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume (
    let expr = parse_expression(t, index)?;

    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume )

    if !matches!(t[*index].token, Token::LBrace) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume {

    let mut cases: Vec<crate::core::SwitchCase> = Vec::new();

    while *index < t.len() && !matches!(t[*index].token, Token::RBrace) {
        if matches!(t[*index].token, Token::Case) {
            *index += 1; // consume case
            let case_expr = parse_expression(t, index)?;
            if !matches!(t[*index].token, Token::Colon) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1; // consume colon

            // collect statements until next Case/Default/RBrace
            let mut stmts: Vec<Statement> = Vec::new();
            loop {
                // Skip stray semicolons/line terminators between statements
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
            *index += 1; // consume default
            if !matches!(t[*index].token, Token::Colon) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1; // consume colon

            let mut stmts: Vec<Statement> = Vec::new();
            loop {
                // Skip stray semicolons/line terminators between statements
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
            *index += 1; // allow stray semicolons/line terminators
        } else {
            return Err(raise_parse_error_at!(t.get(*index)));
        }
    }

    if !matches!(t[*index].token, Token::RBrace) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume }

    Ok(Statement {
        kind: Box::new(StatementKind::Switch(Box::new(SwitchStatement { expr, cases }))),
        line: t[start].line,
        column: t[start].column,
    })
}

fn parse_break_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1; // consume break

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
    *index += 1; // consume continue

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
    *index += 1; // consume 'with'
    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume (
    let obj_expr = parse_expression(t, index)?;
    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume )

    // Skip any line terminators before the statement body
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
    *index += 1; // consume throw
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
    *index += 1; // consume try

    let try_block = parse_block_statement(t, index)?;
    let try_body = if let StatementKind::Block(stmts) = *try_block.kind {
        stmts
    } else {
        return Err(raise_parse_error!("Expected block after try", t[start].line, t[start].column));
    };

    // Skip any line terminators before catch/finally
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }

    let mut catch_param = None;
    let mut catch_body = None;

    if *index < t.len() && matches!(t[*index].token, Token::Catch) {
        *index += 1; // consume catch

        // Optional catch binding
        if *index < t.len() && matches!(t[*index].token, Token::LParen) {
            *index += 1; // consume (
            if *index < t.len() {
                match &t[*index].token {
                    Token::Identifier(name) => {
                        catch_param = Some(CatchParamPattern::Identifier(name.clone()));
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
            *index += 1; // consume )
        }

        let catch_block = parse_block_statement(t, index)?;
        if let StatementKind::Block(stmts) = *catch_block.kind {
            catch_body = Some(stmts);
        } else {
            return Err(raise_parse_error_with_token!(t.get(*index).unwrap(), "Expected block after catch"));
        }
    }

    let mut finally_body = None;
    // Skip any line terminators before finally (may follow a catch block)
    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < t.len() && matches!(t[*index].token, Token::Finally) {
        *index += 1; // consume finally
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
    *index += 1; // consume {
    let body = parse_statements(t, index)?;
    if *index >= t.len() || !matches!(t[*index].token, Token::RBrace) {
        return Err(raise_parse_error_at!(t.get(*index)));
    }
    *index += 1; // consume }
    Ok(Statement {
        kind: Box::new(StatementKind::Block(body)),
        line: t[start].line,
        column: t[start].column,
    })
}

fn parse_var_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1; // consume var

    // Support array/object destructuring in variable declarations
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

            // Convert Vec<DestructuringElement> -> Vec<ObjectDestructuringElement>
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

    // Fallback to simple identifier declarations
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
    *index += 1; // consume let

    // Support array/object destructuring in let declarations
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

            // Convert Vec<DestructuringElement> -> Vec<ObjectDestructuringElement>
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

    // Fallback to simple identifier declarations
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
    *index += 1; // consume const

    // Support array/object destructuring in const declarations
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

            // Convert Vec<DestructuringElement> -> Vec<ObjectDestructuringElement>
            let mut obj_pattern: Vec<ObjectDestructuringElement> = Vec::new();
            for elem in pattern.into_iter() {
                match elem {
                    DestructuringElement::Property(key, boxed) => {
                        obj_pattern.push(ObjectDestructuringElement::Property { key, value: *boxed });
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

    // Fallback to simple identifier declarations (must have initializer for const)
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

fn parse_import_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1; // consume import

    let mut specifiers = Vec::new();
    let mut source = String::new();

    // import "module-name";
    if let Token::StringLit(s) = &t[*index].token {
        source = utf16_to_utf8(s);
        *index += 1;
    } else {
        // import { ... } from "..." or import * as name from "..." or import default from "..."

        // check for default import
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

                let imported_name = if let Some(id_name) = t[*index].token.as_identifier_string() {
                    id_name
                } else {
                    return Err(raise_parse_error!("Expected identifier in named import"));
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
                        *index += 1; // consume as
                        if let Some(alias) = t[*index].token.as_identifier_string() {
                            local_name = Some(alias);
                            *index += 1;
                        } else {
                            return Err(raise_parse_error!("Expected identifier after 'as'"));
                        }
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

    let is_with_clause = matches!(t[*index].token, Token::With) || matches!(&t[*index].token, Token::Identifier(s) if s == "with");

    if !is_with_clause {
        return Ok(());
    }

    *index += 1; // consume `with`

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
                    *index += 1; // consume final `}`
                    return Ok(());
                }
            }
            Token::EOF => return Err(raise_parse_error!("Unterminated import attributes clause")),
            _ => {}
        }
        *index += 1;
    }

    Err(raise_parse_error!("Unterminated import attributes clause"))
}

fn parse_export_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1; // consume export

    while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
        *index += 1;
    }

    let mut specifiers = Vec::new();
    let mut inner_stmt = None;
    let mut source = None;

    if *index < t.len() && matches!(t[*index].token, Token::Default) {
        *index += 1; // consume default
        // export default expression;
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
        // export * from "module";
        // export * as name from "module";
        *index += 1; // consume '*'
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
            *index += 1; // consume 'as'
            let name = if *index < t.len() {
                if let Some(id_name) = t[*index].token.as_identifier_string() {
                    *index += 1;
                    id_name
                } else {
                    return Err(raise_parse_error!("Expected identifier after 'as' in export statement"));
                }
            } else {
                return Err(raise_parse_error!("Expected identifier after 'as' in export statement"));
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
        *index += 1; // consume {
        loop {
            while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                *index += 1;
            }

            if *index < t.len() && matches!(t[*index].token, Token::RBrace) {
                *index += 1;
                break;
            }

            let name = if let Some(id_name) = t[*index].token.as_identifier_string() {
                id_name
            } else {
                return Err(raise_parse_error!("Expected identifier in export specifier"));
            };
            *index += 1;

            let mut alias = None;
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
                        } else {
                            return Err(raise_parse_error!("Expected identifier after as"));
                        }
                    } else {
                        return Err(raise_parse_error!("Expected identifier after as"));
                    }
                }
            }

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

        consume_import_attributes_clause(t, index)?;

        if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
            *index += 1;
        }
    } else {
        // export var ... or export function ...
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

fn parse_variable_declaration_list(t: &[TokenData], index: &mut usize) -> Result<Vec<(String, Option<Expr>)>, JSError> {
    let mut decls = Vec::new();
    loop {
        // Skip LineTerminators
        while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
            *index += 1;
        }

        // Accept plain identifier names (including contextual keywords like `await` and `async`)
        match &t[*index].token {
            Token::Identifier(name) => {
                let name = name.clone();
                *index += 1;
                // Allow line terminators between the identifier and an optional initializer
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
                // Skip line terminators before checking for a comma separating declarations
                while *index < t.len() && matches!(t[*index].token, Token::LineTerminator) {
                    *index += 1;
                }
            }
            Token::Await => {
                // Treat `await` as an IdentifierName in declaration positions for non-module/script parsing
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
                // Accept 'async' as an identifier name in variable declarations
                // when it is not acting as the async keyword (e.g., `var async = 1;`).
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
                // Accept 'static' as an identifier name (e.g., `var static;`) in contexts
                // where an IdentifierName is expected.
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
    // Allow line terminators inside expressions (e.g., after a binary operator
    // at the end of a line). Tokenizer emits `LineTerminator` for newlines —
    // when parsing an expression we should treat those as insignificant
    // whitespace and skip them so expressions that span lines parse correctly.
    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    log::trace!(
        "parse_full_expression: tokens after initial skip (first 8): {:?}",
        tokens.iter().take(8).collect::<Vec<_>>()
    );

    // Pre-check for parenthesized arrow form `( ... ) =>` to correctly parse arrow functions
    // before falling back to normal assignment parsing which may misinterpret `()` as grouping.
    if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
        // Find matching closing paren
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
                // Debug: show j/next and surrounding tokens
                log::trace!(
                    "parse_full_expr paren-scan: index={}, j={} token_j={:?} next={} token_next={:?}",
                    *index,
                    j,
                    tokens.get(j),
                    next,
                    tokens.get(next)
                );
                // Attempt to parse params between '(' and matching ')'
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
                            *index = next + 1; // consume past '=>'
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

// Helper: Generic binary operator parser for left-associative operators
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

// Helper: Flatten nested Comma expressions into a vector of expressions.
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
    // Allow optional leading line terminators before the first parameter
    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
        *index += 1;
    }

    if *index < tokens.len() && !matches!(tokens[*index].token, Token::RParen) {
        loop {
            if matches!(tokens[*index].token, Token::Spread) {
                // Handle rest parameter: ...args
                *index += 1; // consume ...
                if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
                    *index += 1;
                    log::trace!("parse_parameters: found rest parameter name={}", name);
                    params.push(DestructuringElement::Rest(name));

                    if *index >= tokens.len() {
                        return Err(raise_parse_error!("Unexpected end of parameters after rest"));
                    }
                    // Skip optional line terminators after rest identifier
                    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                        *index += 1;
                    }
                    // Rest parameter must be the last one
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
                // Support default initializers: identifier '=' expression
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
            if matches!(tokens[*index].token, Token::RParen) {
                break;
            }
            if !matches!(tokens[*index].token, Token::Comma) {
                return Err(raise_parse_error_with_token!(tokens[*index], "Expected ',' in parameter list"));
            }
            *index += 1; // consume ,
            // Allow trailing comma before ')' and optional line terminators after the comma
            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                *index += 1;
            }
            if *index < tokens.len() && matches!(tokens[*index].token, Token::RParen) {
                // Trailing comma present before the closing paren
                break;
            }
        }
    }
    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
        return Err(raise_parse_error_at!(tokens.get(*index)));
    }
    *index += 1; // consume )
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
    *index += 1; // consume }
    Ok(body)
}

pub fn parse_expression(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    // Allow line terminators inside expressions (e.g., after a binary operator
    // at the end of a line). Tokenizer emits `LineTerminator` for newlines —
    // when parsing an expression we should treat those as insignificant
    // whitespace and skip them so expressions that span lines parse correctly.
    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    log::trace!("parse_expression: entry index={} token_at_index={:?}", *index, tokens.get(*index));
    let mut left = parse_full_expression(tokens, index)?;
    while *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
        *index += 1; // consume ,
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
        *index += 1; // consume ?
        let true_expr = parse_conditional(tokens, index)?; // Allow nesting
        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::Colon) {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
        *index += 1; // consume :
        let false_expr = parse_conditional(tokens, index)?; // Allow nesting
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
    *index += 1; // consume [

    let mut elements: Vec<Option<Expr>> = Vec::new();
    if *index < tokens.len() && matches!(tokens[*index].token, Token::RBracket) {
        *index += 1; // consume ]
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
            *index += 1; // consume ]
            break;
        }
        if matches!(tokens[*index].token, Token::Comma) {
            elements.push(None);
            *index += 1;
            continue;
        }

        if matches!(tokens[*index].token, Token::Spread) {
            *index += 1; // consume ...
            let rest_expr = if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                let inner = parse_array_assignment_pattern(tokens, index)?;
                Expr::Array(inner)
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
                let inner = parse_object_assignment_pattern(tokens, index)?;
                Expr::Object(inner)
            } else {
                parse_assignment(tokens, index)?
            };
            elements.push(Some(Expr::Spread(Box::new(rest_expr))));

            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            *index += 1; // consume ]
            break;
        }

        let mut elem_expr = if matches!(tokens[*index].token, Token::LBracket) {
            let saved = *index;
            let inner = parse_array_assignment_pattern(tokens, index)?;
            if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket | Token::Dot) {
                *index = saved;
                parse_assignment(tokens, index)?
            } else {
                Expr::Array(inner)
            }
        } else if matches!(tokens[*index].token, Token::LBrace) {
            let saved = *index;
            let inner = parse_object_assignment_pattern(tokens, index)?;
            if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket | Token::Dot) {
                *index = saved;
                parse_assignment(tokens, index)?
            } else {
                Expr::Object(inner)
            }
        } else {
            parse_assignment(tokens, index)?
        };

        if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
            *index += 1; // consume '='
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
            *index += 1; // consume ]
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
    *index += 1; // consume {

    let mut properties: Vec<(Expr, Expr, bool, bool)> = Vec::new();
    if *index < tokens.len() && matches!(tokens[*index].token, Token::RBrace) {
        *index += 1; // consume }
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
            *index += 1; // consume }
            break;
        }

        if matches!(tokens[*index].token, Token::Spread) {
            *index += 1; // consume ...
            let rest_expr = if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                let inner = parse_array_assignment_pattern(tokens, index)?;
                Expr::Array(inner)
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
                let inner = parse_object_assignment_pattern(tokens, index)?;
                Expr::Object(inner)
            } else {
                parse_assignment(tokens, index)?
            };
            properties.push((Expr::StringLit(Vec::new()), rest_expr, true, false));

            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            *index += 1; // consume }
            break;
        }

        let mut key_name: Option<String> = None;
        let mut key_expr: Option<Expr> = None;
        let mut is_identifier_key = false;

        if matches!(tokens[*index].token, Token::LBracket) {
            *index += 1; // consume [
            let expr = parse_assignment(tokens, index)?;
            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            *index += 1; // consume ]
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
            *index += 1; // consume :
            let mut value_expr = if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                let saved = *index;
                let inner = parse_array_assignment_pattern(tokens, index)?;
                if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket | Token::Dot) {
                    *index = saved;
                    parse_assignment(tokens, index)?
                } else {
                    Expr::Array(inner)
                }
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
                let saved = *index;
                let inner = parse_object_assignment_pattern(tokens, index)?;
                if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket | Token::Dot) {
                    *index = saved;
                    parse_assignment(tokens, index)?
                } else {
                    Expr::Object(inner)
                }
            } else {
                parse_assignment(tokens, index)?
            };

            if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                *index += 1; // consume '='
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
                *index += 1; // consume '='
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
            *index += 1; // consume }
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

// Operator precedence parsing chain (primary -> exponentiation -> multiplicative
// -> additive -> shift -> relational -> equality -> bitwise-and -> xor -> or
// -> logical-and -> logical-or -> nullish -> conditional -> assignment).

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
    // Right-associative exponentiation operator: a ** b ** c -> a ** (b ** c)
    let left = parse_primary(tokens, index, true)?;
    if *index >= tokens.len() {
        return Ok(left);
    }
    if matches!(tokens[*index].token, Token::Exponent) {
        *index += 1;
        let right = parse_exponentiation(tokens, index)?; // right-associative
        Ok(Expr::Binary(Box::new(left), BinaryOp::Pow, Box::new(right)))
    } else {
        Ok(left)
    }
}

thread_local! {
    // Track a per-thread depth so parallel test runs do not interfere with each
    // other. The parser will increment this while parsing a class body and
    // decrement it when leaving so that checks for private identifier usage
    // (`obj.#x`) can be enforced per-parse without global races.
    static PARSING_CLASS_DEPTH: Cell<usize> = const { Cell::new(0) };

    // Stack of declared private-name sets for nested class parsing contexts.
    // Each entry is an `Rc<RefCell<HashSet<String>>>` containing the names
    // declared in that class. We push when entering a class body and pop
    // when leaving so that inner parsing (e.g. method bodies) can validate
    // private name usage against the current class's declarations.
    static PRIVATE_NAME_STACK: RefCell<Vec<Rc<RefCell<HashSet<String>>>>> = const { RefCell::new(Vec::new()) };
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
    *index += 1; // consume {

    let mut members = Vec::new();
    let mut declared_private_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    let current_private_names = std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashSet::new()));
    let _private_guard = ClassPrivateNamesGuard::new(current_private_names.clone());

    // Pre-scan logic omitted/simplified for now to ensure compilation, or we try to port it.
    // Porting the pre-scan loop:
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

            // Accessor get/set
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
                // Skip params
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
                // Skip body
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

            // Private Identifier Member
            if let Some(Token::PrivateIdentifier(name)) = t.get(pos).map(|tok| &tok.token) {
                current_private_names.borrow_mut().insert(name.clone());
                pos += 1;
                // Method?
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
                // Property
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

            // Regular identifier member
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

    // Actual Parsing Loop
    while *index < t.len() && !matches!(t[*index].token, Token::RBrace) {
        while *index < t.len() && matches!(t[*index].token, Token::Semicolon | Token::LineTerminator) {
            *index += 1;
        }
        if *index >= t.len() || matches!(t[*index].token, Token::RBrace) {
            break;
        }

        let is_static = if *index < t.len() && matches!(t[*index].token, Token::Static) {
            // Check if it is static block or static member
            // If next is LBrace, it is static block.
            // If next is identifier/get/set, it is static member.
            // BUT 'static' can be a method name too! `static() {}`
            // Lookahead
            if let Some(next) = t.get(*index + 1) {
                if matches!(next.token, Token::LBrace) {
                    *index += 1;
                    true
                } else if matches!(next.token, Token::LParen) {
                    // static() {} -> method named static
                    false
                } else if matches!(next.token, Token::Assign) {
                    // static = 1 -> property named static
                    false
                } else if matches!(next.token, Token::Semicolon | Token::LineTerminator) {
                    // static; -> property named static
                    false
                } else {
                    // static x ...
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
            *index += 1; // consume {
            let body = parse_statement_block(t, index)?;
            members.push(ClassMember::StaticBlock(body));
            continue;
        }

        // Accessor check
        let mut is_accessor = false;
        let mut is_getter = false;
        if let Some(Token::Identifier(kw)) = t.get(*index).map(|d| &d.token)
            && (kw == "get" || kw == "set")
        {
            // Diagnostic trace for accessor detection
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

            // Check if it is used as keyword or name
            // get x() {} -> keyword
            // get() {} -> method name 'get'
            // Also allow computed accessor: get [expr]() {}
            if let Some(next) = t.get(*index + 1) {
                // Allow identifiers, private identifiers, strings, numbers, and computed keys immediately
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
                    // Also accept keywords as property names when used with get/set (e.g., `get return() {}`)
                    // but avoid misclassifying standalone 'get() {}' as accessor keyword usage.
                    if !matches!(next.token, Token::LParen) && next.token.as_identifier_string().is_some() {
                        is_accessor = true;
                        is_getter = kw == "get";
                        log::trace!("parse_primary: accessor recognized for keyword-name (kw={}) at idx={}", kw, *index);
                    }
                }
            }
        }

        if is_accessor {
            *index += 1; // consume get/set

            // Support computed accessor names: get [expr]() {} or get id() {}
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
                    prop_expr_opt = Some(Expr::Number(*n));
                    *index += 1;
                }
                Token::BigInt(s) => {
                    prop_expr_opt = Some(Expr::BigInt(crate::unicode::utf8_to_utf16(s)));
                    *index += 1;
                }
                Token::PrivateIdentifier(name) => {
                    prop_name_str = Some(name.clone());
                    is_private = true;
                    *index += 1;
                }
                Token::LBracket => {
                    *index += 1; // consume [
                    let expr = parse_assignment(t, index)?;
                    if *index >= t.len() || !matches!(t[*index].token, Token::RBracket) {
                        return Err(raise_parse_error_at!(t.get(*index)));
                    }
                    *index += 1; // consume ]
                    prop_expr_opt = Some(expr);
                }
                _ => return Err(raise_parse_error_at!(t.get(*index))),
            }

            if *index >= t.len() || !matches!(t[*index].token, Token::LParen) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1; // consume (
            let params = parse_parameters(t, index)?;
            if *index >= t.len() || !matches!(t[*index].token, Token::LBrace) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1; // consume {
            let body = parse_statement_block(t, index)?;

            if is_getter {
                if let Some(prop_expr) = prop_expr_opt {
                    // computed getter
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
                // setter
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

        // Method or Property (support computed names via [expr])
        // Optional 'async' indicates async method; optional '*' indicates a generator method (can appear before an identifier or before a computed '[' key)
        let mut is_async_member = false;
        if *index < t.len() && matches!(t[*index].token, Token::Async) {
            // treat 'async' as keyword in class member position (assuming it precedes a method)
            is_async_member = true;
            *index += 1; // consume 'async'
        }
        let mut is_generator = false;
        if *index < t.len() && matches!(t[*index].token, Token::Multiply) {
            is_generator = true;
            log::debug!("parse_class_member: saw '*' token at index {}", *index);
            *index += 1; // consume '*'
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
                computed_key_expr = Some(Expr::Number(*n));
                *index += 1;
            }
            Token::BigInt(s) => {
                computed_key_expr = Some(Expr::BigInt(crate::unicode::utf8_to_utf16(s)));
                *index += 1;
            }
            Token::LBracket => {
                *index += 1; // consume [
                let expr = parse_assignment(t, index)?;
                if *index >= t.len() || !matches!(t[*index].token, Token::RBracket) {
                    return Err(raise_parse_error_at!(t.get(*index)));
                }
                *index += 1; // consume ]
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

        // Check duplicate private
        if let Some(ref name) = name_str_opt {
            if is_private {
                if declared_private_names.contains(name) {
                    let msg = format!("Duplicate private name: #{}", name);
                    return Err(raise_parse_error_with_token!(&t[*index], msg));
                }
                declared_private_names.insert(name.clone());
                current_private_names.borrow_mut().insert(name.clone());
            }
            *index += 1; // consume name
        }

        // If this is an identifier constructor, handle specially (computed-constructor not special)
        if computed_key_expr.is_none()
            && !is_static
            && !is_private
            && name_str_opt.as_deref() == Some("constructor")
            && matches!(t.get(*index).map(|d| &d.token), Some(Token::LParen))
        {
            *index += 1; // (
            let params = parse_parameters(t, index)?;
            if *index >= t.len() || !matches!(t[*index].token, Token::LBrace) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1; // {
            let body = parse_statement_block(t, index)?;
            members.push(ClassMember::Constructor(params, body));
            continue;
        }

        if *index < t.len() && matches!(t[*index].token, Token::LParen) {
            // Method
            *index += 1; // (
            let params = parse_parameters(t, index)?;
            if *index >= t.len() || !matches!(t[*index].token, Token::LBrace) {
                return Err(raise_parse_error_at!(t.get(*index)));
            }
            *index += 1; // {
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
            // Property
            *index += 1; // =
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
            // Property without initializer
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
    *index += 1; // consume }
    Ok(members)
}

fn parse_primary(tokens: &[TokenData], index: &mut usize, allow_call: bool) -> Result<Expr, JSError> {
    // Skip any leading line terminators inside expressions so multi-line
    // expression continuations like `a +\n b` parse correctly.
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
            // Deleting a private field is a SyntaxError (e.g., `delete this.#priv`)
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
            // `await` is a contextual keyword. In non-async/script contexts it can be used as an identifier.
            // If the next token is an assignment operator, treat this as an identifier reference.
            // Otherwise, only parse an await expression if we are currently inside an async context and
            // the following token can start an expression; otherwise treat as an identifier reference.
            if *index < tokens.len() {
                if matches!(tokens[*index].token, Token::Assign) {
                    Expr::Var("await".to_string(), Some(token_data.line), Some(token_data.column))
                } else if in_await_context() {
                    match &tokens[*index].token {
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
                        | Token::Regex(_, _) => {
                            let inner = parse_primary(tokens, index, true)?;
                            Expr::Await(Box::new(inner))
                        }
                        _ => Expr::Var("await".to_string(), Some(token_data.line), Some(token_data.column)),
                    }
                } else {
                    Expr::Var("await".to_string(), Some(token_data.line), Some(token_data.column))
                }
            } else {
                Expr::Var("await".to_string(), Some(token_data.line), Some(token_data.column))
            }
        }
        Token::Yield => {
            // Handle `yield *` (with optional whitespace) as YieldStar.
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Multiply) {
                *index += 1; // consume '*'
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
                // `yield` with no expression
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
            // Class Expression
            // class [Identifier] [extends Expression] { ClassBody }
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
                    _ => "".to_string(), // Anonymous class expression
                }
            } else {
                "".to_string()
            };

            let extends = if *index < tokens.len() && matches!(tokens[*index].token, Token::Extends) {
                *index += 1; // consume extends
                Some(parse_expression(tokens, index)?)
            } else {
                None
            };

            let members = parse_class_body(tokens, index)?;

            let class_def = crate::core::ClassDefinition { name, extends, members };
            Expr::Class(Box::new(class_def))
        }
        Token::New => {
            // Special-case: `new.target` meta-property (no arguments, just 'new.target')
            // Recognize `new` followed by optional line terminators, a '.' and identifier 'target'
            // Debug: print nearby tokens when encountering 'new' to diagnose parsing issues
            {
                let mut s = String::new();
                for i in 0..5 {
                    if *index + i < tokens.len() {
                        s.push_str(&format!("{:?} ", tokens[*index + i].token));
                    }
                }
                log::trace!("DEBUG-PARSER-New-lookahead: {}", s);
            }
            // Lookahead: parse_primary has already advanced *index to the token after 'new',
            // so start at *index (which may be '.' or line-terminators) rather than *index + 1.
            let mut look = *index;
            // skip intervening line terminators between 'new' and '.' per spacing rules
            while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
                look += 1;
            }
            // Determine whether the `new` token is actually the `new.target` meta-property
            // (e.g., `new.target?.a`). If so, consume the tokens and return `Expr::NewTarget` as
            // the primary expression so the normal postfix loop may attach optional chains.
            let is_new_target = if look < tokens.len() && matches!(tokens[look].token, Token::Dot) {
                look += 1; // skip '.'
                while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
                    look += 1;
                }
                if look < tokens.len()
                    && let Token::Identifier(id) = &tokens[look].token
                    && id == "target"
                {
                    // consume up through identifier (look points at identifier)
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

                // Check for arguments
                let args = if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                    *index += 1; // consume '('
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
                            *index += 1; // consume ','
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
                    *index += 1; // consume ')'
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
            // Parse the inner expression as an AssignmentExpression so that
            // constructs like `...target = source` are parsed as `Spread(Assign(...))`
            // rather than `Assign(Spread(...))` which would make `Spread` the
            // assignment target (invalid for assignment).
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
                        // For a single substitution, TemplateString semantics use ToString on the
                        // expression (hint 'string'), not the default addition coercion.
                        // Use an explicit call to String(e) to get proper ToString behavior.
                        Expr::Call(Box::new(Expr::Var("String".to_string(), None, None)), vec![e])
                    }
                }
            } else {
                // Build binary addition chain
                let mut expr = match &parts[0] {
                    TemplatePart::String(cooked_opt, _raw) => {
                        let cooked = cooked_opt.clone().ok_or_else(|| raise_parse_error_at!(tokens.get(*index - 1)))?;
                        Expr::StringLit(cooked)
                    }
                    TemplatePart::Expr(expr_tokens) => {
                        let expr_tokens = expr_tokens.clone();
                        let e = parse_expression(&expr_tokens, &mut 0)?;
                        // Force string context by prepending "" + e
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
                            // Each substitution in a template literal should be ToString(value)
                            // which corresponds to calling String(e) (hint 'string').
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
        Token::PrivateIdentifier(name) => {
            // Represent a standalone private name so it can be evaluated to a Value::PrivateName
            // for use in contexts like `#name in obj`.
            Expr::PrivateName(name.clone())
        }
        Token::Import => {
            if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                // Dynamic import
                *index += 1; // consume '('
                let arg = parse_assignment(tokens, index)?;
                // Optional second argument (e.g. import attributes/options).
                let mut options_arg: Option<Box<Expr>> = None;
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
                    *index += 1; // consume ','
                    // Allow trailing comma: import(expr,)
                    if !(*index < tokens.len() && matches!(tokens[*index].token, Token::RParen)) {
                        let opt = parse_assignment(tokens, index)?;
                        options_arg = Some(Box::new(opt));
                        // Allow optional trailing comma after second arg: import(expr, opt,)
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
                            *index += 1; // consume trailing ','
                        }
                    }
                }
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                    return Err(raise_parse_error!("Expected ')' after import(...)"));
                }
                *index += 1; // consume ')'
                Expr::DynamicImport(Box::new(arg), options_arg)
            } else {
                Expr::Var("import".to_string(), Some(token_data.line), Some(token_data.column))
            }
        }
        Token::Regex(pattern, flags) => Expr::Regex(pattern.clone(), flags.clone()),
        Token::This => Expr::This,
        Token::Super => {
            // Check if followed by ( for super() call
            if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                *index += 1; // consume '('
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
                        *index += 1; // consume ','
                    }
                }
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1; // consume ')'
                Expr::SuperCall(args)
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Dot) {
                *index += 1; // consume '.'
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::Identifier(_)) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1;
                let prop = if let Token::Identifier(name) = &tokens[*index - 1].token {
                    name.clone()
                } else {
                    return Err(raise_parse_error_at!(tokens.get(*index - 1)));
                };
                // Check if followed by ( for method call
                if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                    *index += 1; // consume '('
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
                            *index += 1; // consume ','
                            // permit trailing comma before ) and skip newlines
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
                    *index += 1; // consume ')'
                    // Flatten accidental single-Comma argument
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
            } else {
                Expr::Super
            }
        }
        Token::LBrace => {
            // Parse object literal
            // Skip any leading line terminators inside the object literal so
            // properties spread across multiple lines parse correctly.
            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                *index += 1;
            }
            let mut properties = Vec::new();
            if *index < tokens.len() && matches!(tokens[*index].token, Token::RBrace) {
                // Empty object {}
                *index += 1; // consume }
            } else {
                loop {
                    log::trace!(
                        "parse_primary: object literal loop; next tokens (first 8): {:?}",
                        tokens.iter().take(8).collect::<Vec<_>>()
                    );
                    // Skip blank lines that may appear between properties.
                    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator | Token::Semicolon) {
                        *index += 1;
                    }

                    // If we hit the closing brace after skipping blank lines,
                    // consume it and finish the object literal. This handles
                    // trailing commas followed by whitespace/newlines before `}`.
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::RBrace) {
                        *index += 1; // consume }
                        break;
                    }
                    if *index >= tokens.len() {
                        return Err(raise_parse_error_at!(tokens.last()));
                    }
                    // Check for spread
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Spread) {
                        log::trace!(
                            "parse_primary: object property is spread; next tokens (first 8): {:?}",
                            tokens.iter().take(8).collect::<Vec<_>>()
                        );
                        *index += 1; // consume ...
                        // Use parse_assignment here so a spread is a single expression
                        // and doesn't accidentally capture following comma-separated
                        // properties via the comma operator.
                        let expr = parse_assignment(tokens, index)?;
                        // Use empty string as key for spread
                        properties.push((Expr::StringLit(Vec::new()), Expr::Spread(Box::new(expr)), false, false));
                    } else {
                        // Check for getter/setter: only treat as getter/setter if the
                        // identifier 'get'/'set' is followed by a property key and
                        // an opening parenthesis (no colon). This avoids confusing a
                        // regular property named 'get'/'set' (e.g. `set: function(...)`) with
                        // the getter/setter syntax.
                        // Recognize getter/setter signatures including computed keys
                        log::trace!(
                            "parse_primary: object literal accessor check at idx {} tok={:?} next={:?}",
                            *index,
                            tokens.get(*index).map(|t| &t.token),
                            tokens.get(*index + 1).map(|t| &t.token)
                        );
                        let is_getter =
                            if tokens.len() > *index + 1 && matches!(tokens[*index].token, Token::Identifier(ref id) if id == "get") {
                                if matches!(tokens[*index + 1].token, Token::Identifier(_) | Token::StringLit(_))
                                    || tokens[*index + 1].token.as_identifier_string().is_some()
                                {
                                    tokens.len() > *index + 2 && matches!(tokens[*index + 2].token, Token::LParen)
                                } else if matches!(tokens[*index + 1].token, Token::LBracket) {
                                    // find matching RBracket and ensure '(' follows
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
                            if tokens.len() > *index + 1 && matches!(tokens[*index].token, Token::Identifier(ref id) if id == "set") {
                                if matches!(tokens[*index + 1].token, Token::Identifier(_) | Token::StringLit(_))
                                    || tokens[*index + 1].token.as_identifier_string().is_some()
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
                            *index += 1; // consume get/set
                        }

                        // Parse key
                        let mut is_shorthand_candidate = false;
                        // Track whether the property name was a computed name (e.g. `[expr]`)
                        let mut key_is_computed = false;
                        // Optional 'async' keyword indicates async concise method
                        let mut is_async_member = false;
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::Async) {
                            is_async_member = true;
                            *index += 1; // consume 'async'
                        }
                        // Optional '*' indicates generator concise method
                        let mut is_generator = false;
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::Multiply) {
                            is_generator = true;
                            *index += 1; // consume '*'
                        }

                        // Special-case: if we saw '*' and the tokenizer emitted a Yield token
                        // where an IdentifierName 'yield' would be expected (e.g. `*yield()`),
                        // treat that Yield token as the identifier name "yield" and parse the
                        // concise generator method accordingly. This keeps changes local and
                        // avoids reworking the surrounding logic.
                        if is_generator && *index < tokens.len() && matches!(tokens[*index].token, Token::Yield) {
                            // Only handle concise method form: name + ( ... )
                            if !is_getter && !is_setter && tokens.len() > *index + 1 && matches!(tokens[*index + 1].token, Token::LParen) {
                                *index += 1; // consume the Yield token (as name)
                                *index += 1; // consume (
                                let params = parse_parameters(tokens, index)?;
                                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                    return Err(raise_parse_error_at!(tokens.get(*index)));
                                }
                                *index += 1; // consume {
                                let body = parse_statements(tokens, index)?;
                                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                    return Err(raise_parse_error_at!(tokens.get(*index)));
                                }
                                *index += 1; // consume }
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

                                // After adding method, skip any newline/semicolons and handle comma/end in outer loop
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
                        }

                        let key_expr = if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
                            // Check for concise method: Identifier + (
                            if !is_getter && !is_setter && tokens.len() > *index + 1 && matches!(tokens[*index + 1].token, Token::LParen) {
                                // Concise method
                                *index += 1; // consume name
                                *index += 1; // consume (
                                let params = parse_parameters(tokens, index)?;
                                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                    return Err(raise_parse_error_at!(tokens.get(*index)));
                                }
                                *index += 1; // consume {
                                let body = parse_statements(tokens, index)?;
                                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                    return Err(raise_parse_error_at!(tokens.get(*index)));
                                }
                                *index += 1; // consume }
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

                                // After adding method, skip any newline/semicolons and handle comma/end in outer loop
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
                            // Numeric property keys are allowed in object literals (they become strings)
                            *index += 1;
                            // Use canonical JS string conversion to preserve formatting like '1e+55'
                            let s = crate::core::value_to_string(&crate::core::Value::Number(n));
                            Expr::StringLit(crate::unicode::utf8_to_utf16(&s))
                        } else if let Some(Token::BigInt(snum)) = tokens.get(*index).map(|t| t.token.clone()) {
                            // BigInt literal as an uncomputed property key -> use its canonical string (no 'n' suffix)
                            *index += 1;
                            Expr::StringLit(crate::unicode::utf8_to_utf16(&snum))
                        } else if let Some(Token::StringLit(s)) = tokens.get(*index).map(|t| t.token.clone()) {
                            *index += 1;
                            Expr::StringLit(s)
                        } else if let Some(tok) = tokens.get(*index).map(|t| t.token.clone()) {
                            // Allow other keywords (e.g., `return`, `export`, `import`, etc.) as
                            // property names. Token::as_identifier_string maps those tokens to
                            // their identifier string when appropriate.
                            if let Some(id) = tok.as_identifier_string() {
                                // Treat identifier-like tokens (including contextual keywords
                                // such as `await`) as shorthand candidates for property
                                // shorthand parsing (e.g. `{ await }`).
                                is_shorthand_candidate = true;
                                *index += 1;
                                Expr::StringLit(crate::unicode::utf8_to_utf16(&id))
                            } else if let Some(Token::Default) = tokens.get(*index).map(|t| t.token.clone()) {
                                // allow the reserved word `default` as an object property key
                                *index += 1;
                                Expr::StringLit(crate::unicode::utf8_to_utf16("default"))
                            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                                // Computed key (e.g., get [Symbol.toPrimitive]())
                                key_is_computed = true;
                                *index += 1; // consume [
                                let expr = parse_assignment(tokens, index)?;
                                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                                    return Err(raise_parse_error_at!(tokens.get(*index)));
                                }
                                *index += 1; // consume ]
                                expr
                            } else {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                        } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                            // Computed key (e.g., get [Symbol.toPrimitive]())
                            key_is_computed = true;
                            *index += 1; // consume [
                            let expr = parse_assignment(tokens, index)?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1; // consume ]
                            expr
                        } else {
                            return Err(raise_parse_error_at!(tokens.get(*index)));
                        };

                        // Check for optional '*' prefix to denote generator method
                        let mut is_generator = false;
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::Multiply) {
                            is_generator = true;
                            *index += 1; // consume '*'
                        }

                        // Check for method definition after computed key
                        if !is_getter && !is_setter && *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                            *index += 1; // consume (
                            let params = parse_parameters(tokens, index)?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1; // consume {
                            let body = parse_statements(tokens, index)?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1; // consume }
                            if is_generator {
                                properties.push((key_expr, Expr::GeneratorFunction(None, params, body), key_is_computed, false));
                            } else {
                                properties.push((key_expr, Expr::Function(None, params, body), key_is_computed, false));
                            }

                            // After adding method, skip any newline/semicolons and handle comma/end in outer loop
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
                            *index += 1; // consume (
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1; // consume )
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1; // consume {
                            let body = parse_statements(tokens, index)?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1; // consume }
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
                            *index += 1; // consume (
                            let params = parse_parameters(tokens, index)?;
                            if params.len() != 1 {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1; // consume {
                            let body = parse_statements(tokens, index)?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                return Err(raise_parse_error_at!(tokens.get(*index)));
                            }
                            *index += 1; // consume }
                            properties.push((key_expr, Expr::Setter(Box::new(Expr::Function(None, params, body))), false, false));
                        } else {
                            // Regular property
                            if *index < tokens.len() && matches!(tokens[*index].token, Token::Colon) {
                                *index += 1; // consume :
                                let value = parse_assignment(tokens, index)?;
                                properties.push((key_expr, value, key_is_computed, true));
                            } else {
                                // Shorthand property { x } -> { x: x }
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

                    // Handle comma
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
                        *index += 1;
                    }
                }
            }
            Expr::Object(properties)
        }
        Token::LBracket => {
            // Parse array literal
            log::trace!("parse_primary: entering LBracket at index {}", *index);
            log::trace!(
                "parse_primary: tokens at idx-1 {:?}, idx {:?}, idx+1 {:?}",
                tokens.get(*index).map(|t| &t.token),
                tokens.get(*index).map(|t| &t.token),
                tokens.get(*index + 1).map(|t| &t.token)
            );

            // Robust empty-array detection: handle cases where *index points to
            // either the '[' or the following ']' (depending on call-site behavior).
            if *index < tokens.len()
                && matches!(tokens[*index].token, Token::RBracket)
                && *index > 0
                && matches!(tokens[*index - 1].token, Token::LBracket)
            {
                // *index currently points at ']' (e.g. caller advanced past '[' already)
                *index += 1; // consume ']'
                log::trace!("parse_primary: detected empty array (case: idx at ']') -> new idx {}", *index);
                Expr::Array(Vec::new())
            } else {
                // Otherwise, consume '[' and parse elements normally
                log::trace!(
                    "parse_primary: starting array literal; next tokens (first 12): {:?}",
                    tokens.iter().take(12).collect::<Vec<_>>()
                );
                log::trace!("parse_primary: after '[' token at index {} -> {:?}", *index, tokens.get(*index));
                let mut elements = Vec::new();
                loop {
                    // Skip leading blank lines inside array literals to avoid
                    // attempting to parse a `]` or other tokens as elements.
                    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator | Token::Semicolon) {
                        *index += 1;
                    }
                    // If next token is a closing bracket then the array is complete
                    // This handles trailing commas like `[1, 2,]` correctly — we should
                    // stop and not attempt to parse a non-existent element.
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::RBracket) {
                        *index += 1; // consume ]
                        log::trace!(
                            "parse_primary: completed array literal with {} elements; remaining tokens (first 12): {:?}",
                            elements.len(),
                            tokens.iter().take(12).collect::<Vec<_>>()
                        );
                        break;
                    }

                    log::trace!("parse_primary: array element next token: {:?}", tokens.get(*index));
                    // Support elisions (sparse arrays) where a comma without an
                    // expression indicates an empty slot, e.g. `[ , ]` or `[a,,b]`.
                    if matches!(tokens[*index].token, Token::Comma) {
                        // Push an explicit `None` element to represent the elision (hole)
                        elements.push(None);
                        *index += 1; // consume comma representing empty slot
                        // After consuming the comma, allow the loop to continue and
                        // possibly encounter another comma or the closing bracket.
                        // If the next token is RBracket we'll handle completion below.
                        continue;
                    }

                    // Parse element expression
                    let elem = parse_assignment(tokens, index)?;
                    elements.push(Some(elem));

                    // Check for comma or end. Allow intervening line terminators
                    // between elements so array items can be split across lines.
                    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator | Token::Semicolon) {
                        *index += 1;
                    }

                    if *index >= tokens.len() {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                    if matches!(tokens[*index].token, Token::RBracket) {
                        *index += 1; // consume ]
                        break;
                    } else if matches!(tokens[*index].token, Token::Comma) {
                        *index += 1; // consume ,
                    } else {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                }
                Expr::Array(elements)
            }
        }
        Token::Function | Token::FunctionStar => {
            let mut is_generator = matches!(current, Token::FunctionStar);
            // Support both `function*` (single token) and `function *` (two tokens)
            if !is_generator && *index < tokens.len() && matches!(tokens[*index].token, Token::Multiply) {
                is_generator = true;
                log::trace!("parse_primary: saw separate '*' token after 'function' - treating as generator");
                *index += 1; // consume '*'
            }
            log::trace!(
                "parse_primary: function expression, next tokens (first 8): {:?}",
                tokens.iter().take(8).collect::<Vec<_>>()
            );

            // Optional name for named function expression. Only treat a
            // following identifier as the function's name if it is followed
            // (possibly after line terminators) by a '('. This avoids
            // misinterpreting the first parameter as a name in the forgiving
            // case where the '(' may have been consumed earlier.
            let name = if *index < tokens.len() {
                match &tokens[*index].token {
                    Token::Identifier(n) => {
                        // Look ahead for next non-LineTerminator token
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
                        // Look ahead for next non-LineTerminator token (same rules as identifier)
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
            // Now expect parameter list. Be forgiving if the '(' was consumed
            // earlier; accept either an explicit '(' or start directly at an
            // identifier (first parameter) or an immediate ')' for empty params.
            // Allow line terminators between the name and the '(' as per ASI rules.
            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                *index += 1;
            }
            if *index < tokens.len()
                && (matches!(tokens[*index].token, Token::LParen) || matches!(tokens[*index].token, Token::Identifier(_)))
            {
                if matches!(tokens[*index].token, Token::LParen) {
                    *index += 1; // consume "("
                }
                log::trace!(
                    "parse_primary: about to call parse_parameters; tokens (first 8): {:?}",
                    tokens.iter().take(8).collect::<Vec<_>>()
                );
                let params = parse_parameters(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1; // consume {
                let body = parse_statements(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1; // consume }
                if is_generator {
                    log::trace!("parse_primary: constructed GeneratorFunction name={:?} params={:?}", name, params);
                    Expr::GeneratorFunction(name, params, body)
                } else {
                    log::trace!("parse_primary: constructed Function name={:?} params={:?}", name, params);
                    Expr::Function(name, params, body)
                }
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::RParen) {
                // Defensive case: treat `) {` as an empty parameter list
                *index += 1; // consume ')'
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1; // consume {
                let body = parse_statements(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1; // consume }
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
            // Use lookahead from the current index so we don't mutate *index until we've
            // determined which production this is.
            // Note: parse_primary increments *index after reading token_data, so
            // *index currently points to the token AFTER the matched token. Compute
            // `start` as the matched token index and `next` as the following token.
            let start = *index - 1;
            let next = *index;
            // Trace entry for Async token (use start/next to avoid mutated *index impacting lookups)
            log::trace!(
                "parse_primary: Token::Async start={} *index={} tokens_slice={:?}",
                start,
                *index,
                tokens.iter().skip(start).take(4).collect::<Vec<_>>()
            );
            // Async functions may be generators (async function*). Determine generator status
            let mut is_generator = false;

            // Async function expression: async function [name] ( ... ) { ... }
            if next < tokens.len() && (matches!(tokens[next].token, Token::Function) || matches!(tokens[next].token, Token::FunctionStar)) {
                log::trace!("parse_primary (async): detected 'async function' at start={} next={}", start, next);
                // Advance index to point after the 'function' or 'function*' token
                if matches!(tokens[next].token, Token::FunctionStar) {
                    is_generator = true;
                    *index = next + 1; // position after 'function*'
                } else {
                    // Function token - check if immediately followed by a '*' token
                    *index = next + 1; // position after 'function'
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Multiply) {
                        is_generator = true;
                        *index += 1; // consume '*'
                    }
                }

                // Optional name for async function expressions (same rules as normal functions)
                let name = if *index < tokens.len() {
                    if let Token::Identifier(n) = &tokens[*index].token {
                        // Look ahead for next non-LineTerminator token
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
                    *index += 1; // consume "("
                    let params = parse_parameters(tokens, index)?;
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                        log::trace!(
                            "parse_primary (async): expected '{{' after params but found {:?} at idx {}",
                            tokens.get(*index),
                            *index
                        );
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                    *index += 1; // consume {
                    push_await_context();
                    let body = parse_statements(tokens, index)?;
                    pop_await_context();
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                    *index += 1; // consume }
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
                // Async arrow function
                log::trace!("parse_primary (async): detected '(' => possible async arrow at idx {}", *index);
                *index += 1; // consume (
                // Fast attempt: try parsing a full parameter list (supports destructuring) followed by '=>'
                let saved_idx = *index;
                if let Ok(p) = parse_parameters(tokens, index) {
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                        *index += 1; // consume '=>'
                        // Parse arrow body
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
                            *index += 1; // consume '{'
                            push_await_context();
                            let body = parse_statement_block(tokens, index)?;
                            pop_await_context();
                            return Ok(Expr::AsyncArrowFunction(p, body));
                        } else {
                            let body_expr = parse_assignment(tokens, index)?;
                            return Ok(Expr::AsyncArrowFunction(
                                p,
                                vec![Statement::from(StatementKind::Return(Some(body_expr)))],
                            ));
                        }
                    } else {
                        // rollback - not an arrow
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
                    // Try to parse params
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
                    // For async arrow functions, we need to create a special async closure
                    // For now, we'll treat them as regular arrow functions but mark them as async
                    // This will need to be handled in evaluation
                    Expr::AsyncArrowFunction(params, parse_arrow_body(tokens, index)?)
                } else {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Identifier(_)) {
                // Handle `async <identifier> =>` form (async binding identifier arrow)
                // Look ahead to see if the identifier is followed (optionally separated by LineTerminator) by an Arrow token
                if let Token::Identifier(name) = &tokens[*index].token {
                    let ident_name = name.clone();
                    let mut j = *index + 1;
                    while j < tokens.len() && matches!(tokens[j].token, Token::LineTerminator) {
                        j += 1;
                    }
                    if j < tokens.len() && matches!(tokens[j].token, Token::Arrow) {
                        // consume identifier and arrow
                        *index = j + 1;
                        return Ok(Expr::AsyncArrowFunction(
                            vec![DestructuringElement::Variable(ident_name, None)],
                            parse_arrow_body(tokens, index)?,
                        ));
                    }
                }
                // Fall back to treating `async` as an identifier name when not followed by function/arrow
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
                // Treat bare 'async' as an identifier name when not followed by function/arrow
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
            // Trace entry into LParen primary branch
            log::trace!(
                "parse_primary: entered LParen branch at idx {} tokens={:?}",
                *index,
                tokens.iter().skip(*index).take(8).collect::<Vec<_>>()
            );
            // Handle case when current index points at a closing paren (')') because '(' was consumed earlier.
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
                        *index = next + 1; // consume past '=>'
                        let body = parse_arrow_body(tokens, index)?;
                        log::trace!("constructing arrow (empty paren via rcase) params=Vec::new()");
                        return Ok(Expr::ArrowFunction(Vec::new(), body));
                    } else {
                        log::trace!("paren-rcase: not arrow; token_next={:?}", tokens.get(next));
                    }
                }
            }

            // Fast-path: detect simple single-identifier arrow `(x) =>` allowing optional line terminators
            {
                // Only use fast-path if the first token inside parens is not Spread (to avoid rest params)
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
                                *index = m + 1; // consume up to after '=>'
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

            // Check if this is an arrow function: (params) => ...
            {
                // Fast-path: immediate empty parameter list `()` followed by `=>` (allowing line terminators)
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
                            // Empty parameter arrow detected
                            *index = next + 1; // consume past '=>'
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

                // Find the matching closing paren for this '(' before considering it as an arrow parameter list.
                // This avoids misinterpreting inner parentheses as the parameter list (e.g., `("prop_" + (() => 42)())`).
                let mut depth = 1usize;
                // Start scanning *after* the opening '(' so the depth accounting is correct
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
                    // `j` now points to the matching RParen; check the token after it (skipping line terminators)
                    let mut next = j + 1;
                    while next < tokens.len() && matches!(tokens[next].token, Token::LineTerminator) {
                        next += 1;
                    }
                    if next < tokens.len() && matches!(tokens[next].token, Token::Arrow) {
                        // Debug/tracing: record indices and tokens to help diagnose arrow param parsing
                        log::trace!(
                            "paren-arrow-check: index={}, j={}, next={} token_at_index={:?} token_at_j={:?}",
                            *index,
                            j,
                            next,
                            tokens.get(*index),
                            tokens.get(j)
                        );
                        // Attempt to parse the parameters precisely between `*index` and `j`.
                        // parse_parameters expects the index to point at the first token AFTER '('.
                        let mut t = *index;
                        log::trace!(
                            "paren-arrow: index={} t={} token_at_t={:?} token_at_j_plus_one={:?}",
                            *index,
                            t,
                            tokens.get(t),
                            tokens.get(j + 1)
                        );
                        if let Ok(params) = parse_parameters(tokens, &mut t) {
                            // Ensure parse_parameters consumed exactly up to the matching paren we found.
                            if t == j + 1 {
                                // It's a valid arrow parameter list matching the paren we found.
                                *index = next + 1; // set index to token after '=>'
                                let body = parse_arrow_body(tokens, index)?;
                                log::trace!("constructing arrow (paren params) params={:?}", params);
                                return Ok(Expr::ArrowFunction(params, body));
                            }
                        }
                    }
                }
            }
            // Not an arrow function; parse as parenthesized expression
            let expr_inner = parse_expression(tokens, index)?;
            // Allow line terminators between the inner expression and the closing ')'
            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                *index += 1;
            }
            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            *index += 1; // consume ')'
            // Return the inner expression for a plain parenthesized expression
            expr_inner
        }
        _ => {
            // Provide better error information for unexpected tokens during parsing
            // Log the remaining tokens for better context to help debugging
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

    // Handle postfix operators like index access. Accept line terminators
    // between the primary and certain postfix operators to support call-chains
    // split across lines (e.g. `promise.then(...)
    // .then(...)`). However, a line terminator must NOT appear before
    // Update operators (`++`/`--`) because `x\n++` should be a SyntaxError
    // (ASI inserts a semicolon after the expression).
    while *index < tokens.len() {
        log::trace!("parse_primary: postfix loop at idx {} -> {:?}", *index, tokens.get(*index));
        // If there's a LineTerminator here, look ahead to the next non-LT token.
        // If that token is `++` or `--`, do not treat this as a continuation
        // (leave the LineTerminator to terminate the statement so the `++`
        // becomes a separate (invalid) statement and produces a parse error).
        if matches!(tokens[*index].token, Token::LineTerminator) {
            let mut look = *index + 1;
            while look < tokens.len() && matches!(tokens[look].token, Token::LineTerminator) {
                look += 1;
            }
            if look < tokens.len() && matches!(tokens[look].token, Token::Increment | Token::Decrement) {
                break;
            }
            // Otherwise consume the leading line terminators and continue
            *index = look;
        }
        if *index >= tokens.len() {
            break;
        }
        match &tokens[*index].token {
            Token::LBracket => {
                *index += 1; // consume '['
                let index_expr = parse_expression(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1; // consume ']'
                expr = Expr::Index(Box::new(expr), Box::new(index_expr));
            }
            Token::Dot => {
                *index += 1; // consume '.'
                // Allow line terminators between '.' and the property name
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
                    // Private identifiers (e.g. `obj.#x`) are only syntactically
                    // valid inside class bodies and furthermore the referenced
                    // private name must have been *declared* in the *enclosing*
                    // class. Check the private-name stack for any matching declaration.
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
                *index += 1; // consume '?.'
                if *index >= tokens.len() {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                if matches!(tokens[*index].token, Token::LParen) {
                    if !allow_call {
                        // If calls are not allowed (e.g. inside `new`), we stop here.
                        // But wait, `?.(` is an optional call.
                        // If we are parsing `new A?.()`, this is invalid syntax in JS?
                        // Actually `new A?.()` is a syntax error in JS.
                        // `new` target cannot contain optional chain.
                        // But `parse_primary` handles optional chains.
                        // If `allow_call` is false, we should probably treat this as end of expression?
                        // Or error?
                        // For now, let's assume `allow_call` only restricts `LParen` calls.
                        // But `OptionalCall` is also a call.
                        // So we should break.
                        // Put back the `?.` token?
                        // We already consumed `?.`.
                        // This is tricky. If we consumed `?.` and see `(`, but calls are not allowed,
                        // then `new A?.(` is invalid.
                        // But `new A?.b` is also invalid because `new` target cannot be optional chain.
                        // So maybe we don't need to worry about `allow_call` for optional chain
                        // because optional chain is invalid in `new` anyway?
                        // Let's check spec: NewExpression cannot contain OptionalChain.
                        // So if we are in `new` (allow_call=false), and we see `?.`, we should probably error or break.
                        // But `parse_primary` is used for `new` target.
                        // If we see `?.`, we should probably let it parse, and then `new` will fail at runtime or we rely on parser error?
                        // Actually, if `new` target cannot be optional chain, we should break before consuming `?.`.
                        // But we are already inside the match arm.
                        //
                        // Let's ignore this for now and focus on `LParen`.
                        // If `allow_call` is false, we should break if we see `LParen`.
                    }
                    // Optional call: obj?.method(args)
                    *index += 1; // consume '('
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
                            *index += 1; // consume ','
                        }
                    }
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                    *index += 1; // consume ')'
                    expr = Expr::OptionalCall(Box::new(expr), args);
                } else if matches!(tokens[*index].token, Token::Identifier(_)) {
                    // Optional property access: obj?.prop
                    if let Some(prop) = tokens[*index].token.as_identifier_string() {
                        *index += 1;
                        expr = Expr::OptionalProperty(Box::new(expr), prop);
                    } else {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                } else if let Token::PrivateIdentifier(prop) = &tokens[*index].token {
                    // Optional private access: obj?.#x
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
                    // Optional computed property access: obj?.[expr]
                    *index += 1; // consume '['
                    let index_expr = parse_expression(tokens, index)?;
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                        return Err(raise_parse_error_at!(tokens.get(*index)));
                    }
                    *index += 1; // consume ']'
                    // If the bracket access is immediately followed by a call,
                    // e.g. `obj?.[expr](...)`, this is an optional call on the
                    // computed property. Parse the call arguments and build an
                    // OptionalCall around the computed access.
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                        *index += 1; // consume '('
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
                                *index += 1; // consume ','
                            }
                        }
                        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                            return Err(raise_parse_error_at!(tokens.get(*index)));
                        }
                        *index += 1; // consume ')'
                        // Flatten accidental single-Comma argument
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
                *index += 1; // consume '('
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
                        *index += 1; // consume ','
                        // allow trailing comma before ')' and skip newlines
                        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                            *index += 1;
                        }
                        if *index >= tokens.len() {
                            return Err(raise_parse_error_at!(tokens.get(*index)));
                        }
                        if matches!(tokens[*index].token, Token::RParen) {
                            break;
                        }
                        // If we have a trailing comma but NOT a RParen, we loop again.
                        // But wait, if we have a trailing comma, we expect another argument OR RParen.
                        // If we see RParen, we break.
                        // If we see something else, we loop and call parse_assignment.
                        // But parse_assignment might fail if the next token is not an expression start.
                    }
                }
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                *index += 1; // consume ')'
                // Flatten accidental single-Comma argument
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
    // Skip optional line terminators between `=>` and the body
    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
        *index += 1;
        let body = parse_statements(tokens, index)?;
        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
        *index += 1;
        Ok(body)
    } else {
        let expr = parse_assignment(tokens, index)?;
        Ok(vec![Statement::from(StatementKind::Return(Some(expr)))])
    }
}

pub fn parse_array_destructuring_pattern(tokens: &[TokenData], index: &mut usize) -> Result<Vec<DestructuringElement>, JSError> {
    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBracket) {
        return Err(raise_parse_error_at!(tokens.get(*index)));
    }
    // Debug: print a slice of upcoming tokens for analysis
    log::trace!(
        "parse_array_destructuring_pattern start tokens (first 20): {:?}",
        tokens.iter().skip(*index).take(20).collect::<Vec<_>>()
    );
    *index += 1; // consume [

    let mut pattern = Vec::new();
    // Skip initial blank lines inside the pattern
    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index < tokens.len() && matches!(tokens[*index].token, Token::RBracket) {
        *index += 1; // consume ]
        return Ok(pattern);
    }

    loop {
        // skip any blank lines at the start of a new property entry
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index < tokens.len() && matches!(tokens[*index].token, Token::Spread) {
            *index += 1; // consume ...
            if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
                *index += 1;
                pattern.push(DestructuringElement::Rest(name));
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
            // Rest must be the last element
            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            *index += 1; // consume ]
            break;
        } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
            pattern.push(DestructuringElement::Empty);
        } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
            // Nested array destructuring
            let nested_pattern = parse_array_destructuring_pattern(tokens, index)?;
            // Optional default initializer after nested pattern: `[...] = expr`
            let mut default_expr: Option<Box<Expr>> = None;
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                *index += 1; // consume '='
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
            // Nested object destructuring
            let nested_pattern = parse_object_destructuring_pattern(tokens, index)?;
            // Optional default initializer after nested pattern: `{...} = expr`
            let mut default_expr: Option<Box<Expr>> = None;
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                *index += 1; // consume '='
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
            // Accept optional default initializer in patterns: e.g. `a = 1`
            let mut default_expr: Option<Box<Expr>> = None;
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                *index += 1; // consume '='
                // capture initializer tokens until top-level comma or ] and parse them
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
        } else {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }

        // allow blank lines between last element and closing brace
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }

        if *index >= tokens.len() {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
        if matches!(tokens[*index].token, Token::RBracket) {
            *index += 1; // consume ]
            break;
        } else if matches!(tokens[*index].token, Token::Comma) {
            *index += 1; // consume ,
            // Allow trailing comma before closing bracket
            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                *index += 1;
            }
            if *index < tokens.len() && matches!(tokens[*index].token, Token::RBracket) {
                *index += 1; // consume ]
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
    *index += 1; // consume {

    let mut pattern = Vec::new();
    log::trace!(
        "parse_object_destructuring_pattern: tokens immediately after '{{' (first 8): {:?}",
        tokens.iter().take(8).collect::<Vec<_>>()
    );
    // Skip leading line terminators inside the pattern so multi-line
    // object patterns like `{
    //   a = 0,
    // }` are accepted.
    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
        *index += 1;
    }

    if *index < tokens.len() && matches!(tokens[*index].token, Token::RBrace) {
        *index += 1; // consume }
        return Ok(pattern);
    }

    loop {
        // allow and skip blank lines between elements
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        // If after skipping blanks we immediately hit a closing brace, accept
        // it. This handles the common formatting where there is a trailing
        // comma and then a newline before the closing `}` (e.g.
        // `a = 0,\n}`) which should be treated as the end of the object
        // pattern instead of expecting another property.
        if *index < tokens.len() && matches!(tokens[*index].token, Token::RBrace) {
            *index += 1; // consume }
            break;
        }
        if *index < tokens.len() && matches!(tokens[*index].token, Token::Spread) {
            *index += 1; // consume ...
            if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
                *index += 1;
                pattern.push(DestructuringElement::Rest(name));
            } else {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            // Rest must be the last element
            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }
            *index += 1; // consume }
            break;
        } else {
            // Parse property key (identifier, string, number, or computed)
            let mut key_name: Option<String> = None;
            let mut computed_key: Option<Expr> = None;
            let mut is_identifier_key = false;

            if matches!(tokens[*index].token, Token::LBracket) {
                *index += 1; // consume [
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
                                *index += 1; // consume ]
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
            } else if let Some(Token::Number(n)) = tokens.get(*index).map(|t| t.token.clone()) {
                *index += 1;
                key_name = Some(n.to_string());
            } else if let Some(Token::BigInt(s)) = tokens.get(*index).map(|t| t.token.clone()) {
                *index += 1;
                key_name = Some(s);
            } else if let Some(Token::StringLit(s)) = tokens.get(*index).map(|t| t.token.clone()) {
                *index += 1;
                key_name = Some(utf16_to_utf8(&s));
            } else {
                log::trace!("expected property key but got {:?}", tokens.get(*index));
                return Err(raise_parse_error_at!(tokens.get(*index)));
            }

            let value = if *index < tokens.len() && matches!(tokens[*index].token, Token::Colon) {
                *index += 1; // consume :
                // Parse the value pattern
                if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                    // Nested array destructuring as property value: `a: [ ... ]` possibly followed by `= init`
                    let nested = parse_array_destructuring_pattern(tokens, index)?;
                    let mut nested_default: Option<Box<Expr>> = None;
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                        *index += 1; // consume '='
                        let mut depth: i32 = 0;
                        let mut init_tokens: Vec<TokenData> = Vec::new();
                        while *index < tokens.len() {
                            if depth == 0 && (matches!(tokens[*index].token, Token::Comma) || matches!(tokens[*index].token, Token::RBrace))
                            {
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
                            nested_default = Some(Box::new(expr));
                        }
                    }
                    DestructuringElement::NestedArray(nested, nested_default)
                } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
                    // Nested object destructuring as property value: `a: { ... }` possibly followed by `= init`
                    let nested = parse_object_destructuring_pattern(tokens, index)?;
                    let mut nested_default: Option<Box<Expr>> = None;
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                        *index += 1; // consume '='
                        let mut depth: i32 = 0;
                        let mut init_tokens: Vec<TokenData> = Vec::new();
                        while *index < tokens.len() {
                            if depth == 0 && (matches!(tokens[*index].token, Token::Comma) || matches!(tokens[*index].token, Token::RBrace))
                            {
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
                            nested_default = Some(Box::new(expr));
                        }
                    }
                    DestructuringElement::NestedObject(nested, nested_default)
                } else if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
                    *index += 1;
                    // Allow default initializer for property value like `a: b = 1`
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
                                Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                                Token::RParen | Token::RBracket | Token::RBrace => depth -= 1,
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
                } else {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
            } else {
                if !is_identifier_key {
                    return Err(raise_parse_error_at!(tokens.get(*index)));
                }
                // Shorthand: key is the same as variable name. Allow optional
                // default initializer after the shorthand, e.g. `{a = 1}`.
                let mut init_tokens: Vec<TokenData> = Vec::new();
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                    *index += 1; // consume '='
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

        // allow whitespace / blank lines before separators or closing brace
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }

        if *index >= tokens.len() {
            return Err(raise_parse_error_at!(tokens.get(*index)));
        }
        if matches!(tokens[*index].token, Token::RBrace) {
            *index += 1; // consume }
            break;
        } else if matches!(tokens[*index].token, Token::Comma) {
            *index += 1; // consume ,
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
                    // Ensure the property name is 'prototype'
                    assert_eq!(prop, "prototype");
                    match &**base {
                        Expr::AsyncFunction(Some(name), _params, _body) | Expr::Function(Some(name), _params, _body) => {
                            assert_eq!(name, "foo");
                        }
                        other => panic!("expected async or function expression as base, got: {:?}", other),
                    }
                } else {
                    panic!("expected property expression");
                }
            }
            _ => panic!("expected expression statement"),
        }
    }
}
