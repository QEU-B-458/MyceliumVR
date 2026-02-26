use nom::{
    IResult,
    error::{Error, ErrorKind},
    multi::{many0, separated_list0},
    sequence::{delimited, tuple, preceded},
    combinator::{map, opt},
};
use crate::lexer::Token;
use crate::ast::*;

// A custom type alias for our token-stream input
type TokenInput<'a> = &'a [Token];

/// A helper to match a specific token variant
fn token<'a>(expected: Token) -> impl Fn(TokenInput<'a>) -> IResult<TokenInput<'a>, Token> {
    move |input: TokenInput<'a>| {
        if !input.is_empty() && input[0] == expected {
            Ok((&input[1..], input[0].clone()))
        } else {
            Err(nom::Err::Error(Error::new(input, ErrorKind::Tag)))
        }
    }
}

/// A helper to extract an Identifier string
fn identifier(input: TokenInput) -> IResult<TokenInput, String> {
    if let Some(Token::Identifier(name)) = input.get(0) {
        Ok((&input[1..], name.clone()))
    } else {
        Err(nom::Err::Error(Error::new(input, ErrorKind::Tag)))
    }
}

/// A helper to extract a Number literal
fn number(input: TokenInput) -> IResult<TokenInput, u64> {
    if let Some(Token::Number(n)) = input.get(0) {
        Ok((&input[1..], *n))
    } else {
        Err(nom::Err::Error(Error::new(input, ErrorKind::Tag)))
    }
}

// --- High Level Parsers ---

pub fn parse_type(input: TokenInput) -> IResult<TokenInput, Type> {
    if let Some(t) = input.get(0) {
        let res = match t {
            Token::TypeU32 => Some(Type::U32),
            Token::TypeString => Some(Type::String),
            Token::Identifier(s) => Some(Type::Custom(s.clone())),
            _ => None,
        };
        if let Some(ty) = res {
            return Ok((&input[1..], ty));
        }
    }
    Err(nom::Err::Error(Error::new(input, ErrorKind::Alt)))
}

pub fn parse_function(input: TokenInput) -> IResult<TokenInput, Function> {
    let (input, _) = token(Token::KwFunc)(input)?;
    let (input, name) = identifier(input)?;
    
    // Parse params: (name: type, name: type)
    let (input, params) = delimited(
        token(Token::LParen),
        separated_list0(token(Token::Comma), parse_param),
        token(Token::RParen)
    )(input)?;

    let (input, _) = token(Token::Arrow)(input)?;
    let (input, ret_type) = parse_type(input)?;
    
    // Parse body: { statements }
    let (input, body) = delimited(
        token(Token::LBrace),
        many0(parse_statement),
        token(Token::RBrace)
    )(input)?;

    Ok((input, Function { name, params, ret_type, body }))
}

fn parse_param(input: TokenInput) -> IResult<TokenInput, Param> {
    let (input, name) = identifier(input)?;
    let (input, _) = token(Token::Colon)(input)?;
    let (input, ty) = parse_type(input)?;
    Ok((input, Param { name, ty }))
}

fn parse_statement(input: TokenInput) -> IResult<TokenInput, Statement> {
    // Try parsing 'let'
    let parse_let = map(
        tuple((
            token(Token::KwLet),
            identifier,
            token(Token::Colon),
            parse_type,
            token(Token::Equals),
            parse_expression,
            token(Token::Semicolon)
        )),
        |(_, name, _, ty, _, value, _)| Statement::Let { name, ty, value }
    );

    // Try parsing 'return'
    let parse_return = map(
        tuple((token(Token::KwReturn), parse_expression, token(Token::Semicolon))),
        |(_, expr, _)| Statement::Return(expr)
    );

    // You can use nom's alt() to choose between them
    nom::branch::alt((parse_let, parse_return))(input)
}

fn parse_expression(input: TokenInput) -> IResult<TokenInput, Expression> {
    // For now, simple primary expression parsing
    let parse_num = map(number, Expression::Number);
    let parse_id = map(identifier, Expression::Identifier);
    
    nom::branch::alt((parse_num, parse_id))(input)
}