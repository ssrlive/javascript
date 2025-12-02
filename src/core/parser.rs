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

fn parse_nullish(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    let left = parse_comparison(tokens)?;
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
    let left = parse_primary(tokens)?;
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

fn parse_primary(tokens: &mut Vec<Token>) -> Result<Expr, JSError> {
    if tokens.is_empty() {
        return Err(JSError::ParseError);
    }
    let mut expr = match tokens.remove(0) {
        Token::Number(n) => Expr::Number(n),
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
            let mut properties = Vec::new();
            if !tokens.is_empty() && matches!(tokens[0], Token::RBrace) {
                // Empty object {}
                tokens.remove(0); // consume }
                return Ok(Expr::Object(properties));
            }
            loop {
                // Check for spread
                if !tokens.is_empty() && matches!(tokens[0], Token::Spread) {
                    tokens.remove(0); // consume ...
                    let expr = parse_expression(tokens)?;
                    properties.push(("".to_string(), Expr::Spread(Box::new(expr))));
                } else {
                    // Check for getter/setter
                    let is_getter = !tokens.is_empty() && matches!(tokens[0], Token::Identifier(ref id) if id == "get");
                    let is_setter = !tokens.is_empty() && matches!(tokens[0], Token::Identifier(ref id) if id == "set");

                    if is_getter || is_setter {
                        tokens.remove(0); // consume get/set
                    }

                    // Parse key
                    let key = if let Some(Token::Identifier(name)) = tokens.first().cloned() {
                        tokens.remove(0);
                        name
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
                        let value = parse_expression(tokens)?;
                        properties.push((key, value));
                    }
                }

                // Check for comma or end
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
            let mut elements = Vec::new();
            if !tokens.is_empty() && matches!(tokens[0], Token::RBracket) {
                // Empty array []
                tokens.remove(0); // consume ]
                return Ok(Expr::Array(elements));
            }
            loop {
                // Parse element
                let elem = parse_expression(tokens)?;
                elements.push(elem);

                // Check for comma or end
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

    // Handle postfix operators like index access
    while !tokens.is_empty() {
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
    if !tokens.is_empty() && matches!(tokens[0], Token::RBracket) {
        tokens.remove(0); // consume ]
        return Ok(pattern);
    }

    loop {
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
            pattern.push(DestructuringElement::Variable(name));
        } else {
            return Err(JSError::ParseError);
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
    if !tokens.is_empty() && matches!(tokens[0], Token::RBrace) {
        tokens.remove(0); // consume }
        return Ok(pattern);
    }

    loop {
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
                    DestructuringElement::Variable(name)
                } else {
                    return Err(JSError::ParseError);
                }
            } else {
                // Shorthand: key is the same as variable name
                DestructuringElement::Variable(key.clone())
            };

            pattern.push(ObjectDestructuringElement::Property { key, value });
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
