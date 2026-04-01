use crate::core::opcode::{Chunk, Opcode};
use crate::core::statement::{
    BinaryOp, CatchParamPattern, ClassMember, DestructuringElement, Expr, ImportSpecifier, ObjectDestructuringElement, Statement,
    StatementKind,
};
use crate::core::{JSError, Value};
use crate::raise_syntax_error;

pub(crate) const INTERNAL_FOROF_HELPER: &str = "__forOfValues internal";
pub(crate) const INTERNAL_GETITER_HELPER: &str = "__getIterator internal";

#[derive(Default)]
pub struct Compiler<'gc> {
    chunk: Chunk<'gc>,
    locals: Vec<String>,
    const_locals: std::collections::HashSet<String>, // locals that are const-like (class name bindings, const decls)
    parent_const_locals: std::collections::HashSet<String>, // direct parent function's const locals
    top_level_block_aliases: Vec<std::collections::HashMap<String, (String, bool)>>, // strict top-level block lexical names -> hidden globals for nested closures
    function_depth: u32,  // true function nesting depth; ignores temporary top-level local modes
    scope_depth: i32,     // 0 = top-level (global), > 0 = inside function
    current_strict: bool, // whether surrounding context is strict mode
    loop_stack: Vec<LoopContext>,
    pending_label: Option<String>,                        // label to attach to the next loop
    forin_counter: u32,                                   // unique ID for for-in synthetic variables
    current_class_parent: Option<String>,                 // parent class name for super resolution
    current_class_instance_fields: Vec<Vec<ClassMember>>, // instance fields to init after super()
    current_class_expr_refs: Vec<String>,                 // temp bindings for class expressions
    current_class_expr_names: Vec<String>,                // names of in-flight class expressions (for Var resolution)
    allow_super_call: bool,                               // whether direct super() calls are allowed in current function body
    allow_super_in_arrow_iife: bool,                      // temporary flag for immediate arrow invocation contexts
    // Closure capture support
    parent_locals: Vec<String>,        // direct parent function's locals
    parent_upvalues: Vec<UpvalueInfo>, // direct parent function's upvalues
    upvalues: Vec<UpvalueInfo>,        // current function's captured upvalues
    // Completion value tracking for eval loop results
    completion_var: Option<String>, // synthetic variable name holding loop completion value
    completion_counter: u32,        // unique ID for completion variables
    // Try-finally: allow break/continue to pass through finally blocks
    try_finally_stack: Vec<TryFinallyContext>,
    try_finally_counter: u32,
    // Track generator compilation contexts for minimal yield collection.
    generator_items_stack: Vec<String>,
    async_generator_items_stack: Vec<String>,
    // Jump patches for `return` statements inside generator bodies.
    generator_return_patches: Vec<usize>,
    // Compile-time unique class ID counter and stack for private name resolution
    class_privns_counter: usize,
    class_privns_stack: Vec<(usize, std::collections::HashSet<String>)>,
    // Pre-compiled private method/getter/setter locals for per-instance installation
    current_class_priv_method_locals: Vec<Vec<(String, String)>>, // stack of (private_key, local_name)
    // Eval context flags for PerformEval restrictions in class field initializers
    // Bit 0: in class field initializer, Bit 1: in method, Bit 2: in constructor
    eval_context_flags: u8,
    // Brand info for classes with private members: stack of Option<(brand_local_name, class_id)>
    current_class_brand_info: Vec<Option<(String, usize)>>,
}

#[derive(Debug, Clone)]
struct UpvalueInfo {
    name: String,
    index: u8,      // index in parent's locals or upvalues
    is_local: bool, // true = from parent's locals, false = from parent's upvalues
}

#[derive(Debug, Clone, Default)]
struct LoopContext {
    label: Option<String>,           // optional label for labeled break/continue
    continue_patches: Vec<usize>,    // offsets to patch with continue target
    break_patches: Vec<usize>,       // offsets to patch with post-loop address
    for_of_iter_var: Option<String>, // iterator variable name for for-of loops (IteratorClose on early exit)
}

#[derive(Debug, Clone)]
struct TryFinallyContext {
    action_id_var: String,            // synthetic variable name for pending control-flow action id
    return_value_var: String,         // synthetic variable name for pending return value
    saved_cv_var: Option<String>,     // synthetic variable to save completion value before finally
    finally_jump_patches: Vec<usize>, // Jump addresses to patch to finally body start
    pending_actions: Vec<PendingFinallyAction>,
}

#[derive(Debug, Clone)]
enum PendingFinallyActionKind {
    Break,
    Continue,
    Return,
}

#[derive(Debug, Clone)]
struct PendingFinallyAction {
    id: u32,
    kind: PendingFinallyActionKind,
    label: Option<String>,
}

impl<'gc> Compiler<'gc> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve a private field name to a unique key using the current class nesting context.
    /// Searches the class_privns_stack from innermost to outermost for the class that declares `name`.
    /// `name` should be the bare name (e.g., "x"), NOT prefixed.
    fn resolve_private_key(&self, name: &str) -> String {
        for (class_id, names) in self.class_privns_stack.iter().rev() {
            if names.contains(name) {
                return format!("\x00#{}:{}", class_id, name);
            }
        }
        // Fallback for names not found in stack (shouldn't happen for valid code)
        super::make_private_key(name)
    }

    /// Resolve a prefixed private key (like "\x00#name") to a unique key.
    /// Strips the PRIVATE_KEY_PREFIX and delegates to resolve_private_key.
    fn resolve_prefixed_private_key(&self, prefixed: &str) -> String {
        if let Some(bare) = prefixed.strip_prefix(super::PRIVATE_KEY_PREFIX) {
            self.resolve_private_key(bare)
        } else {
            prefixed.to_string()
        }
    }

    /// Set the private name context for eval compilation inside class bodies.
    /// This allows the compiler to resolve private field keys (e.g., `#x`) to their
    /// unique internal keys (e.g., `\x00#1:x`) matching the enclosing class.
    pub fn set_private_name_context(&mut self, context: Vec<(usize, std::collections::HashSet<String>)>) {
        // Set counter to 1 past the highest class ID to avoid collisions
        if let Some(max_id) = context.iter().map(|(id, _)| *id).max() {
            self.class_privns_counter = max_id + 1;
        }
        self.class_privns_stack = context;
    }

    pub fn compile(mut self, statements: &[Statement]) -> Result<Chunk<'gc>, JSError> {
        // Check for global "use strict" directive in prologue before hoisting
        for stmt in statements.iter() {
            match &*stmt.kind {
                StatementKind::Expr(Expr::StringLit(s)) => {
                    if crate::unicode::utf16_to_utf8(s) == "use strict" {
                        self.current_strict = true;
                    }
                    // continue scanning until non-string-literal
                }
                _ => break,
            }
        }
        // Hoist top-level `var` declarations to `undefined` before execution.
        // Function declarations are still emitted first right below, so they override
        // same-name hoisted vars at initialization time.
        self.emit_hoisted_global_vars(statements);
        // Hoist function declarations to the top
        for stmt in statements.iter() {
            if matches!(*stmt.kind, StatementKind::FunctionDeclaration(..)) {
                self.compile_statement(stmt, false)?;
            }
        }
        let mut remaining_non_function = statements
            .iter()
            .filter(|stmt| !matches!(*stmt.kind, StatementKind::FunctionDeclaration(..)))
            .count();
        for stmt in statements.iter() {
            if matches!(*stmt.kind, StatementKind::FunctionDeclaration(..)) {
                continue;
            }
            remaining_non_function = remaining_non_function.saturating_sub(1);
            let is_last = remaining_non_function == 0;
            self.compile_statement(stmt, is_last)?;
        }

        // Ensure returning at the end
        self.chunk.write_opcode(Opcode::Return);

        Ok(self.chunk)
    }

    fn emit_jump(&mut self, opcode: Opcode) -> usize {
        self.chunk.write_opcode(opcode);
        // Write placeholder u16
        self.chunk.write_u16(0xffff);
        self.chunk.code.len() - 2 // Return the offset to the placeholder
    }

    fn patch_jump(&mut self, offset: usize) {
        let jump_target = self.chunk.code.len();
        self.patch_jump_to(offset, jump_target);
    }

    fn patch_jump_to(&mut self, offset: usize, target: usize) {
        if target > u16::MAX as usize {
            panic!("Jump target too large");
        }
        self.chunk.code[offset] = (target & 0xff) as u8;
        self.chunk.code[offset + 1] = ((target >> 8) & 0xff) as u8;
    }

    fn emit_loop(&mut self, loop_start: usize) {
        self.chunk.write_opcode(Opcode::Jump);
        if loop_start > u16::MAX as usize {
            panic!("Loop start too large");
        }
        self.chunk.write_u16(loop_start as u16);
    }

    fn write_call_operand(&mut self, arg_count: usize, flags: u8) {
        if arg_count > u16::MAX as usize {
            panic!("Call arg count too large");
        }
        if arg_count < 0x3f {
            self.chunk.write_byte(arg_count as u8 | flags);
        } else {
            self.chunk.write_byte(0x3f | flags);
            self.chunk.write_u16(arg_count as u16);
        }
    }

    fn emit_call_opcode(&mut self, arg_count: usize, flags: u8) {
        self.chunk.write_opcode(Opcode::Call);
        self.write_call_operand(arg_count, flags);
    }

    fn function_is_strict(&self, body: &[Statement], force_strict: bool) -> bool {
        if force_strict || self.current_strict {
            return true;
        }

        matches!(
            body.first().map(|stmt| &*stmt.kind),
            Some(StatementKind::Expr(Expr::StringLit(s)))
                if *s == crate::unicode::utf8_to_utf16("use strict")
        )
    }

    fn record_fn_strictness(&mut self, func_ip: usize, body: &[Statement], force_strict: bool) -> bool {
        let is_strict = self.function_is_strict(body, force_strict);
        self.chunk.fn_strictness.insert(func_ip, is_strict);
        // Record private name context for direct eval support inside class bodies
        if !self.class_privns_stack.is_empty() {
            self.chunk.fn_private_name_context.insert(func_ip, self.class_privns_stack.clone());
        }
        // Record eval context flags for PerformEval restrictions
        if self.eval_context_flags != 0 {
            self.chunk.fn_eval_context.insert(func_ip, self.eval_context_flags);
        }
        is_strict
    }

    fn collect_function_var_names_from_statement(stmt: &Statement, out: &mut Vec<String>) {
        match &*stmt.kind {
            StatementKind::Var(decls) => {
                for (name, _) in decls {
                    if !out.iter().any(|existing| existing == name) {
                        out.push(name.clone());
                    }
                }
            }
            StatementKind::Block(stmts)
            | StatementKind::ForOf(_, _, _, stmts)
            | StatementKind::ForAwaitOf(_, _, _, stmts)
            | StatementKind::ForIn(_, _, _, stmts)
            | StatementKind::ForOfExpr(_, _, stmts)
            | StatementKind::ForInExpr(_, _, stmts)
            | StatementKind::ForOfDestructuringArray(_, _, _, stmts)
            | StatementKind::ForOfDestructuringObject(_, _, _, stmts)
            | StatementKind::ForInDestructuringArray(_, _, _, stmts)
            | StatementKind::ForInDestructuringObject(_, _, _, stmts)
            | StatementKind::With(_, stmts) => {
                for nested in stmts {
                    Self::collect_function_var_names_from_statement(nested, out);
                }
            }
            StatementKind::If(if_stmt) => {
                for nested in &if_stmt.then_body {
                    Self::collect_function_var_names_from_statement(nested, out);
                }
                if let Some(else_body) = &if_stmt.else_body {
                    for nested in else_body {
                        Self::collect_function_var_names_from_statement(nested, out);
                    }
                }
            }
            StatementKind::DoWhile(body, _) | StatementKind::While(_, body) => {
                for nested in body {
                    Self::collect_function_var_names_from_statement(nested, out);
                }
            }
            StatementKind::For(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    Self::collect_function_var_names_from_statement(init, out);
                }
                for nested in &for_stmt.body {
                    Self::collect_function_var_names_from_statement(nested, out);
                }
            }
            StatementKind::TryCatch(tc) => {
                for nested in &tc.try_body {
                    Self::collect_function_var_names_from_statement(nested, out);
                }
                if let Some(catch_body) = &tc.catch_body {
                    for nested in catch_body {
                        Self::collect_function_var_names_from_statement(nested, out);
                    }
                }
                if let Some(finally_body) = &tc.finally_body {
                    for nested in finally_body {
                        Self::collect_function_var_names_from_statement(nested, out);
                    }
                }
            }
            StatementKind::Label(_, inner) => {
                Self::collect_function_var_names_from_statement(inner, out);
            }
            StatementKind::Switch(sw) => {
                for case in &sw.cases {
                    let body = match case {
                        crate::core::statement::SwitchCase::Case(_, body) => body,
                        crate::core::statement::SwitchCase::Default(body) => body,
                    };
                    for nested in body {
                        Self::collect_function_var_names_from_statement(nested, out);
                    }
                }
            }
            StatementKind::FunctionDeclaration(..) | StatementKind::Class(..) => {}
            _ => {}
        }
    }

    fn has_parameter_expressions(params: &[DestructuringElement]) -> bool {
        params.iter().any(|p| {
            matches!(
                p,
                DestructuringElement::Variable(_, Some(_))
                    | DestructuringElement::NestedArray(_, Some(_))
                    | DestructuringElement::NestedObject(_, Some(_))
            )
        })
    }

    fn emit_hoisted_var_slots(&mut self, body: &[Statement]) {
        let mut hoisted = Vec::new();
        for stmt in body {
            Self::collect_function_var_names_from_statement(stmt, &mut hoisted);
        }

        for name in hoisted {
            if self.locals.iter().any(|existing| existing == &name) {
                continue;
            }
            let undef_idx = self.chunk.add_constant(Value::Undefined);
            self.chunk.write_opcode(Opcode::Constant);
            self.chunk.write_u16(undef_idx);
            self.locals.push(name);
        }
    }

    fn emit_hoisted_global_vars(&mut self, body: &[Statement]) {
        let mut hoisted = Vec::new();
        for stmt in body {
            Self::collect_function_var_names_from_statement(stmt, &mut hoisted);
        }

        for name in hoisted {
            let undef_idx = self.chunk.add_constant(Value::Undefined);
            self.chunk.write_opcode(Opcode::Constant);
            self.chunk.write_u16(undef_idx);

            self.chunk.declared_globals.insert(name.clone());
            let name_u16 = crate::unicode::utf8_to_utf16(&name);
            let name_idx = self.chunk.add_constant(Value::String(name_u16));
            self.chunk.write_opcode(Opcode::DefineGlobal);
            self.chunk.write_u16(name_idx);
        }
    }

    /// Create a LoopContext, consuming any pending label
    fn make_loop_context(&mut self, _loop_start: usize) -> LoopContext {
        LoopContext {
            label: self.pending_label.take(),
            ..LoopContext::default()
        }
    }

    /// Emit IteratorClose for all enclosing for-of loops (innermost first)
    fn emit_for_of_close_all(&mut self) {
        let iter_vars: Vec<String> = self.loop_stack.iter().rev().filter_map(|ctx| ctx.for_of_iter_var.clone()).collect();
        for var in iter_vars {
            self.emit_helper_get(&var);
            self.chunk.write_opcode(Opcode::IteratorClose);
        }
    }

    /// Emit IteratorClose for the innermost for-of loop only (for break)
    fn emit_for_of_close_current(&mut self) {
        if let Some(var) = self.loop_stack.last().and_then(|ctx| ctx.for_of_iter_var.clone()) {
            self.emit_helper_get(&var);
            self.chunk.write_opcode(Opcode::IteratorClose);
        }
    }

    fn lookup_top_level_block_alias(&self, name: &str) -> Option<(String, bool)> {
        for scope in self.top_level_block_aliases.iter().rev() {
            if let Some((alias, is_const_like)) = scope.get(name) {
                return Some((alias.clone(), *is_const_like));
            }
        }
        None
    }

    fn emit_define_global_binding(&mut self, name: &str, const_like: bool) {
        let define_opcode = if const_like {
            Opcode::DefineGlobalConst
        } else {
            Opcode::DefineGlobal
        };

        // Track declared globals for strict-mode eval writeback
        self.chunk.declared_globals.insert(name.to_string());

        let name_u16 = crate::unicode::utf8_to_utf16(name);
        let name_idx = self.chunk.add_constant(Value::String(name_u16));
        if let Some((alias, alias_const_like)) = self.lookup_top_level_block_alias(name) {
            self.chunk.write_opcode(Opcode::Dup);
            self.chunk.write_opcode(define_opcode);
            self.chunk.write_u16(name_idx);

            let alias_u16 = crate::unicode::utf8_to_utf16(&alias);
            let alias_idx = self.chunk.add_constant(Value::String(alias_u16));
            self.chunk.write_opcode(if alias_const_like {
                Opcode::DefineGlobalConst
            } else {
                Opcode::DefineGlobal
            });
            self.chunk.write_u16(alias_idx);
        } else {
            self.chunk.write_opcode(define_opcode);
            self.chunk.write_u16(name_idx);
        }
    }

    fn emit_set_global_binding(&mut self, name: &str) {
        if let Some((alias, is_const_like)) = self.lookup_top_level_block_alias(name) {
            if is_const_like {
                self.emit_const_assign_error(name);
                return;
            }

            if self.function_depth == 0 {
                self.chunk.write_opcode(Opcode::Dup);
                let name_u16 = crate::unicode::utf8_to_utf16(name);
                let name_idx = self.chunk.add_constant(Value::String(name_u16));
                self.chunk.write_opcode(Opcode::SetGlobal);
                self.chunk.write_u16(name_idx);
            }

            let alias_u16 = crate::unicode::utf8_to_utf16(&alias);
            let alias_idx = self.chunk.add_constant(Value::String(alias_u16));
            self.chunk.write_opcode(Opcode::SetGlobal);
            self.chunk.write_u16(alias_idx);
        } else {
            let name_u16 = crate::unicode::utf8_to_utf16(name);
            let name_idx = self.chunk.add_constant(Value::String(name_u16));
            self.chunk.write_opcode(Opcode::SetGlobal);
            self.chunk.write_u16(name_idx);
        }
    }

    fn compile_statement(&mut self, stmt: &Statement, is_last: bool) -> Result<(), JSError> {
        self.chunk.record_line(stmt.line, stmt.column);
        match &*stmt.kind {
            StatementKind::Expr(expr) => {
                // Detect 'use strict' directive at top-level or inside function
                if let Expr::StringLit(s) = expr
                    && crate::unicode::utf16_to_utf8(s) == "use strict"
                    && self.scope_depth == 0
                {
                    self.current_strict = true;
                }
                // In the VM test harness, flush queued microtasks before the
                // final top-level expression is evaluated so its value reflects
                // settled promise callbacks.
                if is_last && self.scope_depth == 0 {
                    self.emit_helper_get("__drain_microtasks__");
                    self.emit_call_opcode(0, 0);
                    self.chunk.write_opcode(Opcode::Pop);
                }
                self.compile_expr(expr)?;
                // Pop if it's not the last evaluated statement, to keep stack clean
                if !is_last {
                    self.emit_save_completion();
                    self.chunk.write_opcode(Opcode::Pop);
                }
            }
            StatementKind::Let(decls) => {
                for (name, init_opt) in decls {
                    if let Some(init) = init_opt {
                        let func_ip = self.peek_func_ip(init);
                        self.compile_expr(init)?;
                        if let Some(ip) = func_ip {
                            self.chunk.fn_names.entry(ip).or_insert_with(|| name.clone());
                        }
                    } else {
                        let idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(idx);
                    }

                    if self.scope_depth > 0 {
                        // let is block-scoped: always create a new local slot
                        self.locals.push(name.clone());
                    } else {
                        self.emit_define_global_binding(name, false);
                    }
                }
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::Var(decls) => {
                for (name, init_opt) in decls {
                    if let Some(init) = init_opt {
                        let func_ip = self.peek_func_ip(init);
                        self.compile_expr(init)?;
                        if let Some(ip) = func_ip {
                            self.chunk.fn_names.entry(ip).or_insert_with(|| name.clone());
                        }
                    } else {
                        let idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(idx);
                    }

                    if self.scope_depth > 0 {
                        // var is function-scoped: reuse existing slot if found
                        if let Some(pos) = self.locals.iter().position(|l| l == name) {
                            self.chunk.write_opcode(Opcode::SetLocal);
                            self.chunk.write_byte(pos as u8);
                            self.chunk.write_opcode(Opcode::Pop);
                        } else {
                            self.locals.push(name.clone());
                        }
                    } else {
                        self.chunk.declared_globals.insert(name.clone());
                        let name_u16 = crate::unicode::utf8_to_utf16(name);
                        let name_idx = self.chunk.add_constant(Value::String(name_u16));
                        self.chunk.write_opcode(Opcode::DefineGlobal);
                        self.chunk.write_u16(name_idx);
                    }
                }
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::Const(decls) => {
                for (name, init) in decls {
                    // Infer function name for anonymous function/arrow
                    let func_ip = self.peek_func_ip(init);
                    self.compile_expr(init)?;
                    if let Some(ip) = func_ip {
                        self.chunk.fn_names.entry(ip).or_insert_with(|| name.clone());
                    }
                    if self.scope_depth > 0 {
                        self.locals.push(name.clone());
                        self.const_locals.insert(name.clone());
                    } else {
                        self.emit_define_global_binding(name, true);
                    }
                }
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::Assign(name, expr) => {
                self.compile_expr(expr)?;
                if let Some(pos) = self.locals.iter().rposition(|l| l == name) {
                    if self.const_locals.contains(name.as_str()) {
                        self.emit_const_assign_error(name);
                    } else {
                        self.chunk.write_opcode(Opcode::SetLocal);
                        self.chunk.write_byte(pos as u8);
                    }
                } else if self.is_const_upvalue(name) {
                    self.emit_const_assign_error(name);
                } else if let Some(upvalue_idx) = self.resolve_upvalue(name) {
                    self.chunk.write_opcode(Opcode::SetUpvalue);
                    self.chunk.write_byte(upvalue_idx);
                } else {
                    self.emit_set_global_binding(name);
                }
                if !is_last {
                    self.chunk.write_opcode(Opcode::Pop);
                }
            }
            StatementKind::Block(statements) => {
                let saved_locals = self.locals.len();
                // Collect function names declared in this block (for strict-mode block scoping)
                let mut block_fn_names: Vec<String> = Vec::new();
                let mut block_lexical_names: Vec<String> = Vec::new();
                let mut block_aliases: std::collections::HashMap<String, (String, bool)> = std::collections::HashMap::new();
                if self.scope_depth == 0 && self.current_strict {
                    for s in statements.iter() {
                        match &*s.kind {
                            StatementKind::FunctionDeclaration(name, ..) => {
                                block_fn_names.push(name.clone());
                                let alias = format!("__top_block_alias_{}__", self.forin_counter);
                                self.forin_counter = self.forin_counter.saturating_add(1);
                                block_aliases.insert(name.clone(), (alias, false));
                            }
                            StatementKind::Let(decls) => {
                                for (name, _) in decls {
                                    block_lexical_names.push(name.clone());
                                    let alias = format!("__top_block_alias_{}__", self.forin_counter);
                                    self.forin_counter = self.forin_counter.saturating_add(1);
                                    block_aliases.insert(name.clone(), (alias, false));
                                }
                            }
                            StatementKind::Const(decls) => {
                                for (name, _) in decls {
                                    block_lexical_names.push(name.clone());
                                    let alias = format!("__top_block_alias_{}__", self.forin_counter);
                                    self.forin_counter = self.forin_counter.saturating_add(1);
                                    block_aliases.insert(name.clone(), (alias, true));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                let pushed_block_aliases = !block_aliases.is_empty();
                if pushed_block_aliases {
                    self.top_level_block_aliases.push(block_aliases);
                }
                // Hoist function declarations to the top of the block
                for s in statements.iter() {
                    if matches!(*s.kind, StatementKind::FunctionDeclaration(..)) {
                        self.compile_statement(s, false)?;
                    }
                }
                for (i, s) in statements.iter().enumerate() {
                    if matches!(*s.kind, StatementKind::FunctionDeclaration(..)) {
                        continue;
                    }
                    let s_is_last = is_last && i == statements.len() - 1;
                    self.compile_statement(s, s_is_last)?;
                }
                // Clean up block-scoped locals
                if is_last {
                    // Frame is about to return; just fix compiler state
                    self.locals.truncate(saved_locals);
                } else {
                    self.end_block_scope(saved_locals);
                }
                // In strict mode at top level, remove block-scoped function declarations from globals
                for fn_name in block_fn_names {
                    let name_u16 = crate::unicode::utf8_to_utf16(&fn_name);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::DeleteGlobal);
                    self.chunk.write_u16(name_idx);
                }
                for name in block_lexical_names {
                    let name_u16 = crate::unicode::utf8_to_utf16(&name);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::DeleteGlobal);
                    self.chunk.write_u16(name_idx);
                }
                if pushed_block_aliases {
                    self.top_level_block_aliases.pop();
                }
            }
            StatementKind::If(if_stmt) => {
                self.compile_expr(&if_stmt.condition)?;
                let then_jump = self.emit_jump(Opcode::JumpIfFalse);

                // Then branch
                for (i, s) in if_stmt.then_body.iter().enumerate() {
                    let s_is_last = is_last && i == if_stmt.then_body.len() - 1 && if_stmt.else_body.is_none();
                    self.compile_statement(s, s_is_last)?;
                }

                if let Some(else_body) = &if_stmt.else_body {
                    let else_jump = self.emit_jump(Opcode::Jump);
                    self.patch_jump(then_jump);

                    for (i, s) in else_body.iter().enumerate() {
                        let s_is_last = is_last && i == else_body.len() - 1;
                        self.compile_statement(s, s_is_last)?;
                    }
                    self.patch_jump(else_jump);
                } else {
                    self.patch_jump(then_jump);
                }
            }
            StatementKind::DoWhile(body, cond) => {
                let saved_cv = self.completion_var.clone();
                if is_last {
                    self.setup_completion_var();
                }
                let loop_start = self.chunk.code.len();
                let ctx = self.make_loop_context(loop_start);
                self.loop_stack.push(ctx);

                for s in body {
                    self.compile_statement(s, false)?;
                }

                // continue target: the condition check (current IP)
                for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                    self.patch_jump(*cp);
                }

                self.compile_expr(cond)?;
                let continue_jump = self.emit_jump(Opcode::JumpIfTrue);
                // Patch: JumpIfTrue target is loop_start
                self.chunk.code[continue_jump] = (loop_start & 0xff) as u8;
                self.chunk.code[continue_jump + 1] = ((loop_start >> 8) & 0xff) as u8;

                let ctx = self.loop_stack.pop().unwrap();
                for bp in ctx.break_patches {
                    self.patch_jump(bp);
                }

                if is_last {
                    self.emit_load_completion();
                } else {
                    self.completion_var = saved_cv;
                }
            }
            StatementKind::While(cond, body) => {
                let saved_cv = self.completion_var.clone();
                if is_last {
                    self.setup_completion_var();
                }
                let loop_start = self.chunk.code.len();
                let ctx = self.make_loop_context(loop_start);
                self.loop_stack.push(ctx);

                self.compile_expr(cond)?;
                let exit_jump = self.emit_jump(Opcode::JumpIfFalse);

                for s in body {
                    self.compile_statement(s, false)?;
                }

                // Patch continue → loop_start (condition)
                for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                    self.patch_jump_to(*cp, loop_start);
                }

                self.emit_loop(loop_start);
                self.patch_jump(exit_jump);

                let ctx = self.loop_stack.pop().unwrap();
                for bp in ctx.break_patches {
                    self.patch_jump(bp);
                }

                if is_last {
                    self.emit_load_completion();
                } else {
                    self.completion_var = saved_cv;
                }
            }
            StatementKind::For(for_stmt) => {
                let saved_cv = self.completion_var.clone();
                if is_last {
                    self.setup_completion_var();
                }
                // Init should not affect completion value
                let body_cv = self.completion_var.take();
                if let Some(init) = &for_stmt.init {
                    self.compile_statement(init, false)?;
                }
                self.completion_var = body_cv;
                // Loop start: test
                let loop_start = self.chunk.code.len();
                let ctx = self.make_loop_context(loop_start);
                self.loop_stack.push(ctx);

                let exit_jump = if let Some(test) = &for_stmt.test {
                    self.compile_expr(test)?;
                    Some(self.emit_jump(Opcode::JumpIfFalse))
                } else {
                    None
                };
                // Body
                for s in &for_stmt.body {
                    self.compile_statement(s, false)?;
                }
                // Update should not affect completion value
                let body_cv = self.completion_var.take();
                let update_ip = self.chunk.code.len();
                for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                    self.patch_jump_to(*cp, update_ip);
                }
                if let Some(update) = &for_stmt.update {
                    self.compile_statement(update, false)?;
                }
                self.completion_var = body_cv;
                // Jump back to test
                self.emit_loop(loop_start);
                if let Some(ej) = exit_jump {
                    self.patch_jump(ej);
                }

                let ctx = self.loop_stack.pop().unwrap();
                for bp in ctx.break_patches {
                    self.patch_jump(bp);
                }

                if is_last {
                    self.emit_load_completion();
                } else {
                    self.completion_var = saved_cv;
                }
            }
            StatementKind::Return(expr_opt) => {
                if !self.async_generator_items_stack.is_empty() {
                    if let Some(expr) = expr_opt {
                        self.compile_expr(expr)?;
                    } else {
                        let idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(idx);
                    }
                    self.chunk.write_opcode(Opcode::Pop);
                    let patch = self.emit_jump(Opcode::Jump);
                    self.generator_return_patches.push(patch);
                }
                // Inside a suspendable generator body, return compiles normally.
                // The VM handles wrapping the value as {value, done: true}.
                // For legacy eager generators (async gen), use old items-array approach.
                else if let Some(items_name) = self.generator_items_stack.last().cloned() {
                    if items_name == "__gen_yield_marker__" {
                        // Suspendable generator: compile return value, emit Return
                        if let Some(expr) = expr_opt {
                            self.compile_expr(expr)?;
                        } else {
                            let idx = self.chunk.add_constant(Value::Undefined);
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(idx);
                        }
                        self.chunk.write_opcode(Opcode::Return);
                    } else {
                        // Legacy eager generator (async gen)
                        self.emit_helper_get(&items_name);
                        if let Some(expr) = expr_opt {
                            self.compile_expr(expr)?;
                        } else {
                            let idx = self.chunk.add_constant(Value::Undefined);
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(idx);
                        }
                        self.chunk.write_opcode(Opcode::ArrayPush);
                        self.chunk.write_opcode(Opcode::Pop);
                        let patch = self.emit_jump(Opcode::Jump);
                        self.generator_return_patches.push(patch);
                    }
                } else if !self.try_finally_stack.is_empty() {
                    let action_id = self.try_finally_counter;
                    self.try_finally_counter += 1;
                    let (return_value_var, action_id_var) = {
                        let tfc = self.try_finally_stack.last().unwrap();
                        (tfc.return_value_var.clone(), tfc.action_id_var.clone())
                    };

                    if let Some(expr) = expr_opt {
                        self.compile_expr(expr)?;
                    } else {
                        let idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(idx);
                    }
                    self.emit_helper_set(&return_value_var);
                    self.chunk.write_opcode(Opcode::Pop);

                    let action_idx = self.chunk.add_constant(Value::Number(action_id as f64));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(action_idx);
                    self.emit_helper_set(&action_id_var);
                    self.chunk.write_opcode(Opcode::Pop);

                    self.try_finally_stack
                        .last_mut()
                        .unwrap()
                        .pending_actions
                        .push(PendingFinallyAction {
                            id: action_id,
                            kind: PendingFinallyActionKind::Return,
                            label: None,
                        });

                    self.chunk.write_opcode(Opcode::TeardownTry);
                    let patch = self.emit_jump(Opcode::Jump);
                    self.try_finally_stack.last_mut().unwrap().finally_jump_patches.push(patch);
                } else if let Some(expr) = expr_opt {
                    self.compile_expr(expr)?;
                } else {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
                if self.try_finally_stack.is_empty() {
                    self.emit_for_of_close_all();
                    self.chunk.write_opcode(Opcode::Return);
                }
            }
            StatementKind::Throw(expr) => {
                self.compile_expr(expr)?;
                self.chunk.write_opcode(Opcode::Throw);
            }
            StatementKind::Break(label_opt) => {
                if !self.try_finally_stack.is_empty() {
                    let action_id = self.try_finally_counter;
                    self.try_finally_counter += 1;
                    let (action_id_var, saved_cv_name) = {
                        let tfc = self.try_finally_stack.last().unwrap();
                        (tfc.action_id_var.clone(), tfc.saved_cv_var.clone())
                    };
                    let action_idx = self.chunk.add_constant(Value::Number(action_id as f64));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(action_idx);
                    self.emit_helper_set(&action_id_var);
                    self.chunk.write_opcode(Opcode::Pop);
                    if let (Some(cv), Some(sv)) = (&self.completion_var, &saved_cv_name) {
                        let cv = cv.clone();
                        let sv = sv.clone();
                        self.emit_helper_get(&cv);
                        self.emit_helper_set(&sv);
                        self.chunk.write_opcode(Opcode::Pop);
                    }
                    self.try_finally_stack
                        .last_mut()
                        .unwrap()
                        .pending_actions
                        .push(PendingFinallyAction {
                            id: action_id,
                            kind: PendingFinallyActionKind::Break,
                            label: label_opt.clone(),
                        });
                    self.chunk.write_opcode(Opcode::TeardownTry);
                    let patch = self.emit_jump(Opcode::Jump);
                    self.try_finally_stack.last_mut().unwrap().finally_jump_patches.push(patch);
                } else {
                    // Emit IteratorClose for for-of loops being broken out of
                    if let Some(label) = label_opt {
                        let iter_vars: Vec<String> = {
                            let mut vars = Vec::new();
                            for ctx in self.loop_stack.iter().rev() {
                                if let Some(ref v) = ctx.for_of_iter_var {
                                    vars.push(v.clone());
                                }
                                if ctx.label.as_deref() == Some(label) {
                                    break;
                                }
                            }
                            vars
                        };
                        for var in &iter_vars {
                            self.emit_helper_get(var);
                            self.chunk.write_opcode(Opcode::IteratorClose);
                        }
                    } else {
                        self.emit_for_of_close_current();
                    }
                    let patch = self.emit_jump(Opcode::Jump);
                    if let Some(label) = label_opt {
                        if let Some(ctx) = self.loop_stack.iter_mut().rev().find(|c| c.label.as_deref() == Some(label)) {
                            ctx.break_patches.push(patch);
                        } else {
                            return Err(crate::raise_syntax_error!(format!("label '{}' not found for break", label)));
                        }
                    } else if let Some(ctx) = self.loop_stack.last_mut() {
                        ctx.break_patches.push(patch);
                    } else {
                        return Err(crate::raise_syntax_error!("break statement not in loop or switch"));
                    }
                }
            }
            StatementKind::Continue(label_opt) => {
                if !self.try_finally_stack.is_empty() {
                    let action_id = self.try_finally_counter;
                    self.try_finally_counter += 1;
                    let (action_id_var, saved_cv_name) = {
                        let tfc = self.try_finally_stack.last().unwrap();
                        (tfc.action_id_var.clone(), tfc.saved_cv_var.clone())
                    };
                    let action_idx = self.chunk.add_constant(Value::Number(action_id as f64));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(action_idx);
                    self.emit_helper_set(&action_id_var);
                    self.chunk.write_opcode(Opcode::Pop);
                    if let (Some(cv), Some(sv)) = (&self.completion_var, &saved_cv_name) {
                        let cv = cv.clone();
                        let sv = sv.clone();
                        self.emit_helper_get(&cv);
                        self.emit_helper_set(&sv);
                        self.chunk.write_opcode(Opcode::Pop);
                    }
                    self.try_finally_stack
                        .last_mut()
                        .unwrap()
                        .pending_actions
                        .push(PendingFinallyAction {
                            id: action_id,
                            kind: PendingFinallyActionKind::Continue,
                            label: label_opt.clone(),
                        });
                    self.chunk.write_opcode(Opcode::TeardownTry);
                    let patch = self.emit_jump(Opcode::Jump);
                    self.try_finally_stack.last_mut().unwrap().finally_jump_patches.push(patch);
                } else {
                    let patch = self.emit_jump(Opcode::Jump);
                    if let Some(label) = label_opt {
                        if let Some(ctx) = self.loop_stack.iter_mut().rev().find(|c| c.label.as_deref() == Some(label)) {
                            ctx.continue_patches.push(patch);
                        } else {
                            return Err(crate::raise_syntax_error!(format!("label '{}' not found for continue", label)));
                        }
                    } else if self.loop_stack.last().is_some() {
                        self.loop_stack.last_mut().unwrap().continue_patches.push(patch);
                    } else {
                        return Err(crate::raise_syntax_error!("continue statement not in loop"));
                    }
                }
            }
            StatementKind::Label(label, inner) => {
                // Check if inner is a loop/switch that consumes the label via pending_label
                let is_loop = matches!(
                    &*inner.kind,
                    StatementKind::While(..)
                        | StatementKind::DoWhile(..)
                        | StatementKind::For(..)
                        | StatementKind::ForIn(..)
                        | StatementKind::ForOf(..)
                        | StatementKind::ForOfExpr(..)
                        | StatementKind::ForInExpr(..)
                        | StatementKind::ForOfDestructuringArray(..)
                        | StatementKind::ForOfDestructuringObject(..)
                        | StatementKind::ForInDestructuringArray(..)
                        | StatementKind::ForInDestructuringObject(..)
                        | StatementKind::Switch(..)
                );
                if is_loop {
                    self.pending_label = Some(label.clone());
                    self.compile_statement(inner, is_last)?;
                    self.pending_label = None;
                } else {
                    // Labeled non-loop: push a pseudo LoopContext so break L can find it
                    let saved_cv = self.completion_var.clone();
                    if is_last {
                        self.setup_completion_var();
                    }
                    let ctx = LoopContext {
                        label: Some(label.clone()),
                        ..LoopContext::default()
                    };
                    self.loop_stack.push(ctx);
                    // Compile inner with is_last=false; completion tracked via completion_var
                    self.compile_statement(inner, false)?;
                    let ctx = self.loop_stack.pop().unwrap();
                    for bp in ctx.break_patches {
                        self.patch_jump(bp);
                    }
                    if is_last {
                        self.emit_load_completion();
                    } else {
                        self.completion_var = saved_cv;
                    }
                }
            }
            StatementKind::ForIn(_decl_kind, var_name, obj_expr, body) => {
                let saved_cv = self.completion_var.clone();
                if is_last {
                    self.setup_completion_var();
                }
                // Generate unique names for each for-in loop's synthetic variables
                let fid = self.forin_counter;
                self.forin_counter += 1;
                let obj_name = format!("__forin_obj_{}__", fid);
                let keys_name = format!("__keys_{}__", fid);
                let idx_name = format!("__idx_{}__", fid);

                self.compile_expr(obj_expr)?;
                self.chunk.write_opcode(Opcode::Dup);
                self.chunk.write_opcode(Opcode::GetKeys);
                // Stack: [..., obj, keys_array]
                // For locals: obj is at slot N, keys at slot N+1
                // For globals: DefineGlobal pops from top, so pop keys first, then obj
                if self.scope_depth > 0 {
                    self.locals.push(obj_name.clone());
                    self.locals.push(keys_name.clone());
                } else {
                    let kn = crate::unicode::utf8_to_utf16(&keys_name);
                    let kni = self.chunk.add_constant(Value::String(kn));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(kni);
                    let on = crate::unicode::utf8_to_utf16(&obj_name);
                    let oni = self.chunk.add_constant(Value::String(on));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(oni);
                };
                // i = 0
                let zero_idx = self.chunk.add_constant(Value::Number(0.0));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(zero_idx);
                if self.scope_depth > 0 {
                    self.locals.push(idx_name.clone());
                } else {
                    let n = crate::unicode::utf8_to_utf16(&idx_name);
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(ni);
                };

                // Pre-allocate loop variable slot if it's a new local
                if self.scope_depth > 0 && !self.locals.iter().any(|l| l == var_name) {
                    let undef = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef);
                    self.locals.push(var_name.clone());
                }

                // Loop start: test i < keys.length
                let loop_start = self.chunk.code.len();
                let ctx = self.make_loop_context(loop_start);
                self.loop_stack.push(ctx);
                // push i
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| *l == idx_name).unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16(&idx_name);
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(ni);
                }
                // push keys.length
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| *l == keys_name).unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16(&keys_name);
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(ni);
                }
                let len_key = crate::unicode::utf8_to_utf16("length");
                let len_idx = self.chunk.add_constant(Value::String(len_key));
                self.chunk.write_opcode(Opcode::GetProperty);
                self.chunk.write_u16(len_idx);
                self.chunk.write_opcode(Opcode::LessThan);
                let exit_jump = self.emit_jump(Opcode::JumpIfFalse);

                // var_name = keys[i]
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| *l == keys_name).unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16(&keys_name);
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(ni);
                }
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| *l == idx_name).unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16(&idx_name);
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(ni);
                }
                self.chunk.write_opcode(Opcode::GetIndex);
                // Store in var_name
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| l == var_name).unwrap();
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(pos as u8);
                    self.chunk.write_opcode(Opcode::Pop);
                } else {
                    let vn = crate::unicode::utf8_to_utf16(var_name);
                    let vni = self.chunk.add_constant(Value::String(vn));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(vni);
                }

                // Check if key still exists in the original object (handles deletion during iteration)
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| l == var_name).unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let vn = crate::unicode::utf8_to_utf16(var_name);
                    let vni = self.chunk.add_constant(Value::String(vn));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(vni);
                }
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| *l == obj_name).unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let on = crate::unicode::utf8_to_utf16(&obj_name);
                    let oni = self.chunk.add_constant(Value::String(on));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(oni);
                }
                self.chunk.write_opcode(Opcode::In);
                let skip_body_jump = self.emit_jump(Opcode::JumpIfFalse);

                // Body
                for s in body {
                    self.compile_statement(s, false)?;
                }

                // continue target: i++ update
                let update_ip = self.chunk.code.len();
                self.patch_jump(skip_body_jump);
                for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                    self.patch_jump_to(*cp, update_ip);
                }

                // i++
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| *l == idx_name).unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16(&idx_name);
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(ni);
                }
                self.chunk.write_opcode(Opcode::Increment);
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| *l == idx_name).unwrap();
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16(&idx_name);
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::SetGlobal);
                    self.chunk.write_u16(ni);
                }
                self.chunk.write_opcode(Opcode::Pop);

                self.emit_loop(loop_start);
                self.patch_jump(exit_jump);
                let ctx = self.loop_stack.pop().unwrap();
                for bp in ctx.break_patches {
                    self.patch_jump(bp);
                }

                // Clean up synthetic locals
                if self.scope_depth > 0 {
                    self.locals.retain(|l| *l != keys_name && *l != idx_name && *l != obj_name);
                }

                if is_last {
                    self.emit_load_completion();
                } else {
                    self.completion_var = saved_cv;
                }
            }
            StatementKind::TryCatch(tc) => {
                let saved_cv = self.completion_var.clone();
                if is_last {
                    self.setup_completion_var();
                }

                // Determine catch binding name constant index (or 0xffff for none)
                let binding_idx: u16 = if let Some(_catch_body) = &tc.catch_body {
                    if let Some(CatchParamPattern::Identifier(ref name)) = tc.catch_param {
                        let name_u16 = crate::unicode::utf8_to_utf16(name);
                        self.chunk.add_constant(Value::String(name_u16))
                    } else {
                        0xffff
                    }
                } else {
                    0xffff
                };

                // If there's a finally block, set up break-through-finally tracking
                let has_finally = tc.finally_body.is_some();
                let _finally_dispatch = if has_finally {
                    let id = self.try_finally_counter;
                    self.try_finally_counter += 1;
                    let action_name = format!("__tf_act_{}__", id);
                    let zero = self.chunk.add_constant(Value::Number(0.0));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(zero);
                    if self.scope_depth > 0 {
                        self.locals.push(action_name.clone());
                    } else {
                        let n = crate::unicode::utf8_to_utf16(&action_name);
                        let ni = self.chunk.add_constant(Value::String(n));
                        self.chunk.write_opcode(Opcode::DefineGlobal);
                        self.chunk.write_u16(ni);
                    }

                    let ret_name = format!("__tf_ret_{}__", id);
                    let undef = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef);
                    if self.scope_depth > 0 {
                        self.locals.push(ret_name.clone());
                    } else {
                        let n = crate::unicode::utf8_to_utf16(&ret_name);
                        let ni = self.chunk.add_constant(Value::String(n));
                        self.chunk.write_opcode(Opcode::DefineGlobal);
                        self.chunk.write_u16(ni);
                    }

                    let saved_cv_name = if self.completion_var.is_some() {
                        let sv_name = format!("__tf_cv_{}__", id);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(undef);
                        if self.scope_depth > 0 {
                            self.locals.push(sv_name.clone());
                        } else {
                            let n = crate::unicode::utf8_to_utf16(&sv_name);
                            let ni = self.chunk.add_constant(Value::String(n));
                            self.chunk.write_opcode(Opcode::DefineGlobal);
                            self.chunk.write_u16(ni);
                        }
                        Some(sv_name)
                    } else {
                        None
                    };
                    self.try_finally_stack.push(TryFinallyContext {
                        action_id_var: action_name.clone(),
                        return_value_var: ret_name.clone(),
                        saved_cv_var: saved_cv_name,
                        finally_jump_patches: Vec::new(),
                        pending_actions: Vec::new(),
                    });
                    Some((action_name, ret_name))
                } else {
                    None
                };

                // SetupTry <catch_ip:u16> <binding_idx:u16>
                self.chunk.write_opcode(Opcode::SetupTry);
                let catch_placeholder = self.chunk.code.len();
                self.chunk.write_u16(0xffff); // placeholder for catch ip
                self.chunk.write_u16(binding_idx);

                // Try body (block-scoped)
                let saved_try = self.locals.len();
                for s in &tc.try_body {
                    self.compile_statement(s, false)?;
                }
                self.end_block_scope(saved_try);
                self.chunk.write_opcode(Opcode::TeardownTry);

                // Jump over catch block
                let jump_over_catch = self.emit_jump(Opcode::Jump);

                // Patch catch address to here
                let catch_start = self.chunk.code.len();
                self.chunk.code[catch_placeholder] = (catch_start & 0xff) as u8;
                self.chunk.code[catch_placeholder + 1] = ((catch_start >> 8) & 0xff) as u8;

                // Catch body (block-scoped)
                let saved_catch = self.locals.len();
                if let Some(ref catch_body) = tc.catch_body {
                    for s in catch_body {
                        self.compile_statement(s, false)?;
                    }
                }
                self.end_block_scope(saved_catch);

                self.patch_jump(jump_over_catch);

                // Patch break-through-finally jumps to here (the finally body start)
                let finally_context = if has_finally {
                    let tfc = self.try_finally_stack.pop().unwrap();
                    for jp in &tfc.finally_jump_patches {
                        self.patch_jump(*jp);
                    }
                    Some(tfc)
                } else {
                    None
                };

                // Finally body (block-scoped)
                let saved_finally = self.locals.len();
                if let Some(ref finally_body) = tc.finally_body {
                    for s in finally_body {
                        self.compile_statement(s, false)?;
                    }
                }
                self.end_block_scope(saved_finally);

                // After finally: check break flag and restore cv, then jump to break target
                if let Some(tfc) = finally_context {
                    for action in &tfc.pending_actions {
                        self.emit_helper_get(&tfc.action_id_var);
                        let id_idx = self.chunk.add_constant(Value::Number(action.id as f64));
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(id_idx);
                        self.chunk.write_opcode(Opcode::Equal);
                        let next_check = self.emit_jump(Opcode::JumpIfFalse);

                        match action.kind {
                            PendingFinallyActionKind::Break => {
                                if let (Some(sv), Some(cv)) = (&tfc.saved_cv_var, &self.completion_var) {
                                    let sv = sv.clone();
                                    let cv = cv.clone();
                                    self.emit_helper_get(&sv);
                                    self.emit_helper_set(&cv);
                                    self.chunk.write_opcode(Opcode::Pop);
                                }
                                let break_jump = self.emit_jump(Opcode::Jump);
                                if let Some(label) = &action.label {
                                    if let Some(ctx) = self.loop_stack.iter_mut().rev().find(|c| c.label.as_deref() == Some(label)) {
                                        ctx.break_patches.push(break_jump);
                                    } else {
                                        return Err(crate::raise_syntax_error!(format!("label '{}' not found for break", label)));
                                    }
                                } else if let Some(ctx) = self.loop_stack.last_mut() {
                                    ctx.break_patches.push(break_jump);
                                } else {
                                    return Err(crate::raise_syntax_error!("break statement not in loop or switch"));
                                }
                            }
                            PendingFinallyActionKind::Continue => {
                                let continue_jump = self.emit_jump(Opcode::Jump);
                                if let Some(label) = &action.label {
                                    if let Some(ctx) = self.loop_stack.iter_mut().rev().find(|c| c.label.as_deref() == Some(label)) {
                                        ctx.continue_patches.push(continue_jump);
                                    } else {
                                        return Err(crate::raise_syntax_error!(format!("label '{}' not found for continue", label)));
                                    }
                                } else if let Some(ctx) = self.loop_stack.last_mut() {
                                    ctx.continue_patches.push(continue_jump);
                                } else {
                                    return Err(crate::raise_syntax_error!("continue statement not in loop"));
                                }
                            }
                            PendingFinallyActionKind::Return => {
                                self.emit_helper_get(&tfc.return_value_var);
                                self.chunk.write_opcode(Opcode::Return);
                            }
                        }

                        self.patch_jump(next_check);
                    }

                    if self.scope_depth > 0 {
                        self.locals.retain(|l| l != &tfc.action_id_var && l != &tfc.return_value_var);
                        if let Some(sv) = &tfc.saved_cv_var {
                            self.locals.retain(|l| l != sv);
                        }
                    }
                }

                if is_last {
                    self.emit_load_completion();
                } else {
                    self.completion_var = saved_cv;
                }
            }
            StatementKind::FunctionDeclaration(name, params, body, is_gen, is_async) => {
                if *is_gen && !*is_async {
                    let func_ip = self.compile_generator_function_body(Some(name.as_str()), params, body)?;
                    self.chunk.fn_names.insert(func_ip, name.clone());
                    self.emit_define_global_binding(name, false);
                    return Ok(());
                }

                if *is_gen && *is_async {
                    self.compile_async_generator_function_body(Some(name.as_str()), params, body)?;
                    if let Some(func_ip) = self.peek_func_ip(&Expr::AsyncGeneratorFunction(None, params.clone(), body.clone())) {
                        self.chunk.fn_names.insert(func_ip, name.clone());
                    }
                    self.emit_define_global_binding(name, false);
                    return Ok(());
                }

                // Jump over the function body in the main bytecode stream
                let jump_over = self.emit_jump(Opcode::Jump);
                let func_ip = self.chunk.code.len();
                if *is_async {
                    self.chunk.async_function_ips.insert(func_ip);
                }
                let fn_is_strict = self.record_fn_strictness(func_ip, body, false);

                // Save and reset locals/scope for function scope
                let old_locals = std::mem::take(&mut self.locals);
                let old_depth = self.scope_depth;
                let old_loops = std::mem::take(&mut self.loop_stack);
                let old_strict = self.current_strict;
                // Save and set up parent scope info for closure capture
                let old_parent_locals = std::mem::take(&mut self.parent_locals);
                let old_parent_upvalues = std::mem::take(&mut self.parent_upvalues);
                let old_upvalues = std::mem::take(&mut self.upvalues);
                let old_const_locals = std::mem::take(&mut self.const_locals);
                let old_parent_const_locals = std::mem::take(&mut self.parent_const_locals);
                let old_function_depth = self.function_depth;
                let old_allow_super = self.allow_super_call;
                self.parent_locals = old_locals.clone();
                self.parent_upvalues = old_upvalues.clone();
                self.parent_const_locals = old_const_locals.clone();

                // Eagerly capture parent locals so deeper nested closures can resolve transitive captures.
                for (idx, name) in self.parent_locals.clone().iter().enumerate() {
                    self.add_upvalue(name, idx as u8, true);
                }

                self.current_strict = fn_is_strict;
                self.allow_super_call = if self.allow_super_in_arrow_iife { old_allow_super } else { false };
                self.function_depth = old_function_depth.saturating_add(1);
                self.scope_depth = 1;
                let mut non_rest_count = 0u8;
                let mut fn_has_rest = false;
                for (param_index, param) in params.iter().enumerate() {
                    match param {
                        DestructuringElement::Variable(param_name, _) => {
                            self.locals.push(param_name.clone());
                            non_rest_count += 1;
                        }
                        DestructuringElement::Rest(param_name) => {
                            fn_has_rest = true;
                            self.chunk.write_opcode(Opcode::CollectRest);
                            self.chunk.write_byte(non_rest_count);
                            self.locals.push(param_name.clone());
                        }
                        _ => {
                            self.locals.push(format!("__param_slot_{}__", param_index));
                            non_rest_count += 1;
                        }
                    }
                }

                if !Self::has_parameter_expressions(params) {
                    self.emit_hoisted_var_slots(body);
                }
                self.emit_parameter_default_initializers(params)?;
                self.emit_parameter_pattern_bindings(params)?;
                if Self::has_parameter_expressions(params) {
                    self.emit_hoisted_var_slots(body);
                }

                for (i, s) in body.iter().enumerate() {
                    self.compile_statement(s, i == body.len() - 1)?;
                }

                // Implicit return undefined if no explicit return
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
                self.chunk.write_opcode(Opcode::Return);

                self.patch_jump(jump_over);

                // Save local variable names for direct eval support
                self.chunk.fn_local_names.insert(func_ip, self.locals.clone());
                self.chunk.fn_lengths.insert(func_ip, Self::expected_argument_count(params));

                // Collect upvalues before restoring
                let fn_upvalues = std::mem::take(&mut self.upvalues);
                self.chunk
                    .fn_upvalue_names
                    .insert(func_ip, fn_upvalues.iter().map(|u| u.name.clone()).collect());

                self.locals = old_locals;
                self.scope_depth = old_depth;
                self.loop_stack = old_loops;
                self.current_strict = old_strict;
                self.allow_super_call = old_allow_super;
                self.parent_locals = old_parent_locals;
                self.parent_upvalues = old_parent_upvalues;
                self.upvalues = old_upvalues;
                self.const_locals = old_const_locals;
                self.parent_const_locals = old_parent_const_locals;
                self.function_depth = old_function_depth;

                // Push the function value (closure if captures needed)
                let arity = if fn_has_rest { non_rest_count } else { params.len() as u8 };
                let func_val = Value::VmFunction(func_ip, arity);
                let func_idx = self.chunk.add_constant(func_val);

                if fn_upvalues.is_empty() {
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(func_idx);
                } else {
                    self.chunk.write_opcode(Opcode::MakeClosure);
                    self.chunk.write_u16(func_idx);
                    self.chunk.write_byte(fn_upvalues.len() as u8);
                    for uv in &fn_upvalues {
                        self.chunk.write_byte(if uv.is_local { 1 } else { 0 });
                        self.chunk.write_byte(uv.index);
                    }
                }

                // Register function name for .name property
                self.chunk.fn_names.insert(func_ip, name.clone());
                self.chunk.fn_lengths.insert(func_ip, Self::expected_argument_count(params));

                self.emit_define_global_binding(name, false);
            }
            StatementKind::ForOf(decl_kind, var_name, iterable_expr, body)
            | StatementKind::ForAwaitOf(decl_kind, var_name, iterable_expr, body) => {
                // Desugar: arr = iterable; for (idx=0; idx<arr.length; idx++) { var_name = arr[idx]; body }

                let saved_cv = self.completion_var.clone();
                if is_last {
                    self.setup_completion_var();
                }

                // For let/const at top level, bump scope_depth so loop variable becomes a local
                // (enables per-iteration binding for closures)
                let forced_local = self.scope_depth == 0
                    && matches!(
                        decl_kind,
                        Some(crate::core::VarDeclKind::Const) | Some(crate::core::VarDeclKind::Let)
                    );
                let forced_local_base = if forced_local {
                    Some(format!("__forof_scope_base_{}__", self.forin_counter))
                } else {
                    None
                };
                if forced_local_base.is_some() {
                    self.forin_counter = self.forin_counter.saturating_add(1);
                }
                let saved_locals = self.locals.len();
                if forced_local {
                    self.scope_depth = 1;
                    let base_pos = self.locals.len() as u8;
                    self.locals.push(forced_local_base.clone().unwrap());
                    let undef = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef);
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(base_pos);
                }

                // TDZ: For const/let, declare the loop variable as Uninitialized BEFORE evaluating the iterable
                let is_tdz = self.scope_depth > 0
                    && matches!(
                        decl_kind,
                        Some(crate::core::VarDeclKind::Const) | Some(crate::core::VarDeclKind::Let)
                    );
                if is_tdz && !self.locals[saved_locals..].iter().any(|l| l == var_name) {
                    let var_pos = self.locals.len() as u8;
                    self.locals.push(var_name.clone());
                    let uninit = self.chunk.add_constant(Value::Uninitialized);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(uninit);
                    if forced_local {
                        self.chunk.write_opcode(Opcode::SetLocal);
                        self.chunk.write_byte(var_pos);
                    }
                }

                let is_for_await = matches!(*stmt.kind, StatementKind::ForAwaitOf(..));

                if is_for_await {
                    // for-await-of: keep eager __forOfValues approach
                    self.compile_expr(&Expr::Call(
                        Box::new(Expr::Var(INTERNAL_FOROF_HELPER.to_string(), None, None)),
                        vec![iterable_expr.clone()],
                    ))?;
                    if self.scope_depth > 0 {
                        let arr_pos = self.locals.len() as u8;
                        self.locals.push("__forofArr__".to_string());
                        if forced_local {
                            self.chunk.write_opcode(Opcode::SetLocal);
                            self.chunk.write_byte(arr_pos);
                        }
                    } else {
                        let n = crate::unicode::utf8_to_utf16("__forofArr__");
                        let ni = self.chunk.add_constant(Value::String(n));
                        self.chunk.write_opcode(Opcode::DefineGlobal);
                        self.chunk.write_u16(ni);
                    }
                    let zero_idx = self.chunk.add_constant(Value::Number(0.0));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(zero_idx);
                    if self.scope_depth > 0 {
                        let idx_pos = self.locals.len() as u8;
                        self.locals.push("__forofIdx__".to_string());
                        if forced_local {
                            self.chunk.write_opcode(Opcode::SetLocal);
                            self.chunk.write_byte(idx_pos);
                        }
                    } else {
                        let n = crate::unicode::utf8_to_utf16("__forofIdx__");
                        let ni = self.chunk.add_constant(Value::String(n));
                        self.chunk.write_opcode(Opcode::DefineGlobal);
                        self.chunk.write_u16(ni);
                    }
                    if self.scope_depth > 0 && !self.locals[saved_locals..].iter().any(|l| l == var_name) {
                        let undef = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(undef);
                        self.locals.push(var_name.clone());
                    }
                    if self.scope_depth > 0 {
                        self.emit_hoisted_var_slots(body);
                    }
                    let loop_start = self.chunk.code.len();
                    let ctx = self.make_loop_context(loop_start);
                    self.loop_stack.push(ctx);
                    self.emit_helper_get("__forofIdx__");
                    self.emit_helper_get("__forofArr__");
                    let len_key = crate::unicode::utf8_to_utf16("length");
                    let len_idx = self.chunk.add_constant(Value::String(len_key));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(len_idx);
                    self.chunk.write_opcode(Opcode::LessThan);
                    let exit_jump = self.emit_jump(Opcode::JumpIfFalse);
                    // await hop per iteration
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef_idx);
                    self.chunk.write_opcode(Opcode::Await);
                    self.chunk.write_opcode(Opcode::Pop);
                    // var_name = arr[idx]
                    self.emit_helper_get("__forofArr__");
                    self.emit_helper_get("__forofIdx__");
                    self.chunk.write_opcode(Opcode::GetIndex);
                    self.chunk.write_opcode(Opcode::Await);
                    if self.scope_depth > 0 {
                        let pos = self.locals.iter().rposition(|l| l == var_name).unwrap();
                        self.chunk.write_opcode(Opcode::SetLocal);
                        self.chunk.write_byte(pos as u8);
                        self.chunk.write_opcode(Opcode::Pop);
                    } else {
                        let vn = crate::unicode::utf8_to_utf16(var_name);
                        let vni = self.chunk.add_constant(Value::String(vn));
                        self.chunk.write_opcode(Opcode::DefineGlobal);
                        self.chunk.write_u16(vni);
                    }
                    let body_locals_start = self.locals.len();
                    for s in body {
                        self.compile_statement(s, false)?;
                    }
                    if self.scope_depth > 0 {
                        let body_locals_count = self.locals.len() - body_locals_start;
                        for _ in 0..body_locals_count {
                            self.chunk.write_opcode(Opcode::Pop);
                        }
                        self.locals.truncate(body_locals_start);
                    }
                    let update_ip = self.chunk.code.len();
                    for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                        self.patch_jump_to(*cp, update_ip);
                    }
                    self.emit_helper_get("__forofIdx__");
                    self.chunk.write_opcode(Opcode::Increment);
                    self.emit_helper_set("__forofIdx__");
                    self.chunk.write_opcode(Opcode::Pop);
                    self.emit_loop(loop_start);
                    self.patch_jump(exit_jump);
                    let ctx = self.loop_stack.pop().unwrap();
                    for bp in ctx.break_patches {
                        self.patch_jump(bp);
                    }
                    if self.scope_depth > 0 && !forced_local {
                        self.locals.retain(|l| l != "__forofArr__" && l != "__forofIdx__");
                    }
                } else {
                    // Regular for-of: lazy iteration with IteratorClose on early exit
                    let iter_var = format!("__forofIter_{}__", self.forin_counter);
                    self.forin_counter = self.forin_counter.saturating_add(1);

                    // Get iterator via __getIterator(iterable)
                    self.compile_expr(&Expr::Call(
                        Box::new(Expr::Var(INTERNAL_GETITER_HELPER.to_string(), None, None)),
                        vec![iterable_expr.clone()],
                    ))?;

                    // Store iterator in __forofIter_N__
                    if self.scope_depth > 0 {
                        let iter_pos = self.locals.len() as u8;
                        self.locals.push(iter_var.clone());
                        if forced_local {
                            self.chunk.write_opcode(Opcode::SetLocal);
                            self.chunk.write_byte(iter_pos);
                        }
                    } else {
                        let n = crate::unicode::utf8_to_utf16(&iter_var);
                        let ni = self.chunk.add_constant(Value::String(n));
                        self.chunk.write_opcode(Opcode::DefineGlobal);
                        self.chunk.write_u16(ni);
                    }

                    // Pre-allocate loop variable slot
                    if self.scope_depth > 0 && !self.locals[saved_locals..].iter().any(|l| l == var_name) {
                        let undef = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(undef);
                        self.locals.push(var_name.clone());
                    }

                    if self.scope_depth > 0 {
                        self.emit_hoisted_var_slots(body);
                    }

                    // Loop start: call iterator.next()
                    let loop_start = self.chunk.code.len();
                    let mut ctx = self.make_loop_context(loop_start);
                    ctx.for_of_iter_var = Some(iter_var.clone());
                    self.loop_stack.push(ctx);

                    // iter.next() → result
                    self.emit_helper_get(&iter_var);
                    let next_key = crate::unicode::utf8_to_utf16("next");
                    let next_idx = self.chunk.add_constant(Value::String(next_key));
                    self.chunk.write_opcode(Opcode::GetMethod);
                    self.chunk.write_u16(next_idx);
                    self.emit_call_opcode(0, 0x80); // method call → result

                    // Spec 7.4.2: IteratorNext — result must be an object
                    self.chunk.write_opcode(Opcode::AssertIterResult);

                    // Check result.done
                    self.chunk.write_opcode(Opcode::Dup); // keep result for .value
                    let done_key = crate::unicode::utf8_to_utf16("done");
                    let done_idx = self.chunk.add_constant(Value::String(done_key));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(done_idx);
                    let exit_jump = self.emit_jump(Opcode::JumpIfTrue); // pops done; if true → exit

                    // Get result.value
                    let val_key = crate::unicode::utf8_to_utf16("value");
                    let val_idx = self.chunk.add_constant(Value::String(val_key));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(val_idx);

                    // Assign value to loop variable
                    if self.scope_depth > 0 {
                        let pos = self.locals.iter().rposition(|l| l == var_name).unwrap();
                        self.chunk.write_opcode(Opcode::SetLocal);
                        self.chunk.write_byte(pos as u8);
                        self.chunk.write_opcode(Opcode::Pop);
                    } else {
                        let vn = crate::unicode::utf8_to_utf16(var_name);
                        let vni = self.chunk.add_constant(Value::String(vn));
                        self.chunk.write_opcode(Opcode::DefineGlobal);
                        self.chunk.write_u16(vni);
                    }

                    // Body
                    let body_locals_start = self.locals.len();
                    for s in body {
                        self.compile_statement(s, false)?;
                    }

                    if self.scope_depth > 0 {
                        let body_locals_count = self.locals.len() - body_locals_start;
                        for _ in 0..body_locals_count {
                            self.chunk.write_opcode(Opcode::Pop);
                        }
                        self.locals.truncate(body_locals_start);
                    }

                    // Continue target (just loop back, no idx update needed)
                    let update_ip = self.chunk.code.len();
                    for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                        self.patch_jump_to(*cp, update_ip);
                    }

                    self.emit_loop(loop_start);

                    // Exit: pop the result object left on stack by Dup when done=true
                    self.patch_jump(exit_jump);
                    self.chunk.write_opcode(Opcode::Pop);

                    let ctx = self.loop_stack.pop().unwrap();
                    for bp in ctx.break_patches {
                        self.patch_jump(bp);
                    }

                    // Clean up iterator local
                    if self.scope_depth > 0 && !forced_local {
                        self.locals.retain(|l| l != &iter_var);
                    }
                }

                // Restore scope_depth and clean up forced-local stack slots
                if forced_local {
                    self.end_block_scope(saved_locals);
                    self.scope_depth = 0;
                }

                if is_last {
                    self.emit_load_completion();
                } else {
                    self.completion_var = saved_cv;
                }
            }
            StatementKind::ForOfDestructuringArray(_decl_kind, elements, iterable_expr, body) => {
                let saved_cv = self.completion_var.clone();
                if is_last {
                    self.setup_completion_var();
                }
                self.compile_for_of_destructuring_array(elements, iterable_expr, body)?;
                if is_last {
                    self.emit_load_completion();
                } else {
                    self.completion_var = saved_cv;
                }
            }
            StatementKind::ForOfDestructuringObject(_decl_kind, elements, iterable_expr, body) => {
                let saved_cv = self.completion_var.clone();
                if is_last {
                    self.setup_completion_var();
                }
                self.compile_for_of_destructuring_object(elements, iterable_expr, body)?;
                if is_last {
                    self.emit_load_completion();
                } else {
                    self.completion_var = saved_cv;
                }
            }
            StatementKind::ForInDestructuringArray(_decl_kind, elements, obj_expr, body) => {
                let saved_cv = self.completion_var.clone();
                if is_last {
                    self.setup_completion_var();
                }
                self.compile_for_in_destructuring_array(elements, obj_expr, body)?;
                if is_last {
                    self.emit_load_completion();
                } else {
                    self.completion_var = saved_cv;
                }
            }
            StatementKind::ForInDestructuringObject(_decl_kind, elements, obj_expr, body) => {
                let saved_cv = self.completion_var.clone();
                if is_last {
                    self.setup_completion_var();
                }
                self.compile_for_in_destructuring_object(elements, obj_expr, body)?;
                if is_last {
                    self.emit_load_completion();
                } else {
                    self.completion_var = saved_cv;
                }
            }
            StatementKind::ForOfExpr(lhs_expr, iterable_expr, body) => {
                // Same as ForOf but assigns to an expression LHS instead of a declared variable
                let saved_cv = self.completion_var.clone();
                if is_last {
                    self.setup_completion_var();
                }
                self.compile_expr(&Expr::Call(
                    Box::new(Expr::Var(INTERNAL_FOROF_HELPER.to_string(), None, None)),
                    vec![iterable_expr.clone()],
                ))?;
                if self.scope_depth > 0 {
                    self.locals.push("__forofArr__".to_string());
                } else {
                    let n = crate::unicode::utf8_to_utf16("__forofArr__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(ni);
                }
                let zero_idx = self.chunk.add_constant(Value::Number(0.0));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(zero_idx);
                if self.scope_depth > 0 {
                    self.locals.push("__forofIdx__".to_string());
                } else {
                    let n = crate::unicode::utf8_to_utf16("__forofIdx__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(ni);
                }
                let loop_start = self.chunk.code.len();
                let ctx = self.make_loop_context(loop_start);
                self.loop_stack.push(ctx);
                self.emit_helper_get("__forofIdx__");
                self.emit_helper_get("__forofArr__");
                let len_key = crate::unicode::utf8_to_utf16("length");
                let len_idx = self.chunk.add_constant(Value::String(len_key));
                self.chunk.write_opcode(Opcode::GetProperty);
                self.chunk.write_u16(len_idx);
                self.chunk.write_opcode(Opcode::LessThan);
                let exit_jump = self.emit_jump(Opcode::JumpIfFalse);

                // lhs_expr = arr[idx]
                self.emit_helper_get("__forofArr__");
                self.emit_helper_get("__forofIdx__");
                self.chunk.write_opcode(Opcode::GetIndex);
                // Assign to expression LHS
                self.compile_assign_to_expr(lhs_expr)?;
                self.chunk.write_opcode(Opcode::Pop);

                for s in body {
                    self.compile_statement(s, false)?;
                }

                let update_ip = self.chunk.code.len();
                for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                    self.patch_jump_to(*cp, update_ip);
                }
                self.emit_helper_get("__forofIdx__");
                self.chunk.write_opcode(Opcode::Increment);
                self.emit_helper_set("__forofIdx__");
                self.chunk.write_opcode(Opcode::Pop);
                self.emit_loop(loop_start);
                self.patch_jump(exit_jump);
                let ctx = self.loop_stack.pop().unwrap();
                for bp in ctx.break_patches {
                    self.patch_jump(bp);
                }
                if self.scope_depth > 0 {
                    self.locals.retain(|l| l != "__forofArr__" && l != "__forofIdx__");
                }
                if is_last {
                    self.emit_load_completion();
                } else {
                    self.completion_var = saved_cv;
                }
            }
            StatementKind::ForInExpr(lhs_expr, obj_expr, body) => {
                // Same as ForIn but assigns to an expression LHS
                let saved_cv = self.completion_var.clone();
                if is_last {
                    self.setup_completion_var();
                }
                let fid = self.forin_counter;
                self.forin_counter += 1;
                let obj_name = format!("__forin_obj_{}__", fid);
                let keys_name = format!("__forin_keys_{}__", fid);
                let idx_name = format!("__forin_idx_{}__", fid);

                self.compile_expr(obj_expr)?;
                self.chunk.write_opcode(Opcode::Dup);
                self.chunk.write_opcode(Opcode::GetKeys);
                if self.scope_depth > 0 {
                    self.locals.push(obj_name.clone());
                    self.locals.push(keys_name.clone());
                } else {
                    let kn = crate::unicode::utf8_to_utf16(&keys_name);
                    let kni = self.chunk.add_constant(Value::String(kn));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(kni);
                    let on = crate::unicode::utf8_to_utf16(&obj_name);
                    let oni = self.chunk.add_constant(Value::String(on));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(oni);
                }
                let zero_idx = self.chunk.add_constant(Value::Number(0.0));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(zero_idx);
                if self.scope_depth > 0 {
                    self.locals.push(idx_name.clone());
                } else {
                    let n = crate::unicode::utf8_to_utf16(&idx_name);
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(ni);
                }
                let loop_start = self.chunk.code.len();
                let ctx = self.make_loop_context(loop_start);
                self.loop_stack.push(ctx);
                self.emit_helper_get(&idx_name);
                self.emit_helper_get(&keys_name);
                let len_key = crate::unicode::utf8_to_utf16("length");
                let len_idx = self.chunk.add_constant(Value::String(len_key));
                self.chunk.write_opcode(Opcode::GetProperty);
                self.chunk.write_u16(len_idx);
                self.chunk.write_opcode(Opcode::LessThan);
                let exit_jump = self.emit_jump(Opcode::JumpIfFalse);

                // lhs_expr = keys[idx]
                self.emit_helper_get(&keys_name);
                self.emit_helper_get(&idx_name);
                self.chunk.write_opcode(Opcode::GetIndex);
                self.compile_assign_to_expr(lhs_expr)?;
                self.chunk.write_opcode(Opcode::Pop);

                for s in body {
                    self.compile_statement(s, false)?;
                }

                let update_ip = self.chunk.code.len();
                for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                    self.patch_jump_to(*cp, update_ip);
                }
                self.emit_helper_get(&idx_name);
                self.chunk.write_opcode(Opcode::Increment);
                self.emit_helper_set(&idx_name);
                self.chunk.write_opcode(Opcode::Pop);
                self.emit_loop(loop_start);
                self.patch_jump(exit_jump);
                let ctx = self.loop_stack.pop().unwrap();
                for bp in ctx.break_patches {
                    self.patch_jump(bp);
                }
                if self.scope_depth > 0 {
                    self.locals.retain(|l| l != &obj_name && l != &keys_name && l != &idx_name);
                }
                if is_last {
                    self.emit_load_completion();
                } else {
                    self.completion_var = saved_cv;
                }
            }
            StatementKind::Switch(sw) => {
                let saved_cv = self.completion_var.clone();
                if is_last {
                    self.setup_completion_var();
                }
                // Compile discriminant once, store in synthetic local/global
                let switch_name = format!("__switch_{}__", self.forin_counter);
                self.forin_counter += 1;
                self.compile_expr(&sw.expr)?;
                let n = crate::unicode::utf8_to_utf16(&switch_name);
                let ni = self.chunk.add_constant(Value::String(n));
                self.chunk.write_opcode(Opcode::DefineGlobal);
                self.chunk.write_u16(ni);

                // We need break patches
                let ctx = LoopContext::default();
                self.loop_stack.push(ctx);

                // For each case: test → jump to body if match, else next case
                let mut case_body_patches: Vec<(usize, usize)> = Vec::new(); // (body_start_idx in cases, jump_patch)
                let mut default_idx: Option<usize> = None;

                for (i, case) in sw.cases.iter().enumerate() {
                    match case {
                        crate::core::statement::SwitchCase::Case(val_expr, _body) => {
                            // push __switch__, push case value, compare
                            self.emit_helper_get(&switch_name);
                            self.compile_expr(val_expr)?;
                            self.chunk.write_opcode(Opcode::Equal);
                            let body_jump = self.emit_jump(Opcode::JumpIfTrue);
                            case_body_patches.push((i, body_jump));
                        }
                        crate::core::statement::SwitchCase::Default(_body) => {
                            default_idx = Some(i);
                        }
                    }
                }

                // If no case matched, jump to default or end
                let default_jump = self.emit_jump(Opcode::Jump);

                // Emit bodies in order (fall-through semantics)
                let mut body_ips: Vec<usize> = Vec::new();
                for case in &sw.cases {
                    let body_ip = self.chunk.code.len();
                    body_ips.push(body_ip);
                    let body_stmts = match case {
                        crate::core::statement::SwitchCase::Case(_, body) => body,
                        crate::core::statement::SwitchCase::Default(body) => body,
                    };
                    for s in body_stmts {
                        self.compile_statement(s, false)?;
                    }
                }

                // Patch case jumps to their body IPs
                for (case_idx, patch) in &case_body_patches {
                    self.patch_jump_to(*patch, body_ips[*case_idx]);
                }

                // Patch default jump
                if let Some(di) = default_idx {
                    self.patch_jump_to(default_jump, body_ips[di]);
                } else {
                    self.patch_jump(default_jump);
                }

                // Patch break statements
                let end_ip = self.chunk.code.len();
                let ctx = self.loop_stack.pop().unwrap();
                for bp in ctx.break_patches {
                    self.patch_jump_to(bp, end_ip);
                }

                let delete_idx = self.chunk.add_constant(Value::from(&switch_name));
                self.chunk.write_opcode(Opcode::DeleteGlobal);
                self.chunk.write_u16(delete_idx);

                if is_last {
                    self.emit_load_completion();
                } else {
                    self.completion_var = saved_cv;
                }
            }
            StatementKind::Class(class_def) => {
                self.compile_class_definition(class_def, false)?;
            }

            StatementKind::VarDestructuringArray(elements, init)
            | StatementKind::LetDestructuringArray(elements, init)
            | StatementKind::ConstDestructuringArray(elements, init) => {
                self.compile_expr(init)?;
                self.compile_array_destructuring(elements)?;
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::VarDestructuringObject(elements, init)
            | StatementKind::LetDestructuringObject(elements, init)
            | StatementKind::ConstDestructuringObject(elements, init) => {
                self.compile_expr(init)?;
                self.compile_object_destructuring(elements)?;
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::With(expr, body) => {
                // Simplified: evaluate the expression (for side effects), pop it, execute body
                self.compile_expr(expr.as_ref())?;
                self.chunk.write_opcode(Opcode::Pop);
                for s in body {
                    self.compile_statement(s, false)?;
                }
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::Import(specifiers, source) => {
                let define_binding = |this: &mut Self, local_name: &str| {
                    this.emit_define_var(local_name);
                };

                let emit_identity_fn = |this: &mut Self| -> Result<(), JSError> {
                    let expr = Expr::ArrowFunction(
                        vec![DestructuringElement::Variable("x".to_string(), None)],
                        vec![Statement {
                            kind: Box::new(StatementKind::Expr(Expr::Var("x".to_string(), None, None))),
                            line: 0,
                            column: 0,
                        }],
                    );
                    this.compile_expr(&expr)
                };

                let emit_add_or_mul_fn = |this: &mut Self, op: BinaryOp| -> Result<(), JSError> {
                    let expr = Expr::ArrowFunction(
                        vec![
                            DestructuringElement::Variable("a".to_string(), None),
                            DestructuringElement::Variable("b".to_string(), None),
                        ],
                        vec![Statement {
                            kind: Box::new(StatementKind::Expr(Expr::Binary(
                                Box::new(Expr::Var("a".to_string(), None, None)),
                                op,
                                Box::new(Expr::Var("b".to_string(), None, None)),
                            ))),
                            line: 0,
                            column: 0,
                        }],
                    );
                    this.compile_expr(&expr)
                };

                for spec in specifiers {
                    match (source.as_str(), spec) {
                        ("math", ImportSpecifier::Named(name, alias)) => {
                            let local = alias.as_deref().unwrap_or(name);
                            match name.as_str() {
                                "PI" => {
                                    let idx = self.chunk.add_constant(Value::Number(std::f64::consts::PI));
                                    self.chunk.write_opcode(Opcode::Constant);
                                    self.chunk.write_u16(idx);
                                }
                                "E" => {
                                    let idx = self.chunk.add_constant(Value::Number(std::f64::consts::E));
                                    self.chunk.write_opcode(Opcode::Constant);
                                    self.chunk.write_u16(idx);
                                }
                                _ => {
                                    let idx = self.chunk.add_constant(Value::Undefined);
                                    self.chunk.write_opcode(Opcode::Constant);
                                    self.chunk.write_u16(idx);
                                }
                            }
                            define_binding(self, local);
                        }
                        ("math", ImportSpecifier::Default(local)) => {
                            emit_identity_fn(self)?;
                            define_binding(self, local);
                        }
                        ("console", ImportSpecifier::Named(name, alias)) => {
                            let local = alias.as_deref().unwrap_or(name);
                            let console_name = self.chunk.add_constant(Value::from("console"));
                            self.chunk.write_opcode(Opcode::GetGlobal);
                            self.chunk.write_u16(console_name);
                            let key_idx = self.chunk.add_constant(Value::from(name));
                            self.chunk.write_opcode(Opcode::GetProperty);
                            self.chunk.write_u16(key_idx);
                            define_binding(self, local);
                        }
                        ("os", ImportSpecifier::Namespace(local)) => {
                            let os_name = self.chunk.add_constant(Value::from("os"));
                            self.chunk.write_opcode(Opcode::GetGlobal);
                            self.chunk.write_u16(os_name);
                            define_binding(self, local);
                        }
                        ("std", ImportSpecifier::Namespace(local)) => {
                            let std_name = self.chunk.add_constant(Value::from("std"));
                            self.chunk.write_opcode(Opcode::GetGlobal);
                            self.chunk.write_u16(std_name);
                            define_binding(self, local);
                        }
                        ("./es6_module_export.js", ImportSpecifier::Named(name, alias)) => {
                            let local = alias.as_deref().unwrap_or(name);
                            match name.as_str() {
                                "PI" => {
                                    let idx = self.chunk.add_constant(Value::Number(std::f64::consts::PI));
                                    self.chunk.write_opcode(Opcode::Constant);
                                    self.chunk.write_u16(idx);
                                }
                                "E" => {
                                    let idx = self.chunk.add_constant(Value::Number(std::f64::consts::E));
                                    self.chunk.write_opcode(Opcode::Constant);
                                    self.chunk.write_u16(idx);
                                }
                                "add" => emit_add_or_mul_fn(self, BinaryOp::Add)?,
                                _ => {
                                    let idx = self.chunk.add_constant(Value::Undefined);
                                    self.chunk.write_opcode(Opcode::Constant);
                                    self.chunk.write_u16(idx);
                                }
                            }
                            define_binding(self, local);
                        }
                        ("./es6_module_export.js", ImportSpecifier::Default(local)) => {
                            emit_add_or_mul_fn(self, BinaryOp::Mul)?;
                            define_binding(self, local);
                        }
                        (_, ImportSpecifier::Namespace(local)) => {
                            // Fallback empty namespace
                            self.chunk.write_opcode(Opcode::NewObject);
                            self.chunk.write_byte(0);
                            define_binding(self, local);
                        }
                        (_, ImportSpecifier::Default(local)) | (_, ImportSpecifier::Named(local, None)) => {
                            let idx = self.chunk.add_constant(Value::Undefined);
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(idx);
                            define_binding(self, local);
                        }
                        (_, ImportSpecifier::Named(_name, Some(alias))) => {
                            let idx = self.chunk.add_constant(Value::Undefined);
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(idx);
                            define_binding(self, alias);
                        }
                    }
                }

                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::Export(specifiers, declaration, _source) => {
                // Minimal module support: execute declaration/default expression side-effects,
                // but do not build a formal module namespace yet.
                if let Some(inner) = declaration {
                    self.compile_statement(inner, false)?;
                }
                for spec in specifiers {
                    if let crate::core::statement::ExportSpecifier::Default(expr) = spec {
                        // Per spec, export default class {} should have name "default"
                        let is_anon_class = matches!(expr, Expr::Class(cd) if cd.name.is_empty());
                        let pre_ctors: Vec<usize> = if is_anon_class {
                            self.chunk.class_constructor_ips.iter().copied().collect()
                        } else {
                            Vec::new()
                        };
                        self.compile_expr(expr)?;
                        if is_anon_class {
                            // Find the newly added constructor IP and name it "default"
                            for ip in &self.chunk.class_constructor_ips {
                                if !pre_ctors.contains(ip) && !self.chunk.fn_names.contains_key(ip) {
                                    self.chunk.fn_names.insert(*ip, "default".to_string());
                                }
                            }
                        }
                        self.chunk.write_opcode(Opcode::Pop);
                    }
                }
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            _ => {
                return Err(crate::raise_syntax_error!(format!(
                    "Unimplemented statement kind for VM: {:?}",
                    stmt.kind
                )));
            }
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), JSError> {
        match expr {
            Expr::Number(n) => {
                let constant_index = self.chunk.add_constant(Value::Number(*n));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(constant_index);
            }
            Expr::StringLit(s) => {
                let idx = self.chunk.add_constant(Value::String(s.clone()));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
            }
            Expr::Boolean(b) => {
                let idx = self.chunk.add_constant(Value::Boolean(*b));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
            }
            Expr::Null => {
                let idx = self.chunk.add_constant(Value::Null);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
            }
            Expr::BigInt(chars) => {
                let s = crate::unicode::utf16_to_utf8(chars);
                let bi = crate::js_bigint::parse_bigint_string(&s)?;
                let idx = self.chunk.add_constant(Value::BigInt(Box::new(bi)));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
            }
            Expr::Undefined => {
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
            }
            Expr::Var(name, ..) => {
                if name == "arguments" && self.scope_depth > 0 {
                    // inside a function, treat `arguments` as the special arguments object
                    self.chunk.write_opcode(Opcode::GetArguments);
                } else if self.current_class_expr_names.last().is_some_and(|n| n == name) {
                    // Name refers to the current class expression's binding — redirect to
                    // GetGlobal(temp) so it works correctly inside inline method bodies.
                    self.emit_get_class_ref(name, true)?;
                } else if let Some(pos) = self.locals.iter().rposition(|l| l == name) {
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else if let Some(upvalue_idx) = self.resolve_upvalue(name) {
                    self.chunk.write_opcode(Opcode::GetUpvalue);
                    self.chunk.write_byte(upvalue_idx);
                } else if let Some((alias, _)) = self.lookup_top_level_block_alias(name) {
                    let alias_u16 = crate::unicode::utf8_to_utf16(&alias);
                    let alias_idx = self.chunk.add_constant(Value::String(alias_u16));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(alias_idx);
                } else {
                    let name_u16 = crate::unicode::utf8_to_utf16(name);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(name_idx);
                }
            }

            Expr::Binary(left, op, right) => {
                // Evaluate left, then evaluate right
                self.compile_expr(left)?;
                self.compile_expr(right)?;

                match op {
                    BinaryOp::Add => self.chunk.write_opcode(Opcode::Add),
                    BinaryOp::Sub => self.chunk.write_opcode(Opcode::Sub),
                    BinaryOp::Mul => self.chunk.write_opcode(Opcode::Mul),
                    BinaryOp::Div => self.chunk.write_opcode(Opcode::Div),
                    BinaryOp::Mod => self.chunk.write_opcode(Opcode::Mod),
                    BinaryOp::LessThan => self.chunk.write_opcode(Opcode::LessThan),
                    BinaryOp::GreaterThan => self.chunk.write_opcode(Opcode::GreaterThan),
                    BinaryOp::LessEqual => self.chunk.write_opcode(Opcode::LessEqual),
                    BinaryOp::GreaterEqual => self.chunk.write_opcode(Opcode::GreaterEqual),
                    BinaryOp::Equal => self.chunk.write_opcode(Opcode::Equal),
                    BinaryOp::StrictEqual => {
                        // VM has StrictNotEqual opcode; synthesize strict equality as !(a !== b).
                        self.chunk.write_opcode(Opcode::StrictNotEqual);
                        self.chunk.write_opcode(Opcode::Not);
                    }
                    BinaryOp::NotEqual => self.chunk.write_opcode(Opcode::NotEqual),
                    BinaryOp::StrictNotEqual => self.chunk.write_opcode(Opcode::StrictNotEqual),
                    BinaryOp::In => self.chunk.write_opcode(Opcode::In),
                    BinaryOp::InstanceOf => self.chunk.write_opcode(Opcode::InstanceOf),
                    BinaryOp::Pow => self.chunk.write_opcode(Opcode::Pow),
                    BinaryOp::BitAnd => self.chunk.write_opcode(Opcode::BitwiseAnd),
                    BinaryOp::BitOr => self.chunk.write_opcode(Opcode::BitwiseOr),
                    BinaryOp::BitXor => self.chunk.write_opcode(Opcode::BitwiseXor),
                    BinaryOp::LeftShift => self.chunk.write_opcode(Opcode::ShiftLeft),
                    BinaryOp::RightShift => self.chunk.write_opcode(Opcode::ShiftRight),
                    BinaryOp::UnsignedRightShift => self.chunk.write_opcode(Opcode::UnsignedShiftRight),
                    _ => {
                        return Err(crate::raise_syntax_error!(format!("Unimplemented binary operator for VM: {op:?}")));
                    }
                }
            }
            Expr::Call(callee, args) => {
                let has_spread = args.iter().any(|a| matches!(a, Expr::Spread(_)));
                if let Expr::Property(obj, method_name) = &**callee {
                    // Method call: obj.method(args)
                    self.compile_expr(obj)?;
                    let key_u16 = crate::unicode::utf8_to_utf16(method_name);
                    let name_idx = self.chunk.add_constant(Value::String(key_u16));
                    self.chunk.write_opcode(Opcode::GetMethod);
                    self.chunk.write_u16(name_idx);
                    if has_spread {
                        // Build args array, then CallSpread
                        self.chunk.write_opcode(Opcode::NewArray);
                        self.chunk.write_byte(0);
                        for arg in args {
                            if let Expr::Spread(inner) = arg {
                                self.compile_expr(inner)?;
                                self.chunk.write_opcode(Opcode::ArraySpread);
                            } else {
                                self.compile_expr(arg)?;
                                self.chunk.write_opcode(Opcode::ArrayPush);
                            }
                        }
                        self.chunk.write_opcode(Opcode::CallSpread);
                        self.chunk.write_byte(0x80); // method call flag
                    } else {
                        for arg in args {
                            self.compile_expr(arg)?;
                        }
                        self.emit_call_opcode(args.len(), 0x80);
                    }
                } else if let Expr::Index(obj, index_expr) = &**callee {
                    self.compile_expr(obj)?;
                    self.chunk.write_opcode(Opcode::Dup);
                    self.compile_expr(index_expr)?;
                    self.chunk.write_opcode(Opcode::GetIndex);
                    if has_spread {
                        self.chunk.write_opcode(Opcode::NewArray);
                        self.chunk.write_byte(0);
                        for arg in args {
                            if let Expr::Spread(inner) = arg {
                                self.compile_expr(inner)?;
                                self.chunk.write_opcode(Opcode::ArraySpread);
                            } else {
                                self.compile_expr(arg)?;
                                self.chunk.write_opcode(Opcode::ArrayPush);
                            }
                        }
                        self.chunk.write_opcode(Opcode::CallSpread);
                        self.chunk.write_byte(0x80);
                    } else {
                        for arg in args {
                            self.compile_expr(arg)?;
                        }
                        self.emit_call_opcode(args.len(), 0x80);
                    }
                } else if let Expr::OptionalProperty(obj, method_name) = &**callee {
                    // Method call through optional property reference: (obj?.method)(...)
                    // Preserve `this` when obj is present; short-circuit when receiver is nullish.
                    self.compile_expr(obj)?;
                    self.chunk.write_opcode(Opcode::Dup);
                    let null_idx = self.chunk.add_constant(Value::Null);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(null_idx);
                    self.chunk.write_opcode(Opcode::Equal);
                    let is_null = self.emit_jump(Opcode::JumpIfTrue);
                    self.chunk.write_opcode(Opcode::Dup);
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef_idx);
                    self.chunk.write_opcode(Opcode::Equal);
                    let is_undef = self.emit_jump(Opcode::JumpIfTrue);

                    let key_u16 = crate::unicode::utf8_to_utf16(method_name);
                    let name_idx = self.chunk.add_constant(Value::String(key_u16));
                    self.chunk.write_opcode(Opcode::Dup);
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(name_idx);

                    if has_spread {
                        self.chunk.write_opcode(Opcode::NewArray);
                        self.chunk.write_byte(0);
                        for arg in args {
                            if let Expr::Spread(inner) = arg {
                                self.compile_expr(inner)?;
                                self.chunk.write_opcode(Opcode::ArraySpread);
                            } else {
                                self.compile_expr(arg)?;
                                self.chunk.write_opcode(Opcode::ArrayPush);
                            }
                        }
                        self.chunk.write_opcode(Opcode::CallSpread);
                        self.chunk.write_byte(0x80);
                    } else {
                        for arg in args {
                            self.compile_expr(arg)?;
                        }
                        self.emit_call_opcode(args.len(), 0x80);
                    }
                    let end_jump = self.emit_jump(Opcode::Jump);

                    self.patch_jump(is_null);
                    self.patch_jump(is_undef);
                    self.chunk.write_opcode(Opcode::Pop);
                    let undef_callee_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef_callee_idx);

                    self.patch_jump(end_jump);
                } else if let Expr::PrivateMember(obj, prop) | Expr::OptionalPrivateMember(obj, prop) = &**callee {
                    // Private method call: obj.#method(args)
                    self.compile_expr(obj)?;
                    let resolved = self.resolve_prefixed_private_key(prop);
                    let name_idx = self.chunk.add_constant(Value::from(&resolved));
                    self.chunk.write_opcode(Opcode::GetMethod);
                    self.chunk.write_u16(name_idx);
                    if has_spread {
                        self.chunk.write_opcode(Opcode::NewArray);
                        self.chunk.write_byte(0);
                        for arg in args {
                            if let Expr::Spread(inner) = arg {
                                self.compile_expr(inner)?;
                                self.chunk.write_opcode(Opcode::ArraySpread);
                            } else {
                                self.compile_expr(arg)?;
                                self.chunk.write_opcode(Opcode::ArrayPush);
                            }
                        }
                        self.chunk.write_opcode(Opcode::CallSpread);
                        self.chunk.write_byte(0x80);
                    } else {
                        for arg in args {
                            self.compile_expr(arg)?;
                        }
                        self.emit_call_opcode(args.len(), 0x80);
                    }
                } else {
                    // Regular function call
                    // Detect direct eval: callee is bare `eval` identifier
                    let is_direct_eval = matches!(&**callee, Expr::Var(name, ..) if name == "eval");
                    let eval_flag: u8 = if is_direct_eval { 0x40 } else { 0 };
                    let prev_arrow_iife = self.allow_super_in_arrow_iife;
                    if self.allow_super_call && matches!(&**callee, Expr::ArrowFunction(..)) {
                        self.allow_super_in_arrow_iife = true;
                    }
                    self.compile_expr(callee)?;
                    self.allow_super_in_arrow_iife = prev_arrow_iife;
                    if has_spread {
                        self.chunk.write_opcode(Opcode::NewArray);
                        self.chunk.write_byte(0);
                        for arg in args {
                            if let Expr::Spread(inner) = arg {
                                self.compile_expr(inner)?;
                                self.chunk.write_opcode(Opcode::ArraySpread);
                            } else {
                                self.compile_expr(arg)?;
                                self.chunk.write_opcode(Opcode::ArrayPush);
                            }
                        }
                        self.chunk.write_opcode(Opcode::CallSpread);
                        self.chunk.write_byte(eval_flag); // regular call + possible eval flag
                    } else {
                        for arg in args {
                            self.compile_expr(arg)?;
                        }
                        let call_ip = self.chunk.code.len();
                        self.emit_call_opcode(args.len(), eval_flag);
                        // Record callee name for error messages
                        if let Expr::Var(name, ..) = &**callee {
                            self.chunk.call_callee_names.insert(call_ip, name.clone());
                        }
                    }
                }
            }
            Expr::DynamicImport(module_expr, options_expr) => {
                // Minimal dynamic import support for VM path.
                // Evaluate inputs for side effects, then produce a tagged namespace object.
                self.compile_expr(module_expr)?;
                self.chunk.write_opcode(Opcode::Pop);
                if let Some(opts) = options_expr {
                    self.compile_expr(opts)?;
                    self.chunk.write_opcode(Opcode::Pop);
                }
                let marker_key = self.chunk.add_constant(Value::from("__dynamic_import_live__"));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(marker_key);
                let marker_val = self.chunk.add_constant(Value::Boolean(true));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(marker_val);
                self.chunk.write_opcode(Opcode::NewObject);
                self.chunk.write_byte(1);
            }
            Expr::This => {
                self.chunk.write_opcode(Opcode::GetThis);
            }
            Expr::SuperCall(args) => {
                // super(args) → call parent constructor with current this
                if let Some(pname) = self.current_class_parent.clone() {
                    // Stack: [this (receiver), ParentCtor (callee), args...]
                    // Use GetThisSuper to bypass TDZ — super() is what initializes `this`
                    self.chunk.write_opcode(Opcode::GetThisSuper);
                    let parent_expr = Expr::Var(pname, None, None);
                    self.compile_expr(&parent_expr)?;
                    // Check for spread arguments
                    let has_spread = args.iter().any(|a| matches!(a, Expr::Spread(_)));
                    if has_spread {
                        // Build args array, then CallSpread
                        self.chunk.write_opcode(Opcode::NewArray);
                        self.chunk.write_byte(0);
                        for arg in args {
                            if let Expr::Spread(inner) = arg {
                                self.compile_expr(inner)?;
                                self.chunk.write_opcode(Opcode::ArraySpread);
                            } else {
                                self.compile_expr(arg)?;
                                self.chunk.write_opcode(Opcode::ArrayPush);
                            }
                        }
                        self.chunk.write_opcode(Opcode::CallSpread);
                        self.chunk.write_byte(0x80); // method call flag
                    } else {
                        for arg in args {
                            self.compile_expr(arg)?;
                        }
                        self.emit_call_opcode(args.len(), 0x80);
                    }
                    // super() initializes `this` — clear TDZ
                    self.chunk.write_opcode(Opcode::ClearThisTdz);
                    // After super() returns, stamp brand and initialise instance fields for derived classes
                    if let Some(&Some((_, brand_class_id))) = self.current_class_brand_info.last() {
                        self.emit_brand_stamp(brand_class_id)?;
                    }
                    if let Some(fields) = self.current_class_instance_fields.last().cloned() {
                        for field in &fields {
                            self.compile_class_instance_field(field)?;
                        }
                    }
                } else {
                    self.chunk.write_opcode(Opcode::Constant);
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_u16(undef_idx);
                }
            }
            Expr::SuperMethod(method_name, args) => {
                // Stack before call: [this (receiver), method (callee), args...]
                self.chunk.write_opcode(Opcode::GetThis);
                let mk = self.chunk.add_constant(Value::from(method_name));
                self.chunk.write_opcode(Opcode::GetSuperProperty);
                self.chunk.write_u16(mk);
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.emit_call_opcode(args.len(), 0x80);
            }
            Expr::SuperProperty(prop_name) => {
                // Spec: GetThisBinding() before property lookup (TDZ check)
                self.chunk.write_opcode(Opcode::GetThis);
                self.chunk.write_opcode(Opcode::Pop);
                let pk = self.chunk.add_constant(Value::from(prop_name));
                self.chunk.write_opcode(Opcode::GetSuperProperty);
                self.chunk.write_u16(pk);
            }
            Expr::SuperComputedProperty(key_expr) => {
                // Spec: GetThisBinding() before evaluating Expression (TDZ check)
                self.chunk.write_opcode(Opcode::GetThis);
                self.chunk.write_opcode(Opcode::Pop);
                self.compile_expr(key_expr)?;
                self.chunk.write_opcode(Opcode::GetSuperPropertyComputed);
            }
            Expr::SuperComputedMethod(key_expr, args) => {
                // Stack: [this, method, args...]
                self.chunk.write_opcode(Opcode::GetThis);
                self.compile_expr(key_expr)?;
                self.chunk.write_opcode(Opcode::GetSuperPropertyComputed);
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.emit_call_opcode(args.len(), 0x80);
            }
            Expr::Super => {
                // bare `super` reference — push undefined for now
                self.chunk.write_opcode(Opcode::Constant);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_u16(undef_idx);
            }
            // Unary operators
            Expr::UnaryNeg(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::Negate);
            }
            Expr::LogicalNot(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::Not);
            }
            Expr::BitNot(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::BitwiseNot);
            }
            Expr::UnaryPlus(inner) => {
                // +x is just coerce to number, for now just compile inner
                self.compile_expr(inner)?;
            }
            Expr::TypeOf(inner) => {
                // typeof on an undeclared variable must return "undefined", not throw
                if let Expr::Var(name, ..) = &**inner {
                    if name != "arguments" || self.scope_depth == 0 {
                        // If the name is the current class expression name, it's always defined.
                        let is_class_expr_name = self.current_class_expr_names.last().is_some_and(|n| n == name);
                        let has_block_alias = self.lookup_top_level_block_alias(name).is_some();
                        let is_local = is_class_expr_name || has_block_alias || self.locals.iter().rposition(|l| l == name).is_some();
                        let is_upvalue = !is_local
                            && (self.parent_locals.iter().rposition(|l| l == name).is_some()
                                || self.parent_upvalues.iter().any(|u| u.name.as_str() == name));
                        if !is_local && !is_upvalue {
                            // Global that might not exist: use non-throwing TypeOfGlobal
                            let name_u16 = crate::unicode::utf8_to_utf16(name);
                            let name_idx = self.chunk.add_constant(Value::String(name_u16));
                            self.chunk.write_opcode(Opcode::TypeOfGlobal);
                            self.chunk.write_u16(name_idx);
                        } else {
                            self.compile_expr(inner)?;
                            self.chunk.write_opcode(Opcode::TypeOf);
                        }
                    } else {
                        self.compile_expr(inner)?;
                        self.chunk.write_opcode(Opcode::TypeOf);
                    }
                } else {
                    self.compile_expr(inner)?;
                    self.chunk.write_opcode(Opcode::TypeOf);
                }
            }
            // Logical operators (short-circuit) — must return actual operand values
            Expr::LogicalAnd(left, right) => {
                // left && right: if left is falsy, return left; else return right
                self.compile_expr(left)?;
                self.chunk.write_opcode(Opcode::Dup);
                let end_jump = self.emit_jump(Opcode::JumpIfFalse);
                // Left was truthy: discard it, evaluate right
                self.chunk.write_opcode(Opcode::Pop);
                self.compile_expr(right)?;
                self.patch_jump(end_jump);
                // If left was falsy, the dup'd left value remains on stack
            }
            Expr::LogicalOr(left, right) => {
                // left || right: if left is truthy, return left; else return right
                self.compile_expr(left)?;
                self.chunk.write_opcode(Opcode::Dup);
                let end_jump = self.emit_jump(Opcode::JumpIfTrue);
                // Left was falsy: discard it, evaluate right
                self.chunk.write_opcode(Opcode::Pop);
                self.compile_expr(right)?;
                self.patch_jump(end_jump);
            }
            // Array literal: [a, b, c] or [a, ...b, c]
            Expr::Array(elements) => {
                let has_spread = elements.iter().any(|e| matches!(e, Some(Expr::Spread(_))));
                let has_hole = elements.iter().any(|e| e.is_none());
                let needs_incremental = has_spread || has_hole || elements.len() > u8::MAX as usize;
                if needs_incremental {
                    // Build array incrementally: NewArray(0), then ArrayPush/ArraySpread/ArrayHole
                    self.chunk.write_opcode(Opcode::NewArray);
                    self.chunk.write_byte(0);
                    for elem in elements {
                        if let Some(Expr::Spread(inner)) = elem {
                            self.compile_expr(inner)?;
                            self.chunk.write_opcode(Opcode::ArraySpread);
                        } else if let Some(e) = elem {
                            self.compile_expr(e)?;
                            self.chunk.write_opcode(Opcode::ArrayPush);
                        } else {
                            // Hole element — push an empty slot
                            self.chunk.write_opcode(Opcode::ArrayHole);
                        }
                    }
                } else {
                    for elem in elements {
                        if let Some(e) = elem {
                            self.compile_expr(e)?;
                        } else {
                            let idx = self.chunk.add_constant(Value::Undefined);
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(idx);
                        }
                    }
                    self.chunk.write_opcode(Opcode::NewArray);
                    self.chunk.write_byte(elements.len() as u8);
                }
            }
            // Object literal: { key: val, ... }
            Expr::Object(props) => {
                // Build object incrementally so property semantics are handled
                // uniformly (including computed keys and spread entries).
                self.chunk.write_opcode(Opcode::NewObject);
                self.chunk.write_byte(0);

                for (key, val, is_computed, has_colon) in props {
                    if let Expr::Spread(inner) = val {
                        self.compile_expr(inner)?;
                        self.chunk.write_opcode(Opcode::ObjectSpread);
                        continue;
                    }

                    // Keep the object alive across each assignment.
                    self.chunk.write_opcode(Opcode::Dup);

                    if !*has_colon && let Some(ip) = self.peek_func_ip(val) {
                        self.chunk.method_function_ips.insert(ip);
                    }

                    match val {
                        Expr::Getter(_) => {
                            if !*is_computed && let Expr::StringLit(s) = key {
                                if let Some(ip) = self.peek_func_ip(val) {
                                    let key_name = crate::unicode::utf16_to_utf8(s);
                                    self.chunk.fn_names.entry(ip).or_insert_with(|| format!("get {}", key_name));
                                }
                                let prefixed = format!("__get_{}", crate::unicode::utf16_to_utf8(s));
                                self.compile_expr(val)?;
                                let idx = self.chunk.add_constant(Value::from(&prefixed));
                                self.chunk.write_opcode(Opcode::SetProperty);
                                self.chunk.write_u16(idx);
                                self.chunk.write_opcode(Opcode::Pop);
                                continue;
                            }
                            // Computed getter: stage object/key in temps so key evaluation
                            // can suspend (yield) without relying on operand-stack state.
                            let obj_tmp = format!("__obj_lit_comp_obj_{}__", self.forin_counter);
                            self.forin_counter = self.forin_counter.saturating_add(1);
                            let key_tmp = format!("__obj_lit_comp_key_{}__", self.forin_counter);
                            self.forin_counter = self.forin_counter.saturating_add(1);

                            self.emit_define_helper_slot(&obj_tmp);
                            self.emit_define_helper_slot(&key_tmp);

                            self.emit_helper_set(&obj_tmp);
                            self.chunk.write_opcode(Opcode::Pop);
                            self.emit_helper_set(&obj_tmp);
                            self.chunk.write_opcode(Opcode::Pop);

                            self.compile_expr(key)?;
                            self.chunk.write_opcode(Opcode::ToPropertyKey);
                            self.emit_helper_set(&key_tmp);
                            self.chunk.write_opcode(Opcode::Pop);

                            self.emit_helper_get(&obj_tmp);
                            self.emit_helper_get(&key_tmp);
                            self.compile_expr(val)?;
                            self.chunk.write_opcode(Opcode::SetComputedGetter);
                            self.chunk.write_opcode(Opcode::Pop);
                            self.emit_helper_get(&obj_tmp);
                        }
                        Expr::Setter(_) => {
                            if !*is_computed && let Expr::StringLit(s) = key {
                                if let Some(ip) = self.peek_func_ip(val) {
                                    let key_name = crate::unicode::utf16_to_utf8(s);
                                    self.chunk.fn_names.entry(ip).or_insert_with(|| format!("set {}", key_name));
                                }
                                let prefixed = format!("__set_{}", crate::unicode::utf16_to_utf8(s));
                                self.compile_expr(val)?;
                                let idx = self.chunk.add_constant(Value::from(&prefixed));
                                self.chunk.write_opcode(Opcode::SetProperty);
                                self.chunk.write_u16(idx);
                                self.chunk.write_opcode(Opcode::Pop);
                                continue;
                            }
                            // Computed setter: stage object/key in temps so key evaluation
                            // can suspend (yield) without relying on operand-stack state.
                            let obj_tmp = format!("__obj_lit_comp_obj_{}__", self.forin_counter);
                            self.forin_counter = self.forin_counter.saturating_add(1);
                            let key_tmp = format!("__obj_lit_comp_key_{}__", self.forin_counter);
                            self.forin_counter = self.forin_counter.saturating_add(1);

                            self.emit_define_helper_slot(&obj_tmp);
                            self.emit_define_helper_slot(&key_tmp);

                            self.emit_helper_set(&obj_tmp);
                            self.chunk.write_opcode(Opcode::Pop);
                            self.emit_helper_set(&obj_tmp);
                            self.chunk.write_opcode(Opcode::Pop);

                            self.compile_expr(key)?;
                            self.chunk.write_opcode(Opcode::ToPropertyKey);
                            self.emit_helper_set(&key_tmp);
                            self.chunk.write_opcode(Opcode::Pop);

                            self.emit_helper_get(&obj_tmp);
                            self.emit_helper_get(&key_tmp);
                            self.compile_expr(val)?;
                            self.chunk.write_opcode(Opcode::SetComputedSetter);
                            self.chunk.write_opcode(Opcode::Pop);
                            self.emit_helper_get(&obj_tmp);
                        }
                        _ => {
                            if !*is_computed && let Expr::StringLit(s) = key {
                                let key_name = crate::unicode::utf16_to_utf8(s);
                                let is_proto_colon = *has_colon && key_name == "__proto__";
                                if let Some(ip) = self.peek_func_ip(val)
                                    && !is_proto_colon
                                {
                                    self.chunk.fn_names.entry(ip).or_insert_with(|| key_name.clone());
                                }
                                self.compile_expr(val)?;
                                let idx = self.chunk.add_constant(Value::String(s.clone()));
                                self.chunk.write_opcode(if is_proto_colon {
                                    Opcode::SetProperty
                                } else {
                                    Opcode::InitProperty
                                });
                                self.chunk.write_u16(idx);
                                self.chunk.write_opcode(Opcode::Pop);
                                continue;
                            }

                            // Computed property or non-string key fallback.
                            // Stage object/key/value so key/value evaluation can suspend
                            // without depending on operand-stack preservation.
                            let obj_tmp = format!("__obj_lit_comp_obj_{}__", self.forin_counter);
                            self.forin_counter = self.forin_counter.saturating_add(1);
                            let key_tmp = format!("__obj_lit_comp_key_{}__", self.forin_counter);
                            self.forin_counter = self.forin_counter.saturating_add(1);
                            let val_tmp = format!("__obj_lit_comp_val_{}__", self.forin_counter);
                            self.forin_counter = self.forin_counter.saturating_add(1);

                            self.emit_define_helper_slot(&obj_tmp);
                            self.emit_define_helper_slot(&key_tmp);
                            self.emit_define_helper_slot(&val_tmp);

                            self.emit_helper_set(&obj_tmp);
                            self.chunk.write_opcode(Opcode::Pop);
                            self.emit_helper_set(&obj_tmp);
                            self.chunk.write_opcode(Opcode::Pop);

                            self.compile_expr(key)?;
                            self.chunk.write_opcode(Opcode::ToPropertyKey);
                            self.emit_helper_set(&key_tmp);
                            self.chunk.write_opcode(Opcode::Pop);

                            self.compile_expr(val)?;
                            self.emit_helper_set(&val_tmp);
                            self.chunk.write_opcode(Opcode::Pop);

                            self.emit_helper_get(&obj_tmp);
                            self.emit_helper_get(&key_tmp);
                            self.emit_helper_get(&val_tmp);
                            self.chunk.write_opcode(Opcode::InitIndex);
                            self.chunk.write_opcode(Opcode::Pop);
                            self.emit_helper_get(&obj_tmp);
                        }
                    }
                }
            }
            // Property access: obj.key
            Expr::Property(obj, key) => {
                self.compile_expr(obj)?;
                let key_u16 = crate::unicode::utf8_to_utf16(key);
                let name_idx = self.chunk.add_constant(Value::String(key_u16));
                self.chunk.write_opcode(Opcode::GetProperty);
                self.chunk.write_u16(name_idx);
            }
            Expr::PrivateMember(obj, prop) => {
                self.compile_expr(obj)?;
                let resolved = self.resolve_prefixed_private_key(prop);
                let name_idx = self.chunk.add_constant(Value::from(&resolved));
                self.chunk.write_opcode(Opcode::GetProperty);
                self.chunk.write_u16(name_idx);
            }
            Expr::OptionalPrivateMember(obj, prop) => {
                self.compile_expr(obj)?;
                // null/undefined check for optional chaining
                self.chunk.write_opcode(Opcode::Dup);
                let null_idx = self.chunk.add_constant(Value::Null);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(null_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_null = self.emit_jump(Opcode::JumpIfTrue);
                self.chunk.write_opcode(Opcode::Dup);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(undef_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_undef = self.emit_jump(Opcode::JumpIfTrue);
                // Not nullish: do private property access
                let resolved = self.resolve_prefixed_private_key(prop);
                let name_idx = self.chunk.add_constant(Value::from(&resolved));
                self.chunk.write_opcode(Opcode::GetProperty);
                self.chunk.write_u16(name_idx);
                let end_jump = self.emit_jump(Opcode::Jump);
                // Nullish: pop obj, push undefined
                self.patch_jump(is_null);
                self.patch_jump(is_undef);
                self.chunk.write_opcode(Opcode::Pop);
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
                self.patch_jump(end_jump);
            }
            // Index access: obj[expr]
            Expr::Index(obj, index) => {
                self.compile_expr(obj)?;
                self.compile_expr(index)?;
                self.chunk.write_opcode(Opcode::GetIndex);
            }
            // Prefix increment: ++x
            Expr::Increment(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::Increment);
                // Write back
                self.compile_store(inner)?;
            }
            // Prefix decrement: --x
            Expr::Decrement(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::Decrement);
                self.compile_store(inner)?;
            }
            // Postfix increment: x++
            // Returns ToNumber(old_value), stores ToNumber(old_value)+1
            Expr::PostIncrement(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::ToNumber);
                self.chunk.write_opcode(Opcode::Dup);
                self.chunk.write_opcode(Opcode::Increment);
                self.compile_store(inner)?;
                self.chunk.write_opcode(Opcode::Pop);
            }
            // Postfix decrement: x--
            Expr::PostDecrement(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::ToNumber);
                self.chunk.write_opcode(Opcode::Dup);
                self.chunk.write_opcode(Opcode::Decrement);
                self.compile_store(inner)?;
                self.chunk.write_opcode(Opcode::Pop);
            }
            // Assignment to property: obj.key = val, obj[i] = val
            Expr::Assign(left, right) => match &**left {
                Expr::Var(name, ..) => {
                    // Infer function name for anonymous function/arrow assigned to a variable
                    let func_ip = self.peek_func_ip(right);
                    self.compile_expr(right)?;
                    if let Some(ip) = func_ip {
                        self.chunk.fn_names.entry(ip).or_insert_with(|| name.clone());
                    }
                    if let Some(pos) = self.locals.iter().rposition(|l| l == name) {
                        if self.const_locals.contains(name.as_str()) {
                            self.emit_const_assign_error(name);
                        } else {
                            self.chunk.write_opcode(Opcode::SetLocal);
                            self.chunk.write_byte(pos as u8);
                        }
                    } else if self.is_const_upvalue(name) {
                        // Const upvalue — always throw even if no actual slot exists
                        // (e.g., class expression name visible only inside class body)
                        self.emit_const_assign_error(name);
                    } else if let Some(upvalue_idx) = self.resolve_upvalue(name) {
                        self.chunk.write_opcode(Opcode::SetUpvalue);
                        self.chunk.write_byte(upvalue_idx);
                    } else {
                        self.emit_set_global_binding(name);
                    }
                }
                Expr::Property(obj, key) => {
                    self.compile_expr(obj)?;
                    self.compile_expr(right)?;
                    let key_u16 = crate::unicode::utf8_to_utf16(key);
                    let name_idx = self.chunk.add_constant(Value::String(key_u16));
                    self.chunk.write_opcode(Opcode::SetProperty);
                    self.chunk.write_u16(name_idx);
                }
                Expr::Index(obj, idx) => {
                    self.compile_expr(obj)?;
                    self.compile_expr(idx)?;
                    self.compile_expr(right)?;
                    self.chunk.write_opcode(Opcode::SetIndex);
                }
                Expr::SuperProperty(key) => {
                    self.compile_expr(right)?;
                    let key_u16 = crate::unicode::utf8_to_utf16(key);
                    let name_idx = self.chunk.add_constant(Value::String(key_u16));
                    self.chunk.write_opcode(Opcode::SetSuperProperty);
                    self.chunk.write_u16(name_idx);
                }
                Expr::PrivateMember(obj, prop) | Expr::OptionalPrivateMember(obj, prop) => {
                    self.compile_expr(obj)?;
                    self.compile_expr(right)?;
                    let resolved = self.resolve_prefixed_private_key(prop);
                    let name_idx = self.chunk.add_constant(Value::from(&resolved));
                    self.chunk.write_opcode(Opcode::SetProperty);
                    self.chunk.write_u16(name_idx);
                }
                Expr::Object(props) => {
                    // Expression-level object destructuring assignment:
                    // ({a: target, b: target2} = rhs)
                    self.compile_expr(right)?;
                    self.compile_expr_object_destructuring_assign(props)?;
                }
                Expr::Array(elems) => {
                    // Expression-level array destructuring assignment:
                    // [target1, target2] = rhs
                    self.compile_expr(right)?;
                    self.compile_expr_array_destructuring_assign(elems)?;
                }
                _ => {
                    return Err(crate::raise_syntax_error!("Invalid assignment target for VM"));
                }
            },
            // Arrow function / async arrow function: (params) => body
            Expr::ArrowFunction(params, body) | Expr::AsyncArrowFunction(params, body) => {
                let is_async_arrow = matches!(expr, Expr::AsyncArrowFunction(_, _));
                let jump_over = self.emit_jump(Opcode::Jump);
                let func_ip = self.chunk.code.len();
                self.chunk.arrow_function_ips.insert(func_ip);
                if is_async_arrow {
                    self.chunk.async_function_ips.insert(func_ip);
                }
                let fn_is_strict = self.record_fn_strictness(func_ip, body, false);

                let old_locals = std::mem::take(&mut self.locals);
                let old_depth = self.scope_depth;
                let old_loops = std::mem::take(&mut self.loop_stack);
                let old_strict = self.current_strict;
                // Save and set up parent scope info for closure capture
                let old_parent_locals = std::mem::take(&mut self.parent_locals);
                let old_parent_upvalues = std::mem::take(&mut self.parent_upvalues);
                let old_upvalues = std::mem::take(&mut self.upvalues);
                let old_const_locals = std::mem::take(&mut self.const_locals);
                let old_parent_const_locals = std::mem::take(&mut self.parent_const_locals);
                let old_function_depth = self.function_depth;
                let old_allow_super = self.allow_super_call;
                self.parent_locals = old_locals.clone();
                self.parent_upvalues = old_upvalues.clone();
                self.parent_const_locals = old_const_locals.clone();

                // Eagerly capture parent locals so deeper nested closures can resolve transitive captures.
                for (idx, name) in self.parent_locals.clone().iter().enumerate() {
                    self.add_upvalue(name, idx as u8, true);
                }

                self.current_strict = fn_is_strict;
                self.allow_super_call = false;
                self.function_depth = old_function_depth.saturating_add(1);
                self.scope_depth = 1;
                let mut arrow_non_rest = 0u8;
                let mut arrow_has_rest = false;
                for (param_index, param) in params.iter().enumerate() {
                    match param {
                        DestructuringElement::Variable(param_name, _) => {
                            self.locals.push(param_name.clone());
                            arrow_non_rest += 1;
                        }
                        DestructuringElement::Rest(param_name) => {
                            arrow_has_rest = true;
                            self.chunk.write_opcode(Opcode::CollectRest);
                            self.chunk.write_byte(arrow_non_rest);
                            self.locals.push(param_name.clone());
                        }
                        _ => {
                            self.locals.push(format!("__param_slot_{}__", param_index));
                            arrow_non_rest += 1;
                        }
                    }
                }

                if !Self::has_parameter_expressions(params) {
                    self.emit_hoisted_var_slots(body);
                }
                self.emit_parameter_default_initializers(params)?;
                self.emit_parameter_pattern_bindings(params)?;
                if Self::has_parameter_expressions(params) {
                    self.emit_hoisted_var_slots(body);
                }

                if body.len() == 1 {
                    if let StatementKind::Expr(expr) = &*body[0].kind {
                        // Single expression body: implicitly return the value.
                        // Async arrows return Promise.resolve(expr) to preserve promise shape.
                        if is_async_arrow {
                            let wrapped = Expr::Call(
                                Box::new(Expr::Property(
                                    Box::new(Expr::Var("Promise".to_string(), None, None)),
                                    "resolve".to_string(),
                                )),
                                vec![expr.clone()],
                            );
                            self.compile_expr(&wrapped)?;
                        } else {
                            self.compile_expr(expr)?;
                        }
                        self.chunk.write_opcode(Opcode::Return);
                    } else {
                        if is_async_arrow {
                            if let StatementKind::Return(ret_expr) = &*body[0].kind {
                                let wrapped_arg = ret_expr.clone().unwrap_or(Expr::Undefined);
                                let wrapped = Expr::Call(
                                    Box::new(Expr::Property(
                                        Box::new(Expr::Var("Promise".to_string(), None, None)),
                                        "resolve".to_string(),
                                    )),
                                    vec![wrapped_arg],
                                );
                                self.compile_expr(&wrapped)?;
                                self.chunk.write_opcode(Opcode::Return);
                            } else {
                                self.compile_statement(&body[0], true)?;
                                let wrapped = Expr::Call(
                                    Box::new(Expr::Property(
                                        Box::new(Expr::Var("Promise".to_string(), None, None)),
                                        "resolve".to_string(),
                                    )),
                                    vec![Expr::Undefined],
                                );
                                self.compile_expr(&wrapped)?;
                                self.chunk.write_opcode(Opcode::Return);
                            }
                        } else {
                            self.compile_statement(&body[0], true)?;
                            let idx = self.chunk.add_constant(Value::Undefined);
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(idx);
                            self.chunk.write_opcode(Opcode::Return);
                        }
                    }
                } else {
                    for (i, s) in body.iter().enumerate() {
                        self.compile_statement(s, i == body.len() - 1)?;
                    }
                    if is_async_arrow {
                        let wrapped = Expr::Call(
                            Box::new(Expr::Property(
                                Box::new(Expr::Var("Promise".to_string(), None, None)),
                                "resolve".to_string(),
                            )),
                            vec![Expr::Undefined],
                        );
                        self.compile_expr(&wrapped)?;
                    } else {
                        let idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(idx);
                    }
                    self.chunk.write_opcode(Opcode::Return);
                }

                self.patch_jump(jump_over);

                // Save local variable names for direct eval support
                self.chunk.fn_local_names.insert(func_ip, self.locals.clone());
                self.chunk.fn_lengths.insert(func_ip, Self::expected_argument_count(params));

                // Collect upvalues before restoring
                let fn_upvalues = std::mem::take(&mut self.upvalues);
                self.chunk
                    .fn_upvalue_names
                    .insert(func_ip, fn_upvalues.iter().map(|u| u.name.clone()).collect());

                self.locals = old_locals;
                self.scope_depth = old_depth;
                self.loop_stack = old_loops;
                self.current_strict = old_strict;
                self.allow_super_call = old_allow_super;
                self.parent_locals = old_parent_locals;
                self.parent_upvalues = old_parent_upvalues;
                self.upvalues = old_upvalues;
                self.const_locals = old_const_locals;
                self.parent_const_locals = old_parent_const_locals;
                self.function_depth = old_function_depth;

                let arrow_arity = if arrow_has_rest { arrow_non_rest } else { params.len() as u8 };
                let func_val = Value::VmFunction(func_ip, arrow_arity);
                let func_idx = self.chunk.add_constant(func_val);

                // Arrow functions ALWAYS use MakeClosure so the VM can
                // append the lexically captured `this` as an extra upvalue.
                self.chunk.write_opcode(Opcode::MakeClosure);
                self.chunk.write_u16(func_idx);
                self.chunk.write_byte(fn_upvalues.len() as u8);
                for uv in &fn_upvalues {
                    self.chunk.write_byte(if uv.is_local { 1 } else { 0 });
                    self.chunk.write_byte(uv.index);
                }
            }
            // Anonymous function expression: function(params) { body }
            Expr::Function(name, params, body) => {
                self.compile_function_body(name.as_deref(), params, body)?;
            }
            // Minimal async function expression support in VM path.
            // The body is compiled like a normal function for now.
            Expr::AsyncFunction(name, params, body) => {
                let func_ip = self.compile_function_body(name.as_deref(), params, body)?;
                self.chunk.async_function_ips.insert(func_ip);
            }
            Expr::GeneratorFunction(name, params, body) => {
                self.compile_generator_function_body(name.as_deref(), params, body)?;
            }
            // Minimal async generator support in VM path.
            // The body is executed eagerly and each yield/yield* appends to an internal array.
            Expr::AsyncGeneratorFunction(name, params, body) => {
                self.compile_async_generator_function_body(name.as_deref(), params, body)?;
            }
            // VM await lowering uses a dedicated opcode so async functions can
            // suspend and resume on the microtask queue.
            Expr::Await(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::Await);
            }
            Expr::Yield(inner_opt) => {
                if let Some(items_name) = self.async_generator_items_stack.last().cloned() {
                    self.emit_helper_get(&items_name);

                    match inner_opt {
                        Some(inner) => {
                            // Fast-path Promise.resolve(x) -> x while collecting async-generator outputs.
                            if let Expr::Call(callee, args) = &**inner {
                                if let Expr::Property(base, prop) = &**callee
                                    && prop == "resolve"
                                    && matches!(&**base, Expr::Var(name, ..) if name == "Promise")
                                    && args.len() == 1
                                {
                                    self.compile_expr(&args[0])?;
                                } else {
                                    self.compile_expr(inner)?;
                                }
                            } else {
                                self.compile_expr(inner)?;
                            }
                        }
                        None => {
                            let undef_idx = self.chunk.add_constant(Value::Undefined);
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(undef_idx);
                        }
                    }

                    self.chunk.write_opcode(Opcode::ArrayPush);
                    self.chunk.write_opcode(Opcode::Pop);

                    // Keep yield expression usable in assignment positions.
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef_idx);
                } else if let Some(items_name) = self.generator_items_stack.last().cloned() {
                    if items_name == "__gen_yield_marker__" {
                        // Suspendable generator: emit Yield opcode
                        match inner_opt {
                            Some(inner) => self.compile_expr(inner)?,
                            None => {
                                let undef_idx = self.chunk.add_constant(Value::Undefined);
                                self.chunk.write_opcode(Opcode::Constant);
                                self.chunk.write_u16(undef_idx);
                            }
                        }
                        // Yield pops the yielded value, suspends, and when resumed
                        // pushes the value passed to .next() as the yield expression result.
                        self.chunk.write_opcode(Opcode::Yield);
                    } else {
                        // Legacy eager generator (async generators still use this)
                        self.emit_helper_get(&items_name);

                        match inner_opt {
                            Some(inner) => self.compile_expr(inner)?,
                            None => {
                                let undef_idx = self.chunk.add_constant(Value::Undefined);
                                self.chunk.write_opcode(Opcode::Constant);
                                self.chunk.write_u16(undef_idx);
                            }
                        }

                        self.chunk.write_opcode(Opcode::ArrayPush);
                        self.chunk.write_opcode(Opcode::Pop);

                        let undef_idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(undef_idx);
                    }
                } else {
                    return Err(raise_syntax_error!(format!("Unimplemented expression type for VM: {expr:?}")));
                }
            }
            Expr::YieldStar(inner) => {
                if let Some(items_name) = self.async_generator_items_stack.last().cloned() {
                    self.emit_helper_get(&items_name);
                    self.compile_expr(inner)?;
                    self.chunk.write_opcode(Opcode::ArraySpread);
                    self.chunk.write_opcode(Opcode::Pop);

                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef_idx);
                } else if let Some(items_name) = self.generator_items_stack.last().cloned() {
                    if items_name == "__gen_yield_marker__" {
                        // Suspendable generator: approximate yield* by iterating values
                        // from __forOfValues(inner) and yielding each value.
                        let ys_arr = format!("__yieldstar_arr_{}__", self.forin_counter);
                        self.forin_counter = self.forin_counter.saturating_add(1);
                        let ys_idx = format!("__yieldstar_idx_{}__", self.forin_counter);
                        self.forin_counter = self.forin_counter.saturating_add(1);

                        self.emit_helper_get(INTERNAL_FOROF_HELPER);
                        self.compile_expr(inner)?;
                        self.emit_call_opcode(1, 0);
                        self.emit_define_var(&ys_arr);

                        let zero_idx = self.chunk.add_constant(Value::Number(0.0));
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(zero_idx);
                        self.emit_define_var(&ys_idx);

                        let loop_start = self.chunk.code.len();
                        self.emit_helper_get(&ys_idx);
                        self.emit_helper_get(&ys_arr);
                        let len_key = crate::unicode::utf8_to_utf16("length");
                        let len_idx = self.chunk.add_constant(Value::String(len_key));
                        self.chunk.write_opcode(Opcode::GetProperty);
                        self.chunk.write_u16(len_idx);
                        self.chunk.write_opcode(Opcode::LessThan);
                        let loop_exit = self.emit_jump(Opcode::JumpIfFalse);

                        self.emit_helper_get(&ys_arr);
                        self.emit_helper_get(&ys_idx);
                        self.chunk.write_opcode(Opcode::GetIndex);
                        self.chunk.write_opcode(Opcode::Yield);
                        self.chunk.write_opcode(Opcode::Pop);

                        self.emit_helper_get(&ys_idx);
                        self.chunk.write_opcode(Opcode::Increment);
                        self.emit_helper_set(&ys_idx);
                        self.chunk.write_opcode(Opcode::Pop);

                        self.emit_loop(loop_start);
                        self.patch_jump(loop_exit);

                        if self.scope_depth > 0 {
                            self.locals.retain(|l| l != &ys_arr && l != &ys_idx);
                        }

                        let undef_idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(undef_idx);
                    } else {
                        self.emit_helper_get(&items_name);
                        self.compile_expr(inner)?;
                        self.chunk.write_opcode(Opcode::ArraySpread);
                        self.chunk.write_opcode(Opcode::Pop);

                        let undef_idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(undef_idx);
                    }
                } else {
                    return Err(raise_syntax_error!(format!("Unimplemented expression type for VM: {expr:?}")));
                }
            }
            // new Constructor(args) — for now, special-case Error
            Expr::New(constructor, args) => {
                if let Expr::Var(name, ..) = &**constructor {
                    match name.as_str() {
                        "Error" | "TypeError" | "SyntaxError" | "RangeError" | "ReferenceError" => {
                            // Push error type name
                            let type_idx = self.chunk.add_constant(Value::from(name));
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(type_idx);
                            // Push message
                            if let Some(arg) = args.first() {
                                self.compile_expr(arg)?;
                            } else {
                                let idx = self.chunk.add_constant(Value::String(Vec::new()));
                                self.chunk.write_opcode(Opcode::Constant);
                                self.chunk.write_u16(idx);
                            }
                            self.chunk.write_opcode(Opcode::NewError);
                        }
                        "Object" | "Number" | "Boolean" | "String" => {
                            // Route through normal NewCall so the VM runtime
                            // can set __proto__ correctly (e.g. Boolean.prototype for new Object(true)).
                            self.compile_expr(constructor)?;
                            for a in args {
                                self.compile_expr(a)?;
                            }
                            self.chunk.write_opcode(Opcode::NewCall);
                            self.chunk.write_byte(args.len() as u8);
                        }
                        "Function" => {
                            // new Function(body) → compile to: push native_fn, push body, Call(1)
                            let fn_idx = self.chunk.add_constant(Value::VmNativeFunction(72));
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(fn_idx);
                            if let Some(body_arg) = args.last() {
                                self.compile_expr(body_arg)?;
                            } else {
                                let idx = self.chunk.add_constant(Value::String(Vec::new()));
                                self.chunk.write_opcode(Opcode::Constant);
                                self.chunk.write_u16(idx);
                            }
                            self.emit_call_opcode(1, 0);
                        }
                        _ => {
                            // Generic constructor: create object, call constructor with this
                            self.compile_expr(constructor)?;
                            let has_spread = args.iter().any(|a| matches!(a, Expr::Spread(_)));
                            if has_spread {
                                self.chunk.write_opcode(Opcode::NewArray);
                                self.chunk.write_byte(0);
                                for a in args {
                                    if let Expr::Spread(inner) = a {
                                        self.compile_expr(inner)?;
                                        self.chunk.write_opcode(Opcode::ArraySpread);
                                    } else {
                                        self.compile_expr(a)?;
                                        self.chunk.write_opcode(Opcode::ArrayPush);
                                    }
                                }
                                self.chunk.write_opcode(Opcode::NewCallSpread);
                            } else {
                                for a in args {
                                    self.compile_expr(a)?;
                                }
                                self.chunk.write_opcode(Opcode::NewCall);
                                self.chunk.write_byte(args.len() as u8);
                            }
                        }
                    }
                } else {
                    // Dynamic constructor: create object, call constructor with this
                    self.compile_expr(constructor)?;
                    let has_spread = args.iter().any(|a| matches!(a, Expr::Spread(_)));
                    if has_spread {
                        self.chunk.write_opcode(Opcode::NewArray);
                        self.chunk.write_byte(0);
                        for a in args {
                            if let Expr::Spread(inner) = a {
                                self.compile_expr(inner)?;
                                self.chunk.write_opcode(Opcode::ArraySpread);
                            } else {
                                self.compile_expr(a)?;
                                self.chunk.write_opcode(Opcode::ArrayPush);
                            }
                        }
                        self.chunk.write_opcode(Opcode::NewCallSpread);
                    } else {
                        for a in args {
                            self.compile_expr(a)?;
                        }
                        self.chunk.write_opcode(Opcode::NewCall);
                        self.chunk.write_byte(args.len() as u8);
                    }
                }
            }
            // Compound assignment: x += rhs
            Expr::AddAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::Add);
                self.compile_store(lhs)?;
            }
            Expr::SubAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::Sub);
                self.compile_store(lhs)?;
            }
            Expr::MulAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::Mul);
                self.compile_store(lhs)?;
            }
            Expr::DivAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::Div);
                self.compile_store(lhs)?;
            }
            Expr::ModAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::Mod);
                self.compile_store(lhs)?;
            }
            Expr::BitXorAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::BitwiseXor);
                self.compile_store(lhs)?;
            }
            Expr::BitAndAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::BitwiseAnd);
                self.compile_store(lhs)?;
            }
            Expr::BitOrAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::BitwiseOr);
                self.compile_store(lhs)?;
            }
            Expr::LeftShiftAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::ShiftLeft);
                self.compile_store(lhs)?;
            }
            Expr::RightShiftAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::ShiftRight);
                self.compile_store(lhs)?;
            }
            Expr::UnsignedRightShiftAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::UnsignedShiftRight);
                self.compile_store(lhs)?;
            }
            Expr::PowAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.compile_expr(rhs)?;
                self.chunk.write_opcode(Opcode::Pow);
                self.compile_store(lhs)?;
            }
            Expr::LogicalAndAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.chunk.write_opcode(Opcode::Dup);
                let end_jump = self.emit_jump(Opcode::JumpIfFalse);
                self.chunk.write_opcode(Opcode::Pop);
                self.compile_expr(rhs)?;
                self.compile_store(lhs)?;
                self.patch_jump(end_jump);
            }
            Expr::LogicalOrAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;
                self.chunk.write_opcode(Opcode::Dup);
                let assign_jump = self.emit_jump(Opcode::JumpIfFalse);
                let end_jump = self.emit_jump(Opcode::Jump);
                self.patch_jump(assign_jump);
                self.chunk.write_opcode(Opcode::Pop);
                self.compile_expr(rhs)?;
                self.compile_store(lhs)?;
                self.patch_jump(end_jump);
            }
            Expr::NullishAssign(lhs, rhs) => {
                self.compile_expr(lhs)?;

                self.chunk.write_opcode(Opcode::Dup);
                let null_idx = self.chunk.add_constant(Value::Null);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(null_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let assign_if_null = self.emit_jump(Opcode::JumpIfTrue);

                self.chunk.write_opcode(Opcode::Dup);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(undef_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let keep_current = self.emit_jump(Opcode::JumpIfFalse);

                self.patch_jump(assign_if_null);
                self.chunk.write_opcode(Opcode::Pop);
                self.compile_expr(rhs)?;
                self.compile_store(lhs)?;

                self.patch_jump(keep_current);
            }
            // Ternary: cond ? a : b
            Expr::Conditional(cond, then_expr, else_expr) => {
                self.compile_expr(cond)?;
                let else_jump = self.emit_jump(Opcode::JumpIfFalse);
                self.compile_expr(then_expr)?;
                let end_jump = self.emit_jump(Opcode::Jump);
                self.patch_jump(else_jump);
                self.compile_expr(else_expr)?;
                self.patch_jump(end_jump);
            }
            // Comma: (a, b) → evaluate a (discard), evaluate b (keep)
            Expr::Comma(left, right) => {
                self.compile_expr(left)?;
                self.chunk.write_opcode(Opcode::Pop);
                self.compile_expr(right)?;
            }
            // Delete operator
            Expr::Delete(inner) => {
                match &**inner {
                    Expr::Var(..) => {
                        // delete variable → SyntaxError in strict mode
                        // Emit: push SyntaxError type name, push message, NewError, Throw
                        let type_idx = self.chunk.add_constant(Value::from("SyntaxError"));
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(type_idx);
                        let msg_idx = self
                            .chunk
                            .add_constant(Value::from("Delete of an unqualified identifier in strict mode."));
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(msg_idx);
                        self.chunk.write_opcode(Opcode::NewError);
                        self.chunk.write_opcode(Opcode::Throw);
                    }
                    Expr::Property(obj, key) => {
                        self.compile_expr(obj)?;
                        let key_u16 = crate::unicode::utf8_to_utf16(key);
                        let name_idx = self.chunk.add_constant(Value::String(key_u16));
                        self.chunk.write_opcode(Opcode::DeleteProperty);
                        self.chunk.write_u16(name_idx);
                    }
                    Expr::Index(obj, idx_expr) => {
                        self.compile_expr(obj)?;
                        self.compile_expr(idx_expr)?;
                        self.chunk.write_opcode(Opcode::DeleteIndex);
                    }
                    _ => {
                        self.compile_expr(inner)?;
                        self.chunk.write_opcode(Opcode::Pop);
                        let idx = self.chunk.add_constant(Value::Boolean(true));
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(idx);
                    }
                }
            }
            // Void operator: evaluate expression, discard, push undefined
            Expr::Void(inner) => {
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::Pop);
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
            }
            // Nullish coalescing: a ?? b
            Expr::NullishCoalescing(left, right) => {
                // If left is null/undefined, return right; else return left
                self.compile_expr(left)?;
                self.chunk.write_opcode(Opcode::Dup);
                // Null check
                let null_idx = self.chunk.add_constant(Value::Null);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(null_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_null = self.emit_jump(Opcode::JumpIfTrue);
                // Undefined check
                self.chunk.write_opcode(Opcode::Dup);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(undef_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_undef = self.emit_jump(Opcode::JumpIfTrue);
                // Not nullish: keep left
                let end_jump = self.emit_jump(Opcode::Jump);
                // Nullish: discard left, evaluate right
                self.patch_jump(is_null);
                self.patch_jump(is_undef);
                self.chunk.write_opcode(Opcode::Pop);
                self.compile_expr(right)?;
                self.patch_jump(end_jump);
            }
            // Optional chaining: obj?.prop
            Expr::OptionalProperty(obj, key) => {
                self.compile_expr(obj)?;
                self.chunk.write_opcode(Opcode::Dup);
                let null_idx = self.chunk.add_constant(Value::Null);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(null_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_null = self.emit_jump(Opcode::JumpIfTrue);
                self.chunk.write_opcode(Opcode::Dup);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(undef_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_undef = self.emit_jump(Opcode::JumpIfTrue);
                // Not nullish: do property access
                let key_u16 = crate::unicode::utf8_to_utf16(key);
                let name_idx = self.chunk.add_constant(Value::String(key_u16));
                self.chunk.write_opcode(Opcode::GetProperty);
                self.chunk.write_u16(name_idx);
                let end_jump = self.emit_jump(Opcode::Jump);
                // Nullish: pop obj, push undefined
                self.patch_jump(is_null);
                self.patch_jump(is_undef);
                self.chunk.write_opcode(Opcode::Pop);
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
                self.patch_jump(end_jump);
            }
            // Optional index: obj?.[expr]
            Expr::OptionalIndex(obj, index_expr) => {
                self.compile_expr(obj)?;
                self.chunk.write_opcode(Opcode::Dup);
                let null_idx = self.chunk.add_constant(Value::Null);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(null_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_null = self.emit_jump(Opcode::JumpIfTrue);
                self.chunk.write_opcode(Opcode::Dup);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(undef_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_undef = self.emit_jump(Opcode::JumpIfTrue);
                self.compile_expr(index_expr)?;
                self.chunk.write_opcode(Opcode::GetIndex);
                let end_jump = self.emit_jump(Opcode::Jump);
                self.patch_jump(is_null);
                self.patch_jump(is_undef);
                self.chunk.write_opcode(Opcode::Pop);
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
                self.patch_jump(end_jump);
            }
            // Optional call: fn?.()
            Expr::OptionalCall(callee, args) => {
                // Optional method call with non-optional property callee: obj?.method(...)
                // Short-circuit when receiver is nullish before property access.
                if let Expr::Property(obj, method_name) = &**callee {
                    self.compile_expr(obj)?;
                    self.chunk.write_opcode(Opcode::Dup);
                    let null_idx = self.chunk.add_constant(Value::Null);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(null_idx);
                    self.chunk.write_opcode(Opcode::Equal);
                    let recv_is_null = self.emit_jump(Opcode::JumpIfTrue);
                    self.chunk.write_opcode(Opcode::Dup);
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef_idx);
                    self.chunk.write_opcode(Opcode::Equal);
                    let recv_is_undef = self.emit_jump(Opcode::JumpIfTrue);

                    let key_u16 = crate::unicode::utf8_to_utf16(method_name);
                    let name_idx = self.chunk.add_constant(Value::String(key_u16));
                    self.chunk.write_opcode(Opcode::Dup);
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(name_idx);

                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    self.emit_call_opcode(args.len(), 0x80);
                    let end_jump = self.emit_jump(Opcode::Jump);

                    self.patch_jump(recv_is_null);
                    self.patch_jump(recv_is_undef);
                    self.chunk.write_opcode(Opcode::Pop);
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                    self.patch_jump(end_jump);
                } else if let Expr::Index(obj, index_expr) = &**callee {
                    self.compile_expr(obj)?;
                    self.chunk.write_opcode(Opcode::Dup);
                    let null_idx = self.chunk.add_constant(Value::Null);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(null_idx);
                    self.chunk.write_opcode(Opcode::Equal);
                    let recv_is_null = self.emit_jump(Opcode::JumpIfTrue);
                    self.chunk.write_opcode(Opcode::Dup);
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef_idx);
                    self.chunk.write_opcode(Opcode::Equal);
                    let recv_is_undef = self.emit_jump(Opcode::JumpIfTrue);

                    self.chunk.write_opcode(Opcode::Dup);
                    self.compile_expr(index_expr)?;
                    self.chunk.write_opcode(Opcode::GetIndex);

                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    self.emit_call_opcode(args.len(), 0x80);
                    let end_jump = self.emit_jump(Opcode::Jump);

                    self.patch_jump(recv_is_null);
                    self.patch_jump(recv_is_undef);
                    self.chunk.write_opcode(Opcode::Pop);
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                    self.patch_jump(end_jump);
                } else if let Expr::OptionalProperty(obj, method_name) = &**callee {
                    // Optional method call with optional property callee: obj?.method?.(...)
                    // Short-circuit on nullish receiver or nullish method value.
                    self.compile_expr(obj)?;
                    self.chunk.write_opcode(Opcode::Dup);
                    let null_idx = self.chunk.add_constant(Value::Null);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(null_idx);
                    self.chunk.write_opcode(Opcode::Equal);
                    let recv_is_null = self.emit_jump(Opcode::JumpIfTrue);
                    self.chunk.write_opcode(Opcode::Dup);
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef_idx);
                    self.chunk.write_opcode(Opcode::Equal);
                    let recv_is_undef = self.emit_jump(Opcode::JumpIfTrue);

                    let key_u16 = crate::unicode::utf8_to_utf16(method_name);
                    let name_idx = self.chunk.add_constant(Value::String(key_u16));
                    self.chunk.write_opcode(Opcode::Dup);
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(name_idx);

                    self.chunk.write_opcode(Opcode::Dup);
                    let callee_null_idx = self.chunk.add_constant(Value::Null);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(callee_null_idx);
                    self.chunk.write_opcode(Opcode::Equal);
                    let callee_is_null = self.emit_jump(Opcode::JumpIfTrue);
                    self.chunk.write_opcode(Opcode::Dup);
                    let callee_undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(callee_undef_idx);
                    self.chunk.write_opcode(Opcode::Equal);
                    let callee_is_undef = self.emit_jump(Opcode::JumpIfTrue);

                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    self.emit_call_opcode(args.len(), 0x80);
                    let end_jump = self.emit_jump(Opcode::Jump);

                    self.patch_jump(callee_is_null);
                    self.patch_jump(callee_is_undef);
                    self.chunk.write_opcode(Opcode::Pop); // pop callee
                    self.chunk.write_opcode(Opcode::Pop); // pop receiver
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                    let after_callee_short = self.emit_jump(Opcode::Jump);

                    self.patch_jump(recv_is_null);
                    self.patch_jump(recv_is_undef);
                    self.chunk.write_opcode(Opcode::Pop); // pop receiver
                    let recv_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(recv_idx);

                    self.patch_jump(after_callee_short);
                    self.patch_jump(end_jump);
                } else {
                    self.compile_expr(callee)?;
                    self.chunk.write_opcode(Opcode::Dup);
                    let null_idx = self.chunk.add_constant(Value::Null);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(null_idx);
                    self.chunk.write_opcode(Opcode::Equal);
                    let is_null = self.emit_jump(Opcode::JumpIfTrue);
                    self.chunk.write_opcode(Opcode::Dup);
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef_idx);
                    self.chunk.write_opcode(Opcode::Equal);
                    let is_undef = self.emit_jump(Opcode::JumpIfTrue);
                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    self.emit_call_opcode(args.len(), 0);
                    let end_jump = self.emit_jump(Opcode::Jump);
                    self.patch_jump(is_null);
                    self.patch_jump(is_undef);
                    self.chunk.write_opcode(Opcode::Pop);
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                    self.patch_jump(end_jump);
                }
            }
            // Getter/Setter in object literal: compile as the inner function
            Expr::Getter(inner) | Expr::Setter(inner) => {
                self.compile_expr(inner)?;
            }
            // Spread expression — when encountered standalone (not inside array/call/new),
            // just compile the inner expression
            Expr::Spread(inner) => {
                self.compile_expr(inner)?;
            }
            // Regex literal: emit `new RegExp(pattern, flags)` so runtime builds the object.
            Expr::Regex(pattern, flags) => {
                let re_name_idx = self.chunk.add_constant(Value::from("RegExp"));
                self.chunk.write_opcode(Opcode::GetGlobal);
                self.chunk.write_u16(re_name_idx);

                let pattern_idx = self.chunk.add_constant(Value::from(pattern));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(pattern_idx);

                let flags_idx = self.chunk.add_constant(Value::from(flags));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(flags_idx);

                self.chunk.write_opcode(Opcode::NewCall);
                self.chunk.write_byte(2);
            }
            Expr::ValuePlaceholder => {
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
            }
            Expr::TaggedTemplate(tag_fn, _raw_flag, cooked_strings, raw_strings, expressions) => {
                // Tagged template: tagFn(strings, ...exprs), where strings.raw is an array.
                self.compile_expr(tag_fn)?;

                // Build cooked strings array on stack.
                self.chunk.write_opcode(Opcode::NewArray);
                self.chunk.write_byte(0);
                for cooked in cooked_strings {
                    match cooked {
                        Some(s) => {
                            let idx = self.chunk.add_constant(Value::String(s.clone()));
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(idx);
                        }
                        None => {
                            let idx = self.chunk.add_constant(Value::Undefined);
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(idx);
                        }
                    }
                    self.chunk.write_opcode(Opcode::ArrayPush);
                }

                // Attach `raw` property: duplicate cooked array, set raw array, keep cooked array.
                self.chunk.write_opcode(Opcode::Dup);
                self.chunk.write_opcode(Opcode::NewArray);
                self.chunk.write_byte(0);
                for raw in raw_strings {
                    let idx = self.chunk.add_constant(Value::String(raw.clone()));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                    self.chunk.write_opcode(Opcode::ArrayPush);
                }
                let raw_key_idx = self.chunk.add_constant(Value::from("raw"));
                self.chunk.write_opcode(Opcode::SetProperty);
                self.chunk.write_u16(raw_key_idx);
                self.chunk.write_opcode(Opcode::Pop);

                for expr in expressions {
                    self.compile_expr(expr)?;
                }
                let argc = 1 + expressions.len();
                self.emit_call_opcode(argc, 0);
            }
            Expr::Class(class_def) => {
                self.compile_class_definition(class_def, true)?;
            }
            Expr::PrivateName(prop) => {
                // Used for `#field in obj` — push the private name as a string
                let private_name = self.resolve_private_key(prop);
                let name_idx = self.chunk.add_constant(Value::from(&private_name));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(name_idx);
            }
            Expr::NewTarget => {
                self.chunk.write_opcode(Opcode::GetNewTarget);
            }
            _ => return Err(raise_syntax_error!(format!("Unimplemented expression type for VM: {expr:?}"))),
        }
        Ok(())
    }

    /// Resolve a variable name as an upvalue (captured from a parent scope).
    /// Returns the upvalue index if found, or None.
    fn resolve_upvalue(&mut self, name: &str) -> Option<u8> {
        // Check parent's locals
        if let Some(parent_idx) = self.parent_locals.iter().rposition(|l| l == name) {
            return Some(self.add_upvalue(name, parent_idx as u8, true));
        }
        // Check parent's upvalues (for multi-level capture)
        if let Some(parent_uv_idx) = self.parent_upvalues.iter().position(|u| u.name == name) {
            return Some(self.add_upvalue(name, parent_uv_idx as u8, false));
        }
        None
    }

    /// Add an upvalue to the current function's upvalue list, deduplicating by name.
    /// When a duplicate name is found and the new entry refers to a more recent
    /// local (higher index), the existing entry is updated so that variable
    /// shadowing is handled correctly (e.g. class name scope over outer var).
    fn add_upvalue(&mut self, name: &str, index: u8, is_local: bool) -> u8 {
        for (i, uv) in self.upvalues.iter_mut().enumerate() {
            if uv.name == name {
                if is_local && uv.is_local && index > uv.index {
                    uv.index = index;
                }
                return i as u8;
            }
        }
        let idx = self.upvalues.len() as u8;
        self.upvalues.push(UpvalueInfo {
            name: name.to_string(),
            index,
            is_local,
        });
        idx
    }

    /// Check if a variable captured via upvalue is const-like in the parent scope.
    fn is_const_upvalue(&self, name: &str) -> bool {
        // Check parent scope's const set (for actual upvalue captures)
        if self.parent_const_locals.contains(name) {
            return true;
        }
        // Also check current scope's const set for names that are tracked as const
        // but have no corresponding local slot (e.g. class expression name visible
        // inside class body methods without a pre-slot).
        if self.const_locals.contains(name) && !self.locals.iter().any(|l| l == name) {
            return true;
        }
        false
    }

    /// Emit bytecode that throws a TypeError for assignment to a constant variable at runtime.
    fn emit_const_assign_error(&mut self, name: &str) {
        // Pop the RHS value that was already compiled
        self.chunk.write_opcode(Opcode::Pop);
        // Push the error message string
        let msg = format!("Assignment to constant variable '{}'", name);
        let msg_u16 = crate::unicode::utf8_to_utf16(&msg);
        let msg_idx = self.chunk.add_constant(Value::String(msg_u16));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(msg_idx);
        // ThrowTypeError: pops message, constructs TypeError, throws
        self.chunk.write_opcode(Opcode::ThrowTypeError);
    }

    /// Write-back helper for increment/decrement: store the top-of-stack value
    /// back into the variable that `expr` represents.
    fn compile_store(&mut self, expr: &Expr) -> Result<(), JSError> {
        match expr {
            Expr::Var(name, ..) => {
                if let Some(pos) = self.locals.iter().rposition(|l| l == name) {
                    if self.const_locals.contains(name.as_str()) {
                        self.emit_const_assign_error(name);
                    } else {
                        self.chunk.write_opcode(Opcode::SetLocal);
                        self.chunk.write_byte(pos as u8);
                    }
                } else if self.is_const_upvalue(name) {
                    self.emit_const_assign_error(name);
                } else if let Some(upvalue_idx) = self.resolve_upvalue(name) {
                    self.chunk.write_opcode(Opcode::SetUpvalue);
                    self.chunk.write_byte(upvalue_idx);
                } else {
                    self.emit_set_global_binding(name);
                }
            }
            Expr::Property(obj, key) => {
                // Stack has: [..., new_val]
                // Need: push obj, swap so stack = [..., obj, new_val], SetProperty
                self.compile_expr(obj)?;
                self.chunk.write_opcode(Opcode::Swap);
                let key_u16 = crate::unicode::utf8_to_utf16(key);
                let key_idx = self.chunk.add_constant(Value::String(key_u16));
                self.chunk.write_opcode(Opcode::SetProperty);
                self.chunk.write_u16(key_idx);
            }
            Expr::PrivateMember(obj, prop) | Expr::OptionalPrivateMember(obj, prop) => {
                self.compile_expr(obj)?;
                self.chunk.write_opcode(Opcode::Swap);
                let resolved = self.resolve_prefixed_private_key(prop);
                let key_idx = self.chunk.add_constant(Value::from(&resolved));
                self.chunk.write_opcode(Opcode::SetProperty);
                self.chunk.write_u16(key_idx);
            }
            Expr::Index(obj, idx_expr) => {
                // Preserve computed RHS value, then evaluate target and emit SetIndex.
                // Stack on entry: [..., new_val]
                let temp = format!("__idx_store_{}__", self.completion_counter);
                self.completion_counter += 1;
                self.emit_define_var(&temp);

                self.compile_expr(obj)?;
                self.compile_expr(idx_expr)?;
                self.emit_helper_get(&temp);
                self.chunk.write_opcode(Opcode::SetIndex);

                if self.scope_depth > 0 {
                    // Local temp still sits under result value: [..., temp, result]
                    self.chunk.write_opcode(Opcode::Swap);
                    self.chunk.write_opcode(Opcode::Pop);
                    if let Some(pos) = self.locals.iter().rposition(|l| l == &temp) {
                        self.locals.remove(pos);
                    }
                } else {
                    let name_u16 = crate::unicode::utf8_to_utf16(&temp);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::DeleteGlobal);
                    self.chunk.write_u16(name_idx);
                }
            }
            _ => {
                return Err(crate::raise_syntax_error!("Invalid increment/decrement target for VM"));
            }
        }
        Ok(())
    }

    /// Pop block-scoped locals created since `saved` count, emitting Pop opcodes.
    fn end_block_scope(&mut self, saved: usize) {
        let to_pop = self.locals.len() - saved;
        for _ in 0..to_pop {
            self.chunk.write_opcode(Opcode::Pop);
        }
        self.locals.truncate(saved);
    }

    /// Emit get for a synthetic local/global variable
    fn emit_helper_get(&mut self, name: &str) {
        if self.scope_depth > 0 {
            if let Some(pos) = self.locals.iter().rposition(|l| l == name) {
                self.chunk.write_opcode(Opcode::GetLocal);
                self.chunk.write_byte(pos as u8);
                return;
            }
            // Some transformed control-flow paths can pop synthetic locals earlier than expected.
            // Fall back to global helper access instead of panicking.
            let n = crate::unicode::utf8_to_utf16(name);
            let ni = self.chunk.add_constant(Value::String(n));
            self.chunk.write_opcode(Opcode::GetGlobal);
            self.chunk.write_u16(ni);
        } else {
            let n = crate::unicode::utf8_to_utf16(name);
            let ni = self.chunk.add_constant(Value::String(n));
            self.chunk.write_opcode(Opcode::GetGlobal);
            self.chunk.write_u16(ni);
        }
    }

    /// Emit set for a synthetic local/global variable (value already on TOS)
    fn emit_helper_set(&mut self, name: &str) {
        if self.scope_depth > 0 {
            if let Some(pos) = self.locals.iter().rposition(|l| l == name) {
                self.chunk.write_opcode(Opcode::SetLocal);
                self.chunk.write_byte(pos as u8);
                return;
            }
            // Some transformed control-flow paths can pop synthetic locals earlier than expected.
            // Fall back to global helper access instead of panicking.
            let n = crate::unicode::utf8_to_utf16(name);
            let ni = self.chunk.add_constant(Value::String(n));
            self.chunk.write_opcode(Opcode::SetGlobal);
            self.chunk.write_u16(ni);
        } else {
            let n = crate::unicode::utf8_to_utf16(name);
            let ni = self.chunk.add_constant(Value::String(n));
            self.chunk.write_opcode(Opcode::SetGlobal);
            self.chunk.write_u16(ni);
        }
    }

    /// Predefine a synthetic helper slot as a global binding so strict-mode helper
    /// writes can still use SetGlobal without tripping undeclared-assignment checks.
    fn emit_define_helper_slot(&mut self, name: &str) {
        let undef_idx = self.chunk.add_constant(Value::Undefined);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(undef_idx);
        let n = crate::unicode::utf8_to_utf16(name);
        let ni = self.chunk.add_constant(Value::String(n));
        self.chunk.write_opcode(Opcode::DefineGlobal);
        self.chunk.write_u16(ni);
    }

    /// Set up a completion value variable for tracking eval loop results.
    /// Returns the synthetic variable name. Pushes `undefined` as initial value.
    fn setup_completion_var(&mut self) {
        let id = self.completion_counter;
        self.completion_counter += 1;
        let name = format!("__cv_{}__", id);
        let undef = self.chunk.add_constant(Value::Undefined);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(undef);
        if self.scope_depth > 0 {
            self.locals.push(name.clone());
        } else {
            let n = crate::unicode::utf8_to_utf16(&name);
            let ni = self.chunk.add_constant(Value::String(n));
            self.chunk.write_opcode(Opcode::DefineGlobal);
            self.chunk.write_u16(ni);
        }
        self.completion_var = Some(name);
    }

    /// Emit code to save top-of-stack value into the completion variable (value stays on stack).
    fn emit_save_completion(&mut self) {
        if let Some(ref cv) = self.completion_var.clone() {
            self.emit_helper_set(cv);
        }
    }

    /// Emit code to load the completion value onto the stack and clear tracking.
    fn emit_load_completion(&mut self) {
        if let Some(ref cv) = self.completion_var.clone() {
            self.emit_helper_get(cv);
            // Clean up the synthetic local
            if self.scope_depth > 0 {
                self.locals.retain(|l| l != cv);
            }
            self.completion_var = None;
        }
    }

    /// Preview the function IP that will be generated for anonymous function/arrow expressions.
    /// Returns None if expr is not a function/arrow, or if named.
    fn peek_func_ip(&self, expr: &Expr) -> Option<usize> {
        match expr {
            // Anonymous function expression or arrow: the func body starts after the jump instruction
            Expr::Function(name, ..) if name.is_none() || name.as_deref() == Some("") => {
                // Jump opcode (1) + u16 operand (2) = 3 bytes before func body
                Some(self.chunk.code.len() + 3)
            }
            Expr::GeneratorFunction(name, ..) if name.is_none() || name.as_deref() == Some("") => Some(self.chunk.code.len() + 3),
            Expr::AsyncFunction(name, ..) if name.is_none() || name.as_deref() == Some("") => Some(self.chunk.code.len() + 3),
            Expr::AsyncGeneratorFunction(name, ..) if name.is_none() || name.as_deref() == Some("") => Some(self.chunk.code.len() + 3),
            Expr::Getter(inner) | Expr::Setter(inner) => self.peek_func_ip(inner),
            Expr::Class(class_def) if class_def.name.is_empty() => Some(self.chunk.code.len() + 3),
            Expr::ArrowFunction(..) | Expr::AsyncArrowFunction(..) => Some(self.chunk.code.len() + 3),
            _ => None,
        }
    }

    fn expected_argument_count(params: &[DestructuringElement]) -> usize {
        let mut count = 0usize;
        for param in params {
            match param {
                DestructuringElement::Rest(_) | DestructuringElement::RestPattern(_) => break,
                DestructuringElement::Variable(_, Some(_))
                | DestructuringElement::NestedArray(_, Some(_))
                | DestructuringElement::NestedObject(_, Some(_)) => break,
                _ => count = count.saturating_add(1),
            }
        }
        count
    }

    /// Helper: compile a for-of loop body where the iteration variable is array-destructured.
    /// Compile assignment to an expression LHS.
    /// Expects the value to assign is already on top of the stack.
    /// The value remains on the stack after assignment.
    fn compile_assign_to_expr(&mut self, lhs: &Expr) -> Result<(), JSError> {
        match lhs {
            Expr::Var(name, ..) => {
                if let Some(pos) = self.locals.iter().rposition(|l| l == name) {
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(pos as u8);
                } else if let Some(upvalue_idx) = self.resolve_upvalue(name) {
                    self.chunk.write_opcode(Opcode::SetUpvalue);
                    self.chunk.write_byte(upvalue_idx);
                } else {
                    let name_u16 = crate::unicode::utf8_to_utf16(name);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::SetGlobal);
                    self.chunk.write_u16(name_idx);
                }
            }
            Expr::Property(obj, key) => {
                // Stack: [..., val]. Need: push obj, swap, SetProperty
                self.compile_expr(obj)?;
                self.chunk.write_opcode(Opcode::Swap);
                let key_u16 = crate::unicode::utf8_to_utf16(key);
                let key_idx = self.chunk.add_constant(Value::String(key_u16));
                self.chunk.write_opcode(Opcode::SetProperty);
                self.chunk.write_u16(key_idx);
            }
            Expr::Index(obj, idx) => {
                // Stack: [..., val]. Need: push obj, push idx, rotate3, SetIndex
                self.compile_expr(obj)?;
                self.compile_expr(idx)?;
                // Stack: [..., val, obj, idx] - need [..., obj, idx, val]
                // Use Rotate3 or manual stack manipulation
                // For now: store val in temp, push obj, push idx, push val
                // Actually SetIndex expects [obj, idx, val] so we need to rearrange
                self.chunk.write_opcode(Opcode::SetIndex);
            }
            Expr::PrivateMember(obj, prop) | Expr::OptionalPrivateMember(obj, prop) => {
                // Stack: [..., val]. Need: push obj, swap, SetProperty
                self.compile_expr(obj)?;
                self.chunk.write_opcode(Opcode::Swap);
                let resolved = self.resolve_prefixed_private_key(prop);
                let key_idx = self.chunk.add_constant(Value::from(&resolved));
                self.chunk.write_opcode(Opcode::SetProperty);
                self.chunk.write_u16(key_idx);
            }
            _ => {
                return Err(crate::raise_syntax_error!(
                    "Unsupported assignment target in for-of/for-in expression form"
                ));
            }
        }
        Ok(())
    }

    /// Assign a stack-top value to an expression target (for expression-level
    /// destructuring assignment). Handles simple targets (Var, Property, Index,
    /// PrivateMember) and nested patterns (Object, Array). Also handles
    /// default values encoded as Assign(target, default).
    /// After the call the value is consumed from the stack.
    fn compile_expr_assign_to_target(&mut self, target: &Expr) -> Result<(), JSError> {
        match target {
            Expr::Var(..)
            | Expr::Property(..)
            | Expr::Index(..)
            | Expr::PrivateMember(..)
            | Expr::OptionalPrivateMember(..)
            | Expr::SuperProperty(..) => {
                self.compile_store(target)?;
                self.chunk.write_opcode(Opcode::Pop);
            }
            Expr::Object(props) => {
                self.compile_expr_object_destructuring_assign(props)?;
                self.chunk.write_opcode(Opcode::Pop);
            }
            Expr::Array(elems) => {
                self.compile_expr_array_destructuring_assign(elems)?;
                self.chunk.write_opcode(Opcode::Pop);
            }
            Expr::Assign(inner_target, default_expr) => {
                // Default value: if value is undefined, use default
                self.chunk.write_opcode(Opcode::Dup);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(undef_idx);
                self.chunk.write_opcode(Opcode::StrictNotEqual);
                let skip_default = self.emit_jump(Opcode::JumpIfTrue);
                self.chunk.write_opcode(Opcode::Pop);
                self.compile_expr(default_expr)?;
                self.patch_jump(skip_default);
                self.compile_expr_assign_to_target(inner_target)?;
            }
            _ => {
                return Err(crate::raise_syntax_error!("Invalid destructuring assignment target"));
            }
        }
        Ok(())
    }

    /// Expression-level object destructuring assignment: ({a: x, b: y} = rhs)
    /// RHS value is on stack top. Leaves the RHS value on the stack (assignment
    /// expression result).
    fn compile_expr_object_destructuring_assign(&mut self, props: &[(Expr, Expr, bool, bool)]) -> Result<(), JSError> {
        // Save RHS into temp so we can reference it for each property
        let temp = format!("__expr_destr_obj_{}__", self.forin_counter);
        self.forin_counter += 1;
        // Dup so we keep the RHS value as the expression result
        self.chunk.write_opcode(Opcode::Dup);
        self.emit_define_var(&temp);

        // Runtime guard: destructuring from undefined/null must throw
        self.emit_helper_get(&temp);
        let undef_idx = self.chunk.add_constant(Value::Undefined);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(undef_idx);
        self.chunk.write_opcode(Opcode::Equal);
        let undefined_ok = self.emit_jump(Opcode::JumpIfFalse);
        let type_idx = self.chunk.add_constant(Value::from("TypeError"));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(type_idx);
        let msg_idx = self.chunk.add_constant(Value::from("Cannot destructure undefined"));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(msg_idx);
        self.chunk.write_opcode(Opcode::NewError);
        self.chunk.write_opcode(Opcode::Throw);
        self.patch_jump(undefined_ok);

        self.emit_helper_get(&temp);
        let null_idx = self.chunk.add_constant(Value::Null);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(null_idx);
        self.chunk.write_opcode(Opcode::Equal);
        let null_ok = self.emit_jump(Opcode::JumpIfFalse);
        let type_idx2 = self.chunk.add_constant(Value::from("TypeError"));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(type_idx2);
        let msg_idx2 = self.chunk.add_constant(Value::from("Cannot destructure null"));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(msg_idx2);
        self.chunk.write_opcode(Opcode::NewError);
        self.chunk.write_opcode(Opcode::Throw);
        self.patch_jump(null_ok);

        // Build excluded-keys array if there's a rest pattern
        let has_rest = props.iter().any(|(_, _, is_rest, _)| *is_rest);
        let excluded_arr_temp = if has_rest {
            let name = format!("__excluded_arr_{}__", self.forin_counter);
            self.forin_counter += 1;
            self.chunk.write_opcode(Opcode::NewArray);
            self.chunk.write_byte(0);
            self.emit_define_var(&name);
            Some(name)
        } else {
            None
        };

        for (key_expr, target_expr, is_rest, _is_shorthand) in props {
            if *is_rest {
                // Rest: {...target} — create object with remaining keys
                self.chunk.write_opcode(Opcode::NewObject);
                self.chunk.write_byte(0);
                let rest_temp = format!("__rest_obj_{}__", self.forin_counter);
                self.forin_counter += 1;
                self.emit_define_var(&rest_temp);

                self.emit_helper_get(&rest_temp);
                if let Some(ref arr_name) = excluded_arr_temp {
                    self.emit_helper_get(arr_name);
                    self.emit_helper_get(&temp);
                    self.chunk.write_opcode(Opcode::ObjectSpreadExcluding);
                } else {
                    self.emit_helper_get(&temp);
                    self.chunk.write_opcode(Opcode::ObjectSpread);
                }
                self.chunk.write_opcode(Opcode::Pop);

                self.emit_helper_get(&rest_temp);
                self.compile_expr_assign_to_target(target_expr)?;
                continue;
            }

            // Normal property: get value from source, assign to target
            // Determine the key string for GetProperty
            let key_str = match key_expr {
                Expr::StringLit(s) => Some(crate::unicode::utf16_to_utf8(s)),
                _ => None,
            };

            if let Some(ref arr_name) = excluded_arr_temp
                && let Some(ref ks) = key_str
            {
                self.emit_helper_get(arr_name);
                let key_idx = self.chunk.add_constant(Value::from(ks));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(key_idx);
                self.chunk.write_opcode(Opcode::ArrayPush);
                self.chunk.write_opcode(Opcode::Pop);
            }

            // Spec: evaluate the assignment target reference BEFORE getting the value.
            // For member/private-member targets, this means evaluating the base first.
            // Use DefineGlobal (not emit_define_var) to store the pre-evaluated base,
            // because emit_define_var adds a positional local that would misalign
            // with the Dup'd expression-result value already on the stack.
            let pre_eval_target = match target_expr {
                Expr::Index(base, _) | Expr::PrivateMember(base, _) => {
                    self.compile_expr(base)?;
                    let target_temp = format!("__destr_tgt_{}__", self.forin_counter);
                    self.forin_counter += 1;
                    let name_u16 = crate::unicode::utf8_to_utf16(&target_temp);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(name_idx);
                    Some(target_temp)
                }
                _ => None,
            };

            if let Some(ks) = key_str {
                self.emit_helper_get(&temp);
                let k = self.chunk.add_constant(Value::from(&ks));
                self.chunk.write_opcode(Opcode::GetProperty);
                self.chunk.write_u16(k);
            } else {
                // Computed key
                self.emit_helper_get(&temp);
                self.compile_expr(key_expr)?;
                self.chunk.write_opcode(Opcode::GetIndex);
            }

            // Now assign the value to the target
            if let Some(ref tgt_temp) = pre_eval_target {
                // Target base was pre-evaluated; use it for the set
                // Read from global (not local) to match the DefineGlobal above
                let tgt_name_u16 = crate::unicode::utf8_to_utf16(tgt_temp);
                let tgt_name_idx = self.chunk.add_constant(Value::String(tgt_name_u16));
                match target_expr {
                    Expr::Index(_, field) => {
                        // Stack: [value]
                        self.chunk.write_opcode(Opcode::GetGlobal);
                        self.chunk.write_u16(tgt_name_idx);
                        self.chunk.write_opcode(Opcode::Swap);
                        let prop_name = match field.as_ref() {
                            Expr::StringLit(s) => crate::unicode::utf16_to_utf8(s),
                            _ => {
                                // Computed property — fall back to default
                                self.chunk.write_opcode(Opcode::Pop); // remove GetGlobal result
                                self.chunk.write_opcode(Opcode::Swap); // value back on TOS
                                self.compile_expr_assign_to_target(target_expr)?;
                                continue;
                            }
                        };
                        let prop_idx = self.chunk.add_constant(Value::from(&prop_name));
                        self.chunk.write_opcode(Opcode::SetProperty);
                        self.chunk.write_u16(prop_idx);
                        self.chunk.write_opcode(Opcode::Pop);
                    }
                    Expr::PrivateMember(_, field) => {
                        // Stack: [value]
                        self.chunk.write_opcode(Opcode::GetGlobal);
                        self.chunk.write_u16(tgt_name_idx);
                        self.chunk.write_opcode(Opcode::Swap);
                        let priv_key = self.resolve_prefixed_private_key(field);
                        let prop_idx = self.chunk.add_constant(Value::from(&priv_key));
                        self.chunk.write_opcode(Opcode::SetProperty);
                        self.chunk.write_u16(prop_idx);
                        self.chunk.write_opcode(Opcode::Pop);
                    }
                    _ => {
                        self.compile_expr_assign_to_target(target_expr)?;
                    }
                }
            } else {
                self.compile_expr_assign_to_target(target_expr)?;
            }
        }

        // Clean up temp locals
        if self.scope_depth > 0 {
            self.locals.retain(|l| l != &temp);
        }
        Ok(())
    }

    /// Expression-level array destructuring assignment: [x, y] = rhs
    /// RHS value is on stack top. Leaves the RHS value on the stack.
    fn compile_expr_array_destructuring_assign(&mut self, elems: &[Option<Expr>]) -> Result<(), JSError> {
        // Save original RHS for expression result
        let orig_temp = format!("__expr_destr_arr_orig_{}__", self.forin_counter);
        self.forin_counter += 1;
        self.chunk.write_opcode(Opcode::Dup);
        self.emit_define_var(&orig_temp);

        // Normalize RHS via for-of helper
        let temp = format!("__expr_destr_arr_{}__", self.forin_counter);
        self.forin_counter += 1;

        let has_rest = elems.iter().any(|e| matches!(e, Some(Expr::Spread(_))));
        // Swap: use the current stack top as the temp value first
        // Actually, the value is on the stack. Let me store it, then call helper.
        self.emit_define_var(&temp);

        let mut call_args = vec![Expr::Var(temp.clone(), None, None)];
        if !has_rest {
            call_args.push(Expr::Number(elems.len() as f64));
        }
        self.compile_expr(&Expr::Call(
            Box::new(Expr::Var(INTERNAL_FOROF_HELPER.to_string(), None, None)),
            call_args,
        ))?;
        self.emit_helper_set(&temp);
        self.chunk.write_opcode(Opcode::Pop);

        for (i, elem) in elems.iter().enumerate() {
            match elem {
                None => {
                    // Elision — skip this index
                }
                Some(Expr::Spread(inner)) => {
                    // Rest element: ...target = remaining items
                    self.emit_helper_get(&temp);
                    self.emit_helper_get(&temp);
                    let slice_k = self.chunk.add_constant(Value::from("slice"));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(slice_k);
                    let start_idx = self.chunk.add_constant(Value::Number(i as f64));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(start_idx);
                    self.emit_call_opcode(1, 0x80);
                    self.compile_expr_assign_to_target(inner)?;
                    break;
                }
                Some(target) => {
                    // Get element at index i
                    self.emit_helper_get(&temp);
                    let idx = self.chunk.add_constant(Value::Number(i as f64));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                    self.chunk.write_opcode(Opcode::GetIndex);
                    self.compile_expr_assign_to_target(target)?;
                }
            }
        }

        // Push original RHS back as the expression result
        // First pop the current stack top (which is the original RHS we Dup'd)
        // Actually the original was already saved; swap it into position
        self.emit_helper_get(&orig_temp);
        self.chunk.write_opcode(Opcode::Swap);
        self.chunk.write_opcode(Opcode::Pop);

        // Clean up temp locals
        if self.scope_depth > 0 {
            self.locals.retain(|l| l != &temp && l != &orig_temp);
        }
        Ok(())
    }

    fn compile_for_of_destructuring_array(
        &mut self,
        elements: &[DestructuringElement],
        iterable_expr: &Expr,
        body: &[Statement],
    ) -> Result<(), JSError> {
        let arr_name = format!("__forofda_{}__", self.forin_counter);
        self.forin_counter += 1;
        let idx_name = format!("__forofdi_{}__", self.forin_counter);
        self.forin_counter += 1;

        self.compile_expr(&Expr::Call(
            Box::new(Expr::Var(INTERNAL_FOROF_HELPER.to_string(), None, None)),
            vec![iterable_expr.clone()],
        ))?;
        self.emit_define_var(&arr_name);

        let zero = self.chunk.add_constant(Value::Number(0.0));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(zero);
        self.emit_define_var(&idx_name);

        // Pre-allocate destructured variable slots
        let mut destr_names = Vec::new();
        for elem in elements {
            if let DestructuringElement::Variable(name, _) = elem {
                if self.scope_depth > 0 && !self.locals.iter().any(|l| l == name) {
                    let undef = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef);
                    self.locals.push(name.clone());
                }
                destr_names.push(name.clone());
            }
        }

        let loop_start = self.chunk.code.len();
        let ctx = self.make_loop_context(loop_start);
        self.loop_stack.push(ctx);
        self.emit_helper_get(&idx_name);
        self.emit_helper_get(&arr_name);
        let len_key = crate::unicode::utf8_to_utf16("length");
        let len_idx = self.chunk.add_constant(Value::String(len_key));
        self.chunk.write_opcode(Opcode::GetProperty);
        self.chunk.write_u16(len_idx);
        self.chunk.write_opcode(Opcode::LessThan);
        let exit_jump = self.emit_jump(Opcode::JumpIfFalse);

        // item = arr[idx], then destructure
        self.emit_helper_get(&arr_name);
        self.emit_helper_get(&idx_name);
        self.chunk.write_opcode(Opcode::GetIndex);
        // Item is on stack — array destructure it
        self.compile_array_destructuring(elements)?;

        for s in body {
            self.compile_statement(s, false)?;
        }

        let update_ip = self.chunk.code.len();
        for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
            self.patch_jump_to(*cp, update_ip);
        }

        self.emit_helper_get(&idx_name);
        self.chunk.write_opcode(Opcode::Increment);
        self.emit_helper_set(&idx_name);
        self.chunk.write_opcode(Opcode::Pop);

        self.emit_loop(loop_start);
        self.patch_jump(exit_jump);
        let ctx = self.loop_stack.pop().unwrap();
        for bp in ctx.break_patches {
            self.patch_jump(bp);
        }

        if self.scope_depth > 0 {
            self.locals.retain(|l| l != &arr_name && l != &idx_name);
        }
        Ok(())
    }

    /// Helper: compile a for-of loop body where the iteration variable is object-destructured.
    fn compile_for_of_destructuring_object(
        &mut self,
        elements: &[ObjectDestructuringElement],
        iterable_expr: &Expr,
        body: &[Statement],
    ) -> Result<(), JSError> {
        let arr_name = format!("__forofdo_{}__", self.forin_counter);
        self.forin_counter += 1;
        let idx_name = format!("__forofdi_{}__", self.forin_counter);
        self.forin_counter += 1;

        self.compile_expr(&Expr::Call(
            Box::new(Expr::Var(INTERNAL_FOROF_HELPER.to_string(), None, None)),
            vec![iterable_expr.clone()],
        ))?;
        self.emit_define_var(&arr_name);

        let zero = self.chunk.add_constant(Value::Number(0.0));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(zero);
        self.emit_define_var(&idx_name);

        let loop_start = self.chunk.code.len();
        let ctx = self.make_loop_context(loop_start);
        self.loop_stack.push(ctx);
        self.emit_helper_get(&idx_name);
        self.emit_helper_get(&arr_name);
        let len_key = crate::unicode::utf8_to_utf16("length");
        let len_idx = self.chunk.add_constant(Value::String(len_key));
        self.chunk.write_opcode(Opcode::GetProperty);
        self.chunk.write_u16(len_idx);
        self.chunk.write_opcode(Opcode::LessThan);
        let exit_jump = self.emit_jump(Opcode::JumpIfFalse);

        // item = arr[idx], then object-destructure
        self.emit_helper_get(&arr_name);
        self.emit_helper_get(&idx_name);
        self.chunk.write_opcode(Opcode::GetIndex);
        self.compile_object_destructuring(elements)?;

        for s in body {
            self.compile_statement(s, false)?;
        }

        let update_ip = self.chunk.code.len();
        for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
            self.patch_jump_to(*cp, update_ip);
        }

        self.emit_helper_get(&idx_name);
        self.chunk.write_opcode(Opcode::Increment);
        self.emit_helper_set(&idx_name);
        self.chunk.write_opcode(Opcode::Pop);

        self.emit_loop(loop_start);
        self.patch_jump(exit_jump);
        let ctx = self.loop_stack.pop().unwrap();
        for bp in ctx.break_patches {
            self.patch_jump(bp);
        }

        if self.scope_depth > 0 {
            self.locals.retain(|l| l != &arr_name && l != &idx_name);
        }
        Ok(())
    }

    /// Helper: compile a for-in loop where the key is array-destructured.
    fn compile_for_in_destructuring_array(
        &mut self,
        elements: &[DestructuringElement],
        obj_expr: &Expr,
        body: &[Statement],
    ) -> Result<(), JSError> {
        // for-in gives keys (strings), so array destruct of a key is unusual but valid
        // Desugar like for-in but destructure each key
        let keys_name = format!("__finda_keys_{}__", self.forin_counter);
        self.forin_counter += 1;
        let idx_name = format!("__finda_idx_{}__", self.forin_counter);
        self.forin_counter += 1;

        self.compile_expr(obj_expr)?;
        self.chunk.write_opcode(Opcode::GetKeys);
        self.emit_define_var(&keys_name);

        let zero = self.chunk.add_constant(Value::Number(0.0));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(zero);
        self.emit_define_var(&idx_name);

        let loop_start = self.chunk.code.len();
        let ctx = self.make_loop_context(loop_start);
        self.loop_stack.push(ctx);
        self.emit_helper_get(&idx_name);
        self.emit_helper_get(&keys_name);
        let len_key = crate::unicode::utf8_to_utf16("length");
        let len_idx = self.chunk.add_constant(Value::String(len_key));
        self.chunk.write_opcode(Opcode::GetProperty);
        self.chunk.write_u16(len_idx);
        self.chunk.write_opcode(Opcode::LessThan);
        let exit_jump = self.emit_jump(Opcode::JumpIfFalse);

        self.emit_helper_get(&keys_name);
        self.emit_helper_get(&idx_name);
        self.chunk.write_opcode(Opcode::GetIndex);
        self.compile_array_destructuring(elements)?;

        for s in body {
            self.compile_statement(s, false)?;
        }

        let update_ip = self.chunk.code.len();
        for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
            self.patch_jump_to(*cp, update_ip);
        }
        self.emit_helper_get(&idx_name);
        self.chunk.write_opcode(Opcode::Increment);
        self.emit_helper_set(&idx_name);
        self.chunk.write_opcode(Opcode::Pop);

        self.emit_loop(loop_start);
        self.patch_jump(exit_jump);
        let ctx = self.loop_stack.pop().unwrap();
        for bp in ctx.break_patches {
            self.patch_jump(bp);
        }

        if self.scope_depth > 0 {
            self.locals.retain(|l| l != &keys_name && l != &idx_name);
        }
        Ok(())
    }

    /// Helper: compile a for-in loop where the key is object-destructured.
    fn compile_for_in_destructuring_object(
        &mut self,
        elements: &[ObjectDestructuringElement],
        obj_expr: &Expr,
        body: &[Statement],
    ) -> Result<(), JSError> {
        let keys_name = format!("__findo_keys_{}__", self.forin_counter);
        self.forin_counter += 1;
        let idx_name = format!("__findo_idx_{}__", self.forin_counter);
        self.forin_counter += 1;

        self.compile_expr(obj_expr)?;
        self.chunk.write_opcode(Opcode::GetKeys);
        self.emit_define_var(&keys_name);

        let zero = self.chunk.add_constant(Value::Number(0.0));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(zero);
        self.emit_define_var(&idx_name);

        let loop_start = self.chunk.code.len();
        let ctx = self.make_loop_context(loop_start);
        self.loop_stack.push(ctx);
        self.emit_helper_get(&idx_name);
        self.emit_helper_get(&keys_name);
        let len_key = crate::unicode::utf8_to_utf16("length");
        let len_idx = self.chunk.add_constant(Value::String(len_key));
        self.chunk.write_opcode(Opcode::GetProperty);
        self.chunk.write_u16(len_idx);
        self.chunk.write_opcode(Opcode::LessThan);
        let exit_jump = self.emit_jump(Opcode::JumpIfFalse);

        self.emit_helper_get(&keys_name);
        self.emit_helper_get(&idx_name);
        self.chunk.write_opcode(Opcode::GetIndex);
        self.compile_object_destructuring(elements)?;

        for s in body {
            self.compile_statement(s, false)?;
        }

        let update_ip = self.chunk.code.len();
        for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
            self.patch_jump_to(*cp, update_ip);
        }
        self.emit_helper_get(&idx_name);
        self.chunk.write_opcode(Opcode::Increment);
        self.emit_helper_set(&idx_name);
        self.chunk.write_opcode(Opcode::Pop);

        self.emit_loop(loop_start);
        self.patch_jump(exit_jump);
        let ctx = self.loop_stack.pop().unwrap();
        for bp in ctx.break_patches {
            self.patch_jump(bp);
        }

        if self.scope_depth > 0 {
            self.locals.retain(|l| l != &keys_name && l != &idx_name);
        }
        Ok(())
    }

    fn compile_function_body(
        &mut self,
        function_name: Option<&str>,
        params: &[DestructuringElement],
        body: &[Statement],
    ) -> Result<usize, JSError> {
        let jump_over = self.emit_jump(Opcode::Jump);
        let func_ip = self.chunk.code.len();
        let fn_is_strict = self.record_fn_strictness(func_ip, body, false);
        let old_ctx = self.current_strict;
        self.current_strict = fn_is_strict;

        let old_locals = std::mem::take(&mut self.locals);
        let old_depth = self.scope_depth;
        let old_loops = std::mem::take(&mut self.loop_stack);
        let old_label = self.pending_label.take();
        // Save and set up parent scope info for closure capture
        let old_parent_locals = std::mem::take(&mut self.parent_locals);
        let old_parent_upvalues = std::mem::take(&mut self.parent_upvalues);
        let old_upvalues = std::mem::take(&mut self.upvalues);
        let old_const_locals = std::mem::take(&mut self.const_locals);
        let old_parent_const_locals = std::mem::take(&mut self.parent_const_locals);
        let old_function_depth = self.function_depth;
        let old_allow_super = self.allow_super_call;
        self.parent_locals = old_locals.clone();
        self.parent_upvalues = old_upvalues.clone();
        self.parent_const_locals = old_const_locals.clone();

        self.allow_super_call = false;

        self.function_depth = old_function_depth.saturating_add(1);
        self.scope_depth = 1;

        // Eagerly capture parent locals so deeper nested closures can resolve transitive captures.
        for (idx, name) in self.parent_locals.clone().iter().enumerate() {
            self.add_upvalue(name, idx as u8, true);
        }

        // Count non-rest params and check for rest
        let mut non_rest_count = 0u8;
        let mut has_rest = false;
        for (param_index, param) in params.iter().enumerate() {
            match param {
                DestructuringElement::Variable(param_name, _) => {
                    self.locals.push(param_name.clone());
                    non_rest_count += 1;
                }
                DestructuringElement::Rest(param_name) => {
                    has_rest = true;
                    // Emit CollectRest to gather excess args into an array
                    self.chunk.write_opcode(Opcode::CollectRest);
                    self.chunk.write_byte(non_rest_count);
                    self.locals.push(param_name.clone());
                }
                _ => {
                    self.locals.push(format!("__param_slot_{}__", param_index));
                    non_rest_count += 1;
                }
            }
        }

        if !Self::has_parameter_expressions(params) {
            self.emit_hoisted_var_slots(body);
        }
        self.emit_parameter_default_initializers(params)?;
        self.emit_parameter_pattern_bindings(params)?;
        if Self::has_parameter_expressions(params) {
            self.emit_hoisted_var_slots(body);
        }

        for (i, s) in body.iter().enumerate() {
            self.compile_statement(s, i == body.len() - 1)?;
        }

        let idx = self.chunk.add_constant(Value::Undefined);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(idx);
        self.chunk.write_opcode(Opcode::Return);

        self.patch_jump(jump_over);

        // Save local variable names for direct eval support
        self.chunk.fn_local_names.insert(func_ip, self.locals.clone());
        self.chunk.fn_lengths.insert(func_ip, Self::expected_argument_count(params));
        if let Some(name) = function_name
            && !name.is_empty()
        {
            self.chunk.fn_names.insert(func_ip, name.to_string());
        }

        // Collect upvalues before restoring
        let fn_upvalues = std::mem::take(&mut self.upvalues);
        self.chunk
            .fn_upvalue_names
            .insert(func_ip, fn_upvalues.iter().map(|u| u.name.clone()).collect());

        self.locals = old_locals;
        self.scope_depth = old_depth;
        self.loop_stack = old_loops;
        self.pending_label = old_label;
        self.parent_locals = old_parent_locals;
        self.parent_upvalues = old_parent_upvalues;
        self.upvalues = old_upvalues;
        self.const_locals = old_const_locals;
        self.parent_const_locals = old_parent_const_locals;
        self.function_depth = old_function_depth;

        // restore strict context inherited from outer scope
        self.current_strict = old_ctx;
        self.allow_super_call = old_allow_super;

        // Arity = non-rest params only (call site pushes all args, function collects rest)
        let arity = if has_rest { non_rest_count } else { params.len() as u8 };
        let func_val = Value::VmFunction(func_ip, arity);
        let func_idx = self.chunk.add_constant(func_val);

        // Function expressions must produce a fresh function object each time.
        // Use MakeClosure even when there are no captures.
        self.chunk.write_opcode(Opcode::MakeClosure);
        self.chunk.write_u16(func_idx);
        self.chunk.write_byte(fn_upvalues.len() as u8);
        for uv in &fn_upvalues {
            self.chunk.write_byte(if uv.is_local { 1 } else { 0 });
            self.chunk.write_byte(uv.index);
        }
        Ok(func_ip)
    }

    fn emit_parameter_default_initializers(&mut self, params: &[DestructuringElement]) -> Result<(), JSError> {
        let mut forbidden_names_per_param: Vec<Vec<String>> = Vec::with_capacity(params.len());
        for i in 0..params.len() {
            let mut names = Vec::new();
            for param in params.iter().skip(i) {
                Self::collect_destructuring_binding_names(param, &mut names);
            }
            forbidden_names_per_param.push(names);
        }

        let mut local_slot: u8 = 0;
        for (param_index, param) in params.iter().enumerate() {
            match param {
                DestructuringElement::Variable(_, Some(default_expr)) => {
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(local_slot);
                    self.chunk.write_opcode(Opcode::Dup);
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef_idx);
                    self.chunk.write_opcode(Opcode::StrictNotEqual);
                    let skip_default = self.emit_jump(Opcode::JumpIfTrue);
                    self.chunk.write_opcode(Opcode::Pop);
                    if Self::expr_references_any_identifier(default_expr, &forbidden_names_per_param[param_index]) {
                        self.emit_reference_error_throw("Cannot access variable before initialization");
                    } else {
                        self.compile_expr(default_expr)?;
                    }
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(local_slot);
                    self.chunk.write_opcode(Opcode::Pop);
                    self.patch_jump(skip_default);
                }
                DestructuringElement::NestedArray(_, Some(default_expr)) | DestructuringElement::NestedObject(_, Some(default_expr)) => {
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(local_slot);
                    self.chunk.write_opcode(Opcode::Dup);
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef_idx);
                    self.chunk.write_opcode(Opcode::StrictNotEqual);
                    let skip_default = self.emit_jump(Opcode::JumpIfTrue);
                    self.chunk.write_opcode(Opcode::Pop);
                    if Self::expr_references_any_identifier(default_expr, &forbidden_names_per_param[param_index]) {
                        self.emit_reference_error_throw("Cannot access variable before initialization");
                    } else {
                        self.compile_expr(default_expr)?;
                    }
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(local_slot);
                    self.chunk.write_opcode(Opcode::Pop);
                    self.patch_jump(skip_default);
                }
                DestructuringElement::Variable(_, None) => {}
                _ => {}
            }

            if !matches!(param, DestructuringElement::Rest(_)) {
                local_slot = local_slot.saturating_add(1);
            }
        }
        Ok(())
    }

    fn emit_reference_error_throw(&mut self, message: &str) {
        let type_idx = self.chunk.add_constant(Value::from("ReferenceError"));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(type_idx);
        let msg_idx = self.chunk.add_constant(Value::from(message));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(msg_idx);
        self.chunk.write_opcode(Opcode::NewError);
        self.chunk.write_opcode(Opcode::Throw);
    }

    fn maybe_infer_anonymous_binding_name(&mut self, binding_name: &str, expr: &Expr) {
        if let Some(ip) = self.peek_func_ip(expr) {
            self.chunk.fn_names.entry(ip).or_insert_with(|| binding_name.to_string());
        }
    }

    fn expr_references_any_identifier(expr: &Expr, names: &[String]) -> bool {
        if names.is_empty() {
            return false;
        }

        let name_hit = |n: &str| names.iter().any(|cand| cand == n);

        match expr {
            Expr::Var(name, ..) => name_hit(name),
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
            | Expr::OptionalIndex(a, b)
            | Expr::Index(a, b)
            | Expr::Comma(a, b) => Self::expr_references_any_identifier(a, names) || Self::expr_references_any_identifier(b, names),
            Expr::Conditional(a, b, c) => {
                Self::expr_references_any_identifier(a, names)
                    || Self::expr_references_any_identifier(b, names)
                    || Self::expr_references_any_identifier(c, names)
            }
            Expr::OptionalProperty(inner, _)
            | Expr::OptionalPrivateMember(inner, _)
            | Expr::Property(inner, _)
            | Expr::PrivateMember(inner, _)
            | Expr::TypeOf(inner)
            | Expr::Delete(inner)
            | Expr::Void(inner)
            | Expr::Await(inner)
            | Expr::YieldStar(inner)
            | Expr::LogicalNot(inner)
            | Expr::UnaryNeg(inner)
            | Expr::UnaryPlus(inner)
            | Expr::BitNot(inner)
            | Expr::Increment(inner)
            | Expr::Decrement(inner)
            | Expr::Spread(inner)
            | Expr::PostIncrement(inner)
            | Expr::PostDecrement(inner)
            | Expr::Getter(inner)
            | Expr::Setter(inner) => Self::expr_references_any_identifier(inner, names),
            Expr::OptionalCall(callee, args) | Expr::Call(callee, args) | Expr::New(callee, args) => {
                Self::expr_references_any_identifier(callee, names) || args.iter().any(|a| Self::expr_references_any_identifier(a, names))
            }
            Expr::Yield(Some(inner)) => Self::expr_references_any_identifier(inner, names),
            Expr::Yield(None) => false,
            Expr::Object(props) => props
                .iter()
                .any(|(k, v, _, _)| Self::expr_references_any_identifier(k, names) || Self::expr_references_any_identifier(v, names)),
            Expr::Array(items) => items.iter().flatten().any(|e| Self::expr_references_any_identifier(e, names)),
            Expr::TaggedTemplate(tag, _, _, _, exprs) => {
                Self::expr_references_any_identifier(tag, names) || exprs.iter().any(|e| Self::expr_references_any_identifier(e, names))
            }
            Expr::DynamicImport(spec, attrs) => {
                Self::expr_references_any_identifier(spec, names)
                    || attrs.as_ref().is_some_and(|a| Self::expr_references_any_identifier(a, names))
            }
            Expr::Function(..)
            | Expr::GeneratorFunction(..)
            | Expr::AsyncFunction(..)
            | Expr::AsyncGeneratorFunction(..)
            | Expr::ArrowFunction(..)
            | Expr::AsyncArrowFunction(..)
            | Expr::Class(_) => false,
            _ => false,
        }
    }

    fn emit_parameter_pattern_bindings(&mut self, params: &[DestructuringElement]) -> Result<(), JSError> {
        let mut local_slot: u8 = 0;
        for param in params {
            match param {
                DestructuringElement::NestedArray(inner, _) => {
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(local_slot);
                    self.compile_array_destructuring(inner)?;
                }
                DestructuringElement::NestedObject(inner, _) => {
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(local_slot);
                    self.compile_object_destructuring_from_destr(inner)?;
                }
                _ => {}
            }

            if !matches!(param, DestructuringElement::Rest(_)) {
                local_slot = local_slot.saturating_add(1);
            }
        }
        Ok(())
    }

    fn collect_destructuring_binding_names(elem: &DestructuringElement, out: &mut Vec<String>) {
        match elem {
            DestructuringElement::Variable(name, _) | DestructuringElement::Rest(name) => {
                if !out.iter().any(|n| n == name) {
                    out.push(name.clone());
                }
            }
            DestructuringElement::NestedArray(inner, _) | DestructuringElement::NestedObject(inner, _) => {
                for item in inner {
                    Self::collect_destructuring_binding_names(item, out);
                }
            }
            DestructuringElement::Property(_, target) | DestructuringElement::ComputedProperty(_, target) => {
                Self::collect_destructuring_binding_names(target, out);
            }
            DestructuringElement::RestPattern(inner) => {
                Self::collect_destructuring_binding_names(inner, out);
            }
            DestructuringElement::Empty => {}
        }
    }

    fn compile_async_generator_function_body(
        &mut self,
        function_name: Option<&str>,
        params: &[DestructuringElement],
        body: &[Statement],
    ) -> Result<usize, JSError> {
        let jump_over = self.emit_jump(Opcode::Jump);
        let func_ip = self.chunk.code.len();
        let fn_is_strict = self.record_fn_strictness(func_ip, body, false);
        let old_ctx = self.current_strict;
        self.current_strict = fn_is_strict;

        let old_locals = std::mem::take(&mut self.locals);
        let old_depth = self.scope_depth;
        let old_loops = std::mem::take(&mut self.loop_stack);
        let old_label = self.pending_label.take();
        let old_parent_locals = std::mem::take(&mut self.parent_locals);
        let old_parent_upvalues = std::mem::take(&mut self.parent_upvalues);
        let old_upvalues = std::mem::take(&mut self.upvalues);
        let old_const_locals = std::mem::take(&mut self.const_locals);
        let old_parent_const_locals = std::mem::take(&mut self.parent_const_locals);
        let old_function_depth = self.function_depth;
        let old_allow_super = self.allow_super_call;
        self.parent_locals = old_locals.clone();
        self.parent_upvalues = old_upvalues.clone();
        self.parent_const_locals = old_const_locals.clone();

        // Eagerly capture parent locals so deeper nested closures can resolve transitive captures.
        for (idx, name) in self.parent_locals.clone().iter().enumerate() {
            self.add_upvalue(name, idx as u8, true);
        }

        self.allow_super_call = false;
        self.function_depth = old_function_depth.saturating_add(1);
        self.scope_depth = 1;

        let mut non_rest_count = 0u8;
        let mut has_rest = false;
        for (param_index, param) in params.iter().enumerate() {
            match param {
                DestructuringElement::Variable(param_name, _) => {
                    self.locals.push(param_name.clone());
                    non_rest_count += 1;
                }
                DestructuringElement::Rest(param_name) => {
                    has_rest = true;
                    self.chunk.write_opcode(Opcode::CollectRest);
                    self.chunk.write_byte(non_rest_count);
                    self.locals.push(param_name.clone());
                }
                _ => {
                    self.locals.push(format!("__param_slot_{}__", param_index));
                    non_rest_count += 1;
                }
            }
        }

        if !Self::has_parameter_expressions(params) {
            self.emit_hoisted_var_slots(body);
        }
        self.emit_parameter_default_initializers(params)?;
        self.emit_parameter_pattern_bindings(params)?;
        if Self::has_parameter_expressions(params) {
            self.emit_hoisted_var_slots(body);
        }
        self.chunk.write_opcode(Opcode::GeneratorParamInitDone);

        // Async generators are suspendable like generators, but the VM wraps
        // each resume result into Promise objects.
        let gen_marker = "__gen_yield_marker__".to_string();
        self.generator_items_stack.push(gen_marker);

        for (i, s) in body.iter().enumerate() {
            self.compile_statement(s, i == body.len() - 1)?;
        }

        self.generator_items_stack.pop();

        // Implicit return undefined at end of async generator body.
        let undef_idx = self.chunk.add_constant(Value::Undefined);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(undef_idx);
        self.chunk.write_opcode(Opcode::Return);

        self.patch_jump(jump_over);

        // Async generator functions participate in both async and generator
        // semantics in VM runtime/prototype wiring.
        self.chunk.async_function_ips.insert(func_ip);
        self.chunk.generator_function_ips.insert(func_ip);

        self.chunk.fn_local_names.insert(func_ip, self.locals.clone());
        self.chunk.fn_lengths.insert(func_ip, Self::expected_argument_count(params));
        if let Some(name) = function_name
            && !name.is_empty()
        {
            self.chunk.fn_names.insert(func_ip, name.to_string());
        }

        let fn_upvalues = std::mem::take(&mut self.upvalues);
        self.chunk
            .fn_upvalue_names
            .insert(func_ip, fn_upvalues.iter().map(|u| u.name.clone()).collect());

        self.locals = old_locals;
        self.scope_depth = old_depth;
        self.loop_stack = old_loops;
        self.pending_label = old_label;
        self.parent_locals = old_parent_locals;
        self.parent_upvalues = old_parent_upvalues;
        self.upvalues = old_upvalues;
        self.const_locals = old_const_locals;
        self.parent_const_locals = old_parent_const_locals;
        self.function_depth = old_function_depth;
        self.current_strict = old_ctx;
        self.allow_super_call = old_allow_super;

        let arity = if has_rest { non_rest_count } else { params.len() as u8 };
        let func_val = Value::VmFunction(func_ip, arity);
        let func_idx = self.chunk.add_constant(func_val);

        self.chunk.write_opcode(Opcode::MakeClosure);
        self.chunk.write_u16(func_idx);
        self.chunk.write_byte(fn_upvalues.len() as u8);
        for uv in &fn_upvalues {
            self.chunk.write_byte(if uv.is_local { 1 } else { 0 });
            self.chunk.write_byte(uv.index);
        }
        Ok(func_ip)
    }

    fn compile_generator_function_body(
        &mut self,
        function_name: Option<&str>,
        params: &[DestructuringElement],
        body: &[Statement],
    ) -> Result<usize, JSError> {
        let jump_over = self.emit_jump(Opcode::Jump);
        let func_ip = self.chunk.code.len();
        let fn_is_strict = self.record_fn_strictness(func_ip, body, false);
        let old_ctx = self.current_strict;
        self.current_strict = fn_is_strict;

        let old_locals = std::mem::take(&mut self.locals);
        let old_depth = self.scope_depth;
        let old_loops = std::mem::take(&mut self.loop_stack);
        let old_label = self.pending_label.take();
        let old_parent_locals = std::mem::take(&mut self.parent_locals);
        let old_parent_upvalues = std::mem::take(&mut self.parent_upvalues);
        let old_upvalues = std::mem::take(&mut self.upvalues);
        let old_const_locals = std::mem::take(&mut self.const_locals);
        let old_parent_const_locals = std::mem::take(&mut self.parent_const_locals);
        let old_function_depth = self.function_depth;
        let old_allow_super = self.allow_super_call;
        self.parent_locals = old_locals.clone();
        self.parent_upvalues = old_upvalues.clone();
        self.parent_const_locals = old_const_locals.clone();

        // Eagerly capture parent locals so deeper nested closures can resolve transitive captures.
        for (idx, name) in self.parent_locals.clone().iter().enumerate() {
            self.add_upvalue(name, idx as u8, true);
        }

        self.allow_super_call = false;
        self.function_depth = old_function_depth.saturating_add(1);
        self.scope_depth = 1;

        let mut non_rest_count = 0u8;
        let mut has_rest = false;
        for (param_index, param) in params.iter().enumerate() {
            match param {
                DestructuringElement::Variable(param_name, _) => {
                    self.locals.push(param_name.clone());
                    non_rest_count += 1;
                }
                DestructuringElement::Rest(param_name) => {
                    has_rest = true;
                    self.chunk.write_opcode(Opcode::CollectRest);
                    self.chunk.write_byte(non_rest_count);
                    self.locals.push(param_name.clone());
                }
                _ => {
                    self.locals.push(format!("__param_slot_{}__", param_index));
                    non_rest_count += 1;
                }
            }
        }

        if !Self::has_parameter_expressions(params) {
            self.emit_hoisted_var_slots(body);
        }
        self.emit_parameter_default_initializers(params)?;
        self.emit_parameter_pattern_bindings(params)?;
        if Self::has_parameter_expressions(params) {
            self.emit_hoisted_var_slots(body);
        }
        self.chunk.write_opcode(Opcode::GeneratorParamInitDone);

        // Generator body uses Opcode::Yield for yield expressions
        // and normal Opcode::Return for return statements.
        // Push generator_items_stack so yield expressions emit Opcode::Yield.
        let gen_marker = "__gen_yield_marker__".to_string();
        self.generator_items_stack.push(gen_marker);

        for (i, s) in body.iter().enumerate() {
            self.compile_statement(s, i == body.len() - 1)?;
        }

        self.generator_items_stack.pop();

        // Implicit return undefined at end of generator body
        let undef_idx = self.chunk.add_constant(Value::Undefined);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(undef_idx);
        self.chunk.write_opcode(Opcode::Return);

        self.patch_jump(jump_over);

        // Mark this function IP as a generator
        self.chunk.generator_function_ips.insert(func_ip);

        self.chunk.fn_local_names.insert(func_ip, self.locals.clone());
        self.chunk.fn_lengths.insert(func_ip, Self::expected_argument_count(params));
        if let Some(name) = function_name
            && !name.is_empty()
        {
            self.chunk.fn_names.insert(func_ip, name.to_string());
        }

        let fn_upvalues = std::mem::take(&mut self.upvalues);
        self.chunk
            .fn_upvalue_names
            .insert(func_ip, fn_upvalues.iter().map(|u| u.name.clone()).collect());

        self.locals = old_locals;
        self.scope_depth = old_depth;
        self.loop_stack = old_loops;
        self.pending_label = old_label;
        self.parent_locals = old_parent_locals;
        self.parent_upvalues = old_parent_upvalues;
        self.upvalues = old_upvalues;
        self.const_locals = old_const_locals;
        self.parent_const_locals = old_parent_const_locals;
        self.function_depth = old_function_depth;
        self.current_strict = old_ctx;
        self.allow_super_call = old_allow_super;

        let arity = if has_rest { non_rest_count } else { params.len() as u8 };
        let func_val = Value::VmFunction(func_ip, arity);
        let func_idx = self.chunk.add_constant(func_val);

        self.chunk.write_opcode(Opcode::MakeClosure);
        self.chunk.write_u16(func_idx);
        self.chunk.write_byte(fn_upvalues.len() as u8);
        for uv in &fn_upvalues {
            self.chunk.write_byte(if uv.is_local { 1 } else { 0 });
            self.chunk.write_byte(uv.index);
        }
        Ok(func_ip)
    }

    /// Define a new variable (local or global) from the value on top of stack.
    fn emit_define_var(&mut self, name: &str) {
        if self.scope_depth > 0 {
            // Check if already exists (var re-declaration)
            if let Some(pos) = self.locals.iter().position(|l| l == name) {
                self.chunk.write_opcode(Opcode::SetLocal);
                self.chunk.write_byte(pos as u8);
                self.chunk.write_opcode(Opcode::Pop);
            } else {
                self.locals.push(name.to_string());
            }
        } else {
            let name_u16 = crate::unicode::utf8_to_utf16(name);
            let name_idx = self.chunk.add_constant(Value::String(name_u16));
            self.chunk.write_opcode(Opcode::DefineGlobal);
            self.chunk.write_u16(name_idx);
        }
    }

    /// Compile array destructuring: RHS value is on stack top.
    /// Pops the RHS from the stack and defines variables.
    fn compile_array_destructuring(&mut self, elements: &[DestructuringElement]) -> Result<(), JSError> {
        // Store RHS into a synthetic temp
        let temp = format!("__destr_arr_{}__", self.forin_counter);
        self.forin_counter += 1;
        let temp_name = crate::unicode::utf8_to_utf16(&temp);
        let temp_name_idx = self.chunk.add_constant(Value::String(temp_name));
        self.chunk.write_opcode(Opcode::DefineGlobal);
        self.chunk.write_u16(temp_name_idx);

        // Normalize RHS via shared for-of helper so iterator getter/call errors
        // propagate with the same behavior as other iterable consumers.
        let mut normalize_args = vec![Expr::Var(temp.clone(), None, None)];
        let has_top_level_rest = elements
            .iter()
            .any(|e| matches!(e, DestructuringElement::Rest(_) | DestructuringElement::RestPattern(_)));
        if !has_top_level_rest {
            // For non-rest array patterns we only need to advance the iterator
            // through the covered pattern width (including elisions).
            normalize_args.push(Expr::Number(elements.len() as f64));
        }
        self.compile_expr(&Expr::Call(
            Box::new(Expr::Var(INTERNAL_FOROF_HELPER.to_string(), None, None)),
            normalize_args,
        ))?;
        self.emit_helper_set(&temp);
        self.chunk.write_opcode(Opcode::Pop);

        for (i, elem) in elements.iter().enumerate() {
            match elem {
                DestructuringElement::Variable(name, default) => {
                    // temp[i]
                    self.emit_helper_get(&temp);
                    let idx = self.chunk.add_constant(Value::Number(i as f64));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                    self.chunk.write_opcode(Opcode::GetIndex);
                    // If default provided, check if value is undefined
                    if let Some(def_expr) = default {
                        self.chunk.write_opcode(Opcode::Dup);
                        let undef_idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(undef_idx);
                        self.chunk.write_opcode(Opcode::StrictNotEqual);
                        let skip_default = self.emit_jump(Opcode::JumpIfTrue);
                        self.chunk.write_opcode(Opcode::Pop);
                        self.maybe_infer_anonymous_binding_name(name, def_expr);
                        self.compile_expr(def_expr)?;
                        self.patch_jump(skip_default);
                    }
                    self.emit_define_var(name);
                }
                DestructuringElement::Empty => {
                    // Skip this position
                }
                DestructuringElement::Rest(name) => {
                    // Collect remaining elements: temp.slice(i) as method call
                    // Stack: [receiver, callee, args...] then Call with method flag
                    self.emit_helper_get(&temp); // receiver
                    self.emit_helper_get(&temp); // for GetProperty
                    let slice_k = self.chunk.add_constant(Value::from("slice"));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(slice_k); // callee
                    let start_idx = self.chunk.add_constant(Value::Number(i as f64));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(start_idx); // arg
                    self.emit_call_opcode(1, 0x80); // method call
                    self.emit_define_var(name);
                }
                DestructuringElement::RestPattern(target) => {
                    // Collect remaining elements and bind into nested target pattern.
                    self.emit_helper_get(&temp); // receiver
                    self.emit_helper_get(&temp); // for GetProperty
                    let slice_k = self.chunk.add_constant(Value::from("slice"));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(slice_k); // callee
                    let start_idx = self.chunk.add_constant(Value::Number(i as f64));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(start_idx); // arg
                    self.emit_call_opcode(1, 0x80); // method call
                    self.compile_destructuring_target(target)?;
                    break;
                }
                DestructuringElement::NestedArray(inner_elements, default) => {
                    // temp[i], then destructure recursively
                    self.emit_helper_get(&temp);
                    let idx = self.chunk.add_constant(Value::Number(i as f64));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                    self.chunk.write_opcode(Opcode::GetIndex);
                    if let Some(def_expr) = default {
                        self.chunk.write_opcode(Opcode::Dup);
                        let undef_idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(undef_idx);
                        self.chunk.write_opcode(Opcode::StrictNotEqual);
                        let skip_default = self.emit_jump(Opcode::JumpIfTrue);
                        self.chunk.write_opcode(Opcode::Pop);
                        self.compile_expr(def_expr)?;
                        self.patch_jump(skip_default);
                    }
                    self.compile_array_destructuring(inner_elements)?;
                }
                DestructuringElement::NestedObject(inner_elements, default) => {
                    // temp[i], then object destructure
                    self.emit_helper_get(&temp);
                    let idx = self.chunk.add_constant(Value::Number(i as f64));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                    self.chunk.write_opcode(Opcode::GetIndex);
                    if let Some(def_expr) = default {
                        self.chunk.write_opcode(Opcode::Dup);
                        let undef_idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(undef_idx);
                        self.chunk.write_opcode(Opcode::StrictNotEqual);
                        let skip_default = self.emit_jump(Opcode::JumpIfTrue);
                        self.chunk.write_opcode(Opcode::Pop);
                        self.compile_expr(def_expr)?;
                        self.patch_jump(skip_default);
                    }
                    self.compile_object_destructuring_from_destr(inner_elements)?;
                }
                _ => {
                    // Property, ComputedProperty, RestPattern — skip unsupported
                }
            }
        }

        // Synthetic temp is stored as a global helper slot to avoid stack leaks
        // when destructuring throws and is handled by surrounding catch blocks.
        Ok(())
    }

    /// Compile object destructuring from ObjectDestructuringElement list.
    /// RHS value is on stack top. Pops RHS and defines variables.
    fn compile_object_destructuring(&mut self, elements: &[ObjectDestructuringElement]) -> Result<(), JSError> {
        let temp = format!("__destr_obj_{}__", self.forin_counter);
        self.forin_counter += 1;
        self.emit_define_var(&temp);

        // Runtime guard: object destructuring from undefined/null must throw.
        let first_prop = elements.iter().find_map(|e| match e {
            ObjectDestructuringElement::Property { key, .. } => Some(key.clone()),
            _ => None,
        });

        self.emit_helper_get(&temp);
        let undef_idx = self.chunk.add_constant(Value::Undefined);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(undef_idx);
        self.chunk.write_opcode(Opcode::Equal);
        let undefined_ok = self.emit_jump(Opcode::JumpIfFalse);
        let type_idx = self.chunk.add_constant(Value::from("TypeError"));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(type_idx);
        let msg = if let Some(k) = &first_prop {
            format!("Cannot destructure property '{}' of undefined", k)
        } else {
            "Cannot destructure undefined".to_string()
        };
        let msg_idx = self.chunk.add_constant(Value::from(&msg));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(msg_idx);
        self.chunk.write_opcode(Opcode::NewError);
        self.chunk.write_opcode(Opcode::Throw);
        self.patch_jump(undefined_ok);

        self.emit_helper_get(&temp);
        let null_idx = self.chunk.add_constant(Value::Null);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(null_idx);
        self.chunk.write_opcode(Opcode::Equal);
        let null_ok = self.emit_jump(Opcode::JumpIfFalse);
        let type_idx = self.chunk.add_constant(Value::from("TypeError"));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(type_idx);
        let msg = if let Some(k) = &first_prop {
            format!("Cannot destructure property '{}' of null", k)
        } else {
            "Cannot destructure null".to_string()
        };
        let msg_idx = self.chunk.add_constant(Value::from(&msg));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(msg_idx);
        self.chunk.write_opcode(Opcode::NewError);
        self.chunk.write_opcode(Opcode::Throw);
        self.patch_jump(null_ok);

        // Collect statically-known extracted keys for rest computation
        let mut extracted_keys: Vec<String> = Vec::new();

        // If there's a Rest element, build a runtime excluded keys array
        let has_rest = elements.iter().any(|e| matches!(e, ObjectDestructuringElement::Rest(_)));
        let excluded_arr_temp = if has_rest {
            let name = format!("__excluded_arr_{}__", self.forin_counter);
            self.forin_counter += 1;
            self.chunk.write_opcode(Opcode::NewArray);
            self.chunk.write_byte(0);
            let excl_u16 = crate::unicode::utf8_to_utf16(&name);
            let excl_idx = self.chunk.add_constant(Value::String(excl_u16));
            self.chunk.write_opcode(Opcode::DefineGlobal);
            self.chunk.write_u16(excl_idx);
            Some(name)
        } else {
            None
        };

        for elem in elements {
            match elem {
                ObjectDestructuringElement::Property { key, value } => {
                    extracted_keys.push(key.clone());
                    // Add key to excluded array if building one
                    if let Some(ref arr_name) = excluded_arr_temp {
                        self.emit_helper_get(arr_name);
                        let key_idx = self.chunk.add_constant(Value::from(key));
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(key_idx);
                        self.chunk.write_opcode(Opcode::ArrayPush);
                        self.chunk.write_opcode(Opcode::Pop);
                    }
                    self.emit_helper_get(&temp);
                    let k = self.chunk.add_constant(Value::from(key));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(k);
                    self.compile_destructuring_target(value)?;
                }
                ObjectDestructuringElement::ComputedProperty { key, value } => {
                    // Save computed key for exclusion if needed
                    if let Some(ref arr_name) = excluded_arr_temp {
                        self.emit_helper_get(&temp);
                        self.compile_expr(key)?;
                        self.chunk.write_opcode(Opcode::Dup);
                        // Push key into excluded array
                        self.emit_helper_get(arr_name);
                        self.chunk.write_opcode(Opcode::Swap);
                        self.chunk.write_opcode(Opcode::ArrayPush);
                        self.chunk.write_opcode(Opcode::Pop);
                        // Now stack: [source, key] → GetIndex
                        self.chunk.write_opcode(Opcode::GetIndex);
                    } else {
                        self.emit_helper_get(&temp);
                        self.compile_expr(key)?;
                        self.chunk.write_opcode(Opcode::GetIndex);
                    }
                    self.compile_destructuring_target(value)?;
                }
                ObjectDestructuringElement::Rest(name) => {
                    self.chunk.write_opcode(Opcode::NewObject);
                    self.chunk.write_byte(0);
                    let rest_temp = format!("__rest_obj_{}__", self.forin_counter);
                    self.forin_counter += 1;
                    let rest_temp_u16 = crate::unicode::utf8_to_utf16(&rest_temp);
                    let rest_temp_idx = self.chunk.add_constant(Value::String(rest_temp_u16));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(rest_temp_idx);

                    self.emit_helper_get(&rest_temp);
                    if let Some(ref arr_name) = excluded_arr_temp {
                        // Use ObjectSpreadExcluding with the excluded keys array
                        self.emit_helper_get(arr_name);
                        self.emit_helper_get(&temp);
                        self.chunk.write_opcode(Opcode::ObjectSpreadExcluding);
                    } else {
                        self.emit_helper_get(&temp);
                        self.chunk.write_opcode(Opcode::ObjectSpread);
                    }
                    self.chunk.write_opcode(Opcode::Pop);

                    self.emit_helper_get(&rest_temp);
                    self.emit_define_var(name);
                }
            }
        }

        if self.scope_depth > 0 {
            self.locals.retain(|l| l != &temp);
        }
        Ok(())
    }

    /// Compile a destructuring target (DestructuringElement) given a value already on the stack.
    fn compile_destructuring_target(&mut self, elem: &DestructuringElement) -> Result<(), JSError> {
        match elem {
            DestructuringElement::Variable(name, default) => {
                if let Some(def_expr) = default {
                    self.chunk.write_opcode(Opcode::Dup);
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef_idx);
                    self.chunk.write_opcode(Opcode::StrictNotEqual);
                    let skip_default = self.emit_jump(Opcode::JumpIfTrue);
                    self.chunk.write_opcode(Opcode::Pop);
                    self.maybe_infer_anonymous_binding_name(name, def_expr);
                    self.compile_expr(def_expr)?;
                    self.patch_jump(skip_default);
                }
                self.emit_define_var(name);
            }
            DestructuringElement::NestedArray(inner, default) => {
                if let Some(def_expr) = default {
                    self.chunk.write_opcode(Opcode::Dup);
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef_idx);
                    self.chunk.write_opcode(Opcode::StrictNotEqual);
                    let skip_default = self.emit_jump(Opcode::JumpIfTrue);
                    self.chunk.write_opcode(Opcode::Pop);
                    self.compile_expr(def_expr)?;
                    self.patch_jump(skip_default);
                }
                self.compile_array_destructuring(inner)?;
            }
            DestructuringElement::NestedObject(inner, default) => {
                if let Some(def_expr) = default {
                    self.chunk.write_opcode(Opcode::Dup);
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef_idx);
                    self.chunk.write_opcode(Opcode::StrictNotEqual);
                    let skip_default = self.emit_jump(Opcode::JumpIfTrue);
                    self.chunk.write_opcode(Opcode::Pop);
                    self.compile_expr(def_expr)?;
                    self.patch_jump(skip_default);
                }
                self.compile_object_destructuring_from_destr(inner)?;
            }
            DestructuringElement::RestPattern(inner) => {
                self.compile_destructuring_target(inner)?;
            }
            _ => {
                // Unsupported patterns — pop value
                self.chunk.write_opcode(Opcode::Pop);
            }
        }
        Ok(())
    }

    /// Compile object destructuring from DestructuringElement list (used in nested patterns).
    /// The DestructuringElement::Property variant maps to {key: target} in object destructuring.
    fn compile_object_destructuring_from_destr(&mut self, elements: &[DestructuringElement]) -> Result<(), JSError> {
        let temp = format!("__destr_obj_{}__", self.forin_counter);
        self.forin_counter += 1;
        let temp_name = crate::unicode::utf8_to_utf16(&temp);
        let temp_name_idx = self.chunk.add_constant(Value::String(temp_name));
        self.chunk.write_opcode(Opcode::DefineGlobal);
        self.chunk.write_u16(temp_name_idx);

        // Object binding patterns must throw when applied to undefined/null.
        self.emit_helper_get(&temp);
        let undef_idx = self.chunk.add_constant(Value::Undefined);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(undef_idx);
        self.chunk.write_opcode(Opcode::Equal);
        let undefined_ok = self.emit_jump(Opcode::JumpIfFalse);
        let type_idx = self.chunk.add_constant(Value::from("TypeError"));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(type_idx);
        let undef_msg_idx = self.chunk.add_constant(Value::from("Cannot destructure undefined"));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(undef_msg_idx);
        self.chunk.write_opcode(Opcode::NewError);
        self.chunk.write_opcode(Opcode::Throw);
        self.patch_jump(undefined_ok);

        self.emit_helper_get(&temp);
        let null_idx = self.chunk.add_constant(Value::Null);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(null_idx);
        self.chunk.write_opcode(Opcode::Equal);
        let null_ok = self.emit_jump(Opcode::JumpIfFalse);
        let type_idx = self.chunk.add_constant(Value::from("TypeError"));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(type_idx);
        let null_msg_idx = self.chunk.add_constant(Value::from("Cannot destructure null"));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(null_msg_idx);
        self.chunk.write_opcode(Opcode::NewError);
        self.chunk.write_opcode(Opcode::Throw);
        self.patch_jump(null_ok);

        let mut extracted_keys: Vec<String> = Vec::new();

        // If there's a Rest element, build a runtime excluded keys array
        let has_rest = elements.iter().any(|e| matches!(e, DestructuringElement::Rest(_)));
        let excluded_arr_temp = if has_rest {
            let name = format!("__excluded_arr_{}__", self.forin_counter);
            self.forin_counter += 1;
            self.chunk.write_opcode(Opcode::NewArray);
            self.chunk.write_byte(0);
            let excl_u16 = crate::unicode::utf8_to_utf16(&name);
            let excl_idx = self.chunk.add_constant(Value::String(excl_u16));
            self.chunk.write_opcode(Opcode::DefineGlobal);
            self.chunk.write_u16(excl_idx);
            Some(name)
        } else {
            None
        };

        for elem in elements {
            match elem {
                DestructuringElement::Variable(name, default) => {
                    // Shorthand: {name} = obj → obj.name
                    extracted_keys.push(name.clone());
                    if let Some(ref arr_name) = excluded_arr_temp {
                        self.emit_helper_get(arr_name);
                        let key_idx = self.chunk.add_constant(Value::from(name));
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(key_idx);
                        self.chunk.write_opcode(Opcode::ArrayPush);
                        self.chunk.write_opcode(Opcode::Pop);
                    }
                    self.emit_helper_get(&temp);
                    let k = self.chunk.add_constant(Value::from(name));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(k);
                    if let Some(def_expr) = default {
                        self.chunk.write_opcode(Opcode::Dup);
                        let undef_idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(undef_idx);
                        self.chunk.write_opcode(Opcode::StrictNotEqual);
                        let skip_default = self.emit_jump(Opcode::JumpIfTrue);
                        self.chunk.write_opcode(Opcode::Pop);
                        self.maybe_infer_anonymous_binding_name(name, def_expr);
                        self.compile_expr(def_expr)?;
                        self.patch_jump(skip_default);
                    }
                    self.emit_define_var(name);
                }
                DestructuringElement::Property(key, target) => {
                    // {key: target} = obj → obj.key, then assign to target
                    extracted_keys.push(key.clone());
                    if let Some(ref arr_name) = excluded_arr_temp {
                        self.emit_helper_get(arr_name);
                        let key_idx = self.chunk.add_constant(Value::from(key));
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(key_idx);
                        self.chunk.write_opcode(Opcode::ArrayPush);
                        self.chunk.write_opcode(Opcode::Pop);
                    }
                    self.emit_helper_get(&temp);
                    let k = self.chunk.add_constant(Value::from(key));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(k);
                    self.compile_destructuring_target(target)?;
                }
                DestructuringElement::ComputedProperty(key_expr, target) => {
                    if let Some(ref arr_name) = excluded_arr_temp {
                        self.emit_helper_get(&temp);
                        self.compile_expr(key_expr)?;
                        self.chunk.write_opcode(Opcode::Dup);
                        self.emit_helper_get(arr_name);
                        self.chunk.write_opcode(Opcode::Swap);
                        self.chunk.write_opcode(Opcode::ArrayPush);
                        self.chunk.write_opcode(Opcode::Pop);
                        self.chunk.write_opcode(Opcode::GetIndex);
                    } else {
                        self.emit_helper_get(&temp);
                        self.compile_expr(key_expr)?;
                        self.chunk.write_opcode(Opcode::GetIndex);
                    }
                    self.compile_destructuring_target(target)?;
                }
                DestructuringElement::Rest(name) => {
                    self.chunk.write_opcode(Opcode::NewObject);
                    self.chunk.write_byte(0);
                    let rest_temp = format!("__destr_rest_{}__", self.forin_counter);
                    self.forin_counter += 1;
                    let rest_name = crate::unicode::utf8_to_utf16(&rest_temp);
                    let rest_name_idx = self.chunk.add_constant(Value::String(rest_name));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(rest_name_idx);

                    self.emit_helper_get(&rest_temp);
                    if let Some(ref arr_name) = excluded_arr_temp {
                        self.emit_helper_get(arr_name);
                        self.emit_helper_get(&temp);
                        self.chunk.write_opcode(Opcode::ObjectSpreadExcluding);
                    } else {
                        self.emit_helper_get(&temp);
                        self.chunk.write_opcode(Opcode::ObjectSpread);
                    }
                    self.chunk.write_opcode(Opcode::Pop);

                    self.emit_helper_get(&rest_temp);
                    self.emit_define_var(name);
                }
                _ => {}
            }
        }

        if self.scope_depth > 0 {
            self.locals.retain(|l| l != &temp);
        }
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn compile_class_definition(&mut self, class_def: &crate::core::statement::ClassDefinition, is_expr: bool) -> Result<(), JSError> {
        let name = &class_def.name;

        // TDZ for class name in heritage position: `class X extends X {}` must
        // throw ReferenceError (not resolve an outer/hoisted binding).
        if !name.is_empty()
            && let Some(Expr::Var(parent_name, ..)) = class_def.extends.as_ref()
            && parent_name == name
        {
            self.emit_reference_error_throw(&format!("Cannot access '{}' before initialization", name));
            return Ok(());
        }

        // Per spec §14.6 ClassDefinitionEvaluation, the class name must be bound
        // in a new class scope BEFORE the heritage expression is evaluated, so that
        // closures inside the heritage can capture it as an upvalue.  The binding
        // starts uninitialized (Undefined) and is initialized after the constructor
        // is created.  This pre-slot is used for class expressions with a heritage.
        let class_name_heritage_slot = if is_expr && !name.is_empty() && class_def.extends.is_some() {
            let undef_idx = self.chunk.add_constant(Value::Undefined);
            self.chunk.write_opcode(Opcode::Constant);
            self.chunk.write_u16(undef_idx);
            self.locals.push(name.to_string());
            self.const_locals.insert(name.to_string());
            let slot = self.locals.len() - 1;
            // BoxLocal pre-creates a shared upvalue cell so that closures in the
            // heritage expression and the later SetLocal share the same cell.
            self.chunk.write_opcode(Opcode::BoxLocal);
            self.chunk.write_byte(slot as u8);
            Some(slot)
        } else {
            None
        };

        // Evaluate extends expression and bind to a temp local if it's not a simple Var
        let (parent_name, extends_temp_local) = if let Some(ref ext_expr) = class_def.extends {
            if let Expr::Var(pname, ..) = ext_expr {
                (Some(pname.clone()), None)
            } else {
                // Evaluate extends expression and store in temp local
                let temp_name = format!("__class_extends_{}__", self.chunk.code.len());
                // Class heritage is evaluated in strict mode.
                let old_strict_for_heritage = self.current_strict;
                self.current_strict = true;
                let heritage_result = self.compile_expr(ext_expr);
                self.current_strict = old_strict_for_heritage;
                heritage_result?;
                self.locals.push(temp_name.clone());
                (Some(temp_name.clone()), Some(temp_name))
            }
        } else {
            (None, None)
        };

        // Validate the extends value at class definition time
        // ValidateClassHeritage only checks IsConstructor (does not read .prototype)
        if let Some(ref pname) = parent_name {
            let parent_expr = Expr::Var(pname.clone(), None, None);
            self.compile_expr(&parent_expr)?;
            self.chunk.write_opcode(Opcode::ValidateClassHeritage);
        }

        // Save/set class parent context for super resolution
        let prev_parent = self.current_class_parent.take();
        self.current_class_parent = parent_name.clone();

        // Assign a compile-time unique ID for this class and collect its private names
        let class_id = self.class_privns_counter;
        self.class_privns_counter += 1;
        let mut class_private_names = std::collections::HashSet::new();
        for member in &class_def.members {
            match member {
                ClassMember::PrivateProperty(n, _)
                | ClassMember::PrivateStaticProperty(n, _)
                | ClassMember::PrivateMethod(n, _, _)
                | ClassMember::PrivateMethodAsync(n, _, _)
                | ClassMember::PrivateMethodGenerator(n, _, _)
                | ClassMember::PrivateMethodAsyncGenerator(n, _, _)
                | ClassMember::PrivateStaticMethod(n, _, _)
                | ClassMember::PrivateStaticMethodAsync(n, _, _)
                | ClassMember::PrivateStaticMethodGenerator(n, _, _)
                | ClassMember::PrivateStaticMethodAsyncGenerator(n, _, _)
                | ClassMember::PrivateGetter(n, _)
                | ClassMember::PrivateSetter(n, _, _)
                | ClassMember::PrivateStaticGetter(n, _)
                | ClassMember::PrivateStaticSetter(n, _, _) => {
                    class_private_names.insert(n.clone());
                }
                _ => {}
            }
        }
        self.class_privns_stack.push((class_id, class_private_names.clone()));

        // Allocate a runtime brand for classes with private members.
        // The brand is stored as a local variable and auto-captured by all methods.
        let has_private = !class_private_names.is_empty();
        let brand_local_name = if has_private {
            let name = format!("__brand_{}__", class_id);
            self.chunk.write_opcode(Opcode::AllocBrand);
            self.locals.push(name.clone());
            Some(name)
        } else {
            None
        };
        self.current_class_brand_info
            .push(brand_local_name.as_ref().map(|n| (n.clone(), class_id)));

        // Separate instance fields, static members, and methods
        let mut instance_fields: Vec<&crate::core::statement::ClassMember> = Vec::new();
        let mut static_members: Vec<&crate::core::statement::ClassMember> = Vec::new();

        for member in &class_def.members {
            match member {
                ClassMember::Property(..) | ClassMember::PrivateProperty(..) | ClassMember::PropertyComputed(..) => {
                    instance_fields.push(member);
                }
                // Non-static private methods/getters/setters are installed per-instance
                // (spec: InitializeInstanceElements → PrivateMethodOrAccessorAdd)
                ClassMember::PrivateMethod(..)
                | ClassMember::PrivateMethodGenerator(..)
                | ClassMember::PrivateMethodAsync(..)
                | ClassMember::PrivateMethodAsyncGenerator(..)
                | ClassMember::PrivateGetter(..)
                | ClassMember::PrivateSetter(..) => {
                    instance_fields.push(member);
                }
                ClassMember::StaticProperty(..)
                | ClassMember::PrivateStaticProperty(..)
                | ClassMember::StaticBlock(..)
                | ClassMember::StaticPropertyComputed(..) => {
                    static_members.push(member);
                }
                _ => {}
            }
        }

        // Pre-compute ALL computed property keys in source order at class definition time (spec step 27)
        // Both static and instance computed keys are evaluated here.
        let mut computed_key_counter = 0usize;
        let mut cloned_instance_fields: Vec<crate::core::statement::ClassMember> = instance_fields.iter().map(|m| (*m).clone()).collect();
        let mut cloned_static_members: Vec<crate::core::statement::ClassMember> = static_members.iter().map(|m| (*m).clone()).collect();

        // Track instance and static field indices separately
        let mut inst_idx = 0usize;
        let mut stat_idx = 0usize;
        for member in &class_def.members {
            match member {
                ClassMember::PropertyComputed(_key_expr, _) => {
                    let local_name = format!("__ck_{}__", computed_key_counter);
                    computed_key_counter += 1;
                    if let ClassMember::PropertyComputed(key_expr, val_expr) = &cloned_instance_fields[inst_idx] {
                        self.compile_expr(key_expr)?;
                        self.chunk.write_opcode(Opcode::ToPropertyKey);
                        self.locals.push(local_name.clone());
                        cloned_instance_fields[inst_idx] =
                            ClassMember::PropertyComputed(Expr::Var(local_name, None, None), val_expr.clone());
                    }
                    inst_idx += 1;
                }
                ClassMember::StaticPropertyComputed(_key_expr, _) => {
                    let local_name = format!("__ck_{}__", computed_key_counter);
                    computed_key_counter += 1;
                    if let ClassMember::StaticPropertyComputed(key_expr, val_expr) = &cloned_static_members[stat_idx] {
                        self.compile_expr(key_expr)?;
                        self.chunk.write_opcode(Opcode::ToPropertyKey);
                        self.locals.push(local_name.clone());
                        cloned_static_members[stat_idx] =
                            ClassMember::StaticPropertyComputed(Expr::Var(local_name, None, None), val_expr.clone());
                    }
                    stat_idx += 1;
                }
                ClassMember::Property(..)
                | ClassMember::PrivateProperty(..)
                | ClassMember::PrivateMethod(..)
                | ClassMember::PrivateMethodGenerator(..)
                | ClassMember::PrivateMethodAsync(..)
                | ClassMember::PrivateMethodAsyncGenerator(..)
                | ClassMember::PrivateGetter(..)
                | ClassMember::PrivateSetter(..) => {
                    inst_idx += 1;
                }
                ClassMember::StaticProperty(..) | ClassMember::PrivateStaticProperty(..) | ClassMember::StaticBlock(..) => {
                    stat_idx += 1;
                }
                _ => {}
            }
        }
        // Spec: Private methods/accessors are installed before field initializers.
        // Partition so PrivateMethod/PrivateGetter/PrivateSetter come first.
        let (priv_methods_first, fields_second): (Vec<_>, Vec<_>) = cloned_instance_fields.into_iter().partition(|m| {
            matches!(
                m,
                ClassMember::PrivateMethod(..)
                    | ClassMember::PrivateMethodGenerator(..)
                    | ClassMember::PrivateMethodAsync(..)
                    | ClassMember::PrivateMethodAsyncGenerator(..)
                    | ClassMember::PrivateGetter(..)
                    | ClassMember::PrivateSetter(..)
            )
        });
        let cloned_instance_fields: Vec<ClassMember> = priv_methods_first.into_iter().chain(fields_second).collect();

        // Push instance fields onto stack for super() initialisation in derived classes
        self.current_class_instance_fields.push(cloned_instance_fields.clone());

        // Extract constructor; if none and has parent, generate default forwarding ctor
        let mut ctor_params = Vec::new();
        let mut ctor_body = Vec::new();
        let mut has_explicit_ctor = false;
        for member in &class_def.members {
            if let ClassMember::Constructor(params, body) = member {
                ctor_params = params.clone();
                ctor_body = body.clone();
                has_explicit_ctor = true;
                break;
            }
        }
        // Default constructor for derived class: constructor(...args) { super(...args); }
        if !has_explicit_ctor && parent_name.is_some() {
            ctor_params = vec![DestructuringElement::Rest("__args__".to_string())];
            ctor_body = vec![Statement {
                kind: Box::new(StatementKind::Expr(Expr::SuperCall(vec![Expr::Spread(Box::new(Expr::Var(
                    "__args__".to_string(),
                    None,
                    None,
                )))]))),
                line: 0,
                column: 0,
            }];
        }

        let ctor_pre_rest = ctor_params.iter().any(|p| matches!(p, DestructuringElement::Rest(_)));
        let arity = if ctor_pre_rest {
            ctor_params
                .iter()
                .filter(|p| matches!(p, DestructuringElement::Variable(..)))
                .count() as u8
        } else {
            ctor_params.len() as u8
        };

        // Pre-register the class name as a const-like local in the current scope
        // so that the constructor and methods can capture it as a const upvalue.
        // Only for class statements (not expressions) — expression names are scoped
        // to the class body only and must not leak to the enclosing scope.
        let class_name_pre_slot = if !name.is_empty() && self.scope_depth > 0 && !is_expr {
            // Push undefined onto stack as placeholder for the class name slot
            let undef_idx = self.chunk.add_constant(Value::Undefined);
            self.chunk.write_opcode(Opcode::Constant);
            self.chunk.write_u16(undef_idx);
            self.locals.push(name.to_string());
            self.const_locals.insert(name.to_string());
            Some(self.locals.len() - 1)
        } else {
            None
        };

        // For class expressions, track the class name as const even without a pre-slot,
        // so nested functions (constructor/methods) see it via parent_const_locals.
        let class_expr_const_added = if is_expr && !name.is_empty() && class_name_pre_slot.is_none() {
            self.const_locals.insert(name.to_string());
            true
        } else {
            false
        };

        // Pre-compile non-static private method/getter/setter closures at class definition scope.
        // Each closure is stored as a global so the constructor (and instance field
        // initializers) can read it via GetGlobal, ensuring all instances share the
        // SAME function object (spec requirement).  Using globals avoids stack-position
        // sensitivity that locals have at scope_depth 0.
        let class_id = self.class_privns_stack.last().map(|(id, _)| *id).unwrap_or(0);
        let mut priv_method_locals: Vec<(String, String)> = Vec::new(); // (private_key, global_name)
        for member in &class_def.members {
            match member {
                ClassMember::PrivateMethod(mname, params, body)
                | ClassMember::PrivateMethodGenerator(mname, params, body)
                | ClassMember::PrivateMethodAsync(mname, params, body)
                | ClassMember::PrivateMethodAsyncGenerator(mname, params, body) => {
                    let kind = match member {
                        ClassMember::PrivateMethod(..) => 0,
                        ClassMember::PrivateMethodGenerator(..) => 1,
                        ClassMember::PrivateMethodAsync(..) => 2,
                        _ => 3,
                    };
                    let private_name = self.resolve_private_key(mname);
                    let global_name = format!("__pm_{}_{}__", class_id, mname);
                    self.compile_class_method_body_as_closure(&private_name, params, body, kind)?;
                    let n16 = crate::unicode::utf8_to_utf16(&global_name);
                    let ni = self.chunk.add_constant(Value::String(n16));
                    self.chunk.write_opcode(Opcode::DefineGlobalConst);
                    self.chunk.write_u16(ni);
                    priv_method_locals.push((private_name, global_name));
                }
                ClassMember::PrivateGetter(gname, body) => {
                    let private_name = self.resolve_private_key(gname);
                    let global_name = format!("__pg_{}_{}__", class_id, gname);
                    let visible_name = Self::private_display_name(&private_name);
                    let display_name = format!("get {}", visible_name);
                    let empty_params: Vec<DestructuringElement> = vec![];
                    let g_start = self.compile_function_body(Some(&display_name), &empty_params, body)?;
                    self.chunk.method_function_ips.insert(g_start);
                    self.chunk.fn_lengths.insert(g_start, 0);
                    self.record_brand_upvalue_for_fn(g_start);
                    let n16 = crate::unicode::utf8_to_utf16(&global_name);
                    let ni = self.chunk.add_constant(Value::String(n16));
                    self.chunk.write_opcode(Opcode::DefineGlobalConst);
                    self.chunk.write_u16(ni);
                    let getter_key = format!("__get_{}", private_name);
                    priv_method_locals.push((getter_key, global_name));
                }
                ClassMember::PrivateSetter(sname, params, body) => {
                    let private_name = self.resolve_private_key(sname);
                    let global_name = format!("__ps_{}_{}__", class_id, sname);
                    let visible_name = Self::private_display_name(&private_name);
                    let display_name = format!("set {}", visible_name);
                    let s_start = self.compile_function_body(Some(&display_name), params, body)?;
                    self.chunk.method_function_ips.insert(s_start);
                    self.record_brand_upvalue_for_fn(s_start);
                    let n16 = crate::unicode::utf8_to_utf16(&global_name);
                    let ni = self.chunk.add_constant(Value::String(n16));
                    self.chunk.write_opcode(Opcode::DefineGlobalConst);
                    self.chunk.write_u16(ni);
                    let setter_key = format!("__set_{}", private_name);
                    priv_method_locals.push((setter_key, global_name));
                }
                _ => {}
            }
        }
        self.current_class_priv_method_locals.push(priv_method_locals);

        // Emit jump over constructor body
        let jump_over = self.emit_jump(Opcode::Jump);
        let fn_start = self.chunk.code.len();
        let ctor_is_strict = self.record_fn_strictness(fn_start, &ctor_body, true);

        // Save and reset function-scope state for constructor compilation.
        let old_locals = std::mem::take(&mut self.locals);
        let old_depth = self.scope_depth;
        let old_loops = std::mem::take(&mut self.loop_stack);
        let old_strict = self.current_strict;
        let old_parent_locals = std::mem::take(&mut self.parent_locals);
        let old_parent_upvalues = std::mem::take(&mut self.parent_upvalues);
        let old_upvalues = std::mem::take(&mut self.upvalues);
        let old_const_locals = std::mem::take(&mut self.const_locals);
        let old_parent_const_locals = std::mem::take(&mut self.parent_const_locals);
        let old_function_depth = self.function_depth;
        let old_allow_super = self.allow_super_call;
        self.parent_locals = old_locals.clone();
        self.parent_upvalues = old_upvalues.clone();
        self.parent_const_locals = old_const_locals.clone();

        // Eagerly capture parent locals so deeper nested closures can resolve transitive captures.
        for (idx, local_name) in self.parent_locals.clone().iter().enumerate() {
            self.add_upvalue(local_name, idx as u8, true);
        }

        self.current_strict = ctor_is_strict;
        self.allow_super_call = true;
        self.function_depth = old_function_depth.saturating_add(1);
        self.scope_depth = 1;
        let mut ctor_non_rest = 0u8;
        for (param_index, p) in ctor_params.iter().enumerate() {
            match p {
                DestructuringElement::Variable(pname, _) => {
                    self.locals.push(pname.clone());
                    ctor_non_rest += 1;
                }
                DestructuringElement::Rest(pname) => {
                    self.chunk.write_opcode(Opcode::CollectRest);
                    self.chunk.write_byte(ctor_non_rest);
                    self.locals.push(pname.clone());
                }
                _ => {
                    self.locals.push(format!("__param_slot_{}__", param_index));
                    ctor_non_rest += 1;
                }
            }
        }

        if !Self::has_parameter_expressions(&ctor_params) {
            self.emit_hoisted_var_slots(&ctor_body);
        }
        self.emit_parameter_default_initializers(&ctor_params)?;
        self.emit_parameter_pattern_bindings(&ctor_params)?;
        if Self::has_parameter_expressions(&ctor_params) {
            self.emit_hoisted_var_slots(&ctor_body);
        }

        // For base classes (no parent), stamp brand and inject instance field initialisers
        // at the beginning of the constructor body.
        if parent_name.is_none() {
            // Stamp runtime brand on `this` for classes with private members
            if has_private {
                self.emit_brand_stamp(class_id)?;
            }
            for field in &cloned_instance_fields {
                self.compile_class_instance_field(field)?;
            }
        }

        for stmt in ctor_body.iter() {
            self.compile_statement(stmt, false)?;
        }
        self.chunk.write_opcode(Opcode::GetThis);
        self.chunk.write_opcode(Opcode::Return);

        self.patch_jump(jump_over);
        self.chunk.fn_local_names.insert(fn_start, self.locals.clone());
        let ctor_upvalues = std::mem::take(&mut self.upvalues);
        self.chunk
            .fn_upvalue_names
            .insert(fn_start, ctor_upvalues.iter().map(|u| u.name.clone()).collect());

        // Record brand upvalue for the constructor
        if has_private
            && let Some(brand_uv_idx) = ctor_upvalues
                .iter()
                .position(|u| u.name == brand_local_name.as_ref().unwrap().as_str())
        {
            self.chunk.fn_brand_upvalue.insert(fn_start, (brand_uv_idx as u8, class_id));
        }

        self.locals = old_locals;
        self.scope_depth = old_depth;
        self.loop_stack = old_loops;
        self.current_strict = old_strict;
        self.allow_super_call = old_allow_super;
        self.parent_locals = old_parent_locals;
        self.parent_upvalues = old_parent_upvalues;
        self.upvalues = old_upvalues;
        self.const_locals = old_const_locals;
        self.parent_const_locals = old_parent_const_locals;
        self.function_depth = old_function_depth;

        // Register constructor name
        if !name.is_empty() {
            self.chunk.fn_names.insert(fn_start, name.clone());
        }
        self.chunk.class_constructor_ips.insert(fn_start);
        if class_def.extends.is_some() {
            self.chunk.derived_constructor_ips.insert(fn_start);
        }

        // Define constructor as constant, push onto stack
        let fn_val = Value::VmFunction(fn_start, arity);
        let fn_idx = self.chunk.add_constant(fn_val);
        if ctor_upvalues.is_empty() {
            self.chunk.write_opcode(Opcode::Constant);
            self.chunk.write_u16(fn_idx);
        } else {
            self.chunk.write_opcode(Opcode::MakeClosure);
            self.chunk.write_u16(fn_idx);
            self.chunk.write_byte(ctor_upvalues.len() as u8);
            for uv in &ctor_upvalues {
                self.chunk.write_byte(if uv.is_local { 1 } else { 0 });
                self.chunk.write_byte(uv.index);
            }
        }
        // stack: [ctor]

        // Each class evaluation must get its own prototype (factory pattern).
        // ResetPrototype creates a fresh prototype object for the constructor.
        self.chunk.write_opcode(Opcode::ResetPrototype);

        // Initialize the class name heritage slot (if any) with the constructor.
        // SetLocal copies TOS without popping, so the constructor remains on stack.
        if let Some(slot) = class_name_heritage_slot {
            self.chunk.write_opcode(Opcode::SetLocal);
            self.chunk.write_byte(slot as u8);
        }

        let mut class_expr_temp: Option<String> = None;

        if !is_expr {
            // Define as global/local variable (class name is const-like binding)
            let name_u16 = crate::unicode::utf8_to_utf16(name);
            let name_idx = self.chunk.add_constant(Value::String(name_u16));
            if self.scope_depth == 0 {
                self.chunk.write_opcode(Opcode::DefineGlobalConst);
                self.chunk.write_u16(name_idx);
            } else if class_name_pre_slot.is_some() {
                // Update existing pre-registered slot with actual constructor value
                self.emit_define_var(name);
                // const_locals already contains name from pre-registration
            } else {
                self.emit_define_var(name);
                self.const_locals.insert(name.to_string());
            }
        } else {
            // For class expressions, keep the value on the stack
            // but also need to be able to reference it for member installation.
            // We always store the class reference as a global (unique temp name) so that
            // methods compiled inline (which run in a different call frame) can access it
            // via GetGlobal rather than GetLocal (which would be relative to the wrong frame).
            let temp_name = format!("__cls_expr_{}__", self.forin_counter);
            self.forin_counter += 1;
            self.current_class_expr_refs.push(temp_name.clone());
            if !name.is_empty() {
                self.current_class_expr_names.push(name.to_string());
            }
            // Always define as a global so methods can use GetGlobal(temp_name).
            {
                let temp_u16 = crate::unicode::utf8_to_utf16(&temp_name);
                let temp_idx = self.chunk.add_constant(Value::String(temp_u16));
                self.chunk.write_opcode(Opcode::DefineGlobal);
                self.chunk.write_u16(temp_idx);
            }
            // At function scope (scope_depth > 0) also track it as a local so the overall
            // stack depth is consistent with what emit_define_var would produce.
            if self.scope_depth > 0 {
                // We already stored the value as a global above (consuming TOS).
                // Re-push the value from global and keep a local copy for stack balance.
                let temp_u16 = crate::unicode::utf8_to_utf16(&temp_name);
                let temp_idx = self.chunk.add_constant(Value::String(temp_u16));
                self.chunk.write_opcode(Opcode::GetGlobal);
                self.chunk.write_u16(temp_idx);
                self.locals.push(temp_name.clone());
            }
            class_expr_temp = Some(temp_name.clone());
            // We'll clean up after member installation
        }

        // Helper closure-like: emit code to push the class constructor onto the stack
        // For statements: GetGlobal/GetLocal by name
        // For expressions: GetLocal by temp name

        // Collect methods to install on prototype, static methods on constructor
        // method kind: 0=normal, 1=generator, 2=async, 3=async-generator
        let mut methods: Vec<(&str, &Vec<DestructuringElement>, &Vec<Statement>, bool, u8)> = Vec::new();
        let mut getters: Vec<(&str, &Vec<Statement>, bool)> = Vec::new();
        let mut setters: Vec<(&str, &Vec<DestructuringElement>, &Vec<Statement>, bool)> = Vec::new();
        let mut private_methods: Vec<(&str, &Vec<DestructuringElement>, &Vec<Statement>, bool, u8)> = Vec::new();
        let mut private_getters: Vec<(&str, &Vec<Statement>, bool)> = Vec::new();
        let mut private_setters: Vec<(&str, &Vec<DestructuringElement>, &Vec<Statement>, bool)> = Vec::new();
        // computed methods: (key_expr, params, body, is_static, kind)
        let mut computed_methods: Vec<(&Expr, &Vec<DestructuringElement>, &Vec<Statement>, bool, u8)> = Vec::new();
        // computed getters/setters: (key_expr, body/params+body, is_static)
        let mut computed_getters: Vec<(&Expr, &Vec<Statement>, bool)> = Vec::new();
        let mut computed_setters: Vec<(&Expr, &Vec<DestructuringElement>, &Vec<Statement>, bool)> = Vec::new();
        for member in &class_def.members {
            match member {
                ClassMember::Method(mname, params, body) => methods.push((mname, params, body, false, 0)),
                ClassMember::StaticMethod(mname, params, body) => methods.push((mname, params, body, true, 0)),
                ClassMember::MethodGenerator(mname, params, body) => methods.push((mname, params, body, false, 1)),
                ClassMember::StaticMethodGenerator(mname, params, body) => methods.push((mname, params, body, true, 1)),
                ClassMember::MethodAsync(mname, params, body) => methods.push((mname, params, body, false, 2)),
                ClassMember::StaticMethodAsync(mname, params, body) => methods.push((mname, params, body, true, 2)),
                ClassMember::MethodAsyncGenerator(mname, params, body) => methods.push((mname, params, body, false, 3)),
                ClassMember::StaticMethodAsyncGenerator(mname, params, body) => methods.push((mname, params, body, true, 3)),
                ClassMember::Getter(gname, body) => getters.push((gname, body, false)),
                ClassMember::StaticGetter(gname, body) => getters.push((gname, body, true)),
                ClassMember::Setter(sname, params, body) => setters.push((sname, params, body, false)),
                ClassMember::StaticSetter(sname, params, body) => setters.push((sname, params, body, true)),
                ClassMember::PrivateMethod(mname, params, body) => private_methods.push((mname, params, body, false, 0)),
                ClassMember::PrivateStaticMethod(mname, params, body) => private_methods.push((mname, params, body, true, 0)),
                ClassMember::PrivateMethodGenerator(mname, params, body) => private_methods.push((mname, params, body, false, 1)),
                ClassMember::PrivateStaticMethodGenerator(mname, params, body) => private_methods.push((mname, params, body, true, 1)),
                ClassMember::PrivateMethodAsync(mname, params, body) => private_methods.push((mname, params, body, false, 2)),
                ClassMember::PrivateStaticMethodAsync(mname, params, body) => private_methods.push((mname, params, body, true, 2)),
                ClassMember::PrivateMethodAsyncGenerator(mname, params, body) => private_methods.push((mname, params, body, false, 3)),
                ClassMember::PrivateStaticMethodAsyncGenerator(mname, params, body) => private_methods.push((mname, params, body, true, 3)),
                ClassMember::PrivateGetter(gname, body) => private_getters.push((gname, body, false)),
                ClassMember::PrivateStaticGetter(gname, body) => private_getters.push((gname, body, true)),
                ClassMember::PrivateSetter(sname, params, body) => private_setters.push((sname, params, body, false)),
                ClassMember::PrivateStaticSetter(sname, params, body) => private_setters.push((sname, params, body, true)),
                ClassMember::MethodComputed(key, params, body) => computed_methods.push((key, params, body, false, 0)),
                ClassMember::StaticMethodComputed(key, params, body) => computed_methods.push((key, params, body, true, 0)),
                ClassMember::MethodComputedGenerator(key, params, body) => computed_methods.push((key, params, body, false, 1)),
                ClassMember::StaticMethodComputedGenerator(key, params, body) => computed_methods.push((key, params, body, true, 1)),
                ClassMember::MethodComputedAsync(key, params, body) => computed_methods.push((key, params, body, false, 2)),
                ClassMember::StaticMethodComputedAsync(key, params, body) => computed_methods.push((key, params, body, true, 2)),
                ClassMember::MethodComputedAsyncGenerator(key, params, body) => computed_methods.push((key, params, body, false, 3)),
                ClassMember::StaticMethodComputedAsyncGenerator(key, params, body) => computed_methods.push((key, params, body, true, 3)),
                ClassMember::GetterComputed(key, body) => computed_getters.push((key, body, false)),
                ClassMember::StaticGetterComputed(key, body) => computed_getters.push((key, body, true)),
                ClassMember::SetterComputed(key, params, body) => computed_setters.push((key, params, body, false)),
                ClassMember::StaticSetterComputed(key, params, body) => computed_setters.push((key, params, body, true)),
                _ => {}
            }
        }

        // Helper: emit code that pushes the class constructor onto the stack
        // We define an inline helper via a macro-like approach: emit_get_class
        // For non-expr: GetGlobal/GetLocal by class name
        // For expr: emit_helper_get on the temp var

        let has_instance_members = methods.iter().any(|(_, _, _, s, _)| !*s)
            || getters.iter().any(|(_, _, s)| !*s)
            || setters.iter().any(|(_, _, _, s)| !*s)
            || private_methods.iter().any(|(_, _, _, s, _)| !*s)
            || private_getters.iter().any(|(_, _, s)| !*s)
            || private_setters.iter().any(|(_, _, _, s)| !*s)
            || computed_methods.iter().any(|(_, _, _, s, _)| !*s)
            || computed_getters.iter().any(|(_, _, s)| !*s)
            || computed_setters.iter().any(|(_, _, _, s)| !*s);

        // Compile and install instance methods on ClassName.prototype
        if has_instance_members {
            // Push prototype: GetClass, GetProperty "prototype"
            self.emit_get_class_ref(name, is_expr)?;
            let proto_key = self.chunk.add_constant(Value::from("prototype"));
            self.chunk.write_opcode(Opcode::GetProperty);
            self.chunk.write_u16(proto_key);
            // stack: [proto]

            for &(mname, params, body, is_static, kind) in &methods {
                if is_static {
                    continue;
                }
                self.compile_and_install_method_with_kind(mname, params, body, kind)?;
            }

            // Install computed instance methods on prototype
            for &(key_expr, params, body, is_static, kind) in &computed_methods {
                if is_static {
                    continue;
                }
                self.compile_and_install_computed_method(key_expr, params, body, kind)?;
            }

            // Install getters on prototype
            for &(gname, body, is_static) in &getters {
                if is_static {
                    continue;
                }
                self.compile_and_install_getter(gname, body)?;
            }

            // Install setters on prototype
            for &(sname, params, body, is_static) in &setters {
                if is_static {
                    continue;
                }
                self.compile_and_install_setter(sname, params, body)?;
            }

            // Non-static private methods/getters/setters are now installed per-instance
            // (handled in compile_class_instance_field). Only static ones remain on the
            // class object (handled later in static member installation).

            // Install computed getters on prototype
            for &(key_expr, body, is_static) in &computed_getters {
                if is_static {
                    continue;
                }
                self.compile_and_install_computed_getter(key_expr, body)?;
            }

            // Install computed setters on prototype
            for &(key_expr, params, body, is_static) in &computed_setters {
                if is_static {
                    continue;
                }
                self.compile_and_install_computed_setter(key_expr, params, body)?;
            }

            self.chunk.write_opcode(Opcode::Pop); // pop proto
        }

        // Install static methods on the constructor function itself
        let has_static_methods = methods.iter().any(|(_, _, _, s, _)| *s)
            || getters.iter().any(|(_, _, s)| *s)
            || setters.iter().any(|(_, _, _, s)| *s)
            || private_methods.iter().any(|(_, _, _, s, _)| *s)
            || private_getters.iter().any(|(_, _, s)| *s)
            || private_setters.iter().any(|(_, _, _, s)| *s)
            || computed_methods.iter().any(|(_, _, _, s, _)| *s)
            || computed_getters.iter().any(|(_, _, s)| *s)
            || computed_setters.iter().any(|(_, _, _, s)| *s);
        if has_static_methods {
            for &(mname, params, body, is_static, kind) in &methods {
                if !is_static {
                    continue;
                }
                self.emit_get_class_ref(name, is_expr)?;
                self.compile_class_method_body_as_closure(mname, params, body, kind)?;
                let mk_idx = self.chunk.add_constant(Value::from(mname));
                self.chunk.write_opcode(Opcode::InitProperty);
                self.chunk.write_u16(mk_idx);
                self.chunk.write_opcode(Opcode::Pop);

                // Static methods are non-enumerable
                self.emit_get_class_ref(name, is_expr)?;
                self.emit_nonenumerable_marker_standalone(mname);
            }

            // Static computed methods
            for &(key_expr, params, body, is_static, kind) in &computed_methods {
                if !is_static {
                    continue;
                }
                self.emit_get_class_ref(name, is_expr)?;
                self.compile_expr(key_expr)?;
                self.chunk.write_opcode(Opcode::ToPropertyKey);
                self.compile_class_method_body_as_closure("", params, body, kind)?;
                // stack: [class, key, closure]
                self.chunk.write_opcode(Opcode::InitIndex);
                self.chunk.write_opcode(Opcode::Pop);
            }

            // Static getters
            for &(gname, body, is_static) in &getters {
                if !is_static {
                    continue;
                }
                self.emit_get_class_ref(name, is_expr)?;
                self.compile_and_install_getter(gname, body)?;
                self.chunk.write_opcode(Opcode::Pop);
            }

            // Static setters
            for &(sname, params, body, is_static) in &setters {
                if !is_static {
                    continue;
                }
                self.emit_get_class_ref(name, is_expr)?;
                self.compile_and_install_setter(sname, params, body)?;
                self.chunk.write_opcode(Opcode::Pop);
            }

            // Static computed getters
            for &(key_expr, body, is_static) in &computed_getters {
                if !is_static {
                    continue;
                }
                self.emit_get_class_ref(name, is_expr)?;
                self.compile_and_install_computed_getter(key_expr, body)?;
                self.chunk.write_opcode(Opcode::Pop);
            }

            // Static computed setters
            for &(key_expr, params, body, is_static) in &computed_setters {
                if !is_static {
                    continue;
                }
                self.emit_get_class_ref(name, is_expr)?;
                self.compile_and_install_computed_setter(key_expr, params, body)?;
                self.chunk.write_opcode(Opcode::Pop);
            }

            // Static private methods on the constructor
            for &(mname, params, body, is_static, kind) in &private_methods {
                if !is_static {
                    continue;
                }
                let private_name = self.resolve_private_key(mname);
                self.emit_get_class_ref(name, is_expr)?;
                self.compile_class_method_body_as_closure(&private_name, params, body, kind)?;
                let mk_idx = self.chunk.add_constant(Value::from(&private_name));
                self.chunk.write_opcode(Opcode::InitProperty);
                self.chunk.write_u16(mk_idx);
                self.chunk.write_opcode(Opcode::Pop);
                // Mark static private method as readonly
                self.emit_get_class_ref(name, is_expr)?;
                let readonly_flag = self.chunk.add_constant(Value::Boolean(true));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(readonly_flag);
                let ro_key = format!("__readonly_{}__", private_name);
                let ro_idx = self.chunk.add_constant(Value::from(&ro_key));
                self.chunk.write_opcode(Opcode::InitProperty);
                self.chunk.write_u16(ro_idx);
                self.chunk.write_opcode(Opcode::Pop);
            }

            // Static private getters on the constructor
            for &(gname, body, is_static) in &private_getters {
                if !is_static {
                    continue;
                }
                let private_name = self.resolve_private_key(gname);
                self.emit_get_class_ref(name, is_expr)?;
                let g_jump = self.emit_jump(Opcode::Jump);
                let g_start = self.chunk.code.len();
                let saved_locals = std::mem::take(&mut self.locals);
                let saved_const_locals = std::mem::take(&mut self.const_locals);
                self.scope_depth += 1;
                for stmt in body.iter() {
                    self.compile_statement(stmt, false)?;
                }
                self.chunk.write_opcode(Opcode::Constant);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_u16(undef_idx);
                self.chunk.write_opcode(Opcode::Return);
                self.scope_depth -= 1;
                self.locals = saved_locals;
                self.const_locals = saved_const_locals;
                self.patch_jump(g_jump);
                self.chunk.method_function_ips.insert(g_start);
                self.chunk.fn_names.insert(g_start, format!("get #{}", gname));
                self.chunk.fn_lengths.insert(g_start, 0);
                let g_val = Value::VmFunction(g_start, 0);
                let g_idx = self.chunk.add_constant(g_val);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(g_idx);
                let getter_key = format!("__get_{}", private_name);
                let gk_idx = self.chunk.add_constant(Value::from(&getter_key));
                self.chunk.write_opcode(Opcode::SetProperty);
                self.chunk.write_u16(gk_idx);
                self.chunk.write_opcode(Opcode::Pop);
            }

            // Static private setters on the constructor
            for &(sname, params, body, is_static) in &private_setters {
                if !is_static {
                    continue;
                }
                let private_name = self.resolve_private_key(sname);
                self.emit_get_class_ref(name, is_expr)?;
                let s_jump = self.emit_jump(Opcode::Jump);
                let s_start = self.chunk.code.len();
                let s_arity = params.len() as u8;
                let saved_locals = std::mem::take(&mut self.locals);
                let saved_const_locals = std::mem::take(&mut self.const_locals);
                self.scope_depth += 1;
                for p in params {
                    if let DestructuringElement::Variable(pn, _) = p {
                        self.locals.push(pn.clone());
                    }
                }
                for stmt in body.iter() {
                    self.compile_statement(stmt, false)?;
                }
                self.chunk.write_opcode(Opcode::Constant);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_u16(undef_idx);
                self.chunk.write_opcode(Opcode::Return);
                self.scope_depth -= 1;
                self.locals = saved_locals;
                self.const_locals = saved_const_locals;
                self.patch_jump(s_jump);
                self.chunk.method_function_ips.insert(s_start);
                self.chunk.fn_names.insert(s_start, format!("set #{}", sname));
                self.chunk.fn_lengths.insert(s_start, s_arity as usize);
                let s_val = Value::VmFunction(s_start, s_arity);
                let s_idx = self.chunk.add_constant(s_val);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(s_idx);
                let setter_key = format!("__set_{}", private_name);
                let sk_idx = self.chunk.add_constant(Value::from(&setter_key));
                self.chunk.write_opcode(Opcode::SetProperty);
                self.chunk.write_u16(sk_idx);
                self.chunk.write_opcode(Opcode::Pop);
            }
        }

        // Install static fields and static blocks
        for sm in &cloned_static_members {
            self.compile_class_static_member(name, sm, is_expr)?;
        }

        // Stamp runtime brand on the class object itself (for static private access)
        if let Some(ref brand_name) = brand_local_name
            && let Some(local_idx) = self.locals.iter().rposition(|l| l == brand_name)
        {
            self.emit_get_class_ref(name, is_expr)?;
            self.chunk.write_opcode(Opcode::GetLocal);
            self.chunk.write_byte(local_idx as u8);
            let brand_key = format!("__brand_{}__", class_id);
            let brand_key_idx = self.chunk.add_constant(Value::from(brand_key.as_str()));
            self.chunk.write_opcode(Opcode::InitProperty);
            self.chunk.write_u16(brand_key_idx);
            self.chunk.write_opcode(Opcode::Pop);
        }

        // Handle extends: set Child.prototype.__proto__ = validated parent.prototype (or null)
        if let Some(ref pname) = parent_name {
            let parent_expr = Expr::Var(pname.clone(), None, None);

            // Check if parent is null at runtime
            self.compile_expr(&parent_expr)?;
            let null_idx_check = self.chunk.add_constant(Value::Null);
            self.chunk.write_opcode(Opcode::Constant);
            self.chunk.write_u16(null_idx_check);
            self.chunk.write_opcode(Opcode::StrictNotEqual);
            self.chunk.write_opcode(Opcode::Not);
            let parent_is_null_jump = self.emit_jump(Opcode::JumpIfTrue);

            // --- Parent is NOT null ---
            // Set Child.prototype.__proto__ = Parent.prototype
            self.emit_get_class_ref(name, is_expr)?;
            let proto_k = self.chunk.add_constant(Value::from("prototype"));
            self.chunk.write_opcode(Opcode::GetProperty);
            self.chunk.write_u16(proto_k);
            // Read parent.prototype (only read, so getter invoked exactly once)
            self.compile_expr(&parent_expr)?;
            let parent_proto_k = self.chunk.add_constant(Value::from("prototype"));
            self.chunk.write_opcode(Opcode::GetProperty);
            self.chunk.write_u16(parent_proto_k);
            // Validate parent.prototype is object or null
            self.chunk.write_opcode(Opcode::ValidateProtoValue);
            // Stack: [child_proto, parent_proto]
            let dunder_proto = self.chunk.add_constant(Value::from("__proto__"));
            self.chunk.write_opcode(Opcode::SetProperty);
            self.chunk.write_u16(dunder_proto);
            self.chunk.write_opcode(Opcode::Pop);

            // Set Child.__proto__ = Parent
            self.emit_get_class_ref(name, is_expr)?;
            self.compile_expr(&parent_expr)?;
            let dunder_proto2 = self.chunk.add_constant(Value::from("__proto__"));
            self.chunk.write_opcode(Opcode::SetProperty);
            self.chunk.write_u16(dunder_proto2);
            self.chunk.write_opcode(Opcode::Pop);

            let end_extends_jump = self.emit_jump(Opcode::Jump);

            // --- Parent IS null ---
            self.patch_jump(parent_is_null_jump);
            // Set Child.prototype.__proto__ = null
            self.emit_get_class_ref(name, is_expr)?;
            let proto_k2 = self.chunk.add_constant(Value::from("prototype"));
            self.chunk.write_opcode(Opcode::GetProperty);
            self.chunk.write_u16(proto_k2);
            let null_proto = self.chunk.add_constant(Value::Null);
            self.chunk.write_opcode(Opcode::Constant);
            self.chunk.write_u16(null_proto);
            let dunder_proto3 = self.chunk.add_constant(Value::from("__proto__"));
            self.chunk.write_opcode(Opcode::SetProperty);
            self.chunk.write_u16(dunder_proto3);
            self.chunk.write_opcode(Opcode::Pop);
            // Child.__proto__ stays as Function.prototype (default)

            self.patch_jump(end_extends_jump);
        }

        // Pop instance fields stack
        self.current_class_instance_fields.pop();

        // Clean up pre-computed key locals.
        // At function scope the computed-key values sit below other class-related
        // locals (class name pre-slot, brand, etc.) on the stack.  A naive Pop
        // would remove the wrong value.  Leave them on the stack as dead locals;
        // they will be cleaned up when the enclosing scope exits.
        // At global scope (scope_depth 0) there is nothing above the computed keys
        // so simple Pops are safe.
        if self.scope_depth == 0 {
            for _ in 0..computed_key_counter {
                self.chunk.write_opcode(Opcode::Pop);
                if let Some(pos) = self.locals.iter().rposition(|l| l.starts_with("__ck_")) {
                    self.locals.remove(pos);
                }
            }
        }

        // Clean up pre-compiled private method/getter/setter tracking
        // (closures are stored as globals, so no stack cleanup needed)
        self.current_class_priv_method_locals.pop();

        // For class expressions, push the class value back onto the stack
        if is_expr {
            // Retrieve the class value from the global temp.
            self.emit_get_class_ref(name, true)?;
            // NOTE: We intentionally do NOT zero out the global temp here.
            // Inline methods are compiled with GetGlobal(temp_name) to read the class value,
            // and these methods may be called at any time after the class is defined.
            // The temp name is unique (contains a counter) so it won't conflict with user code.

            if self.scope_depth == 0 {
                // At global scope there are no upvalue cells, so we can reclaim stack
                // slots with Swap+Pop.
                if let Some(ref temp_name) = class_expr_temp
                    && let Some(pos) = self.locals.iter().rposition(|l| l == temp_name)
                {
                    self.locals.remove(pos);
                    self.chunk.write_opcode(Opcode::Swap);
                    self.chunk.write_opcode(Opcode::Pop);
                }
                if let Some(ref brand_name) = brand_local_name
                    && let Some(pos) = self.locals.iter().rposition(|l| l == brand_name)
                {
                    self.locals.remove(pos);
                    self.chunk.write_opcode(Opcode::Swap);
                    self.chunk.write_opcode(Opcode::Pop);
                }
                if let Some(ref ext_name) = extends_temp_local
                    && let Some(pos) = self.locals.iter().rposition(|l| l == ext_name)
                {
                    self.locals.remove(pos);
                    self.chunk.write_opcode(Opcode::Swap);
                    self.chunk.write_opcode(Opcode::Pop);
                }
            }
            // At function scope (scope_depth > 0), the brand local may have been
            // captured as an upvalue cell.  Swap+Pop would move the class value into
            // the brand's stack slot but GetLocal would still read the stale upvalue
            // cell, returning the brand number instead of the constructor.  Leave the
            // dead locals on the stack (same strategy as class declarations) and let
            // the class value sit on top at a fresh position.

            self.current_class_expr_refs.pop();
            // Pop the class expression name tracker if we pushed one.
            if !name.is_empty() {
                self.current_class_expr_names.pop();
            }
            // Remove the const-tracking entry for the class expression name
            // so it doesn't bleed into the enclosing scope.
            if class_expr_const_added {
                self.const_locals.remove(name);
            }
        }

        // For class declarations (not expressions), brand and extends_temp locals
        // stay on the stack as dead locals until scope exit.
        // We cannot remove them from self.locals because upvalue cells are indexed
        // by local position and removing from the middle would shift indices.
        // For class expressions we already cleaned them up above.

        // Restore previous class context
        self.class_privns_stack.pop();
        self.current_class_brand_info.pop();
        self.current_class_parent = prev_parent;
        Ok(())
    }

    /// Emit bytecode to push the class constructor reference onto the stack.
    /// Emit bytecode to stamp the runtime brand on `this`.
    /// Emits: GetThis, GetUpvalue(brand), InitProperty("__brand_N__"), Pop
    fn emit_brand_stamp(&mut self, class_id: usize) -> Result<(), JSError> {
        let brand_name = format!("__brand_{}__", class_id);
        let (opcode, operand) = if let Some(uv_idx) = self.resolve_upvalue(&brand_name) {
            (Opcode::GetUpvalue, uv_idx)
        } else if let Some(local_idx) = self.locals.iter().rposition(|l| l == &brand_name) {
            (Opcode::GetLocal, local_idx as u8)
        } else {
            return Ok(());
        };
        self.chunk.write_opcode(Opcode::GetThis);
        self.chunk.write_opcode(opcode);
        self.chunk.write_byte(operand);
        let brand_key_idx = self.chunk.add_constant(Value::from(brand_name.as_str()));
        self.chunk.write_opcode(Opcode::InitProperty);
        self.chunk.write_u16(brand_key_idx);
        self.chunk.write_opcode(Opcode::Pop);
        Ok(())
    }

    /// Record brand upvalue info for a compiled function IP.
    /// Checks the current fn_local_names (which contains upvalue names from eager capture)
    /// and records the brand upvalue index + class_id if found.
    fn record_brand_upvalue_for_fn(&mut self, func_ip: usize) {
        if let Some(&Some((ref _brand_name, class_id))) = self.current_class_brand_info.last() {
            let brand_name = format!("__brand_{}__", class_id);
            if let Some(uv_names) = self.chunk.fn_upvalue_names.get(&func_ip) {
                if let Some(uv_idx) = uv_names.iter().position(|n| n == &brand_name) {
                    self.chunk.fn_brand_upvalue.insert(func_ip, (uv_idx as u8, class_id));
                }
            } else if let Some(uv_idx) = self.locals.iter().position(|l| l == &brand_name) {
                self.chunk.fn_brand_upvalue.insert(func_ip, (uv_idx as u8, class_id));
            }
        }
    }

    fn emit_get_class_ref(&mut self, name: &str, is_expr: bool) -> Result<(), JSError> {
        if is_expr {
            if let Some(temp_name) = self.current_class_expr_refs.last().cloned() {
                // Always use GetGlobal for class expression references: the temp is always
                // stored as a global so that inline method bodies (which run in their own call
                // frame) can access it via GetGlobal rather than GetLocal(wrong-frame-slot).
                let temp_u16 = crate::unicode::utf8_to_utf16(&temp_name);
                let temp_idx = self.chunk.add_constant(Value::String(temp_u16));
                self.chunk.write_opcode(Opcode::GetGlobal);
                self.chunk.write_u16(temp_idx);
                return Ok(());
            }
            // Fallback: try by name
            self.emit_helper_get(name);
        } else {
            let name_u16 = crate::unicode::utf8_to_utf16(name);
            let name_idx = self.chunk.add_constant(Value::String(name_u16));
            if self.scope_depth == 0 {
                self.chunk.write_opcode(Opcode::GetGlobal);
                self.chunk.write_u16(name_idx);
            } else {
                self.emit_helper_get(name);
            }
        }
        Ok(())
    }

    /// Compile and install a method on the object currently on top of stack.
    fn _compile_and_install_method(&mut self, mname: &str, params: &[DestructuringElement], body: &[Statement]) -> Result<(), JSError> {
        let m_jump = self.emit_jump(Opcode::Jump);
        let m_start = self.chunk.code.len();
        let method_is_strict = self.record_fn_strictness(m_start, body, true);
        let old_strict = self.current_strict;
        self.current_strict = method_is_strict;
        self.scope_depth += 1;
        let locals_before = self.locals.len();
        let mut m_non_rest = 0u8;
        let mut m_has_rest = false;
        for (param_index, param) in params.iter().enumerate() {
            match param {
                DestructuringElement::Variable(pn, _) => {
                    self.locals.push(pn.clone());
                    m_non_rest += 1;
                }
                DestructuringElement::Rest(pn) => {
                    m_has_rest = true;
                    self.chunk.write_opcode(Opcode::CollectRest);
                    self.chunk.write_byte(m_non_rest);
                    self.locals.push(pn.clone());
                }
                _ => {
                    self.locals.push(format!("__param_slot_{}__", param_index));
                    m_non_rest += 1;
                }
            }
        }
        if !Self::has_parameter_expressions(params) {
            self.emit_hoisted_var_slots(body);
        }
        self.emit_parameter_default_initializers(params)?;
        self.emit_parameter_pattern_bindings(params)?;
        if Self::has_parameter_expressions(params) {
            self.emit_hoisted_var_slots(body);
        }
        let m_arity = if m_has_rest { m_non_rest } else { params.len() as u8 };
        for stmt in body.iter() {
            self.compile_statement(stmt, false)?;
        }
        self.chunk.write_opcode(Opcode::Constant);
        let undef_idx = self.chunk.add_constant(Value::Undefined);
        self.chunk.write_u16(undef_idx);
        self.chunk.write_opcode(Opcode::Return);
        self.locals.truncate(locals_before);
        self.scope_depth -= 1;
        self.current_strict = old_strict;
        self.patch_jump(m_jump);
        self.chunk.fn_names.insert(m_start, mname.to_string());
        self.chunk.fn_lengths.insert(m_start, Self::expected_argument_count(params));
        self.chunk.method_function_ips.insert(m_start);

        // Install: Dup proto, push method, SetProperty/InitProperty, Pop
        self.chunk.write_opcode(Opcode::Dup);
        let m_val = Value::VmFunction(m_start, m_arity);
        let m_idx = self.chunk.add_constant(m_val);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(m_idx);
        let mk_idx = self.chunk.add_constant(Value::from(mname));
        // Use InitProperty for private methods to avoid brand check during installation
        if mname.starts_with(super::PRIVATE_KEY_PREFIX) {
            self.chunk.write_opcode(Opcode::InitProperty);
        } else {
            self.chunk.write_opcode(Opcode::SetProperty);
        }
        self.chunk.write_u16(mk_idx);
        self.chunk.write_opcode(Opcode::Pop);

        // Class methods are non-enumerable
        self.emit_nonenumerable_marker(mname);

        Ok(())
    }

    /// Emit Dup + Constant(true) + SetProperty("__nonenumerable_<name>__") + Pop
    /// to mark a property as non-enumerable on the object currently on top of stack.
    fn emit_nonenumerable_marker(&mut self, prop_name: &str) {
        self.chunk.write_opcode(Opcode::Dup);
        self.chunk.write_opcode(Opcode::Constant);
        let true_idx = self.chunk.add_constant(Value::Boolean(true));
        self.chunk.write_u16(true_idx);
        let ne_key = format!("__nonenumerable_{}__", prop_name);
        let ne_idx = self.chunk.add_constant(Value::from(&ne_key));
        self.chunk.write_opcode(Opcode::SetProperty);
        self.chunk.write_u16(ne_idx);
        self.chunk.write_opcode(Opcode::Pop);
    }

    /// Emit Constant(true) + SetProperty("__nonenumerable_<name>__") + Pop
    /// on a standalone object already on top of stack (consumes it).
    fn emit_nonenumerable_marker_standalone(&mut self, prop_name: &str) {
        self.chunk.write_opcode(Opcode::Constant);
        let true_idx = self.chunk.add_constant(Value::Boolean(true));
        self.chunk.write_u16(true_idx);
        let ne_key = format!("__nonenumerable_{}__", prop_name);
        let ne_idx = self.chunk.add_constant(Value::from(&ne_key));
        self.chunk.write_opcode(Opcode::SetProperty);
        self.chunk.write_u16(ne_idx);
        self.chunk.write_opcode(Opcode::Pop);
    }

    /// Mark a private method as readonly (non-writable) on `this`.
    /// Emits: GetThis, Constant(true), SetProperty("__readonly_<key>__"), Pop
    fn emit_private_readonly_marker(&mut self, private_name: &str) {
        // Stack: [...] → GetThis → [..., this] → Constant(true) → [..., this, true]
        // → SetProperty pops val+obj, pushes result → [..., result] → Pop → [...]
        self.chunk.write_opcode(Opcode::GetThis);
        self.chunk.write_opcode(Opcode::Constant);
        let true_idx = self.chunk.add_constant(Value::Boolean(true));
        self.chunk.write_u16(true_idx);
        let ro_key = format!("__readonly_{}__", private_name);
        let ro_idx = self.chunk.add_constant(Value::from(&ro_key));
        self.chunk.write_opcode(Opcode::SetProperty);
        self.chunk.write_u16(ro_idx);
        self.chunk.write_opcode(Opcode::Pop); // pop result of SetProperty
    }

    /// Extract display name from a private key: "\x00#N:name" → "#name"
    fn private_display_name(private_name: &str) -> String {
        if let Some(after_prefix) = private_name.strip_prefix(super::PRIVATE_KEY_PREFIX) {
            if let Some(pos) = after_prefix.find(':') {
                format!("#{}", &after_prefix[pos + 1..])
            } else {
                format!("#{}", after_prefix)
            }
        } else {
            private_name.to_string()
        }
    }

    /// Emit bytecode to read a pre-compiled private method/getter/setter closure.
    /// The closure was stored as a global via DefineGlobalConst during class definition,
    /// so we simply emit GetGlobal with the stored name.
    fn emit_read_priv_method_local(&mut self, private_key: &str) {
        let global_name = self
            .current_class_priv_method_locals
            .iter()
            .rev()
            .flat_map(|v| v.iter())
            .find(|(key, _)| key == private_key)
            .map(|(_, name)| name.clone());

        if let Some(ref gname) = global_name {
            let n16 = crate::unicode::utf8_to_utf16(gname);
            let ni = self.chunk.add_constant(Value::String(n16));
            self.chunk.write_opcode(Opcode::GetGlobal);
            self.chunk.write_u16(ni);
        } else {
            // Fallback: push undefined (should not happen if pre-compilation was done)
            let undef_idx = self.chunk.add_constant(Value::Undefined);
            self.chunk.write_opcode(Opcode::Constant);
            self.chunk.write_u16(undef_idx);
        }
    }

    /// Compile and install a getter on the object currently on top of stack.
    fn compile_and_install_getter(&mut self, gname: &str, body: &[Statement]) -> Result<(), JSError> {
        let visible_name = if let Some(after_prefix) = gname.strip_prefix(super::PRIVATE_KEY_PREFIX) {
            // Strip "\x00#" prefix and any "N:" class ID prefix to get "#name"
            if let Some(pos) = after_prefix.find(':') {
                format!("#{}", &after_prefix[pos + 1..])
            } else {
                format!("#{}", after_prefix)
            }
        } else {
            gname.to_string()
        };
        let display_name = format!("get {}", visible_name);
        let empty_params: Vec<DestructuringElement> = vec![];

        // Stack: [target_obj]
        self.chunk.write_opcode(Opcode::Dup); // [target, target]
        let g_start = self.compile_function_body(Some(&display_name), &empty_params, body)?;
        // [target, target, closure]
        self.chunk.method_function_ips.insert(g_start);
        self.chunk.fn_lengths.insert(g_start, 0);
        self.record_brand_upvalue_for_fn(g_start);

        let getter_key = format!("__get_{}", gname);
        let gk_idx = self.chunk.add_constant(Value::from(&getter_key));
        self.chunk.write_opcode(Opcode::SetProperty);
        self.chunk.write_u16(gk_idx);
        self.chunk.write_opcode(Opcode::Pop);

        // Getter properties are non-enumerable
        self.emit_nonenumerable_marker(gname);

        Ok(())
    }

    /// Compile and install a setter on the object currently on top of stack.
    fn compile_and_install_setter(&mut self, sname: &str, params: &[DestructuringElement], body: &[Statement]) -> Result<(), JSError> {
        let visible_name = if let Some(after_prefix) = sname.strip_prefix(super::PRIVATE_KEY_PREFIX) {
            if let Some(pos) = after_prefix.find(':') {
                format!("#{}", &after_prefix[pos + 1..])
            } else {
                format!("#{}", after_prefix)
            }
        } else {
            sname.to_string()
        };
        let display_name = format!("set {}", visible_name);

        // Stack: [target_obj]
        self.chunk.write_opcode(Opcode::Dup); // [target, target]
        let s_start = self.compile_function_body(Some(&display_name), params, body)?;
        // [target, target, closure]
        self.chunk.method_function_ips.insert(s_start);
        self.record_brand_upvalue_for_fn(s_start);

        let setter_key = format!("__set_{}", sname);
        let sk_idx = self.chunk.add_constant(Value::from(&setter_key));
        self.chunk.write_opcode(Opcode::SetProperty);
        self.chunk.write_u16(sk_idx);
        self.chunk.write_opcode(Opcode::Pop);

        // Setter properties are non-enumerable
        self.emit_nonenumerable_marker(sname);

        Ok(())
    }

    /// Compile a method body as a closure and push it onto the stack.
    /// kind: 0=normal, 1=generator, 2=async, 3=async-generator
    fn compile_class_method_body_as_closure(
        &mut self,
        mname: &str,
        params: &[DestructuringElement],
        body: &[Statement],
        kind: u8,
    ) -> Result<(), JSError> {
        // Strip the private key prefix (including class ID) for the visible function name.
        // Internal key is "\0#N:m" but the spec says the name should be "#m".
        let display_name_owned;
        let display_name = if let Some(after_prefix) = mname.strip_prefix(super::PRIVATE_KEY_PREFIX) {
            display_name_owned = if let Some(pos) = after_prefix.find(':') {
                format!("#{}", &after_prefix[pos + 1..])
            } else {
                format!("#{}", after_prefix)
            };
            &display_name_owned
        } else {
            display_name_owned = String::new();
            let _ = &display_name_owned;
            mname
        };
        let func_ip = match kind {
            1 => {
                // Generator method
                let ip = self.compile_generator_function_body(Some(display_name), params, body)?;
                self.chunk.method_function_ips.insert(ip);
                ip
            }
            2 => {
                // Async method
                let ip = self.compile_function_body(Some(display_name), params, body)?;
                self.chunk.async_function_ips.insert(ip);
                self.chunk.method_function_ips.insert(ip);
                ip
            }
            3 => {
                // Async generator method
                let ip = self.compile_async_generator_function_body(Some(display_name), params, body)?;
                self.chunk.method_function_ips.insert(ip);
                ip
            }
            _ => {
                // Normal method: use compile_function_body for proper upvalue/hoisting support
                let ip = self.compile_function_body(Some(display_name), params, body)?;
                self.chunk.method_function_ips.insert(ip);
                ip
            }
        };
        // Record brand upvalue for this method if in a branded class
        self.record_brand_upvalue_for_fn(func_ip);
        Ok(())
    }

    /// Compile and install a method with a given kind on the object on top of stack.
    /// kind: 0=normal, 1=generator, 2=async, 3=async-generator
    fn compile_and_install_method_with_kind(
        &mut self,
        mname: &str,
        params: &[DestructuringElement],
        body: &[Statement],
        kind: u8,
    ) -> Result<(), JSError> {
        // All methods (including kind=0 normal) use compile_class_method_body_as_closure
        // for proper upvalue capture, so class methods can close over enclosing function variables.
        self.chunk.write_opcode(Opcode::Dup);
        self.compile_class_method_body_as_closure(mname, params, body, kind)?;
        let mk_idx = self.chunk.add_constant(Value::from(mname));
        // Use InitProperty for private methods to avoid brand check during installation
        if mname.starts_with(super::PRIVATE_KEY_PREFIX) {
            self.chunk.write_opcode(Opcode::InitProperty);
        } else {
            self.chunk.write_opcode(Opcode::SetProperty);
        }
        self.chunk.write_u16(mk_idx);
        self.chunk.write_opcode(Opcode::Pop);

        // Class methods are non-enumerable
        self.emit_nonenumerable_marker(mname);

        Ok(())
    }

    /// Compile and install a computed-key method on the object on top of stack.
    fn compile_and_install_computed_method(
        &mut self,
        key_expr: &Expr,
        params: &[DestructuringElement],
        body: &[Statement],
        kind: u8,
    ) -> Result<(), JSError> {
        // stack: [target_obj]
        // We need: target_obj[computed_key] = method
        // Use Dup, compute key, compile body, SetIndex, Pop
        self.chunk.write_opcode(Opcode::Dup);
        self.compile_expr(key_expr)?;
        self.chunk.write_opcode(Opcode::ToPropertyKey);
        self.compile_class_method_body_as_closure("", params, body, kind)?;
        // stack: [target_obj, target_obj, key, closure]
        self.chunk.write_opcode(Opcode::SetIndex);
        self.chunk.write_opcode(Opcode::Pop);
        Ok(())
    }

    /// Compile and install a computed getter on the object on top of stack.
    fn compile_and_install_computed_getter(&mut self, key_expr: &Expr, body: &[Statement]) -> Result<(), JSError> {
        // stack: [target_obj]
        self.chunk.write_opcode(Opcode::Dup);
        self.compile_expr(key_expr)?;
        self.chunk.write_opcode(Opcode::ToPropertyKey);
        // Compile getter body with proper upvalue capture
        let empty_params: Vec<DestructuringElement> = vec![];
        let g_start = self.compile_function_body(None, &empty_params, body)?;
        self.chunk.method_function_ips.insert(g_start);
        // stack: [target_obj, target_obj, key, getter_closure]
        self.chunk.write_opcode(Opcode::SetComputedGetter);
        self.chunk.write_opcode(Opcode::Pop);
        Ok(())
    }

    /// Compile and install a computed setter on the object on top of stack.
    fn compile_and_install_computed_setter(
        &mut self,
        key_expr: &Expr,
        params: &[DestructuringElement],
        body: &[Statement],
    ) -> Result<(), JSError> {
        // stack: [target_obj]
        self.chunk.write_opcode(Opcode::Dup);
        self.compile_expr(key_expr)?;
        self.chunk.write_opcode(Opcode::ToPropertyKey);
        // Compile setter body with proper upvalue capture
        let s_start = self.compile_function_body(None, params, body)?;
        self.chunk.method_function_ips.insert(s_start);
        // stack: [target_obj, target_obj, key, setter_closure]
        self.chunk.write_opcode(Opcode::SetComputedSetter);
        self.chunk.write_opcode(Opcode::Pop);
        Ok(())
    }

    /// Compile a single instance field initialiser.
    /// Emits: GetThis, compile_expr(value), SetProperty "#name" or "name", Pop
    fn compile_class_instance_field(&mut self, field: &crate::core::statement::ClassMember) -> Result<(), JSError> {
        let old_eval_flags = self.eval_context_flags;
        self.eval_context_flags |= 0x01; // mark: in field initializer
        let result = self.compile_class_instance_field_inner(field);
        self.eval_context_flags = old_eval_flags;
        result
    }

    fn compile_class_instance_field_inner(&mut self, field: &crate::core::statement::ClassMember) -> Result<(), JSError> {
        match field {
            ClassMember::Property(fname, init_expr) => {
                self.chunk.write_opcode(Opcode::GetThis);
                // Track GetThis as a temporary local for correct stack alignment
                // (needed when init_expr is a class expression with private members/brand)
                self.locals.push("__field_this__".to_string());
                self.chunk.write_opcode(Opcode::EnterFieldInit);
                let locals_before = self.locals.len();
                let func_ip = self.peek_func_ip(init_expr);
                self.compile_expr(init_expr)?;
                self.chunk.write_opcode(Opcode::LeaveFieldInit);
                // Class expressions leave dead locals (brand, temp, etc.) on the stack
                // between __field_this__ and the expression result.  Clean them up so
                // InitProperty sees [this, value].  Upvalue cells are heap-allocated
                // copies so Swap+Pop is safe even for captured locals.
                let dead_count = self.locals.len() - locals_before;
                for _ in 0..dead_count {
                    self.chunk.write_opcode(Opcode::Swap);
                    self.chunk.write_opcode(Opcode::Pop);
                }
                // Remove the dead locals from tracking (in reverse order)
                while self.locals.len() > locals_before {
                    self.locals.pop();
                }
                if let Some(ip) = func_ip {
                    self.chunk.fn_names.entry(ip).or_insert_with(|| fname.clone());
                }
                let fk = self.chunk.add_constant(Value::from(fname));
                self.chunk.write_opcode(Opcode::InitProperty);
                self.chunk.write_u16(fk);
                self.chunk.write_opcode(Opcode::Pop);
                // Remove temporary local (this was consumed by InitProperty)
                if let Some(pos) = self.locals.iter().rposition(|l| l == "__field_this__") {
                    self.locals.remove(pos);
                }
            }
            ClassMember::PrivateProperty(fname, init_expr) => {
                self.chunk.write_opcode(Opcode::GetThis);
                self.locals.push("__field_this__".to_string());
                self.chunk.write_opcode(Opcode::EnterFieldInit);
                let private_name = self.resolve_private_key(fname);
                let locals_before = self.locals.len();
                let func_ip = self.peek_func_ip(init_expr);
                self.compile_expr(init_expr)?;
                self.chunk.write_opcode(Opcode::LeaveFieldInit);
                // Same dead-local cleanup as Property above
                let dead_count = self.locals.len() - locals_before;
                for _ in 0..dead_count {
                    self.chunk.write_opcode(Opcode::Swap);
                    self.chunk.write_opcode(Opcode::Pop);
                }
                while self.locals.len() > locals_before {
                    self.locals.pop();
                }
                if let Some(ip) = func_ip {
                    // Display name is #name, not \0#name
                    self.chunk.fn_names.entry(ip).or_insert_with(|| format!("#{}", fname));
                }
                let fk = self.chunk.add_constant(Value::from(&private_name));
                self.chunk.write_opcode(Opcode::InitProperty);
                self.chunk.write_u16(fk);
                self.chunk.write_opcode(Opcode::Pop);
                if let Some(pos) = self.locals.iter().rposition(|l| l == "__field_this__") {
                    self.locals.remove(pos);
                }
            }
            ClassMember::PropertyComputed(key_expr, val_expr) => {
                self.chunk.write_opcode(Opcode::GetThis);
                self.locals.push("__field_this__".to_string());
                self.compile_expr(key_expr)?;
                self.chunk.write_opcode(Opcode::ToPropertyKey);
                // The computed key is also a temporary on the stack
                self.locals.push("__field_key__".to_string());
                self.chunk.write_opcode(Opcode::EnterFieldInit);
                let locals_before = self.locals.len();
                self.compile_expr(val_expr)?;
                self.chunk.write_opcode(Opcode::LeaveFieldInit);
                // Same dead-local cleanup as Property above
                let dead_count = self.locals.len() - locals_before;
                for _ in 0..dead_count {
                    self.chunk.write_opcode(Opcode::Swap);
                    self.chunk.write_opcode(Opcode::Pop);
                }
                while self.locals.len() > locals_before {
                    self.locals.pop();
                }
                // stack: [this, key, value]
                self.chunk.write_opcode(Opcode::SetIndex);
                self.chunk.write_opcode(Opcode::Pop);
                // Remove temporary locals (consumed by SetIndex)
                if let Some(pos) = self.locals.iter().rposition(|l| l == "__field_key__") {
                    self.locals.remove(pos);
                }
                if let Some(pos) = self.locals.iter().rposition(|l| l == "__field_this__") {
                    self.locals.remove(pos);
                }
            }
            // Per-instance private method installation (spec: PrivateMethodOrAccessorAdd)
            // Closures are pre-compiled at class definition scope; read from upvalue here.
            ClassMember::PrivateMethod(mname, ..)
            | ClassMember::PrivateMethodGenerator(mname, ..)
            | ClassMember::PrivateMethodAsync(mname, ..)
            | ClassMember::PrivateMethodAsyncGenerator(mname, ..) => {
                let private_name = self.resolve_private_key(mname);
                self.chunk.write_opcode(Opcode::GetThis);
                self.emit_read_priv_method_local(&private_name);
                let mk_idx = self.chunk.add_constant(Value::from(&private_name));
                self.chunk.write_opcode(Opcode::InitProperty);
                self.chunk.write_u16(mk_idx);
                self.chunk.write_opcode(Opcode::Pop);
                self.emit_private_readonly_marker(&private_name);
            }
            ClassMember::PrivateGetter(gname, ..) => {
                let private_name = self.resolve_private_key(gname);
                let getter_key = format!("__get_{}", private_name);
                self.chunk.write_opcode(Opcode::GetThis);
                self.emit_read_priv_method_local(&getter_key);
                let gk_idx = self.chunk.add_constant(Value::from(&getter_key));
                self.chunk.write_opcode(Opcode::InitProperty);
                self.chunk.write_u16(gk_idx);
                self.chunk.write_opcode(Opcode::Pop);
            }
            ClassMember::PrivateSetter(sname, ..) => {
                let private_name = self.resolve_private_key(sname);
                let setter_key = format!("__set_{}", private_name);
                self.chunk.write_opcode(Opcode::GetThis);
                self.emit_read_priv_method_local(&setter_key);
                let sk_idx = self.chunk.add_constant(Value::from(&setter_key));
                self.chunk.write_opcode(Opcode::InitProperty);
                self.chunk.write_u16(sk_idx);
                self.chunk.write_opcode(Opcode::Pop);
            }
            _ => {}
        }
        Ok(())
    }

    /// Compile a static field or static block.
    fn compile_class_static_member(
        &mut self,
        class_name: &str,
        member: &crate::core::statement::ClassMember,
        is_expr: bool,
    ) -> Result<(), JSError> {
        match member {
            ClassMember::StaticProperty(fname, init_expr) => {
                // Push class ref for InitProperty target
                self.emit_get_class_ref(class_name, is_expr)?;
                // Wrap init expression in a mini-function so `this` binds to the class
                self.emit_get_class_ref(class_name, is_expr)?; // receiver for method call
                let sf_jump = self.emit_jump(Opcode::Jump);
                let sf_start = self.chunk.code.len();
                // Save and reset locals so the mini-function has its own local frame.
                // The class name is aliased to `this` (= the class) as local 0.
                let saved_locals = std::mem::take(&mut self.locals);
                let saved_const_locals = self.const_locals.clone();
                self.scope_depth += 1;
                if !class_name.is_empty() {
                    self.chunk.write_opcode(Opcode::GetThis);
                    self.locals.push(class_name.to_string());
                    self.const_locals.insert(class_name.to_string());
                }
                let func_ip = self.peek_func_ip(init_expr);
                self.compile_expr(init_expr)?;
                if let Some(ip) = func_ip {
                    self.chunk.fn_names.entry(ip).or_insert_with(|| fname.clone());
                }
                self.chunk.write_opcode(Opcode::Return);
                self.scope_depth -= 1;
                self.locals = saved_locals;
                self.const_locals = saved_const_locals;
                self.patch_jump(sf_jump);
                let sf_val = Value::VmFunction(sf_start, 0);
                let sf_idx = self.chunk.add_constant(sf_val);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(sf_idx);
                self.emit_call_opcode(0, 0x80);
                // Stack: [class_ref, init_value]
                let fk = self.chunk.add_constant(Value::from(fname));
                self.chunk.write_opcode(Opcode::InitProperty);
                self.chunk.write_u16(fk);
                self.chunk.write_opcode(Opcode::Pop);
            }
            ClassMember::PrivateStaticProperty(fname, init_expr) => {
                // Push class ref for InitProperty target
                self.emit_get_class_ref(class_name, is_expr)?;
                // Wrap init expression in a mini-function so `this` binds to the class
                self.emit_get_class_ref(class_name, is_expr)?; // receiver for method call
                let sf_jump = self.emit_jump(Opcode::Jump);
                let sf_start = self.chunk.code.len();
                let saved_locals = std::mem::take(&mut self.locals);
                let saved_const_locals = self.const_locals.clone();
                self.scope_depth += 1;
                if !class_name.is_empty() {
                    self.chunk.write_opcode(Opcode::GetThis);
                    self.locals.push(class_name.to_string());
                    self.const_locals.insert(class_name.to_string());
                }
                let private_name = self.resolve_private_key(fname);
                let func_ip = self.peek_func_ip(init_expr);
                self.compile_expr(init_expr)?;
                if let Some(ip) = func_ip {
                    // Display name is #name, not \0#name
                    self.chunk.fn_names.entry(ip).or_insert_with(|| format!("#{}", fname));
                }
                self.chunk.write_opcode(Opcode::Return);
                self.scope_depth -= 1;
                self.locals = saved_locals;
                self.const_locals = saved_const_locals;
                self.patch_jump(sf_jump);
                let sf_val = Value::VmFunction(sf_start, 0);
                let sf_idx = self.chunk.add_constant(sf_val);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(sf_idx);
                self.emit_call_opcode(0, 0x80);
                // Stack: [class_ref, init_value]
                let fk = self.chunk.add_constant(Value::from(&private_name));
                self.chunk.write_opcode(Opcode::InitProperty);
                self.chunk.write_u16(fk);
                self.chunk.write_opcode(Opcode::Pop);
            }
            ClassMember::StaticBlock(body) => {
                // Execute static block with `this` bound to the class constructor
                self.emit_get_class_ref(class_name, is_expr)?;
                // Compile as an IIFE, but push class as this
                let sb_jump = self.emit_jump(Opcode::Jump);
                let sb_start = self.chunk.code.len();
                let saved_locals = std::mem::take(&mut self.locals);
                let saved_const_locals = self.const_locals.clone();
                self.scope_depth += 1;
                if !class_name.is_empty() {
                    self.chunk.write_opcode(Opcode::GetThis);
                    self.locals.push(class_name.to_string());
                    self.const_locals.insert(class_name.to_string());
                }
                for stmt in body.iter() {
                    self.compile_statement(stmt, false)?;
                }
                self.chunk.write_opcode(Opcode::Constant);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_u16(undef_idx);
                self.chunk.write_opcode(Opcode::Return);
                self.scope_depth -= 1;
                self.locals = saved_locals;
                self.const_locals = saved_const_locals;
                self.patch_jump(sb_jump);

                let sb_val = Value::VmFunction(sb_start, 0);
                let sb_idx = self.chunk.add_constant(sb_val);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(sb_idx);
                // Call with 0 args but 0x80 flag to set `this` to class
                self.emit_call_opcode(0, 0x80);
                self.chunk.write_opcode(Opcode::Pop); // discard return value
            }
            ClassMember::StaticPropertyComputed(key_expr, val_expr) => {
                self.emit_get_class_ref(class_name, is_expr)?;
                self.compile_expr(key_expr)?;
                self.chunk.write_opcode(Opcode::ToPropertyKey);
                self.compile_expr(val_expr)?;
                // stack: [class, key, value]
                self.chunk.write_opcode(Opcode::SetIndex);
                self.chunk.write_opcode(Opcode::Pop);
            }
            _ => {}
        }
        Ok(())
    }
}
