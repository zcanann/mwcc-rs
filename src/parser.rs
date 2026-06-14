//! Tiny recursive-descent parser for the v0 subset:
//!   function := type ident '(' params ')' '{' 'return' expr ';' '}'
//!   expr     := term (('+'|'-') term)*
//!   term     := factor (('*'|'/') factor)*
//!   factor   := literal | ident | '(' expr ')'

use crate::lexer::Tok;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ty {
    Int,
    Float,
    Void,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub ty: Ty,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum Expr {
    IntLit(i64),
    FloatLit(f64),
    Var(String),
    Bin(char, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone)]
pub struct Func {
    pub ret: Ty,
    pub name: String,
    pub params: Vec<Param>,
    pub ret_expr: Expr,
}

pub struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    pub fn new(toks: Vec<Tok>) -> Self {
        Parser { toks, pos: 0 }
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.pos]
    }
    fn next(&mut self) -> Tok {
        let t = self.toks[self.pos].clone();
        self.pos += 1;
        t
    }
    fn expect(&mut self, t: Tok) -> Result<(), String> {
        if *self.peek() == t {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected {:?}, got {:?}", t, self.peek()))
        }
    }

    fn ty(&mut self) -> Result<Ty, String> {
        match self.next() {
            Tok::Int => Ok(Ty::Int),
            Tok::Float => Ok(Ty::Float),
            Tok::Void => Ok(Ty::Void),
            t => Err(format!("expected type, got {t:?}")),
        }
    }

    fn ident(&mut self) -> Result<String, String> {
        match self.next() {
            Tok::Ident(s) => Ok(s),
            t => Err(format!("expected identifier, got {t:?}")),
        }
    }

    /// Parse a single top-level function definition.
    pub fn func(&mut self) -> Result<Func, String> {
        let ret = self.ty()?;
        let name = self.ident()?;
        self.expect(Tok::LParen)?;
        let mut params = Vec::new();
        if *self.peek() == Tok::Void {
            self.next();
        } else if *self.peek() != Tok::RParen {
            loop {
                let ty = self.ty()?;
                let name = self.ident()?;
                params.push(Param { ty, name });
                if *self.peek() == Tok::Comma {
                    self.next();
                } else {
                    break;
                }
            }
        }
        self.expect(Tok::RParen)?;
        self.expect(Tok::LBrace)?;
        self.expect(Tok::Return)?;
        let ret_expr = self.expr()?;
        self.expect(Tok::Semi)?;
        self.expect(Tok::RBrace)?;
        Ok(Func { ret, name, params, ret_expr })
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let mut lhs = self.term()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => '+',
                Tok::Minus => '-',
                _ => break,
            };
            self.next();
            let rhs = self.term()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn term(&mut self) -> Result<Expr, String> {
        let mut lhs = self.factor()?;
        loop {
            let op = match self.peek() {
                Tok::Star => '*',
                Tok::Slash => '/',
                _ => break,
            };
            self.next();
            let rhs = self.factor()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn factor(&mut self) -> Result<Expr, String> {
        match self.next() {
            Tok::IntLit(n) => Ok(Expr::IntLit(n)),
            Tok::FloatLit(f) => Ok(Expr::FloatLit(f)),
            Tok::Ident(s) => Ok(Expr::Var(s)),
            Tok::LParen => {
                let e = self.expr()?;
                self.expect(Tok::RParen)?;
                Ok(e)
            }
            t => Err(format!("expected factor, got {t:?}")),
        }
    }
}
