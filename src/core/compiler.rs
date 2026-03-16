use crate::core::opcode::{Chunk, Opcode};
use crate::core::statement::{
    BinaryOp, CatchParamPattern, ClassMember, DestructuringElement, Expr, ObjectDestructuringElement, Statement, StatementKind,
};
use crate::core::{JSError, Value};
use crate::raise_syntax_error;

pub struct Compiler<'gc> {
    chunk: Chunk<'gc>,
    locals: Vec<String>,
    scope_depth: i32,     // 0 = top-level (global), > 0 = inside function
    current_strict: bool, // whether surrounding context is strict mode
    loop_stack: Vec<LoopContext>,
    pending_label: Option<String>,        // label to attach to the next loop
    forin_counter: u32,                   // unique ID for for-in synthetic variables
    current_class_parent: Option<String>, // parent class name for super resolution
}

#[derive(Debug, Clone, Default)]
struct LoopContext {
    #[allow(dead_code)]
    loop_start: usize, // IP to jump back to (top of loop)
    label: Option<String>,        // optional label for labeled break/continue
    continue_patches: Vec<usize>, // offsets to patch with continue target
    break_patches: Vec<usize>,    // offsets to patch with post-loop address
}

impl<'gc> Compiler<'gc> {
    pub fn new() -> Self {
        Self {
            chunk: Chunk::new(),
            locals: Vec::new(),
            scope_depth: 0,
            current_strict: false,
            loop_stack: Vec::new(),
            pending_label: None,
            forin_counter: 0,
            current_class_parent: None,
        }
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
        // Hoist function declarations to the top
        for stmt in statements.iter() {
            if matches!(*stmt.kind, StatementKind::FunctionDeclaration(..)) {
                self.compile_statement(stmt, false)?;
            }
        }
        for (i, stmt) in statements.iter().enumerate() {
            if matches!(*stmt.kind, StatementKind::FunctionDeclaration(..)) {
                continue;
            }
            let is_last = i == statements.len() - 1;
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
        is_strict
    }

    /// Create a LoopContext, consuming any pending label
    fn make_loop_context(&mut self, loop_start: usize) -> LoopContext {
        LoopContext {
            loop_start,
            label: self.pending_label.take(),
            ..LoopContext::default()
        }
    }

    fn compile_statement(&mut self, stmt: &Statement, is_last: bool) -> Result<(), JSError> {
        match &*stmt.kind {
            StatementKind::Expr(expr) => {
                // Detect 'use strict' directive at top-level or inside function
                if let Expr::StringLit(s) = expr
                    && crate::unicode::utf16_to_utf8(s) == "use strict"
                    && self.scope_depth == 0
                {
                    self.current_strict = true;
                }
                self.compile_expr(expr)?;
                // Pop if it's not the last evaluated statement, to keep stack clean
                if !is_last {
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
                    } else {
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
            StatementKind::Assign(name, expr) => {
                self.compile_expr(expr)?;
                if let Some(pos) = self.locals.iter().rposition(|l| l == name) {
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let name_u16 = crate::unicode::utf8_to_utf16(name);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::SetGlobal);
                    self.chunk.write_u16(name_idx);
                }
                if !is_last {
                    self.chunk.write_opcode(Opcode::Pop);
                }
            }
            StatementKind::Block(statements) => {
                let saved_locals = self.locals.len();
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
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::While(cond, body) => {
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
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::For(for_stmt) => {
                // Compile init
                if let Some(init) = &for_stmt.init {
                    self.compile_statement(init, false)?;
                }
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
                // Update — continue jumps here (not to condition)
                let update_ip = self.chunk.code.len();
                for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                    self.patch_jump_to(*cp, update_ip);
                }
                if let Some(update) = &for_stmt.update {
                    self.compile_statement(update, false)?;
                }
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
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::Return(expr_opt) => {
                if let Some(expr) = expr_opt {
                    self.compile_expr(expr)?;
                } else {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
                self.chunk.write_opcode(Opcode::Return);
            }
            StatementKind::Throw(expr) => {
                self.compile_expr(expr)?;
                self.chunk.write_opcode(Opcode::Throw);
            }
            StatementKind::Break(label_opt) => {
                let patch = self.emit_jump(Opcode::Jump);
                if let Some(label) = label_opt {
                    // Labeled break: find the matching loop context
                    if let Some(ctx) = self.loop_stack.iter_mut().rev().find(|c| c.label.as_deref() == Some(label)) {
                        ctx.break_patches.push(patch);
                    } else {
                        return Err(crate::raise_syntax_error!(format!("label '{}' not found for break", label)));
                    }
                } else if let Some(ctx) = self.loop_stack.last_mut() {
                    ctx.break_patches.push(patch);
                } else {
                    return Err(crate::raise_syntax_error!("break outside of loop"));
                }
            }
            StatementKind::Continue(label_opt) => {
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
                    return Err(crate::raise_syntax_error!("continue outside of loop"));
                }
            }
            StatementKind::Label(label, inner) => {
                // Set pending label so the next loop picks it up
                self.pending_label = Some(label.clone());
                self.compile_statement(inner, is_last)?;
                // Clear in case it wasn't consumed (non-loop label)
                self.pending_label = None;
            }
            StatementKind::ForIn(_decl_kind, var_name, obj_expr, body) => {
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
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::TryCatch(tc) => {
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

                // Finally body (block-scoped)
                let saved_finally = self.locals.len();
                if let Some(ref finally_body) = tc.finally_body {
                    for s in finally_body {
                        self.compile_statement(s, false)?;
                    }
                }
                self.end_block_scope(saved_finally);

                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::FunctionDeclaration(name, params, body, _is_gen, _is_async) => {
                // Jump over the function body in the main bytecode stream
                let jump_over = self.emit_jump(Opcode::Jump);
                let func_ip = self.chunk.code.len();
                let fn_is_strict = self.record_fn_strictness(func_ip, body, false);

                // Save and reset locals/scope for function scope
                let old_locals = std::mem::take(&mut self.locals);
                let old_depth = self.scope_depth;
                let old_loops = std::mem::take(&mut self.loop_stack);
                let old_strict = self.current_strict;
                self.current_strict = fn_is_strict;
                self.scope_depth = 1;
                for param in params {
                    if let DestructuringElement::Variable(param_name, _) = param {
                        self.locals.push(param_name.clone());
                    }
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
                self.locals = old_locals;
                self.scope_depth = old_depth;
                self.loop_stack = old_loops;
                self.current_strict = old_strict;

                // Push the VmFunction value and define it as a global
                let func_val = Value::VmFunction(func_ip, params.len() as u8);
                let func_idx = self.chunk.add_constant(func_val);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(func_idx);

                // Register function name for .name property
                self.chunk.fn_names.insert(func_ip, name.clone());

                let name_u16 = crate::unicode::utf8_to_utf16(name);
                let name_idx = self.chunk.add_constant(Value::String(name_u16));
                self.chunk.write_opcode(Opcode::DefineGlobal);
                self.chunk.write_u16(name_idx);
            }
            StatementKind::ForOf(_decl_kind, var_name, iterable_expr, body) => {
                // Desugar: arr = iterable; for (idx=0; idx<arr.length; idx++) { var_name = arr[idx]; body }
                self.compile_expr(iterable_expr)?;
                // Store iterable as __forofArr__
                if self.scope_depth > 0 {
                    self.locals.push("__forofArr__".to_string());
                } else {
                    let n = crate::unicode::utf8_to_utf16("__forofArr__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(ni);
                }
                // idx = 0
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

                // Pre-allocate loop variable slot if it's a new local
                if self.scope_depth > 0 && !self.locals.iter().any(|l| l == var_name) {
                    let undef = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(undef);
                    self.locals.push(var_name.clone());
                }

                // Loop start: test idx < arr.length
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

                // var_name = arr[idx]
                self.emit_helper_get("__forofArr__");
                self.emit_helper_get("__forofIdx__");
                self.chunk.write_opcode(Opcode::GetIndex);
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
                for s in body {
                    self.compile_statement(s, false)?;
                }

                // continue target: idx++ update
                let update_ip = self.chunk.code.len();
                for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                    self.patch_jump_to(*cp, update_ip);
                }

                // idx++
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

                // Clean up synthetic locals
                if self.scope_depth > 0 {
                    self.locals.retain(|l| l != "__forofArr__" && l != "__forofIdx__");
                }

                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::ForOfDestructuringArray(_decl_kind, elements, iterable_expr, body) => {
                self.compile_for_of_destructuring_array(elements, iterable_expr, body)?;
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::ForOfDestructuringObject(_decl_kind, elements, iterable_expr, body) => {
                self.compile_for_of_destructuring_object(elements, iterable_expr, body)?;
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::ForInDestructuringArray(_decl_kind, elements, obj_expr, body) => {
                self.compile_for_in_destructuring_array(elements, obj_expr, body)?;
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::ForInDestructuringObject(_decl_kind, elements, obj_expr, body) => {
                self.compile_for_in_destructuring_object(elements, obj_expr, body)?;
                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::Switch(sw) => {
                // Compile discriminant once, store in synthetic local/global
                self.compile_expr(&sw.expr)?;
                if self.scope_depth > 0 {
                    self.locals.push("__switch__".to_string());
                } else {
                    let n = crate::unicode::utf8_to_utf16("__switch__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(ni);
                }

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
                            self.emit_helper_get("__switch__");
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

                // Clean up synthetic local
                if self.scope_depth > 0 {
                    self.locals.retain(|l| l != "__switch__");
                }

                if is_last {
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                }
            }
            StatementKind::Class(class_def) => {
                let name = &class_def.name;
                let parent_name = if let Some(Expr::Var(pname, ..)) = class_def.extends.as_ref() {
                    Some(pname.clone())
                } else {
                    None
                };

                // Save/set class parent context for super resolution
                let prev_parent = self.current_class_parent.take();
                self.current_class_parent = parent_name.clone();

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

                let arity = ctor_params.len() as u8;

                // Emit jump over constructor body
                let jump_over = self.emit_jump(Opcode::Jump);
                let fn_start = self.chunk.code.len();
                let ctor_is_strict = self.record_fn_strictness(fn_start, &ctor_body, true);

                let old_strict = self.current_strict;
                self.current_strict = ctor_is_strict;
                self.scope_depth += 1;
                for p in &ctor_params {
                    match p {
                        DestructuringElement::Variable(pname, _) => self.locals.push(pname.clone()),
                        DestructuringElement::Rest(pname) => self.locals.push(pname.clone()),
                        _ => {}
                    }
                }

                for stmt in ctor_body.iter() {
                    self.compile_statement(stmt, false)?;
                }
                self.chunk.write_opcode(Opcode::GetThis);
                self.chunk.write_opcode(Opcode::Return);

                let locals_to_remove = ctor_params.len();
                for _ in 0..locals_to_remove {
                    self.locals.pop();
                }
                self.scope_depth -= 1;
                self.current_strict = old_strict;

                self.patch_jump(jump_over);
                // Register constructor name
                self.chunk.fn_names.insert(fn_start, name.clone());

                // Define constructor as global
                let fn_val = Value::VmFunction(fn_start, arity);
                let fn_idx = self.chunk.add_constant(fn_val);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(fn_idx);

                let name_u16 = crate::unicode::utf8_to_utf16(name);
                let name_idx = self.chunk.add_constant(Value::String(name_u16));
                self.chunk.write_opcode(Opcode::DefineGlobal);
                self.chunk.write_u16(name_idx);

                // Collect methods to install on prototype, static methods on constructor
                let mut methods: Vec<(&str, &Vec<DestructuringElement>, &Vec<Statement>, bool)> = Vec::new();
                let mut getters: Vec<(&str, &Vec<Statement>, bool)> = Vec::new();
                let mut setters: Vec<(&str, &Vec<DestructuringElement>, &Vec<Statement>, bool)> = Vec::new();
                for member in &class_def.members {
                    match member {
                        ClassMember::Method(mname, params, body) => methods.push((mname, params, body, false)),
                        ClassMember::StaticMethod(mname, params, body) => methods.push((mname, params, body, true)),
                        ClassMember::Getter(gname, body) => getters.push((gname, body, false)),
                        ClassMember::StaticGetter(gname, body) => getters.push((gname, body, true)),
                        ClassMember::Setter(sname, params, body) => setters.push((sname, params, body, false)),
                        ClassMember::StaticSetter(sname, params, body) => setters.push((sname, params, body, true)),
                        _ => {}
                    }
                }

                // Compile and install instance methods on ClassName.prototype
                if !methods.iter().any(|(_, _, _, is_static)| !is_static)
                    && !getters.iter().any(|(_, _, is_static)| !is_static)
                    && !setters.iter().any(|(_, _, _, is_static)| !is_static)
                {
                    // No instance members to install
                } else {
                    // Push prototype: GetGlobal ClassName, GetProperty "prototype"
                    let cls_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(name)));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(cls_idx);
                    let proto_key = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16("prototype")));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(proto_key);
                    // stack: [proto]

                    for &(mname, params, body, is_static) in &methods {
                        if is_static {
                            continue;
                        }
                        // Compile method as function
                        let m_jump = self.emit_jump(Opcode::Jump);
                        let m_start = self.chunk.code.len();
                        let method_is_strict = self.record_fn_strictness(m_start, body, true);
                        let m_arity = params.len() as u8;
                        let old_strict = self.current_strict;
                        self.current_strict = method_is_strict;
                        self.scope_depth += 1;
                        for p in params {
                            match p {
                                DestructuringElement::Variable(pn, _) => self.locals.push(pn.clone()),
                                DestructuringElement::Rest(pn) => self.locals.push(pn.clone()),
                                _ => {}
                            }
                        }
                        for stmt in body.iter() {
                            self.compile_statement(stmt, false)?;
                        }
                        self.chunk.write_opcode(Opcode::Constant);
                        let undef_idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_u16(undef_idx);
                        self.chunk.write_opcode(Opcode::Return);
                        for _ in 0..params.len() {
                            self.locals.pop();
                        }
                        self.scope_depth -= 1;
                        self.current_strict = old_strict;
                        self.patch_jump(m_jump);
                        self.chunk.fn_names.insert(m_start, mname.to_string());

                        // Install: Dup proto, push method, SetProperty, Pop
                        self.chunk.write_opcode(Opcode::Dup);
                        let m_val = Value::VmFunction(m_start, m_arity);
                        let m_idx = self.chunk.add_constant(m_val);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(m_idx);
                        let mk_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(mname)));
                        self.chunk.write_opcode(Opcode::SetProperty);
                        self.chunk.write_u16(mk_idx);
                        self.chunk.write_opcode(Opcode::Pop);
                    }

                    // Install getters on prototype
                    for &(gname, body, is_static) in &getters {
                        if is_static {
                            continue;
                        }
                        let g_jump = self.emit_jump(Opcode::Jump);
                        let g_start = self.chunk.code.len();
                        self.scope_depth += 1;
                        for stmt in body.iter() {
                            self.compile_statement(stmt, false)?;
                        }
                        self.chunk.write_opcode(Opcode::Constant);
                        let undef_idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_u16(undef_idx);
                        self.chunk.write_opcode(Opcode::Return);
                        self.scope_depth -= 1;
                        self.patch_jump(g_jump);

                        self.chunk.write_opcode(Opcode::Dup);
                        let g_val = Value::VmFunction(g_start, 0);
                        let g_idx = self.chunk.add_constant(g_val);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(g_idx);
                        let getter_key = format!("__get_{}", gname);
                        let gk_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(&getter_key)));
                        self.chunk.write_opcode(Opcode::SetProperty);
                        self.chunk.write_u16(gk_idx);
                        self.chunk.write_opcode(Opcode::Pop);
                    }

                    // Install setters on prototype
                    for &(sname, params, body, is_static) in &setters {
                        if is_static {
                            continue;
                        }
                        let s_jump = self.emit_jump(Opcode::Jump);
                        let s_start = self.chunk.code.len();
                        let s_arity = params.len() as u8;
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
                        for _ in 0..params.len() {
                            self.locals.pop();
                        }
                        self.scope_depth -= 1;
                        self.patch_jump(s_jump);

                        self.chunk.write_opcode(Opcode::Dup);
                        let s_val = Value::VmFunction(s_start, s_arity);
                        let s_idx = self.chunk.add_constant(s_val);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(s_idx);
                        let setter_key = format!("__set_{}", sname);
                        let sk_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(&setter_key)));
                        self.chunk.write_opcode(Opcode::SetProperty);
                        self.chunk.write_u16(sk_idx);
                        self.chunk.write_opcode(Opcode::Pop);
                    }

                    self.chunk.write_opcode(Opcode::Pop); // pop proto
                }

                // Install static methods on the constructor function itself
                for &(mname, params, body, is_static) in &methods {
                    if !is_static {
                        continue;
                    }
                    let m_jump = self.emit_jump(Opcode::Jump);
                    let m_start = self.chunk.code.len();
                    let m_arity = params.len() as u8;
                    self.scope_depth += 1;
                    for p in params {
                        match p {
                            DestructuringElement::Variable(pn, _) => self.locals.push(pn.clone()),
                            DestructuringElement::Rest(pn) => self.locals.push(pn.clone()),
                            _ => {}
                        }
                    }
                    for stmt in body.iter() {
                        self.compile_statement(stmt, false)?;
                    }
                    self.chunk.write_opcode(Opcode::Constant);
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_u16(undef_idx);
                    self.chunk.write_opcode(Opcode::Return);
                    for _ in 0..params.len() {
                        self.locals.pop();
                    }
                    self.scope_depth -= 1;
                    self.patch_jump(m_jump);

                    // GetGlobal ClassName, push method, SetProperty
                    let cls_idx2 = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(name)));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(cls_idx2);
                    let m_val = Value::VmFunction(m_start, m_arity);
                    let m_idx = self.chunk.add_constant(m_val);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(m_idx);
                    let mk_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(mname)));
                    self.chunk.write_opcode(Opcode::SetProperty);
                    self.chunk.write_u16(mk_idx);
                    self.chunk.write_opcode(Opcode::Pop);
                }

                // Handle extends: set Child.prototype.__proto__ = Parent.prototype
                if let Some(ref pname) = parent_name {
                    // GetGlobal ClassName, GetProperty "prototype" → child proto
                    let cls_idx3 = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(name)));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(cls_idx3);
                    let proto_k = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16("prototype")));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(proto_k);
                    // GetGlobal ParentClass, GetProperty "prototype" → parent proto
                    let par_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(pname)));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(par_idx);
                    let proto_k2 = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16("prototype")));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(proto_k2);
                    // SetProperty "__proto__" on child prototype
                    let dunder_proto = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16("__proto__")));
                    self.chunk.write_opcode(Opcode::SetProperty);
                    self.chunk.write_u16(dunder_proto);
                    self.chunk.write_opcode(Opcode::Pop);
                }

                // Restore previous class context
                self.current_class_parent = prev_parent;
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
            Expr::Undefined => {
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
            }
            Expr::Var(name, ..) => {
                if name == "arguments" && self.scope_depth > 0 {
                    // inside a function, treat `arguments` as the special arguments object
                    self.chunk.write_opcode(Opcode::GetArguments);
                } else if let Some(pos) = self.locals.iter().rposition(|l| l == name) {
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
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
                    BinaryOp::StrictEqual => self.chunk.write_opcode(Opcode::Equal),
                    BinaryOp::NotEqual => self.chunk.write_opcode(Opcode::NotEqual),
                    BinaryOp::StrictNotEqual => self.chunk.write_opcode(Opcode::StrictNotEqual),
                    BinaryOp::In => self.chunk.write_opcode(Opcode::In),
                    BinaryOp::InstanceOf => self.chunk.write_opcode(Opcode::InstanceOf),
                    _ => {
                        return Err(crate::raise_syntax_error!(format!("Unimplemented binary operator for VM: {op:?}")));
                    }
                }
            }
            Expr::Call(callee, args) => {
                if let Expr::Property(obj, method_name) = &**callee {
                    // Method call: obj.method(args)
                    // Stack layout: [..., obj, method_fn, arg0, arg1, ...]
                    // GetMethod peeks at obj and pushes method on top
                    self.compile_expr(obj)?;
                    let key_u16 = crate::unicode::utf8_to_utf16(method_name);
                    let name_idx = self.chunk.add_constant(Value::String(key_u16));
                    self.chunk.write_opcode(Opcode::GetMethod);
                    self.chunk.write_u16(name_idx);
                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    self.chunk.write_opcode(Opcode::Call);
                    // arg_count + 1 signals method call (the +128 flag)
                    self.chunk.write_byte(args.len() as u8 | 0x80);
                } else {
                    // Regular function call
                    self.compile_expr(callee)?;
                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    self.chunk.write_opcode(Opcode::Call);
                    self.chunk.write_byte(args.len() as u8);
                }
            }
            Expr::This => {
                self.chunk.write_opcode(Opcode::GetThis);
            }
            Expr::SuperCall(args) => {
                // super(args) → call parent constructor with current this
                if let Some(ref pname) = self.current_class_parent {
                    // Stack: [this (receiver), ParentCtor (callee), args...]
                    self.chunk.write_opcode(Opcode::GetThis);
                    let par_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(pname)));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(par_idx);
                    // Handle spread in super(...args)
                    let mut real_count = 0u8;
                    let mut has_spread = false;
                    for arg in args {
                        if let Expr::Spread(inner) = arg {
                            has_spread = true;
                            self.compile_expr(inner)?;
                        } else {
                            self.compile_expr(arg)?;
                            real_count += 1;
                        }
                    }
                    if has_spread {
                        // For default ctor with spread args, we pop the array and push individual elements
                        // at runtime. For now, just pass the rest array directly — the parent ctor will
                        // receive it as a single arg. This is a simplification; proper spread needs
                        // runtime unrolling. We pass 1 arg (the rest array).
                        self.chunk.write_opcode(Opcode::Call);
                        self.chunk.write_byte(1u8 | 0x80);
                    } else {
                        self.chunk.write_opcode(Opcode::Call);
                        self.chunk.write_byte(real_count | 0x80);
                    }
                } else {
                    self.chunk.write_opcode(Opcode::Constant);
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_u16(undef_idx);
                }
            }
            Expr::SuperMethod(method_name, args) => {
                // super.method(args) → get method from parent prototype, call with current this
                if let Some(ref pname) = self.current_class_parent {
                    // Stack after: [this (receiver), method (callee), args...]
                    self.chunk.write_opcode(Opcode::GetThis);
                    let par_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(pname)));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(par_idx);
                    let proto_k = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16("prototype")));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(proto_k);
                    let mk = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(method_name)));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(mk);
                    for arg in args {
                        self.compile_expr(arg)?;
                    }
                    self.chunk.write_opcode(Opcode::Call);
                    self.chunk.write_byte(args.len() as u8 | 0x80);
                } else {
                    self.chunk.write_opcode(Opcode::Constant);
                    let undef_idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_u16(undef_idx);
                }
            }
            Expr::SuperProperty(prop_name) => {
                let pk = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(prop_name)));
                self.chunk.write_opcode(Opcode::GetSuperProperty);
                self.chunk.write_u16(pk);
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
            Expr::UnaryPlus(inner) => {
                // +x is just coerce to number, for now just compile inner
                self.compile_expr(inner)?;
            }
            Expr::TypeOf(inner) => {
                // typeof on an undeclared variable must return "undefined", not throw
                if let Expr::Var(name, ..) = &**inner {
                    if name != "arguments" || self.scope_depth == 0 {
                        if self.locals.iter().rposition(|l| l == name).is_none() {
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
            // Array literal: [a, b, c]
            Expr::Array(elements) => {
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
            // Object literal: { key: val, ... }
            Expr::Object(props) => {
                let mut count = 0u8;
                for (key, val, _computed, _shorthand) in props {
                    // For getters/setters, prefix the key so the VM can detect them
                    match val {
                        Expr::Getter(_) => {
                            // Emit "__get_<key>" as the property name
                            let key_str = match key {
                                Expr::StringLit(s) => crate::unicode::utf16_to_utf8(s),
                                _ => {
                                    self.compile_expr(key)?;
                                    self.compile_expr(val)?;
                                    count += 1;
                                    continue;
                                }
                            };
                            let prefixed = format!("__get_{}", key_str);
                            let idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(&prefixed)));
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(idx);
                            self.compile_expr(val)?;
                            count += 1;
                        }
                        Expr::Setter(_) => {
                            let key_str = match key {
                                Expr::StringLit(s) => crate::unicode::utf16_to_utf8(s),
                                _ => {
                                    self.compile_expr(key)?;
                                    self.compile_expr(val)?;
                                    count += 1;
                                    continue;
                                }
                            };
                            let prefixed = format!("__set_{}", key_str);
                            let idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(&prefixed)));
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(idx);
                            self.compile_expr(val)?;
                            count += 1;
                        }
                        _ => {
                            self.compile_expr(key)?;
                            self.compile_expr(val)?;
                            count += 1;
                        }
                    }
                }
                self.chunk.write_opcode(Opcode::NewObject);
                self.chunk.write_byte(count);
            }
            // Property access: obj.key
            Expr::Property(obj, key) => {
                self.compile_expr(obj)?;
                let key_u16 = crate::unicode::utf8_to_utf16(key);
                let name_idx = self.chunk.add_constant(Value::String(key_u16));
                self.chunk.write_opcode(Opcode::GetProperty);
                self.chunk.write_u16(name_idx);
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
                        self.chunk.write_opcode(Opcode::SetLocal);
                        self.chunk.write_byte(pos as u8);
                    } else {
                        let name_u16 = crate::unicode::utf8_to_utf16(name);
                        let name_idx = self.chunk.add_constant(Value::String(name_u16));
                        self.chunk.write_opcode(Opcode::SetGlobal);
                        self.chunk.write_u16(name_idx);
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
                _ => {
                    return Err(crate::raise_syntax_error!("Invalid assignment target for VM"));
                }
            },
            // Arrow function: (params) => body
            Expr::ArrowFunction(params, body) => {
                let jump_over = self.emit_jump(Opcode::Jump);
                let func_ip = self.chunk.code.len();
                let fn_is_strict = self.record_fn_strictness(func_ip, body, false);

                let old_locals = std::mem::take(&mut self.locals);
                let old_depth = self.scope_depth;
                let old_loops = std::mem::take(&mut self.loop_stack);
                let old_strict = self.current_strict;
                self.current_strict = fn_is_strict;
                self.scope_depth = 1;
                for param in params {
                    if let DestructuringElement::Variable(param_name, _) = param {
                        self.locals.push(param_name.clone());
                    }
                }

                if body.len() == 1 {
                    if let StatementKind::Expr(expr) = &*body[0].kind {
                        // Single expression body: implicitly return the value
                        self.compile_expr(expr)?;
                        self.chunk.write_opcode(Opcode::Return);
                    } else {
                        self.compile_statement(&body[0], true)?;
                        let idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(idx);
                        self.chunk.write_opcode(Opcode::Return);
                    }
                } else {
                    for (i, s) in body.iter().enumerate() {
                        self.compile_statement(s, i == body.len() - 1)?;
                    }
                    let idx = self.chunk.add_constant(Value::Undefined);
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                    self.chunk.write_opcode(Opcode::Return);
                }

                self.patch_jump(jump_over);
                self.locals = old_locals;
                self.scope_depth = old_depth;
                self.loop_stack = old_loops;
                self.current_strict = old_strict;

                let func_val = Value::VmFunction(func_ip, params.len() as u8);
                let func_idx = self.chunk.add_constant(func_val);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(func_idx);
            }
            // Anonymous function expression: function(params) { body }
            Expr::Function(_name, params, body) => {
                self.compile_function_body(params, body)?;
            }
            // new Constructor(args) — for now, special-case Error
            Expr::New(constructor, args) => {
                if let Expr::Var(name, ..) = &**constructor {
                    match name.as_str() {
                        "Error" | "TypeError" | "SyntaxError" | "RangeError" | "ReferenceError" => {
                            // Push error type name
                            let type_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(name)));
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
                        "Array" => {
                            // new Array("a","b","c") → NewArray
                            for a in args {
                                self.compile_expr(a)?;
                            }
                            self.chunk.write_opcode(Opcode::NewArray);
                            self.chunk.write_byte(args.len() as u8);
                        }
                        "Object" | "Number" | "Boolean" | "String" | "Date" => {
                            // Create typed wrapper: { __type__: "TypeName", __value__: arg }
                            let type_key = crate::unicode::utf8_to_utf16("__type__");
                            let type_key_idx = self.chunk.add_constant(Value::String(type_key));
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(type_key_idx);
                            let type_val = crate::unicode::utf8_to_utf16(name);
                            let type_val_idx = self.chunk.add_constant(Value::String(type_val));
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(type_val_idx);
                            if let Some(first_arg) = args.first() {
                                let val_key = crate::unicode::utf8_to_utf16("__value__");
                                let val_key_idx = self.chunk.add_constant(Value::String(val_key));
                                self.chunk.write_opcode(Opcode::Constant);
                                self.chunk.write_u16(val_key_idx);
                                self.compile_expr(first_arg)?;
                                self.chunk.write_opcode(Opcode::NewObject);
                                self.chunk.write_byte(2); // 2 key-value pairs
                            } else {
                                self.chunk.write_opcode(Opcode::NewObject);
                                self.chunk.write_byte(1); // 1 key-value pair
                            }
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
                            self.chunk.write_opcode(Opcode::Call);
                            self.chunk.write_byte(1);
                        }
                        _ => {
                            // Generic constructor: create object, call constructor with this
                            self.compile_expr(constructor)?;
                            for a in args {
                                self.compile_expr(a)?;
                            }
                            self.chunk.write_opcode(Opcode::NewCall);
                            self.chunk.write_byte(args.len() as u8);
                        }
                    }
                } else {
                    // Dynamic constructor: create object, call constructor with this
                    self.compile_expr(constructor)?;
                    for a in args {
                        self.compile_expr(a)?;
                    }
                    self.chunk.write_opcode(Opcode::NewCall);
                    self.chunk.write_byte(args.len() as u8);
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
                        let type_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16("SyntaxError")));
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(type_idx);
                        let msg_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(
                            "Delete of an unqualified identifier in strict mode.",
                        )));
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
                // Check if value is null or undefined: dup, typeof, compare to "undefined", or use IsNullish
                // Simpler: dup, push null, equal → if null jump; dup, push undefined, equal → if undef jump
                // Even simpler: just use Dup + JumpIfFalse pattern but also jump on null...
                // Best approach: dup, check null; if not null dup check undefined; if neither, keep left
                // Actually the simplest: since null and undefined are both falsy, but 0/"" are also falsy,
                // we need a proper nullish check. Let's inline it:
                // push null, Equal → if true jump to rhs
                // else dup original, push undefined, Equal → if true jump to rhs
                // Hmm, we already consumed the dup. Let me use a different approach:
                // eval left → dup → dup → push null → equal → jumpIfTrue(use_right)
                //                          → push undefined → equal → jumpIfTrue(use_right)
                //                          → jump(end) → use_right: pop → eval right → end:
                self.chunk.write_opcode(Opcode::Dup);
                let null_idx = self.chunk.add_constant(Value::Null);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(null_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_null = self.emit_jump(Opcode::JumpIfTrue);
                // Not null — check undefined
                self.chunk.write_opcode(Opcode::Dup);
                let undef_idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(undef_idx);
                self.chunk.write_opcode(Opcode::Equal);
                let is_undef = self.emit_jump(Opcode::JumpIfTrue);
                // Not nullish — keep left, jump to end
                let end_jump = self.emit_jump(Opcode::Jump);
                // is_null / is_undef: pop left value, evaluate right
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
                self.chunk.write_opcode(Opcode::Call);
                self.chunk.write_byte(args.len() as u8);
                let end_jump = self.emit_jump(Opcode::Jump);
                self.patch_jump(is_null);
                self.patch_jump(is_undef);
                self.chunk.write_opcode(Opcode::Pop);
                let idx = self.chunk.add_constant(Value::Undefined);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
                self.patch_jump(end_jump);
            }
            // Getter/Setter in object literal: compile as the inner function
            Expr::Getter(inner) | Expr::Setter(inner) => {
                self.compile_expr(inner)?;
            }
            _ => return Err(raise_syntax_error!(format!("Unimplemented expression type for VM: {expr:?}"))),
        }
        Ok(())
    }

    /// Write-back helper for increment/decrement: store the top-of-stack value
    /// back into the variable that `expr` represents.
    fn compile_store(&mut self, expr: &Expr) -> Result<(), JSError> {
        match expr {
            Expr::Var(name, ..) => {
                if let Some(pos) = self.locals.iter().rposition(|l| l == name) {
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let name_u16 = crate::unicode::utf8_to_utf16(name);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::SetGlobal);
                    self.chunk.write_u16(name_idx);
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
            Expr::Index(_obj, _idx) => {
                // Index store needs 3-way rotate which is complex
                // For now, fall through to error — handle inline in inc/dec if needed
                return Err(crate::raise_syntax_error!("Index increment/decrement not yet supported in VM"));
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
            let pos = self.locals.iter().position(|l| l == name).unwrap();
            self.chunk.write_opcode(Opcode::GetLocal);
            self.chunk.write_byte(pos as u8);
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
            let pos = self.locals.iter().position(|l| l == name).unwrap();
            self.chunk.write_opcode(Opcode::SetLocal);
            self.chunk.write_byte(pos as u8);
        } else {
            let n = crate::unicode::utf8_to_utf16(name);
            let ni = self.chunk.add_constant(Value::String(n));
            self.chunk.write_opcode(Opcode::SetGlobal);
            self.chunk.write_u16(ni);
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
            Expr::ArrowFunction(..) => Some(self.chunk.code.len() + 3),
            _ => None,
        }
    }

    /// Helper: compile a for-of loop body where the iteration variable is array-destructured.
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

        self.compile_expr(iterable_expr)?;
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

        self.compile_expr(iterable_expr)?;
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

    fn compile_function_body(&mut self, params: &[DestructuringElement], body: &[Statement]) -> Result<(), JSError> {
        let jump_over = self.emit_jump(Opcode::Jump);
        let func_ip = self.chunk.code.len();
        let fn_is_strict = self.record_fn_strictness(func_ip, body, false);
        let old_ctx = self.current_strict;
        self.current_strict = fn_is_strict;

        let old_locals = std::mem::take(&mut self.locals);
        let old_depth = self.scope_depth;
        let old_loops = std::mem::take(&mut self.loop_stack);
        let old_label = self.pending_label.take();
        self.scope_depth = 1;

        // Count non-rest params and check for rest
        let mut non_rest_count = 0u8;
        let mut has_rest = false;
        for param in params {
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
                _ => {}
            }
        }

        for (i, s) in body.iter().enumerate() {
            self.compile_statement(s, i == body.len() - 1)?;
        }

        let idx = self.chunk.add_constant(Value::Undefined);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(idx);
        self.chunk.write_opcode(Opcode::Return);

        self.patch_jump(jump_over);
        self.locals = old_locals;
        self.scope_depth = old_depth;
        self.loop_stack = old_loops;
        self.pending_label = old_label;

        // restore strict context inherited from outer scope
        self.current_strict = old_ctx;

        // Arity = non-rest params only (call site pushes all args, function collects rest)
        let arity = if has_rest { non_rest_count } else { params.len() as u8 };
        let func_val = Value::VmFunction(func_ip, arity);
        let func_idx = self.chunk.add_constant(func_val);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(func_idx);
        Ok(())
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
        self.emit_define_var(&temp);

        // runtime check: ensure iterator exists on the object (via prototype)
        self.emit_helper_get(&temp); // push arr
        let iter_key = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16("iterator")));
        self.chunk.write_opcode(Opcode::GetProperty); // will traverse prototype
        self.chunk.write_u16(iter_key);
        let ok_jump = self.emit_jump(Opcode::JumpIfTrue);
        // iterator missing → throw TypeError
        // Use NewError special-case constructor used elsewhere
        let type_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16("TypeError")));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(type_idx);
        let msg_idx = self
            .chunk
            .add_constant(Value::String(crate::unicode::utf8_to_utf16("iterator missing")));
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(msg_idx);
        self.chunk.write_opcode(Opcode::NewError);
        self.chunk.write_opcode(Opcode::Throw);
        self.patch_jump(ok_jump);
        // push arr again for further work
        self.emit_helper_get(&temp);

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
                        self.chunk.write_opcode(Opcode::Equal);
                        let skip_default = self.emit_jump(Opcode::JumpIfFalse);
                        self.chunk.write_opcode(Opcode::Pop); // pop undefined
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
                    let slice_k = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16("slice")));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(slice_k); // callee
                    let start_idx = self.chunk.add_constant(Value::Number(i as f64));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(start_idx); // arg
                    self.chunk.write_opcode(Opcode::Call);
                    self.chunk.write_byte(1 | 0x80); // method call
                    self.emit_define_var(name);
                }
                DestructuringElement::NestedArray(inner_elements, _default) => {
                    // temp[i], then destructure recursively
                    self.emit_helper_get(&temp);
                    let idx = self.chunk.add_constant(Value::Number(i as f64));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                    self.chunk.write_opcode(Opcode::GetIndex);
                    self.compile_array_destructuring(inner_elements)?;
                }
                DestructuringElement::NestedObject(inner_elements, _default) => {
                    // temp[i], then object destructure
                    self.emit_helper_get(&temp);
                    let idx = self.chunk.add_constant(Value::Number(i as f64));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(idx);
                    self.chunk.write_opcode(Opcode::GetIndex);
                    self.compile_object_destructuring_from_destr(inner_elements)?;
                }
                _ => {
                    // Property, ComputedProperty, RestPattern — skip unsupported
                }
            }
        }

        // Clean up synthetic temp
        if self.scope_depth > 0 {
            self.locals.retain(|l| l != &temp);
        }
        Ok(())
    }

    /// Compile object destructuring from ObjectDestructuringElement list.
    /// RHS value is on stack top. Pops RHS and defines variables.
    fn compile_object_destructuring(&mut self, elements: &[ObjectDestructuringElement]) -> Result<(), JSError> {
        let temp = format!("__destr_obj_{}__", self.forin_counter);
        self.forin_counter += 1;
        self.emit_define_var(&temp);

        // Collect statically-known extracted keys for rest computation
        let mut extracted_keys: Vec<String> = Vec::new();

        for elem in elements {
            match elem {
                ObjectDestructuringElement::Property { key, value } => {
                    extracted_keys.push(key.clone());
                    self.emit_helper_get(&temp);
                    let k = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(key)));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(k);
                    self.compile_destructuring_target(value)?;
                }
                ObjectDestructuringElement::ComputedProperty { key, value } => {
                    self.emit_helper_get(&temp);
                    self.compile_expr(key)?;
                    self.chunk.write_opcode(Opcode::GetIndex);
                    self.compile_destructuring_target(value)?;
                }
                ObjectDestructuringElement::Rest(name) => {
                    // Build a new object with all keys from temp except extracted_keys.
                    // Emit: empty object {}, then for each key in temp, if key not in excluded list, copy it.
                    // Approach: use Object.keys(temp) iteration at runtime.
                    // For simplicity, emit inline: NewObject(0), then for each key we need to
                    // iterate — but we can't know keys at compile time.
                    // Use a runtime approach: call a synthetic helper.
                    // Actually, simplest: build the rest object by using for-in style iteration
                    // at compile time is impossible (we don't know the keys).
                    // Compromise: emit code that creates rest as {} and copies non-excluded props.
                    // We'll use Object.keys + for loop — but that's complex bytecode.
                    //
                    // Simpler approach: emit a new object {}, then for each key returned by
                    // a keys builtin, if it's not in excluded set, copy value.
                    // For now, use a simpler approach that works for the test:
                    // Just use Object.assign-style copy and delete the extracted keys.
                    //
                    // Simplest correct approach: push excluded keys as array, push temp,
                    // and call a builtin that filters. Since we don't have that builtin,
                    // let's do compile-time key enumeration using for-in.

                    // Step 1: Create empty object → rest = {}
                    self.chunk.write_opcode(Opcode::NewObject);
                    self.chunk.write_byte(0);
                    let rest_temp = format!("__rest_{}__", self.forin_counter);
                    self.forin_counter += 1;
                    self.emit_define_var(&rest_temp);

                    // Step 2: for (k in temp) { if k not in excluded, rest[k] = temp[k] }
                    // Use the same for-in desugaring as ForIn statement
                    let keys_temp = format!("__rest_keys_{}__", self.forin_counter);
                    self.forin_counter += 1;
                    let ki_temp = format!("__rest_ki_{}__", self.forin_counter);
                    self.forin_counter += 1;

                    // Push Object.keys(temp) as array
                    self.emit_helper_get(&temp);
                    self.chunk.write_opcode(Opcode::GetKeys);
                    self.emit_define_var(&keys_temp);

                    // idx = 0
                    let zero = self.chunk.add_constant(Value::Number(0.0));
                    self.chunk.write_opcode(Opcode::Constant);
                    self.chunk.write_u16(zero);
                    self.emit_define_var(&ki_temp);

                    // Loop: while idx < keys.length
                    let loop_start = self.chunk.code.len();
                    self.emit_helper_get(&ki_temp);
                    self.emit_helper_get(&keys_temp);
                    let len_key = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16("length")));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(len_key);
                    self.chunk.write_opcode(Opcode::LessThan);
                    let exit_jump = self.emit_jump(Opcode::JumpIfFalse);

                    // k = keys[idx]
                    self.emit_helper_get(&keys_temp);
                    self.emit_helper_get(&ki_temp);
                    self.chunk.write_opcode(Opcode::GetIndex);
                    // Now key is on stack. Check if it's in excluded list.
                    // For each excluded key, compare and skip if match.
                    let mut skip_patches = Vec::new();
                    for ek in &extracted_keys {
                        self.chunk.write_opcode(Opcode::Dup);
                        let ek_idx = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(ek)));
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(ek_idx);
                        self.chunk.write_opcode(Opcode::Equal);
                        let skip = self.emit_jump(Opcode::JumpIfTrue);
                        skip_patches.push(skip);
                    }

                    // Not excluded: rest[k] = temp[k]
                    // Stack: [..., key]
                    // Dup key for SetIndex later
                    self.chunk.write_opcode(Opcode::Dup); // key key
                    // Get value: temp[key]
                    self.emit_helper_get(&temp);
                    // Stack: key key temp
                    // Swap so we can do GetIndex: temp key → need to rearrange
                    // Actually, let's use a different approach: store key in temp var
                    let k_temp = format!("__rest_k_{}__", self.forin_counter);
                    self.forin_counter += 1;
                    // Stack: key key temp
                    // Hmm, let me restructure. Stack at this point: key
                    // Let me pop the dup and redo.
                    self.chunk.write_opcode(Opcode::Pop); // undo the dup above

                    // key is on stack. Store it.
                    self.chunk.write_opcode(Opcode::Dup);
                    self.emit_define_var(&k_temp);
                    // Stack: key (still on top from Dup before define)
                    // Actually emit_define_var may push or pop differently...
                    // Let me be more careful.
                    // After Dup+emit_define_var, if scope>0 and new local: Dup pushes copy,
                    //   then the copy stays as local slot. Original key still on stack? No.
                    // If local is new: Dup → [key, key], emit_define_var pushes key copy as local,
                    //   key stays on top? No: locals.push means TOS becomes the local, so stack is [key].
                    //   Wait — Dup pushes a copy, then the copy IS the new local. So stack: [key].
                    //   We still have the original key on stack. Actually no.
                    //
                    // Let me think again more carefully:
                    // Before Dup: stack has [..., key]
                    // After Dup: stack has [..., key, key_copy]
                    // emit_define_var(&k_temp) when scope>0 and new local: key_copy becomes local slot.
                    //   Stack is just [..., key]. Good, original key is still on top.

                    // Now: rest[key] = temp[key]
                    // We need: rest obj on stack, key, value → SetIndex
                    // rest (obj)
                    self.emit_helper_get(&rest_temp);
                    // Stack: [..., key, rest_obj]
                    // Swap: we need obj, key, val order for SetIndex? Let me check SetIndex.
                    // SetIndex pops val, index, obj.
                    // So we need stack: [..., obj, index, val]
                    // i.e.: rest, key, temp[key]
                    // But we have: key, rest on stack. We need to swap.
                    // Instead, let me just reload everything.
                    self.chunk.write_opcode(Opcode::Pop); // pop rest
                    self.chunk.write_opcode(Opcode::Pop); // pop key

                    // Emit: rest[k_temp] = temp[k_temp]
                    self.emit_helper_get(&rest_temp); // obj
                    self.emit_helper_get(&k_temp); // index (key)
                    // value: temp[k_temp]
                    self.emit_helper_get(&temp);
                    self.emit_helper_get(&k_temp);
                    self.chunk.write_opcode(Opcode::GetIndex); // temp[key]
                    self.chunk.write_opcode(Opcode::SetIndex);
                    self.chunk.write_opcode(Opcode::Pop); // SetIndex leaves obj on stack? Check...

                    // Jump to increment
                    let to_inc = self.emit_jump(Opcode::Jump);

                    // Patch skip jumps (excluded key matched) — pop key and continue
                    for sp in skip_patches {
                        self.patch_jump(sp);
                    }
                    self.chunk.write_opcode(Opcode::Pop); // pop key

                    self.patch_jump(to_inc);

                    // idx++
                    self.emit_helper_get(&ki_temp);
                    self.chunk.write_opcode(Opcode::Increment);
                    self.emit_helper_set(&ki_temp);
                    self.chunk.write_opcode(Opcode::Pop);

                    self.emit_loop(loop_start);
                    self.patch_jump(exit_jump);

                    // Clean up synthetic locals
                    if self.scope_depth > 0 {
                        self.locals.retain(|l| l != &keys_temp && l != &ki_temp && l != &k_temp);
                    }

                    // Push rest object and define the actual variable
                    self.emit_helper_get(&rest_temp);
                    self.emit_define_var(name);

                    if self.scope_depth > 0 {
                        self.locals.retain(|l| l != &rest_temp);
                    }
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
                    self.chunk.write_opcode(Opcode::Equal);
                    let skip_default = self.emit_jump(Opcode::JumpIfFalse);
                    self.chunk.write_opcode(Opcode::Pop);
                    self.compile_expr(def_expr)?;
                    self.patch_jump(skip_default);
                }
                self.emit_define_var(name);
            }
            DestructuringElement::NestedArray(inner, _default) => {
                self.compile_array_destructuring(inner)?;
            }
            DestructuringElement::NestedObject(inner, _default) => {
                self.compile_object_destructuring_from_destr(inner)?;
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
        self.emit_define_var(&temp);

        for elem in elements {
            match elem {
                DestructuringElement::Variable(name, default) => {
                    // Shorthand: {name} = obj → obj.name
                    self.emit_helper_get(&temp);
                    let k = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(name)));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(k);
                    if let Some(def_expr) = default {
                        self.chunk.write_opcode(Opcode::Dup);
                        let undef_idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(undef_idx);
                        self.chunk.write_opcode(Opcode::Equal);
                        let skip_default = self.emit_jump(Opcode::JumpIfFalse);
                        self.chunk.write_opcode(Opcode::Pop);
                        self.compile_expr(def_expr)?;
                        self.patch_jump(skip_default);
                    }
                    self.emit_define_var(name);
                }
                DestructuringElement::Property(key, target) => {
                    // {key: target} = obj → obj.key, then assign to target
                    self.emit_helper_get(&temp);
                    let k = self.chunk.add_constant(Value::String(crate::unicode::utf8_to_utf16(key)));
                    self.chunk.write_opcode(Opcode::GetProperty);
                    self.chunk.write_u16(k);
                    self.compile_destructuring_target(target)?;
                }
                DestructuringElement::Rest(name) => {
                    self.emit_helper_get(&temp);
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
}
