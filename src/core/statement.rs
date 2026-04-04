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
    ForAwaitOf(Option<VarDeclKind>, String, Expr, Vec<Statement>), // async for-await-of
    ForAwaitOfExpr(Expr, Expr, Vec<Statement>),               // assignment-form for-await-of with expression LHS
    ForIn(Option<VarDeclKind>, String, Expr, Vec<Statement>), // decl kind (None = declaration), variable, object, body
    ForInExpr(Expr, Expr, Vec<Statement>),                    // assignment-form for-in with expression LHS, iterable, body
    ForInDestructuringObject(Option<VarDeclKind>, Vec<ObjectDestructuringElement>, Expr, Vec<Statement>), // decl kind, var { .. } in object
    ForInDestructuringArray(Option<VarDeclKind>, Vec<DestructuringElement>, Expr, Vec<Statement>), // decl kind, var [ .. ] in object
    ForOfDestructuringObject(Option<VarDeclKind>, Vec<ObjectDestructuringElement>, Expr, Vec<Statement>), // decl kind, var { .. } of iterable
    ForOfDestructuringArray(Option<VarDeclKind>, Vec<DestructuringElement>, Expr, Vec<Statement>), // decl kind, var [ .. ] of iterable
    ForAwaitOfDestructuringObject(Option<VarDeclKind>, Vec<ObjectDestructuringElement>, Expr, Vec<Statement>), // async for-await-of {..}
    ForAwaitOfDestructuringArray(Option<VarDeclKind>, Vec<DestructuringElement>, Expr, Vec<Statement>), // async for-await-of [..]
    While(Expr, Vec<Statement>),                                                                   // condition, body
    DoWhile(Vec<Statement>, Expr),                                                                 // body, condition
    Switch(Box<SwitchStatement>),
    With(Box<Expr>, Vec<Statement>), // with (expr) body
    Break(Option<String>),
    Continue(Option<String>),
    Debugger,
    Label(String, Box<Statement>),
    Import(Vec<ImportSpecifier>, String), // import specifiers, module name
    Export(Vec<ExportSpecifier>, Option<Box<Statement>>, Option<String>), // export specifiers, optional inner declaration, optional source
    Using(Vec<(String, Expr)>),           // using declarations: using x = expr, y = expr;
    AwaitUsing(Vec<(String, Expr)>),      // await using declarations: await using x = expr;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VarDeclKind {
    Var,
    Let,
    Const,
    Using,
    AwaitUsing,
}

unsafe impl<'gc> Collect<'gc> for VarDeclKind {
    fn trace<T: GcTrace<'gc>>(&self, _cc: &mut T) {}
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
    pub catch_param: Option<CatchParamPattern>,
    pub catch_body: Option<Vec<Statement>>,
    pub finally_body: Option<Vec<Statement>>,
}

#[derive(Clone, Debug)]
pub enum CatchParamPattern {
    Identifier(String),
    Array(Vec<DestructuringElement>),
    Object(Vec<DestructuringElement>),
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
    OptionalPrivateMember(Box<Expr>, String),
    OptionalIndex(Box<Expr>, Box<Expr>),
    OptionalCall(Box<Expr>, Vec<Expr>),
    Property(Box<Expr>, String),
    PrivateName(String),
    PrivateMember(Box<Expr>, String),
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
    SuperComputedProperty(Box<Expr>),
    SuperComputedMethod(Box<Expr>, Vec<Expr>),
    Super,
    Object(Vec<(Expr, Expr, bool, bool)>),
    Getter(Box<Expr>),
    Setter(Box<Expr>),
    Array(Vec<Option<Expr>>),
    GeneratorFunction(Option<String>, Vec<DestructuringElement>, Vec<Statement>),
    AsyncFunction(Option<String>, Vec<DestructuringElement>, Vec<Statement>),
    AsyncGeneratorFunction(Option<String>, Vec<DestructuringElement>, Vec<Statement>),
    AsyncArrowFunction(Vec<DestructuringElement>, Vec<Statement>),
    PostIncrement(Box<Expr>),
    PostDecrement(Box<Expr>),
    // Tagged template literal call.
    // `site_id` is a stable per-parse unique id used for GetTemplateObject caching.
    // `cooked` entries are None when the template contains an invalid escape sequence.
    TaggedTemplate(Box<Expr>, u64, Vec<Option<Vec<u16>>>, Vec<Vec<u16>>, Vec<Expr>),
    TemplateString(Vec<TemplatePart>),
    Regex(String, String),
    Comma(Box<Expr>, Box<Expr>),
    Function(Option<String>, Vec<DestructuringElement>, Vec<Statement>),
    Call(Box<Expr>, Vec<Expr>),
    DynamicImport(Box<Expr>, Option<Box<Expr>>),
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
    Property { key: String, value: DestructuringElement },       // a: b or a
    ComputedProperty { key: Expr, value: DestructuringElement }, // [expr]: val
    Rest(String),                                                // ...rest
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
    PrivateMethodAsync(String, Vec<DestructuringElement>, Vec<Statement>), // private async method
    PrivateMethodGenerator(String, Vec<DestructuringElement>, Vec<Statement>), // name, parameters, body
    PrivateMethodAsyncGenerator(String, Vec<DestructuringElement>, Vec<Statement>), // private async generator method
    PrivateStaticMethodAsyncGenerator(String, Vec<DestructuringElement>, Vec<Statement>), // private static async generator method
    PrivateStaticMethodGenerator(String, Vec<DestructuringElement>, Vec<Statement>), // private static generator method
    PrivateStaticMethod(String, Vec<DestructuringElement>, Vec<Statement>), // name, parameters, body
    PrivateStaticMethodAsync(String, Vec<DestructuringElement>, Vec<Statement>), // private static async method
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
    Namespace(String),             // export * as name from "module"
    Star,                          // export * from "module"
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

// ── Eval restriction helpers (sec-performeval-rules-in-initializer) ──────────
// These functions walk the AST looking for constructs forbidden in eval inside
// class field initializers.  They recurse into arrow functions (which inherit
// the enclosing arguments/super/new.target bindings) but NOT into regular
// functions, generators, async functions, methods, or class bodies (which
// create new scopes for those bindings).

/// Bit flags returned by `eval_ast_scan`.
pub const SCAN_ARGUMENTS: u8 = 0x01;
pub const SCAN_SUPER_CALL: u8 = 0x02;
pub const SCAN_SUPER_PROP: u8 = 0x04;
pub const SCAN_NEW_TARGET: u8 = 0x08;

/// Scan `statements` for forbidden constructs indicated by `mask`.
/// Returns the subset of `mask` bits that were actually found.
pub fn eval_ast_scan(statements: &[Statement], mask: u8) -> u8 {
    let mut found: u8 = 0;
    for stmt in statements {
        scan_statement(stmt, mask, &mut found);
        if found & mask == mask {
            return found;
        }
    }
    found
}

fn scan_statement(stmt: &Statement, mask: u8, found: &mut u8) {
    if *found & mask == mask {
        return;
    }
    match &*stmt.kind {
        StatementKind::Expr(e) => scan_expr(e, mask, found),
        StatementKind::Let(decls) | StatementKind::Var(decls) => {
            for (_, init) in decls {
                if let Some(e) = init {
                    scan_expr(e, mask, found);
                }
            }
        }
        StatementKind::Const(decls) => {
            for (_, e) in decls {
                scan_expr(e, mask, found);
            }
        }
        StatementKind::Return(opt) => {
            if let Some(e) = opt {
                scan_expr(e, mask, found);
            }
        }
        StatementKind::Throw(e) => scan_expr(e, mask, found),
        StatementKind::Block(stmts) => {
            for s in stmts {
                scan_statement(s, mask, found);
            }
        }
        StatementKind::If(if_stmt) => {
            scan_expr(&if_stmt.condition, mask, found);
            for s in &if_stmt.then_body {
                scan_statement(s, mask, found);
            }
            if let Some(eb) = &if_stmt.else_body {
                for s in eb {
                    scan_statement(s, mask, found);
                }
            }
        }
        StatementKind::While(cond, body) => {
            scan_expr(cond, mask, found);
            for s in body {
                scan_statement(s, mask, found);
            }
        }
        StatementKind::DoWhile(body, cond) => {
            for s in body {
                scan_statement(s, mask, found);
            }
            scan_expr(cond, mask, found);
        }
        StatementKind::For(f) => {
            if let Some(init) = &f.init {
                scan_statement(init, mask, found);
            }
            if let Some(test) = &f.test {
                scan_expr(test, mask, found);
            }
            if let Some(upd) = &f.update {
                scan_statement(upd, mask, found);
            }
            for s in &f.body {
                scan_statement(s, mask, found);
            }
        }
        StatementKind::ForOf(_, _, iter, body) | StatementKind::ForIn(_, _, iter, body) | StatementKind::ForAwaitOf(_, _, iter, body) => {
            scan_expr(iter, mask, found);
            for s in body {
                scan_statement(s, mask, found);
            }
        }
        StatementKind::ForOfExpr(lhs, iter, body)
        | StatementKind::ForInExpr(lhs, iter, body)
        | StatementKind::ForAwaitOfExpr(lhs, iter, body) => {
            scan_expr(lhs, mask, found);
            scan_expr(iter, mask, found);
            for s in body {
                scan_statement(s, mask, found);
            }
        }
        StatementKind::ForOfDestructuringObject(_, _, iter, body)
        | StatementKind::ForOfDestructuringArray(_, _, iter, body)
        | StatementKind::ForInDestructuringObject(_, _, iter, body)
        | StatementKind::ForInDestructuringArray(_, _, iter, body)
        | StatementKind::ForAwaitOfDestructuringObject(_, _, iter, body)
        | StatementKind::ForAwaitOfDestructuringArray(_, _, iter, body) => {
            scan_expr(iter, mask, found);
            for s in body {
                scan_statement(s, mask, found);
            }
        }
        StatementKind::Switch(sw) => {
            scan_expr(&sw.expr, mask, found);
            for case in &sw.cases {
                match case {
                    SwitchCase::Case(test, body) => {
                        scan_expr(test, mask, found);
                        for s in body {
                            scan_statement(s, mask, found);
                        }
                    }
                    SwitchCase::Default(body) => {
                        for s in body {
                            scan_statement(s, mask, found);
                        }
                    }
                }
            }
        }
        StatementKind::TryCatch(tc) => {
            for s in &tc.try_body {
                scan_statement(s, mask, found);
            }
            if let Some(cb) = &tc.catch_body {
                for s in cb {
                    scan_statement(s, mask, found);
                }
            }
            if let Some(fb) = &tc.finally_body {
                for s in fb {
                    scan_statement(s, mask, found);
                }
            }
        }
        StatementKind::With(expr, body) => {
            scan_expr(expr, mask, found);
            for s in body {
                scan_statement(s, mask, found);
            }
        }
        StatementKind::Label(_, inner) => scan_statement(inner, mask, found),
        StatementKind::Assign(_, e) => scan_expr(e, mask, found),
        StatementKind::LetDestructuringArray(_, e)
        | StatementKind::VarDestructuringArray(_, e)
        | StatementKind::ConstDestructuringArray(_, e)
        | StatementKind::LetDestructuringObject(_, e)
        | StatementKind::VarDestructuringObject(_, e)
        | StatementKind::ConstDestructuringObject(_, e) => scan_expr(e, mask, found),
        StatementKind::Using(decls) | StatementKind::AwaitUsing(decls) => {
            for (_, e) in decls {
                scan_expr(e, mask, found);
            }
        }
        // Function/class declarations create new scopes — do NOT recurse
        StatementKind::FunctionDeclaration(..) | StatementKind::Class(..) => {}
        // Other statements with no sub-expressions
        StatementKind::Break(_)
        | StatementKind::Continue(_)
        | StatementKind::Debugger
        | StatementKind::Import(..)
        | StatementKind::Export(..) => {}
    }
}

fn scan_expr(expr: &Expr, mask: u8, found: &mut u8) {
    if *found & mask == mask {
        return;
    }
    match expr {
        // Leaf checks
        Expr::Var(name, _, _) if mask & SCAN_ARGUMENTS != 0 && name == "arguments" => {
            *found |= SCAN_ARGUMENTS;
        }
        Expr::SuperCall(_) => {
            *found |= SCAN_SUPER_CALL;
        }
        Expr::SuperMethod(_, _) => {
            *found |= SCAN_SUPER_CALL;
        }
        Expr::SuperProperty(_) | Expr::SuperComputedProperty(_) => {
            *found |= SCAN_SUPER_PROP;
        }
        Expr::SuperComputedMethod(_, _) => {
            *found |= SCAN_SUPER_PROP | SCAN_SUPER_CALL;
        }
        Expr::NewTarget => {
            *found |= SCAN_NEW_TARGET;
        }

        // Arrow functions: recurse (they inherit arguments/super/new.target)
        Expr::ArrowFunction(_, body) | Expr::AsyncArrowFunction(_, body) => {
            for s in body {
                scan_statement(s, mask, found);
            }
        }

        // Regular functions/generators/async — do NOT recurse (new scope)
        Expr::Function(..) | Expr::GeneratorFunction(..) | Expr::AsyncFunction(..) | Expr::AsyncGeneratorFunction(..) => {}

        // Class expressions — do NOT recurse
        Expr::Class(_) => {}

        // Recursive cases for compound expressions
        Expr::Assign(a, b)
        | Expr::Binary(a, _, b)
        | Expr::LogicalAnd(a, b)
        | Expr::LogicalOr(a, b)
        | Expr::NullishCoalescing(a, b)
        | Expr::Mod(a, b)
        | Expr::Pow(a, b)
        | Expr::LogicalAndAssign(a, b)
        | Expr::LogicalOrAssign(a, b)
        | Expr::NullishAssign(a, b)
        | Expr::AddAssign(a, b)
        | Expr::SubAssign(a, b)
        | Expr::PowAssign(a, b)
        | Expr::MulAssign(a, b)
        | Expr::DivAssign(a, b)
        | Expr::ModAssign(a, b)
        | Expr::BitXorAssign(a, b)
        | Expr::BitAndAssign(a, b)
        | Expr::BitOrAssign(a, b)
        | Expr::LeftShiftAssign(a, b)
        | Expr::RightShiftAssign(a, b)
        | Expr::UnsignedRightShiftAssign(a, b)
        | Expr::Index(a, b)
        | Expr::Comma(a, b)
        | Expr::OptionalIndex(a, b) => {
            scan_expr(a, mask, found);
            scan_expr(b, mask, found);
        }
        Expr::Conditional(a, b, c) => {
            scan_expr(a, mask, found);
            scan_expr(b, mask, found);
            scan_expr(c, mask, found);
        }
        Expr::Property(e, _)
        | Expr::OptionalProperty(e, _)
        | Expr::PrivateMember(e, _)
        | Expr::OptionalPrivateMember(e, _)
        | Expr::TypeOf(e)
        | Expr::Delete(e)
        | Expr::Void(e)
        | Expr::Await(e)
        | Expr::LogicalNot(e)
        | Expr::UnaryNeg(e)
        | Expr::UnaryPlus(e)
        | Expr::BitNot(e)
        | Expr::Increment(e)
        | Expr::Decrement(e)
        | Expr::Spread(e)
        | Expr::PostIncrement(e)
        | Expr::PostDecrement(e)
        | Expr::Getter(e)
        | Expr::Setter(e)
        | Expr::YieldStar(e)
        | Expr::DynamicImport(e, None) => {
            scan_expr(e, mask, found);
        }
        Expr::DynamicImport(e, Some(opts)) => {
            scan_expr(e, mask, found);
            scan_expr(opts, mask, found);
        }
        Expr::Yield(Some(e)) => {
            scan_expr(e, mask, found);
        }
        Expr::Call(callee, args) | Expr::OptionalCall(callee, args) | Expr::New(callee, args) => {
            scan_expr(callee, mask, found);
            for a in args {
                scan_expr(a, mask, found);
            }
        }
        Expr::Object(entries) => {
            for (k, v, _, _) in entries {
                scan_expr(k, mask, found);
                scan_expr(v, mask, found);
            }
        }
        Expr::Array(elems) => {
            for e in elems.iter().flatten() {
                scan_expr(e, mask, found);
            }
        }
        Expr::TaggedTemplate(tag, _, _, _, exprs) => {
            scan_expr(tag, mask, found);
            for e in exprs {
                scan_expr(e, mask, found);
            }
        }
        Expr::TemplateString(_) => {
            // Template parts store raw tokens, not parsed expressions — skip
        }
        // Leaves with no sub-expressions
        _ => {
            log::warn!("Unhandled expression in scan_expr: {:?}", expr);
        }
    }
}
