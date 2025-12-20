use crate::{
    JSError,
    core::{
        DestructuringElement, Expr, ObjectDestructuringElement, Token, TokenData, parse_array_destructuring_pattern, parse_assignment,
        parse_expression, parse_object_destructuring_pattern, parse_parameters, parse_statement_block,
    },
    js_class::ClassMember,
    raise_parse_error, raise_parse_error_with_token,
};

fn raise_parse_error_at(tokens: &[TokenData]) -> JSError {
    if let Some(t) = tokens.first() {
        raise_parse_error_with_token!(t)
    } else {
        raise_parse_error!()
    }
}

#[derive(Clone, Debug)]
pub enum SwitchCase {
    Case(Expr, Vec<Statement>), // case value, statements
    Default(Vec<Statement>),    // default statements
}

#[derive(Clone, Debug)]
pub enum ImportSpecifier {
    Default(String),               // import name from "module"
    Named(String, Option<String>), // import { name as alias } from "module"
    Namespace(String),             // import * as name from "module"
}

#[derive(Clone, Debug)]
pub enum ExportSpecifier {
    Named(String, Option<String>), // export { name as alias }
    Default(Expr),                 // export default value
}

#[derive(Clone, Debug)]
pub struct Statement {
    pub kind: StatementKind,
    pub line: usize,
    pub column: usize,
}

impl From<StatementKind> for Statement {
    fn from(kind: StatementKind) -> Self {
        Statement { kind, line: 0, column: 0 }
    }
}

#[derive(Clone)]
pub enum StatementKind {
    Let(Vec<(String, Option<Expr>)>),
    Var(Vec<(String, Option<Expr>)>),
    Const(Vec<(String, Expr)>),
    FunctionDeclaration(String, Vec<DestructuringElement>, Vec<Statement>, bool), // name, params, body, is_generator
    LetDestructuringArray(Vec<DestructuringElement>, Expr),                       // array destructuring: let [a, b] = [1, 2];
    VarDestructuringArray(Vec<DestructuringElement>, Expr),                       // array destructuring: var [a, b] = [1, 2];
    ConstDestructuringArray(Vec<DestructuringElement>, Expr),                     // const [a, b] = [1, 2];
    LetDestructuringObject(Vec<ObjectDestructuringElement>, Expr),                // object destructuring: let {a, b} = {a: 1, b: 2};
    VarDestructuringObject(Vec<ObjectDestructuringElement>, Expr),                // object destructuring: var {a, b} = {a: 1, b: 2};
    ConstDestructuringObject(Vec<ObjectDestructuringElement>, Expr),              // const {a, b} = {a: 1, b: 2};
    Class(String, Option<crate::core::Expr>, Vec<ClassMember>),                   // name, extends, members
    Assign(String, Expr),                                                         // variable assignment
    Expr(Expr),
    Return(Option<Expr>),
    If(Expr, Vec<Statement>, Option<Vec<Statement>>), // condition, then_body, else_body
    For(Option<Box<Statement>>, Option<Expr>, Option<Box<Statement>>, Vec<Statement>), // init, condition, increment, body
    ForOf(String, Expr, Vec<Statement>),              // variable, iterable, body
    ForIn(String, Expr, Vec<Statement>),              // variable, object, body
    ForOfDestructuringObject(Vec<ObjectDestructuringElement>, Expr, Vec<Statement>), // var { .. } of iterable
    ForOfDestructuringArray(Vec<DestructuringElement>, Expr, Vec<Statement>), // var [ .. ] of iterable
    While(Expr, Vec<Statement>),                      // condition, body
    DoWhile(Vec<Statement>, Expr),                    // body, condition
    Switch(Expr, Vec<SwitchCase>),                    // expression, cases
    Block(Vec<Statement>),                            // block statement `{ ... }`
    Break(Option<String>),
    Continue(Option<String>),
    Label(String, Box<Statement>),
    TryCatch(Vec<Statement>, String, Vec<Statement>, Option<Vec<Statement>>), // try_body, catch_param, catch_body, finally_body
    Throw(Expr),                                                              // throw expression
    Import(Vec<ImportSpecifier>, String),                                     // import specifiers, module name
    Export(Vec<ExportSpecifier>, Option<Box<Statement>>),                     // export specifiers, optional inner declaration
}

impl std::fmt::Debug for StatementKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StatementKind::Let(decls) => write!(f, "Let({:?})", decls),
            StatementKind::Var(decls) => write!(f, "Var({:?})", decls),
            StatementKind::Const(decls) => write!(f, "Const({:?})", decls),
            StatementKind::FunctionDeclaration(name, params, body, is_gen) => {
                write!(f, "FunctionDeclaration({}, {:?}, {:?}, {})", name, params, body, is_gen)
            }
            StatementKind::LetDestructuringArray(pattern, expr) => write!(f, "LetDestructuringArray({:?}, {:?})", pattern, expr),
            StatementKind::VarDestructuringArray(pattern, expr) => write!(f, "VarDestructuringArray({:?}, {:?})", pattern, expr),
            StatementKind::ConstDestructuringArray(pattern, expr) => write!(f, "ConstDestructuringArray({:?}, {:?})", pattern, expr),
            StatementKind::LetDestructuringObject(pattern, expr) => write!(f, "LetDestructuringObject({:?}, {:?})", pattern, expr),
            StatementKind::VarDestructuringObject(pattern, expr) => write!(f, "VarDestructuringObject({:?}, {:?})", pattern, expr),
            StatementKind::ConstDestructuringObject(pattern, expr) => write!(f, "ConstDestructuringObject({:?}, {:?})", pattern, expr),
            StatementKind::Class(name, extends, members) => write!(f, "Class({name}, {extends:?}, {members:?})"),
            StatementKind::Assign(var, expr) => write!(f, "Assign({}, {:?})", var, expr),
            StatementKind::Expr(expr) => write!(f, "Expr({:?})", expr),
            StatementKind::Return(Some(expr)) => write!(f, "Return({:?})", expr),
            StatementKind::Return(None) => write!(f, "Return(None)"),
            StatementKind::If(cond, then_body, else_body) => {
                write!(f, "If({:?}, {:?}, {:?})", cond, then_body, else_body)
            }
            StatementKind::For(init, cond, incr, body) => {
                write!(f, "For({:?}, {:?}, {:?}, {:?})", init, cond, incr, body)
            }
            StatementKind::ForOf(var, iterable, body) => {
                write!(f, "ForOf({}, {:?}, {:?})", var, iterable, body)
            }
            StatementKind::ForIn(var, object, body) => {
                write!(f, "ForIn({}, {:?}, {:?})", var, object, body)
            }
            StatementKind::ForOfDestructuringObject(pat, iterable, body) => {
                write!(f, "ForOfDestructuringObject({:?}, {:?}, {:?})", pat, iterable, body)
            }
            StatementKind::ForOfDestructuringArray(pat, iterable, body) => {
                write!(f, "ForOfDestructuringArray({:?}, {:?}, {:?})", pat, iterable, body)
            }
            StatementKind::While(cond, body) => {
                write!(f, "While({:?}, {:?})", cond, body)
            }
            StatementKind::DoWhile(body, cond) => {
                write!(f, "DoWhile({:?}, {:?})", body, cond)
            }
            StatementKind::Switch(expr, cases) => {
                write!(f, "Switch({:?}, {:?})", expr, cases)
            }
            StatementKind::Break(None) => write!(f, "Break"),
            StatementKind::Break(Some(lbl)) => write!(f, "Break({})", lbl),
            StatementKind::Continue(None) => write!(f, "Continue"),
            StatementKind::Continue(Some(lbl)) => write!(f, "Continue({})", lbl),
            StatementKind::Label(name, stmt) => write!(f, "Label({}, {:?})", name, stmt),
            StatementKind::Block(stmts) => write!(f, "Block({:?})", stmts),
            StatementKind::TryCatch(try_body, catch_param, catch_body, finally_body) => {
                write!(f, "TryCatch({:?}, {}, {:?}, {:?})", try_body, catch_param, catch_body, finally_body)
            }
            StatementKind::Throw(expr) => {
                write!(f, "Throw({:?})", expr)
            }
            StatementKind::Import(specifiers, module) => {
                write!(f, "Import({:?}, {})", specifiers, module)
            }
            StatementKind::Export(specifiers, maybe_decl) => {
                if let Some(decl) = maybe_decl {
                    write!(f, "Export({:?}, {:?})", specifiers, decl)
                } else {
                    write!(f, "Export({:?})", specifiers)
                }
            }
        }
    }
}

pub fn parse_statements(tokens: &mut Vec<TokenData>) -> Result<Vec<Statement>, JSError> {
    let mut statements = Vec::new();
    while !tokens.is_empty() && !matches!(tokens[0].token, Token::RBrace) {
        // Skip empty statement semicolons that may appear (e.g. from inserted tokens)
        if matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
            tokens.remove(0);
            continue;
        }
        log::trace!("parse_statements next token: {:?}", tokens.first());
        // Keep a snapshot of tokens so we can log the original context in case
        // parse_statement mutates the token stream before returning an error.
        let tokens_snapshot = tokens.clone();
        let stmt = match parse_statement(tokens) {
            Ok(s) => s,
            Err(e) => {
                // Provide context for parse errors to aid debugging large harness files.
                // Use the snapshot (pre-parse) so partial consumption doesn't hide
                // the original tokens at the start of the statement.
                log::warn!("parse_statements error at token start. remaining tokens (first 40):");
                for (i, t) in tokens_snapshot.iter().take(40).enumerate() {
                    log::warn!("  {}: {:?}", i, t);
                }
                return Err(e);
            }
        };
        statements.push(stmt);
        if !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
            tokens.remove(0);
        }
    }
    Ok(statements)
}

pub fn parse_statement(tokens: &mut Vec<TokenData>) -> Result<Statement, JSError> {
    // Skip any leading line terminators so statements inside blocks (e.g., cases)
    // can contain blank lines or comments that emitted line terminators.
    while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
        tokens.remove(0);
    }

    let (line, column) = if let Some(t) = tokens.first() { (t.line, t.column) } else { (0, 0) };

    let kind = parse_statement_kind(tokens)?;
    Ok(Statement { kind, line, column })
}

pub fn parse_statement_kind(tokens: &mut Vec<TokenData>) -> Result<StatementKind, JSError> {
    // Skip any leading line terminators so statements inside blocks (e.g., cases)
    // can contain blank lines or comments that emitted line terminators.
    while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
        tokens.remove(0);
    }
    // This function parses a single statement from the token stream.
    // It handles various types of statements including break, continue, and return.

    // Import statement (static imports only)
    if !tokens.is_empty() && matches!(tokens[0].token, Token::Import) {
        // Check if this is a dynamic import (import followed by parenthesis)
        // If so, it's an expression, not a statement - fall through to expression parsing
        if tokens.len() > 1 && matches!(tokens[1].token, Token::LParen) {
            // This is a dynamic import expression, not a statement
        } else {
            tokens.remove(0); // consume import
            let mut specifiers = Vec::new();

            // Parse import specifiers
            if !tokens.is_empty() && matches!(tokens[0].token, Token::Multiply) {
                // import * as name
                tokens.remove(0); // consume *
                if tokens.is_empty() || !matches!(tokens[0].token, Token::As) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume as
                if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                    if let Token::Identifier(_) = tokens[0].token {
                        tokens.remove(0);
                    }
                    specifiers.push(ImportSpecifier::Namespace(name));
                } else {
                    return Err(raise_parse_error_at(tokens));
                }
            } else if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
                // import { ... }
                tokens.remove(0); // consume {
                while !tokens.is_empty() && !matches!(tokens[0].token, Token::RBrace) {
                    if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                        tokens.remove(0);
                        let alias = if !tokens.is_empty() && matches!(tokens[0].token, Token::As) {
                            tokens.remove(0); // consume as
                            if let Some(Token::Identifier(alias_name)) = tokens.first().map(|t| t.token.clone()) {
                                tokens.remove(0);
                                Some(alias_name)
                            } else {
                                return Err(raise_parse_error_at(tokens));
                            }
                        } else {
                            None
                        };
                        specifiers.push(ImportSpecifier::Named(name, alias));

                        if !tokens.is_empty() && matches!(tokens[0].token, Token::Comma) {
                            tokens.remove(0); // consume ,
                        } else if !matches!(tokens[0].token, Token::RBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                    } else {
                        return Err(raise_parse_error_at(tokens));
                    }
                }
                if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume }
            } else if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                // import name
                if let Token::Identifier(_) = tokens[0].token {
                    tokens.remove(0);
                }
                specifiers.push(ImportSpecifier::Default(name));

                // Check for comma followed by named imports
                if !tokens.is_empty() && matches!(tokens[0].token, Token::Comma) {
                    tokens.remove(0); // consume ,
                    if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    tokens.remove(0); // consume {
                    while !tokens.is_empty() && !matches!(tokens[0].token, Token::RBrace) {
                        if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                            tokens.remove(0);
                            let alias = if !tokens.is_empty() && matches!(tokens[0].token, Token::As) {
                                tokens.remove(0); // consume as
                                if let Some(Token::Identifier(alias_name)) = tokens.first().map(|t| t.token.clone()) {
                                    tokens.remove(0);
                                    Some(alias_name)
                                } else {
                                    return Err(raise_parse_error_at(tokens));
                                }
                            } else {
                                None
                            };
                            specifiers.push(ImportSpecifier::Named(name, alias));

                            if !tokens.is_empty() && matches!(tokens[0].token, Token::Comma) {
                                tokens.remove(0); // consume ,
                            } else if !matches!(tokens[0].token, Token::RBrace) {
                                return Err(raise_parse_error_at(tokens));
                            }
                        } else {
                            return Err(raise_parse_error_at(tokens));
                        }
                    }
                    if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    tokens.remove(0); // consume }
                }
            }

            // Expect "from"
            if tokens.is_empty() || !matches!(tokens[0].token, Token::Identifier(_)) {
                return Err(raise_parse_error_at(tokens));
            }
            if let Token::Identifier(from_keyword) = tokens.remove(0).token {
                if from_keyword != "from" {
                    return Err(raise_parse_error_at(tokens));
                }
            } else {
                return Err(raise_parse_error_at(tokens));
            }

            // Parse module name
            let module_name = if let Some(Token::StringLit(utf16_chars)) = tokens.first().map(|t| t.token.clone()) {
                tokens.remove(0);
                String::from_utf16(&utf16_chars).map_err(|_| raise_parse_error_at(tokens))?
            } else {
                return Err(raise_parse_error_at(tokens));
            };

            return Ok(StatementKind::Import(specifiers, module_name));
        }
    } // Export statement
    if !tokens.is_empty() && matches!(tokens[0].token, Token::Export) {
        tokens.remove(0); // consume export
        let mut specifiers = Vec::new();

        if !tokens.is_empty() && matches!(tokens[0].token, Token::Default) {
            // export default <expr> or export default function ...
            tokens.remove(0); // consume default
            if !tokens.is_empty() && (matches!(tokens[0].token, Token::Function) || matches!(tokens[0].token, Token::FunctionStar)) {
                // export default function [name]?(...) { ... }
                let is_generator = matches!(tokens[0].token, Token::FunctionStar);
                tokens.remove(0); // consume function or function*
                let _name = if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                    tokens.remove(0);
                    Some(name)
                } else {
                    None
                };
                if tokens.is_empty() || !matches!(tokens[0].token, Token::LParen) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume "("
                let params = crate::core::parser::parse_parameters(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume {
                let body = parse_statements(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume }
                let func_expr = if is_generator {
                    Expr::GeneratorFunction(None, params, body)
                } else {
                    Expr::Function(None, params, body)
                };
                specifiers.push(ExportSpecifier::Default(func_expr));
            } else {
                // export default <expr>
                let expr = parse_expression(tokens)?;
                specifiers.push(ExportSpecifier::Default(expr));
            }
        } else if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
            // export { ... }
            tokens.remove(0); // consume {
            while !tokens.is_empty() && !matches!(tokens[0].token, Token::RBrace) {
                if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                    tokens.remove(0);
                    let alias = if !tokens.is_empty() && matches!(tokens[0].token, Token::As) {
                        tokens.remove(0); // consume as
                        if let Some(Token::Identifier(alias_name)) = tokens.first().map(|t| t.token.clone()) {
                            tokens.remove(0);
                            Some(alias_name)
                        } else {
                            return Err(raise_parse_error_at(tokens));
                        }
                    } else {
                        None
                    };
                    specifiers.push(ExportSpecifier::Named(name, alias));

                    if !tokens.is_empty() && matches!(tokens[0].token, Token::Comma) {
                        tokens.remove(0); // consume ,
                    } else if !matches!(tokens[0].token, Token::RBrace) {
                        return Err(raise_parse_error_at(tokens));
                    }
                } else {
                    return Err(raise_parse_error_at(tokens));
                }
            }
            if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume }
        } else {
            // export <declaration> (const, let, var, function, class)
            let stmt = parse_statement(tokens)?;
            match stmt.clone().kind {
                StatementKind::Const(decls) => {
                    for (name, _) in decls {
                        specifiers.push(ExportSpecifier::Named(name, None));
                    }
                    return Ok(StatementKind::Export(specifiers, Some(Box::new(stmt))));
                }
                StatementKind::Let(decls) => {
                    for (name, _) in decls {
                        specifiers.push(ExportSpecifier::Named(name, None));
                    }
                    return Ok(StatementKind::Export(specifiers, Some(Box::new(stmt))));
                }
                StatementKind::Var(decls) => {
                    for (name, _) in decls {
                        specifiers.push(ExportSpecifier::Named(name, None));
                    }
                    return Ok(StatementKind::Export(specifiers, Some(Box::new(stmt))));
                }
                StatementKind::Class(name, _, _) => {
                    specifiers.push(ExportSpecifier::Named(name, None));
                    return Ok(StatementKind::Export(specifiers, Some(Box::new(stmt))));
                }
                StatementKind::FunctionDeclaration(name, _, _, _) => {
                    specifiers.push(ExportSpecifier::Named(name, None));
                    return Ok(StatementKind::Export(specifiers, Some(Box::new(stmt))));
                }
                _ => return Err(raise_parse_error_at(tokens)),
            }
        }

        return Ok(StatementKind::Export(specifiers, None));
    }

    // Labelled statement: `label: <statement>`
    if tokens.len() > 1 {
        if let Token::Identifier(name) = tokens[0].token.clone() {
            if matches!(tokens[1].token, Token::Colon) {
                // consume identifier and colon
                tokens.remove(0);
                tokens.remove(0);
                let stmt = parse_statement(tokens)?;
                return Ok(StatementKind::Label(name, Box::new(stmt)));
            }
        }
    }

    // Block statement as a standalone statement
    if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
        tokens.remove(0); // consume {
        let body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume }
        return Ok(StatementKind::Block(body));
    }

    if !tokens.is_empty() && matches!(tokens[0].token, Token::Break) {
        tokens.remove(0); // consume break
        // optional label identifier
        let label = if !tokens.is_empty() {
            if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                tokens.remove(0);
                Some(name)
            } else {
                None
            }
        } else {
            None
        };
        // Accept either an explicit semicolon, or allow automatic semicolon
        // insertion when the next token is a block terminator (e.g. `}`), or
        // part of a try/catch/finally sequence. This keeps `throw <expr>` and
        // `throw <expr>;` equivalent inside blocks.
        if !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
            tokens.remove(0); // consume ;
        } else if !tokens.is_empty()
            && (matches!(tokens[0].token, Token::RBrace)
                || matches!(tokens[0].token, Token::Catch)
                || matches!(tokens[0].token, Token::Finally))
        {
            // semicolon omitted but next token terminates the statement; accept
        } else {
            return Err(raise_parse_error_at(tokens));
        }
        return Ok(StatementKind::Break(label));
    }
    if !tokens.is_empty() && matches!(tokens[0].token, Token::Continue) {
        tokens.remove(0); // consume continue
        // optional label identifier
        let label = if !tokens.is_empty() {
            if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                tokens.remove(0);
                Some(name)
            } else {
                None
            }
        } else {
            None
        };
        if tokens.is_empty() || !matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume ;
        return Ok(StatementKind::Continue(label));
    }
    if !tokens.is_empty() && matches!(tokens[0].token, Token::While) {
        tokens.remove(0); // consume while
        if tokens.is_empty() || !matches!(tokens[0].token, Token::LParen) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume (
        let condition = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume )
        // Body may be a block (`{ ... }`) or a single statement (e.g. `while (...) stmt;`)
        let body = if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
            tokens.remove(0); // consume {
            let b = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume }
            b
        } else {
            // Single-statement body: parse one statement and remove a trailing semicolon if present.
            let s = parse_statement(tokens)?;
            if !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
                tokens.remove(0);
            }
            vec![s]
        };
        return Ok(StatementKind::While(condition, body));
    }
    if !tokens.is_empty() && matches!(tokens[0].token, Token::Do) {
        tokens.remove(0); // consume do
        // Do body may be a block or a single statement
        let body = if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
            tokens.remove(0); // consume {
            let b = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume }
            b
        } else {
            // Single-statement body: parse one statement and remove trailing semicolon if present
            let s = parse_statement(tokens)?;
            if !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
                tokens.remove(0);
            }
            vec![s]
        };
        // Allow intervening line terminators between the do-body and the `while`
        // keyword, e.g. `do\nwhile (...)`.
        while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
            tokens.remove(0);
        }

        if tokens.is_empty() || !matches!(tokens[0].token, Token::While) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume while
        if tokens.is_empty() || !matches!(tokens[0].token, Token::LParen) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume (
        let condition = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume )
        if tokens.is_empty() || !matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume ;
        return Ok(StatementKind::DoWhile(body, condition));
    }
    if !tokens.is_empty() && matches!(tokens[0].token, Token::Switch) {
        tokens.remove(0); // consume switch
        if tokens.is_empty() || !matches!(tokens[0].token, Token::LParen) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume (
        let expr = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume )
        if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume {
        let mut cases = Vec::new();
        while !tokens.is_empty() && !matches!(tokens[0].token, Token::RBrace) {
            // Skip any blank lines inside switch body (LineTerminators coming from tokenizer)
            while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
                tokens.remove(0);
            }
            if tokens.is_empty() || matches!(tokens[0].token, Token::RBrace) {
                break;
            }
            if matches!(tokens[0].token, Token::Case) {
                tokens.remove(0); // consume case
                let case_value = parse_expression(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0].token, Token::Colon) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume :
                let mut case_stmts = Vec::new();
                while !tokens.is_empty()
                    && !matches!(tokens[0].token, Token::Case)
                    && !matches!(tokens[0].token, Token::Default)
                    && !matches!(tokens[0].token, Token::RBrace)
                {
                    // Skip blank lines inside a case body before parsing the next
                    // statement so cases like `case X: \n` fall through properly
                    // rather than trying to parse `}` as a statement.
                    while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
                        tokens.remove(0);
                    }
                    // If skipping blank lines lands us at a case/default/rbrace
                    // there's no statement to parse — break out of the case loop.
                    if tokens.is_empty()
                        || matches!(tokens[0].token, Token::Case)
                        || matches!(tokens[0].token, Token::Default)
                        || matches!(tokens[0].token, Token::RBrace)
                    {
                        break;
                    }
                    log::trace!("switch case parsing stmt, next token = {:?}", tokens.first());
                    let stmt = parse_statement(tokens)?;
                    case_stmts.push(stmt);
                    if !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
                        tokens.remove(0);
                    }
                }
                cases.push(SwitchCase::Case(case_value, case_stmts));
            } else if matches!(tokens[0].token, Token::Default) {
                tokens.remove(0); // consume default
                if tokens.is_empty() || !matches!(tokens[0].token, Token::Colon) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume :
                let mut default_stmts = Vec::new();
                while !tokens.is_empty() && !matches!(tokens[0].token, Token::RBrace) {
                    // Skip leading blank lines inside default branch to avoid
                    // attempting to parse a closing brace as a statement.
                    while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
                        tokens.remove(0);
                    }
                    if tokens.is_empty() || matches!(tokens[0].token, Token::RBrace) {
                        break;
                    }
                    log::trace!("switch default parsing stmt, next token = {:?}", tokens.first());
                    let stmt = parse_statement(tokens)?;
                    default_stmts.push(stmt);
                    if !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
                        tokens.remove(0);
                    }
                }
                cases.push(SwitchCase::Default(default_stmts));
            } else {
                return Err(raise_parse_error_at(tokens));
            }
        }
        if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume }
        return Ok(StatementKind::Switch(expr, cases));
    }
    if !tokens.is_empty() && matches!(tokens[0].token, Token::Throw) {
        tokens.remove(0); // consume throw
        let expr = parse_expression(tokens)?;
        // Accept an explicit semicolon, or allow ASI-ish omission when next token
        // ends the block or begins a catch/finally block.
        if !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
            tokens.remove(0); // consume ;
        } else if !tokens.is_empty()
            && (matches!(tokens[0].token, Token::RBrace)
                || matches!(tokens[0].token, Token::Catch)
                || matches!(tokens[0].token, Token::Finally))
        {
            // semicolon omitted but next token terminates the statement; accept
        } else {
            return Err(raise_parse_error_at(tokens));
        }
        return Ok(StatementKind::Throw(expr));
    }
    if !tokens.is_empty() && matches!(tokens[0].token, Token::Async) {
        tokens.remove(0); // consume async
        if !tokens.is_empty() && matches!(tokens[0].token, Token::Function) {
            tokens.remove(0); // consume function
            if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                tokens.remove(0);
                if !tokens.is_empty() && matches!(tokens[0].token, Token::LParen) {
                    tokens.remove(0); // consume "("
                    let params = crate::core::parser::parse_parameters(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    tokens.remove(0); // consume {
                    let body = parse_statements(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    tokens.remove(0); // consume }
                    return Ok(StatementKind::Let(vec![(
                        name.clone(),
                        Some(Expr::AsyncFunction(Some(name), params, body)),
                    )]));
                }
            }
        }
        return Err(raise_parse_error_at(tokens));
    }
    if !tokens.is_empty() && (matches!(tokens[0].token, Token::Function) || matches!(tokens[0].token, Token::FunctionStar)) {
        let is_generator = matches!(tokens[0].token, Token::FunctionStar);
        tokens.remove(0); // consume function or function*
        log::trace!(
            "parse_statement: entered Function branch; next tokens: {:?}",
            tokens.iter().take(12).collect::<Vec<_>>()
        );
        if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
            tokens.remove(0);
            log::trace!(
                "parse_statement: function name parsed: {} ; remaining: {:?}",
                name,
                tokens.iter().take(12).collect::<Vec<_>>()
            );
            if !tokens.is_empty() && matches!(tokens[0].token, Token::LParen) {
                tokens.remove(0); // consume "("
                let params = crate::core::parser::parse_parameters(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                    return Err(raise_parse_error_at(tokens));
                }
                log::trace!(
                    "parse_statement: function params parsed; entering body parse; remaining tokens: {:?}",
                    tokens.iter().take(12).collect::<Vec<_>>()
                );
                tokens.remove(0); // consume {
                let body = parse_statements(tokens)?;
                log::trace!(
                    "parse_statement: parsed function body, stmt count {} ; remaining tokens: {:?}",
                    body.len(),
                    tokens.iter().take(8).collect::<Vec<_>>()
                );
                if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume }
                return Ok(StatementKind::FunctionDeclaration(name, params, body, is_generator));
            }
        }
    }
    if !tokens.is_empty() && matches!(tokens[0].token, Token::If) {
        tokens.remove(0); // consume if
        if tokens.is_empty() || !matches!(tokens[0].token, Token::LParen) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume (
        let condition = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume )
        // Allow either a block (`{ ... }`) or a single statement as the
        // then-body of an `if` (e.g. `if (cond) stmt;`), matching JavaScript.
        let then_body = if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
            tokens.remove(0); // consume {
            let body = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume }
            body
        } else {
            // Single-statement body: parse a single statement and wrap it.
            let stmt = parse_statement(tokens)?;
            // Consume trailing semicolon after the inner statement if present so
            // `if (cond) stmt; else ...` is handled correctly.
            if !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
                tokens.remove(0);
            }
            vec![stmt]
        };

        // Allow newline(s) between then-body and `else` so `else` can be on the
        // following line (real-world JS commonly formats code like this).
        while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
            tokens.remove(0);
        }

        let else_body = if !tokens.is_empty() && matches!(tokens[0].token, Token::Else) {
            tokens.remove(0); // consume else
            // Support both `else { ... }` and `else if (...) { ... }` forms.
            if !tokens.is_empty() && matches!(tokens[0].token, Token::If) {
                // `else if` form — parse the nested if statement and wrap it
                // as a single-item body.
                let nested_if = parse_statement(tokens)?;
                Some(vec![nested_if])
            } else if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
                // Block body for else
                tokens.remove(0); // consume {
                let body = parse_statements(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume }
                Some(body)
            } else {
                // Single-statement else body (e.g. `else stmt;`)
                let stmt = parse_statement(tokens)?;
                if !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
                    tokens.remove(0);
                }
                Some(vec![stmt])
            }
        } else {
            None
        };

        return Ok(StatementKind::If(condition, then_body, else_body));
    }
    if !tokens.is_empty() && matches!(tokens[0].token, Token::Try) {
        tokens.remove(0); // consume try
        if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume {
        let try_body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume }

        // Parse optional catch
        let mut catch_param = String::new();
        let mut catch_body: Vec<Statement> = Vec::new();
        let mut finally_body: Option<Vec<Statement>> = None;

        // Skip any intervening line terminators before checking for `catch`
        // so patterns like `}\ncatch (e) { ... }` are accepted.
        while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
            tokens.remove(0);
        }

        if !tokens.is_empty() && matches!(tokens[0].token, Token::Catch) {
            tokens.remove(0); // consume catch
            if tokens.is_empty() || !matches!(tokens[0].token, Token::LParen) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume (
            if tokens.is_empty() {
                return Err(raise_parse_error_at(tokens));
            }
            if let Token::Identifier(name) = tokens.remove(0).token {
                catch_param = name;
            } else {
                return Err(raise_parse_error_at(tokens));
            }
            if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume )
            if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume {
            catch_body = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume }
        }

        // Optional finally — allow line terminators between catch and finally
        while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
            tokens.remove(0);
        }

        // Optional finally
        if !tokens.is_empty() && matches!(tokens[0].token, Token::Finally) {
            tokens.remove(0); // consume finally
            if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume {
            let fb = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume }
            finally_body = Some(fb);
        }

        return Ok(StatementKind::TryCatch(try_body, catch_param, catch_body, finally_body));
    }
    if !tokens.is_empty() && matches!(tokens[0].token, Token::For) {
        tokens.remove(0); // consume for
        if tokens.is_empty() || !matches!(tokens[0].token, Token::LParen) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume (

        // Check if this is a for-of loop
        if !tokens.is_empty()
            && (matches!(tokens[0].token, Token::Let) || matches!(tokens[0].token, Token::Var) || matches!(tokens[0].token, Token::Const))
        {
            let saved_declaration_token = tokens[0].clone();
            tokens.remove(0); // consume let/var/const
            if let Some(Token::Identifier(var_name)) = tokens.first().map(|t| t.token.clone()) {
                let saved_identifier_token = tokens[0].clone();
                tokens.remove(0);
                if !tokens.is_empty() && matches!(tokens[0].token, Token::Identifier(ref s) if s == "of") {
                    // This is a for-of loop
                    tokens.remove(0); // consume of
                    let iterable = parse_expression(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    tokens.remove(0); // consume )
                    // For-of body may be a block or a single statement
                    let body = if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
                        tokens.remove(0); // consume {
                        let b = parse_statements(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume }
                        b
                    } else {
                        let s = parse_statement(tokens)?;
                        if !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
                            tokens.remove(0);
                        }
                        vec![s]
                    };
                    return Ok(StatementKind::ForOf(var_name, iterable, body));
                } else if !tokens.is_empty() && matches!(tokens[0].token, Token::In) {
                    // This is a for-in loop
                    tokens.remove(0); // consume in
                    let object = parse_expression(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    tokens.remove(0); // consume )
                    // For-in body may be a block or a single statement
                    let body = if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
                        tokens.remove(0); // consume {
                        let b = parse_statements(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume }
                        b
                    } else {
                        let s = parse_statement(tokens)?;
                        if !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
                            tokens.remove(0);
                        }
                        vec![s]
                    };
                    return Ok(StatementKind::ForIn(var_name, object, body));
                } else {
                    // This is a regular for loop with variable declaration, put tokens back
                    tokens.insert(0, saved_identifier_token);
                    tokens.insert(0, saved_declaration_token);
                }
            } else if matches!(tokens.first().map(|t| &t.token), Some(Token::LBrace))
                && (matches!(saved_declaration_token.token, Token::Var)
                    || matches!(saved_declaration_token.token, Token::Let)
                    || matches!(saved_declaration_token.token, Token::Const))
            {
                // var { ... } of iterable
                let pattern = parse_object_destructuring_pattern(tokens)?;
                if !tokens.is_empty() && matches!(tokens[0].token, Token::Identifier(ref s) if s == "of") {
                    tokens.remove(0); // consume of
                    let iterable = parse_expression(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    tokens.remove(0); // consume )
                    // parse body
                    let body = if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
                        tokens.remove(0); // consume {
                        let b = parse_statements(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume }
                        b
                    } else {
                        let s = parse_statement(tokens)?;
                        if !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
                            tokens.remove(0);
                        }
                        vec![s]
                    };
                    return Ok(StatementKind::ForOfDestructuringObject(pattern, iterable, body));
                } else {
                    // Not a for-of; restore declaration token
                    tokens.insert(0, saved_declaration_token);
                }
            } else if matches!(tokens.first().map(|t| &t.token), Some(Token::LBracket))
                && (matches!(saved_declaration_token.token, Token::Var)
                    || matches!(saved_declaration_token.token, Token::Let)
                    || matches!(saved_declaration_token.token, Token::Const))
            {
                // var [ ... ] of iterable
                let pattern = parse_array_destructuring_pattern(tokens)?;
                if !tokens.is_empty() && matches!(tokens[0].token, Token::Identifier(ref s) if s == "of") {
                    tokens.remove(0); // consume of
                    let iterable = parse_expression(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    tokens.remove(0); // consume )
                    // parse body
                    let body = if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
                        tokens.remove(0); // consume {
                        let b = parse_statements(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume }
                        b
                    } else {
                        let s = parse_statement(tokens)?;
                        if !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
                            tokens.remove(0);
                        }
                        vec![s]
                    };
                    return Ok(StatementKind::ForOfDestructuringArray(pattern, iterable, body));
                } else {
                    // Not a for-of; restore declaration token
                    tokens.insert(0, saved_declaration_token);
                }
            } else {
                // Not an identifier, put back the declaration token
                tokens.insert(0, saved_declaration_token);
            }
        }

        // Parse initialization (regular for loop)
        let init = if !tokens.is_empty() && (matches!(tokens[0].token, Token::Let) || matches!(tokens[0].token, Token::Var)) {
            Some(Box::new(parse_statement(tokens)?))
        } else if !matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
            Some(Box::new(Statement::from(StatementKind::Expr(parse_expression(tokens)?))))
        } else {
            None
        };

        if tokens.is_empty() || !matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume first ;

        // Parse condition
        let condition = if !matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
            Some(parse_expression(tokens)?)
        } else {
            None
        };

        if tokens.is_empty() || !matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume second ;

        // Parse increment
        let increment = if !matches!(tokens[0].token, Token::RParen) {
            Some(Box::new(Statement::from(StatementKind::Expr(parse_expression(tokens)?))))
        } else {
            None
        };

        if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0); // consume )

        // For-loop body may be a block or a single statement
        let body = if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
            tokens.remove(0); // consume {
            let b = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume }
            b
        } else {
            let s = parse_statement(tokens)?;
            if !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
                tokens.remove(0);
            }
            vec![s]
        };

        return Ok(StatementKind::For(init, condition, increment, body));
    }
    if !tokens.is_empty() && matches!(tokens[0].token, Token::Return) {
        tokens.remove(0); // consume return
        if tokens.is_empty() || matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
            return Ok(StatementKind::Return(None));
        }
        let expr = parse_expression(tokens)?;
        return Ok(StatementKind::Return(Some(expr)));
    }
    if !tokens.is_empty()
        && (matches!(tokens[0].token, Token::Let) || matches!(tokens[0].token, Token::Var) || matches!(tokens[0].token, Token::Const))
    {
        let is_const = matches!(tokens[0].token, Token::Const);
        let is_var = matches!(tokens[0].token, Token::Var);
        let decl_keyword_token = tokens[0].clone();
        tokens.remove(0); // consume let/var/const
        log::trace!(
            "parse_statement: after consuming declaration keyword; next tokens (first 8): {:?}",
            tokens.iter().take(8).collect::<Vec<_>>()
        );

        // Check for destructuring
        if !tokens.is_empty() && matches!(tokens[0].token, Token::LBracket) {
            // Array destructuring
            let pattern = parse_array_destructuring_pattern(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0].token, Token::Assign) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume =
            let expr = parse_expression(tokens)?;
            if is_const {
                return Ok(StatementKind::ConstDestructuringArray(pattern, expr));
            } else if is_var {
                return Ok(StatementKind::VarDestructuringArray(pattern, expr));
            } else {
                return Ok(StatementKind::LetDestructuringArray(pattern, expr));
            }
        } else if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
            // Object destructuring
            let pattern = parse_object_destructuring_pattern(tokens)?;
            log::trace!(
                "parse_statement: after object pattern parse; next tokens (first 8): {:?}",
                tokens.iter().take(8).collect::<Vec<_>>()
            );
            if tokens.is_empty() || !matches!(tokens[0].token, Token::Assign) {
                log::error!(
                    "parse_statement: expected '=' after object pattern but found (first 8): {:?}",
                    tokens.iter().take(8).collect::<Vec<_>>()
                );
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume =
            let expr = parse_expression(tokens)?;
            if is_const {
                return Ok(StatementKind::ConstDestructuringObject(pattern, expr));
            } else if is_var {
                return Ok(StatementKind::VarDestructuringObject(pattern, expr));
            } else {
                return Ok(StatementKind::LetDestructuringObject(pattern, expr));
            }
        } else {
            // Regular variable declaration
            let mut declarations = Vec::new();
            loop {
                let name = if let Some(Token::Identifier(n)) = tokens.first().map(|t| t.token.clone()) {
                    tokens.remove(0);
                    n
                } else {
                    // Not an identifier, put back the declaration token if this is the first one
                    if declarations.is_empty() {
                        tokens.insert(0, decl_keyword_token);
                    }
                    return Err(raise_parse_error_at(tokens));
                };

                let init = if !tokens.is_empty() && matches!(tokens[0].token, Token::Assign) {
                    tokens.remove(0); // consume '='
                    Some(parse_assignment(tokens)?)
                } else {
                    None
                };

                declarations.push((name, init));

                if !tokens.is_empty() && matches!(tokens[0].token, Token::Comma) {
                    tokens.remove(0); // consume comma
                    while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
                        tokens.remove(0);
                    }
                } else {
                    break;
                }
            }

            if is_const {
                let mut const_decls = Vec::new();
                for (name, init) in declarations {
                    if let Some(expr) = init {
                        const_decls.push((name, expr));
                    } else {
                        return Err(raise_parse_error!("Const declaration must have initializer"));
                    }
                }
                return Ok(StatementKind::Const(const_decls));
            } else if is_var {
                return Ok(StatementKind::Var(declarations));
            } else {
                return Ok(StatementKind::Let(declarations));
            }
        }
    }
    if !tokens.is_empty() && matches!(tokens[0].token, Token::Class) {
        tokens.remove(0); // consume class
        if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
            tokens.remove(0);
            let extends = if !tokens.is_empty() && matches!(tokens[0].token, Token::Extends) {
                tokens.remove(0); // consume extends
                // Parse an arbitrary expression for the superclass (e.g. Intl.PluralRules)
                let super_expr = parse_expression(tokens)?;
                Some(super_expr)
            } else {
                None
            };

            // Parse class body
            if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume {

            let mut members = Vec::new();
            while !tokens.is_empty() && !matches!(tokens[0].token, Token::RBrace) {
                // Skip blank lines or stray semicolons in class body (tokenizer
                // emits LineTerminator tokens). This allows class members to be
                // separated by newlines or semicolons without confusing the
                // parser which expects identifiers/static/get/set/paren next.
                while !tokens.is_empty() && matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
                    tokens.remove(0);
                }
                if tokens.is_empty() || matches!(tokens[0].token, Token::RBrace) {
                    break;
                }
                let is_static = if !tokens.is_empty() && matches!(tokens[0].token, Token::Static) {
                    tokens.remove(0);
                    true
                } else {
                    false
                };

                if let Some(Token::Identifier(method_name)) = tokens.first().map(|t| &t.token) {
                    let method_name = method_name.clone();
                    if method_name == "constructor" {
                        tokens.remove(0);
                        // Parse constructor
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::LParen) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume (
                        let params = parse_parameters(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume {
                        let body = parse_statement_block(tokens)?;
                        members.push(ClassMember::Constructor(params, body));
                    } else {
                        tokens.remove(0);
                        if tokens.is_empty() {
                            return Err(raise_parse_error_at(tokens));
                        }
                        // Check for getter/setter
                        let is_getter = matches!(tokens[0].token, Token::Identifier(ref id) if id == "get");
                        let is_setter = matches!(tokens[0].token, Token::Identifier(ref id) if id == "set");
                        if is_getter || is_setter {
                            tokens.remove(0); // consume get/set
                            if tokens.is_empty() || !matches!(tokens[0].token, Token::Identifier(_)) {
                                return Err(raise_parse_error_at(tokens));
                            }
                            let prop_name = if let Token::Identifier(name) = &tokens.remove(0).token {
                                name.clone()
                            } else {
                                return Err(raise_parse_error_at(tokens));
                            };
                            if tokens.is_empty() || !matches!(tokens[0].token, Token::LParen) {
                                return Err(raise_parse_error_at(tokens));
                            }
                            tokens.remove(0); // consume (
                            let params = parse_parameters(tokens)?;
                            if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                                return Err(raise_parse_error_at(tokens));
                            }
                            tokens.remove(0); // consume {
                            let body = parse_statement_block(tokens)?;
                            if is_getter {
                                if !params.is_empty() {
                                    return Err(raise_parse_error_at(tokens)); // getters should have no parameters
                                }
                                if is_static {
                                    members.push(ClassMember::StaticGetter(prop_name, body));
                                } else {
                                    members.push(ClassMember::Getter(prop_name, body));
                                }
                            } else {
                                // setter
                                if params.len() != 1 {
                                    return Err(raise_parse_error_at(tokens)); // setters should have exactly one parameter
                                }
                                if is_static {
                                    members.push(ClassMember::StaticSetter(prop_name, params, body));
                                } else {
                                    members.push(ClassMember::Setter(prop_name, params, body));
                                }
                            }
                        } else if matches!(tokens[0].token, Token::LParen) {
                            // This is a method
                            tokens.remove(0); // consume (
                            let params = parse_parameters(tokens)?;
                            if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                                return Err(raise_parse_error_at(tokens));
                            }
                            tokens.remove(0); // consume {
                            let body = parse_statement_block(tokens)?;
                            if is_static {
                                members.push(ClassMember::StaticMethod(method_name, params, body));
                            } else {
                                members.push(ClassMember::Method(method_name, params, body));
                            }
                        } else if matches!(tokens[0].token, Token::Assign) {
                            // This is a property
                            tokens.remove(0); // consume =
                            let value = parse_expression(tokens)?;
                            if tokens.is_empty() || !matches!(tokens[0].token, Token::Semicolon | Token::LineTerminator) {
                                return Err(raise_parse_error_at(tokens));
                            }
                            tokens.remove(0); // consume ;
                            if is_static {
                                members.push(ClassMember::StaticProperty(method_name, value));
                            } else {
                                members.push(ClassMember::Property(method_name, value));
                            }
                        } else {
                            return Err(raise_parse_error_at(tokens));
                        }
                    }
                } else {
                    return Err(raise_parse_error_at(tokens));
                }
            }

            if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume }

            return Ok(StatementKind::Class(name, extends, members));
        }
    }
    let expr = parse_expression(tokens)?;
    // Check if this is an assignment expression
    if let Expr::Assign(target, value) = &expr
        && let Expr::Var(name, _, _) = target.as_ref()
    {
        return Ok(StatementKind::Assign(name.clone(), *value.clone()));
    }
    Ok(StatementKind::Expr(expr))
}
