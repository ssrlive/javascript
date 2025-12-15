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
    Null,
    Arrow,
    Spread,
    OptionalChain,
    QuestionMark,
    NullishCoalescing,
    LogicalNot,
    LogicalAnd,
    LogicalOr,
    BitXor,
    LogicalAndAssign,
    LogicalOrAssign,
    BitXorAssign,
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
    Yield,
    YieldStar,
    FunctionStar,
    LineTerminator,
    /// Exponentiation assignment (`**=`)
    PowAssign,
    BitAnd,
    BitNot,
    BitAndAssign,
    BitOr,
    BitOrAssign,
    LeftShift,
    LeftShiftAssign,
    RightShift,
    RightShiftAssign,
    UnsignedRightShift,
    UnsignedRightShiftAssign,
    As,
    Import,
    Export,
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
            Token::As => Some("as".to_string()),
            Token::Import => Some("import".to_string()),
            Token::Export => Some("export".to_string()),
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
            Token::Null => Some("null".to_string()),
            Token::Async => Some("async".to_string()),
            Token::Await => Some("await".to_string()),
            Token::Yield => Some("yield".to_string()),
            Token::FunctionStar => Some("function*".to_string()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TokenData {
    pub token: Token,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub enum TemplatePart {
    String(Vec<u16>),
    Expr(Vec<TokenData>),
}

pub fn tokenize(expr: &str) -> Result<Vec<TokenData>, JSError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    let mut line = 1;
    let mut column = 1;

    while i < chars.len() {
        let start_col = column;
        match chars[i] {
            ' ' | '\t' | '\r' => {
                i += 1;
                column += 1;
            }
            '\n' => {
                tokens.push(TokenData {
                    token: Token::LineTerminator,
                    line,
                    column,
                });
                i += 1;
                line += 1;
                column = 1;
            }
            '+' => {
                if i + 1 < chars.len() && chars[i + 1] == '+' {
                    tokens.push(TokenData {
                        token: Token::Increment,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(TokenData {
                        token: Token::AddAssign,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else {
                    tokens.push(TokenData {
                        token: Token::Plus,
                        line,
                        column: start_col,
                    });
                    i += 1;
                    column += 1;
                }
            }
            '-' => {
                if i + 1 < chars.len() && chars[i + 1] == '-' {
                    tokens.push(TokenData {
                        token: Token::Decrement,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(TokenData {
                        token: Token::SubAssign,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else {
                    tokens.push(TokenData {
                        token: Token::Minus,
                        line,
                        column: start_col,
                    });
                    i += 1;
                    column += 1;
                }
            }
            '*' => {
                // Handle exponentiation '**' and '**=' first, then '*='
                if i + 2 < chars.len() && chars[i + 1] == '*' && chars[i + 2] == '=' {
                    tokens.push(TokenData {
                        token: Token::PowAssign,
                        line,
                        column: start_col,
                    });
                    i += 3;
                    column += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '*' {
                    tokens.push(TokenData {
                        token: Token::Exponent,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(TokenData {
                        token: Token::MulAssign,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else {
                    tokens.push(TokenData {
                        token: Token::Multiply,
                        line,
                        column: start_col,
                    });
                    i += 1;
                    column += 1;
                }
            }
            '/' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(TokenData {
                        token: Token::DivAssign,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '/' {
                    // Single-line comment: //
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                        column += 1;
                    }
                    // Don't consume the newline, let the whitespace handler deal with it
                } else if i + 1 < chars.len() && chars[i + 1] == '*' {
                    // Multi-line comment: /*
                    i += 2; // skip /*
                    column += 2;
                    let mut terminated = false;
                    while i + 1 < chars.len() {
                        if chars[i] == '*' && chars[i + 1] == '/' {
                            i += 2; // skip */
                            column += 2;
                            terminated = true;
                            break;
                        }
                        if chars[i] == '\n' {
                            tokens.push(TokenData {
                                token: Token::LineTerminator,
                                line,
                                column,
                            });
                            line += 1;
                            column = 1;
                        } else {
                            column += 1;
                        }
                        i += 1;
                    }
                    if !terminated {
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
                    // Iterate backwards skipping LineTerminators to find the last significant token
                    let last_token = tokens.iter().rev().find(|t| !matches!(t.token, Token::LineTerminator));

                    if let Some(token_data) = last_token {
                        match token_data.token {
                            Token::Number(_)
                            | Token::BigInt(_)
                            | Token::StringLit(_)
                            | Token::Identifier(_)
                            | Token::RBracket
                            | Token::RParen
                            | Token::RBrace
                            | Token::True
                            | Token::False
                            | Token::Increment
                            | Token::Decrement => {
                                prev_end_expr = true;
                            }
                            _ => {}
                        }
                    }

                    if prev_end_expr {
                        tokens.push(TokenData {
                            token: Token::Divide,
                            line,
                            column: start_col,
                        });
                        i += 1;
                        column += 1;
                    } else {
                        // Parse regex literal: /.../flags
                        let mut j = i + 1;
                        let mut col_j = column + 1;
                        let mut in_class = false;
                        while j < chars.len() {
                            if chars[j] == '\\' {
                                // escape, skip next char
                                j += 2;
                                col_j += 2;
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
                            col_j += 1;
                        }
                        if j >= chars.len() || chars[j] != '/' {
                            return Err(raise_tokenize_error!()); // unterminated regex
                        }
                        // pattern is between i+1 and j-1
                        let pattern: String = chars[i + 1..j].iter().collect();
                        j += 1; // skip closing '/'
                        col_j += 1;

                        // parse flags (letters only)
                        let mut flags = String::new();
                        while j < chars.len() && chars[j].is_alphabetic() {
                            flags.push(chars[j]);
                            j += 1;
                            col_j += 1;
                        }
                        tokens.push(TokenData {
                            token: Token::Regex(pattern, flags),
                            line,
                            column: start_col,
                        });
                        i = j;
                        column = col_j;
                    }
                }
            }
            '%' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(TokenData {
                        token: Token::ModAssign,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else {
                    tokens.push(TokenData {
                        token: Token::Mod,
                        line,
                        column: start_col,
                    });
                    i += 1;
                    column += 1;
                }
            }
            '(' => {
                tokens.push(TokenData {
                    token: Token::LParen,
                    line,
                    column: start_col,
                });
                i += 1;
                column += 1;
            }
            ')' => {
                tokens.push(TokenData {
                    token: Token::RParen,
                    line,
                    column: start_col,
                });
                i += 1;
                column += 1;
            }
            '[' => {
                tokens.push(TokenData {
                    token: Token::LBracket,
                    line,
                    column: start_col,
                });
                i += 1;
                column += 1;
            }
            ']' => {
                tokens.push(TokenData {
                    token: Token::RBracket,
                    line,
                    column: start_col,
                });
                i += 1;
                column += 1;
            }
            '{' => {
                tokens.push(TokenData {
                    token: Token::LBrace,
                    line,
                    column: start_col,
                });
                i += 1;
                column += 1;
            }
            '}' => {
                tokens.push(TokenData {
                    token: Token::RBrace,
                    line,
                    column: start_col,
                });
                i += 1;
                column += 1;
            }
            ':' => {
                tokens.push(TokenData {
                    token: Token::Colon,
                    line,
                    column: start_col,
                });
                i += 1;
                column += 1;
            }
            '.' => {
                if i + 2 < chars.len() && chars[i + 1] == '.' && chars[i + 2] == '.' {
                    tokens.push(TokenData {
                        token: Token::Spread,
                        line,
                        column: start_col,
                    });
                    i += 3;
                    column += 3;
                } else {
                    tokens.push(TokenData {
                        token: Token::Dot,
                        line,
                        column: start_col,
                    });
                    i += 1;
                    column += 1;
                }
            }
            '?' => {
                // Recognize '??=' (nullish coalescing assignment), '??' (nullish coalescing), '?.' (optional chaining), and '?' (conditional)
                if i + 2 < chars.len() && chars[i + 1] == '?' && chars[i + 2] == '=' {
                    tokens.push(TokenData {
                        token: Token::NullishAssign,
                        line,
                        column: start_col,
                    });
                    i += 3;
                    column += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '?' {
                    tokens.push(TokenData {
                        token: Token::NullishCoalescing,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '.' {
                    tokens.push(TokenData {
                        token: Token::OptionalChain,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else {
                    tokens.push(TokenData {
                        token: Token::QuestionMark,
                        line,
                        column: start_col,
                    });
                    i += 1;
                    column += 1;
                }
            }
            '!' => {
                if i + 2 < chars.len() && chars[i + 1] == '=' && chars[i + 2] == '=' {
                    tokens.push(TokenData {
                        token: Token::StrictNotEqual,
                        line,
                        column: start_col,
                    });
                    i += 3;
                    column += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(TokenData {
                        token: Token::NotEqual,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else {
                    tokens.push(TokenData {
                        token: Token::LogicalNot,
                        line,
                        column: start_col,
                    });
                    i += 1;
                    column += 1;
                }
            }
            '=' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    if i + 2 < chars.len() && chars[i + 2] == '=' {
                        tokens.push(TokenData {
                            token: Token::StrictEqual,
                            line,
                            column: start_col,
                        });
                        i += 3;
                        column += 3;
                    } else {
                        tokens.push(TokenData {
                            token: Token::Equal,
                            line,
                            column: start_col,
                        });
                        i += 2;
                        column += 2;
                    }
                } else if i + 1 < chars.len() && chars[i + 1] == '>' {
                    tokens.push(TokenData {
                        token: Token::Arrow,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '+' {
                    tokens.push(TokenData {
                        token: Token::AddAssign,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '-' {
                    tokens.push(TokenData {
                        token: Token::SubAssign,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '*' {
                    tokens.push(TokenData {
                        token: Token::MulAssign,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '/' {
                    tokens.push(TokenData {
                        token: Token::DivAssign,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '%' {
                    tokens.push(TokenData {
                        token: Token::ModAssign,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else {
                    tokens.push(TokenData {
                        token: Token::Assign,
                        line,
                        column: start_col,
                    });
                    i += 1;
                    column += 1;
                }
            }
            '<' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(TokenData {
                        token: Token::LessEqual,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else if i + 2 < chars.len() && chars[i + 1] == '<' && chars[i + 2] == '=' {
                    // Recognize '<<=' (left shift assignment)
                    tokens.push(TokenData {
                        token: Token::LeftShiftAssign,
                        line,
                        column: start_col,
                    });
                    i += 3;
                    column += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '<' {
                    tokens.push(TokenData {
                        token: Token::LeftShift,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else {
                    tokens.push(TokenData {
                        token: Token::LessThan,
                        line,
                        column: start_col,
                    });
                    i += 1;
                    column += 1;
                }
            }
            '>' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(TokenData {
                        token: Token::GreaterEqual,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else if i + 3 < chars.len() && chars[i + 1] == '>' && chars[i + 2] == '>' && chars[i + 3] == '=' {
                    // Recognize '>>>=' (unsigned right shift assignment)
                    tokens.push(TokenData {
                        token: Token::UnsignedRightShiftAssign,
                        line,
                        column: start_col,
                    });
                    i += 4;
                    column += 4;
                } else if i + 2 < chars.len() && chars[i + 1] == '>' && chars[i + 2] == '>' {
                    // Recognize '>>>' (unsigned right shift)
                    tokens.push(TokenData {
                        token: Token::UnsignedRightShift,
                        line,
                        column: start_col,
                    });
                    i += 3;
                    column += 3;
                } else if i + 2 < chars.len() && chars[i + 1] == '>' && chars[i + 2] == '=' {
                    // Recognize '>>=' (right shift assignment)
                    tokens.push(TokenData {
                        token: Token::RightShiftAssign,
                        line,
                        column: start_col,
                    });
                    i += 3;
                    column += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '>' {
                    tokens.push(TokenData {
                        token: Token::RightShift,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else {
                    tokens.push(TokenData {
                        token: Token::GreaterThan,
                        line,
                        column: start_col,
                    });
                    i += 1;
                    column += 1;
                }
            }
            '&' => {
                // Recognize '&&=' (logical AND assignment) and '&&' (logical AND)
                if i + 2 < chars.len() && chars[i + 1] == '&' && chars[i + 2] == '=' {
                    tokens.push(TokenData {
                        token: Token::LogicalAndAssign,
                        line,
                        column: start_col,
                    });
                    i += 3;
                    column += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '&' {
                    tokens.push(TokenData {
                        token: Token::LogicalAnd,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '=' {
                    // Bitwise AND assignment '&='
                    tokens.push(TokenData {
                        token: Token::BitAndAssign,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else {
                    tokens.push(TokenData {
                        token: Token::BitAnd,
                        line,
                        column: start_col,
                    });
                    i += 1;
                    column += 1;
                }
            }
            '|' => {
                // Recognize '||=' (logical OR assignment) and '||' (logical OR)
                if i + 2 < chars.len() && chars[i + 1] == '|' && chars[i + 2] == '=' {
                    tokens.push(TokenData {
                        token: Token::LogicalOrAssign,
                        line,
                        column: start_col,
                    });
                    i += 3;
                    column += 3;
                } else if i + 1 < chars.len() && chars[i + 1] == '|' {
                    tokens.push(TokenData {
                        token: Token::LogicalOr,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else if i + 1 < chars.len() && chars[i + 1] == '=' {
                    // Bitwise OR assignment '|='
                    tokens.push(TokenData {
                        token: Token::BitOrAssign,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else {
                    tokens.push(TokenData {
                        token: Token::BitOr,
                        line,
                        column: start_col,
                    });
                    i += 1;
                    column += 1;
                }
            }
            '^' => {
                // Recognize '^=' (bitwise XOR assignment) and '^' (bitwise XOR)
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(TokenData {
                        token: Token::BitXorAssign,
                        line,
                        column: start_col,
                    });
                    i += 2;
                    column += 2;
                } else {
                    tokens.push(TokenData {
                        token: Token::BitXor,
                        line,
                        column: start_col,
                    });
                    i += 1;
                    column += 1;
                }
            }
            '~' => {
                tokens.push(TokenData {
                    token: Token::BitNot,
                    line,
                    column: start_col,
                });
                i += 1;
                column += 1;
            }
            '0'..='9' => {
                let start = i;
                // integer part (allow underscores as numeric separators)
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                    i += 1;
                    column += 1;
                }

                // BigInt literal: digits (possibly with underscores) followed by 'n' (no decimal/exponent allowed)
                if i < chars.len() && chars[i] == 'n' {
                    let mut num_str: String = chars[start..i].iter().collect();
                    num_str.retain(|c| c != '_');
                    if num_str.is_empty() || !num_str.chars().all(|c| c.is_ascii_digit()) {
                        return Err(raise_tokenize_error!());
                    }
                    tokens.push(TokenData {
                        token: Token::BigInt(num_str),
                        line,
                        column: start_col,
                    });
                    i += 1; // consume trailing 'n'
                    column += 1;
                    continue;
                }

                // fractional part
                if i < chars.len() && chars[i] == '.' {
                    i += 1;
                    column += 1;
                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                        i += 1;
                        column += 1;
                    }
                }

                // optional exponent part
                if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                    let mut j = i + 1;
                    let mut col_j = column + 1;
                    // optional sign after e/E
                    if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                        j += 1;
                        col_j += 1;
                    }
                    // require at least one digit in exponent (underscores allowed inside digits)
                    if j >= chars.len() || !(chars[j].is_ascii_digit()) {
                        return Err(raise_tokenize_error!());
                    }
                    while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '_') {
                        j += 1;
                        col_j += 1;
                    }
                    i = j;
                    column = col_j;
                }

                // Build numeric string and remove numeric separators
                let mut num_str: String = chars[start..i].iter().collect();
                num_str.retain(|c| c != '_');
                // Convert to f64
                match num_str.parse::<f64>() {
                    Ok(n) => tokens.push(TokenData {
                        token: Token::Number(n),
                        line,
                        column: start_col,
                    }),
                    Err(_) => return Err(raise_tokenize_error!()),
                }
            }
            '"' => {
                i += 1; // skip opening quote
                column += 1;
                let mut start = i;
                let str_lit = parse_string_literal(&chars, &mut start, '"')?;
                tokens.push(TokenData {
                    token: Token::StringLit(str_lit),
                    line,
                    column: start_col,
                });

                for &chars_k in chars[i..start].iter() {
                    if chars_k == '\n' {
                        line += 1;
                        column = 1;
                    } else {
                        column += 1;
                    }
                }

                i = start + 1; // skip closing quote
                column += 1;
            }
            '\'' => {
                i += 1; // skip opening quote
                column += 1;
                let mut start = i;
                let str_lit = parse_string_literal(&chars, &mut start, '\'')?;
                tokens.push(TokenData {
                    token: Token::StringLit(str_lit),
                    line,
                    column: start_col,
                });

                for &chars_k in chars[i..start].iter() {
                    if chars_k == '\n' {
                        line += 1;
                        column = 1;
                    } else {
                        column += 1;
                    }
                }

                i = start + 1; // skip closing quote
                column += 1;
            }
            '`' => {
                i += 1; // skip opening backtick
                column += 1;
                let mut parts = Vec::new();
                let mut current_start = i;
                while i < chars.len() && chars[i] != '`' {
                    if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1] == '{' {
                        // Found ${, add string part before it
                        if current_start < i {
                            let mut start_idx = current_start;
                            let str_part = parse_string_literal(&chars, &mut start_idx, '$')?;
                            parts.push(TemplatePart::String(str_part));

                            for &chars_k in chars[current_start..start_idx].iter() {
                                if chars_k == '\n' {
                                    line += 1;
                                    column = 1;
                                } else {
                                    column += 1;
                                }
                            }
                            i = start_idx; // Update i to after the parsed string
                        }
                        i += 2; // skip ${
                        column += 2;
                        let expr_start = i;
                        let mut brace_count = 1;
                        while i < chars.len() && brace_count > 0 {
                            if chars[i] == '{' {
                                brace_count += 1;
                            } else if chars[i] == '}' {
                                brace_count -= 1;
                            }
                            if chars[i] == '\n' {
                                line += 1;
                                column = 1;
                            } else {
                                column += 1;
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
                        // Handle escapes to avoid stopping at escaped backtick
                        if chars[i] == '\\' {
                            if chars[i] == '\n' {
                                line += 1;
                                column = 1;
                            } else {
                                column += 1;
                            }
                            i += 1;
                            if i < chars.len() {
                                if chars[i] == '\n' {
                                    line += 1;
                                    column = 1;
                                } else {
                                    column += 1;
                                }
                                i += 1;
                            }
                        } else {
                            if chars[i] == '\n' {
                                line += 1;
                                column = 1;
                            } else {
                                column += 1;
                            }
                            i += 1;
                        }
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
                    for &chars_k in chars[current_start..start_idx].iter() {
                        if chars_k == '\n' {
                            line += 1;
                            column = 1;
                        } else {
                            column += 1;
                        }
                    }
                }
                tokens.push(TokenData {
                    token: Token::TemplateString(parts),
                    line,
                    column: start_col,
                });
                i += 1; // skip closing backtick
                column += 1;
            }
            'a'..='z' | 'A'..='Z' | '_' | '$' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                    i += 1;
                    column += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                let token = match ident.as_str() {
                    "let" => Token::Let,
                    "var" => Token::Var,
                    "const" => Token::Const,
                    "class" => Token::Class,
                    "extends" => Token::Extends,
                    "super" => Token::Super,
                    "this" => Token::This,
                    "static" => Token::Static,
                    "new" => Token::New,
                    "instanceof" => Token::InstanceOf,
                    "typeof" => Token::TypeOf,
                    "delete" => Token::Delete,
                    "void" => Token::Void,
                    "in" => Token::In,
                    "as" => Token::As,
                    "import" => Token::Import,
                    "export" => Token::Export,
                    "try" => Token::Try,
                    "catch" => Token::Catch,
                    "finally" => Token::Finally,
                    "throw" => Token::Throw,
                    "function" => {
                        // Check if followed by '*'
                        if i < chars.len() && chars[i] == '*' {
                            i += 1; // consume '*'
                            column += 1;
                            Token::FunctionStar
                        } else {
                            Token::Function
                        }
                    }
                    "return" => Token::Return,
                    "if" => Token::If,
                    "else" => Token::Else,
                    "for" => Token::For,
                    "while" => Token::While,
                    "do" => Token::Do,
                    "switch" => Token::Switch,
                    "case" => Token::Case,
                    "default" => Token::Default,
                    "break" => Token::Break,
                    "continue" => Token::Continue,
                    "true" => Token::True,
                    "false" => Token::False,
                    "null" => Token::Null,
                    "async" => Token::Async,
                    "await" => Token::Await,
                    "yield" => {
                        // Check if followed by '*'
                        if i < chars.len() && chars[i] == '*' {
                            i += 1; // consume '*'
                            column += 1;
                            Token::YieldStar
                        } else {
                            Token::Yield
                        }
                    }
                    _ => Token::Identifier(ident),
                };
                tokens.push(TokenData {
                    token,
                    line,
                    column: start_col,
                });
            }
            ',' => {
                tokens.push(TokenData {
                    token: Token::Comma,
                    line,
                    column: start_col,
                });
                i += 1;
                column += 1;
            }
            ';' => {
                tokens.push(TokenData {
                    token: Token::Semicolon,
                    line,
                    column: start_col,
                });
                i += 1;
                column += 1;
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
            // Properly encode Unicode scalar values into UTF-16 code units
            let ch = chars[*start];
            for code_unit in ch.to_string().encode_utf16() {
                result.push(code_unit);
            }
        }
        *start += 1;
    }
    if *start >= chars.len() {
        return Err(raise_tokenize_error!()); // Unterminated string literal
    }
    Ok(result)
}
