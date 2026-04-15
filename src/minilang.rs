use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::ir::{Function, IrBuildError, IrBuilder, Type, ValueId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MiniLangError {
    Lex(LexError),
    Parse(ParseError),
    Codegen(CodegenError),
}

impl Display for MiniLangError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lex(err) => write!(f, "lexer error: {err}"),
            Self::Parse(err) => write!(f, "parser error: {err}"),
            Self::Codegen(err) => write!(f, "codegen error: {err}"),
        }
    }
}

impl Error for MiniLangError {}

impl From<LexError> for MiniLangError {
    fn from(value: LexError) -> Self {
        Self::Lex(value)
    }
}

impl From<ParseError> for MiniLangError {
    fn from(value: ParseError) -> Self {
        Self::Parse(value)
    }
}

impl From<CodegenError> for MiniLangError {
    fn from(value: CodegenError) -> Self {
        Self::Codegen(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub functions: Vec<FunctionAst>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionAst {
    pub name: String,
    pub params: Vec<String>,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Number(i64),
    Var(String),
    Call {
        callee: String,
        args: Vec<Expr>,
    },
    UnaryNeg(Box<Expr>),
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    And,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Fn,
    Ident(String),
    Number(i64),
    LParen,
    RParen,
    Comma,
    Eq,
    Semicolon,
    Plus,
    Minus,
    Star,
    Slash,
    Amp,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub pos: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub pos: usize,
    pub message: String,
}

impl Display for LexError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "at byte {}: {}", self.pos, self.message)
    }
}

impl Error for LexError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub pos: usize,
    pub message: String,
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "at byte {}: {}", self.pos, self.message)
    }
}

impl Error for ParseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    UnknownVariable(String),
    IntegerOutOfRange(i64),
    VoidCallResult(String),
    Ir(IrBuildError),
}

impl Display for CodegenError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownVariable(name) => write!(f, "unknown variable: {name}"),
            Self::IntegerOutOfRange(value) => {
                write!(f, "integer literal out of i32 range: {value}")
            }
            Self::VoidCallResult(name) => write!(f, "call to {name} did not produce a value"),
            Self::Ir(err) => write!(f, "IR error: {err}"),
        }
    }
}

impl Error for CodegenError {}

impl From<IrBuildError> for CodegenError {
    fn from(value: IrBuildError) -> Self {
        Self::Ir(value)
    }
}

pub fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    let bytes = source.as_bytes();
    let mut pos = 0usize;
    let mut tokens = Vec::new();

    while pos < bytes.len() {
        let ch = bytes[pos] as char;
        match ch {
            ' ' | '\t' | '\n' | '\r' => {
                pos += 1;
            }
            '(' => {
                tokens.push(Token {
                    kind: TokenKind::LParen,
                    pos,
                });
                pos += 1;
            }
            ')' => {
                tokens.push(Token {
                    kind: TokenKind::RParen,
                    pos,
                });
                pos += 1;
            }
            ',' => {
                tokens.push(Token {
                    kind: TokenKind::Comma,
                    pos,
                });
                pos += 1;
            }
            '=' => {
                tokens.push(Token {
                    kind: TokenKind::Eq,
                    pos,
                });
                pos += 1;
            }
            ';' => {
                tokens.push(Token {
                    kind: TokenKind::Semicolon,
                    pos,
                });
                pos += 1;
            }
            '+' => {
                tokens.push(Token {
                    kind: TokenKind::Plus,
                    pos,
                });
                pos += 1;
            }
            '-' => {
                tokens.push(Token {
                    kind: TokenKind::Minus,
                    pos,
                });
                pos += 1;
            }
            '*' => {
                tokens.push(Token {
                    kind: TokenKind::Star,
                    pos,
                });
                pos += 1;
            }
            '/' => {
                tokens.push(Token {
                    kind: TokenKind::Slash,
                    pos,
                });
                pos += 1;
            }
            '&' => {
                tokens.push(Token {
                    kind: TokenKind::Amp,
                    pos,
                });
                pos += 1;
            }
            '0'..='9' => {
                let start = pos;
                while pos < bytes.len() && (bytes[pos] as char).is_ascii_digit() {
                    pos += 1;
                }
                let text = &source[start..pos];
                let number = text.parse::<i64>().map_err(|_| LexError {
                    pos: start,
                    message: format!("invalid integer literal: {text}"),
                })?;
                tokens.push(Token {
                    kind: TokenKind::Number(number),
                    pos: start,
                });
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let start = pos;
                while pos < bytes.len() {
                    let c = bytes[pos] as char;
                    if c.is_ascii_alphanumeric() || c == '_' {
                        pos += 1;
                    } else {
                        break;
                    }
                }
                let ident = &source[start..pos];
                let kind = if ident == "fn" {
                    TokenKind::Fn
                } else {
                    TokenKind::Ident(ident.to_string())
                };
                tokens.push(Token { kind, pos: start });
            }
            _ => {
                return Err(LexError {
                    pos,
                    message: format!("unexpected character '{ch}'"),
                });
            }
        }
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        pos,
    });

    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, index: 0 }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.index]
    }

    fn bump(&mut self) {
        if self.index + 1 < self.tokens.len() {
            self.index += 1;
        }
    }

    fn expect_symbol(&mut self, expected: TokenKind, text: &'static str) -> Result<(), ParseError> {
        if self.current().kind == expected {
            self.bump();
            Ok(())
        } else {
            Err(ParseError {
                pos: self.current().pos,
                message: format!("expected {text}"),
            })
        }
    }

    fn expect_ident(&mut self, context: &'static str) -> Result<String, ParseError> {
        match &self.current().kind {
            TokenKind::Ident(name) => {
                let out = name.clone();
                self.bump();
                Ok(out)
            }
            _ => Err(ParseError {
                pos: self.current().pos,
                message: format!("expected identifier for {context}"),
            }),
        }
    }

    fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut functions = Vec::new();
        while self.current().kind != TokenKind::Eof {
            functions.push(self.parse_function()?);
        }
        Ok(Program { functions })
    }

    fn parse_function(&mut self) -> Result<FunctionAst, ParseError> {
        self.expect_symbol(TokenKind::Fn, "'fn'")?;
        let name = self.expect_ident("function name")?;
        self.expect_symbol(TokenKind::LParen, "'('")?;

        let mut params = Vec::new();
        if self.current().kind != TokenKind::RParen {
            loop {
                params.push(self.expect_ident("parameter")?);
                if self.current().kind == TokenKind::Comma {
                    self.bump();
                    continue;
                }
                break;
            }
        }

        self.expect_symbol(TokenKind::RParen, "')'")?;
        self.expect_symbol(TokenKind::Eq, "'='")?;
        let body = self.parse_expr(0)?;
        self.expect_symbol(TokenKind::Semicolon, "';'")?;

        Ok(FunctionAst { name, params, body })
    }

    fn parse_expr(&mut self, min_prec: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_prefix()?;

        while let Some((op, prec)) = self.current_binary_op() {
            if prec < min_prec {
                break;
            }
            self.bump();
            let rhs = self.parse_expr(prec + 1)?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }

        Ok(lhs)
    }

    fn parse_prefix(&mut self) -> Result<Expr, ParseError> {
        match &self.current().kind {
            TokenKind::Number(value) => {
                let out = Expr::Number(*value);
                self.bump();
                Ok(out)
            }
            TokenKind::Ident(name) => {
                let ident = name.clone();
                self.bump();
                if self.current().kind == TokenKind::LParen {
                    self.bump();
                    let mut args = Vec::new();
                    if self.current().kind != TokenKind::RParen {
                        loop {
                            args.push(self.parse_expr(0)?);
                            if self.current().kind == TokenKind::Comma {
                                self.bump();
                                continue;
                            }
                            break;
                        }
                    }
                    self.expect_symbol(TokenKind::RParen, "')'")?;
                    Ok(Expr::Call {
                        callee: ident,
                        args,
                    })
                } else {
                    Ok(Expr::Var(ident))
                }
            }
            TokenKind::LParen => {
                self.bump();
                let expr = self.parse_expr(0)?;
                self.expect_symbol(TokenKind::RParen, "')'")?;
                Ok(expr)
            }
            TokenKind::Minus => {
                self.bump();
                let inner = self.parse_expr(40)?;
                Ok(Expr::UnaryNeg(Box::new(inner)))
            }
            _ => Err(ParseError {
                pos: self.current().pos,
                message: "expected expression".to_string(),
            }),
        }
    }

    fn current_binary_op(&self) -> Option<(BinaryOp, u8)> {
        match self.current().kind {
            TokenKind::Amp => Some((BinaryOp::And, 10)),
            TokenKind::Plus => Some((BinaryOp::Add, 20)),
            TokenKind::Minus => Some((BinaryOp::Sub, 20)),
            TokenKind::Star => Some((BinaryOp::Mul, 30)),
            TokenKind::Slash => Some((BinaryOp::Div, 30)),
            _ => None,
        }
    }
}

pub fn parse(tokens: Vec<Token>) -> Result<Program, ParseError> {
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

pub fn parse_source(source: &str) -> Result<Program, MiniLangError> {
    let tokens = lex(source)?;
    let program = parse(tokens)?;
    Ok(program)
}

pub fn compile_source_to_ir(source: &str) -> Result<Vec<Function>, MiniLangError> {
    let program = parse_source(source)?;
    codegen_program(&program).map_err(MiniLangError::from)
}

pub fn codegen_program(program: &Program) -> Result<Vec<Function>, CodegenError> {
    let mut out = Vec::new();
    for function in &program.functions {
        out.push(codegen_function(function)?);
    }
    Ok(out)
}

fn codegen_function(function: &FunctionAst) -> Result<Function, CodegenError> {
    let mut builder = IrBuilder::new(function.name.clone(), Type::I32);
    let mut vars = HashMap::new();

    for param in &function.params {
        let value = builder.add_param(param.clone(), Type::I32);
        vars.insert(param.clone(), value);
    }

    let entry = builder.create_block("entry");
    builder.position_at_end(entry)?;

    let value = codegen_expr(&mut builder, &mut vars, &function.body)?;
    builder.build_ret(Some(value))?;

    Ok(builder.finish())
}

fn codegen_expr(
    builder: &mut IrBuilder,
    vars: &mut HashMap<String, ValueId>,
    expr: &Expr,
) -> Result<ValueId, CodegenError> {
    match expr {
        Expr::Number(value) => {
            let value =
                i32::try_from(*value).map_err(|_| CodegenError::IntegerOutOfRange(*value))?;
            Ok(builder.build_const_i32(value)?)
        }
        Expr::Var(name) => vars
            .get(name)
            .copied()
            .ok_or_else(|| CodegenError::UnknownVariable(name.clone())),
        Expr::Call { callee, args } => {
            let mut lowered_args = Vec::new();
            for arg in args {
                let value = codegen_expr(builder, vars, arg)?;
                lowered_args.push((Type::I32, value));
            }
            let result = builder
                .build_call(Type::I32, callee.clone(), lowered_args)?
                .ok_or_else(|| CodegenError::VoidCallResult(callee.clone()))?;
            Ok(result)
        }
        Expr::UnaryNeg(inner) => {
            let zero = builder.build_const_i32(0)?;
            let value = codegen_expr(builder, vars, inner)?;
            Ok(builder.build_sub(zero, value)?)
        }
        Expr::Binary { op, lhs, rhs } => {
            let lhs = codegen_expr(builder, vars, lhs)?;
            let rhs = codegen_expr(builder, vars, rhs)?;
            match op {
                BinaryOp::Add => Ok(builder.build_add(lhs, rhs)?),
                BinaryOp::Sub => Ok(builder.build_sub(lhs, rhs)?),
                BinaryOp::Mul => Ok(builder.build_mul(lhs, rhs)?),
                BinaryOp::Div => Ok(builder.build_sdiv(lhs, rhs)?),
                BinaryOp::And => Ok(builder.build_and(lhs, rhs)?),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_simple_program() {
        let tokens = lex("fn main() = 40 + 2;").expect("lex should succeed");
        assert!(tokens.iter().any(|tok| matches!(tok.kind, TokenKind::Fn)));
        assert!(tokens.iter().any(|tok| matches!(tok.kind, TokenKind::Plus)));
    }

    #[test]
    fn parses_call_expression() {
        let program = parse_source("fn main() = add2(40, 2);").expect("parse should succeed");
        assert_eq!(program.functions.len(), 1);
        match &program.functions[0].body {
            Expr::Call { callee, args } => {
                assert_eq!(callee, "add2");
                assert_eq!(args.len(), 2);
            }
            other => panic!("expected call expression, got {other:?}"),
        }
    }

    #[test]
    fn codegen_builds_ir_functions() {
        let src = r#"
            fn add2(a, b) = a + b;
            fn main() = add2(40, 2);
        "#;
        let functions = compile_source_to_ir(src).expect("compile should succeed");
        assert_eq!(functions.len(), 2);
        assert!(functions.iter().any(|func| func.name == "main"));
        assert!(functions.iter().any(|func| func.name == "add2"));
    }
}
