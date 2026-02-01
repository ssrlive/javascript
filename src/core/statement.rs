use crate::core::TemplatePart;
use crate::core::{Collect, GcTrace};

#[derive(Clone, Debug)]
pub struct Statement {
    pub kind: Box<StatementKind>,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug)]
pub enum StatementKind {
    Expr(Expr),
    Let(Vec<(String, Option<Expr>)>),
    Var(Vec<(String, Option<Expr>)>),
    Const(Vec<(String, Expr)>),
    Return(Option<Expr>),
    Throw(Expr),           // throw expression
    Block(Vec<Statement>), // block statement `{ ... }`
    If(Box<IfStatement>),
    FunctionDeclaration(String, Vec<DestructuringElement>, Vec<Statement>, bool, bool), // name, params, body, is_generator, is_async
    TryCatch(Box<TryCatchStatement>),
    LetDestructuringArray(Vec<DestructuringElement>, Expr), // array destructuring: let [a, b] = [1, 2];
    VarDestructuringArray(Vec<DestructuringElement>, Expr), // array destructuring: var [a, b] = [1, 2];
    ConstDestructuringArray(Vec<DestructuringElement>, Expr), // const [a, b] = [1, 2];
    LetDestructuringObject(Vec<ObjectDestructuringElement>, Expr), // object destructuring: let {a, b} = {a: 1, b: 2};
    VarDestructuringObject(Vec<ObjectDestructuringElement>, Expr), // object destructuring: var {a, b} = {a: 1, b: 2};
    ConstDestructuringObject(Vec<ObjectDestructuringElement>, Expr), // const {a, b} = {a: 1, b: 2};
    Class(Box<ClassDefinition>),                            // name, extends, members
    Assign(String, Expr),                                   // variable assignment
    For(Box<ForStatement>),
    ForOf(Option<VarDeclKind>, String, Expr, Vec<Statement>), // decl kind, variable, iterable, body
    ForOfExpr(Expr, Expr, Vec<Statement>),                    // assignment-form for-of with expression LHS, iterable, body
    ForIn(Option<VarDeclKind>, String, Expr, Vec<Statement>), // decl kind (None = declaration), variable, object, body
    ForInExpr(Expr, Expr, Vec<Statement>),                    // assignment-form for-in with expression LHS, iterable, body
    ForInDestructuringObject(Option<VarDeclKind>, Vec<ObjectDestructuringElement>, Expr, Vec<Statement>), // decl kind, var { .. } in object
    ForInDestructuringArray(Option<VarDeclKind>, Vec<DestructuringElement>, Expr, Vec<Statement>), // decl kind, var [ .. ] in object
    ForOfDestructuringObject(Option<VarDeclKind>, Vec<ObjectDestructuringElement>, Expr, Vec<Statement>), // decl kind, var { .. } of iterable
    ForOfDestructuringArray(Option<VarDeclKind>, Vec<DestructuringElement>, Expr, Vec<Statement>), // decl kind, var [ .. ] of iterable
    While(Expr, Vec<Statement>),                                                                   // condition, body
    DoWhile(Vec<Statement>, Expr),                                                                 // body, condition
    Switch(Box<SwitchStatement>),
    With(Box<Expr>, Vec<Statement>), // with (expr) body
    Break(Option<String>),
    Continue(Option<String>),
    Debugger,
    Label(String, Box<Statement>),
    Import(Vec<ImportSpecifier>, String),                 // import specifiers, module name
    Export(Vec<ExportSpecifier>, Option<Box<Statement>>), // export specifiers, optional inner declaration
}

#[derive(Clone, Copy, Debug)]
pub enum VarDeclKind {
    Var,
    Let,
    Const,
}

#[derive(Clone, Debug)]
pub struct IfStatement {
    pub condition: Expr,
    pub then_body: Vec<Statement>,
    pub else_body: Option<Vec<Statement>>,
}

#[derive(Clone, Debug)]
pub struct TryCatchStatement {
    pub try_body: Vec<Statement>,
    pub catch_param: Option<String>,
    pub catch_body: Option<Vec<Statement>>,
    pub finally_body: Option<Vec<Statement>>,
}

#[derive(Clone, Debug)]
pub struct ForStatement {
    pub init: Option<Box<Statement>>,
    pub test: Option<Expr>,
    pub update: Option<Box<Statement>>,
    pub body: Vec<Statement>,
}

#[derive(Clone, Debug)]
pub struct SwitchStatement {
    pub expr: Expr,
    pub cases: Vec<SwitchCase>,
}

impl From<StatementKind> for Statement {
    fn from(kind: StatementKind) -> Self {
        Statement {
            kind: Box::new(kind),
            line: 0,
            column: 0,
        }
    }
}

unsafe impl<'gc> Collect<'gc> for Statement {
    fn trace<T: GcTrace<'gc>>(&self, cc: &mut T) {
        crate::core::gc::trace_stmt(cc, self);
    }
}

unsafe impl<'gc> Collect<'gc> for StatementKind {
    fn trace<T: GcTrace<'gc>>(&self, _cc: &mut T) {
        // Handled via Statement trace
    }
}

#[derive(Clone, Debug)]
pub enum Expr {
    Number(f64),
    StringLit(Vec<u16>),
    Boolean(bool),
    Null,
    Undefined,
    Var(String, Option<usize>, Option<usize>),
    Assign(Box<Expr>, Box<Expr>),
    Binary(Box<Expr>, BinaryOp, Box<Expr>),
    LogicalAnd(Box<Expr>, Box<Expr>),
    LogicalOr(Box<Expr>, Box<Expr>),
    NullishCoalescing(Box<Expr>, Box<Expr>),
    Mod(Box<Expr>, Box<Expr>),
    Pow(Box<Expr>, Box<Expr>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    LogicalAndAssign(Box<Expr>, Box<Expr>),
    LogicalOrAssign(Box<Expr>, Box<Expr>),
    NullishAssign(Box<Expr>, Box<Expr>),
    AddAssign(Box<Expr>, Box<Expr>),
    SubAssign(Box<Expr>, Box<Expr>),
    PowAssign(Box<Expr>, Box<Expr>),
    MulAssign(Box<Expr>, Box<Expr>),
    DivAssign(Box<Expr>, Box<Expr>),
    ModAssign(Box<Expr>, Box<Expr>),
    BitXorAssign(Box<Expr>, Box<Expr>),
    BitAndAssign(Box<Expr>, Box<Expr>),
    BitOrAssign(Box<Expr>, Box<Expr>),
    LeftShiftAssign(Box<Expr>, Box<Expr>),
    RightShiftAssign(Box<Expr>, Box<Expr>),
    UnsignedRightShiftAssign(Box<Expr>, Box<Expr>),
    OptionalProperty(Box<Expr>, String),
    OptionalIndex(Box<Expr>, Box<Expr>),
    OptionalCall(Box<Expr>, Vec<Expr>),
    Property(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    BigInt(Vec<u16>),
    TypeOf(Box<Expr>),
    Delete(Box<Expr>),
    Void(Box<Expr>),
    Await(Box<Expr>),
    Yield(Option<Box<Expr>>),
    YieldStar(Box<Expr>),
    LogicalNot(Box<Expr>),
    Class(Box<ClassDefinition>),
    New(Box<Expr>, Vec<Expr>),
    UnaryNeg(Box<Expr>),
    UnaryPlus(Box<Expr>),
    BitNot(Box<Expr>),
    Increment(Box<Expr>),
    Decrement(Box<Expr>),
    Spread(Box<Expr>),
    ArrowFunction(Vec<DestructuringElement>, Vec<Statement>),
    This,
    NewTarget,
    SuperCall(Vec<Expr>),
    SuperMethod(String, Vec<Expr>),
    SuperProperty(String),
    Super,
    Object(Vec<(Expr, Expr, bool)>),
    Getter(Box<Expr>),
    Setter(Box<Expr>),
    Array(Vec<Option<Expr>>),
    GeneratorFunction(Option<String>, Vec<DestructuringElement>, Vec<Statement>),
    AsyncFunction(Option<String>, Vec<DestructuringElement>, Vec<Statement>),
    AsyncGeneratorFunction(Option<String>, Vec<DestructuringElement>, Vec<Statement>),
    AsyncArrowFunction(Vec<DestructuringElement>, Vec<Statement>),
    PostIncrement(Box<Expr>),
    PostDecrement(Box<Expr>),
    TaggedTemplate(Box<Expr>, Vec<Vec<u16>>, Vec<Expr>),
    TemplateString(Vec<TemplatePart>),
    Regex(String, String),
    Comma(Box<Expr>, Box<Expr>),
    Function(Option<String>, Vec<DestructuringElement>, Vec<Statement>),
    Call(Box<Expr>, Vec<Expr>),
    DynamicImport(Box<Expr>),
    ValuePlaceholder,
}

unsafe impl<'gc> Collect<'gc> for Expr {
    fn trace<T: GcTrace<'gc>>(&self, cc: &mut T) {
        crate::core::gc::trace_expr(cc, self);
    }
}

#[derive(Clone, Copy, Debug)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    LeftShift,
    RightShift,
    UnsignedRightShift,
    LessThan,
    GreaterThan,
    LessEqual,
    GreaterEqual,
    InstanceOf,
    In,
    Equal,
    StrictEqual,
    NotEqual,
    StrictNotEqual,
    BitAnd,
    BitXor,
    BitOr,
    NullishCoalescing,
    Mod,
    Pow,
}

unsafe impl<'gc> Collect<'gc> for BinaryOp {
    fn trace<T: GcTrace<'gc>>(&self, _cc: &mut T) {}
}

#[derive(Debug, Clone)]
pub enum ObjectDestructuringElement {
    Property { key: String, value: DestructuringElement }, // a: b or a
    Rest(String),                                          // ...rest
}

#[derive(Debug, Clone, Collect)]
#[collect(require_static)]
pub enum ClassMember {
    Constructor(Vec<DestructuringElement>, Vec<Statement>),             // parameters, body
    Method(String, Vec<DestructuringElement>, Vec<Statement>),          // name, parameters, body
    MethodGenerator(String, Vec<DestructuringElement>, Vec<Statement>), // name, parameters, body (generator)
    MethodAsync(String, Vec<DestructuringElement>, Vec<Statement>),     // async method
    MethodAsyncGenerator(String, Vec<DestructuringElement>, Vec<Statement>), // async generator method
    StaticMethod(String, Vec<DestructuringElement>, Vec<Statement>),    // name, parameters, body
    StaticMethodGenerator(String, Vec<DestructuringElement>, Vec<Statement>), // name, parameters, body (generator)
    StaticMethodAsync(String, Vec<DestructuringElement>, Vec<Statement>), // static async method
    StaticMethodAsyncGenerator(String, Vec<DestructuringElement>, Vec<Statement>), // static async generator method
    MethodComputed(Expr, Vec<DestructuringElement>, Vec<Statement>),    // computed-key, parameters, body
    MethodComputedGenerator(Expr, Vec<DestructuringElement>, Vec<Statement>), // computed-key, generator
    MethodComputedAsync(Expr, Vec<DestructuringElement>, Vec<Statement>), // computed-key, async
    MethodComputedAsyncGenerator(Expr, Vec<DestructuringElement>, Vec<Statement>), // computed-key, async generator
    StaticMethodComputed(Expr, Vec<DestructuringElement>, Vec<Statement>), // computed-key, parameters, body
    StaticMethodComputedGenerator(Expr, Vec<DestructuringElement>, Vec<Statement>), // computed-key, generator
    StaticMethodComputedAsync(Expr, Vec<DestructuringElement>, Vec<Statement>), // computed-key, async
    StaticMethodComputedAsyncGenerator(Expr, Vec<DestructuringElement>, Vec<Statement>), // computed-key, async generator
    Property(String, Expr),                                             // name, value
    StaticProperty(String, Expr),                                       // name, value
    PropertyComputed(Expr, Expr),                                       // computed-key, value
    StaticPropertyComputed(Expr, Expr),                                 // computed-key, value
    PrivateProperty(String, Expr),                                      // name, value
    PrivateStaticProperty(String, Expr),                                // name, value
    PrivateMethod(String, Vec<DestructuringElement>, Vec<Statement>),   // name, parameters, body
    PrivateMethodAsyncGenerator(String, Vec<DestructuringElement>, Vec<Statement>), // private async generator method
    PrivateStaticMethodAsyncGenerator(String, Vec<DestructuringElement>, Vec<Statement>), // private static async generator method
    PrivateStaticMethod(String, Vec<DestructuringElement>, Vec<Statement>), // name, parameters, body
    PrivateGetter(String, Vec<Statement>),                              // name, body
    PrivateSetter(String, Vec<DestructuringElement>, Vec<Statement>),   // name, parameter, body
    PrivateStaticGetter(String, Vec<Statement>),                        // name, body
    PrivateStaticSetter(String, Vec<DestructuringElement>, Vec<Statement>), // name, parameter, body
    StaticBlock(Vec<Statement>),                                        // body
    Getter(String, Vec<Statement>),                                     // name, body
    Setter(String, Vec<DestructuringElement>, Vec<Statement>),          // name, parameter, body
    GetterComputed(Expr, Vec<Statement>),                               // computed-key, body
    SetterComputed(Expr, Vec<DestructuringElement>, Vec<Statement>),    // computed-key, parameter, body
    StaticGetter(String, Vec<Statement>),                               // name, body
    StaticSetter(String, Vec<DestructuringElement>, Vec<Statement>),    // name, parameter, body
    StaticGetterComputed(Expr, Vec<Statement>),                         // computed-key, body
    StaticSetterComputed(Expr, Vec<DestructuringElement>, Vec<Statement>), // computed-key, parameter, body
}

#[derive(Debug, Clone, Collect)]
#[collect(require_static)]
pub struct ClassDefinition {
    pub name: String,
    pub extends: Option<Expr>,
    pub members: Vec<ClassMember>,
}

#[derive(Clone, Debug)]
pub enum SwitchCase {
    Case(Expr, Vec<Statement>), // case value, statements
    Default(Vec<Statement>),    // default statements
}

#[derive(Clone, Debug)]
pub enum ImportSpecifier {
    Default(String),               // import name from "module"
    Named(String, Option<String>), // import { name as alias } from "module"
    Namespace(String),             // import * as name from "module"
}

#[derive(Clone, Debug)]
pub enum ExportSpecifier {
    Named(String, Option<String>), // export { name as alias }
    Default(Expr),                 // export default value
}

#[derive(Clone, Debug)]
pub enum DestructuringElement {
    Variable(String, Option<Box<Expr>>),
    Property(String, Box<DestructuringElement>),
    ComputedProperty(Expr, Box<DestructuringElement>),
    Rest(String),
    RestPattern(Box<DestructuringElement>),
    Empty,
    NestedArray(Vec<DestructuringElement>, Option<Box<Expr>>),
    NestedObject(Vec<DestructuringElement>, Option<Box<Expr>>),
}

unsafe impl<'gc> Collect<'gc> for DestructuringElement {
    fn trace<T: GcTrace<'gc>>(&self, cc: &mut T) {
        match self {
            DestructuringElement::Variable(_, e) => {
                if let Some(e) = e {
                    e.trace(cc);
                }
            }
            DestructuringElement::Property(_, elem) => {
                elem.trace(cc);
            }
            DestructuringElement::ComputedProperty(expr, elem) => {
                expr.trace(cc);
                elem.trace(cc);
            }
            DestructuringElement::Rest(_) => {}
            DestructuringElement::RestPattern(elem) => {
                elem.trace(cc);
            }
            DestructuringElement::Empty => {}
            DestructuringElement::NestedArray(arr, default_expr) => {
                for elem in arr {
                    elem.trace(cc);
                }
                if let Some(d) = default_expr {
                    d.trace(cc);
                }
            }
            DestructuringElement::NestedObject(obj, default_expr) => {
                for elem in obj {
                    elem.trace(cc);
                }
                if let Some(d) = default_expr {
                    d.trace(cc);
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum ForOfPattern {
    Object(Vec<DestructuringElement>),
    Array(Vec<DestructuringElement>),
}
