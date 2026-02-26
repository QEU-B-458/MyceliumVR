#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    U32,
    U64,
    F32,
    String,
    Bool,
    Result(Box<Type>, Box<Type>),
    Custom(String),
    Unit,
}

#[derive(Debug)]
pub enum Expression {
    Binary { left: Box<Expression>, op: String, right: Box<Expression> },
    Number(u64),
    Identifier(String),
    Call { name: String, args: Vec<Expression> },
}

#[derive(Debug)]
pub enum Statement {
    Let { name: String, ty: Type, value: Expression },
    Return(Expression),
    If { condition: Expression, then_block: Vec<Statement>, else_block: Option<Vec<Statement>> },
}

#[derive(Debug)]
pub struct Param {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>, // Change this from vec![]
    pub ret_type: Type,
    pub body: Vec<Statement>,
}