use crate::{
    JSError,
    core::{
        DestructuringElement, Expr, ObjectDestructuringElement, Token, parse_array_destructuring_pattern, parse_expression,
        parse_object_destructuring_pattern, parse_parameters, parse_statement_block,
    },
    js_class::ClassMember,
    parse_error_here,
};

#[derive(Clone, Debug)]
pub enum SwitchCase {
    Case(Expr, Vec<Statement>), // case value, statements
    Default(Vec<Statement>),    // default statements
}

#[derive(Clone)]
pub enum Statement {
    Let(String, Option<Expr>),
    Var(String, Option<Expr>),
    Const(String, Expr),
    LetDestructuringArray(Vec<DestructuringElement>, Expr), // array destructuring: let [a, b] = [1, 2];
    ConstDestructuringArray(Vec<DestructuringElement>, Expr), // const [a, b] = [1, 2];
    LetDestructuringObject(Vec<ObjectDestructuringElement>, Expr), // object destructuring: let {a, b} = {a: 1, b: 2};
    ConstDestructuringObject(Vec<ObjectDestructuringElement>, Expr), // const {a, b} = {a: 1, b: 2};
    Class(String, Option<crate::core::Expr>, Vec<ClassMember>), // name, extends, members
    Assign(String, Expr),                                   // variable assignment
    Expr(Expr),
    Return(Option<Expr>),
    If(Expr, Vec<Statement>, Option<Vec<Statement>>), // condition, then_body, else_body
    For(Option<Box<Statement>>, Option<Expr>, Option<Box<Statement>>, Vec<Statement>), // init, condition, increment, body
    ForOf(String, Expr, Vec<Statement>),              // variable, iterable, body
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
}

impl std::fmt::Debug for Statement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Statement::Let(var, expr) => write!(f, "Let({}, {:?})", var, expr),
            Statement::Var(var, expr) => write!(f, "Var({}, {:?})", var, expr),
            Statement::Const(var, expr) => write!(f, "Const({}, {:?})", var, expr),
            Statement::LetDestructuringArray(pattern, expr) => write!(f, "LetDestructuringArray({:?}, {:?})", pattern, expr),
            Statement::ConstDestructuringArray(pattern, expr) => write!(f, "ConstDestructuringArray({:?}, {:?})", pattern, expr),
            Statement::LetDestructuringObject(pattern, expr) => write!(f, "LetDestructuringObject({:?}, {:?})", pattern, expr),
            Statement::ConstDestructuringObject(pattern, expr) => write!(f, "ConstDestructuringObject({:?}, {:?})", pattern, expr),
            Statement::Class(name, extends, members) => write!(f, "Class({name}, {extends:?}, {members:?})"),
            Statement::Assign(var, expr) => write!(f, "Assign({}, {:?})", var, expr),
            Statement::Expr(expr) => write!(f, "Expr({:?})", expr),
            Statement::Return(Some(expr)) => write!(f, "Return({:?})", expr),
            Statement::Return(None) => write!(f, "Return(None)"),
            Statement::If(cond, then_body, else_body) => {
                write!(f, "If({:?}, {:?}, {:?})", cond, then_body, else_body)
            }
            Statement::For(init, cond, incr, body) => {
                write!(f, "For({:?}, {:?}, {:?}, {:?})", init, cond, incr, body)
            }
            Statement::ForOf(var, iterable, body) => {
                write!(f, "ForOf({}, {:?}, {:?})", var, iterable, body)
            }
            Statement::ForOfDestructuringObject(pat, iterable, body) => {
                write!(f, "ForOfDestructuringObject({:?}, {:?}, {:?})", pat, iterable, body)
            }
            Statement::ForOfDestructuringArray(pat, iterable, body) => {
                write!(f, "ForOfDestructuringArray({:?}, {:?}, {:?})", pat, iterable, body)
            }
            Statement::While(cond, body) => {
                write!(f, "While({:?}, {:?})", cond, body)
            }
            Statement::DoWhile(body, cond) => {
                write!(f, "DoWhile({:?}, {:?})", body, cond)
            }
            Statement::Switch(expr, cases) => {
                write!(f, "Switch({:?}, {:?})", expr, cases)
            }
            Statement::Break(None) => write!(f, "Break"),
            Statement::Break(Some(lbl)) => write!(f, "Break({})", lbl),
            Statement::Continue(None) => write!(f, "Continue"),
            Statement::Continue(Some(lbl)) => write!(f, "Continue({})", lbl),
            Statement::Label(name, stmt) => write!(f, "Label({}, {:?})", name, stmt),
            Statement::Block(stmts) => write!(f, "Block({:?})", stmts),
            Statement::TryCatch(try_body, catch_param, catch_body, finally_body) => {
                write!(f, "TryCatch({:?}, {}, {:?}, {:?})", try_body, catch_param, catch_body, finally_body)
            }
            Statement::Throw(expr) => {
                write!(f, "Throw({:?})", expr)
            }
        }
    }
}

pub fn parse_statements(tokens: &mut Vec<Token>) -> Result<Vec<Statement>, JSError> {
    let mut statements = Vec::new();
    while !tokens.is_empty() && !matches!(tokens[0], Token::RBrace) {
        // Skip empty statement semicolons that may appear (e.g. from inserted tokens)
        if matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
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
                log::error!("parse_statements error at token start. remaining tokens (first 40):");
                for (i, t) in tokens_snapshot.iter().take(40).enumerate() {
                    log::error!("  {}: {:?}", i, t);
                }
                return Err(e);
            }
        };
        statements.push(stmt);
        if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
            tokens.remove(0);
        }
    }
    Ok(statements)
}

pub fn parse_statement(tokens: &mut Vec<Token>) -> Result<Statement, JSError> {
    // Skip any leading line terminators so statements inside blocks (e.g., cases)
    // can contain blank lines or comments that emitted line terminators.
    while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
        tokens.remove(0);
    }
    // This function parses a single statement from the token stream.
    // It handles various types of statements including break, continue, and return.

    // Labelled statement: `label: <statement>`
    if tokens.len() > 1
        && let Token::Identifier(name) = tokens[0].clone()
        && matches!(tokens[1], Token::Colon)
    {
        // consume identifier and colon
        tokens.remove(0);
        tokens.remove(0);
        let stmt = parse_statement(tokens)?;
        return Ok(Statement::Label(name, Box::new(stmt)));
    }

    // Block statement as a standalone statement
    if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
        tokens.remove(0); // consume {
        let body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume }
        return Ok(Statement::Block(body));
    }

    if !tokens.is_empty() && matches!(tokens[0], Token::Break) {
        tokens.remove(0); // consume break
        // optional label identifier
        let label = if !tokens.is_empty() {
            if let Some(Token::Identifier(name)) = tokens.first().cloned() {
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
        if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
            tokens.remove(0); // consume ;
        } else if !tokens.is_empty()
            && (matches!(tokens[0], Token::RBrace) || matches!(tokens[0], Token::Catch) || matches!(tokens[0], Token::Finally))
        {
            // semicolon omitted but next token terminates the statement; accept
        } else {
            return Err(parse_error_here!());
        }
        return Ok(Statement::Break(label));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Continue) {
        tokens.remove(0); // consume continue
        // optional label identifier
        let label = if !tokens.is_empty() {
            if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                tokens.remove(0);
                Some(name)
            } else {
                None
            }
        } else {
            None
        };
        if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume ;
        return Ok(Statement::Continue(label));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::While) {
        tokens.remove(0); // consume while
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume (
        let condition = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume )
        // Body may be a block (`{ ... }`) or a single statement (e.g. `while (...) stmt;`)
        let body = if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
            tokens.remove(0); // consume {
            let b = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(parse_error_here!());
            }
            tokens.remove(0); // consume }
            b
        } else {
            // Single-statement body: parse one statement and remove a trailing semicolon if present.
            let s = parse_statement(tokens)?;
            if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
                tokens.remove(0);
            }
            vec![s]
        };
        return Ok(Statement::While(condition, body));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Do) {
        tokens.remove(0); // consume do
        // Do body may be a block or a single statement
        let body = if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
            tokens.remove(0); // consume {
            let b = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(parse_error_here!());
            }
            tokens.remove(0); // consume }
            b
        } else {
            // Single-statement body: parse one statement and remove trailing semicolon if present
            let s = parse_statement(tokens)?;
            if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
                tokens.remove(0);
            }
            vec![s]
        };
        // Allow intervening line terminators between the do-body and the `while`
        // keyword, e.g. `do\nwhile (...)`.
        while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
            tokens.remove(0);
        }

        if tokens.is_empty() || !matches!(tokens[0], Token::While) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume while
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume (
        let condition = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume )
        if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume ;
        return Ok(Statement::DoWhile(body, condition));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Switch) {
        tokens.remove(0); // consume switch
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume (
        let expr = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume )
        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume {
        let mut cases = Vec::new();
        while !tokens.is_empty() && !matches!(tokens[0], Token::RBrace) {
            // Skip any blank lines inside switch body (LineTerminators coming from tokenizer)
            while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
                tokens.remove(0);
            }
            if tokens.is_empty() || matches!(tokens[0], Token::RBrace) {
                break;
            }
            if matches!(tokens[0], Token::Case) {
                tokens.remove(0); // consume case
                let case_value = parse_expression(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0], Token::Colon) {
                    return Err(parse_error_here!());
                }
                tokens.remove(0); // consume :
                let mut case_stmts = Vec::new();
                while !tokens.is_empty()
                    && !matches!(tokens[0], Token::Case)
                    && !matches!(tokens[0], Token::Default)
                    && !matches!(tokens[0], Token::RBrace)
                {
                    // Skip blank lines inside a case body before parsing the next
                    // statement so cases like `case X: \n` fall through properly
                    // rather than trying to parse `}` as a statement.
                    while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
                        tokens.remove(0);
                    }
                    // If skipping blank lines lands us at a case/default/rbrace
                    // there's no statement to parse — break out of the case loop.
                    if tokens.is_empty()
                        || matches!(tokens[0], Token::Case)
                        || matches!(tokens[0], Token::Default)
                        || matches!(tokens[0], Token::RBrace)
                    {
                        break;
                    }
                    log::trace!("switch case parsing stmt, next token = {:?}", tokens.first());
                    let stmt = parse_statement(tokens)?;
                    case_stmts.push(stmt);
                    if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
                        tokens.remove(0);
                    }
                }
                cases.push(SwitchCase::Case(case_value, case_stmts));
            } else if matches!(tokens[0], Token::Default) {
                tokens.remove(0); // consume default
                if tokens.is_empty() || !matches!(tokens[0], Token::Colon) {
                    return Err(parse_error_here!());
                }
                tokens.remove(0); // consume :
                let mut default_stmts = Vec::new();
                while !tokens.is_empty() && !matches!(tokens[0], Token::RBrace) {
                    // Skip leading blank lines inside default branch to avoid
                    // attempting to parse a closing brace as a statement.
                    while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
                        tokens.remove(0);
                    }
                    if tokens.is_empty() || matches!(tokens[0], Token::RBrace) {
                        break;
                    }
                    log::trace!("switch default parsing stmt, next token = {:?}", tokens.first());
                    let stmt = parse_statement(tokens)?;
                    default_stmts.push(stmt);
                    if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
                        tokens.remove(0);
                    }
                }
                cases.push(SwitchCase::Default(default_stmts));
            } else {
                return Err(parse_error_here!());
            }
        }
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume }
        return Ok(Statement::Switch(expr, cases));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Throw) {
        tokens.remove(0); // consume throw
        let expr = parse_expression(tokens)?;
        // Accept an explicit semicolon, or allow ASI-ish omission when next token
        // ends the block or begins a catch/finally block.
        if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
            tokens.remove(0); // consume ;
        } else if !tokens.is_empty()
            && (matches!(tokens[0], Token::RBrace) || matches!(tokens[0], Token::Catch) || matches!(tokens[0], Token::Finally))
        {
            // semicolon omitted but next token terminates the statement; accept
        } else {
            return Err(parse_error_here!());
        }
        return Ok(Statement::Throw(expr));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Async) {
        tokens.remove(0); // consume async
        if !tokens.is_empty() && matches!(tokens[0], Token::Function) {
            tokens.remove(0); // consume function
            if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                tokens.remove(0);
                if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
                    tokens.remove(0); // consume (
                    let mut params = Vec::new();
                    if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
                        loop {
                            if let Some(Token::Identifier(param)) = tokens.first().cloned() {
                                tokens.remove(0);
                                params.push(param);
                                if tokens.is_empty() {
                                    return Err(parse_error_here!());
                                }
                                if matches!(tokens[0], Token::RParen) {
                                    break;
                                }
                                if !matches!(tokens[0], Token::Comma) {
                                    return Err(parse_error_here!());
                                }
                                tokens.remove(0); // consume ,
                            } else {
                                return Err(parse_error_here!());
                            }
                        }
                    }
                    if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                        return Err(parse_error_here!());
                    }
                    tokens.remove(0); // consume )
                    if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                        return Err(parse_error_here!());
                    }
                    tokens.remove(0); // consume {
                    let body = parse_statements(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                        return Err(parse_error_here!());
                    }
                    tokens.remove(0); // consume }
                    return Ok(Statement::Let(name, Some(Expr::AsyncFunction(params, body))));
                }
            }
        }
        return Err(parse_error_here!());
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Function) {
        tokens.remove(0); // consume function
        log::trace!(
            "parse_statement: entered Function branch; next tokens: {:?}",
            tokens.iter().take(12).collect::<Vec<_>>()
        );
        if let Some(Token::Identifier(name)) = tokens.first().cloned() {
            tokens.remove(0);
            log::trace!(
                "parse_statement: function name parsed: {} ; remaining: {:?}",
                name,
                tokens.iter().take(12).collect::<Vec<_>>()
            );
            if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
                tokens.remove(0); // consume (
                let mut params = Vec::new();
                if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
                    loop {
                        if let Some(Token::Identifier(param)) = tokens.first().cloned() {
                            tokens.remove(0);
                            params.push(param);
                            if tokens.is_empty() {
                                return Err(parse_error_here!());
                            }
                            if matches!(tokens[0], Token::RParen) {
                                break;
                            }
                            if !matches!(tokens[0], Token::Comma) {
                                return Err(parse_error_here!());
                            }
                            tokens.remove(0); // consume ,
                        } else {
                            return Err(parse_error_here!());
                        }
                    }
                }
                if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                    return Err(parse_error_here!());
                }
                tokens.remove(0); // consume )
                if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                    return Err(parse_error_here!());
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
                if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                    return Err(parse_error_here!());
                }
                tokens.remove(0); // consume }
                return Ok(Statement::Let(name, Some(Expr::Function(params, body))));
            }
        }
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::If) {
        tokens.remove(0); // consume if
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume (
        let condition = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume )
        // Allow either a block (`{ ... }`) or a single statement as the
        // then-body of an `if` (e.g. `if (cond) stmt;`), matching JavaScript.
        let then_body = if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
            tokens.remove(0); // consume {
            let body = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(parse_error_here!());
            }
            tokens.remove(0); // consume }
            body
        } else {
            // Single-statement body: parse a single statement and wrap it.
            let stmt = parse_statement(tokens)?;
            // Consume trailing semicolon after the inner statement if present so
            // `if (cond) stmt; else ...` is handled correctly.
            if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
                tokens.remove(0);
            }
            vec![stmt]
        };

        // Allow newline(s) between then-body and `else` so `else` can be on the
        // following line (real-world JS commonly formats code like this).
        while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
            tokens.remove(0);
        }

        let else_body = if !tokens.is_empty() && matches!(tokens[0], Token::Else) {
            tokens.remove(0); // consume else
            // Support both `else { ... }` and `else if (...) { ... }` forms.
            if !tokens.is_empty() && matches!(tokens[0], Token::If) {
                // `else if` form — parse the nested if statement and wrap it
                // as a single-item body.
                let nested_if = parse_statement(tokens)?;
                Some(vec![nested_if])
            } else if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
                // Block body for else
                tokens.remove(0); // consume {
                let body = parse_statements(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                    return Err(parse_error_here!());
                }
                tokens.remove(0); // consume }
                Some(body)
            } else {
                // Single-statement else body (e.g. `else stmt;`)
                let stmt = parse_statement(tokens)?;
                if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
                    tokens.remove(0);
                }
                Some(vec![stmt])
            }
        } else {
            None
        };

        return Ok(Statement::If(condition, then_body, else_body));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Try) {
        tokens.remove(0); // consume try
        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume {
        let try_body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume }

        // Parse optional catch
        let mut catch_param = String::new();
        let mut catch_body: Vec<Statement> = Vec::new();
        let mut finally_body: Option<Vec<Statement>> = None;

        // Skip any intervening line terminators before checking for `catch`
        // so patterns like `}\ncatch (e) { ... }` are accepted.
        while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
            tokens.remove(0);
        }

        if !tokens.is_empty() && matches!(tokens[0], Token::Catch) {
            tokens.remove(0); // consume catch
            if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
                return Err(parse_error_here!());
            }
            tokens.remove(0); // consume (
            if tokens.is_empty() {
                return Err(parse_error_here!());
            }
            if let Token::Identifier(name) = tokens.remove(0) {
                catch_param = name;
            } else {
                return Err(parse_error_here!());
            }
            if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                return Err(parse_error_here!());
            }
            tokens.remove(0); // consume )
            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                return Err(parse_error_here!());
            }
            tokens.remove(0); // consume {
            catch_body = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(parse_error_here!());
            }
            tokens.remove(0); // consume }
        }

        // Optional finally — allow line terminators between catch and finally
        while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
            tokens.remove(0);
        }

        // Optional finally
        if !tokens.is_empty() && matches!(tokens[0], Token::Finally) {
            tokens.remove(0); // consume finally
            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                return Err(parse_error_here!());
            }
            tokens.remove(0); // consume {
            let fb = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(parse_error_here!());
            }
            tokens.remove(0); // consume }
            finally_body = Some(fb);
        }

        return Ok(Statement::TryCatch(try_body, catch_param, catch_body, finally_body));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::For) {
        tokens.remove(0); // consume for
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume (

        // Check if this is a for-of loop
        if !tokens.is_empty() && (matches!(tokens[0], Token::Let) || matches!(tokens[0], Token::Var) || matches!(tokens[0], Token::Const)) {
            let saved_declaration_token = tokens[0].clone();
            tokens.remove(0); // consume let/var/const
            if let Some(Token::Identifier(var_name)) = tokens.first().cloned() {
                let saved_identifier_token = tokens[0].clone();
                tokens.remove(0);
                if !tokens.is_empty() && matches!(tokens[0], Token::Identifier(ref s) if s == "of") {
                    // This is a for-of loop
                    tokens.remove(0); // consume of
                    let iterable = parse_expression(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                        return Err(parse_error_here!());
                    }
                    tokens.remove(0); // consume )
                    // For-of body may be a block or a single statement
                    let body = if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
                        tokens.remove(0); // consume {
                        let b = parse_statements(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                            return Err(parse_error_here!());
                        }
                        tokens.remove(0); // consume }
                        b
                    } else {
                        let s = parse_statement(tokens)?;
                        if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
                            tokens.remove(0);
                        }
                        vec![s]
                    };
                    return Ok(Statement::ForOf(var_name, iterable, body));
                } else {
                    // This is a regular for loop with variable declaration, put tokens back
                    tokens.insert(0, saved_identifier_token);
                    tokens.insert(0, saved_declaration_token);
                }
            } else if let Some(Token::LBrace) = tokens.first().cloned()
                && (matches!(saved_declaration_token, Token::Var)
                    || matches!(saved_declaration_token, Token::Let)
                    || matches!(saved_declaration_token, Token::Const))
            {
                // var { ... } of iterable
                let pattern = parse_object_destructuring_pattern(tokens)?;
                if !tokens.is_empty() && matches!(tokens[0], Token::Identifier(ref s) if s == "of") {
                    tokens.remove(0); // consume of
                    let iterable = parse_expression(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                        return Err(parse_error_here!());
                    }
                    tokens.remove(0); // consume )
                    // parse body
                    let body = if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
                        tokens.remove(0); // consume {
                        let b = parse_statements(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                            return Err(parse_error_here!());
                        }
                        tokens.remove(0); // consume }
                        b
                    } else {
                        let s = parse_statement(tokens)?;
                        if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
                            tokens.remove(0);
                        }
                        vec![s]
                    };
                    return Ok(Statement::ForOfDestructuringObject(pattern, iterable, body));
                } else {
                    // Not a for-of; restore declaration token
                    tokens.insert(0, saved_declaration_token);
                }
            } else if let Some(Token::LBracket) = tokens.first().cloned()
                && (matches!(saved_declaration_token, Token::Var)
                    || matches!(saved_declaration_token, Token::Let)
                    || matches!(saved_declaration_token, Token::Const))
            {
                // var [ ... ] of iterable
                let pattern = parse_array_destructuring_pattern(tokens)?;
                if !tokens.is_empty() && matches!(tokens[0], Token::Identifier(ref s) if s == "of") {
                    tokens.remove(0); // consume of
                    let iterable = parse_expression(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                        return Err(parse_error_here!());
                    }
                    tokens.remove(0); // consume )
                    // parse body
                    let body = if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
                        tokens.remove(0); // consume {
                        let b = parse_statements(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                            return Err(parse_error_here!());
                        }
                        tokens.remove(0); // consume }
                        b
                    } else {
                        let s = parse_statement(tokens)?;
                        if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
                            tokens.remove(0);
                        }
                        vec![s]
                    };
                    return Ok(Statement::ForOfDestructuringArray(pattern, iterable, body));
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
        let init = if !tokens.is_empty() && (matches!(tokens[0], Token::Let) || matches!(tokens[0], Token::Var)) {
            Some(Box::new(parse_statement(tokens)?))
        } else if !matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
            Some(Box::new(Statement::Expr(parse_expression(tokens)?)))
        } else {
            None
        };

        if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume first ;

        // Parse condition
        let condition = if !matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
            Some(parse_expression(tokens)?)
        } else {
            None
        };

        if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume second ;

        // Parse increment
        let increment = if !matches!(tokens[0], Token::RParen) {
            Some(Box::new(Statement::Expr(parse_expression(tokens)?)))
        } else {
            None
        };

        if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
            return Err(parse_error_here!());
        }
        tokens.remove(0); // consume )

        // For-loop body may be a block or a single statement
        let body = if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
            tokens.remove(0); // consume {
            let b = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(parse_error_here!());
            }
            tokens.remove(0); // consume }
            b
        } else {
            let s = parse_statement(tokens)?;
            if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
                tokens.remove(0);
            }
            vec![s]
        };

        return Ok(Statement::For(init, condition, increment, body));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Return) {
        tokens.remove(0); // consume return
        if tokens.is_empty() || matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
            return Ok(Statement::Return(None));
        }
        let expr = parse_expression(tokens)?;
        return Ok(Statement::Return(Some(expr)));
    }
    if !tokens.is_empty() && (matches!(tokens[0], Token::Let) || matches!(tokens[0], Token::Var) || matches!(tokens[0], Token::Const)) {
        let is_const = matches!(tokens[0], Token::Const);
        let is_var = matches!(tokens[0], Token::Var);
        tokens.remove(0); // consume let/var/const
        log::trace!(
            "parse_statement: after consuming declaration keyword; next tokens (first 8): {:?}",
            tokens.iter().take(8).collect::<Vec<_>>()
        );

        // Check for destructuring
        if !tokens.is_empty() && matches!(tokens[0], Token::LBracket) {
            // Array destructuring
            let pattern = parse_array_destructuring_pattern(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::Assign) {
                return Err(parse_error_here!());
            }
            tokens.remove(0); // consume =
            let expr = parse_expression(tokens)?;
            if is_const {
                return Ok(Statement::ConstDestructuringArray(pattern, expr));
            } else {
                return Ok(Statement::LetDestructuringArray(pattern, expr));
            }
        } else if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
            // Object destructuring
            let pattern = parse_object_destructuring_pattern(tokens)?;
            log::trace!(
                "parse_statement: after object pattern parse; next tokens (first 8): {:?}",
                tokens.iter().take(8).collect::<Vec<_>>()
            );
            if tokens.is_empty() || !matches!(tokens[0], Token::Assign) {
                log::error!(
                    "parse_statement: expected '=' after object pattern but found (first 8): {:?}",
                    tokens.iter().take(8).collect::<Vec<_>>()
                );
                return Err(parse_error_here!());
            }
            tokens.remove(0); // consume =
            let expr = parse_expression(tokens)?;
            if is_const {
                return Ok(Statement::ConstDestructuringObject(pattern, expr));
            } else {
                return Ok(Statement::LetDestructuringObject(pattern, expr));
            }
        } else {
            // Regular variable declaration
            if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                tokens.remove(0);
                log::trace!(
                    "parse_statement: consumed identifier {}; next tokens (first 8): {:?}",
                    name,
                    tokens.iter().take(8).collect::<Vec<_>>()
                );
                // Handle optional initializer (e.g., var x = 1)
                if !tokens.is_empty() && matches!(tokens[0], Token::Assign) {
                    tokens.remove(0);
                    log::trace!(
                        "parse_statement: consumed '='; next tokens (first 8): {:?}",
                        tokens.iter().take(8).collect::<Vec<_>>()
                    );
                    // Debug: log a short snapshot of tokens before attempting to
                    // parse a potentially large/complex initializer. This helps
                    // triage failures in big merged harness files where
                    // parse_expression may fail silently after consuming tokens.
                    log::trace!(
                        "parse_statement: parsing initializer for '{}' ; next tokens (first 40): {:?}",
                        name,
                        tokens.iter().take(40).collect::<Vec<_>>()
                    );
                    // parse initializer with debug: if parsing fails, print a short token context
                    let expr = match parse_expression(tokens) {
                        Ok(e) => {
                            log::trace!(
                                "parse_statement: succeeded parsing initializer for '{}' ; remaining tokens after init (first 40): {:?}",
                                name,
                                tokens.iter().take(40).collect::<Vec<_>>()
                            );
                            e
                        }
                        Err(err) => {
                            // Provide a clearer debug dump on failure so we can
                            // see whether parse_expression failed mid-consumption
                            // or returned an error without consuming tokens.
                            log::error!(
                                "parse_statement: failed parsing initializer for '{}' ; remaining tokens (first 40): {:?}",
                                name,
                                tokens.iter().take(40).collect::<Vec<_>>()
                            );
                            return Err(err);
                        }
                    };

                    // If there's a comma after this initialized declarator, we
                    // need to handle the rest of the comma-separated list. The
                    // parser represents single-declarator statements as
                    // `Statement::Var/Let/Const`, so we convert each following
                    // declarator into its own standalone declaration token
                    // sequence and insert them at the front of the token
                    // stream (reverse insertion to preserve order) so they
                    // get parsed on subsequent iterations.
                    let mut follow_decls: Vec<Vec<Token>> = Vec::new();
                    loop {
                        // Skip any blank lines between the comma and next ident
                        while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
                            tokens.remove(0);
                        }

                        if tokens.is_empty() || !matches!(tokens[0], Token::Comma) {
                            break;
                        }

                        tokens.remove(0); // consume comma

                        // Skip blank lines after comma
                        while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
                            tokens.remove(0);
                        }

                        // Next must be an identifier
                        let next_name = if let Some(Token::Identifier(n)) = tokens.first().cloned() {
                            tokens.remove(0);
                            n
                        } else {
                            return Err(parse_error_here!());
                        };

                        // Capture initializer tokens if present (we'll build a
                        // separate statement for this declarator later).
                        let mut init_tokens: Vec<Token> = Vec::new();
                        if !tokens.is_empty() && matches!(tokens[0], Token::Assign) {
                            tokens.remove(0); // consume '='
                            // Grab tokens into init_tokens until top-level comma/semicolon
                            let mut depth: i32 = 0;
                            while !tokens.is_empty() {
                                if depth == 0 && (matches!(tokens[0], Token::Comma) || matches!(tokens[0], Token::Semicolon)) {
                                    break;
                                }
                                match tokens[0] {
                                    Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                                    Token::RParen | Token::RBracket | Token::RBrace => depth -= 1,
                                    _ => {}
                                }
                                init_tokens.push(tokens.remove(0));
                            }
                        }

                        // Build a token sequence for the standalone declaration
                        // in left-to-right order: <var/let/const> <ident> [= <init>];
                        let mut decl_tokens: Vec<Token> = Vec::new();
                        if is_var {
                            decl_tokens.push(Token::Var);
                        } else if is_const {
                            decl_tokens.push(Token::Const);
                        } else {
                            decl_tokens.push(Token::Let);
                        }
                        // keep a copy for debug logging (we'll move the original into the token list)
                        let next_name_for_log = next_name.clone();
                        decl_tokens.push(Token::Identifier(next_name));
                        if !init_tokens.is_empty() {
                            decl_tokens.push(Token::Assign);
                            decl_tokens.extend(init_tokens);
                        }
                        decl_tokens.push(Token::Semicolon);

                        log::trace!(
                            "parse_statement: collected follow-declarator '{}' ({} tokens)",
                            next_name_for_log,
                            decl_tokens.len()
                        );
                        follow_decls.push(decl_tokens);

                        // If the next token is a semicolon that terminates the
                        // whole declaration, consume it and stop extracting more
                        // declarators.
                        if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon) {
                            tokens.remove(0);
                            break;
                        }
                    }

                    // Insert the collected following declarations back into
                    // the token stream so they will be parsed as standalone
                    // statements in left-to-right order on subsequent
                    // iterations.
                    log::trace!("parse_statement: reinserting {} following declarator(s)", follow_decls.len());
                    for decl in follow_decls.into_iter().rev() {
                        for t in decl.into_iter().rev() {
                            tokens.insert(0, t);
                        }
                    }

                    if is_const {
                        return Ok(Statement::Const(name, expr));
                    } else if is_var {
                        return Ok(Statement::Var(name, Some(expr)));
                    } else {
                        return Ok(Statement::Let(name, Some(expr)));
                    }
                } else if !is_const {
                    // Support comma-separated declarations like `var a, b, c;` by
                    // inserting subsequent simple declarators back into the token
                    // stream as separate var/let statements. For now we only support
                    // subsequent declarators without initializers (e.g., `, b, c`).
                    if !tokens.is_empty() && matches!(tokens[0], Token::Comma) {
                        let mut extra_names: Vec<String> = Vec::new();
                        // Collect following identifiers separated by commas
                        while !tokens.is_empty() && matches!(tokens[0], Token::Comma) {
                            tokens.remove(0); // consume comma
                            // Skip blank lines between comma and identifier
                            while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
                                tokens.remove(0);
                            }
                            if let Some(Token::Identifier(n)) = tokens.first().cloned() {
                                tokens.remove(0);
                                // If there is an initializer on the later decl, bail out (not supported here)
                                if !tokens.is_empty() && matches!(tokens[0], Token::Assign) {
                                    return Err(parse_error_here!());
                                }
                                extra_names.push(n);
                            } else {
                                return Err(parse_error_here!());
                            }
                        }

                        // Insert the remaining declarations back into tokens as separate
                        // var/let statements so the parser will parse them on subsequent
                        // iterations. Insert in reverse order so they are parsed in the
                        // original left-to-right order.
                        for n in extra_names.into_iter().rev() {
                            // Add a semicolon terminator for each inserted declaration
                            tokens.insert(0, Token::Semicolon);
                            // Identifier
                            tokens.insert(0, Token::Identifier(n));
                            // var/let token
                            if is_var {
                                tokens.insert(0, Token::Var);
                            } else {
                                tokens.insert(0, Token::Let);
                            }
                        }

                        // Return the first declaration
                        if is_var {
                            return Ok(Statement::Var(name, None));
                        } else {
                            return Ok(Statement::Let(name, None));
                        }
                    }
                    if is_var {
                        return Ok(Statement::Var(name, None));
                    } else {
                        return Ok(Statement::Let(name, None));
                    }
                }
            }
        }
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Class) {
        tokens.remove(0); // consume class
        if let Some(Token::Identifier(name)) = tokens.first().cloned() {
            tokens.remove(0);
            let extends = if !tokens.is_empty() && matches!(tokens[0], Token::Extends) {
                tokens.remove(0); // consume extends
                // Parse an arbitrary expression for the superclass (e.g. Intl.PluralRules)
                let super_expr = parse_expression(tokens)?;
                Some(super_expr)
            } else {
                None
            };

            // Parse class body
            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                return Err(parse_error_here!());
            }
            tokens.remove(0); // consume {

            let mut members = Vec::new();
            while !tokens.is_empty() && !matches!(tokens[0], Token::RBrace) {
                // Skip blank lines or stray semicolons in class body (tokenizer
                // emits LineTerminator tokens). This allows class members to be
                // separated by newlines or semicolons without confusing the
                // parser which expects identifiers/static/get/set/paren next.
                while !tokens.is_empty() && matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
                    tokens.remove(0);
                }
                if tokens.is_empty() || matches!(tokens[0], Token::RBrace) {
                    break;
                }
                let is_static = if !tokens.is_empty() && matches!(tokens[0], Token::Static) {
                    tokens.remove(0);
                    true
                } else {
                    false
                };

                if let Some(Token::Identifier(method_name)) = tokens.first() {
                    let method_name = method_name.clone();
                    if method_name == "constructor" {
                        tokens.remove(0);
                        // Parse constructor
                        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
                            return Err(parse_error_here!());
                        }
                        tokens.remove(0); // consume (
                        let params = parse_parameters(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                            return Err(parse_error_here!());
                        }
                        tokens.remove(0); // consume {
                        let body = parse_statement_block(tokens)?;
                        members.push(ClassMember::Constructor(params, body));
                    } else {
                        tokens.remove(0);
                        if tokens.is_empty() {
                            return Err(parse_error_here!());
                        }
                        // Check for getter/setter
                        let is_getter = matches!(tokens[0], Token::Identifier(ref id) if id == "get");
                        let is_setter = matches!(tokens[0], Token::Identifier(ref id) if id == "set");
                        if is_getter || is_setter {
                            tokens.remove(0); // consume get/set
                            if tokens.is_empty() || !matches!(tokens[0], Token::Identifier(_)) {
                                return Err(parse_error_here!());
                            }
                            let prop_name = if let Token::Identifier(name) = tokens.remove(0) {
                                name
                            } else {
                                return Err(parse_error_here!());
                            };
                            if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
                                return Err(parse_error_here!());
                            }
                            tokens.remove(0); // consume (
                            let params = parse_parameters(tokens)?;
                            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                                return Err(parse_error_here!());
                            }
                            tokens.remove(0); // consume {
                            let body = parse_statement_block(tokens)?;
                            if is_getter {
                                if !params.is_empty() {
                                    return Err(parse_error_here!()); // getters should have no parameters
                                }
                                if is_static {
                                    members.push(ClassMember::StaticGetter(prop_name, body));
                                } else {
                                    members.push(ClassMember::Getter(prop_name, body));
                                }
                            } else {
                                // setter
                                if params.len() != 1 {
                                    return Err(parse_error_here!()); // setters should have exactly one parameter
                                }
                                if is_static {
                                    members.push(ClassMember::StaticSetter(prop_name, params, body));
                                } else {
                                    members.push(ClassMember::Setter(prop_name, params, body));
                                }
                            }
                        } else if matches!(tokens[0], Token::LParen) {
                            // This is a method
                            tokens.remove(0); // consume (
                            let params = parse_parameters(tokens)?;
                            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                                return Err(parse_error_here!());
                            }
                            tokens.remove(0); // consume {
                            let body = parse_statement_block(tokens)?;
                            if is_static {
                                members.push(ClassMember::StaticMethod(method_name, params, body));
                            } else {
                                members.push(ClassMember::Method(method_name, params, body));
                            }
                        } else if matches!(tokens[0], Token::Assign) {
                            // This is a property
                            tokens.remove(0); // consume =
                            let value = parse_expression(tokens)?;
                            if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon | Token::LineTerminator) {
                                return Err(parse_error_here!());
                            }
                            tokens.remove(0); // consume ;
                            if is_static {
                                members.push(ClassMember::StaticProperty(method_name, value));
                            } else {
                                members.push(ClassMember::Property(method_name, value));
                            }
                        } else {
                            return Err(parse_error_here!());
                        }
                    }
                } else {
                    return Err(parse_error_here!());
                }
            }

            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(parse_error_here!());
            }
            tokens.remove(0); // consume }

            return Ok(Statement::Class(name, extends, members));
        }
    }
    let expr = parse_expression(tokens)?;
    // Check if this is an assignment expression
    if let Expr::Assign(target, value) = &expr
        && let Expr::Var(name) = target.as_ref()
    {
        return Ok(Statement::Assign(name.clone(), *value.clone()));
    }
    Ok(Statement::Expr(expr))
}
