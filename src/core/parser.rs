use crate::{
    JSError,
    core::{
        BinaryOp, DestructuringElement, Expr, ObjectDestructuringElement, Statement, StatementKind, TemplatePart, Token, TokenData, Value,
        parse_statements,
    },
    raise_parse_error, raise_parse_error_with_token,
};

pub fn raise_parse_error_at(tokens: &[TokenData]) -> JSError {
    if let Some(t) = tokens.first() {
        raise_parse_error_with_token!(t)
    } else {
        raise_parse_error!()
    }
}

// Helper: Generic binary operator parser for left-associative operators
fn parse_binary_op<F, M>(tokens: &mut Vec<TokenData>, parse_next_level: F, op_mapper: M) -> Result<Expr, JSError>
where
    F: Fn(&mut Vec<TokenData>) -> Result<Expr, JSError>,
    M: Fn(&Token) -> Option<BinaryOp>,
{
    let mut left = parse_next_level(tokens)?;
    loop {
        if tokens.is_empty() {
            break;
        }
        if let Some(op) = op_mapper(&tokens[0].token) {
            tokens.remove(0);
            let right = parse_next_level(tokens)?;
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

#[allow(clippy::type_complexity)]
pub fn parse_parameters(tokens: &mut Vec<TokenData>) -> Result<Vec<(String, Option<Box<Expr>>)>, JSError> {
    let mut params = Vec::new();
    log::trace!(
        "parse_parameters: starting tokens (first 16): {:?}",
        tokens.iter().take(16).collect::<Vec<_>>()
    );
    if !tokens.is_empty() && !matches!(tokens[0].token, Token::RParen) {
        loop {
            if let Some(Token::Identifier(param)) = tokens.first().map(|t| &t.token).cloned() {
                tokens.remove(0);
                log::trace!(
                    "parse_parameters: consumed identifier '{}', remaining (first 8): {:?}",
                    param,
                    tokens.iter().take(8).collect::<Vec<_>>()
                );
                let mut default_expr: Option<Box<Expr>> = None;
                // Support default initializers: identifier '=' expression
                if !tokens.is_empty() && matches!(tokens[0].token, Token::Assign) {
                    tokens.remove(0);
                    let expr = parse_assignment(tokens)?;
                    default_expr = Some(Box::new(expr));
                }
                params.push((param.clone(), default_expr));
                // Support default initializers: identifier '=' expression

                if tokens.is_empty() {
                    return Err(raise_parse_error!(format!(
                        "Unexpected end of parameters; next tokens: {:?}",
                        tokens.iter().take(8).collect::<Vec<_>>()
                    )));
                }
                if matches!(tokens[0].token, Token::RParen) {
                    break;
                }
                if !matches!(tokens[0].token, Token::Comma) {
                    return Err(raise_parse_error!(format!(
                        "Expected ',' in parameter list; next tokens: {:?}",
                        tokens.iter().take(8).collect::<Vec<_>>()
                    )));
                }
                tokens.remove(0); // consume ,
                log::trace!(
                    "parse_parameters: consumed comma, remaining (first 8): {:?}",
                    tokens.iter().take(8).collect::<Vec<_>>()
                );
            } else {
                return Err(raise_parse_error!(format!(
                    "Expected identifier in parameter list; next tokens: {:?}",
                    tokens.iter().take(8).collect::<Vec<_>>()
                )));
            }
        }
    }
    if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
        return Err(raise_parse_error!(format!(
            "Unterminated parameter list or missing ')'; next tokens: {:?}",
            tokens.iter().take(8).collect::<Vec<_>>()
        )));
    }
    tokens.remove(0); // consume )
    log::trace!(
        "parse_parameters: consumed ')', remaining tokens (first 16): {:?}",
        tokens.iter().take(16).collect::<Vec<_>>()
    );
    Ok(params)
}

pub fn parse_statement_block(tokens: &mut Vec<TokenData>) -> Result<Vec<Statement>, JSError> {
    let body = parse_statements(tokens)?;
    if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
        return Err(raise_parse_error!(format!(
            "Expected '}}' to close block; next tokens: {:?}",
            tokens.iter().take(8).collect::<Vec<_>>()
        )));
    }
    tokens.remove(0); // consume }
    Ok(body)
}

pub fn parse_expression(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    // Allow line terminators inside expressions (e.g., after a binary operator
    // at the end of a line). Tokenizer emits `LineTerminator` for newlines —
    // when parsing an expression we should treat those as insignificant
    // whitespace and skip them so expressions that span lines parse correctly.
    while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
        tokens.remove(0);
    }
    log::trace!(
        "parse_object_destructuring_pattern: tokens after initial skip (first 8): {:?}",
        tokens.iter().take(8).collect::<Vec<_>>()
    );
    let mut left = parse_assignment(tokens)?;
    while !tokens.is_empty() && matches!(tokens[0].token, Token::Comma) {
        tokens.remove(0); // consume ,
        let right = parse_assignment(tokens)?;
        left = Expr::Comma(Box::new(left), Box::new(right));
    }
    Ok(left)
}

pub fn parse_conditional(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    let condition = parse_nullish(tokens)?;
    if tokens.is_empty() {
        return Ok(condition);
    }
    if matches!(tokens[0].token, Token::QuestionMark) {
        tokens.remove(0); // consume ?
        let true_expr = parse_conditional(tokens)?; // Allow nesting
        if tokens.is_empty() || !matches!(tokens[0].token, Token::Colon) {
            return Err(raise_parse_error!(format!(
                "Expected ':' in conditional expression; next tokens: {:?}",
                tokens.iter().take(8).collect::<Vec<_>>()
            )));
        }
        tokens.remove(0); // consume :
        let false_expr = parse_conditional(tokens)?; // Allow nesting
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

pub fn parse_assignment(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    let left = parse_conditional(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }

    if let Some(ctor) = get_assignment_ctor(&tokens[0].token) {
        if contains_optional_chain(&left) {
            return Err(raise_parse_error!(format!(
                "Invalid assignment target containing optional chaining; next tokens: {:?}",
                tokens.iter().take(8).collect::<Vec<_>>()
            )));
        }
        tokens.remove(0);
        let right = parse_assignment(tokens)?;
        return Ok(ctor(Box::new(left), Box::new(right)));
    }

    Ok(left)
}

// Operator precedence parsing chain (primary -> exponentiation -> multiplicative
// -> additive -> shift -> relational -> equality -> bitwise-and -> xor -> or
// -> logical-and -> logical-or -> nullish -> conditional -> assignment).

fn parse_shift(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    parse_binary_op(tokens, parse_additive, |token| match token {
        Token::LeftShift => Some(BinaryOp::LeftShift),
        Token::RightShift => Some(BinaryOp::RightShift),
        Token::UnsignedRightShift => Some(BinaryOp::UnsignedRightShift),
        _ => None,
    })
}

fn parse_relational(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    parse_binary_op(tokens, parse_shift, |token| match token {
        Token::LessThan => Some(BinaryOp::LessThan),
        Token::GreaterThan => Some(BinaryOp::GreaterThan),
        Token::LessEqual => Some(BinaryOp::LessEqual),
        Token::GreaterEqual => Some(BinaryOp::GreaterEqual),
        Token::InstanceOf => Some(BinaryOp::InstanceOf),
        Token::In => Some(BinaryOp::In),
        _ => None,
    })
}

fn parse_equality(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    parse_binary_op(tokens, parse_relational, |token| match token {
        Token::Equal => Some(BinaryOp::Equal),
        Token::StrictEqual => Some(BinaryOp::StrictEqual),
        Token::NotEqual => Some(BinaryOp::NotEqual),
        Token::StrictNotEqual => Some(BinaryOp::StrictNotEqual),
        _ => None,
    })
}

fn parse_bitwise_and(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    parse_binary_op(tokens, parse_equality, |token| match token {
        Token::BitAnd => Some(BinaryOp::BitAnd),
        _ => None,
    })
}

fn parse_bitwise_xor_chain(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    parse_binary_op(tokens, parse_bitwise_and, |token| match token {
        Token::BitXor => Some(BinaryOp::BitXor),
        _ => None,
    })
}

fn parse_bitwise_or(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    parse_binary_op(tokens, parse_bitwise_xor_chain, |token| match token {
        Token::BitOr => Some(BinaryOp::BitOr),
        _ => None,
    })
}

fn parse_logical_and(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    let left = parse_bitwise_or(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    if matches!(tokens[0].token, Token::LogicalAnd) {
        tokens.remove(0);
        let right = parse_logical_and(tokens)?;
        Ok(Expr::LogicalAnd(Box::new(left), Box::new(right)))
    } else {
        Ok(left)
    }
}

fn parse_logical_or(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    let left = parse_logical_and(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    if matches!(tokens[0].token, Token::LogicalOr) {
        tokens.remove(0);
        let right = parse_logical_or(tokens)?;
        Ok(Expr::LogicalOr(Box::new(left), Box::new(right)))
    } else {
        Ok(left)
    }
}

fn parse_nullish(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    let left = parse_logical_or(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    if matches!(tokens[0].token, Token::NullishCoalescing) {
        tokens.remove(0);
        let right = parse_nullish(tokens)?;
        Ok(Expr::Binary(Box::new(left), BinaryOp::NullishCoalescing, Box::new(right)))
    } else {
        Ok(left)
    }
}

fn parse_additive(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    parse_binary_op(tokens, parse_multiplicative, |token| match token {
        Token::Plus => Some(BinaryOp::Add),
        Token::Minus => Some(BinaryOp::Sub),
        _ => None,
    })
}

fn parse_multiplicative(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    parse_binary_op(tokens, parse_exponentiation, |token| match token {
        Token::Multiply => Some(BinaryOp::Mul),
        Token::Divide => Some(BinaryOp::Div),
        Token::Mod => Some(BinaryOp::Mod),
        _ => None,
    })
}

fn parse_exponentiation(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    // Right-associative exponentiation operator: a ** b ** c -> a ** (b ** c)
    let left = parse_primary(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    if matches!(tokens[0].token, Token::Exponent) {
        tokens.remove(0);
        let right = parse_exponentiation(tokens)?; // right-associative
        Ok(Expr::Binary(Box::new(left), BinaryOp::Pow, Box::new(right)))
    } else {
        Ok(left)
    }
}

fn parse_primary(tokens: &mut Vec<TokenData>) -> Result<Expr, JSError> {
    // Skip any leading line terminators inside expressions so multi-line
    // expression continuations like `a +\n b` parse correctly.
    while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
        tokens.remove(0);
    }
    if tokens.is_empty() {
        return Err(raise_parse_error_at(tokens));
    }
    let token_data = tokens.remove(0);
    let current = token_data.token.clone();
    let mut expr = match current {
        Token::Number(n) => Expr::Number(n),
        Token::BigInt(s) => Expr::BigInt(s.clone()),
        Token::StringLit(s) => Expr::StringLit(s),
        Token::True => Expr::Boolean(true),
        Token::False => Expr::Boolean(false),
        Token::Null => Expr::Value(Value::Null),
        Token::TypeOf => {
            let inner = parse_primary(tokens)?;
            Expr::TypeOf(Box::new(inner))
        }
        Token::Delete => {
            let inner = parse_primary(tokens)?;
            Expr::Delete(Box::new(inner))
        }
        Token::Void => {
            let inner = parse_primary(tokens)?;
            Expr::Void(Box::new(inner))
        }
        Token::Await => {
            let inner = parse_primary(tokens)?;
            Expr::Await(Box::new(inner))
        }
        Token::Yield => {
            // yield can be followed by an optional expression
            if tokens.is_empty()
                || matches!(
                    tokens[0].token,
                    Token::Semicolon | Token::Comma | Token::RParen | Token::RBracket | Token::RBrace | Token::Colon
                )
            {
                Expr::Yield(None)
            } else {
                let inner = parse_assignment(tokens)?;
                Expr::Yield(Some(Box::new(inner)))
            }
        }
        Token::YieldStar => {
            let inner = parse_assignment(tokens)?;
            Expr::YieldStar(Box::new(inner))
        }
        Token::LogicalNot => {
            let inner = parse_primary(tokens)?;
            Expr::LogicalNot(Box::new(inner))
        }
        Token::New => {
            // Constructor should be a simple identifier or property access, not a full expression
            let constructor = if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                let line = tokens[0].line;
                let column = tokens[0].column;
                tokens.remove(0);
                Expr::Var(name, Some(line), Some(column))
            } else {
                return Err(raise_parse_error_at(tokens));
            };
            let args = if !tokens.is_empty() && matches!(tokens[0].token, Token::LParen) {
                tokens.remove(0); // consume '('
                let mut args = Vec::new();
                if !tokens.is_empty() && !matches!(tokens[0].token, Token::RParen) {
                    loop {
                        let arg = parse_assignment(tokens)?;
                        args.push(arg);
                        if tokens.is_empty() {
                            return Err(raise_parse_error_at(tokens));
                        }
                        if matches!(tokens[0].token, Token::RParen) {
                            break;
                        }
                        if !matches!(tokens[0].token, Token::Comma) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume ','
                        // allow trailing comma + optional line terminators before ')'
                        while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
                            tokens.remove(0);
                        }
                        if tokens.is_empty() {
                            return Err(raise_parse_error_at(tokens));
                        }
                        if matches!(tokens[0].token, Token::RParen) {
                            // trailing comma
                            break;
                        }
                    }
                }
                if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume ')'
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
            let inner = parse_primary(tokens)?;
            Expr::UnaryNeg(Box::new(inner))
        }
        Token::Plus => {
            let inner = parse_primary(tokens)?;
            Expr::UnaryPlus(Box::new(inner))
        }
        Token::BitNot => {
            let inner = parse_primary(tokens)?;
            Expr::BitNot(Box::new(inner))
        }
        Token::Increment => {
            let inner = parse_primary(tokens)?;
            Expr::Increment(Box::new(inner))
        }
        Token::Decrement => {
            let inner = parse_primary(tokens)?;
            Expr::Decrement(Box::new(inner))
        }
        Token::Spread => {
            let inner = parse_primary(tokens)?;
            Expr::Spread(Box::new(inner))
        }
        Token::TemplateString(parts) => {
            if parts.is_empty() {
                Expr::StringLit(Vec::new())
            } else if parts.len() == 1 {
                match &parts[0] {
                    TemplatePart::String(s) => Expr::StringLit(s.clone()),
                    TemplatePart::Expr(expr_tokens) => {
                        let mut expr_tokens = expr_tokens.clone();
                        parse_expression(&mut expr_tokens)?
                    }
                }
            } else {
                // Build binary addition chain
                let mut expr = match &parts[0] {
                    TemplatePart::String(s) => Expr::StringLit(s.clone()),
                    TemplatePart::Expr(expr_tokens) => {
                        let mut expr_tokens = expr_tokens.clone();
                        parse_expression(&mut expr_tokens)?
                    }
                };
                for part in &parts[1..] {
                    let right = match part {
                        TemplatePart::String(s) => Expr::StringLit(s.clone()),
                        TemplatePart::Expr(expr_tokens) => {
                            let mut expr_tokens = expr_tokens.clone();
                            parse_expression(&mut expr_tokens)?
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
            if !tokens.is_empty() && matches!(tokens[0].token, Token::Arrow) {
                tokens.remove(0);
                let body = parse_arrow_body(tokens)?;
                expr = Expr::ArrowFunction(vec![(name, None)], body);
            }
            expr
        }
        Token::Import => Expr::Var("import".to_string(), Some(token_data.line), Some(token_data.column)),
        Token::Regex(pattern, flags) => Expr::Regex(pattern.clone(), flags.clone()),
        Token::This => Expr::This,
        Token::Super => {
            // Check if followed by ( for super() call
            if !tokens.is_empty() && matches!(tokens[0].token, Token::LParen) {
                tokens.remove(0); // consume '('
                let mut args = Vec::new();
                if !tokens.is_empty() && !matches!(tokens[0].token, Token::RParen) {
                    loop {
                        let arg = parse_assignment(tokens)?;
                        args.push(arg);
                        if tokens.is_empty() {
                            return Err(raise_parse_error_at(tokens));
                        }
                        if matches!(tokens[0].token, Token::RParen) {
                            break;
                        }
                        if !matches!(tokens[0].token, Token::Comma) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume ','
                    }
                }
                if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume ')'
                Expr::SuperCall(args)
            } else if !tokens.is_empty() && matches!(tokens[0].token, Token::Dot) {
                tokens.remove(0); // consume '.'
                if tokens.is_empty() || !matches!(tokens[0].token, Token::Identifier(_)) {
                    return Err(raise_parse_error_at(tokens));
                }
                let prop = if let Token::Identifier(name) = tokens.remove(0).token {
                    name
                } else {
                    return Err(raise_parse_error_at(tokens));
                };
                // Check if followed by ( for method call
                if !tokens.is_empty() && matches!(tokens[0].token, Token::LParen) {
                    tokens.remove(0); // consume '('
                    let mut args = Vec::new();
                    if !tokens.is_empty() && !matches!(tokens[0].token, Token::RParen) {
                        loop {
                            let arg = parse_assignment(tokens)?;
                            args.push(arg);
                            if tokens.is_empty() {
                                return Err(raise_parse_error_at(tokens));
                            }
                            if matches!(tokens[0].token, Token::RParen) {
                                break;
                            }
                            if !matches!(tokens[0].token, Token::Comma) {
                                return Err(raise_parse_error_at(tokens));
                            }
                            tokens.remove(0); // consume ','
                            // permit trailing comma before ) and skip newlines
                            while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
                                tokens.remove(0);
                            }
                            if tokens.is_empty() {
                                return Err(raise_parse_error_at(tokens));
                            }
                            if matches!(tokens[0].token, Token::RParen) {
                                break;
                            }
                        }
                    }
                    if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    tokens.remove(0); // consume ')'
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
            while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
                tokens.remove(0);
            }
            let mut properties = Vec::new();
            if !tokens.is_empty() && matches!(tokens[0].token, Token::RBrace) {
                // Empty object {}
                tokens.remove(0); // consume }
                return Ok(Expr::Object(properties));
            }
            loop {
                log::trace!(
                    "parse_primary: object literal loop; next tokens (first 8): {:?}",
                    tokens.iter().take(8).collect::<Vec<_>>()
                );
                // Skip blank lines that may appear between properties.
                while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator | Token::Semicolon) {
                    tokens.remove(0);
                }

                // If we hit the closing brace after skipping blank lines,
                // consume it and finish the object literal. This handles
                // trailing commas followed by whitespace/newlines before `}`.
                if !tokens.is_empty() && matches!(tokens[0].token, Token::RBrace) {
                    tokens.remove(0); // consume }
                    break;
                }
                if tokens.is_empty() {
                    return Err(raise_parse_error_at(tokens));
                }
                // Check for spread
                if !tokens.is_empty() && matches!(tokens[0].token, Token::Spread) {
                    log::trace!(
                        "parse_primary: object property is spread; next tokens (first 8): {:?}",
                        tokens.iter().take(8).collect::<Vec<_>>()
                    );
                    tokens.remove(0); // consume ...
                    // Use parse_assignment here so a spread is a single expression
                    // and doesn't accidentally capture following comma-separated
                    // properties via the comma operator.
                    let expr = parse_assignment(tokens)?;
                    // Use empty string as key for spread
                    properties.push((Expr::Value(Value::String(Vec::new())), Expr::Spread(Box::new(expr)), false));
                } else {
                    // Check for getter/setter: only treat as getter/setter if the
                    // identifier 'get'/'set' is followed by a property key and
                    // an opening parenthesis (no colon). This avoids confusing a
                    // regular property named 'get'/'set' (e.g. `set: function(...)`) with
                    // the getter/setter syntax.
                    // Recognize getter/setter signatures including computed keys
                    let is_getter = if tokens.len() >= 2 && matches!(tokens[0].token, Token::Identifier(ref id) if id == "get") {
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

                    let is_setter = if tokens.len() >= 2 && matches!(tokens[0].token, Token::Identifier(ref id) if id == "set") {
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
                        tokens.remove(0); // consume get/set
                    }

                    // Parse key
                    let mut is_shorthand_candidate = false;
                    let key_expr = if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                        // Check for concise method: Identifier + (
                        if !is_getter && !is_setter && tokens.len() >= 2 && matches!(tokens[1].token, Token::LParen) {
                            // Concise method
                            tokens.remove(0); // consume name
                            tokens.remove(0); // consume (
                            let params = parse_parameters(tokens)?;
                            if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                                return Err(raise_parse_error_at(tokens));
                            }
                            tokens.remove(0); // consume {
                            let body = parse_statements(tokens)?;
                            if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                                return Err(raise_parse_error_at(tokens));
                            }
                            tokens.remove(0); // consume }
                            properties.push((
                                Expr::Value(Value::String(crate::unicode::utf8_to_utf16(&name))),
                                Expr::Function(None, params, body),
                                true,
                            ));

                            // After adding method, skip any newline/semicolons and handle comma/end in outer loop
                            while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator | Token::Semicolon) {
                                tokens.remove(0);
                            }
                            if tokens.is_empty() {
                                return Err(raise_parse_error_at(tokens));
                            }
                            if matches!(tokens[0].token, Token::RBrace) {
                                tokens.remove(0);
                                break;
                            }
                            if matches!(tokens[0].token, Token::Comma) {
                                tokens.remove(0);
                                continue;
                            }
                            continue;
                        }
                        is_shorthand_candidate = true;
                        tokens.remove(0);
                        Expr::Value(Value::String(crate::unicode::utf8_to_utf16(&name)))
                    } else if let Some(Token::Number(n)) = tokens.first().map(|t| t.token.clone()) {
                        // Numeric property keys are allowed in object literals (they become strings)
                        tokens.remove(0);
                        // Format as integer if whole number, otherwise use default representation
                        let s = if n.fract() == 0.0 { format!("{}", n as i64) } else { n.to_string() };
                        Expr::Value(Value::String(crate::unicode::utf8_to_utf16(&s)))
                    } else if let Some(Token::StringLit(s)) = tokens.first().map(|t| t.token.clone()) {
                        tokens.remove(0);
                        Expr::Value(Value::String(s))
                    } else if let Some(Token::Default) = tokens.first().map(|t| t.token.clone()) {
                        // allow the reserved word `default` as an object property key
                        tokens.remove(0);
                        Expr::Value(Value::String(crate::unicode::utf8_to_utf16("default")))
                    } else if !tokens.is_empty() && matches!(tokens[0].token, Token::LBracket) {
                        // Computed key (e.g., get [Symbol.toPrimitive]())
                        tokens.remove(0); // consume [
                        let expr = parse_assignment(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::RBracket) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume ]
                        expr
                    } else {
                        return Err(raise_parse_error_at(tokens));
                    };

                    // Check for method definition after computed key
                    if !is_getter && !is_setter && !tokens.is_empty() && matches!(tokens[0].token, Token::LParen) {
                        tokens.remove(0); // consume (
                        let params = parse_parameters(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume {
                        let body = parse_statements(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume }
                        properties.push((key_expr, Expr::Function(None, params, body), true));

                        // After adding method, skip any newline/semicolons and handle comma/end in outer loop
                        while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator | Token::Semicolon) {
                            tokens.remove(0);
                        }
                        if tokens.is_empty() {
                            return Err(raise_parse_error_at(tokens));
                        }
                        if matches!(tokens[0].token, Token::RBrace) {
                            tokens.remove(0);
                            break;
                        }
                        if matches!(tokens[0].token, Token::Comma) {
                            tokens.remove(0);
                            continue;
                        }
                        continue;
                    }

                    if is_getter {
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::LParen) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume (
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume )
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume {
                        let body = parse_statements(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume }
                        properties.push((key_expr, Expr::Getter(Box::new(Expr::Function(None, Vec::new(), body))), false));
                    } else if is_setter {
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::LParen) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume (
                        let params = parse_parameters(tokens)?;
                        if params.len() != 1 {
                            return Err(raise_parse_error!(format!("Setter must have exactly one parameter")));
                        }
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume {
                        let body = parse_statements(tokens)?;
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume }
                        properties.push((key_expr, Expr::Setter(Box::new(Expr::Function(None, params, body))), false));
                    } else {
                        // Regular property
                        if !tokens.is_empty() && matches!(tokens[0].token, Token::Colon) {
                            tokens.remove(0); // consume :
                            let value = parse_assignment(tokens)?;
                            properties.push((key_expr, value, false));
                        } else {
                            // Shorthand property { x } -> { x: x }
                            if is_shorthand_candidate {
                                if let Expr::Value(Value::String(s)) = &key_expr {
                                    let name = String::from_utf16_lossy(s);
                                    properties.push((key_expr, Expr::Var(name, None, None), false));
                                } else {
                                    return Err(raise_parse_error!(format!("Invalid shorthand property")));
                                }
                            } else {
                                return Err(raise_parse_error!(format!("Expected ':' after property key")));
                            }
                        }
                    }
                }

                // Handle comma
                if !tokens.is_empty() && matches!(tokens[0].token, Token::Comma) {
                    tokens.remove(0);
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
            if !tokens.is_empty() && matches!(tokens[0].token, Token::RBracket) {
                // Empty array []
                tokens.remove(0); // consume ]
                return Ok(Expr::Array(elements));
            }
            loop {
                // Skip leading blank lines inside array literals to avoid
                // attempting to parse a `]` or other tokens as elements.
                while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator | Token::Semicolon) {
                    tokens.remove(0);
                }
                // If next token is a closing bracket then the array is complete
                // This handles trailing commas like `[1, 2,]` correctly — we should
                // stop and not attempt to parse a non-existent element.
                if !tokens.is_empty() && matches!(tokens[0].token, Token::RBracket) {
                    tokens.remove(0); // consume ]
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
                if matches!(tokens[0].token, Token::Comma) {
                    // Push an explicit `undefined` element to represent the elision
                    elements.push(Expr::Value(Value::Undefined));
                    tokens.remove(0); // consume comma representing empty slot
                    // After consuming the comma, allow the loop to continue and
                    // possibly encounter another comma or the closing bracket.
                    // If the next token is RBracket we'll handle completion below.
                    continue;
                }

                // Parse element expression
                let elem = parse_assignment(tokens)?;
                elements.push(elem);

                // Check for comma or end. Allow intervening line terminators
                // between elements so array items can be split across lines.
                while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator | Token::Semicolon) {
                    tokens.remove(0);
                }

                if tokens.is_empty() {
                    return Err(raise_parse_error_at(tokens));
                }
                if matches!(tokens[0].token, Token::RBracket) {
                    tokens.remove(0); // consume ]
                    break;
                } else if matches!(tokens[0].token, Token::Comma) {
                    tokens.remove(0); // consume ,
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
            let name = if !tokens.is_empty() {
                if let Token::Identifier(n) = &tokens[0].token {
                    // Look ahead for next non-LineTerminator token
                    let mut idx = 1usize;
                    while idx < tokens.len() && matches!(tokens[idx].token, Token::LineTerminator) {
                        idx += 1;
                    }
                    if idx < tokens.len() && matches!(tokens[idx].token, Token::LParen) {
                        let name = n.clone();
                        log::trace!("parse_primary: treating '{}' as function name", name);
                        tokens.remove(0);
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
            if !tokens.is_empty() && (matches!(tokens[0].token, Token::LParen) || matches!(tokens[0].token, Token::Identifier(_))) {
                if matches!(tokens[0].token, Token::LParen) {
                    tokens.remove(0); // consume "("
                }
                log::trace!(
                    "parse_primary: about to call parse_parameters; tokens (first 8): {:?}",
                    tokens.iter().take(8).collect::<Vec<_>>()
                );
                let params = parse_parameters(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume {
                let body = parse_statements(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume }
                if is_generator {
                    log::trace!("parse_primary: constructed GeneratorFunction name={:?} params={:?}", name, params);
                    Expr::GeneratorFunction(name, params, body)
                } else {
                    log::trace!("parse_primary: constructed Function name={:?} params={:?}", name, params);
                    Expr::Function(name, params, body)
                }
            } else if !tokens.is_empty() && matches!(tokens[0].token, Token::RParen) {
                // Defensive case: treat `) {` as an empty parameter list
                tokens.remove(0); // consume ')'
                if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume {
                let body = parse_statements(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume }
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
            if !tokens.is_empty() && matches!(tokens[0].token, Token::Function) {
                tokens.remove(0); // consume function
                // Optional name for async function expressions (same rules as normal functions)
                let name = if !tokens.is_empty() {
                    if let Token::Identifier(n) = &tokens[0].token {
                        // Look ahead for next non-LineTerminator token
                        let mut idx = 1usize;
                        while idx < tokens.len() && matches!(tokens[idx].token, Token::LineTerminator) {
                            idx += 1;
                        }
                        if idx < tokens.len() && matches!(tokens[idx].token, Token::LParen) {
                            let name = n.clone();
                            log::trace!("parse_primary: treating '{}' as async function name", name);
                            tokens.remove(0);
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

                if !tokens.is_empty() && matches!(tokens[0].token, Token::LParen) {
                    tokens.remove(0); // consume "("
                    let params = parse_parameters(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    tokens.remove(0); // consume {
                    let body = parse_statements(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    tokens.remove(0); // consume }
                    Expr::AsyncFunction(name, params, body)
                } else {
                    return Err(raise_parse_error_at(tokens));
                }
            } else if !tokens.is_empty() && matches!(tokens[0].token, Token::LParen) {
                // Async arrow function
                tokens.remove(0); // consume (
                let mut params: Vec<(String, Option<Box<Expr>>)> = Vec::new();
                let mut is_arrow = false;
                if matches!(tokens.first().map(|t| &t.token), Some(&Token::RParen)) {
                    tokens.remove(0);
                    if !tokens.is_empty() && matches!(tokens[0].token, Token::Arrow) {
                        tokens.remove(0);
                        is_arrow = true;
                    } else {
                        return Err(raise_parse_error_at(tokens));
                    }
                } else {
                    // Try to parse params
                    let mut param_names: Vec<(String, Option<Box<Expr>>)> = Vec::new();
                    let mut local_consumed = Vec::new();
                    let mut valid = true;
                    loop {
                        if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                            let t = tokens.remove(0);
                            local_consumed.push(t);
                            param_names.push((name, None));
                            if tokens.is_empty() {
                                valid = false;
                                break;
                            }
                            if matches!(tokens[0].token, Token::RParen) {
                                let t = tokens.remove(0);
                                local_consumed.push(t);
                                if !tokens.is_empty() && matches!(tokens[0].token, Token::Arrow) {
                                    tokens.remove(0);
                                    is_arrow = true;
                                } else {
                                    valid = false;
                                }
                                break;
                            } else if matches!(tokens[0].token, Token::Comma) {
                                let t = tokens.remove(0);
                                local_consumed.push(t);
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
                        // Put back local_consumed
                        for t in local_consumed.into_iter().rev() {
                            tokens.insert(0, t);
                        }
                        return Err(raise_parse_error_at(tokens));
                    }
                    params = param_names;
                }
                if is_arrow {
                    // For async arrow functions, we need to create a special async closure
                    // For now, we'll treat them as regular arrow functions but mark them as async
                    // This will need to be handled in evaluation
                    Expr::AsyncArrowFunction(params, parse_arrow_body(tokens)?)
                } else {
                    return Err(raise_parse_error_at(tokens));
                }
            } else {
                return Err(raise_parse_error_at(tokens));
            }
        }
        Token::LParen => {
            // Check if it's arrow function
            let mut params: Vec<(String, Option<Box<Expr>>)> = Vec::new();
            let mut is_arrow = false;
            let mut result_expr = None;
            if matches!(tokens.first().map(|t| &t.token), Some(&Token::RParen)) {
                tokens.remove(0);
                if !tokens.is_empty() && matches!(tokens[0].token, Token::Arrow) {
                    tokens.remove(0);
                    is_arrow = true;
                } else {
                    return Err(raise_parse_error_at(tokens));
                }
            } else {
                // Try to parse params
                let mut param_names: Vec<(String, Option<Box<Expr>>)> = Vec::new();
                let mut local_consumed = Vec::new();
                let mut valid = true;
                loop {
                    log::trace!(
                        "parse_primary LParen param loop: tokens first={:?} local_consumed_len={} param_names_len={}",
                        tokens.first(),
                        local_consumed.len(),
                        param_names.len()
                    );
                    if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                        let t = tokens.remove(0);
                        local_consumed.push(t);
                        param_names.push((name, None));
                        if tokens.is_empty() {
                            valid = false;
                            break;
                        }

                        // Support default initializers in parameter lists: identifier '=' expression
                        if matches!(tokens[0].token, Token::Assign) {
                            // Speculatively parse the default expression on a clone so we can
                            // rollback if this isn't actually an arrow parameter list.
                            let mut tmp = tokens.clone();
                            tmp.remove(0); // consume '=' in tmp
                            if parse_expression(&mut tmp).is_ok() {
                                // After parsing the default expression, tmp should start with
                                // either ',' or ')' for a valid parameter list.
                                if !tmp.is_empty() && (matches!(tmp[0].token, Token::Comma) || matches!(tmp[0].token, Token::RParen)) {
                                    // consume the same tokens from the real tokens vector and
                                    // record them for possible rollback
                                    let consumed = tokens.len() - tmp.len();
                                    for _ in 0..consumed {
                                        local_consumed.push(tokens.remove(0));
                                    }
                                    if tokens.is_empty() {
                                        valid = false;
                                        break;
                                    }
                                    if matches!(tokens[0].token, Token::RParen) {
                                        let t = tokens.remove(0);
                                        local_consumed.push(t);
                                        if !tokens.is_empty() && matches!(tokens[0].token, Token::Arrow) {
                                            tokens.remove(0);
                                            is_arrow = true;
                                        } else {
                                            valid = false;
                                        }
                                        break;
                                    } else if matches!(tokens[0].token, Token::Comma) {
                                        let t = tokens.remove(0);
                                        local_consumed.push(t);
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

                        if matches!(tokens[0].token, Token::RParen) {
                            let t = tokens.remove(0);
                            local_consumed.push(t);
                            if !tokens.is_empty() && matches!(tokens[0].token, Token::Arrow) {
                                tokens.remove(0);
                                is_arrow = true;
                            } else {
                                valid = false;
                            }
                            break;
                        } else if matches!(tokens[0].token, Token::Comma) {
                            let t = tokens.remove(0);
                            local_consumed.push(t);
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
                    // Put back local_consumed
                    for t in local_consumed.into_iter().rev() {
                        tokens.insert(0, t);
                    }
                    // Parse as expression
                    let expr_inner = parse_expression(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    tokens.remove(0);
                    result_expr = Some(expr_inner);
                }
            }
            if is_arrow {
                Expr::ArrowFunction(params, parse_arrow_body(tokens)?)
            } else {
                result_expr.unwrap()
            }
        }
        _ => {
            // Provide better error information for unexpected tokens during parsing
            // Log the remaining tokens for better context to help debugging
            if !tokens.is_empty() {
                log::debug!(
                    "parse_expression unexpected token: {:?}; remaining tokens: {:?}",
                    tokens[0].token,
                    tokens
                );
            } else {
                log::debug!("parse_expression unexpected end of tokens; tokens empty");
            }
            return Err(raise_parse_error_at(tokens));
        }
    };

    // Handle postfix operators like index access. Accept line terminators
    // between the primary and the postfix operator to support call-chains
    // split across lines (e.g. `promise.then(...)
    // .then(...)`).
    while !tokens.is_empty() {
        while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
            tokens.remove(0);
        }
        if tokens.is_empty() {
            break;
        }
        match &tokens[0].token {
            Token::LBracket => {
                tokens.remove(0); // consume '['
                let index_expr = parse_expression(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0].token, Token::RBracket) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume ']'
                expr = Expr::Index(Box::new(expr), Box::new(index_expr));
            }
            Token::Dot => {
                tokens.remove(0); // consume '.'
                if tokens.is_empty() {
                    return Err(raise_parse_error_at(tokens));
                }
                if let Some(prop) = tokens[0].token.as_identifier_string() {
                    tokens.remove(0);
                    expr = Expr::Property(Box::new(expr), prop);
                } else {
                    return Err(raise_parse_error_at(tokens));
                }
            }
            Token::OptionalChain => {
                tokens.remove(0); // consume '?.'
                if tokens.is_empty() {
                    return Err(raise_parse_error_at(tokens));
                }
                if matches!(tokens[0].token, Token::LParen) {
                    // Optional call: obj?.method(args)
                    tokens.remove(0); // consume '('
                    let mut args = Vec::new();
                    if !tokens.is_empty() && !matches!(tokens[0].token, Token::RParen) {
                        loop {
                            let arg = parse_assignment(tokens)?;
                            args.push(arg);
                            if tokens.is_empty() {
                                return Err(raise_parse_error_at(tokens));
                            }
                            if matches!(tokens[0].token, Token::RParen) {
                                break;
                            }
                            if !matches!(tokens[0].token, Token::Comma) {
                                return Err(raise_parse_error_at(tokens));
                            }
                            tokens.remove(0); // consume ','
                        }
                    }
                    if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    tokens.remove(0); // consume ')'
                    expr = Expr::OptionalCall(Box::new(expr), args);
                } else if matches!(tokens[0].token, Token::Identifier(_)) {
                    // Optional property access: obj?.prop
                    if let Some(prop) = tokens[0].token.as_identifier_string() {
                        tokens.remove(0);
                        expr = Expr::OptionalProperty(Box::new(expr), prop);
                    } else {
                        return Err(raise_parse_error_at(tokens));
                    }
                } else if matches!(tokens[0].token, Token::LBracket) {
                    // Optional computed property access: obj?.[expr]
                    tokens.remove(0); // consume '['
                    let index_expr = parse_expression(tokens)?;
                    if tokens.is_empty() || !matches!(tokens[0].token, Token::RBracket) {
                        return Err(raise_parse_error_at(tokens));
                    }
                    tokens.remove(0); // consume ']'
                    // If the bracket access is immediately followed by a call,
                    // e.g. `obj?.[expr](...)`, this is an optional call on the
                    // computed property. Parse the call arguments and build an
                    // OptionalCall around the computed access.
                    if !tokens.is_empty() && matches!(tokens[0].token, Token::LParen) {
                        tokens.remove(0); // consume '('
                        let mut args = Vec::new();
                        if !tokens.is_empty() && !matches!(tokens[0].token, Token::RParen) {
                            loop {
                                let arg = parse_assignment(tokens)?;
                                args.push(arg);
                                if tokens.is_empty() {
                                    return Err(raise_parse_error_at(tokens));
                                }
                                if matches!(tokens[0].token, Token::RParen) {
                                    break;
                                }
                                if !matches!(tokens[0].token, Token::Comma) {
                                    return Err(raise_parse_error_at(tokens));
                                }
                                tokens.remove(0); // consume ','
                            }
                        }
                        if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume ')'
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
                tokens.remove(0); // consume '('
                let mut args = Vec::new();
                if !tokens.is_empty() && !matches!(tokens[0].token, Token::RParen) {
                    loop {
                        let arg = parse_assignment(tokens)?;
                        args.push(arg);
                        if tokens.is_empty() {
                            return Err(raise_parse_error_at(tokens));
                        }
                        if matches!(tokens[0].token, Token::RParen) {
                            break;
                        }
                        if !matches!(tokens[0].token, Token::Comma) {
                            return Err(raise_parse_error_at(tokens));
                        }
                        tokens.remove(0); // consume ','
                        // allow trailing comma before ')' and skip newlines
                        while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
                            tokens.remove(0);
                        }
                        if tokens.is_empty() {
                            return Err(raise_parse_error_at(tokens));
                        }
                        if matches!(tokens[0].token, Token::RParen) {
                            break;
                        }
                    }
                }
                if tokens.is_empty() || !matches!(tokens[0].token, Token::RParen) {
                    return Err(raise_parse_error_at(tokens));
                }
                tokens.remove(0); // consume ')'
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
                tokens.remove(0);
                expr = Expr::PostIncrement(Box::new(expr));
            }
            Token::Decrement => {
                tokens.remove(0);
                expr = Expr::PostDecrement(Box::new(expr));
            }
            _ => break,
        }
    }

    Ok(expr)
}

fn parse_arrow_body(tokens: &mut Vec<TokenData>) -> Result<Vec<Statement>, JSError> {
    if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
        tokens.remove(0);
        let body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
            return Err(raise_parse_error_at(tokens));
        }
        tokens.remove(0);
        Ok(body)
    } else {
        let expr = parse_expression(tokens)?;
        Ok(vec![Statement::from(StatementKind::Return(Some(expr)))])
    }
}

pub fn parse_array_destructuring_pattern(tokens: &mut Vec<TokenData>) -> Result<Vec<DestructuringElement>, JSError> {
    if tokens.is_empty() || !matches!(tokens[0].token, Token::LBracket) {
        return Err(raise_parse_error_at(tokens));
    }
    tokens.remove(0); // consume [

    let mut pattern = Vec::new();
    // Skip initial blank lines inside the pattern
    while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
        tokens.remove(0);
    }
    if !tokens.is_empty() && matches!(tokens[0].token, Token::RBracket) {
        tokens.remove(0); // consume ]
        return Ok(pattern);
    }

    loop {
        // skip any blank lines at the start of a new property entry
        while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
            tokens.remove(0);
        }
        if !tokens.is_empty() && matches!(tokens[0].token, Token::Spread) {
            tokens.remove(0); // consume ...
            if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                tokens.remove(0);
                pattern.push(DestructuringElement::Rest(name));
            } else {
                return Err(raise_parse_error_at(tokens));
            }
            // Rest must be the last element
            if tokens.is_empty() || !matches!(tokens[0].token, Token::RBracket) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume ]
            break;
        } else if !tokens.is_empty() && matches!(tokens[0].token, Token::Comma) {
            tokens.remove(0); // consume ,
            pattern.push(DestructuringElement::Empty);
        } else if !tokens.is_empty() && matches!(tokens[0].token, Token::LBracket) {
            // Nested array destructuring
            let nested_pattern = parse_array_destructuring_pattern(tokens)?;
            pattern.push(DestructuringElement::NestedArray(nested_pattern));
        } else if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
            // Nested object destructuring
            let nested_pattern = parse_object_destructuring_pattern(tokens)?;
            pattern.push(DestructuringElement::NestedObject(nested_pattern));
        } else if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
            tokens.remove(0);
            // Accept optional default initializer in patterns: e.g. `a = 1`
            let mut default_expr: Option<Box<Expr>> = None;
            if !tokens.is_empty() && matches!(tokens[0].token, Token::Assign) {
                tokens.remove(0); // consume '='
                // capture initializer tokens until top-level comma or ] and parse them
                let mut depth: i32 = 0;
                let mut init_tokens: Vec<TokenData> = Vec::new();
                while !tokens.is_empty() {
                    if depth == 0 && (matches!(tokens[0].token, Token::Comma) || matches!(tokens[0].token, Token::RBracket)) {
                        break;
                    }
                    match tokens[0].token {
                        Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                        Token::RParen | Token::RBracket | Token::RBrace => depth -= 1,
                        _ => {}
                    }
                    init_tokens.push(tokens.remove(0));
                }
                if !init_tokens.is_empty() {
                    let mut tmp = init_tokens.clone();
                    let expr = parse_expression(&mut tmp)?;
                    default_expr = Some(Box::new(expr));
                }
            }
            pattern.push(DestructuringElement::Variable(name, default_expr));
        } else {
            return Err(raise_parse_error_at(tokens));
        }

        // allow blank lines between last element and closing brace
        while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
            tokens.remove(0);
        }

        if tokens.is_empty() {
            return Err(raise_parse_error_at(tokens));
        }
        if matches!(tokens[0].token, Token::RBracket) {
            tokens.remove(0); // consume ]
            break;
        } else if matches!(tokens[0].token, Token::Comma) {
            tokens.remove(0); // consume ,
        } else {
            return Err(raise_parse_error_at(tokens));
        }
    }

    Ok(pattern)
}

pub fn parse_object_destructuring_pattern(tokens: &mut Vec<TokenData>) -> Result<Vec<ObjectDestructuringElement>, JSError> {
    if tokens.is_empty() || !matches!(tokens[0].token, Token::LBrace) {
        return Err(raise_parse_error_at(tokens));
    }
    tokens.remove(0); // consume {

    let mut pattern = Vec::new();
    log::trace!(
        "parse_object_destructuring_pattern: tokens immediately after '{{' (first 8): {:?}",
        tokens.iter().take(8).collect::<Vec<_>>()
    );
    // Skip leading line terminators inside the pattern so multi-line
    // object patterns like `{
    //   a = 0,
    // }` are accepted.
    while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
        tokens.remove(0);
    }

    if !tokens.is_empty() && matches!(tokens[0].token, Token::RBrace) {
        tokens.remove(0); // consume }
        return Ok(pattern);
    }

    loop {
        // allow and skip blank lines between elements
        while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
            tokens.remove(0);
        }
        // If after skipping blanks we immediately hit a closing brace, accept
        // it. This handles the common formatting where there is a trailing
        // comma and then a newline before the closing `}` (e.g.
        // `a = 0,\n}`) which should be treated as the end of the object
        // pattern instead of expecting another property.
        if !tokens.is_empty() && matches!(tokens[0].token, Token::RBrace) {
            tokens.remove(0); // consume }
            break;
        }
        if !tokens.is_empty() && matches!(tokens[0].token, Token::Spread) {
            tokens.remove(0); // consume ...
            if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                tokens.remove(0);
                pattern.push(ObjectDestructuringElement::Rest(name));
            } else {
                return Err(raise_parse_error_at(tokens));
            }
            // Rest must be the last element
            if tokens.is_empty() || !matches!(tokens[0].token, Token::RBrace) {
                return Err(raise_parse_error_at(tokens));
            }
            tokens.remove(0); // consume }
            break;
        } else {
            // Parse property
            let key = if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                tokens.remove(0);
                name
            } else {
                log::trace!(
                    "parse_object_destructuring_pattern: expected Identifier for property key but got {:?}",
                    tokens.first()
                );
                return Err(raise_parse_error_at(tokens));
            };

            let value = if !tokens.is_empty() && matches!(tokens[0].token, Token::Colon) {
                tokens.remove(0); // consume :
                // Parse the value pattern
                if !tokens.is_empty() && matches!(tokens[0].token, Token::LBracket) {
                    DestructuringElement::NestedArray(parse_array_destructuring_pattern(tokens)?)
                } else if !tokens.is_empty() && matches!(tokens[0].token, Token::LBrace) {
                    DestructuringElement::NestedObject(parse_object_destructuring_pattern(tokens)?)
                } else if let Some(Token::Identifier(name)) = tokens.first().map(|t| t.token.clone()) {
                    tokens.remove(0);
                    // Allow default initializer for property value like `a: b = 1`
                    let mut default_expr: Option<Box<Expr>> = None;
                    if !tokens.is_empty() && matches!(tokens[0].token, Token::Assign) {
                        tokens.remove(0);
                        let mut depth: i32 = 0;
                        let mut init_tokens: Vec<TokenData> = Vec::new();
                        while !tokens.is_empty() {
                            if depth == 0 && (matches!(tokens[0].token, Token::Comma) || matches!(tokens[0].token, Token::RBrace)) {
                                break;
                            }
                            match tokens[0].token {
                                Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                                Token::RParen | Token::RBracket | Token::RBrace => depth -= 1,
                                _ => {}
                            }
                            init_tokens.push(tokens.remove(0));
                        }
                        if !init_tokens.is_empty() {
                            let mut tmp = init_tokens.clone();
                            let expr = parse_expression(&mut tmp)?;
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
                if !tokens.is_empty() && matches!(tokens[0].token, Token::Assign) {
                    tokens.remove(0); // consume '='
                    let mut depth: i32 = 0;
                    while !tokens.is_empty() {
                        if depth == 0 && (matches!(tokens[0].token, Token::Comma) || matches!(tokens[0].token, Token::RBrace)) {
                            break;
                        }
                        match tokens[0].token {
                            Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                            Token::RParen | Token::RBracket | Token::RBrace => depth -= 1,
                            _ => {}
                        }
                        init_tokens.push(tokens.remove(0));
                    }
                }
                let mut default_expr: Option<Box<Expr>> = None;
                if !init_tokens.is_empty() {
                    let mut tmp = init_tokens.clone();
                    let expr = parse_expression(&mut tmp)?;
                    default_expr = Some(Box::new(expr));
                }
                DestructuringElement::Variable(key.clone(), default_expr)
            };

            pattern.push(ObjectDestructuringElement::Property { key, value });
        }

        // allow whitespace / blank lines before separators or closing brace
        while !tokens.is_empty() && matches!(tokens[0].token, Token::LineTerminator) {
            tokens.remove(0);
        }

        if tokens.is_empty() {
            return Err(raise_parse_error_at(tokens));
        }
        if matches!(tokens[0].token, Token::RBrace) {
            tokens.remove(0); // consume }
            break;
        } else if matches!(tokens[0].token, Token::Comma) {
            tokens.remove(0); // consume ,
        } else {
            return Err(raise_parse_error_at(tokens));
        }
    }

    Ok(pattern)
}
