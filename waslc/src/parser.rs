//! Recursive-descent parser (§ 5).

use crate::ast::*;
use crate::diag::{Code, Diagnostic, Result, err};
use crate::lexer::{Token, TokenKind};
use crate::source::Span;

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
            kind => match kind.keyword() {
                Some(word) => err(
                    Code::ReservedKeyword,
                    self.span(),
                    format!("`{word}` is a keyword and cannot be used as an identifier"),
                ),
                None => Err(self.unexpected(expected)),
            },
        }
    }

    /// `>` closing a list of type arguments. `>>` and `>=` are split, so that
    /// `list<list<u8>>` closes two lists.
    fn close_angle(&mut self) -> Result<()> {
        let token = &mut self.tokens[self.pos];
        let rest = match token.kind {
            TokenKind::Gt => {
                self.bump();
                return Ok(());
            }
            TokenKind::Shr => TokenKind::Gt,
            TokenKind::Ge => TokenKind::Eq,
            _ => return Err(self.unexpected("`>`")),
        };
        token.kind = rest;
        token.span.start += 1;
        Ok(())
    }

    /// A type (§ v2.3.3).
    fn ty(&mut self) -> Result<TypeExpr> {
        let start = self.span();
        let kind = match self.peek().clone() {
            TokenKind::Type(ty) => {
                self.bump();
                TypeExprKind::Prim(ty)
            }
            TokenKind::KwChar => {
                self.bump();
                TypeExprKind::Char
            }
            TokenKind::KwString => {
                self.bump();
                TypeExprKind::String
            }
            TokenKind::KwList => {
                self.bump();
                self.expect(&TokenKind::Lt, "`<` (a list is written `list<T>`)")?;
                let mutable = self.eat(&TokenKind::Mut);
                let elem = Box::new(self.ty()?);
                self.close_angle()?;
                TypeExprKind::List { mutable, elem }
            }
            TokenKind::KwOption => {
                self.bump();
                self.expect(&TokenKind::Lt, "`<` (an option is written `option<T>`)")?;
                let inner = Box::new(self.ty()?);
                self.close_angle()?;
                TypeExprKind::Option(inner)
            }
            TokenKind::KwTuple => {
                self.bump();
                self.expect(&TokenKind::Lt, "`<` (a tuple is written `tuple<T1, T2>`)")?;
                let mut elems = vec![self.ty()?];
                while self.eat(&TokenKind::Comma) {
                    elems.push(self.ty()?);
                }
                self.close_angle()?;
                TypeExprKind::Tuple(elems)
            }
            TokenKind::KwResult => {
                self.bump();
                if self.eat(&TokenKind::Lt) {
                    let ok = if self.eat(&TokenKind::Underscore) {
                        if *self.peek() != TokenKind::Comma {
                            return Err(self.unexpected("`,` (`_` is only used in `result<_, E>`)"));
                        }
                        None
                    } else {
                        Some(Box::new(self.ty()?))
                    };
                    let err = if self.eat(&TokenKind::Comma) {
                        Some(Box::new(self.ty()?))
                    } else {
                        None
                    };
                    self.close_angle()?;
                    TypeExprKind::Result { ok, err }
                } else {
                    TypeExprKind::Result {
                        ok: None,
                        err: None,
                    }
                }
            }
            TokenKind::Ident(_) => TypeExprKind::Named(self.type_name()?),
            _ => return Err(self.unexpected("a type")),
        };
        Ok(TypeExpr {
            kind,
            span: start.to(self.prev_span()),
        })
    }

    /// `name` or `interface::name`.
    fn type_name(&mut self) -> Result<TypeName> {
        let first = self.ident("a type name")?;
        if self.eat(&TokenKind::ColonColon) {
            let name = self.ident("a type name")?;
            Ok(TypeName {
                iface: Some(first),
                name,
            })
        } else {
            Ok(TypeName {
                iface: None,
                name: first,
            })
        }
    }

    // ---- Declarations ----

    fn item(&mut self) -> Result<Item> {
        match self.peek() {
            TokenKind::World => self.world().map(Item::World),
            TokenKind::Interface => self.interface().map(Item::Interface),
            TokenKind::Global => self.global().map(|g| Item::Global(Box::new(g))),
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
        let mut items = Vec::new();
        while !self.eat(&TokenKind::RBrace) {
            let item = match self.peek() {
                TokenKind::Func => InterfaceItem::Func(self.func()?),
                TokenKind::Record | TokenKind::Enum | TokenKind::Variant | TokenKind::Flags => {
                    InterfaceItem::Type(self.type_decl()?)
                }
                TokenKind::Global => {
                    return err(
                        Code::Syntax,
                        self.span(),
                        "globals are declared at the top level, not inside an interface",
                    );
                }
                _ => {
                    return Err(
                        self.unexpected("`func`, `record`, `enum`, `variant`, `flags` or `}`")
                    );
                }
            };
            items.push(item);
        }
        Ok(Interface { name, items })
    }

    /// `record`, `enum`, `variant` or `flags` declaration (§ v2.2.3).
    fn type_decl(&mut self) -> Result<TypeDecl> {
        let keyword = self.bump().kind;
        let name = self.ident("a type name")?;
        self.expect(&TokenKind::LBrace, "`{`")?;
        let what = match keyword {
            TokenKind::Record => "a field name",
            TokenKind::Flags => "a flag name",
            _ => "a case name",
        };
        if *self.peek() == TokenKind::RBrace {
            return Err(self.unexpected(&format!("{what} (a {keyword} cannot be empty)")));
        }
        let mut fields = Vec::new();
        let mut names = Vec::new();
        let mut cases = Vec::new();
        loop {
            let item = self.ident(what)?;
            match keyword {
                TokenKind::Record => {
                    self.expect(&TokenKind::Colon, "`:` (the type of a field is required)")?;
                    let mutable = self.eat(&TokenKind::Mut);
                    let ty = self.ty()?;
                    fields.push(Field {
                        name: item,
                        mutable,
                        ty,
                    });
                }
                TokenKind::Variant => {
                    let payload = if self.eat(&TokenKind::LParen) {
                        let ty = self.ty()?;
                        self.expect(&TokenKind::RParen, "`)` (a case has a payload of one type: use a record or a tuple for several values)")?;
                        Some(ty)
                    } else {
                        None
                    };
                    cases.push(Case {
                        name: item,
                        payload,
                    });
                }
                _ => names.push(item),
            }
            if self.eat(&TokenKind::RBrace) {
                break;
            }
            self.expect(&TokenKind::Comma, "`,` or `}`")?;
            if self.eat(&TokenKind::RBrace) {
                break;
            }
        }
        let kind = match keyword {
            TokenKind::Record => TypeDeclKind::Record(fields),
            TokenKind::Variant => TypeDeclKind::Variant(cases),
            TokenKind::Enum => TypeDeclKind::Enum(names),
            _ => TypeDeclKind::Flags(names),
        };
        Ok(TypeDecl { name, kind })
    }

    fn global(&mut self) -> Result<Global> {
        self.bump();
        let name = self.ident("a global name")?;
        self.expect(&TokenKind::Colon, "`:` (the type of a global is required)")?;
        let mutable = self.eat(&TokenKind::Mut);
        let ty = self.ty()?;
        self.expect(&TokenKind::Eq, "`=` (a global must be initialized)")?;
        let init = self.expr()?;
        self.expect(&TokenKind::Semi, "`;`")?;
        Ok(Global {
            name,
            mutable,
            ty,
            init,
        })
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
            TokenKind::Match => StmtKind::Match(self.match_()?),
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
                TokenKind::Eq | TokenKind::Dot | TokenKind::LBracket => {
                    let target = self.place()?;
                    self.expect(&TokenKind::Eq, "`=` (a statement that starts with a name followed by `.` or `[` is an assignment)")?;
                    let value = self.expr()?;
                    self.expect(&TokenKind::Semi, "`;`")?;
                    StmtKind::Assign { target, value }
                }
                TokenKind::LParen | TokenKind::ColonColon => self.call_stmt(start)?,
                _ => {
                    return err(
                        Code::Syntax,
                        start,
                        "invalid statement: only an assignment or a function call can start with a name",
                    );
                }
            },
            TokenKind::KwString
            | TokenKind::KwList
            | TokenKind::KwChar
            | TokenKind::KwOption
            | TokenKind::KwResult
                if *self.peek_at(1) == TokenKind::ColonColon =>
            {
                self.call_stmt(start)?
            }
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

    /// A call used as a statement: `f(…);`, `i::f(…);`.
    fn call_stmt(&mut self, start: Span) -> Result<StmtKind> {
        let path = self.path_segments()?;
        if *self.peek() != TokenKind::LParen {
            return err(
                Code::Syntax,
                start,
                "invalid statement: only an assignment or a function call can start with a name",
            );
        }
        let call = self.call_args(path)?;
        self.expect(&TokenKind::Semi, "`;`")?;
        Ok(StmtKind::Call(call))
    }

    /// Target of an assignment: `x`, `inv.status`, `rows[i].cells[j]`.
    fn place(&mut self) -> Result<Place> {
        let name = self.ident("a name")?;
        let mut selectors = Vec::new();
        loop {
            match self.peek() {
                TokenKind::Dot => {
                    self.bump();
                    selectors.push(self.dot_selector()?);
                }
                TokenKind::LBracket => {
                    self.bump();
                    let index = self.expr()?;
                    self.expect(&TokenKind::RBracket, "`]`")?;
                    selectors.push(Selector::Index(index));
                }
                _ => return Ok(Place { name, selectors }),
            }
        }
    }

    /// What follows a `.`: a field name or a tuple index.
    fn dot_selector(&mut self) -> Result<Selector> {
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                let span = self.bump().span;
                Ok(Selector::Field(Ident { name, span }))
            }
            TokenKind::Int(index) => {
                let span = self.bump().span;
                Ok(Selector::Elem(index, span))
            }
            _ => Err(self.unexpected("a field name or a tuple index after `.`")),
        }
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

    /// `match (e) { case c { … } case d(x) { … } else { … } }` (§ v2.4.9).
    fn match_(&mut self) -> Result<Match> {
        self.bump();
        let scrutinee = self.condition("match")?;
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut cases = Vec::new();
        let mut else_ = None;
        loop {
            match self.peek() {
                TokenKind::Case => {
                    self.bump();
                    let name = self.ident("a case name")?;
                    let binding = if self.eat(&TokenKind::LParen) {
                        let binding = self.ident("a name for the payload")?;
                        self.expect(&TokenKind::RParen, "`)`")?;
                        Some(binding)
                    } else {
                        None
                    };
                    let body = self.block("`{` (braces are required after `case`)")?;
                    cases.push(MatchCase {
                        name,
                        binding,
                        body,
                    });
                }
                TokenKind::Else if !cases.is_empty() => {
                    self.bump();
                    else_ = Some(self.block("`{` (braces are required after `else`)")?);
                    self.expect(
                        &TokenKind::RBrace,
                        "`}` (`else` is the last block of a `match`)",
                    )?;
                    break;
                }
                TokenKind::RBrace if !cases.is_empty() => {
                    self.bump();
                    break;
                }
                _ if cases.is_empty() => {
                    return Err(self.unexpected("`case` (a `match` has at least one case)"));
                }
                _ => return Err(self.unexpected("`case`, `else` or `}`")),
            }
        }
        Ok(Match {
            scrutinee,
            cases,
            else_,
        })
    }

    /// Parenthesized condition of an `if`, a `while` or a `match`.
    fn condition(&mut self, keyword: &str) -> Result<Expr> {
        self.expect(
            &TokenKind::LParen,
            &format!("`(` (the expression of `{keyword}` must be in parentheses)"),
        )?;
        let cond = self.expr()?;
        self.expect(&TokenKind::RParen, "`)`")?;
        Ok(cond)
    }

    /// A name, or a path of up to three names separated by `::` (§ v2.3.3). The first name
    /// may be one of the built-in types that have functions or cases.
    fn path_segments(&mut self) -> Result<Vec<Ident>> {
        let head = match self.peek().clone() {
            TokenKind::Ident(name) => name,
            TokenKind::KwChar => "char".to_string(),
            TokenKind::KwString => "string".to_string(),
            TokenKind::KwList => "list".to_string(),
            TokenKind::KwOption => "option".to_string(),
            TokenKind::KwResult => "result".to_string(),
            _ => return Err(self.unexpected("a name")),
        };
        let span = self.bump().span;
        let mut segments = vec![Ident { name: head, span }];
        while self.eat(&TokenKind::ColonColon) {
            if segments.len() == 3 {
                return err(
                    Code::Syntax,
                    self.prev_span(),
                    "a path has at most two `::`",
                );
            }
            segments.push(self.ident("a name after `::`")?);
        }
        Ok(segments)
    }

    /// Arguments of a call, after the path `segments`.
    fn call_args(&mut self, mut segments: Vec<Ident>) -> Result<Call> {
        self.expect(&TokenKind::LParen, "`(`")?;
        let args = self.expr_list(&TokenKind::RParen, "`)`", false)?;
        let name = segments.pop().expect("a path has a name");
        Ok(Call {
            path: segments,
            name,
            args,
        })
    }

    /// Expressions separated by commas up to `close`, with an optional trailing comma if
    /// `trailing`.
    fn expr_list(
        &mut self,
        close: &TokenKind,
        close_text: &str,
        trailing: bool,
    ) -> Result<Vec<Expr>> {
        let mut items = Vec::new();
        if self.eat(close) {
            return Ok(items);
        }
        loop {
            items.push(self.expr()?);
            if self.eat(close) {
                return Ok(items);
            }
            self.expect(&TokenKind::Comma, &format!("`,` or {close_text}"))?;
            if trailing && self.eat(close) {
                return Ok(items);
            }
        }
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
            _ => return self.postfix(),
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

    /// A primary expression followed by field accesses and indexes (§ v2.3.5).
    fn postfix(&mut self) -> Result<Expr> {
        let mut expr = self.primary()?;
        loop {
            let kind = match self.peek() {
                TokenKind::Dot => {
                    self.bump();
                    match self.dot_selector()? {
                        Selector::Field(name) => ExprKind::Field {
                            expr: Box::new(expr),
                            name,
                        },
                        Selector::Elem(index, index_span) => ExprKind::Elem {
                            expr: Box::new(expr),
                            index,
                            index_span,
                        },
                        Selector::Index(_) => unreachable!(),
                    }
                }
                TokenKind::LBracket => {
                    self.bump();
                    let index = self.expr()?;
                    self.expect(&TokenKind::RBracket, "`]`")?;
                    ExprKind::Index {
                        expr: Box::new(expr),
                        index: Box::new(index),
                    }
                }
                _ => return Ok(expr),
            };
            expr = Expr {
                span: expr_span(&kind).to(self.prev_span()),
                kind,
            };
        }
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
            TokenKind::Str(s) => {
                self.bump();
                ExprKind::Str(s)
            }
            TokenKind::Char(c) => {
                self.bump();
                ExprKind::Char(c)
            }
            TokenKind::True => {
                self.bump();
                ExprKind::Bool(true)
            }
            TokenKind::False => {
                self.bump();
                ExprKind::Bool(false)
            }
            TokenKind::Ident(_) => self.name_expr()?,
            TokenKind::KwChar | TokenKind::KwString if *self.peek_at(1) == TokenKind::LParen => {
                self.conversion()?
            }
            TokenKind::KwChar
            | TokenKind::KwString
            | TokenKind::KwList
            | TokenKind::KwOption
            | TokenKind::KwResult => self.name_expr()?,
            TokenKind::Type(_) => self.conversion()?,
            TokenKind::KwTuple => {
                self.bump();
                self.expect(&TokenKind::LParen, "`(` (a tuple is written `tuple(a, b)`)")?;
                let elems = self.expr_list(&TokenKind::RParen, "`)`", true)?;
                if elems.is_empty() {
                    return err(
                        Code::Syntax,
                        start.to(self.prev_span()),
                        "a tuple has at least one element",
                    );
                }
                ExprKind::Tuple(elems)
            }
            TokenKind::LBracket => {
                self.bump();
                ExprKind::List(self.expr_list(&TokenKind::RBracket, "`]`", true)?)
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

    /// `T(x)`: a conversion.
    fn conversion(&mut self) -> Result<ExprKind> {
        let ty = self.ty()?;
        self.expect(
            &TokenKind::LParen,
            "`(` (a conversion is written `type(value)`)",
        )?;
        let arg = self.expr()?;
        self.expect(&TokenKind::RParen, "`)`")?;
        Ok(ExprKind::Convert {
            ty,
            arg: Box::new(arg),
        })
    }

    /// An expression that starts with a name: a variable, a path, a call, or a record or
    /// flags literal.
    fn name_expr(&mut self) -> Result<ExprKind> {
        let builtin_head = !matches!(self.peek(), TokenKind::Ident(_));
        let mut segments = self.path_segments()?;
        match self.peek() {
            TokenKind::LParen => Ok(ExprKind::Call(self.call_args(segments)?)),
            // `{` right after a type name: a record or flags literal (§ v2.3.4).
            TokenKind::LBrace if !builtin_head && segments.len() <= 2 => {
                let name = segments.pop().expect("a path has a name");
                let ty = TypeName {
                    iface: segments.pop(),
                    name,
                };
                self.bump();
                let is_record = matches!(self.peek(), TokenKind::Ident(_))
                    && *self.peek_at(1) == TokenKind::Colon;
                if is_record {
                    let mut fields = Vec::new();
                    loop {
                        let name = self.ident("a field name")?;
                        self.expect(&TokenKind::Colon, "`:`")?;
                        let value = self.expr()?;
                        fields.push((name, value));
                        if self.eat(&TokenKind::RBrace) {
                            break;
                        }
                        self.expect(&TokenKind::Comma, "`,` or `}`")?;
                        if self.eat(&TokenKind::RBrace) {
                            break;
                        }
                    }
                    Ok(ExprKind::Record { ty, fields })
                } else {
                    let mut flags = Vec::new();
                    loop {
                        if self.eat(&TokenKind::RBrace) {
                            break;
                        }
                        flags.push(self.ident("a flag name or `}`")?);
                        if self.eat(&TokenKind::RBrace) {
                            break;
                        }
                        self.expect(&TokenKind::Comma, "`,` or `}`")?;
                    }
                    Ok(ExprKind::Flags { ty, flags })
                }
            }
            _ if segments.len() == 1 && !builtin_head => {
                Ok(ExprKind::Name(segments.pop().unwrap().name))
            }
            _ if segments.len() == 1 => Err(self.unexpected("`::` or `(`")),
            _ => Ok(ExprKind::Path(segments)),
        }
    }
}

/// Start of the span of a postfix expression: the span of its operand.
fn expr_span(kind: &ExprKind) -> Span {
    match kind {
        ExprKind::Field { expr, .. }
        | ExprKind::Elem { expr, .. }
        | ExprKind::Index { expr, .. } => expr.span,
        _ => unreachable!(),
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
        let StmtKind::Return(Some(e)) = &i.funcs().next().unwrap().body.stmts[0].kind else {
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
    fn target_program() {
        let items = parse_str(include_str!("../../examples/invoice/src/invoice.wasl")).unwrap();
        assert_eq!(items.len(), 5);
    }

    #[test]
    fn v2_constructs() {
        let items = parse_str(
            "global first: invoices::line = invoices::line { label: \"x\", quantity: 1, unit_price: 0, };
             interface i {
               enum e { a, b, }
               flags f { x, y }
               variant v { p(list<list<u8>>), q, r(result<_, e>) }
               record r { a: mut tuple<u32, option<char>>, b: list<mut string>, c: result }
               func f(t: tuple<u32, u32>, r: result<u8>) -> result<list<u8>, e> {
                 local xs: list<mut u8>= [1, 2, 3,];
                 xs[0] = t.0 + t.1;
                 rows[i].cells[j] = string::len(\"a\") + u32('a');
                 local d: f = f { x, y } | f { };
                 local m: bool = d.x && a.b[0].c.0.1;
                 string::len(s);
                 match (v::q) {
                   case p(x) { return result::ok(list::freeze(x)); }
                   case q { }
                   else { break; }
                 }
                 return result::err(e::a);
               }
             }",
        )
        .unwrap();
        assert_eq!(items.len(), 2);
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
        syntax_error("inv.status;");
        syntax_error("f().x = 1;");
        syntax_error("match (n) { }");
        syntax_error("match (n) { else { } }");
        syntax_error("match (n) { case a { } else { } case b { } }");
        syntax_error("match (n) { case a return; }");
        syntax_error("local t: tuple<> = tuple();");
        syntax_error("local t: u32 = tuple();");
        syntax_error("local x: u32 = a::b::c::d;");
        syntax_error("local x: u32 = f(1,);");
        syntax_error("local x: result<_> = result::ok;");
        syntax_error("option::none;");
        for decl in [
            "record r { }",
            "enum e { }",
            "flags f { }",
            "variant v { }",
            "variant v { a(u8, u8) }",
        ] {
            assert_eq!(
                parse_str(&format!("interface i {{ {decl} }}"))
                    .unwrap_err()
                    .code,
                Code::Syntax,
                "{decl}"
            );
        }
        assert_eq!(
            parse_str("world w { record r { a: u32 } }")
                .unwrap_err()
                .code,
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
