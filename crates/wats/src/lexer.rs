use logos::{Logos, Lexer};

/// Handles WIT-style nested block comments: /* /* nested */ */
fn nested_comments(lex: &mut Lexer<Token>) -> bool {
    let mut depth = 1;
    let bytes = lex.remainder().as_bytes();
    let mut i = 0;
    while depth > 0 && i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            depth += 1;
            i += 2;
        } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            depth -= 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    lex.bump(i);
    depth == 0
}

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(skip r"[ \t\n\f\r]+")] // Skip whitespace and carriage returns
pub enum Token {

    // --- Keywords ---
    // Note: Keywords must come BEFORE the Identifier regex
    #[token("func")] KwFunc,
    #[token("let")] KwLet,
    #[token("if")] KwIf,
    #[token("else")] KwElse,
    #[token("return")] KwReturn,
    #[token("export")] KwExport,
    #[token("import")] KwImport,
    #[token("world")] KwWorld,
    #[token("interface")] KwInterface,



    // --- Symbols & Operators ---
    #[token("=")] Equals,
    #[token(":")] Colon,
    #[token(";")] Semicolon,
    #[token(",")] Comma,
    #[token(".")] Dot,
    #[token("(")] LParen,
    #[token(")")] RParen,
    #[token("{")] LBrace,
    #[token("}")] RBrace,
    #[token("->")] Arrow,
    #[token("<")] Less,
    #[token(">")] Greater,
    #[token("+")] Plus,
    #[token("-")] Minus,
    #[token("*")] Mul,
    #[token("/")] Div,

    
    
    // --- Built-in WIT Types ---
    #[token("u32")] TypeU32,
    #[token("u64")] TypeU64,
    #[token("i32")] TypeI32,
    #[token("i64")] TypeI64,
    #[token("f32")] TypeF32,
    #[token("f64")] TypeF64,
    #[token("string")] TypeString,
    #[token("bool")] TypeBool,

    // --- Literals & Identifiers ---
    
    // Matches standard idents and kebab-case: move-lift, crane-1
    // The regex ensures it starts with a letter.
   #[regex(r"[a-zA-Z_][a-zA-Z0-9_-]*", |lex| lex.slice().to_string())]
    Identifier(String),

    #[regex(r"[0-9]+", |lex| lex.slice().parse::<u64>().ok())]
    Number(u64),

    // --- Comments ---
    #[token("/*", nested_comments)]
    #[regex(r"//.*", logos::skip)]
    _Comment,
}