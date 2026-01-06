#![allow(
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::collapsible_else_if,
    unused_variables,
    dead_code,
    unused_imports
)]

use crate::JSError;
use crate::core::statement::{Statement, StatementKind};
use crate::core::{BinaryOp, DestructuringElement, Expr, TemplatePart, Token, TokenData};
use crate::raise_parse_error;
use crate::{core::value::Value, raise_parse_error_with_token, unicode::utf16_to_utf8};
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
    if *index >= t.len() {
        return Err(raise_parse_error!("Unexpected end of input"));
    }
    let start_token = &t[*index];
    let line = start_token.line;
    let column = start_token.column;

    match start_token.token {
        Token::Function => parse_function_declaration(t, index),
        Token::If => parse_if_statement(t, index),
        Token::Return => parse_return_statement(t, index),
        Token::Throw => parse_throw_statement(t, index),
        Token::Try => parse_try_statement(t, index),
        Token::LBrace => parse_block_statement(t, index),
        Token::Var => parse_var_statement(t, index),
        Token::Let => parse_let_statement(t, index),
        Token::Const => parse_const_statement(t, index),
        _ => {
            let expr = parse_expression(t, index)?;
            if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
                *index += 1;
            }
            Ok(Statement {
                kind: StatementKind::Expr(expr),
                line,
                column,
            })
        }
    }
}

fn parse_function_declaration(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1; // consume function
    let name = if let Token::Identifier(name) = &t[*index].token {
        name.clone()
    } else {
        return Err(raise_parse_error_at(&t[*index..]));
    };
    *index += 1;

    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at(&t[*index..]));
    }
    *index += 1; // consume (

    let params = parse_parameters(t, index)?;

    if !matches!(t[*index].token, Token::LBrace) {
        return Err(raise_parse_error_at(&t[*index..]));
    }
    *index += 1; // consume {

    let body = parse_statement_block(t, index)?;

    Ok(Statement {
        kind: StatementKind::FunctionDeclaration(name, params, body, false),
        line: t[start].line,
        column: t[start].column,
    })
}

fn parse_if_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1; // consume if
    if !matches!(t[*index].token, Token::LParen) {
        return Err(raise_parse_error_at(&t[*index..]));
    }
    *index += 1; // consume (
    let condition = parse_expression(t, index)?;
    if !matches!(t[*index].token, Token::RParen) {
        return Err(raise_parse_error_at(&t[*index..]));
    }
    *index += 1; // consume )

    let then_stmt = parse_statement_item(t, index)?;
    let then_block = match then_stmt.kind {
        StatementKind::Block(stmts) => stmts,
        _ => vec![then_stmt],
    };

    let else_block = if *index < t.len() && matches!(t[*index].token, Token::Else) {
        *index += 1;
        let else_stmt = parse_statement_item(t, index)?;
        match else_stmt.kind {
            StatementKind::Block(stmts) => Some(stmts),
            _ => Some(vec![else_stmt]),
        }
    } else {
        None
    };

    Ok(Statement {
        kind: StatementKind::If(condition, then_block, else_block),
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
        kind: StatementKind::Return(expr),
        line: t[start].line,
        column: t[start].column,
    })
}

fn parse_throw_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1; // consume throw
    if matches!(t[*index].token, Token::LineTerminator) {
        return Err(raise_parse_error!("Illegal newline after throw"));
    }
    let expr = parse_expression(t, index)?;
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
    }
    Ok(Statement {
        kind: StatementKind::Throw(expr),
        line: t[start].line,
        column: t[start].column,
    })
}

fn parse_try_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1; // consume try

    let try_block = parse_block_statement(t, index)?;
    let try_body = if let StatementKind::Block(stmts) = try_block.kind {
        stmts
    } else {
        return Err(raise_parse_error!("Expected block after try"));
    };

    let mut catch_param = None;
    let mut catch_body = None;

    if *index < t.len() && matches!(t[*index].token, Token::Catch) {
        *index += 1; // consume catch

        // Optional catch binding
        if *index < t.len() && matches!(t[*index].token, Token::LParen) {
            *index += 1; // consume (
            if let Token::Identifier(name) = &t[*index].token {
                catch_param = Some(name.clone());
                *index += 1;
            } else {
                return Err(raise_parse_error!("Expected identifier in catch binding"));
            }
            if *index >= t.len() || !matches!(t[*index].token, Token::RParen) {
                return Err(raise_parse_error!("Expected ) after catch binding"));
            }
            *index += 1; // consume )
        }

        let catch_block = parse_block_statement(t, index)?;
        if let StatementKind::Block(stmts) = catch_block.kind {
            catch_body = Some(stmts);
        } else {
            return Err(raise_parse_error!("Expected block after catch"));
        }
    }

    let mut finally_body = None;
    if *index < t.len() && matches!(t[*index].token, Token::Finally) {
        *index += 1; // consume finally
        let finally_block = parse_block_statement(t, index)?;
        if let StatementKind::Block(stmts) = finally_block.kind {
            finally_body = Some(stmts);
        } else {
            return Err(raise_parse_error!("Expected block after finally"));
        }
    }

    if catch_body.is_none() && finally_body.is_none() {
        return Err(raise_parse_error!("Missing catch or finally after try"));
    }

    Ok(Statement {
        kind: StatementKind::TryCatch(try_body, catch_param, catch_body, finally_body),
        line: t[start].line,
        column: t[start].column,
    })
}

fn parse_block_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1; // consume {
    let body = parse_statements(t, index)?;
    if *index >= t.len() || !matches!(t[*index].token, Token::RBrace) {
        return Err(raise_parse_error_at(&t[*index..]));
    }
    *index += 1; // consume }
    Ok(Statement {
        kind: StatementKind::Block(body),
        line: t[start].line,
        column: t[start].column,
    })
}

fn parse_var_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1; // consume var
    let decls = parse_variable_declaration_list(t, index)?;
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
    }
    Ok(Statement {
        kind: StatementKind::Var(decls),
        line: t[start].line,
        column: t[start].column,
    })
}

fn parse_let_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1; // consume let
    let decls = parse_variable_declaration_list(t, index)?;
    if *index < t.len() && matches!(t[*index].token, Token::Semicolon) {
        *index += 1;
    }
    Ok(Statement {
        kind: StatementKind::Let(decls),
        line: t[start].line,
        column: t[start].column,
    })
}

fn parse_const_statement(t: &[TokenData], index: &mut usize) -> Result<Statement, JSError> {
    let start = *index;
    *index += 1; // consume const
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
        kind: StatementKind::Const(const_decls),
        line: t[start].line,
        column: t[start].column,
    })
}

fn parse_variable_declaration_list(t: &[TokenData], index: &mut usize) -> Result<Vec<(String, Option<Expr>)>, JSError> {
    let mut decls = Vec::new();
    loop {
        if let Token::Identifier(name) = &t[*index].token {
            let name = name.clone();
            *index += 1;
            let init = if *index < t.len() && matches!(t[*index].token, Token::Assign) {
                *index += 1;
                Some(parse_assignment(t, index)?)
            } else {
                None
            };
            decls.push((name, init));
        } else {
            return Err(raise_parse_error_at(&t[*index..]));
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
    let left = parse_assignment(tokens, index)?;
    Ok(left)
}

pub fn raise_parse_error_at(tokens: &[TokenData]) -> JSError {
    if let Some(t) = tokens.first() {
        raise_parse_error_with_token!(t)
    } else {
        raise_parse_error!()
    }
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

pub fn parse_parameters(tokens: &[TokenData], index: &mut usize) -> Result<Vec<DestructuringElement>, JSError> {
    let mut params = Vec::new();
    log::trace!(
        "parse_parameters: starting tokens (first 16): {:?}",
        tokens.iter().take(16).collect::<Vec<_>>()
    );
    if *index < tokens.len() && !matches!(tokens[*index].token, Token::RParen) {
        loop {
            if matches!(tokens[*index].token, Token::Spread) {
                // Handle rest parameter: ...args
                *index += 1; // consume ...
                if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
                    *index += 1;
                    params.push(DestructuringElement::Rest(name));

                    if *index >= tokens.len() {
                        return Err(raise_parse_error!("Unexpected end of parameters after rest"));
                    }
                    // Rest parameter must be the last one
                    if !matches!(tokens[*index].token, Token::RParen) {
                        return Err(raise_parse_error_with_token!(
                            tokens[*index],
                            "Rest parameter must be last formal parameter"
                        ));
                    }
                    break;
                } else {
                    return Err(raise_parse_error_at(tokens));
                }
            } else if matches!(tokens[*index].token, Token::LBrace) {
                let pattern = parse_object_destructuring_pattern(tokens, index)?;
                params.push(DestructuringElement::NestedObject(pattern));
            } else if matches!(tokens[*index].token, Token::LBracket) {
                let pattern = parse_array_destructuring_pattern(tokens, index)?;
                params.push(DestructuringElement::NestedArray(pattern));
            } else if let Some(Token::Identifier(param)) = tokens.get(*index).map(|t| &t.token).cloned() {
                *index += 1;
                let mut default_expr: Option<Box<Expr>> = None;
                // Support default initializers: identifier '=' expression
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Assign) {
                    *index += 1;
                    let expr = parse_assignment(tokens, index)?;
                    default_expr = Some(Box::new(expr));
                }
                params.push(DestructuringElement::Variable(param, default_expr));
            } else {
                return Err(raise_parse_error_at(tokens));
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
        }
    }
    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
        return Err(raise_parse_error_at(tokens));
    }
    *index += 1; // consume )
    log::trace!(
        "parse_parameters: consumed ')', remaining tokens (first 16): {:?}",
        tokens.iter().take(16).collect::<Vec<_>>()
    );
    Ok(params)
}

pub fn parse_statement_block(tokens: &[TokenData], index: &mut usize) -> Result<Vec<Statement>, JSError> {
    let body = parse_statements(tokens, index)?;
    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
        return Err(raise_parse_error_at(&tokens[*index..]));
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
    log::trace!(
        "parse_object_destructuring_pattern: tokens after initial skip (first 8): {:?}",
        tokens.iter().take(8).collect::<Vec<_>>()
    );
    let mut left = parse_assignment(tokens, index)?;
    while *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
        *index += 1; // consume ,
        let right = parse_assignment(tokens, index)?;
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
            return Err(raise_parse_error_at(&tokens[*index..]));
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
        Expr::OptionalProperty(_, _) | Expr::OptionalIndex(_, _) | Expr::OptionalCall(_, _) => true,
        Expr::Property(obj, _) => contains_optional_chain(obj.as_ref()),
        Expr::Index(obj, idx) => contains_optional_chain(obj.as_ref()) || contains_optional_chain(idx.as_ref()),
        Expr::Call(obj, _) => contains_optional_chain(obj.as_ref()),
        _ => false,
    }
}

pub fn parse_assignment(tokens: &[TokenData], index: &mut usize) -> Result<Expr, JSError> {
    let left = parse_conditional(tokens, index)?;
    if *index >= tokens.len() {
        return Ok(left);
    }

    if let Some(ctor) = get_assignment_ctor(&tokens[*index].token) {
        if contains_optional_chain(&left) {
            return Err(raise_parse_error_at(&tokens[*index..]));
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
        Ok(Expr::Binary(Box::new(left), BinaryOp::NullishCoalescing, Box::new(right)))
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

/*
pub fn parse_class_body(tokens: &mut Vec<TokenData>) -> Result<Vec<ClassMember>, JSError> {
    // Mark that we're parsing inside a class body so that private identifier
    // property access (like `obj.#x`) can be validated syntactically only
    // when parsing class element bodies. Use thread-local depth to avoid
    // cross-thread races while running tests in parallel.
    let _guard = ClassContextGuard::new();

    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
        return Err(raise_parse_error_at(tokens));
    }
    *index += 1; // consume {

    let mut members = Vec::new();
    // Track declared private names to detect duplicate private declarations.
    let mut declared_private_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Create an Rc-backed set we can push into a thread-local stack so inner
    // parsing of method bodies can validate private name usage against the
    // current class's declared private names.
    let current_private_names = std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashSet::new()));
    let _private_guard = ClassPrivateNamesGuard::new(current_private_names.clone());

    // Pre-scan the class body (without consuming tokens) to collect all declared
    // private names. This ensures that uses of private names inside earlier
    // members (e.g. constructor referencing a private method declared later)
    // are considered valid during parsing of those members. The scan is
    // conservative and only looks at top-level class members.
    {
        let mut pos: usize = 0;
        while pos < tokens.len() {
            if matches!(tokens[pos].token, Token::RBrace) {
                break;
            }
            // skip separators
            if matches!(tokens[pos].token, Token::Semicolon | Token::LineTerminator) {
                pos += 1;
                continue;
            }
            // optional 'static' prefix
            if matches!(tokens[pos].token, Token::Static) {
                pos += 1;
                // static block: '{' ... '}'
                if pos < tokens.len() && matches!(tokens[pos].token, Token::LBrace) {
                    // skip balanced braces
                    let mut depth: usize = 1;
                    pos += 1;
                    while pos < tokens.len() && depth > 0 {
                        if matches!(tokens[pos].token, Token::LBrace) {
                            depth += 1;
                        } else if matches!(tokens[pos].token, Token::RBrace) {
                            depth -= 1;
                        }
                        pos += 1;
                    }
                    continue;
                }
            }

            // Accessor `get`/`set` followed by a private identifier
            if let Some(Token::Identifier(id)) = tokens.get(pos).map(|t| &t.token) {
                if id == "get" || id == "set" {
                    if let Some(Token::PrivateIdentifier(name)) = tokens.get(pos + 1).map(|t| &t.token) {
                        current_private_names.borrow_mut(mc).insert(name.clone());
                    }
                    // Advance past 'get'/'set' and the following name (if any)
                    pos += 1;
                    if pos < tokens.len()
                        && (matches!(tokens[pos].token, Token::Identifier(_)) || matches!(tokens[pos].token, Token::PrivateIdentifier(_)))
                    {
                        pos += 1;
                    }
                    // Skip params (balanced parentheses)
                    if pos < tokens.len() && matches!(tokens[pos].token, Token::LParen) {
                        let mut depth = 1usize;
                        pos += 1;
                        while pos < tokens.len() && depth > 0 {
                            if matches!(tokens[pos].token, Token::LParen) {
                                depth += 1;
                            } else if matches!(tokens[pos].token, Token::RParen) {
                                depth -= 1;
                            }
                            pos += 1;
                        }
                    }
                    // Skip the function body if present (balanced braces)
                    if pos < tokens.len() && matches!(tokens[pos].token, Token::LBrace) {
                        let mut depth = 1usize;
                        pos += 1;
                        while pos < tokens.len() && depth > 0 {
                            if matches!(tokens[pos].token, Token::LBrace) {
                                depth += 1;
                            } else if matches!(tokens[pos].token, Token::RBrace) {
                                depth -= 1;
                            }
                            pos += 1;
                        }
                    }
                    continue;
                }
            }

            // Private identifier starting a member (private method/property)
            if let Some(Token::PrivateIdentifier(name)) = tokens.get(pos).map(|t| &t.token) {
                current_private_names.borrow_mut(mc).insert(name.clone());
                pos += 1;
                // If this is a method, skip params and body
                if pos < tokens.len() && matches!(tokens[pos].token, Token::LParen) {
                    let mut depth = 1usize;
                    pos += 1;
                    while pos < tokens.len() && depth > 0 {
                        if matches!(tokens[pos].token, Token::LParen) {
                            depth += 1;
                        } else if matches!(tokens[pos].token, Token::RParen) {
                            depth -= 1;
                        }
                        pos += 1;
                    }
                    if pos < tokens.len() && matches!(tokens[pos].token, Token::LBrace) {
                        let mut depth = 1usize;
                        pos += 1;
                        while pos < tokens.len() && depth > 0 {
                            if matches!(tokens[pos].token, Token::LBrace) {
                                depth += 1;
                            } else if matches!(tokens[pos].token, Token::RBrace) {
                                depth -= 1;
                            }
                            pos += 1;
                        }
                    }
                    continue;
                }
                // If this is a property with initializer, skip until semicolon
                if pos < tokens.len() && matches!(tokens[pos].token, Token::Assign) {
                    pos += 1;
                    while pos < tokens.len() && !matches!(tokens[pos].token, Token::Semicolon | Token::LineTerminator) {
                        // For safety, advance by one. We don't need to parse the expression fully.
                        pos += 1;
                    }
                    if pos < tokens.len() && matches!(tokens[pos].token, Token::Semicolon | Token::LineTerminator) {
                        pos += 1;
                    }
                    continue;
                }
                // property without initializer
                if pos < tokens.len() && matches!(tokens[pos].token, Token::Semicolon | Token::LineTerminator) {
                    pos += 1;
                    continue;
                }
            }

            // Regular identifier member: skip to end of member
            if let Some(Token::Identifier(_)) = tokens.get(pos).map(|t| &t.token) {
                // Advance past name
                pos += 1;
                // Method
                if pos < tokens.len() && matches!(tokens[pos].token, Token::LParen) {
                    let mut depth = 1usize;
                    pos += 1;
                    while pos < tokens.len() && depth > 0 {
                        if matches!(tokens[pos].token, Token::LParen) {
                            depth += 1;
                        } else if matches!(tokens[pos].token, Token::RParen) {
                            depth -= 1;
                        }
                        pos += 1;
                    }
                    if pos < tokens.len() && matches!(tokens[pos].token, Token::LBrace) {
                        let mut depth = 1usize;
                        pos += 1;
                        while pos < tokens.len() && depth > 0 {
                            if matches!(tokens[pos].token, Token::LBrace) {
                                depth += 1;
                            } else if matches!(tokens[pos].token, Token::RBrace) {
                                depth -= 1;
                            }
                            pos += 1;
                        }
                    }
                    continue;
                }
                // Property with initializer
                if pos < tokens.len() && matches!(tokens[pos].token, Token::Assign) {
                    pos += 1;
                    while pos < tokens.len() && !matches!(tokens[pos].token, Token::Semicolon | Token::LineTerminator) {
                        pos += 1;
                    }
                    if pos < tokens.len() && matches!(tokens[pos].token, Token::Semicolon | Token::LineTerminator) {
                        pos += 1;
                    }
                    continue;
                }
                // Property without initializer
                if pos < tokens.len() && matches!(tokens[pos].token, Token::Semicolon | Token::LineTerminator) {
                    pos += 1;
                    continue;
                }
            }

            // Fallback: advance one token to avoid infinite loop
            pos += 1;
        }
    }

    while *index < tokens.len() && !matches!(tokens[*index].token, Token::RBrace) {
        // Skip blank lines or stray semicolons in class body
        while *index < tokens.len() && matches!(tokens[*index].token, Token::Semicolon | Token::LineTerminator) {
            *index += 1;
        }
        if *index >= tokens.len() || matches!(tokens[*index].token, Token::RBrace) {
            break;
        }
        let is_static = if *index < tokens.len() && matches!(tokens[*index].token, Token::Static) {
            *index += 1;
            true
        } else {
            false
        };

        if is_static && *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
            *index += 1; // consume {
            let body = parse_statement_block(tokens, index)?;
            members.push(ClassMember::StaticBlock(body));
            continue;
        }

        // Support accessor-first syntax: `get prop() {}` or `set prop(v) {}` or `get #prop() {}` etc.
        if let Some(Token::Identifier(kw)) = tokens.first().map(|t| &t.token) {
            if kw == "get" || kw == "set" {
                let is_getter = kw == "get";
                *index += 1; // consume get/set keyword
                if *index >= tokens.len()
                    || (!matches!(tokens[*index].token, Token::Identifier(_)) && !matches!(tokens[*index].token, Token::PrivateIdentifier(_)))
                {
                    return Err(raise_parse_error_at(tokens));
                }
                // Capture whether this accessor uses a private identifier
                let (prop_name, is_private) = match & *index += 1.token {
                    Token::Identifier(name) => (name.clone(), false),
                    Token::PrivateIdentifier(name) => (name.clone(), true),
                    _ => return Err(raise_parse_error_at(tokens)),
                };
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LParen) {
                    return Err(raise_parse_error_at(tokens));
                }
                *index += 1; // consume (
                let params = parse_parameters(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                    return Err(raise_parse_error_at(tokens));
                }
                *index += 1; // consume {
                let body = parse_statement_block(tokens, index)?;
                if is_getter {
                    if !params.is_empty() {
                        return Err(raise_parse_error_at(tokens)); // getters should have no parameters
                    }
                    if is_static {
                        if is_private {
                            members.push(ClassMember::PrivateStaticGetter(prop_name, body));
                        } else {
                            members.push(ClassMember::StaticGetter(prop_name, body));
                        }
                    } else {
                        if is_private {
                            members.push(ClassMember::PrivateGetter(prop_name, body));
                        } else {
                            members.push(ClassMember::Getter(prop_name, body));
                        }
                    }
                } else {
                    // setter
                    if params.len() != 1 {
                        return Err(raise_parse_error_at(tokens)); // setters should have exactly one parameter
                    }
                    if is_static {
                        if is_private {
                            members.push(ClassMember::PrivateStaticSetter(prop_name, params, body));
                        } else {
                            members.push(ClassMember::StaticSetter(prop_name, params, body));
                        }
                    } else {
                        if is_private {
                            members.push(ClassMember::PrivateSetter(prop_name, params, body));
                        } else {
                            members.push(ClassMember::Setter(prop_name, params, body));
                        }
                    }
                }
                continue;
            }
        }

        if let Some(Token::Identifier(method_name)) = tokens.first().map(|t| &t.token) {
            let method_name = method_name.clone();
            if method_name == "constructor" {
                *index += 1;
                // Parse constructor
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LParen) {
                    return Err(raise_parse_error_at(tokens));
                }
                *index += 1; // consume (
                let params = parse_parameters(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                    return Err(raise_parse_error_at(tokens));
                }
                *index += 1; // consume {
                let body = parse_statement_block(tokens, index)?;
                members.push(ClassMember::Constructor(params, body));
            } else {
                *index += 1;
                if *index >= tokens.len() {
                    return Err(raise_parse_error_at(tokens));
                }
                // Check for getter/setter
                let is_getter = matches!(tokens[*index].token, Token::Identifier(ref id) if id == "get");
                let is_setter = matches!(tokens[*index].token, Token::Identifier(ref id) if id == "set");
                if is_getter || is_setter {
                    *index += 1; // consume get/set
                    if *index >= tokens.len()
                        || (!matches!(tokens[*index].token, Token::Identifier(_)) && !matches!(tokens[*index].token, Token::PrivateIdentifier(_)))
                    {
                        return Err(raise_parse_error_at(tokens));
                    }
                    let (prop_name, is_private) = match & *index += 1.token {
                        Token::Identifier(name) => (name.clone(), false),
                        Token::PrivateIdentifier(name) => (name.clone(), true),
                        _ => return Err(raise_parse_error_at(tokens)),
                    };
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LParen) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    *index += 1; // consume (
                    let params = parse_parameters(tokens, index)?;
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    *index += 1; // consume {
                    let body = parse_statement_block(tokens, index)?;
                    if is_getter {
                        if !params.is_empty() {
                            return Err(raise_parse_error_at(tokens)); // getters should have no parameters
                        }
                        if is_static {
                            if is_private {
                                members.push(ClassMember::PrivateStaticGetter(prop_name, body));
                            } else {
                                members.push(ClassMember::StaticGetter(prop_name, body));
                            }
                        } else {
                            if is_private {
                                members.push(ClassMember::PrivateGetter(prop_name, body));
                            } else {
                                members.push(ClassMember::Getter(prop_name, body));
                            }
                        }
                    } else {
                        // setter
                        if params.len() != 1 {
                            return Err(raise_parse_error_at(tokens)); // setters should have exactly one parameter
                        }
                        if is_static {
                            if is_private {
                                members.push(ClassMember::PrivateStaticSetter(prop_name, params, body));
                            } else {
                                members.push(ClassMember::StaticSetter(prop_name, params, body));
                            }
                        } else {
                            if is_private {
                                members.push(ClassMember::PrivateSetter(prop_name, params, body));
                            } else {
                                members.push(ClassMember::Setter(prop_name, params, body));
                            }
                        }
                    }
                } else if matches!(tokens[*index].token, Token::LParen) {
                    // This is a method
                    *index += 1; // consume (
                    let params = parse_parameters(tokens, index)?;
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    *index += 1; // consume {
                    let body = parse_statement_block(tokens, index)?;
                    if is_static {
                        members.push(ClassMember::StaticMethod(method_name, params, body));
                    } else {
                        members.push(ClassMember::Method(method_name, params, body));
                    }
                } else if matches!(tokens[*index].token, Token::Assign) {
                    // This is a property
                    *index += 1; // consume =
                    let value = parse_expression(tokens)?;
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::Semicolon | Token::LineTerminator) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    *index += 1; // consume ;
                    if is_static {
                        members.push(ClassMember::StaticProperty(method_name, value));
                    } else {
                        members.push(ClassMember::Property(method_name, value));
                    }
                } else if matches!(tokens[*index].token, Token::Semicolon | Token::LineTerminator) {
                    // Property without initializer
                    *index += 1; // consume ;
                    if is_static {
                        members.push(ClassMember::StaticProperty(method_name, Expr::Undefined));
                    } else {
                        members.push(ClassMember::Property(method_name, Expr::Undefined));
                    }
                } else {
                    return Err(raise_parse_error_at(tokens));
                }
            }
        } else if let Some(Token::PrivateIdentifier(name)) = tokens.first().map(|t| &t.token) {
            let name = name.clone();
            // Duplicate private names are a syntax error
            if declared_private_names.contains(&name) {
                let msg = format!("Identifier '#{name}' has already been declared");
                return Err(raise_parse_error_with_token!(tokens[*index], msg));
            }
            // Record declaration
            declared_private_names.insert(name.clone());
            // Also record in the current private-name set for validation inside
            // method bodies parsed subsequently.
            current_private_names.borrow_mut(mc).insert(name.clone());

            *index += 1;
            if matches!(tokens[*index].token, Token::LParen) {
                // Private method
                *index += 1; // consume (
                let params = parse_parameters(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                    return Err(raise_parse_error_at(tokens));
                }
                *index += 1; // consume {
                let body = parse_statement_block(tokens, index)?;
                if is_static {
                    members.push(ClassMember::PrivateStaticMethod(name, params, body));
                } else {
                    members.push(ClassMember::PrivateMethod(name, params, body));
                }
            } else if matches!(tokens[*index].token, Token::Assign) {
                // Private property
                *index += 1; // consume =
                let value = parse_expression(tokens)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::Semicolon | Token::LineTerminator) {
                    return Err(raise_parse_error_at(tokens));
                }
                *index += 1; // consume ;
                if is_static {
                    members.push(ClassMember::PrivateStaticProperty(name, value));
                } else {
                    members.push(ClassMember::PrivateProperty(name, value));
                }
            } else if matches!(tokens[*index].token, Token::Semicolon | Token::LineTerminator) {
                // Private property without initializer
                *index += 1; // consume ;
                if is_static {
                    members.push(ClassMember::PrivateStaticProperty(name, Expr::Undefined));
                } else {
                    members.push(ClassMember::PrivateProperty(name, Expr::Undefined));
                }
            } else {
                return Err(raise_parse_error_at(tokens));
            }
        } else {
            return Err(raise_parse_error_at(tokens));
        }
    }

    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
        return Err(raise_parse_error_at(tokens));
    }
    *index += 1; // consume }
    Ok(members)
}
*/

fn parse_primary(tokens: &[TokenData], index: &mut usize, allow_call: bool) -> Result<Expr, JSError> {
    // Skip any leading line terminators inside expressions so multi-line
    // expression continuations like `a +\n b` parse correctly.
    while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
        *index += 1;
    }
    if *index >= tokens.len() {
        return Err(raise_parse_error_at(&tokens[*index..]));
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
            if let Expr::Property(_, prop_name) = &inner {
                if prop_name.starts_with('#') {
                    let msg = format!("Private field '{prop_name}' cannot be deleted");
                    return Err(raise_parse_error_with_token!(token_data, msg));
                }
            }
            Expr::Delete(Box::new(inner))
        }
        Token::Void => {
            let inner = parse_primary(tokens, index, true)?;
            Expr::Void(Box::new(inner))
        }
        Token::Await => {
            let inner = parse_primary(tokens, index, true)?;
            Expr::Await(Box::new(inner))
        }
        Token::Yield => {
            // yield can be followed by an optional expression
            if *index >= tokens.len()
                || matches!(
                    tokens[*index].token,
                    Token::Semicolon | Token::Comma | Token::RParen | Token::RBracket | Token::RBrace | Token::Colon
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
            // Class Expression
            // class [Identifier] [extends Expression] { ClassBody }
            let name = if *index < tokens.len() {
                if let Token::Identifier(n) = &tokens[*index].token {
                    let n = n.clone();
                    *index += 1;
                    n
                } else {
                    "".to_string() // Anonymous class expression
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

            // let members = parse_class_body(tokens)?;

            // let class_def = crate::js_class::ClassDefinition { name, extends, members };
            // Expr::Class(std::rc::Rc::new(class_def))
            Expr::Number(0.0) // TODO: class
        }
        Token::New => {
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
                            return Err(raise_parse_error_at(tokens));
                        }
                        if matches!(tokens[*index].token, Token::RParen) {
                            break;
                        }
                        if !matches!(tokens[*index].token, Token::Comma) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        *index += 1; // consume ','
                        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                            *index += 1;
                        }
                        if *index >= tokens.len() {
                            return Err(raise_parse_error_at(tokens));
                        }
                        if matches!(tokens[*index].token, Token::RParen) {
                            break;
                        }
                    }
                }
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                    return Err(raise_parse_error_at(tokens));
                }
                *index += 1; // consume ')'
                if args.len() == 1 {
                    if let Expr::Comma(_, _) = &args[0] {
                        let first = args.remove(0);
                        let new_args = flatten_commas(first);
                        args.extend(new_args);
                    }
                }
                args
            } else {
                Vec::new()
            };
            Expr::New(Box::new(constructor), args)
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
            let inner = parse_primary(tokens, index, true)?;
            Expr::Spread(Box::new(inner))
        }
        Token::TemplateString(parts) => {
            if parts.is_empty() {
                Expr::StringLit(Vec::new())
            } else if parts.len() == 1 {
                match &parts[0] {
                    TemplatePart::String(s) => Expr::StringLit(s.clone()),
                    TemplatePart::Expr(expr_tokens) => {
                        let expr_tokens = expr_tokens.clone();
                        let e = parse_expression(&expr_tokens, &mut 0)?;
                        // Force string context by prepending "" + e
                        Expr::Binary(Box::new(Expr::StringLit(Vec::new())), BinaryOp::Add, Box::new(e))
                    }
                }
            } else {
                // Build binary addition chain
                let mut expr = match &parts[0] {
                    TemplatePart::String(s) => Expr::StringLit(s.clone()),
                    TemplatePart::Expr(expr_tokens) => {
                        let expr_tokens = expr_tokens.clone();
                        let e = parse_expression(&expr_tokens, &mut 0)?;
                        // Force string context by prepending "" + e
                        Expr::Binary(Box::new(Expr::StringLit(Vec::new())), BinaryOp::Add, Box::new(e))
                    }
                };
                for part in &parts[1..] {
                    let right = match part {
                        TemplatePart::String(s) => Expr::StringLit(s.clone()),
                        TemplatePart::Expr(expr_tokens) => {
                            let expr_tokens = expr_tokens.clone();
                            parse_expression(&expr_tokens, &mut 0)?
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
        Token::PrivateIdentifier(name) => {
            // Represent a standalone private name as a string like "#name" so
            // it can be used in contexts like `#name in obj`.
            Expr::StringLit(crate::unicode::utf8_to_utf16(&format!("#{}", name)))
        }
        Token::Import => Expr::Var("import".to_string(), Some(token_data.line), Some(token_data.column)),
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
                            return Err(raise_parse_error_at(tokens));
                        }
                        if matches!(tokens[*index].token, Token::RParen) {
                            break;
                        }
                        if !matches!(tokens[*index].token, Token::Comma) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        *index += 1; // consume ','
                    }
                }
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                    return Err(raise_parse_error_at(tokens));
                }
                *index += 1; // consume ')'
                Expr::SuperCall(args)
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Dot) {
                *index += 1; // consume '.'
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::Identifier(_)) {
                    return Err(raise_parse_error_at(tokens));
                }
                *index += 1;
                let prop = if let Token::Identifier(name) = &tokens[*index - 1].token {
                    name.clone()
                } else {
                    return Err(raise_parse_error_at(tokens));
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
                                return Err(raise_parse_error_at(tokens));
                            }
                            if matches!(tokens[*index].token, Token::RParen) {
                                break;
                            }
                            if !matches!(tokens[*index].token, Token::Comma) {
                                return Err(raise_parse_error_at(tokens));
                            }
                            *index += 1; // consume ','
                            // permit trailing comma before ) and skip newlines
                            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                                *index += 1;
                            }
                            if *index >= tokens.len() {
                                return Err(raise_parse_error_at(tokens));
                            }
                            if matches!(tokens[*index].token, Token::RParen) {
                                break;
                            }
                        }
                    }
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    *index += 1; // consume ')'
                    // Flatten accidental single-Comma argument
                    if args.len() == 1 {
                        if let Expr::Comma(_, _) = &args[0] {
                            let first = args.remove(0);
                            let new_args = flatten_commas(first);
                            args.extend(new_args);
                        }
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
                return Ok(Expr::Object(properties));
            }
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
                    return Err(raise_parse_error_at(tokens));
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
                    properties.push((Expr::StringLit(Vec::new()), Expr::Spread(Box::new(expr)), false));
                } else {
                    // Check for getter/setter: only treat as getter/setter if the
                    // identifier 'get'/'set' is followed by a property key and
                    // an opening parenthesis (no colon). This avoids confusing a
                    // regular property named 'get'/'set' (e.g. `set: function(...)`) with
                    // the getter/setter syntax.
                    // Recognize getter/setter signatures including computed keys
                    let is_getter = if tokens.len() >= 2 && matches!(tokens[*index].token, Token::Identifier(ref id) if id == "get") {
                        if matches!(tokens[1].token, Token::Identifier(_) | Token::StringLit(_)) {
                            tokens.len() >= 3 && matches!(tokens[2].token, Token::LParen)
                        } else if matches!(tokens[1].token, Token::LBracket) {
                            // find matching RBracket and ensure '(' follows
                            let mut depth = 0i32;
                            let mut idx_after = None;
                            for (i, t) in tokens.iter().enumerate().skip(1) {
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

                    let is_setter = if tokens.len() >= 2 && matches!(tokens[*index].token, Token::Identifier(ref id) if id == "set") {
                        if matches!(tokens[1].token, Token::Identifier(_) | Token::StringLit(_)) {
                            tokens.len() >= 3 && matches!(tokens[2].token, Token::LParen)
                        } else if matches!(tokens[1].token, Token::LBracket) {
                            let mut depth = 0i32;
                            let mut idx_after = None;
                            for (i, t) in tokens.iter().enumerate().skip(1) {
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
                    let key_expr = if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                        // Check for concise method: Identifier + (
                        if !is_getter && !is_setter && tokens.len() >= 2 && matches!(tokens[1].token, Token::LParen) {
                            // Concise method
                            *index += 1; // consume name
                            *index += 1; // consume (
                            let params = parse_parameters(tokens, index)?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                                return Err(raise_parse_error_at(tokens));
                            }
                            *index += 1; // consume {
                            let body = parse_statements(tokens, index)?;
                            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                                return Err(raise_parse_error_at(tokens));
                            }
                            *index += 1; // consume }
                            properties.push((
                                Expr::StringLit(crate::unicode::utf8_to_utf16(&name)),
                                Expr::Function(None, params, body),
                                true,
                            ));

                            // After adding method, skip any newline/semicolons and handle comma/end in outer loop
                            while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator | Token::Semicolon) {
                                *index += 1;
                            }
                            if *index >= tokens.len() {
                                return Err(raise_parse_error_at(tokens));
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
                    } else if let Some(Token::Number(n)) = tokens.first().map(|t| t.token.clone()) {
                        // Numeric property keys are allowed in object literals (they become strings)
                        *index += 1;
                        // Format as integer if whole number, otherwise use default representation
                        let s = if n.fract() == 0.0 { format!("{}", n as i64) } else { n.to_string() };
                        Expr::StringLit(crate::unicode::utf8_to_utf16(&s))
                    } else if let Some(Token::StringLit(s)) = tokens.first().map(|t| t.token.clone()) {
                        *index += 1;
                        Expr::StringLit(s)
                    } else if let Some(Token::Default) = tokens.first().map(|t| t.token.clone()) {
                        // allow the reserved word `default` as an object property key
                        *index += 1;
                        Expr::StringLit(crate::unicode::utf8_to_utf16("default"))
                    } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                        // Computed key (e.g., get [Symbol.toPrimitive]())
                        *index += 1; // consume [
                        let expr = parse_assignment(tokens, index)?;
                        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        *index += 1; // consume ]
                        expr
                    } else {
                        return Err(raise_parse_error_at(tokens));
                    };

                    // Check for method definition after computed key
                    if !is_getter && !is_setter && *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                        *index += 1; // consume (
                        let params = parse_parameters(tokens, index)?;
                        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        *index += 1; // consume {
                        let body = parse_statements(tokens, index)?;
                        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        *index += 1; // consume }
                        properties.push((key_expr, Expr::Function(None, params, body), true));

                        // After adding method, skip any newline/semicolons and handle comma/end in outer loop
                        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator | Token::Semicolon) {
                            *index += 1;
                        }
                        if *index >= tokens.len() {
                            return Err(raise_parse_error_at(tokens));
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
                            return Err(raise_parse_error_at(tokens));
                        }
                        *index += 1; // consume (
                        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        *index += 1; // consume )
                        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        *index += 1; // consume {
                        let body = parse_statements(tokens, index)?;
                        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        *index += 1; // consume }
                        properties.push((key_expr, Expr::Getter(Box::new(Expr::Function(None, Vec::new(), body))), false));
                    } else if is_setter {
                        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LParen) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        *index += 1; // consume (
                        let params = parse_parameters(tokens, index)?;
                        if params.len() != 1 {
                            return Err(raise_parse_error_at(tokens));
                        }
                        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        *index += 1; // consume {
                        let body = parse_statements(tokens, index)?;
                        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        *index += 1; // consume }
                        properties.push((key_expr, Expr::Setter(Box::new(Expr::Function(None, params, body))), false));
                    } else {
                        // Regular property
                        if *index < tokens.len() && matches!(tokens[*index].token, Token::Colon) {
                            *index += 1; // consume :
                            let value = parse_assignment(tokens, index)?;
                            properties.push((key_expr, value, false));
                        } else {
                            // Shorthand property { x } -> { x: x }
                            if is_shorthand_candidate {
                                if let Expr::StringLit(s) = &key_expr {
                                    let name = utf16_to_utf8(s);
                                    properties.push((key_expr, Expr::Var(name, None, None), false));
                                } else {
                                    return Err(raise_parse_error_at(tokens));
                                }
                            } else {
                                return Err(raise_parse_error_at(tokens));
                            }
                        }
                    }
                }

                // Handle comma
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
                    *index += 1;
                }
            }
            Expr::Object(properties)
        }
        Token::LBracket => {
            // Parse array literal
            log::trace!(
                "parse_primary: starting array literal; next tokens (first 12): {:?}",
                tokens.iter().take(12).collect::<Vec<_>>()
            );
            let mut elements = Vec::new();
            if *index < tokens.len() && matches!(tokens[*index].token, Token::RBracket) {
                // Empty array []
                *index += 1; // consume ]
                return Ok(Expr::Array(elements));
            }
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

                log::trace!("parse_primary: array element next token: {:?}", tokens.first());
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
                    return Err(raise_parse_error_at(tokens));
                }
                if matches!(tokens[*index].token, Token::RBracket) {
                    *index += 1; // consume ]
                    break;
                } else if matches!(tokens[*index].token, Token::Comma) {
                    *index += 1; // consume ,
                } else {
                    return Err(raise_parse_error_at(tokens));
                }
            }
            Expr::Array(elements)
        }
        Token::Function | Token::FunctionStar => {
            let is_generator = matches!(current, Token::FunctionStar);
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
                if let Token::Identifier(n) = &tokens[*index].token {
                    // Look ahead for next non-LineTerminator token
                    let mut idx = 1usize;
                    while idx < tokens.len() && matches!(tokens[idx].token, Token::LineTerminator) {
                        idx += 1;
                    }
                    if idx < tokens.len() && matches!(tokens[idx].token, Token::LParen) {
                        let name = n.clone();
                        log::trace!("parse_primary: treating '{}' as function name", name);
                        *index += 1;
                        Some(name)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            // Now expect parameter list. Be forgiving if the '(' was consumed
            // earlier; accept either an explicit '(' or start directly at an
            // identifier (first parameter) or an immediate ')' for empty params.
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
                    return Err(raise_parse_error_at(tokens));
                }
                *index += 1; // consume {
                let body = parse_statements(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                    return Err(raise_parse_error_at(tokens));
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
                    return Err(raise_parse_error_at(tokens));
                }
                *index += 1; // consume {
                let body = parse_statements(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                    return Err(raise_parse_error_at(tokens));
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
                return Err(raise_parse_error_at(tokens));
            }
        }
        Token::Async => {
            // Check if followed by function or arrow function parameters
            if *index < tokens.len() && matches!(tokens[*index].token, Token::Function) {
                *index += 1; // consume function
                // Optional name for async function expressions (same rules as normal functions)
                let name = if *index < tokens.len() {
                    if let Token::Identifier(n) = &tokens[*index].token {
                        // Look ahead for next non-LineTerminator token
                        let mut idx = 1usize;
                        while idx < tokens.len() && matches!(tokens[idx].token, Token::LineTerminator) {
                            idx += 1;
                        }
                        if idx < tokens.len() && matches!(tokens[idx].token, Token::LParen) {
                            let name = n.clone();
                            log::trace!("parse_primary: treating '{}' as async function name", name);
                            *index += 1;
                            Some(name)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                    *index += 1; // consume "("
                    let params = parse_parameters(tokens, index)?;
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    *index += 1; // consume {
                    let body = parse_statements(tokens, index)?;
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    *index += 1; // consume }
                    Expr::AsyncFunction(name, params, body)
                } else {
                    return Err(raise_parse_error_at(tokens));
                }
            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LParen) {
                // Async arrow function
                *index += 1; // consume (
                let mut params: Vec<DestructuringElement> = Vec::new();
                let mut is_arrow = false;
                if matches!(tokens.first().map(|t| &t.token), Some(&Token::RParen)) {
                    *index += 1;
                    if *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                        *index += 1;
                        is_arrow = true;
                    } else {
                        return Err(raise_parse_error_at(tokens));
                    }
                } else {
                    // Try to parse params
                    let mut param_names: Vec<DestructuringElement> = Vec::new();
                    let mut valid = true;
                    loop {
                        if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
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
                        return Err(raise_parse_error_at(&tokens[*index..]));
                    }
                    params = param_names;
                }
                if is_arrow {
                    // For async arrow functions, we need to create a special async closure
                    // For now, we'll treat them as regular arrow functions but mark them as async
                    // This will need to be handled in evaluation
                    Expr::AsyncArrowFunction(params, parse_arrow_body(tokens, index)?)
                } else {
                    return Err(raise_parse_error_at(tokens));
                }
            } else {
                return Err(raise_parse_error_at(tokens));
            }
        }
        Token::LParen => {
            // Check if it's arrow function
            let mut params: Vec<DestructuringElement> = Vec::new();
            let mut is_arrow = false;
            let mut result_expr = None;
            if matches!(tokens.first().map(|t| &t.token), Some(&Token::RParen)) {
                *index += 1;
                if *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                    *index += 1;
                    is_arrow = true;
                } else {
                    return Err(raise_parse_error_at(tokens));
                }
            } else {
                // Try to parse params
                let mut param_names: Vec<DestructuringElement> = Vec::new();
                let mut valid = true;
                loop {
                    log::trace!(
                        "parse_primary LParen param loop: tokens first={:?} local_consumed_len={} param_names_len={}",
                        tokens.first(),
                        param_names.len(),
                        param_names.len()
                    );
                    if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                        *index += 1;
                        param_names.push(DestructuringElement::Variable(name, None));
                        if *index >= tokens.len() {
                            valid = false;
                            break;
                        }

                        // Support default initializers in parameter lists: identifier '=' expression
                        if matches!(tokens[*index].token, Token::Assign) {
                            // Speculatively parse the default expression on a clone so we can
                            // rollback if this isn't actually an arrow parameter list.
                            let tmp = &tokens[*index + 1..];
                            if parse_expression(tmp, &mut 0).is_ok() {
                                // After parsing the default expression, tmp should start with
                                // either ',' or ')' for a valid parameter list.
                                if !tmp.is_empty() && (matches!(tmp[0].token, Token::Comma) || matches!(tmp[0].token, Token::RParen)) {
                                    // consume the same tokens from the real tokens vector and
                                    // record them for possible rollback
                                    let consumed = tokens.len() - tmp.len();
                                    for _ in 0..consumed {}
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
                                        continue;
                                    } else {
                                        valid = false;
                                        break;
                                    }
                                } else {
                                    valid = false;
                                    break;
                                }
                            } else {
                                valid = false;
                                break;
                            }
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
                    } else if tokens.get(*index).map(|t| &t.token) == Some(&Token::LBrace) {
                        let start = *index;
                        if let Ok(pattern) = parse_object_destructuring_pattern(tokens, index) {
                            param_names.push(DestructuringElement::NestedObject(pattern));
                            // } else {
                            //     *index = start;
                            //     valid = false;
                            //     break;
                            // }

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
                            } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
                                *index += 1;
                            } else {
                                valid = false;
                                break;
                            }
                        } else {
                            valid = false;
                            break;
                        }
                    } else if tokens.get(*index).map(|t| &t.token) == Some(&Token::LBracket) {
                        let start = *index;
                        if let Ok(pattern) = parse_array_destructuring_pattern(tokens, index) {
                            param_names.push(DestructuringElement::NestedArray(pattern));
                            // } else {
                            //     *index = start;
                            //     valid = false;
                            //     break;
                            // }

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
                    } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Spread) {
                        // Handle rest parameter: ...args
                        *index += 1;
                        if let Some(Token::Identifier(name)) = tokens.get(*index).map(|t| t.token.clone()) {
                            *index += 1;
                            // Rest parameter must be the last one, so expect RParen
                            if *index < tokens.len() && matches!(tokens[*index].token, Token::RParen) {
                                *index += 1;
                                if *index < tokens.len() && matches!(tokens[*index].token, Token::Arrow) {
                                    *index += 1;
                                    is_arrow = true;
                                    // Store rest param as a regular param for now, but we might need
                                    // to mark it specially in the AST if we want strict validation.
                                    // For now, just treating it as a param named "...name" or similar
                                    // isn't quite right because the AST expects (String, Option<Expr>).
                                    // We'll store it as the name, but the evaluator needs to know it's a rest param.
                                    // A common hack if AST doesn't support it is to prefix the name,
                                    // but let's see if we can just support it by convention or if we need AST changes.
                                    // For this fix, we'll just accept it and maybe the evaluator handles it?
                                    // Actually, the evaluator likely doesn't support rest params in arrow functions yet
                                    // if the AST doesn't have a way to represent them.
                                    // Let's check `Expr::ArrowFunction`. It takes `Vec<(String, Option<Box<Expr>>)>`.
                                    // We might need to change the AST or use a convention.
                                    // Let's try using a convention for now: if name starts with "...", it's a rest param?
                                    // Or better, just pass it through and see if we can handle it.
                                    // Wait, `parse_parameters` handles rest params?
                                    // Let's check `parse_parameters`.
                                    param_names.push(DestructuringElement::Rest(name)); // We need to flag this as rest!
                                // But `Expr::ArrowFunction` signature is `Vec<(String, Option<Box<Expr>>)>`.
                                // It doesn't seem to have a separate field for rest param.
                                // However, `FunctionDeclaration` also uses `Vec<(String, Option<Box<Expr>>)>`.
                                // If `parse_parameters` supports rest, how does it return it?
                                // `parse_parameters` returns `Result<Vec<(String, Option<Box<Expr>>)>, JSError>`.
                                // It seems it DOES NOT support rest parameters in its return type signature either!
                                // Let's look at `parse_parameters` implementation.
                                } else {
                                    valid = false;
                                }
                                break;
                            } else {
                                valid = false;
                                break;
                            }
                        } else {
                            valid = false;
                            break;
                        }
                    } else {
                        valid = false;
                        break;
                    }
                }
                if valid && is_arrow {
                    params = param_names;
                } else {
                    // Parse as expression
                    let expr_inner = parse_expression(tokens, index)?;
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                        return Err(raise_parse_error_at(&tokens[*index..]));
                    }
                    *index += 1;
                    result_expr = Some(expr_inner);
                }
            }
            if is_arrow {
                Expr::ArrowFunction(params, parse_arrow_body(tokens, index)?)
            } else {
                result_expr.unwrap()
            }
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
            return Err(raise_parse_error_at(&tokens[*index - 1..]));
        }
    };

    // Handle postfix operators like index access. Accept line terminators
    // between the primary and the postfix operator to support call-chains
    // split across lines (e.g. `promise.then(...)
    // .then(...)`).
    while *index < tokens.len() {
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }
        if *index >= tokens.len() {
            break;
        }
        match &tokens[*index].token {
            Token::LBracket => {
                *index += 1; // consume '['
                let index_expr = parse_expression(tokens, index)?;
                if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                    return Err(raise_parse_error_at(tokens));
                }
                *index += 1; // consume ']'
                expr = Expr::Index(Box::new(expr), Box::new(index_expr));
            }
            Token::Dot => {
                *index += 1; // consume '.'
                if *index >= tokens.len() {
                    return Err(raise_parse_error_at(tokens));
                }
                if let Some(prop) = tokens[*index].token.as_identifier_string() {
                    *index += 1;
                    expr = Expr::Property(Box::new(expr), prop);
                } else if let Token::PrivateIdentifier(prop) = &tokens[*index].token {
                    // Private identifiers (e.g. `obj.#x`) are only syntactically
                    // valid inside class bodies and furthermore the referenced
                    // private name must have been *declared* in the *enclosing*
                    // class. Check the top-most declared private-name set.
                    let invalid = PRIVATE_NAME_STACK.with(|s| s.borrow().last().map(|rc| !rc.borrow().contains(prop)).unwrap_or(true));
                    if invalid {
                        let msg = format!("Private field '#{}' must be declared in an enclosing class", prop);
                        return Err(raise_parse_error_with_token!(tokens[*index], msg));
                    }
                    let prop = format!("#{}", prop);
                    *index += 1;
                    expr = Expr::Property(Box::new(expr), prop);
                } else {
                    return Err(raise_parse_error_at(tokens));
                }
            }
            Token::OptionalChain => {
                *index += 1; // consume '?.'
                if *index >= tokens.len() {
                    return Err(raise_parse_error_at(tokens));
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
                                return Err(raise_parse_error_at(tokens));
                            }
                            if matches!(tokens[*index].token, Token::RParen) {
                                break;
                            }
                            if !matches!(tokens[*index].token, Token::Comma) {
                                return Err(raise_parse_error_at(tokens));
                            }
                            *index += 1; // consume ','
                        }
                    }
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    *index += 1; // consume ')'
                    expr = Expr::OptionalCall(Box::new(expr), args);
                } else if matches!(tokens[*index].token, Token::Identifier(_)) {
                    // Optional property access: obj?.prop
                    if let Some(prop) = tokens[*index].token.as_identifier_string() {
                        *index += 1;
                        expr = Expr::OptionalProperty(Box::new(expr), prop);
                    } else {
                        return Err(raise_parse_error_at(tokens));
                    }
                } else if matches!(tokens[*index].token, Token::LBracket) {
                    // Optional computed property access: obj?.[expr]
                    *index += 1; // consume '['
                    let index_expr = parse_expression(tokens, index)?;
                    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                        return Err(raise_parse_error_at(tokens));
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
                                    return Err(raise_parse_error_at(tokens));
                                }
                                if matches!(tokens[*index].token, Token::RParen) {
                                    break;
                                }
                                if !matches!(tokens[*index].token, Token::Comma) {
                                    return Err(raise_parse_error_at(tokens));
                                }
                                *index += 1; // consume ','
                            }
                        }
                        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RParen) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        *index += 1; // consume ')'
                        // Flatten accidental single-Comma argument
                        if args.len() == 1 {
                            if let Expr::Comma(_, _) = &args[0] {
                                let first = args.remove(0);
                                let new_args = flatten_commas(first);
                                args.extend(new_args);
                            }
                        }
                        expr = Expr::OptionalCall(Box::new(Expr::Index(Box::new(expr), Box::new(index_expr))), args);
                    } else {
                        expr = Expr::OptionalIndex(Box::new(expr), Box::new(index_expr));
                    }
                } else {
                    return Err(raise_parse_error_at(tokens));
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
                            return Err(raise_parse_error_at(tokens));
                        }
                        if matches!(tokens[*index].token, Token::RParen) {
                            break;
                        }
                        if !matches!(tokens[*index].token, Token::Comma) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        *index += 1; // consume ','
                        // allow trailing comma before ')' and skip newlines
                        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
                            *index += 1;
                        }
                        if *index >= tokens.len() {
                            return Err(raise_parse_error_at(tokens));
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
                    return Err(raise_parse_error_at(tokens));
                }
                *index += 1; // consume ')'
                // Flatten accidental single-Comma argument
                if args.len() == 1 {
                    if let Expr::Comma(_, _) = &args[0] {
                        let first = args.remove(0);
                        let new_args = flatten_commas(first);
                        args.extend(new_args);
                    }
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
                let mut strings = Vec::new();
                let mut exprs = Vec::new();
                for part in parts {
                    match part {
                        TemplatePart::String(s) => strings.push(s.clone()),
                        TemplatePart::Expr(expr_tokens) => {
                            let expr_tokens = expr_tokens.clone();
                            let e = parse_expression(&expr_tokens, &mut 0)?;
                            exprs.push(e);
                        }
                    }
                }
                expr = Expr::TaggedTemplate(Box::new(expr), strings, exprs);
            }
            _ => break,
        }
    }

    Ok(expr)
}

fn parse_arrow_body(tokens: &[TokenData], index: &mut usize) -> Result<Vec<Statement>, JSError> {
    if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
        *index += 1;
        let body = parse_statements(tokens, index)?;
        if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
            return Err(raise_parse_error_at(tokens));
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
        return Err(raise_parse_error_at(tokens));
    }
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
            if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                *index += 1;
                pattern.push(DestructuringElement::Rest(name));
            } else {
                return Err(raise_parse_error_at(tokens));
            }
            // Rest must be the last element
            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBracket) {
                return Err(raise_parse_error_at(tokens));
            }
            *index += 1; // consume ]
            break;
        } else if *index < tokens.len() && matches!(tokens[*index].token, Token::Comma) {
            pattern.push(DestructuringElement::Empty);
        } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
            // Nested array destructuring
            let nested_pattern = parse_array_destructuring_pattern(tokens, index)?;
            pattern.push(DestructuringElement::NestedArray(nested_pattern));
        } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
            // Nested object destructuring
            let nested_pattern = parse_object_destructuring_pattern(tokens, index)?;
            pattern.push(DestructuringElement::NestedObject(nested_pattern));
        } else if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
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
                    let expr = parse_expression(&tmp, &mut 0)?;
                    default_expr = Some(Box::new(expr));
                }
            }
            pattern.push(DestructuringElement::Variable(name, default_expr));
        } else {
            return Err(raise_parse_error_at(tokens));
        }

        // allow blank lines between last element and closing brace
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }

        if *index >= tokens.len() {
            return Err(raise_parse_error_at(tokens));
        }
        if matches!(tokens[*index].token, Token::RBracket) {
            *index += 1; // consume ]
            break;
        } else if matches!(tokens[*index].token, Token::Comma) {
            *index += 1; // consume ,
        } else {
            return Err(raise_parse_error_at(tokens));
        }
    }

    Ok(pattern)
}

pub fn parse_object_destructuring_pattern(tokens: &[TokenData], index: &mut usize) -> Result<Vec<DestructuringElement>, JSError> {
    if *index >= tokens.len() || !matches!(tokens[*index].token, Token::LBrace) {
        return Err(raise_parse_error_at(tokens));
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
            if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                *index += 1;
                pattern.push(DestructuringElement::Rest(name));
            } else {
                return Err(raise_parse_error_at(tokens));
            }
            // Rest must be the last element
            if *index >= tokens.len() || !matches!(tokens[*index].token, Token::RBrace) {
                return Err(raise_parse_error_at(tokens));
            }
            *index += 1; // consume }
            break;
        } else {
            // Parse property
            let key = if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                *index += 1;
                name
            } else {
                log::trace!(
                    "parse_object_destructuring_pattern: expected Identifier for property key but got {:?}",
                    tokens.first()
                );
                return Err(raise_parse_error_at(tokens));
            };

            let value = if *index < tokens.len() && matches!(tokens[*index].token, Token::Colon) {
                *index += 1; // consume :
                // Parse the value pattern
                if *index < tokens.len() && matches!(tokens[*index].token, Token::LBracket) {
                    DestructuringElement::NestedArray(parse_array_destructuring_pattern(tokens, index)?)
                } else if *index < tokens.len() && matches!(tokens[*index].token, Token::LBrace) {
                    DestructuringElement::NestedObject(parse_object_destructuring_pattern(tokens, index)?)
                } else if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
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
                    return Err(raise_parse_error_at(tokens));
                }
            } else {
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
                DestructuringElement::Variable(key.clone(), default_expr)
            };

            pattern.push(DestructuringElement::Property(key, Box::new(value)));
        }

        // allow whitespace / blank lines before separators or closing brace
        while *index < tokens.len() && matches!(tokens[*index].token, Token::LineTerminator) {
            *index += 1;
        }

        if *index >= tokens.len() {
            return Err(raise_parse_error_at(tokens));
        }
        if matches!(tokens[*index].token, Token::RBrace) {
            *index += 1; // consume }
            break;
        } else if matches!(tokens[*index].token, Token::Comma) {
            *index += 1; // consume ,
        } else {
            return Err(raise_parse_error_at(tokens));
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

        match &stmts[0].kind {
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
}
