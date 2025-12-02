use crate::{
    JSError,
    core::{
        DestructuringElement, Expr, ObjectDestructuringElement, Token, parse_array_destructuring_pattern, parse_expression,
        parse_object_destructuring_pattern, parse_parameters, parse_statement_block,
    },
    js_class::ClassMember,
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
    Class(String, Option<String>, Vec<ClassMember>),        // name, extends, members
    Assign(String, Expr),                                   // variable assignment
    Expr(Expr),
    Return(Option<Expr>),
    If(Expr, Vec<Statement>, Option<Vec<Statement>>), // condition, then_body, else_body
    For(Option<Box<Statement>>, Option<Expr>, Option<Box<Statement>>, Vec<Statement>), // init, condition, increment, body
    ForOf(String, Expr, Vec<Statement>),              // variable, iterable, body
    While(Expr, Vec<Statement>),                      // condition, body
    DoWhile(Vec<Statement>, Expr),                    // body, condition
    Switch(Expr, Vec<SwitchCase>),                    // expression, cases
    Break,
    Continue,
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
            Statement::While(cond, body) => {
                write!(f, "While({:?}, {:?})", cond, body)
            }
            Statement::DoWhile(body, cond) => {
                write!(f, "DoWhile({:?}, {:?})", body, cond)
            }
            Statement::Switch(expr, cases) => {
                write!(f, "Switch({:?}, {:?})", expr, cases)
            }
            Statement::Break => write!(f, "Break"),
            Statement::Continue => write!(f, "Continue"),
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
        if matches!(tokens[0], Token::Semicolon) {
            tokens.remove(0);
            continue;
        }
        log::trace!("parse_statements next token: {:?}", tokens.first());
        let stmt = parse_statement(tokens)?;
        statements.push(stmt);
        if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon) {
            tokens.remove(0);
        }
    }
    Ok(statements)
}

fn parse_statement(tokens: &mut Vec<Token>) -> Result<Statement, JSError> {
    if !tokens.is_empty() && matches!(tokens[0], Token::Break) {
        tokens.remove(0); // consume break
        // Accept either an explicit semicolon, or allow automatic semicolon
        // insertion when the next token is a block terminator (e.g. `}`), or
        // part of a try/catch/finally sequence. This keeps `throw <expr>` and
        // `throw <expr>;` equivalent inside blocks.
        if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon) {
            tokens.remove(0); // consume ;
        } else if !tokens.is_empty()
            && (matches!(tokens[0], Token::RBrace) || matches!(tokens[0], Token::Catch) || matches!(tokens[0], Token::Finally))
        {
            // semicolon omitted but next token terminates the statement; accept
        } else {
            return Err(JSError::ParseError);
        }
        return Ok(Statement::Break);
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Continue) {
        tokens.remove(0); // consume continue
        if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume ;
        return Ok(Statement::Continue);
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::While) {
        tokens.remove(0); // consume while
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume (
        let condition = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume )
        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume {
        let body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume }
        return Ok(Statement::While(condition, body));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Do) {
        tokens.remove(0); // consume do
        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume {
        let body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume }
        if tokens.is_empty() || !matches!(tokens[0], Token::While) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume while
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume (
        let condition = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume )
        if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume ;
        return Ok(Statement::DoWhile(body, condition));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Switch) {
        tokens.remove(0); // consume switch
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume (
        let expr = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume )
        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume {
        let mut cases = Vec::new();
        while !tokens.is_empty() && !matches!(tokens[0], Token::RBrace) {
            if matches!(tokens[0], Token::Case) {
                tokens.remove(0); // consume case
                let case_value = parse_expression(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0], Token::Colon) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume :
                let mut case_stmts = Vec::new();
                while !tokens.is_empty()
                    && !matches!(tokens[0], Token::Case)
                    && !matches!(tokens[0], Token::Default)
                    && !matches!(tokens[0], Token::RBrace)
                {
                    let stmt = parse_statement(tokens)?;
                    case_stmts.push(stmt);
                    if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon) {
                        tokens.remove(0);
                    }
                }
                cases.push(SwitchCase::Case(case_value, case_stmts));
            } else if matches!(tokens[0], Token::Default) {
                tokens.remove(0); // consume default
                if tokens.is_empty() || !matches!(tokens[0], Token::Colon) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume :
                let mut default_stmts = Vec::new();
                while !tokens.is_empty() && !matches!(tokens[0], Token::RBrace) {
                    let stmt = parse_statement(tokens)?;
                    default_stmts.push(stmt);
                    if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon) {
                        tokens.remove(0);
                    }
                }
                cases.push(SwitchCase::Default(default_stmts));
            } else {
                return Err(JSError::ParseError);
            }
        }
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume }
        return Ok(Statement::Switch(expr, cases));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Throw) {
        tokens.remove(0); // consume throw
        let expr = parse_expression(tokens)?;
        // Accept an explicit semicolon, or allow ASI-ish omission when next token
        // ends the block or begins a catch/finally block.
        if !tokens.is_empty() && matches!(tokens[0], Token::Semicolon) {
            tokens.remove(0); // consume ;
        } else if !tokens.is_empty()
            && (matches!(tokens[0], Token::RBrace) || matches!(tokens[0], Token::Catch) || matches!(tokens[0], Token::Finally))
        {
            // semicolon omitted but next token terminates the statement; accept
        } else {
            return Err(JSError::ParseError);
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
                                    return Err(JSError::ParseError);
                                }
                                if matches!(tokens[0], Token::RParen) {
                                    break;
                                }
                                if !matches!(tokens[0], Token::Comma) {
                                    return Err(JSError::ParseError);
                                }
                                tokens.remove(0); // consume ,
                            } else {
                                return Err(JSError::ParseError);
                            }
                        }
                    }
                    if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                        return Err(JSError::ParseError);
                    }
                    tokens.remove(0); // consume )
                    if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                        return Err(JSError::ParseError);
                    }
                    tokens.remove(0); // consume {
                    let body = parse_statements(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                        return Err(JSError::ParseError);
                    }
                    tokens.remove(0); // consume }
                    return Ok(Statement::Let(name, Some(Expr::AsyncFunction(params, body))));
                }
            }
        }
        return Err(JSError::ParseError);
    }
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
                                return Err(JSError::ParseError);
                            }
                            if matches!(tokens[0], Token::RParen) {
                                break;
                            }
                            if !matches!(tokens[0], Token::Comma) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume ,
                        } else {
                            return Err(JSError::ParseError);
                        }
                    }
                }
                if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume )
                if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume {
                let body = parse_statements(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume }
                return Ok(Statement::Let(name, Some(Expr::Function(params, body))));
            }
        }
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::If) {
        tokens.remove(0); // consume if
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume (
        let condition = parse_expression(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume )
        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume {
        let then_body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume }

        let else_body = if !tokens.is_empty() && matches!(tokens[0], Token::Else) {
            tokens.remove(0); // consume else
            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume {
            let body = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume }
            Some(body)
        } else {
            None
        };

        return Ok(Statement::If(condition, then_body, else_body));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Try) {
        tokens.remove(0); // consume try
        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume {
        let try_body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume }

        // Parse optional catch
        let mut catch_param = String::new();
        let mut catch_body: Vec<Statement> = Vec::new();
        let mut finally_body: Option<Vec<Statement>> = None;

        if !tokens.is_empty() && matches!(tokens[0], Token::Catch) {
            tokens.remove(0); // consume catch
            if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume (
            if tokens.is_empty() {
                return Err(JSError::ParseError);
            }
            if let Token::Identifier(name) = tokens.remove(0) {
                catch_param = name;
            } else {
                return Err(JSError::ParseError);
            }
            if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume )
            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume {
            catch_body = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume }
        }

        // Optional finally
        if !tokens.is_empty() && matches!(tokens[0], Token::Finally) {
            tokens.remove(0); // consume finally
            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume {
            let fb = parse_statements(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume }
            finally_body = Some(fb);
        }

        return Ok(Statement::TryCatch(try_body, catch_param, catch_body, finally_body));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::For) {
        tokens.remove(0); // consume for
        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
            return Err(JSError::ParseError);
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
                        return Err(JSError::ParseError);
                    }
                    tokens.remove(0); // consume )
                    if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                        return Err(JSError::ParseError);
                    }
                    tokens.remove(0); // consume {
                    let body = parse_statements(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                        return Err(JSError::ParseError);
                    }
                    tokens.remove(0); // consume }
                    return Ok(Statement::ForOf(var_name, iterable, body));
                } else {
                    // This is a regular for loop with variable declaration, put tokens back
                    tokens.insert(0, saved_identifier_token);
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
        } else if !matches!(tokens[0], Token::Semicolon) {
            Some(Box::new(Statement::Expr(parse_expression(tokens)?)))
        } else {
            None
        };

        if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume first ;

        // Parse condition
        let condition = if !matches!(tokens[0], Token::Semicolon) {
            Some(parse_expression(tokens)?)
        } else {
            None
        };

        if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume second ;

        // Parse increment
        let increment = if !matches!(tokens[0], Token::RParen) {
            Some(Box::new(Statement::Expr(parse_expression(tokens)?)))
        } else {
            None
        };

        if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume )

        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume {

        let body = parse_statements(tokens)?;

        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume }

        return Ok(Statement::For(init, condition, increment, body));
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::Return) {
        tokens.remove(0); // consume return
        if tokens.is_empty() || matches!(tokens[0], Token::Semicolon) {
            return Ok(Statement::Return(None));
        }
        let expr = parse_expression(tokens)?;
        return Ok(Statement::Return(Some(expr)));
    }
    if !tokens.is_empty() && (matches!(tokens[0], Token::Let) || matches!(tokens[0], Token::Var) || matches!(tokens[0], Token::Const)) {
        let is_const = matches!(tokens[0], Token::Const);
        let is_var = matches!(tokens[0], Token::Var);
        tokens.remove(0); // consume let/var/const

        // Check for destructuring
        if !tokens.is_empty() && matches!(tokens[0], Token::LBracket) {
            // Array destructuring
            let pattern = parse_array_destructuring_pattern(tokens)?;
            if tokens.is_empty() || !matches!(tokens[0], Token::Assign) {
                return Err(JSError::ParseError);
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
            if tokens.is_empty() || !matches!(tokens[0], Token::Assign) {
                return Err(JSError::ParseError);
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
                // Handle optional initializer (e.g., var x = 1)
                if !tokens.is_empty() && matches!(tokens[0], Token::Assign) {
                    tokens.remove(0);
                    let expr = parse_expression(tokens)?;
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
                            if let Some(Token::Identifier(n)) = tokens.first().cloned() {
                                tokens.remove(0);
                                // If there is an initializer on the later decl, bail out (not supported here)
                                if !tokens.is_empty() && matches!(tokens[0], Token::Assign) {
                                    return Err(JSError::ParseError);
                                }
                                extra_names.push(n);
                            } else {
                                return Err(JSError::ParseError);
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
                if let Some(Token::Identifier(parent_name)) = tokens.first().cloned() {
                    tokens.remove(0);
                    Some(parent_name)
                } else {
                    return Err(JSError::ParseError);
                }
            } else {
                None
            };

            // Parse class body
            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume {

            let mut members = Vec::new();
            while !tokens.is_empty() && !matches!(tokens[0], Token::RBrace) {
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
                            return Err(JSError::ParseError);
                        }
                        tokens.remove(0); // consume (
                        let params = parse_parameters(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                            return Err(JSError::ParseError);
                        }
                        tokens.remove(0); // consume {
                        let body = parse_statement_block(tokens)?;
                        members.push(ClassMember::Constructor(params, body));
                    } else {
                        tokens.remove(0);
                        if tokens.is_empty() {
                            return Err(JSError::ParseError);
                        }
                        // Check for getter/setter
                        let is_getter = matches!(tokens[0], Token::Identifier(ref id) if id == "get");
                        let is_setter = matches!(tokens[0], Token::Identifier(ref id) if id == "set");
                        if is_getter || is_setter {
                            tokens.remove(0); // consume get/set
                            if tokens.is_empty() || !matches!(tokens[0], Token::Identifier(_)) {
                                return Err(JSError::ParseError);
                            }
                            let prop_name = if let Token::Identifier(name) = tokens.remove(0) {
                                name
                            } else {
                                return Err(JSError::ParseError);
                            };
                            if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume (
                            let params = parse_parameters(tokens)?;
                            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume {
                            let body = parse_statement_block(tokens)?;
                            if is_getter {
                                if !params.is_empty() {
                                    return Err(JSError::ParseError); // getters should have no parameters
                                }
                                if is_static {
                                    members.push(ClassMember::StaticGetter(prop_name, body));
                                } else {
                                    members.push(ClassMember::Getter(prop_name, body));
                                }
                            } else {
                                // setter
                                if params.len() != 1 {
                                    return Err(JSError::ParseError); // setters should have exactly one parameter
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
                                return Err(JSError::ParseError);
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
                            if tokens.is_empty() || !matches!(tokens[0], Token::Semicolon) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume ;
                            if is_static {
                                members.push(ClassMember::StaticProperty(method_name, value));
                            } else {
                                members.push(ClassMember::Property(method_name, value));
                            }
                        } else {
                            return Err(JSError::ParseError);
                        }
                    }
                } else {
                    return Err(JSError::ParseError);
                }
            }

            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(JSError::ParseError);
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
