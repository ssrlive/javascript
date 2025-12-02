use crate::JSError;

#[derive(Debug, Clone)]
pub enum Token {
    Number(f64),
    StringLit(Vec<u16>),
    TemplateString(Vec<TemplatePart>),
    Identifier(String),
    Plus,
    Minus,
    Multiply,
    Divide,
    Mod,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Colon,
    Dot,
    Comma,
    Let,
    Var,
    Const,
    Class,
    Extends,
    Super,
    This,
    Static,
    New,
    InstanceOf,
    TypeOf,
    In,
    Delete,
    Void,
    Function,
    Return,
    If,
    Else,
    For,
    While,
    Do,
    Switch,
    Case,
    Default,
    Break,
    Continue,
    Try,
    Catch,
    Finally,
    Throw,
    Assign,
    Semicolon,
    Equal,
    StrictEqual,
    NotEqual,
    StrictNotEqual,
    LessThan,
    GreaterThan,
    LessEqual,
    GreaterEqual,
    True,
    False,
    Arrow,
    Spread,
    OptionalChain,
    QuestionMark,
    NullishCoalescing,
    LogicalNot,
    LogicalAnd,
    LogicalOr,
    LogicalAndAssign,
    LogicalOrAssign,
    NullishAssign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    Increment,
    Decrement,
    Async,
    Await,
}

impl Token {
    /// Get the string representation of a token that can be used as an identifier/property name
    pub fn as_identifier_string(&self) -> Option<String> {
        match self {
            Token::Identifier(s) => Some(s.clone()),
            Token::Let => Some("let".to_string()),
            Token::Var => Some("var".to_string()),
            Token::Const => Some("const".to_string()),
            Token::Class => Some("class".to_string()),
            Token::Extends => Some("extends".to_string()),
            Token::Super => Some("super".to_string()),
            Token::This => Some("this".to_string()),
            Token::Static => Some("static".to_string()),
            Token::New => Some("new".to_string()),
            Token::InstanceOf => Some("instanceof".to_string()),
            Token::TypeOf => Some("typeof".to_string()),
            Token::In => Some("in".to_string()),
            Token::Delete => Some("delete".to_string()),
            Token::Void => Some("void".to_string()),
            Token::Function => Some("function".to_string()),
            Token::Return => Some("return".to_string()),
            Token::If => Some("if".to_string()),
            Token::Else => Some("else".to_string()),
            Token::For => Some("for".to_string()),
            Token::While => Some("while".to_string()),
            Token::Do => Some("do".to_string()),
            Token::Switch => Some("switch".to_string()),
            Token::Case => Some("case".to_string()),
            Token::Default => Some("default".to_string()),
            Token::Break => Some("break".to_string()),
            Token::Continue => Some("continue".to_string()),
            Token::Try => Some("try".to_string()),
            Token::Catch => Some("catch".to_string()),
            Token::Finally => Some("finally".to_string()),
            Token::Throw => Some("throw".to_string()),
            Token::True => Some("true".to_string()),
            Token::False => Some("false".to_string()),
            Token::Async => Some("async".to_string()),
            Token::Await => Some("await".to_string()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TemplatePart {
    String(Vec<u16>),
    Expr(Vec<Token>),
}

pub fn tokenize(expr: &str) -> Result<Vec<Token>, JSError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\n' => i += 1,
            '+' => {
                if i + 1 < chars.len() && chars[i + 1] == '+' {
                    tokens.push(Token::Increment);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::AddAssign);
                    i += 2;
                } else {
                    tokens.push(Token::Plus);
                    i += 1;
                }
            }
            '-' => {
                if i + 1 < chars.len() && chars[i + 1] == '-' {
                    tokens.push(Token::Decrement);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::SubAssign);
                    i += 2;
                } else {
                    tokens.push(Token::Minus);
                    i += 1;
                }
            }
            '*' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::MulAssign);
                    i += 2;
                } else {
                    tokens.push(Token::Multiply);
                    i += 1;
                }
            }
            '/' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::DivAssign);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '/' {
                    // Single-line comment: //
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    // Don't consume the newline, let the whitespace handler deal with it
                } else if i + 1 < chars.len() && chars[i + 1] == '*' {
                    // Multi-line comment: /*
                    i += 2; // skip /*
                    while i + 1 < chars.len() {
                        if chars[i] == '*' && chars[i + 1] == '/' {
                            i += 2; // skip */
                            break;
                        }
                        i += 1;
                    }
                    if i >= chars.len() {
                        return Err(JSError::TokenizationError); // Unterminated comment
                    }
                } else {
                    tokens.push(Token::Divide);
                    i += 1;
                }
            }
            '%' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::ModAssign);
                    i += 2;
                } else {
                    tokens.push(Token::Mod);
                    i += 1;
                }
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '[' => {
                tokens.push(Token::LBracket);
                i += 1;
            }
            ']' => {
                tokens.push(Token::RBracket);
                i += 1;
            }
            '{' => {
                tokens.push(Token::LBrace);
                i += 1;
            }
            '}' => {
                tokens.push(Token::RBrace);
                i += 1;
            }
            ':' => {
                tokens.push(Token::Colon);
                i += 1;
            }
            '.' => {
                if i + 2 < chars.len() && chars[i + 1] == '.' && chars[i + 2] == '.' {
                    tokens.push(Token::Spread);
                    i += 3;
                } else {
                    tokens.push(Token::Dot);
                    i += 1;
                }
            }
            '?' => {
                // Recognize '??=' (nullish coalescing assignment), '??' (nullish coalescing), '?.' (optional chaining), and '?' (conditional)
                if i + 2 < chars.len() && chars[i + 1] == '?' && chars[i + 2] == '=' {
                    tokens.push(Token::NullishAssign);
                    i += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '?' {
                    tokens.push(Token::NullishCoalescing);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '.' {
                    tokens.push(Token::OptionalChain);
                    i += 2;
                } else {
                    tokens.push(Token::QuestionMark);
                    i += 1;
                }
            }
            '!' => {
                if i + 2 < chars.len() && chars[i + 1] == '=' && chars[i + 2] == '=' {
                    tokens.push(Token::StrictNotEqual);
                    i += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::NotEqual);
                    i += 2;
                } else {
                    tokens.push(Token::LogicalNot);
                    i += 1;
                }
            }
            '=' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    if i + 2 < chars.len() && chars[i + 2] == '=' {
                        tokens.push(Token::StrictEqual);
                        i += 3;
                    } else {
                        tokens.push(Token::Equal);
                        i += 2;
                    }
                } else if i + 1 < chars.len() && chars[i + 1] == '>' {
                    tokens.push(Token::Arrow);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '+' {
                    tokens.push(Token::AddAssign);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '-' {
                    tokens.push(Token::SubAssign);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '*' {
                    tokens.push(Token::MulAssign);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '/' {
                    tokens.push(Token::DivAssign);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '%' {
                    tokens.push(Token::ModAssign);
                    i += 2;
                } else {
                    tokens.push(Token::Assign);
                    i += 1;
                }
            }
            '<' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::LessEqual);
                    i += 2;
                } else {
                    tokens.push(Token::LessThan);
                    i += 1;
                }
            }
            '>' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token::GreaterEqual);
                    i += 2;
                } else {
                    tokens.push(Token::GreaterThan);
                    i += 1;
                }
            }
            '&' => {
                // Recognize '&&=' (logical AND assignment) and '&&' (logical AND)
                if i + 2 < chars.len() && chars[i + 1] == '&' && chars[i + 2] == '=' {
                    tokens.push(Token::LogicalAndAssign);
                    i += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '&' {
                    tokens.push(Token::LogicalAnd);
                    i += 2;
                } else {
                    return Err(JSError::TokenizationError);
                }
            }
            '|' => {
                // Recognize '||=' (logical OR assignment) and '||' (logical OR)
                if i + 2 < chars.len() && chars[i + 1] == '|' && chars[i + 2] == '=' {
                    tokens.push(Token::LogicalOrAssign);
                    i += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '|' {
                    tokens.push(Token::LogicalOr);
                    i += 2;
                } else {
                    return Err(JSError::TokenizationError);
                }
            }
            '0'..='9' => {
                let start = i;
                // integer and fractional part
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                // optional exponent part
                if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                    let mut j = i + 1;
                    // optional sign after e/E
                    if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                        j += 1;
                    }
                    // require at least one digit in exponent
                    if j >= chars.len() || !chars[j].is_ascii_digit() {
                        return Err(JSError::TokenizationError);
                    }
                    // consume exponent digits
                    while j < chars.len() && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                    i = j;
                }
                let num_str: String = chars[start..i].iter().collect();
                let num = num_str.parse::<f64>().map_err(|_| JSError::TokenizationError)?;
                tokens.push(Token::Number(num));
            }
            '"' => {
                i += 1; // skip opening quote
                let mut start = i;
                let str_lit = parse_string_literal(&chars, &mut start, '"')?;
                tokens.push(Token::StringLit(str_lit));
                i = start + 1; // skip closing quote
            }
            '\'' => {
                i += 1; // skip opening quote
                let mut start = i;
                let str_lit = parse_string_literal(&chars, &mut start, '\'')?;
                tokens.push(Token::StringLit(str_lit));
                i = start + 1; // skip closing quote
            }
            '`' => {
                i += 1; // skip opening backtick
                let mut parts = Vec::new();
                let mut current_start = i;
                while i < chars.len() && chars[i] != '`' {
                    if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1] == '{' {
                        // Found ${, add string part before it
                        if current_start < i {
                            let mut start_idx = current_start;
                            let str_part = parse_string_literal(&chars, &mut start_idx, '$')?;
                            parts.push(TemplatePart::String(str_part));
                            i = start_idx; // Update i to after the parsed string
                        }
                        i += 2; // skip ${
                        let expr_start = i;
                        let mut brace_count = 1;
                        while i < chars.len() && brace_count > 0 {
                            if chars[i] == '{' {
                                brace_count += 1;
                            } else if chars[i] == '}' {
                                brace_count -= 1;
                            }
                            i += 1;
                        }
                        if brace_count != 0 {
                            return Err(JSError::TokenizationError);
                        }
                        let expr_str: String = chars[expr_start..i - 1].iter().collect();
                        // Tokenize the expression inside ${}
                        let expr_tokens = tokenize(&expr_str)?;
                        parts.push(TemplatePart::Expr(expr_tokens));
                        current_start = i;
                    } else {
                        i += 1;
                    }
                }
                if i >= chars.len() {
                    return Err(JSError::TokenizationError);
                }
                // Add remaining string part
                if current_start < i {
                    let mut start_idx = current_start;
                    let str_part = parse_string_literal(&chars, &mut start_idx, '`')?;
                    parts.push(TemplatePart::String(str_part));
                }
                tokens.push(Token::TemplateString(parts));
                i += 1; // skip closing backtick
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                match ident.as_str() {
                    "let" => tokens.push(Token::Let),
                    "var" => tokens.push(Token::Var),
                    "const" => tokens.push(Token::Const),
                    "class" => tokens.push(Token::Class),
                    "extends" => tokens.push(Token::Extends),
                    "super" => tokens.push(Token::Super),
                    "this" => tokens.push(Token::This),
                    "static" => tokens.push(Token::Static),
                    "new" => tokens.push(Token::New),
                    "instanceof" => tokens.push(Token::InstanceOf),
                    "typeof" => tokens.push(Token::TypeOf),
                    "delete" => tokens.push(Token::Delete),
                    "void" => tokens.push(Token::Void),
                    "in" => tokens.push(Token::In),
                    "try" => tokens.push(Token::Try),
                    "catch" => tokens.push(Token::Catch),
                    "finally" => tokens.push(Token::Finally),
                    "throw" => tokens.push(Token::Throw),
                    "function" => tokens.push(Token::Function),
                    "return" => tokens.push(Token::Return),
                    "if" => tokens.push(Token::If),
                    "else" => tokens.push(Token::Else),
                    "for" => tokens.push(Token::For),
                    "while" => tokens.push(Token::While),
                    "do" => tokens.push(Token::Do),
                    "switch" => tokens.push(Token::Switch),
                    "case" => tokens.push(Token::Case),
                    "default" => tokens.push(Token::Default),
                    "break" => tokens.push(Token::Break),
                    "continue" => tokens.push(Token::Continue),
                    "true" => tokens.push(Token::True),
                    "false" => tokens.push(Token::False),
                    "async" => tokens.push(Token::Async),
                    "await" => tokens.push(Token::Await),
                    _ => tokens.push(Token::Identifier(ident)),
                }
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            ';' => {
                tokens.push(Token::Semicolon);
                i += 1;
            }
            _ => return Err(JSError::TokenizationError),
        }
    }
    Ok(tokens)
}

fn parse_string_literal(chars: &[char], start: &mut usize, end_char: char) -> Result<Vec<u16>, JSError> {
    let mut result = Vec::new();
    while *start < chars.len() && chars[*start] != end_char {
        if chars[*start] == '\\' {
            *start += 1;
            if *start >= chars.len() {
                return Err(JSError::TokenizationError);
            }
            match chars[*start] {
                'n' => result.push('\n' as u16),
                't' => result.push('\t' as u16),
                'r' => result.push('\r' as u16),
                '\\' => result.push('\\' as u16),
                '"' => result.push('"' as u16),
                '\'' => result.push('\'' as u16),
                '`' => result.push('`' as u16),
                'u' => {
                    // Unicode escape sequence \uXXXX
                    *start += 1;
                    if *start + 4 > chars.len() {
                        return Err(JSError::TokenizationError);
                    }
                    let hex_str: String = chars[*start..*start + 4].iter().collect();
                    *start += 3; // will be incremented by 1 at the end
                    match u16::from_str_radix(&hex_str, 16) {
                        Ok(code) => {
                            result.push(code);
                        }
                        Err(_) => return Err(JSError::TokenizationError), // Invalid hex
                    }
                }
                'x' => {
                    // Hex escape sequence \xHH
                    *start += 1;
                    if *start + 2 > chars.len() {
                        return Err(JSError::TokenizationError);
                    }
                    let hex_str: String = chars[*start..*start + 2].iter().collect();
                    *start += 1; // will be incremented by 1 at the end
                    match u8::from_str_radix(&hex_str, 16) {
                        Ok(code) => {
                            result.push(code as u16);
                        }
                        Err(_) => return Err(JSError::TokenizationError),
                    }
                }
                // For other escapes (regex escapes like \., \s, \], etc.) keep the backslash
                // so the regex engine receives the escape sequence. Push '\' then the char.
                other => {
                    result.push('\\' as u16);
                    result.push(other as u16);
                }
            }
        } else {
            result.push(chars[*start] as u16);
        }
        *start += 1;
    }
    if *start >= chars.len() {
        return Err(JSError::TokenizationError);
    }
    Ok(result)
}
