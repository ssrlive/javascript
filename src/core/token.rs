use crate::{JSError, raise_tokenize_error};

#[derive(Debug, Clone)]
pub enum Token {
    Number(f64),
    /// BigInt literal: integer digits followed by an 'n' suffix
    BigInt(String),
    StringLit(Vec<u16>),
    TemplateString(Vec<TemplatePart>),
    Identifier(String),
    Plus,
    Minus,
    Multiply,
    /// Exponentiation operator `**`
    Exponent,
    Divide,
    /// Regex literal with pattern and flags (e.g. /pattern/flags)
    Regex(String, String),
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
    LineTerminator,
    /// Exponentiation assignment (`**=`)
    PowAssign,
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
            ' ' | '\t' => i += 1,
            '\n' => {
                tokens.push(Token::LineTerminator);
                i += 1;
            }
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
                // Handle exponentiation '**' and '**=' first, then '*='
                if i + 2 < chars.len() && chars[i + 1] == '*' && chars[i + 2] == '=' {
                    tokens.push(Token::PowAssign);
                    i += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '*' {
                    tokens.push(Token::Exponent);
                    i += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '=' {
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
                        if chars[i] == '\n' {
                            tokens.push(Token::LineTerminator);
                        }
                        i += 1;
                    }
                    if i >= chars.len() {
                        return Err(raise_tokenize_error!()); // Unterminated comment
                    }
                } else {
                    // Heuristic: when '/' occurs in a position that cannot end an
                    // expression, it's likely the start of a regex literal (e.g.
                    // `foo(/a/)` or `if(x) /a/.test(y)`). If the previous token
                    // can end an expression (like an Identifier, Number, String,
                    // true/false, or a closing punctuation), treat this as a
                    // division operator instead.
                    let mut prev_end_expr = false;
                    if let Some(
                        Token::Number(_)
                        | Token::StringLit(_)
                        | Token::Identifier(_)
                        | Token::RBracket
                        | Token::RParen
                        | Token::RBrace
                        | Token::True
                        | Token::False
                        | Token::Increment
                        | Token::Decrement,
                    ) = tokens.iter().rev().find(|t| !matches!(t, Token::LineTerminator))
                    {
                        prev_end_expr = true;
                    }

                    if prev_end_expr {
                        tokens.push(Token::Divide);
                        i += 1;
                    } else {
                        // Parse regex literal: /.../flags
                        let mut j = i + 1;
                        let mut in_class = false;
                        while j < chars.len() {
                            if chars[j] == '\\' {
                                // escape, skip next char
                                j += 2;
                                continue;
                            }
                            if !in_class && chars[j] == '/' {
                                break;
                            }
                            if chars[j] == '[' {
                                in_class = true;
                            } else if chars[j] == ']' {
                                in_class = false;
                            }
                            j += 1;
                        }
                        if j >= chars.len() || chars[j] != '/' {
                            return Err(raise_tokenize_error!()); // unterminated regex
                        }
                        // pattern is between i+1 and j-1
                        let pattern: String = chars[i + 1..j].iter().collect();
                        j += 1; // skip closing '/'

                        // parse flags (letters only)
                        let mut flags = String::new();
                        while j < chars.len() && chars[j].is_alphabetic() {
                            flags.push(chars[j]);
                            j += 1;
                        }
                        tokens.push(Token::Regex(pattern, flags));
                        i = j;
                    }
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
                    return Err(raise_tokenize_error!());
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
                    return Err(raise_tokenize_error!());
                }
            }
            '0'..='9' => {
                let start = i;
                // integer part (allow underscores as numeric separators)
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                    i += 1;
                }

                // BigInt literal: digits (possibly with underscores) followed by 'n' (no decimal/exponent allowed)
                if i < chars.len() && chars[i] == 'n' {
                    let mut num_str: String = chars[start..i].iter().collect();
                    num_str.retain(|c| c != '_');
                    if num_str.is_empty() || !num_str.chars().all(|c| c.is_ascii_digit()) {
                        return Err(raise_tokenize_error!());
                    }
                    tokens.push(Token::BigInt(num_str));
                    i += 1; // consume trailing 'n'
                    continue;
                }

                // fractional part
                if i < chars.len() && chars[i] == '.' {
                    i += 1;
                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                        i += 1;
                    }
                }

                // optional exponent part
                if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                    let mut j = i + 1;
                    // optional sign after e/E
                    if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                        j += 1;
                    }
                    // require at least one digit in exponent (underscores allowed inside digits)
                    if j >= chars.len() || !(chars[j].is_ascii_digit()) {
                        return Err(raise_tokenize_error!());
                    }
                    while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '_') {
                        j += 1;
                    }
                    i = j;
                }

                // Build numeric string and remove numeric separators
                let mut num_str: String = chars[start..i].iter().collect();
                num_str.retain(|c| c != '_');
                // Convert to f64
                match num_str.parse::<f64>() {
                    Ok(n) => tokens.push(Token::Number(n)),
                    Err(_) => return Err(raise_tokenize_error!()),
                }
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
                            return Err(raise_tokenize_error!());
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
                    return Err(raise_tokenize_error!());
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
            'a'..='z' | 'A'..='Z' | '_' | '$' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
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
            _ => return Err(raise_tokenize_error!()),
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
                return Err(raise_tokenize_error!());
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
                    // Unicode escape sequences: either \uXXXX or \u{HEX...}
                    *start += 1;
                    if *start >= chars.len() {
                        return Err(raise_tokenize_error!());
                    }
                    if chars[*start] == '{' {
                        // \u{HEX...}
                        *start += 1; // skip '{'
                        let mut hex_str = String::new();
                        while *start < chars.len() && chars[*start] != '}' {
                            hex_str.push(chars[*start]);
                            *start += 1;
                        }
                        if *start >= chars.len() || chars[*start] != '}' {
                            return Err(raise_tokenize_error!()); // no closing brace
                        }
                        // parse hex as codepoint
                        match u32::from_str_radix(&hex_str, 16) {
                            Ok(cp) if cp <= 0x10FFFF => {
                                if cp <= 0xFFFF {
                                    result.push(cp as u16);
                                } else {
                                    // Convert to UTF-16 surrogate pair
                                    let u = cp - 0x10000;
                                    let high = 0xD800u16 + ((u >> 10) as u16);
                                    let low = 0xDC00u16 + ((u & 0x3FF) as u16);
                                    result.push(high);
                                    result.push(low);
                                }
                            }
                            _ => return Err(raise_tokenize_error!()),
                        }
                        // `start` currently at closing '}', the outer loop will increment it further
                    } else {
                        // Unicode escape sequence \uXXXX
                        if *start + 4 > chars.len() {
                            return Err(raise_tokenize_error!());
                        }
                        let hex_str: String = chars[*start..*start + 4].iter().collect();
                        *start += 3; // will be incremented by 1 at the end
                        match u16::from_str_radix(&hex_str, 16) {
                            Ok(code) => {
                                result.push(code);
                            }
                            Err(_) => return Err(raise_tokenize_error!()), // Invalid hex
                        }
                    }
                }
                'x' => {
                    // Hex escape sequence \xHH
                    *start += 1;
                    if *start + 2 > chars.len() {
                        return Err(raise_tokenize_error!());
                    }
                    let hex_str: String = chars[*start..*start + 2].iter().collect();
                    *start += 1; // will be incremented by 1 at the end
                    match u8::from_str_radix(&hex_str, 16) {
                        Ok(code) => {
                            result.push(code as u16);
                        }
                        Err(_) => return Err(raise_tokenize_error!()),
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
        return Err(raise_tokenize_error!()); // Unterminated string literal
    }
    Ok(result)
}
