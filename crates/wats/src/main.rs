mod ast;
mod lexer;
mod parser;

use crate::lexer::Token;
use logos::Logos;

fn main() {
    let code = r#"
    func move-lift(target: u32) -> u32 {
        let height: u32 = 100;
        return height;
    }
    "#;

    // 1. Lex the code into a Vec
    let tokens: Vec<Token> = Token::lexer(code).map(|res| res.unwrap()).collect();

    // 2. Pass the slice of tokens to the nom parser
    match parser::parse_function(&tokens) {
        Ok((remainder, ast)) => println!("{:#?}", ast),
        Err(e) => eprintln!("Nom Error: {:?}", e),
    }
}
