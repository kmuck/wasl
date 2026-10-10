//! Lexical analysis (§ 4).

use std::fmt;

use crate::diag::{Code, Result, err};
use crate::source::{FileId, Span};
use crate::types::Type;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    /// Integer literal. A value too large for a `u128` is `u128::MAX`.
    Int(u128),
    /// Float literal, as written in the source.
    Float(String),
    /// Primitive type name (`u32`, `f64`…).
    Type(Type),

    // Keywords
    World,
    Interface,
    Export,
    Global,
    Mut,
    Local,
    Func,
    If,
    Else,
    While,
    Break,
    Continue,
    Return,
    True,
    False,

    // Punctuation and operators
    LBrace,
    RBrace,
    LParen,
    RParen,
    Comma,
    Semi,
    Colon,
    ColonColon,
    Arrow,
    Eq,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    Pipe,
    Caret,
    Shl,
    Shr,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,

    Eof,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TokenKind::Ident(name) => return write!(f, "identifier `{name}`"),
            TokenKind::Int(_) => "integer literal",
            TokenKind::Float(_) => "float literal",
            TokenKind::Type(ty) => return write!(f, "`{ty}`"),
            TokenKind::Eof => "end of file",
            other => return write!(f, "`{}`", other.symbol()),
        };
        f.write_str(s)
    }
}

impl TokenKind {
    /// Text of a keyword or symbol.
    fn symbol(&self) -> &'static str {
        match self {
            TokenKind::World => "world",
            TokenKind::Interface => "interface",
            TokenKind::Export => "export",
            TokenKind::Global => "global",
            TokenKind::Mut => "mut",
            TokenKind::Local => "local",
            TokenKind::Func => "func",
            TokenKind::If => "if",
            TokenKind::Else => "else",
            TokenKind::While => "while",
            TokenKind::Break => "break",
            TokenKind::Continue => "continue",
            TokenKind::Return => "return",
            TokenKind::True => "true",
            TokenKind::False => "false",
            TokenKind::LBrace => "{",
            TokenKind::RBrace => "}",
            TokenKind::LParen => "(",
            TokenKind::RParen => ")",
            TokenKind::Comma => ",",
            TokenKind::Semi => ";",
            TokenKind::Colon => ":",
            TokenKind::ColonColon => "::",
            TokenKind::Arrow => "->",
            TokenKind::Eq => "=",
            TokenKind::Plus => "+",
            TokenKind::Minus => "-",
            TokenKind::Star => "*",
            TokenKind::Slash => "/",
            TokenKind::Percent => "%",
            TokenKind::Amp => "&",
            TokenKind::Pipe => "|",
            TokenKind::Caret => "^",
            TokenKind::Shl => "<<",
            TokenKind::Shr => ">>",
            TokenKind::EqEq => "==",
            TokenKind::Ne => "!=",
            TokenKind::Lt => "<",
            TokenKind::Le => "<=",
            TokenKind::Gt => ">",
            TokenKind::Ge => ">=",
            TokenKind::AndAnd => "&&",
            TokenKind::OrOr => "||",
            TokenKind::Bang => "!",
            TokenKind::Ident(_)
            | TokenKind::Int(_)
            | TokenKind::Float(_)
            | TokenKind::Type(_)
            | TokenKind::Eof => unreachable!(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// Words reserved for future versions and for compatibility with WIT (§ 4.4).
const RESERVED: &[&str] = &[
    "import",
    "package",
    "use",
    "type",
    "record",
    "enum",
    "variant",
    "flags",
    "resource",
    "list",
    "option",
    "result",
    "tuple",
    "string",
    "char",
    "map",
    "own",
    "borrow",
    "stream",
    "future",
    "async",
    "await",
    "static",
    "constructor",
    "include",
    "with",
    "as",
    "self",
];

fn keyword(word: &str) -> Option<TokenKind> {
    Some(match word {
        "world" => TokenKind::World,
        "interface" => TokenKind::Interface,
        "export" => TokenKind::Export,
        "global" => TokenKind::Global,
        "mut" => TokenKind::Mut,
        "local" => TokenKind::Local,
        "func" => TokenKind::Func,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "while" => TokenKind::While,
        "break" => TokenKind::Break,
        "continue" => TokenKind::Continue,
        "return" => TokenKind::Return,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        _ => return Type::from_keyword(word).map(TokenKind::Type),
    })
}

/// Whether `word` is a keyword, a type name or a reserved word, and therefore cannot be
/// used as an identifier.
pub fn is_reserved(word: &str) -> bool {
    keyword(word).is_some() || RESERVED.contains(&word)
}

/// Splits a source file into tokens. The last token is always [`TokenKind::Eof`].
pub fn tokenize(file: FileId, text: &str) -> Result<Vec<Token>> {
    Lexer {
        file,
        text,
        bytes: text.as_bytes(),
        pos: 0,
    }
    .run()
}

struct Lexer<'a> {
    file: FileId,
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl Lexer<'_> {
    fn run(mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            let start = self.pos;
            let Some(&c) = self.bytes.get(self.pos) else {
                tokens.push(Token {
                    kind: TokenKind::Eof,
                    span: self.span(start, start),
                });
                return Ok(tokens);
            };
            let kind = match c {
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.ident()?,
                b'0'..=b'9' => self.number()?,
                _ => self.punct()?,
            };
            tokens.push(Token {
                kind,
                span: self.span(start, self.pos),
            });
        }
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.file, start, end)
    }

    fn peek(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek(0) {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.pos += 1,
                Some(b'/') if self.peek(1) == Some(b'/') => {
                    while let Some(c) = self.peek(0) {
                        if c == b'\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                _ => return,
            }
        }
    }

    fn ident(&mut self) -> Result<TokenKind> {
        let start = self.pos;
        while matches!(
            self.peek(0),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
        ) {
            self.pos += 1;
        }
        let word = &self.text[start..self.pos];
        let span = self.span(start, self.pos);

        if word.bytes().any(|c| c.is_ascii_uppercase()) {
            return err(
                Code::InvalidIdent,
                span,
                format!("invalid identifier `{word}`: only lowercase letters are allowed"),
            );
        }
        if word.starts_with('_') || word.ends_with('_') || word.contains("__") {
            return err(
                Code::InvalidIdent,
                span,
                format!(
                    "invalid identifier `{word}`: `_` separates words and cannot be leading, trailing or doubled"
                ),
            );
        }
        if word.split('_').any(|w| w.as_bytes()[0].is_ascii_digit()) {
            return err(
                Code::InvalidIdent,
                span,
                format!("invalid identifier `{word}`: each word must start with a letter"),
            );
        }
        if let Some(kw) = keyword(word) {
            return Ok(kw);
        }
        if RESERVED.contains(&word) {
            return err(
                Code::ReservedKeyword,
                span,
                format!("`{word}` is a reserved word and cannot be used as an identifier"),
            );
        }
        Ok(TokenKind::Ident(word.to_string()))
    }

    fn number(&mut self) -> Result<TokenKind> {
        let start = self.pos;
        self.eat_digits();

        // Float: digits "." digits
        if self.peek(0) == Some(b'.') {
            self.pos += 1;
            if !matches!(self.peek(0), Some(b'0'..=b'9')) {
                return err(
                    Code::InvalidFloatLiteral,
                    self.span(start, self.pos),
                    "invalid float literal: a digit is required after the point",
                );
            }
            self.eat_digits();
            if self.continues_literal() {
                self.eat_literal_tail();
                return err(
                    Code::InvalidFloatLiteral,
                    self.span(start, self.pos),
                    format!(
                        "invalid float literal `{}`: only the form `digits.digits` is allowed",
                        &self.text[start..self.pos]
                    ),
                );
            }
            return Ok(TokenKind::Float(self.text[start..self.pos].to_string()));
        }

        let digits = &self.text[start..self.pos];
        if self.continues_literal() {
            self.eat_literal_tail();
            return err(
                Code::InvalidIntLiteral,
                self.span(start, self.pos),
                format!(
                    "invalid literal `{}`: only decimal digits are allowed",
                    &self.text[start..self.pos]
                ),
            );
        }
        if digits.len() > 1 && digits.starts_with('0') {
            return err(
                Code::InvalidIntLiteral,
                self.span(start, self.pos),
                format!("invalid integer literal `{digits}`: leading zeros are not allowed"),
            );
        }
        let value = digits
            .bytes()
            .try_fold(0u128, |acc, d| {
                acc.checked_mul(10)?.checked_add(u128::from(d - b'0'))
            })
            .unwrap_or(u128::MAX);
        Ok(TokenKind::Int(value))
    }

    fn eat_digits(&mut self) {
        while matches!(self.peek(0), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
    }

    /// A literal immediately followed by a letter, `_` or `.` is malformed.
    fn continues_literal(&self) -> bool {
        matches!(self.peek(0), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'.'))
    }

    fn eat_literal_tail(&mut self) {
        while matches!(
            self.peek(0),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.')
        ) {
            self.pos += 1;
        }
    }

    fn punct(&mut self) -> Result<TokenKind> {
        let start = self.pos;
        let c = self.bytes[self.pos];
        let next = self.peek(1);
        let (kind, len) = match (c, next) {
            (b'{', _) => (TokenKind::LBrace, 1),
            (b'}', _) => (TokenKind::RBrace, 1),
            (b'(', _) => (TokenKind::LParen, 1),
            (b')', _) => (TokenKind::RParen, 1),
            (b',', _) => (TokenKind::Comma, 1),
            (b';', _) => (TokenKind::Semi, 1),
            (b':', Some(b':')) => (TokenKind::ColonColon, 2),
            (b':', _) => (TokenKind::Colon, 1),
            (b'-', Some(b'>')) => (TokenKind::Arrow, 2),
            (b'-', _) => (TokenKind::Minus, 1),
            (b'=', Some(b'=')) => (TokenKind::EqEq, 2),
            (b'=', _) => (TokenKind::Eq, 1),
            (b'!', Some(b'=')) => (TokenKind::Ne, 2),
            (b'!', _) => (TokenKind::Bang, 1),
            (b'<', Some(b'<')) => (TokenKind::Shl, 2),
            (b'<', Some(b'=')) => (TokenKind::Le, 2),
            (b'<', _) => (TokenKind::Lt, 1),
            (b'>', Some(b'>')) => (TokenKind::Shr, 2),
            (b'>', Some(b'=')) => (TokenKind::Ge, 2),
            (b'>', _) => (TokenKind::Gt, 1),
            (b'&', Some(b'&')) => (TokenKind::AndAnd, 2),
            (b'&', _) => (TokenKind::Amp, 1),
            (b'|', Some(b'|')) => (TokenKind::OrOr, 2),
            (b'|', _) => (TokenKind::Pipe, 1),
            (b'+', _) => (TokenKind::Plus, 1),
            (b'*', _) => (TokenKind::Star, 1),
            (b'/', _) => (TokenKind::Slash, 1),
            (b'%', _) => (TokenKind::Percent, 1),
            (b'^', _) => (TokenKind::Caret, 1),
            (b'.', Some(b'0'..=b'9')) => {
                return err(
                    Code::InvalidFloatLiteral,
                    self.span(start, start + 1),
                    "invalid float literal: a digit is required before the point",
                );
            }
            _ => {
                let ch = self.text[start..].chars().next().unwrap();
                let shown = if ch.is_control() || ch.is_whitespace() {
                    format!("U+{:04X}", ch as u32)
                } else {
                    format!("`{ch}`")
                };
                return err(
                    Code::InvalidChar,
                    self.span(start, start + ch.len_utf8()),
                    format!("invalid character {shown}"),
                );
            }
        };
        self.pos += len;
        Ok(kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<TokenKind> {
        tokenize(FileId(0), text)
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    fn error_code(text: &str) -> Code {
        tokenize(FileId(0), text).unwrap_err().code
    }

    #[test]
    fn tokens() {
        use TokenKind::*;
        assert_eq!(
            kinds("global calls: mut u32 = 0; // comment é\nfib_rec(n - 1) -> :: <= << && != 3.14"),
            vec![
                Global,
                Ident("calls".into()),
                Colon,
                Mut,
                Type(crate::types::Type::U32),
                Eq,
                Int(0),
                Semi,
                Ident("fib_rec".into()),
                LParen,
                Ident("n".into()),
                Minus,
                Int(1),
                RParen,
                Arrow,
                ColonColon,
                Le,
                Shl,
                AndAnd,
                Ne,
                Float("3.14".into()),
                Eof,
            ]
        );
    }

    #[test]
    fn big_int_saturates() {
        assert_eq!(
            kinds("999999999999999999999999999999999999999999999")[0],
            TokenKind::Int(u128::MAX)
        );
    }

    #[test]
    fn identifiers() {
        assert_eq!(kinds("x2 max_u8")[1], TokenKind::Ident("max_u8".into()));
        assert_eq!(error_code("Fib"), Code::InvalidIdent);
        assert_eq!(error_code("_x"), Code::InvalidIdent);
        assert_eq!(error_code("x_"), Code::InvalidIdent);
        assert_eq!(error_code("a__b"), Code::InvalidIdent);
        assert_eq!(error_code("a_1"), Code::InvalidIdent);
        assert_eq!(error_code("record"), Code::ReservedKeyword);
    }

    #[test]
    fn literals() {
        assert_eq!(error_code("007"), Code::InvalidIntLiteral);
        assert_eq!(error_code("0x1F"), Code::InvalidIntLiteral);
        assert_eq!(error_code("1e10"), Code::InvalidIntLiteral);
        assert_eq!(error_code("1."), Code::InvalidFloatLiteral);
        assert_eq!(error_code(".5"), Code::InvalidFloatLiteral);
        assert_eq!(error_code("1.5e3"), Code::InvalidFloatLiteral);
        assert_eq!(
            kinds("0 0.5")[..2],
            [TokenKind::Int(0), TokenKind::Float("0.5".into())]
        );
    }

    #[test]
    fn invalid_chars() {
        assert_eq!(error_code("élan"), Code::InvalidChar);
        assert_eq!(error_code("a @ b"), Code::InvalidChar);
    }
}
