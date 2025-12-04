use crate::{
    JSError,
    core::{BinaryOp, DestructuringElement, Expr, ObjectDestructuringElement, Statement, TemplatePart, Token, parse_statements},
};

pub fn parse_parameters(tokens: &mut Vec<Token>) -> Result<Vec<String>, JSError> {
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
    Ok(params)
}

pub fn parse_statement_block(tokens: &mut Vec<Token>) -> Result<Vec<Statement>, JSError> {
    let body = parse_statements(tokens)?;
    if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
        return Err(JSError::ParseError);
    }
    tokens.remove(0); // consume }
    Ok(body)
}

pub fn parse_expression(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    // Allow line terminators inside expressions (e.g., after a binary operator
    // at the end of a line). Tokenizer emits `LineTerminator` for newlines —
    // when parsing an expression we should treat those as insignificant
    // whitespace and skip them so expressions that span lines parse correctly.
    while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
        tokens.remove(0);
    }
    log::trace!(
        "parse_object_destructuring_pattern: tokens after initial skip (first 8): {:?}",
        tokens.iter().take(8).collect::<Vec<_>>()
    );
    parse_conditional(tokens)
}

pub fn parse_conditional(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let condition = parse_assignment(tokens)?;
    if tokens.is_empty() {
        return Ok(condition);
    }
    if matches!(tokens[0], Token::QuestionMark) {
        tokens.remove(0); // consume ?
        let true_expr = parse_conditional(tokens)?; // Allow nesting
        if tokens.is_empty() || !matches!(tokens[0], Token::Colon) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0); // consume :
        let false_expr = parse_conditional(tokens)?; // Allow nesting
        Ok(Expr::Conditional(Box::new(condition), Box::new(true_expr), Box::new(false_expr)))
    } else {
        Ok(condition)
    }
}

pub fn parse_assignment(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let left = parse_nullish(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    match &tokens[0] {
        Token::Assign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::Assign(Box::new(left), Box::new(right)))
        }
        Token::LogicalAndAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::LogicalAndAssign(Box::new(left), Box::new(right)))
        }
        Token::LogicalOrAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::LogicalOrAssign(Box::new(left), Box::new(right)))
        }
        Token::NullishAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::NullishAssign(Box::new(left), Box::new(right)))
        }
        Token::AddAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::AddAssign(Box::new(left), Box::new(right)))
        }
        Token::SubAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::SubAssign(Box::new(left), Box::new(right)))
        }
        Token::PowAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::PowAssign(Box::new(left), Box::new(right)))
        }
        Token::MulAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::MulAssign(Box::new(left), Box::new(right)))
        }
        Token::DivAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::DivAssign(Box::new(left), Box::new(right)))
        }
        Token::ModAssign => {
            tokens.remove(0);
            let right = parse_assignment(tokens)?;
            Ok(Expr::ModAssign(Box::new(left), Box::new(right)))
        }
        _ => Ok(left),
    }
}

fn parse_logical_and(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let left = parse_comparison(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    if matches!(tokens[0], Token::LogicalAnd) {
        tokens.remove(0);
        let right = parse_logical_and(tokens)?;
        Ok(Expr::LogicalAnd(Box::new(left), Box::new(right)))
    } else {
        Ok(left)
    }
}

fn parse_logical_or(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let left = parse_logical_and(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    if matches!(tokens[0], Token::LogicalOr) {
        tokens.remove(0);
        let right = parse_logical_or(tokens)?;
        Ok(Expr::LogicalOr(Box::new(left), Box::new(right)))
    } else {
        Ok(left)
    }
}

fn parse_nullish(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let left = parse_logical_or(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    if matches!(tokens[0], Token::NullishCoalescing) {
        tokens.remove(0);
        let right = parse_nullish(tokens)?;
        Ok(Expr::Binary(Box::new(left), BinaryOp::NullishCoalescing, Box::new(right)))
    } else {
        Ok(left)
    }
}

fn parse_comparison(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let left = parse_additive(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    match &tokens[0] {
        Token::Equal => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::Equal, Box::new(right)))
        }
        Token::StrictEqual => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::StrictEqual, Box::new(right)))
        }
        Token::NotEqual => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::NotEqual, Box::new(right)))
        }
        Token::StrictNotEqual => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::StrictNotEqual, Box::new(right)))
        }
        Token::LessThan => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::LessThan, Box::new(right)))
        }
        Token::GreaterThan => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::GreaterThan, Box::new(right)))
        }
        Token::LessEqual => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::LessEqual, Box::new(right)))
        }
        Token::GreaterEqual => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::GreaterEqual, Box::new(right)))
        }
        Token::InstanceOf => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::InstanceOf, Box::new(right)))
        }
        Token::In => {
            tokens.remove(0);
            let right = parse_comparison(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::In, Box::new(right)))
        }
        _ => Ok(left),
    }
}

fn parse_additive(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let left = parse_multiplicative(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    match &tokens[0] {
        Token::Plus => {
            tokens.remove(0);
            let right = parse_additive(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::Add, Box::new(right)))
        }
        Token::Minus => {
            tokens.remove(0);
            let right = parse_additive(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::Sub, Box::new(right)))
        }
        _ => Ok(left),
    }
}

fn parse_multiplicative(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let left = parse_exponentiation(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    match &tokens[0] {
        Token::Multiply => {
            tokens.remove(0);
            let right = parse_multiplicative(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::Mul, Box::new(right)))
        }
        Token::Divide => {
            tokens.remove(0);
            let right = parse_multiplicative(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::Div, Box::new(right)))
        }
        Token::Mod => {
            tokens.remove(0);
            let right = parse_multiplicative(tokens)?;
            Ok(Expr::Binary(Box::new(left), BinaryOp::Mod, Box::new(right)))
        }
        _ => Ok(left),
    }
}

fn parse_exponentiation(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    // Right-associative exponentiation operator: a ** b ** c -> a ** (b ** c)
    let left = parse_primary(tokens)?;
    if tokens.is_empty() {
        return Ok(left);
    }
    if matches!(tokens[0], Token::Exponent) {
        tokens.remove(0);
        let right = parse_exponentiation(tokens)?; // right-associative
        Ok(Expr::Binary(Box::new(left), BinaryOp::Pow, Box::new(right)))
    } else {
        Ok(left)
    }
}

fn parse_primary(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    // Skip any leading line terminators inside expressions so multi-line
    // expression continuations like `a +\n b` parse correctly.
    while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
        tokens.remove(0);
    }
    if tokens.is_empty() {
        return Err(JSError::ParseError);
    }
    let mut expr = match tokens.remove(0) {
        Token::Number(n) => Expr::Number(n),
        Token::BigInt(s) => Expr::BigInt(s.clone()),
        Token::StringLit(s) => Expr::StringLit(s),
        Token::True => Expr::Boolean(true),
        Token::False => Expr::Boolean(false),
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
        Token::LogicalNot => {
            let inner = parse_primary(tokens)?;
            Expr::LogicalNot(Box::new(inner))
        }
        Token::New => {
            // Constructor should be a simple identifier or property access, not a full expression
            let constructor = if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                tokens.remove(0);
                Expr::Var(name)
            } else {
                return Err(JSError::ParseError);
            };
            let args = if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
                tokens.remove(0); // consume '('
                let mut args = Vec::new();
                if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
                    loop {
                        let arg = parse_expression(tokens)?;
                        args.push(arg);
                        if tokens.is_empty() {
                            return Err(JSError::ParseError);
                        }
                        if matches!(tokens[0], Token::RParen) {
                            break;
                        }
                        if !matches!(tokens[0], Token::Comma) {
                            return Err(JSError::ParseError);
                        }
                        tokens.remove(0); // consume ','
                        // allow trailing comma + optional line terminators before ')'
                        while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
                            tokens.remove(0);
                        }
                        if tokens.is_empty() {
                            return Err(JSError::ParseError);
                        }
                        if matches!(tokens[0], Token::RParen) {
                            // trailing comma
                            break;
                        }
                    }
                }
                if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume ')'
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
            let mut expr = Expr::Var(name.clone());
            if !tokens.is_empty() && matches!(tokens[0], Token::Arrow) {
                tokens.remove(0);
                let body = parse_arrow_body(tokens)?;
                expr = Expr::ArrowFunction(vec![name], body);
            }
            expr
        }
        Token::Regex(pattern, flags) => Expr::Regex(pattern.clone(), flags.clone()),
        Token::This => Expr::This,
        Token::Super => {
            // Check if followed by ( for super() call
            if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
                tokens.remove(0); // consume '('
                let mut args = Vec::new();
                if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
                    loop {
                        let arg = parse_expression(tokens)?;
                        args.push(arg);
                        if tokens.is_empty() {
                            return Err(JSError::ParseError);
                        }
                        if matches!(tokens[0], Token::RParen) {
                            break;
                        }
                        if !matches!(tokens[0], Token::Comma) {
                            return Err(JSError::ParseError);
                        }
                        tokens.remove(0); // consume ','
                    }
                }
                if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume ')'
                Expr::SuperCall(args)
            } else if !tokens.is_empty() && matches!(tokens[0], Token::Dot) {
                tokens.remove(0); // consume '.'
                if tokens.is_empty() || !matches!(tokens[0], Token::Identifier(_)) {
                    return Err(JSError::ParseError);
                }
                let prop = if let Token::Identifier(name) = tokens.remove(0) {
                    name
                } else {
                    return Err(JSError::ParseError);
                };
                // Check if followed by ( for method call
                if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
                    tokens.remove(0); // consume '('
                    let mut args = Vec::new();
                    if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
                        loop {
                            let arg = parse_expression(tokens)?;
                            args.push(arg);
                            if tokens.is_empty() {
                                return Err(JSError::ParseError);
                            }
                            if matches!(tokens[0], Token::RParen) {
                                break;
                            }
                            if !matches!(tokens[0], Token::Comma) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume ','
                            // permit trailing comma before ) and skip newlines
                            while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
                                tokens.remove(0);
                            }
                            if tokens.is_empty() {
                                return Err(JSError::ParseError);
                            }
                            if matches!(tokens[0], Token::RParen) {
                                break;
                            }
                        }
                    }
                    if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                        return Err(JSError::ParseError);
                    }
                    tokens.remove(0); // consume ')'
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
            while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
                tokens.remove(0);
            }
            let mut properties = Vec::new();
            if !tokens.is_empty() && matches!(tokens[0], Token::RBrace) {
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
                while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator | Token::Semicolon) {
                    tokens.remove(0);
                }

                // If we hit the closing brace after skipping blank lines,
                // consume it and finish the object literal. This handles
                // trailing commas followed by whitespace/newlines before `}`.
                if !tokens.is_empty() && matches!(tokens[0], Token::RBrace) {
                    tokens.remove(0); // consume }
                    break;
                }
                if tokens.is_empty() {
                    return Err(JSError::ParseError);
                }
                // Check for spread
                if !tokens.is_empty() && matches!(tokens[0], Token::Spread) {
                    log::trace!(
                        "parse_primary: object property is spread; next tokens (first 8): {:?}",
                        tokens.iter().take(8).collect::<Vec<_>>()
                    );
                    tokens.remove(0); // consume ...
                    let expr = parse_expression(tokens)?;
                    properties.push(("".to_string(), Expr::Spread(Box::new(expr))));
                } else {
                    // Check for getter/setter: only treat as getter/setter if the
                    // identifier 'get'/'set' is followed by a property key and
                    // an opening parenthesis (no colon). This avoids confusing a
                    // regular property named 'get'/'set' (e.g. `set: function(...)`) with
                    // the getter/setter syntax.
                    // Recognize getter/setter signatures including computed keys
                    let is_getter = if tokens.len() >= 2 && matches!(tokens[0], Token::Identifier(ref id) if id == "get") {
                        if matches!(tokens[1], Token::Identifier(_) | Token::StringLit(_)) {
                            tokens.len() >= 3 && matches!(tokens[2], Token::LParen)
                        } else if matches!(tokens[1], Token::LBracket) {
                            // find matching RBracket and ensure '(' follows
                            let mut depth = 0i32;
                            let mut idx_after = None;
                            for (i, t) in tokens.iter().enumerate().skip(1) {
                                match t {
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
                                next_i < tokens.len() && matches!(tokens[next_i], Token::LParen)
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    let is_setter = if tokens.len() >= 2 && matches!(tokens[0], Token::Identifier(ref id) if id == "set") {
                        if matches!(tokens[1], Token::Identifier(_) | Token::StringLit(_)) {
                            tokens.len() >= 3 && matches!(tokens[2], Token::LParen)
                        } else if matches!(tokens[1], Token::LBracket) {
                            let mut depth = 0i32;
                            let mut idx_after = None;
                            for (i, t) in tokens.iter().enumerate().skip(1) {
                                match t {
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
                                next_i < tokens.len() && matches!(tokens[next_i], Token::LParen)
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    // Support method shorthand e.g. `foo() { ... }` or computed
                    // methods like `[Symbol.toPrimitive]() { ... }` in object
                    // literals. Handle these before parsing key:value pairs.
                    if !tokens.is_empty() {
                        // Computed method: starts with [ ... ] followed by (
                        if matches!(tokens[0], Token::LBracket) {
                            // Capture the computed key tokens (until matching RBracket)
                            let mut depth: i32 = 0;
                            let mut inner: Vec<Token> = Vec::new();
                            // consume '['
                            tokens.remove(0);
                            depth += 1;
                            while !tokens.is_empty() {
                                match tokens[0] {
                                    Token::LBracket => depth += 1,
                                    Token::RBracket => {
                                        depth -= 1;
                                        if depth == 0 {
                                            tokens.remove(0);
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                                inner.push(tokens.remove(0));
                            }
                            // If next is '(', this is a method definition
                            if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
                                // Create a simple printed key for storage (e.g. "[Symbol.toPrimitive]")
                                let mut key_str = String::new();
                                key_str.push('[');
                                for t in &inner {
                                    match t {
                                        Token::Identifier(n) => {
                                            key_str.push_str(n);
                                        }
                                        Token::Dot => {
                                            key_str.push('.');
                                        }
                                        Token::StringLit(s) => key_str.push_str(&String::from_utf16_lossy(s)),
                                        _ => {}
                                    }
                                }
                                key_str.push(']');

                                // parse parameters
                                tokens.remove(0); // consume '('
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
                                            tokens.remove(0);
                                        } else {
                                            return Err(JSError::ParseError);
                                        }
                                    }
                                }
                                if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                                    return Err(JSError::ParseError);
                                }
                                tokens.remove(0); // consume ')'
                                if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                                    return Err(JSError::ParseError);
                                }
                                tokens.remove(0); // consume '{'
                                let body = parse_statements(tokens)?;
                                if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                                    return Err(JSError::ParseError);
                                }
                                tokens.remove(0); // consume '}'
                                properties.push((key_str, Expr::Function(params, body)));
                                // After adding method, skip any newline/semicolons and handle comma/end in outer loop
                                while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator | Token::Semicolon) {
                                    tokens.remove(0);
                                }
                                if tokens.is_empty() {
                                    return Err(JSError::ParseError);
                                }
                                if matches!(tokens[0], Token::RBrace) {
                                    tokens.remove(0);
                                    break;
                                }
                                if matches!(tokens[0], Token::Comma) {
                                    tokens.remove(0);
                                    continue;
                                }
                            } else {
                                return Err(JSError::ParseError);
                            }
                        }

                        // Identifier followed by '(' indicates a concise method: name(...) { ... }
                        if tokens.len() >= 2
                            && matches!(tokens[0], Token::Identifier(_))
                            && matches!(tokens[1], Token::LParen)
                            && let Token::Identifier(name) = tokens.remove(0)
                        {
                            // tokens[0] is '('
                            tokens.remove(0);
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
                                        tokens.remove(0);
                                    } else {
                                        return Err(JSError::ParseError);
                                    }
                                }
                            }
                            if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume ')'
                            if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume '{'
                            let body = parse_statements(tokens)?;
                            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume '}'
                            properties.push((name, Expr::Function(params, body)));
                            while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator | Token::Semicolon) {
                                tokens.remove(0);
                            }
                            if tokens.is_empty() {
                                return Err(JSError::ParseError);
                            }
                            if matches!(tokens[0], Token::RBrace) {
                                tokens.remove(0);
                                break;
                            }
                            if matches!(tokens[0], Token::Comma) {
                                tokens.remove(0);
                                continue;
                            }
                        }
                    }

                    if is_getter || is_setter {
                        log::trace!(
                            "parse_primary: object property is getter/setter; next tokens (first 8): {:?}",
                            tokens.iter().take(8).collect::<Vec<_>>()
                        );
                        tokens.remove(0); // consume get/set
                    }

                    // Parse key
                    let key = if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                        tokens.remove(0);
                        name
                    } else if !tokens.is_empty() && matches!(tokens[0], Token::LBracket) {
                        // Computed key (e.g., get [Symbol.toPrimitive]())
                        // Capture the inner tokens up to the matching ']'
                        let mut depth: i32 = 0;
                        let mut inner: Vec<Token> = Vec::new();
                        // consume '['
                        tokens.remove(0);
                        depth += 1;
                        while !tokens.is_empty() {
                            match tokens[0] {
                                Token::LBracket => depth += 1,
                                Token::RBracket => {
                                    depth -= 1;
                                    if depth == 0 {
                                        tokens.remove(0);
                                        break;
                                    }
                                }
                                _ => {}
                            }
                            inner.push(tokens.remove(0));
                        }
                        // Build a printable representation for the computed key
                        let mut key_str = String::new();
                        key_str.push('[');
                        for t in &inner {
                            match t {
                                Token::Identifier(n) => {
                                    key_str.push_str(n);
                                }
                                Token::Dot => {
                                    key_str.push('.');
                                }
                                Token::StringLit(s) => key_str.push_str(&String::from_utf16_lossy(s)),
                                _ => {}
                            }
                        }
                        key_str.push(']');
                        key_str
                    } else if let Some(Token::Default) = tokens.first().cloned() {
                        // allow the reserved word `default` as an object property key
                        tokens.remove(0);
                        "default".to_string()
                    } else if let Some(Token::StringLit(s)) = tokens.first().cloned() {
                        tokens.remove(0);
                        String::from_utf16_lossy(&s)
                    } else {
                        return Err(JSError::ParseError);
                    };

                    // Expect colon or parentheses for getter/setter
                    if is_getter || is_setter {
                        // Parse function for getter/setter
                        if tokens.is_empty() || !matches!(tokens[0], Token::LParen) {
                            return Err(JSError::ParseError);
                        }
                        tokens.remove(0); // consume (

                        let mut params = Vec::new();
                        if is_setter {
                            // Setter should have exactly one parameter
                            if let Some(Token::Identifier(param)) = tokens.first().cloned() {
                                tokens.remove(0);
                                params.push(param);
                            } else {
                                return Err(JSError::ParseError);
                            }
                        } else if is_getter {
                            // Getter should have no parameters
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

                        if is_getter {
                            properties.push((key, Expr::Getter(Box::new(Expr::Function(params, body)))));
                        } else {
                            properties.push((key, Expr::Setter(Box::new(Expr::Function(params, body)))));
                        }
                    } else {
                        // Regular property
                        if tokens.is_empty() || !matches!(tokens[0], Token::Colon) {
                            return Err(JSError::ParseError);
                        }
                        tokens.remove(0); // consume :

                        // Parse value
                        log::trace!(
                            "parse_primary: parsing value for key '{}' ; next tokens (first 8): {:?}",
                            key,
                            tokens.iter().take(8).collect::<Vec<_>>()
                        );
                        let value = parse_expression(tokens)?;
                        properties.push((key, value));
                    }
                }

                // Check for comma or end. Allow intervening line terminators or
                // stray semicolons between properties and the closing `}`.
                while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator | Token::Semicolon) {
                    tokens.remove(0);
                }

                if tokens.is_empty() {
                    return Err(JSError::ParseError);
                }
                if matches!(tokens[0], Token::RBrace) {
                    tokens.remove(0); // consume }
                    break;
                } else if matches!(tokens[0], Token::Comma) {
                    tokens.remove(0); // consume ,
                } else {
                    return Err(JSError::ParseError);
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
            if !tokens.is_empty() && matches!(tokens[0], Token::RBracket) {
                // Empty array []
                tokens.remove(0); // consume ]
                return Ok(Expr::Array(elements));
            }
            loop {
                // Skip leading blank lines inside array literals to avoid
                // attempting to parse a `]` or other tokens as elements.
                while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator | Token::Semicolon) {
                    tokens.remove(0);
                }
                // If next token is a closing bracket then the array is complete
                // This handles trailing commas like `[1, 2,]` correctly — we should
                // stop and not attempt to parse a non-existent element.
                if !tokens.is_empty() && matches!(tokens[0], Token::RBracket) {
                    tokens.remove(0); // consume ]
                    log::trace!(
                        "parse_primary: completed array literal with {} elements; remaining tokens (first 12): {:?}",
                        elements.len(),
                        tokens.iter().take(12).collect::<Vec<_>>()
                    );
                    break;
                }

                log::trace!("parse_primary: array element next token: {:?}", tokens.first());
                // Parse element
                let elem = parse_expression(tokens)?;
                elements.push(elem);

                // Check for comma or end. Allow intervening line terminators
                // between elements so array items can be split across lines.
                while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator | Token::Semicolon) {
                    tokens.remove(0);
                }

                if tokens.is_empty() {
                    return Err(JSError::ParseError);
                }
                if matches!(tokens[0], Token::RBracket) {
                    tokens.remove(0); // consume ]
                    break;
                } else if matches!(tokens[0], Token::Comma) {
                    tokens.remove(0); // consume ,
                } else {
                    return Err(JSError::ParseError);
                }
            }
            Expr::Array(elements)
        }
        Token::Function => {
            // Parse function expression
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
                Expr::Function(params, body)
            } else {
                return Err(JSError::ParseError);
            }
        }
        Token::Async => {
            // Check if followed by function or arrow function parameters
            if !tokens.is_empty() && matches!(tokens[0], Token::Function) {
                tokens.remove(0); // consume function
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
                    Expr::AsyncFunction(params, body)
                } else {
                    return Err(JSError::ParseError);
                }
            } else if !tokens.is_empty() && matches!(tokens[0], Token::LParen) {
                // Async arrow function
                tokens.remove(0); // consume (
                let mut params = Vec::new();
                let mut is_arrow = false;
                if matches!(tokens.first(), Some(&Token::RParen)) {
                    tokens.remove(0);
                    if !tokens.is_empty() && matches!(tokens[0], Token::Arrow) {
                        tokens.remove(0);
                        is_arrow = true;
                    } else {
                        return Err(JSError::ParseError);
                    }
                } else {
                    // Try to parse params
                    let mut param_names = Vec::new();
                    let mut local_consumed = Vec::new();
                    let mut valid = true;
                    loop {
                        if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                            tokens.remove(0);
                            local_consumed.push(Token::Identifier(name.clone()));
                            param_names.push(name);
                            if tokens.is_empty() {
                                valid = false;
                                break;
                            }
                            if matches!(tokens[0], Token::RParen) {
                                tokens.remove(0);
                                local_consumed.push(Token::RParen);
                                if !tokens.is_empty() && matches!(tokens[0], Token::Arrow) {
                                    tokens.remove(0);
                                    is_arrow = true;
                                } else {
                                    valid = false;
                                }
                                break;
                            } else if matches!(tokens[0], Token::Comma) {
                                tokens.remove(0);
                                local_consumed.push(Token::Comma);
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
                        return Err(JSError::ParseError);
                    }
                    params = param_names;
                }
                if is_arrow {
                    // For async arrow functions, we need to create a special async closure
                    // For now, we'll treat them as regular arrow functions but mark them as async
                    // This will need to be handled in evaluation
                    Expr::ArrowFunction(params, parse_arrow_body(tokens)?)
                } else {
                    return Err(JSError::ParseError);
                }
            } else {
                return Err(JSError::ParseError);
            }
        }
        Token::LParen => {
            // Check if it's arrow function
            let mut params = Vec::new();
            let mut is_arrow = false;
            let mut result_expr = None;
            if matches!(tokens.first(), Some(&Token::RParen)) {
                tokens.remove(0);
                if !tokens.is_empty() && matches!(tokens[0], Token::Arrow) {
                    tokens.remove(0);
                    is_arrow = true;
                } else {
                    return Err(JSError::ParseError);
                }
            } else {
                // Try to parse params
                let mut param_names = Vec::new();
                let mut local_consumed = Vec::new();
                let mut valid = true;
                loop {
                    if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                        tokens.remove(0);
                        local_consumed.push(Token::Identifier(name.clone()));
                        param_names.push(name);
                        if tokens.is_empty() {
                            valid = false;
                            break;
                        }
                        if matches!(tokens[0], Token::RParen) {
                            tokens.remove(0);
                            local_consumed.push(Token::RParen);
                            if !tokens.is_empty() && matches!(tokens[0], Token::Arrow) {
                                tokens.remove(0);
                                is_arrow = true;
                            } else {
                                valid = false;
                            }
                            break;
                        } else if matches!(tokens[0], Token::Comma) {
                            tokens.remove(0);
                            local_consumed.push(Token::Comma);
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
                    if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                        return Err(JSError::ParseError);
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
                log::debug!("parse_expression unexpected token: {:?}; remaining tokens: {:?}", tokens[0], tokens);
            } else {
                log::debug!("parse_expression unexpected end of tokens; tokens empty");
            }
            return Err(JSError::ParseError);
        }
    };

    // Handle postfix operators like index access. Accept line terminators
    // between the primary and the postfix operator to support call-chains
    // split across lines (e.g. `promise.then(...)
    // .then(...)`).
    while !tokens.is_empty() {
        while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
            tokens.remove(0);
        }
        if tokens.is_empty() {
            break;
        }
        match &tokens[0] {
            Token::LBracket => {
                tokens.remove(0); // consume '['
                let index_expr = parse_expression(tokens)?;
                if tokens.is_empty() || !matches!(tokens[0], Token::RBracket) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume ']'
                expr = Expr::Index(Box::new(expr), Box::new(index_expr));
            }
            Token::Dot => {
                tokens.remove(0); // consume '.'
                if tokens.is_empty() {
                    return Err(JSError::ParseError);
                }
                if let Some(prop) = tokens[0].as_identifier_string() {
                    tokens.remove(0);
                    expr = Expr::Property(Box::new(expr), prop);
                } else {
                    return Err(JSError::ParseError);
                }
            }
            Token::OptionalChain => {
                tokens.remove(0); // consume '?.'
                if tokens.is_empty() {
                    return Err(JSError::ParseError);
                }
                if matches!(tokens[0], Token::LParen) {
                    // Optional call: obj?.method(args)
                    tokens.remove(0); // consume '('
                    let mut args = Vec::new();
                    if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
                        loop {
                            let arg = parse_expression(tokens)?;
                            args.push(arg);
                            if tokens.is_empty() {
                                return Err(JSError::ParseError);
                            }
                            if matches!(tokens[0], Token::RParen) {
                                break;
                            }
                            if !matches!(tokens[0], Token::Comma) {
                                return Err(JSError::ParseError);
                            }
                            tokens.remove(0); // consume ','
                        }
                    }
                    if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                        return Err(JSError::ParseError);
                    }
                    tokens.remove(0); // consume ')'
                    expr = Expr::OptionalCall(Box::new(expr), args);
                } else if matches!(tokens[0], Token::Identifier(_)) {
                    // Optional property access: obj?.prop
                    if let Some(prop) = tokens[0].as_identifier_string() {
                        tokens.remove(0);
                        expr = Expr::OptionalProperty(Box::new(expr), prop);
                    } else {
                        return Err(JSError::ParseError);
                    }
                } else {
                    return Err(JSError::ParseError);
                }
            }
            Token::LParen => {
                tokens.remove(0); // consume '('
                let mut args = Vec::new();
                if !tokens.is_empty() && !matches!(tokens[0], Token::RParen) {
                    loop {
                        let arg = parse_expression(tokens)?;
                        args.push(arg);
                        if tokens.is_empty() {
                            return Err(JSError::ParseError);
                        }
                        if matches!(tokens[0], Token::RParen) {
                            break;
                        }
                        if !matches!(tokens[0], Token::Comma) {
                            return Err(JSError::ParseError);
                        }
                        tokens.remove(0); // consume ','
                        // allow trailing comma before ')' and skip newlines
                        while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
                            tokens.remove(0);
                        }
                        if tokens.is_empty() {
                            return Err(JSError::ParseError);
                        }
                        if matches!(tokens[0], Token::RParen) {
                            break;
                        }
                    }
                }
                if tokens.is_empty() || !matches!(tokens[0], Token::RParen) {
                    return Err(JSError::ParseError);
                }
                tokens.remove(0); // consume ')'
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

fn parse_arrow_body(tokens: &mut Vec<Token>) -> Result<Vec<Statement>, JSError> {
    if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
        tokens.remove(0);
        let body = parse_statements(tokens)?;
        if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
            return Err(JSError::ParseError);
        }
        tokens.remove(0);
        Ok(body)
    } else {
        let expr = parse_expression(tokens)?;
        Ok(vec![Statement::Return(Some(expr))])
    }
}

pub fn parse_array_destructuring_pattern(tokens: &mut Vec<Token>) -> Result<Vec<DestructuringElement>, JSError> {
    if tokens.is_empty() || !matches!(tokens[0], Token::LBracket) {
        return Err(JSError::ParseError);
    }
    tokens.remove(0); // consume [

    let mut pattern = Vec::new();
    // Skip initial blank lines inside the pattern
    while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
        tokens.remove(0);
    }
    if !tokens.is_empty() && matches!(tokens[0], Token::RBracket) {
        tokens.remove(0); // consume ]
        return Ok(pattern);
    }

    loop {
        // skip any blank lines at the start of a new property entry
        while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
            tokens.remove(0);
        }
        if !tokens.is_empty() && matches!(tokens[0], Token::Spread) {
            tokens.remove(0); // consume ...
            if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                tokens.remove(0);
                pattern.push(DestructuringElement::Rest(name));
            } else {
                return Err(JSError::ParseError);
            }
            // Rest must be the last element
            if tokens.is_empty() || !matches!(tokens[0], Token::RBracket) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume ]
            break;
        } else if !tokens.is_empty() && matches!(tokens[0], Token::Comma) {
            tokens.remove(0); // consume ,
            pattern.push(DestructuringElement::Empty);
        } else if !tokens.is_empty() && matches!(tokens[0], Token::LBracket) {
            // Nested array destructuring
            let nested_pattern = parse_array_destructuring_pattern(tokens)?;
            pattern.push(DestructuringElement::NestedArray(nested_pattern));
        } else if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
            // Nested object destructuring
            let nested_pattern = parse_object_destructuring_pattern(tokens)?;
            pattern.push(DestructuringElement::NestedObject(nested_pattern));
        } else if let Some(Token::Identifier(name)) = tokens.first().cloned() {
            tokens.remove(0);
            // Accept optional default initializer in patterns: e.g. `a = 1`
            let mut default_expr: Option<Box<Expr>> = None;
            if !tokens.is_empty() && matches!(tokens[0], Token::Assign) {
                tokens.remove(0); // consume '='
                // capture initializer tokens until top-level comma or ] and parse them
                let mut depth: i32 = 0;
                let mut init_tokens: Vec<Token> = Vec::new();
                while !tokens.is_empty() {
                    if depth == 0 && (matches!(tokens[0], Token::Comma) || matches!(tokens[0], Token::RBracket)) {
                        break;
                    }
                    match tokens[0] {
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
            return Err(JSError::ParseError);
        }

        // allow blank lines between last element and closing brace
        while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
            tokens.remove(0);
        }

        if tokens.is_empty() {
            return Err(JSError::ParseError);
        }
        if matches!(tokens[0], Token::RBracket) {
            tokens.remove(0); // consume ]
            break;
        } else if matches!(tokens[0], Token::Comma) {
            tokens.remove(0); // consume ,
        } else {
            return Err(JSError::ParseError);
        }
    }

    Ok(pattern)
}

pub fn parse_object_destructuring_pattern(tokens: &mut Vec<Token>) -> Result<Vec<ObjectDestructuringElement>, JSError> {
    if tokens.is_empty() || !matches!(tokens[0], Token::LBrace) {
        return Err(JSError::ParseError);
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
    while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
        tokens.remove(0);
    }

    if !tokens.is_empty() && matches!(tokens[0], Token::RBrace) {
        tokens.remove(0); // consume }
        return Ok(pattern);
    }

    loop {
        // allow and skip blank lines between elements
        while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
            tokens.remove(0);
        }
        // If after skipping blanks we immediately hit a closing brace, accept
        // it. This handles the common formatting where there is a trailing
        // comma and then a newline before the closing `}` (e.g.
        // `a = 0,\n}`) which should be treated as the end of the object
        // pattern instead of expecting another property.
        if !tokens.is_empty() && matches!(tokens[0], Token::RBrace) {
            tokens.remove(0); // consume }
            break;
        }
        if !tokens.is_empty() && matches!(tokens[0], Token::Spread) {
            tokens.remove(0); // consume ...
            if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                tokens.remove(0);
                pattern.push(ObjectDestructuringElement::Rest(name));
            } else {
                return Err(JSError::ParseError);
            }
            // Rest must be the last element
            if tokens.is_empty() || !matches!(tokens[0], Token::RBrace) {
                return Err(JSError::ParseError);
            }
            tokens.remove(0); // consume }
            break;
        } else {
            // Parse property
            let key = if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                tokens.remove(0);
                name
            } else {
                log::trace!(
                    "parse_object_destructuring_pattern: expected Identifier for property key but got {:?}",
                    tokens.first()
                );
                return Err(JSError::ParseError);
            };

            let value = if !tokens.is_empty() && matches!(tokens[0], Token::Colon) {
                tokens.remove(0); // consume :
                // Parse the value pattern
                if !tokens.is_empty() && matches!(tokens[0], Token::LBracket) {
                    DestructuringElement::NestedArray(parse_array_destructuring_pattern(tokens)?)
                } else if !tokens.is_empty() && matches!(tokens[0], Token::LBrace) {
                    DestructuringElement::NestedObject(parse_object_destructuring_pattern(tokens)?)
                } else if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                    tokens.remove(0);
                    // Allow default initializer for property value like `a: b = 1`
                    let mut default_expr: Option<Box<Expr>> = None;
                    if !tokens.is_empty() && matches!(tokens[0], Token::Assign) {
                        tokens.remove(0);
                        let mut depth: i32 = 0;
                        let mut init_tokens: Vec<Token> = Vec::new();
                        while !tokens.is_empty() {
                            if depth == 0 && (matches!(tokens[0], Token::Comma) || matches!(tokens[0], Token::RBrace)) {
                                break;
                            }
                            match tokens[0] {
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
                    return Err(JSError::ParseError);
                }
            } else {
                // Shorthand: key is the same as variable name. Allow optional
                // default initializer after the shorthand, e.g. `{a = 1}`.
                let mut init_tokens: Vec<Token> = Vec::new();
                if !tokens.is_empty() && matches!(tokens[0], Token::Assign) {
                    tokens.remove(0); // consume '='
                    let mut depth: i32 = 0;
                    while !tokens.is_empty() {
                        if depth == 0 && (matches!(tokens[0], Token::Comma) || matches!(tokens[0], Token::RBrace)) {
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
        while !tokens.is_empty() && matches!(tokens[0], Token::LineTerminator) {
            tokens.remove(0);
        }

        if tokens.is_empty() {
            return Err(JSError::ParseError);
        }
        if matches!(tokens[0], Token::RBrace) {
            tokens.remove(0); // consume }
            break;
        } else if matches!(tokens[0], Token::Comma) {
            tokens.remove(0); // consume ,
        } else {
            return Err(JSError::ParseError);
        }
    }

    Ok(pattern)
}
