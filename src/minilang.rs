use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::ir::{Function, IcmpPredicate, IrBuildError, IrBuilder, PhiIncoming, Type, ValueId};

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
    StringLiteral(String),
    ArrayLiteral(Vec<Expr>),
    Var(String),
    Call {
        callee: String,
        args: Vec<Expr>,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    If {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
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
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Fn,
    If,
    Then,
    Else,
    Ident(String),
    Number(i64),
    StringLit(String),
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Eq,
    Semicolon,
    Plus,
    Minus,
    Star,
    Slash,
    Amp,
    EqEq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
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
    TypeMismatch {
        context: &'static str,
        expected: &'static str,
        found: &'static str,
    },
    VoidCallResult(String),
    InvalidIndexBase,
    NonConstantIndex,
    IndexOutOfBounds {
        index: i64,
        len: usize,
    },
    InvalidLenOperand,
    Ir(IrBuildError),
}

impl Display for CodegenError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownVariable(name) => write!(f, "unknown variable: {name}"),
            Self::IntegerOutOfRange(value) => {
                write!(f, "integer literal out of i32 range: {value}")
            }
            Self::TypeMismatch {
                context,
                expected,
                found,
            } => write!(
                f,
                "type mismatch in {context}: expected {expected}, found {found}"
            ),
            Self::VoidCallResult(name) => write!(f, "call to {name} did not produce a value"),
            Self::InvalidIndexBase => write!(f, "indexing is only valid on arrays/strings"),
            Self::NonConstantIndex => {
                write!(f, "array/string index must be a compile-time constant")
            }
            Self::IndexOutOfBounds { index, len } => {
                write!(f, "index {index} out of bounds for length {len}")
            }
            Self::InvalidLenOperand => write!(f, "len(...) expects an array or string"),
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
            '[' => {
                tokens.push(Token {
                    kind: TokenKind::LBracket,
                    pos,
                });
                pos += 1;
            }
            ']' => {
                tokens.push(Token {
                    kind: TokenKind::RBracket,
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
            '=' => {
                if pos + 1 < bytes.len() && bytes[pos + 1] as char == '=' {
                    tokens.push(Token {
                        kind: TokenKind::EqEq,
                        pos,
                    });
                    pos += 2;
                } else {
                    tokens.push(Token {
                        kind: TokenKind::Eq,
                        pos,
                    });
                    pos += 1;
                }
            }
            '!' => {
                if pos + 1 < bytes.len() && bytes[pos + 1] as char == '=' {
                    tokens.push(Token {
                        kind: TokenKind::Neq,
                        pos,
                    });
                    pos += 2;
                } else {
                    return Err(LexError {
                        pos,
                        message: "expected '!='".to_string(),
                    });
                }
            }
            '<' => {
                if pos + 1 < bytes.len() && bytes[pos + 1] as char == '=' {
                    tokens.push(Token {
                        kind: TokenKind::Le,
                        pos,
                    });
                    pos += 2;
                } else {
                    tokens.push(Token {
                        kind: TokenKind::Lt,
                        pos,
                    });
                    pos += 1;
                }
            }
            '>' => {
                if pos + 1 < bytes.len() && bytes[pos + 1] as char == '=' {
                    tokens.push(Token {
                        kind: TokenKind::Ge,
                        pos,
                    });
                    pos += 2;
                } else {
                    tokens.push(Token {
                        kind: TokenKind::Gt,
                        pos,
                    });
                    pos += 1;
                }
            }
            '"' => {
                let start = pos;
                pos += 1;
                let mut out = String::new();
                while pos < bytes.len() {
                    let c = bytes[pos] as char;
                    if c == '"' {
                        pos += 1;
                        break;
                    }
                    if c == '\\' {
                        pos += 1;
                        if pos >= bytes.len() {
                            return Err(LexError {
                                pos: start,
                                message: "unterminated string literal".to_string(),
                            });
                        }
                        let esc = bytes[pos] as char;
                        let decoded = match esc {
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            '"' => '"',
                            '\\' => '\\',
                            _ => {
                                return Err(LexError {
                                    pos,
                                    message: format!("unsupported escape sequence: \\{esc}"),
                                });
                            }
                        };
                        out.push(decoded);
                        pos += 1;
                        continue;
                    }

                    out.push(c);
                    pos += 1;
                }

                if pos > bytes.len() || bytes.get(pos.saturating_sub(1)).copied() != Some(b'"') {
                    return Err(LexError {
                        pos: start,
                        message: "unterminated string literal".to_string(),
                    });
                }

                tokens.push(Token {
                    kind: TokenKind::StringLit(out),
                    pos: start,
                });
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
                let kind = match ident {
                    "fn" => TokenKind::Fn,
                    "if" => TokenKind::If,
                    "then" => TokenKind::Then,
                    "else" => TokenKind::Else,
                    _ => TokenKind::Ident(ident.to_string()),
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
        if self.current().kind == TokenKind::Minus {
            self.bump();
            let inner = self.parse_expr(40)?;
            return Ok(Expr::UnaryNeg(Box::new(inner)));
        }

        let primary = self.parse_primary()?;
        self.parse_postfix(primary)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match &self.current().kind {
            TokenKind::Number(value) => {
                let out = Expr::Number(*value);
                self.bump();
                Ok(out)
            }
            TokenKind::StringLit(text) => {
                let out = Expr::StringLiteral(text.clone());
                self.bump();
                Ok(out)
            }
            TokenKind::Ident(name) => {
                let out = Expr::Var(name.clone());
                self.bump();
                Ok(out)
            }
            TokenKind::LParen => {
                self.bump();
                let expr = self.parse_expr(0)?;
                self.expect_symbol(TokenKind::RParen, "')'")?;
                Ok(expr)
            }
            TokenKind::LBracket => self.parse_array_literal(),
            TokenKind::If => self.parse_if_expr(),
            _ => Err(ParseError {
                pos: self.current().pos,
                message: "expected expression".to_string(),
            }),
        }
    }

    fn parse_array_literal(&mut self) -> Result<Expr, ParseError> {
        self.expect_symbol(TokenKind::LBracket, "'['")?;
        let mut elements = Vec::new();
        if self.current().kind != TokenKind::RBracket {
            loop {
                elements.push(self.parse_expr(0)?);
                if self.current().kind == TokenKind::Comma {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.expect_symbol(TokenKind::RBracket, "']'")?;
        Ok(Expr::ArrayLiteral(elements))
    }

    fn parse_if_expr(&mut self) -> Result<Expr, ParseError> {
        self.expect_symbol(TokenKind::If, "'if'")?;
        let cond = self.parse_expr(0)?;
        self.expect_symbol(TokenKind::Then, "'then'")?;
        let then_expr = self.parse_expr(0)?;
        self.expect_symbol(TokenKind::Else, "'else'")?;
        let else_expr = self.parse_expr(0)?;
        Ok(Expr::If {
            cond: Box::new(cond),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
        })
    }

    fn parse_postfix(&mut self, mut expr: Expr) -> Result<Expr, ParseError> {
        loop {
            match self.current().kind {
                TokenKind::LParen => {
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

                    let Expr::Var(callee) = expr else {
                        return Err(ParseError {
                            pos: self.current().pos,
                            message: "call target must be an identifier".to_string(),
                        });
                    };

                    expr = Expr::Call { callee, args };
                }
                TokenKind::LBracket => {
                    self.bump();
                    let index = self.parse_expr(0)?;
                    self.expect_symbol(TokenKind::RBracket, "']'")?;
                    expr = Expr::Index {
                        base: Box::new(expr),
                        index: Box::new(index),
                    };
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn current_binary_op(&self) -> Option<(BinaryOp, u8)> {
        match self.current().kind {
            TokenKind::EqEq => Some((BinaryOp::Eq, 5)),
            TokenKind::Neq => Some((BinaryOp::Ne, 5)),
            TokenKind::Lt => Some((BinaryOp::Lt, 5)),
            TokenKind::Le => Some((BinaryOp::Le, 5)),
            TokenKind::Gt => Some((BinaryOp::Gt, 5)),
            TokenKind::Ge => Some((BinaryOp::Ge, 5)),
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

#[derive(Debug, Clone)]
enum LoweredValue {
    Scalar { ty: Type, value: ValueId },
    Array(Vec<ValueId>),
}

impl LoweredValue {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Scalar { ty, .. } => match ty {
                Type::I8 => "i8",
                Type::I32 => "i32",
                Type::I64 => "i64",
                Type::Ptr => "ptr",
                Type::Void => "void",
            },
            Self::Array(_) => "array",
        }
    }
}

struct CodegenCtx {
    builder: IrBuilder,
    vars: HashMap<String, ValueId>,
    next_block_id: usize,
}

impl CodegenCtx {
    fn new(function: &FunctionAst) -> Self {
        let mut builder = IrBuilder::new(function.name.clone(), Type::I32);
        let mut vars = HashMap::new();

        for param in &function.params {
            let value = builder.add_param(param.clone(), Type::I32);
            vars.insert(param.clone(), value);
        }

        Self {
            builder,
            vars,
            next_block_id: 0,
        }
    }

    fn finish(self) -> Function {
        self.builder.finish()
    }

    fn fresh_block_name(&mut self, prefix: &str) -> String {
        let name = format!("{}_{}", prefix, self.next_block_id);
        self.next_block_id += 1;
        name
    }

    fn expect_i32(
        &self,
        value: LoweredValue,
        context: &'static str,
    ) -> Result<ValueId, CodegenError> {
        match value {
            LoweredValue::Scalar {
                ty: Type::I32,
                value,
            } => Ok(value),
            other => Err(CodegenError::TypeMismatch {
                context,
                expected: "i32",
                found: other.kind_name(),
            }),
        }
    }

    fn expect_i8(
        &self,
        value: LoweredValue,
        context: &'static str,
    ) -> Result<ValueId, CodegenError> {
        match value {
            LoweredValue::Scalar {
                ty: Type::I8,
                value,
            } => Ok(value),
            other => Err(CodegenError::TypeMismatch {
                context,
                expected: "i8",
                found: other.kind_name(),
            }),
        }
    }

    fn expect_array(
        &self,
        value: LoweredValue,
        context: &'static str,
    ) -> Result<Vec<ValueId>, CodegenError> {
        match value {
            LoweredValue::Array(values) => Ok(values),
            _ => Err(match context {
                "len" => CodegenError::InvalidLenOperand,
                _ => CodegenError::InvalidIndexBase,
            }),
        }
    }

    fn codegen_expr(&mut self, expr: &Expr) -> Result<LoweredValue, CodegenError> {
        match expr {
            Expr::Number(value) => {
                let value =
                    i32::try_from(*value).map_err(|_| CodegenError::IntegerOutOfRange(*value))?;
                Ok(LoweredValue::Scalar {
                    ty: Type::I32,
                    value: self.builder.build_const_i32(value)?,
                })
            }
            Expr::StringLiteral(text) => {
                let mut bytes = Vec::new();
                for byte in text.bytes() {
                    bytes.push(self.builder.build_const_i32(i32::from(byte))?);
                }
                Ok(LoweredValue::Array(bytes))
            }
            Expr::ArrayLiteral(elements) => {
                let mut lowered = Vec::new();
                for element in elements {
                    let value = self.codegen_expr(element)?;
                    lowered.push(self.expect_i32(value, "array element")?);
                }
                Ok(LoweredValue::Array(lowered))
            }
            Expr::Var(name) => {
                let value = self
                    .vars
                    .get(name)
                    .copied()
                    .ok_or_else(|| CodegenError::UnknownVariable(name.clone()))?;
                Ok(LoweredValue::Scalar {
                    ty: Type::I32,
                    value,
                })
            }
            Expr::Call { callee, args } => {
                if callee == "len" {
                    if args.len() != 1 {
                        return Err(CodegenError::TypeMismatch {
                            context: "len args",
                            expected: "one argument",
                            found: "different arity",
                        });
                    }
                    let arg0 = self.codegen_expr(&args[0])?;
                    let array = self.expect_array(arg0, "len")?;
                    let len = i32::try_from(array.len())
                        .map_err(|_| CodegenError::IntegerOutOfRange(array.len() as i64))?;
                    return Ok(LoweredValue::Scalar {
                        ty: Type::I32,
                        value: self.builder.build_const_i32(len)?,
                    });
                }

                let mut lowered_args = Vec::new();
                for arg in args {
                    let arg_value = self.codegen_expr(arg)?;
                    let value = self.expect_i32(arg_value, "call argument")?;
                    lowered_args.push((Type::I32, value));
                }
                let result = self
                    .builder
                    .build_call(Type::I32, callee.clone(), lowered_args)?
                    .ok_or_else(|| CodegenError::VoidCallResult(callee.clone()))?;
                Ok(LoweredValue::Scalar {
                    ty: Type::I32,
                    value: result,
                })
            }
            Expr::Index { base, index } => {
                let base_value = self.codegen_expr(base)?;
                let array = self.expect_array(base_value, "index")?;
                let index_value = eval_const_int(index).ok_or(CodegenError::NonConstantIndex)?;
                if index_value < 0 {
                    return Err(CodegenError::IndexOutOfBounds {
                        index: index_value,
                        len: array.len(),
                    });
                }
                let index = index_value as usize;
                if index >= array.len() {
                    return Err(CodegenError::IndexOutOfBounds {
                        index: index_value,
                        len: array.len(),
                    });
                }
                Ok(LoweredValue::Scalar {
                    ty: Type::I32,
                    value: array[index],
                })
            }
            Expr::If {
                cond,
                then_expr,
                else_expr,
            } => {
                let cond_value = {
                    let cond_lowered = self.codegen_expr(cond)?;
                    self.expect_i8(cond_lowered, "if condition")?
                };
                let then_name = self.fresh_block_name("if_then");
                let else_name = self.fresh_block_name("if_else");
                let merge_name = self.fresh_block_name("if_merge");
                let then_block = self.builder.create_block(then_name);
                let else_block = self.builder.create_block(else_name);
                let merge_block = self.builder.create_block(merge_name);

                self.builder.build_br(cond_value, then_block, else_block)?;

                self.builder.position_at_end(then_block)?;
                let then_lowered = self.codegen_expr(then_expr)?;
                let LoweredValue::Scalar {
                    ty: then_ty,
                    value: then_value,
                } = then_lowered
                else {
                    return Err(CodegenError::TypeMismatch {
                        context: "if then branch",
                        expected: "scalar",
                        found: "array",
                    });
                };
                let then_exit = self
                    .builder
                    .current_block()
                    .ok_or(CodegenError::Ir(IrBuildError::MissingCurrentBlock))?;
                self.builder.build_jmp(merge_block)?;

                self.builder.position_at_end(else_block)?;
                let else_lowered = self.codegen_expr(else_expr)?;
                let LoweredValue::Scalar {
                    ty: else_ty,
                    value: else_value,
                } = else_lowered
                else {
                    return Err(CodegenError::TypeMismatch {
                        context: "if else branch",
                        expected: "scalar",
                        found: "array",
                    });
                };
                let else_exit = self
                    .builder
                    .current_block()
                    .ok_or(CodegenError::Ir(IrBuildError::MissingCurrentBlock))?;
                self.builder.build_jmp(merge_block)?;

                if then_ty != else_ty {
                    return Err(CodegenError::TypeMismatch {
                        context: "if branch type",
                        expected: lowered_type_name(then_ty),
                        found: lowered_type_name(else_ty),
                    });
                }

                self.builder.position_at_end(merge_block)?;
                let phi = self.builder.build_phi(
                    then_ty,
                    vec![
                        PhiIncoming {
                            value: then_value,
                            block: then_exit,
                        },
                        PhiIncoming {
                            value: else_value,
                            block: else_exit,
                        },
                    ],
                )?;
                Ok(LoweredValue::Scalar {
                    ty: then_ty,
                    value: phi,
                })
            }
            Expr::UnaryNeg(inner) => {
                let zero = self.builder.build_const_i32(0)?;
                let inner_value = {
                    let lowered = self.codegen_expr(inner)?;
                    self.expect_i32(lowered, "unary neg")?
                };
                let value = self.builder.build_sub(zero, inner_value)?;
                Ok(LoweredValue::Scalar {
                    ty: Type::I32,
                    value,
                })
            }
            Expr::Binary { op, lhs, rhs } => {
                let lhs_lowered = self.codegen_expr(lhs)?;
                let rhs_lowered = self.codegen_expr(rhs)?;

                match op {
                    BinaryOp::Add => {
                        let lhs = self.expect_i32(lhs_lowered, "add lhs")?;
                        let rhs = self.expect_i32(rhs_lowered, "add rhs")?;
                        Ok(LoweredValue::Scalar {
                            ty: Type::I32,
                            value: self.builder.build_add(lhs, rhs)?,
                        })
                    }
                    BinaryOp::Sub => {
                        let lhs = self.expect_i32(lhs_lowered, "sub lhs")?;
                        let rhs = self.expect_i32(rhs_lowered, "sub rhs")?;
                        Ok(LoweredValue::Scalar {
                            ty: Type::I32,
                            value: self.builder.build_sub(lhs, rhs)?,
                        })
                    }
                    BinaryOp::Mul => {
                        let lhs = self.expect_i32(lhs_lowered, "mul lhs")?;
                        let rhs = self.expect_i32(rhs_lowered, "mul rhs")?;
                        Ok(LoweredValue::Scalar {
                            ty: Type::I32,
                            value: self.builder.build_mul(lhs, rhs)?,
                        })
                    }
                    BinaryOp::Div => {
                        let lhs = self.expect_i32(lhs_lowered, "div lhs")?;
                        let rhs = self.expect_i32(rhs_lowered, "div rhs")?;
                        Ok(LoweredValue::Scalar {
                            ty: Type::I32,
                            value: self.builder.build_sdiv(lhs, rhs)?,
                        })
                    }
                    BinaryOp::And => {
                        let lhs = self.expect_i32(lhs_lowered, "and lhs")?;
                        let rhs = self.expect_i32(rhs_lowered, "and rhs")?;
                        Ok(LoweredValue::Scalar {
                            ty: Type::I32,
                            value: self.builder.build_and(lhs, rhs)?,
                        })
                    }
                    BinaryOp::Eq
                    | BinaryOp::Ne
                    | BinaryOp::Lt
                    | BinaryOp::Le
                    | BinaryOp::Gt
                    | BinaryOp::Ge => {
                        let lhs = self.expect_i32(lhs_lowered, "cmp lhs")?;
                        let rhs = self.expect_i32(rhs_lowered, "cmp rhs")?;
                        let pred = match op {
                            BinaryOp::Eq => IcmpPredicate::Eq,
                            BinaryOp::Ne => IcmpPredicate::Ne,
                            BinaryOp::Lt => IcmpPredicate::Slt,
                            BinaryOp::Le => IcmpPredicate::Sle,
                            BinaryOp::Gt => IcmpPredicate::Sgt,
                            BinaryOp::Ge => IcmpPredicate::Sge,
                            _ => unreachable!(),
                        };
                        Ok(LoweredValue::Scalar {
                            ty: Type::I8,
                            value: self.builder.build_icmp(pred, Type::I32, lhs, rhs)?,
                        })
                    }
                }
            }
        }
    }
}

fn lowered_type_name(ty: Type) -> &'static str {
    match ty {
        Type::I8 => "i8",
        Type::I32 => "i32",
        Type::I64 => "i64",
        Type::Ptr => "ptr",
        Type::Void => "void",
    }
}

fn codegen_function(function: &FunctionAst) -> Result<Function, CodegenError> {
    let mut ctx = CodegenCtx::new(function);
    let entry = ctx.builder.create_block("entry");
    ctx.builder.position_at_end(entry)?;

    let body = ctx.codegen_expr(&function.body)?;
    let result = ctx.expect_i32(body, "function result")?;
    ctx.builder.build_ret(Some(result))?;

    Ok(ctx.finish())
}

fn eval_const_int(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Number(value) => Some(*value),
        Expr::UnaryNeg(inner) => Some(-eval_const_int(inner)?),
        Expr::Binary { op, lhs, rhs } => {
            let lhs = eval_const_int(lhs)?;
            let rhs = eval_const_int(rhs)?;
            match op {
                BinaryOp::Add => Some(lhs + rhs),
                BinaryOp::Sub => Some(lhs - rhs),
                BinaryOp::Mul => Some(lhs * rhs),
                BinaryOp::Div => {
                    if rhs == 0 {
                        None
                    } else {
                        Some(lhs / rhs)
                    }
                }
                BinaryOp::And => Some(lhs & rhs),
                BinaryOp::Eq => Some(i64::from(lhs == rhs)),
                BinaryOp::Ne => Some(i64::from(lhs != rhs)),
                BinaryOp::Lt => Some(i64::from(lhs < rhs)),
                BinaryOp::Le => Some(i64::from(lhs <= rhs)),
                BinaryOp::Gt => Some(i64::from(lhs > rhs)),
                BinaryOp::Ge => Some(i64::from(lhs >= rhs)),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_strings_arrays_and_if_tokens() {
        let tokens =
            lex("fn main() = if 1 <= 2 then [1,2][0] else \"x\"[0];").expect("lex should succeed");
        assert!(tokens.iter().any(|tok| matches!(tok.kind, TokenKind::If)));
        assert!(tokens.iter().any(|tok| matches!(tok.kind, TokenKind::Le)));
        assert!(
            tokens
                .iter()
                .any(|tok| matches!(tok.kind, TokenKind::StringLit(_)))
        );
        assert!(
            tokens
                .iter()
                .any(|tok| matches!(tok.kind, TokenKind::LBracket))
        );
    }

    #[test]
    fn parses_recursive_fib_shape() {
        let source = "fn fib(n) = if n <= 1 then n else fib(n - 1) + fib(n - 2);";
        let program = parse_source(source).expect("parse should succeed");
        assert_eq!(program.functions.len(), 1);
        assert_eq!(program.functions[0].name, "fib");
        assert_eq!(program.functions[0].params, vec!["n".to_string()]);
    }

    #[test]
    fn codegen_supports_arrays_strings_and_calls() {
        let src = r#"
            fn id(x) = x;
            fn main() = if 3 > 2 then id([10,20,30][1] + len("abc")) else 0;
        "#;
        let functions = compile_source_to_ir(src).expect("compile should succeed");
        assert_eq!(functions.len(), 2);
        assert!(functions.iter().any(|func| func.name == "main"));
        assert!(functions.iter().any(|func| func.name == "id"));
    }
}
