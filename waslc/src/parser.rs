//! Recursive-descent parser (§ 5).

use crate::ast::*;
use crate::diag::{Code, Diagnostic, Result, err};
use crate::lexer::{Token, TokenKind};
use crate::source::Span;
use crate::types::Type;

/// Builds the syntax tree of a file from its tokens.
pub fn parse(tokens: Vec<Token>) -> Result<Vec<Item>> {
    let mut parser = Parser { tokens, pos: 0 };
    let mut items = Vec::new();
    while *parser.peek() != TokenKind::Eof {
        items.push(parser.item()?);
    }
    Ok(items)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    // ---- Helpers ----

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_at(&self, offset: usize) -> &TokenKind {
        let i = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[i].kind
    }

    fn span(&self) -> Span {
        self.tokens[self.pos].span
    }

    /// Span of the previous token.
    fn prev_span(&self) -> Span {
        self.tokens[self.pos.saturating_sub(1)].span
    }

    fn bump(&mut self) -> Token {
        let token = self.tokens[self.pos].clone();
        if token.kind != TokenKind::Eof {
            self.pos += 1;
        }
        token
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.peek() == kind {
            self.bump();
            true
        } else {
            false
        }
    }

    fn unexpected(&self, expected: &str) -> Diagnostic {
        Diagnostic::new(
            Code::Syntax,
            self.span(),
            format!("expected {expected}, found {}", self.peek()),
        )
    }

    fn expect(&mut self, kind: &TokenKind, expected: &str) -> Result<Token> {
        if self.peek() == kind {
            Ok(self.bump())
        } else {
            Err(self.unexpected(expected))
        }
    }

    fn ident(&mut self, expected: &str) -> Result<Ident> {
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                let span = self.bump().span;
                Ok(Ident { name, span })
            }
            _ => Err(self.unexpected(expected)),
        }
    }

    fn ty(&mut self) -> Result<Type> {
        match *self.peek() {
            TokenKind::Type(ty) => {
                self.bump();
                Ok(ty)
            }
            _ => Err(self.unexpected("a type")),
        }
    }

    // ---- Declarations ----

    fn item(&mut self) -> Result<Item> {
        match self.peek() {
            TokenKind::World => self.world().map(Item::World),
            TokenKind::Interface => self.interface().map(Item::Interface),
            TokenKind::Global => self.global().map(Item::Global),
            _ => Err(self.unexpected("`world`, `interface` or `global`")),
        }
    }

    fn world(&mut self) -> Result<World> {
        self.bump();
        let name = self.ident("a world name")?;
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut exports = Vec::new();
        while !self.eat(&TokenKind::RBrace) {
            if *self.peek() != TokenKind::Export {
                return Err(self.unexpected("`export` or `}`"));
            }
            self.bump();
            exports.push(self.ident("an interface name")?);
            self.expect(&TokenKind::Semi, "`;`")?;
        }
        Ok(World { name, exports })
    }

    fn interface(&mut self) -> Result<Interface> {
        self.bump();
        let name = self.ident("an interface name")?;
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut funcs = Vec::new();
        while !self.eat(&TokenKind::RBrace) {
            match self.peek() {
                TokenKind::Func => funcs.push(self.func()?),
                TokenKind::Global => {
                    return err(
                        Code::Syntax,
                        self.span(),
                        "globals are declared at the top level, not inside an interface",
                    );
                }
                _ => return Err(self.unexpected("`func` or `}`")),
            }
        }
        Ok(Interface { name, funcs })
    }

    fn global(&mut self) -> Result<Global> {
        self.bump();
        let name = self.ident("a global name")?;
        self.expect(&TokenKind::Colon, "`:` (the type of a global is required)")?;
        let mutable = self.eat(&TokenKind::Mut);
        let ty = self.ty()?;
        self.expect(&TokenKind::Eq, "`=` (a global must be initialized)")?;
        let init = self.global_init()?;
        self.expect(&TokenKind::Semi, "`;`")?;
        Ok(Global {
            name,
            mutable,
            ty,
            init,
        })
    }

    /// `[ "-" ] int_literal | [ "-" ] float_literal | "true" | "false"`
    fn global_init(&mut self) -> Result<Expr> {
        let start = self.span();
        let neg = self.eat(&TokenKind::Minus);
        let kind = match self.peek().clone() {
            TokenKind::Int(v) => ExprKind::Int(v),
            TokenKind::Float(s) => ExprKind::Float(s),
            TokenKind::True if !neg => ExprKind::Bool(true),
            TokenKind::False if !neg => ExprKind::Bool(false),
            _ => {
                return err(
                    Code::Syntax,
                    self.span(),
                    "the initial value of a global must be a literal",
                );
            }
        };
        let lit_span = self.bump().span;
        let mut expr = Expr {
            kind,
            span: lit_span,
        };
        if neg {
            expr = Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                },
                span: start.to(lit_span),
            };
        }
        if !matches!(self.peek(), TokenKind::Semi) {
            return err(
                Code::Syntax,
                self.span(),
                "the initial value of a global must be a literal",
            );
        }
        Ok(expr)
    }

    fn func(&mut self) -> Result<Func> {
        self.bump();
        let name = self.ident("a function name")?;
        self.expect(&TokenKind::LParen, "`(`")?;
        let mut params = Vec::new();
        if !self.eat(&TokenKind::RParen) {
            loop {
                let name = self.ident("a parameter name")?;
                self.expect(
                    &TokenKind::Colon,
                    "`:` (the type of a parameter is required)",
                )?;
                let ty = self.ty()?;
                params.push(Param { name, ty });
                if self.eat(&TokenKind::RParen) {
                    break;
                }
                self.expect(&TokenKind::Comma, "`,` or `)`")?;
            }
        }
        let ret = if self.eat(&TokenKind::Arrow) {
            Some(self.ty()?)
        } else {
            None
        };
        let body = self.block("`{`")?;
        Ok(Func {
            name,
            params,
            ret,
            body,
        })
    }

    // ---- Statements ----

    fn block(&mut self, expected: &str) -> Result<Block> {
        self.expect(&TokenKind::LBrace, expected)?;
        let mut stmts = Vec::new();
        while !self.eat(&TokenKind::RBrace) {
            if *self.peek() == TokenKind::Eof {
                return Err(self.unexpected("`}`"));
            }
            stmts.push(self.stmt()?);
        }
        Ok(Block { stmts })
    }

    fn stmt(&mut self) -> Result<Stmt> {
        let start = self.span();
        let kind = match self.peek() {
            TokenKind::Local => {
                self.bump();
                let name = self.ident("a local name")?;
                self.expect(&TokenKind::Colon, "`:` (the type of a local is required)")?;
                let ty = self.ty()?;
                let init = if self.eat(&TokenKind::Eq) {
                    Some(self.expr()?)
                } else {
                    None
                };
                self.expect(&TokenKind::Semi, "`;`")?;
                StmtKind::Local { name, ty, init }
            }
            TokenKind::If => StmtKind::If(self.if_()?),
            TokenKind::While => {
                self.bump();
                let cond = self.condition("while")?;
                let body = self.block("`{` (braces are required after `while`)")?;
                StmtKind::While { cond, body }
            }
            TokenKind::Break => {
                self.bump();
                self.expect(&TokenKind::Semi, "`;`")?;
                StmtKind::Break
            }
            TokenKind::Continue => {
                self.bump();
                self.expect(&TokenKind::Semi, "`;`")?;
                StmtKind::Continue
            }
            TokenKind::Return => {
                self.bump();
                let value = if *self.peek() == TokenKind::Semi {
                    None
                } else {
                    Some(self.expr()?)
                };
                self.expect(&TokenKind::Semi, "`;`")?;
                StmtKind::Return(value)
            }
            TokenKind::Ident(_) => match self.peek_at(1) {
                TokenKind::Eq => {
                    let target = self.ident("a name")?;
                    self.bump();
                    let value = self.expr()?;
                    self.expect(&TokenKind::Semi, "`;`")?;
                    StmtKind::Assign { target, value }
                }
                TokenKind::LParen | TokenKind::ColonColon => {
                    let call = self.call()?;
                    self.expect(&TokenKind::Semi, "`;`")?;
                    StmtKind::Call(call)
                }
                _ => {
                    return err(
                        Code::Syntax,
                        start,
                        "invalid statement: only an assignment or a function call can start with a name",
                    );
                }
            },
            TokenKind::RBrace | TokenKind::Eof => return Err(self.unexpected("a statement")),
            _ => {
                return err(
                    Code::Syntax,
                    start,
                    format!(
                        "expected a statement, found {} (only function calls can be used as statements)",
                        self.peek()
                    ),
                );
            }
        };
        Ok(Stmt {
            kind,
            span: start.to(self.prev_span()),
        })
    }

    fn if_(&mut self) -> Result<If> {
        self.bump();
        let cond = self.condition("if")?;
        let then = self.block("`{` (braces are required after `if`)")?;
        let else_ = if self.eat(&TokenKind::Else) {
            if *self.peek() == TokenKind::If {
                Some(Else::If(Box::new(self.if_()?)))
            } else {
                Some(Else::Block(
                    self.block("`{` or `if` (braces are required after `else`)")?,
                ))
            }
        } else {
            None
        };
        Ok(If { cond, then, else_ })
    }

    /// Parenthesized condition of an `if` or a `while`.
    fn condition(&mut self, keyword: &str) -> Result<Expr> {
        self.expect(
            &TokenKind::LParen,
            &format!("`(` (the condition of `{keyword}` must be in parentheses)"),
        )?;
        let cond = self.expr()?;
        self.expect(&TokenKind::RParen, "`)`")?;
        Ok(cond)
    }

    fn call(&mut self) -> Result<Call> {
        let first = self.ident("a function name")?;
        let (iface, name) = if self.eat(&TokenKind::ColonColon) {
            (Some(first), self.ident("a function name")?)
        } else {
            (None, first)
        };
        self.expect(&TokenKind::LParen, "`(`")?;
        let mut args = Vec::new();
        if !self.eat(&TokenKind::RParen) {
            loop {
                args.push(self.expr()?);
                if self.eat(&TokenKind::RParen) {
                    break;
                }
                self.expect(&TokenKind::Comma, "`,` or `)`")?;
            }
        }
        Ok(Call { iface, name, args })
    }

    // ---- Expressions (§ 8.4) ----

    fn expr(&mut self) -> Result<Expr> {
        self.or()
    }

    fn binary_left(
        &mut self,
        next: fn(&mut Self) -> Result<Expr>,
        ops: &[(TokenKind, BinOp)],
    ) -> Result<Expr> {
        let mut lhs = next(self)?;
        while let Some(op) = ops
            .iter()
            .find(|(t, _)| t == self.peek())
            .map(|(_, op)| *op)
        {
            self.bump();
            let rhs = next(self)?;
            lhs = Expr {
                span: lhs.span.to(rhs.span),
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            };
        }
        Ok(lhs)
    }

    fn or(&mut self) -> Result<Expr> {
        self.binary_left(Self::and, &[(TokenKind::OrOr, BinOp::Or)])
    }

    fn and(&mut self) -> Result<Expr> {
        self.binary_left(Self::comparison, &[(TokenKind::AndAnd, BinOp::And)])
    }

    fn comparison_op(&self) -> Option<BinOp> {
        Some(match self.peek() {
            TokenKind::EqEq => BinOp::Eq,
            TokenKind::Ne => BinOp::Ne,
            TokenKind::Lt => BinOp::Lt,
            TokenKind::Le => BinOp::Le,
            TokenKind::Gt => BinOp::Gt,
            TokenKind::Ge => BinOp::Ge,
            _ => return None,
        })
    }

    fn comparison(&mut self) -> Result<Expr> {
        let lhs = self.bit_or()?;
        let Some(op) = self.comparison_op() else {
            return Ok(lhs);
        };
        self.bump();
        let rhs = self.bit_or()?;
        if self.comparison_op().is_some() {
            return err(
                Code::Syntax,
                self.span(),
                "comparisons cannot be chained: use `&&` (e.g. `a < b && b < c`)",
            );
        }
        Ok(Expr {
            span: lhs.span.to(rhs.span),
            kind: ExprKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
        })
    }

    fn bit_or(&mut self) -> Result<Expr> {
        self.binary_left(Self::bit_xor, &[(TokenKind::Pipe, BinOp::BitOr)])
    }

    fn bit_xor(&mut self) -> Result<Expr> {
        self.binary_left(Self::bit_and, &[(TokenKind::Caret, BinOp::BitXor)])
    }

    fn bit_and(&mut self) -> Result<Expr> {
        self.binary_left(Self::shift, &[(TokenKind::Amp, BinOp::BitAnd)])
    }

    fn shift(&mut self) -> Result<Expr> {
        self.binary_left(
            Self::additive,
            &[(TokenKind::Shl, BinOp::Shl), (TokenKind::Shr, BinOp::Shr)],
        )
    }

    fn additive(&mut self) -> Result<Expr> {
        self.binary_left(
            Self::multiplicative,
            &[
                (TokenKind::Plus, BinOp::Add),
                (TokenKind::Minus, BinOp::Sub),
            ],
        )
    }

    fn multiplicative(&mut self) -> Result<Expr> {
        self.binary_left(
            Self::unary,
            &[
                (TokenKind::Star, BinOp::Mul),
                (TokenKind::Slash, BinOp::Div),
                (TokenKind::Percent, BinOp::Rem),
            ],
        )
    }

    fn unary(&mut self) -> Result<Expr> {
        let op = match self.peek() {
            TokenKind::Minus => UnaryOp::Neg,
            TokenKind::Bang => UnaryOp::Not,
            _ => return self.primary(),
        };
        let start = self.bump().span;
        let expr = self.unary()?;
        Ok(Expr {
            span: start.to(expr.span),
            kind: ExprKind::Unary {
                op,
                expr: Box::new(expr),
            },
        })
    }

    fn primary(&mut self) -> Result<Expr> {
        let start = self.span();
        let kind = match self.peek().clone() {
            TokenKind::Int(v) => {
                self.bump();
                ExprKind::Int(v)
            }
            TokenKind::Float(s) => {
                self.bump();
                ExprKind::Float(s)
            }
            TokenKind::True => {
                self.bump();
                ExprKind::Bool(true)
            }
            TokenKind::False => {
                self.bump();
                ExprKind::Bool(false)
            }
            TokenKind::Ident(name) => match self.peek_at(1) {
                TokenKind::LParen | TokenKind::ColonColon => ExprKind::Call(self.call()?),
                _ => {
                    self.bump();
                    ExprKind::Name(name)
                }
            },
            TokenKind::Type(ty) => {
                self.bump();
                self.expect(
                    &TokenKind::LParen,
                    &format!("`(` after `{ty}` (a conversion is written `{ty}(value)`)"),
                )?;
                let arg = self.expr()?;
                self.expect(&TokenKind::RParen, "`)`")?;
                ExprKind::Convert {
                    ty,
                    arg: Box::new(arg),
                }
            }
            TokenKind::LParen => {
                self.bump();
                let inner = self.expr()?;
                self.expect(&TokenKind::RParen, "`)`")?;
                ExprKind::Paren(Box::new(inner))
            }
            _ => return Err(self.unexpected("an expression")),
        };
        Ok(Expr {
            kind,
            span: start.to(self.prev_span()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::source::FileId;

    fn parse_str(text: &str) -> Result<Vec<Item>> {
        parse(tokenize(FileId(0), text)?)
    }

    fn func_body(body: &str) -> Result<Vec<Item>> {
        parse_str(&format!(
            "interface i {{ func f(n: u32) -> u32 {{ {body} }} }}"
        ))
    }

    fn syntax_error(body: &str) {
        let e = func_body(body).unwrap_err();
        assert_eq!(e.code, Code::Syntax, "{body}: {}", e.message);
    }

    #[test]
    fn full_program() {
        let items = parse_str(
            "world calculator { export math; }
             global pi: f64 = 3.14159;
             global calls: mut u32 = 0;
             global neg: s32 = -5;
             interface math {
               func fib(n: u32) -> u64 {
                 calls = calls + 1;
                 local a: u64 = 0;
                 local i: u32;
                 while (i < n) { i = i + 1; if (i == 3) { break; } else if (i == 4) { continue; } else { } }
                 return u64(a) + math::id(1) * (2 - -3);
               }
               func reset() { calls = 0; return; }
             }",
        )
        .unwrap();
        assert_eq!(items.len(), 5);
    }

    #[test]
    fn precedence() {
        let items = func_body("return 1 + 2 * 3 & 4 == 5 || !true && false;").unwrap();
        let Item::Interface(i) = &items[0] else {
            panic!()
        };
        let StmtKind::Return(Some(e)) = &i.funcs[0].body.stmts[0].kind else {
            panic!()
        };
        // ((((1 + (2 * 3)) & 4) == 5) || ((!true) && false))
        let ExprKind::Binary {
            op: BinOp::Or, lhs, ..
        } = &e.kind
        else {
            panic!()
        };
        let ExprKind::Binary {
            op: BinOp::Eq, lhs, ..
        } = &lhs.kind
        else {
            panic!()
        };
        let ExprKind::Binary {
            op: BinOp::BitAnd,
            lhs,
            ..
        } = &lhs.kind
        else {
            panic!()
        };
        let ExprKind::Binary { op: BinOp::Add, .. } = &lhs.kind else {
            panic!()
        };
    }

    #[test]
    fn errors() {
        syntax_error("local x = 5;");
        syntax_error("if (n > 0) return n;");
        syntax_error("if n > 0 { return n; }");
        syntax_error("while n > 0 { }");
        syntax_error("if (1 < 2 < 3) { }");
        syntax_error("n + 1;");
        syntax_error("f(1) + 2;");
        syntax_error("1;");
        syntax_error("return n");
        syntax_error("n += 1;");
        assert_eq!(
            parse_str("global g: u32 = 1 + 1;").unwrap_err().code,
            Code::Syntax
        );
        assert_eq!(
            parse_str("interface i { global g: u32 = 1; }")
                .unwrap_err()
                .code,
            Code::Syntax
        );
    }
}
