use crate::core::opcode::{Chunk, Opcode};
use crate::core::statement::{BinaryOp, CatchParamPattern, DestructuringElement, Expr, Statement, StatementKind};
use crate::core::{JSError, Value};

pub struct Compiler<'gc> {
    chunk: Chunk<'gc>,
    locals: Vec<String>,
    scope_depth: i32, // 0 = top-level (global), > 0 = inside function
    loop_stack: Vec<LoopContext>,
    pending_label: Option<String>, // label to attach to the next loop
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
            loop_stack: Vec::new(),
            pending_label: None,
        }
    }

    pub fn compile(mut self, statements: &[Statement]) -> Result<Chunk<'gc>, JSError> {
        for (i, stmt) in statements.iter().enumerate() {
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
                self.compile_expr(expr)?;
                // Pop if it's not the last evaluated statement, to keep stack clean
                if !is_last {
                    self.chunk.write_opcode(Opcode::Pop);
                }
            }
            StatementKind::Let(decls) | StatementKind::Var(decls) => {
                for (name, init_opt) in decls {
                    if let Some(init) = init_opt {
                        self.compile_expr(init)?;
                    } else {
                        let idx = self.chunk.add_constant(Value::Undefined);
                        self.chunk.write_opcode(Opcode::Constant);
                        self.chunk.write_u16(idx);
                    }

                    if self.scope_depth > 0 {
                        // Inside a function: check if var already exists (var is function-scoped)
                        if let Some(pos) = self.locals.iter().position(|l| l == name) {
                            // Re-declaration: assign to existing slot
                            self.chunk.write_opcode(Opcode::SetLocal);
                            self.chunk.write_byte(pos as u8);
                            self.chunk.write_opcode(Opcode::Pop);
                        } else {
                            // New local: value stays on stack as a local slot
                            self.locals.push(name.clone());
                        }
                    } else {
                        // Top-level: define as global
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
                    self.compile_expr(init)?;
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
                if let Some(pos) = self.locals.iter().position(|l| l == name) {
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
                for (i, s) in statements.iter().enumerate() {
                    let s_is_last = is_last && i == statements.len() - 1;
                    self.compile_statement(s, s_is_last)?;
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
                // Compile: keys = GetKeys(obj); for (i=0; i<keys.length; i++) { var_name = keys[i]; body }
                self.compile_expr(obj_expr)?;
                self.chunk.write_opcode(Opcode::GetKeys);
                // keys array is now on stack; store as a local (or global)
                let _keys_local = if self.scope_depth > 0 {
                    self.locals.push("__keys__".to_string());
                    None
                } else {
                    let n = crate::unicode::utf8_to_utf16("__keys__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(ni);
                    Some(ni)
                };
                // i = 0
                let zero_idx = self.chunk.add_constant(Value::Number(0.0));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(zero_idx);
                let _idx_local = if self.scope_depth > 0 {
                    self.locals.push("__idx__".to_string());
                    None
                } else {
                    let n = crate::unicode::utf8_to_utf16("__idx__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::DefineGlobal);
                    self.chunk.write_u16(ni);
                    Some(ni)
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
                    let pos = self.locals.iter().position(|l| l == "__idx__").unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16("__idx__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(ni);
                }
                // push keys.length
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| l == "__keys__").unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16("__keys__");
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
                    let pos = self.locals.iter().position(|l| l == "__keys__").unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16("__keys__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(ni);
                }
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| l == "__idx__").unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16("__idx__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(ni);
                }
                self.chunk.write_opcode(Opcode::GetIndex);
                // Store in var_name (always emit SetLocal+Pop so it works on every iteration)
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

                // Body
                for s in body {
                    self.compile_statement(s, false)?;
                }

                // continue target: i++ update
                let update_ip = self.chunk.code.len();
                for cp in &self.loop_stack.last().unwrap().continue_patches.clone() {
                    self.patch_jump_to(*cp, update_ip);
                }

                // i++
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| l == "__idx__").unwrap();
                    self.chunk.write_opcode(Opcode::GetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16("__idx__");
                    let ni = self.chunk.add_constant(Value::String(n));
                    self.chunk.write_opcode(Opcode::GetGlobal);
                    self.chunk.write_u16(ni);
                }
                self.chunk.write_opcode(Opcode::Increment);
                if self.scope_depth > 0 {
                    let pos = self.locals.iter().position(|l| l == "__idx__").unwrap();
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let n = crate::unicode::utf8_to_utf16("__idx__");
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
                    self.locals.retain(|l| l != "__keys__" && l != "__idx__");
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

                // Try body
                for s in &tc.try_body {
                    self.compile_statement(s, false)?;
                }
                self.chunk.write_opcode(Opcode::TeardownTry);

                // Jump over catch block
                let jump_over_catch = self.emit_jump(Opcode::Jump);

                // Patch catch address to here
                let catch_start = self.chunk.code.len();
                self.chunk.code[catch_placeholder] = (catch_start & 0xff) as u8;
                self.chunk.code[catch_placeholder + 1] = ((catch_start >> 8) & 0xff) as u8;

                // Catch body
                if let Some(ref catch_body) = tc.catch_body {
                    for s in catch_body {
                        self.compile_statement(s, false)?;
                    }
                }

                self.patch_jump(jump_over_catch);

                // Finally body
                if let Some(ref finally_body) = tc.finally_body {
                    for s in finally_body {
                        self.compile_statement(s, false)?;
                    }
                }

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

                // Save and reset locals/scope for function scope
                let old_locals = std::mem::take(&mut self.locals);
                let old_depth = self.scope_depth;
                let old_loops = std::mem::take(&mut self.loop_stack);
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

                // Push the VmFunction value and define it as a global
                let func_val = Value::VmFunction(func_ip, params.len() as u8);
                let func_idx = self.chunk.add_constant(func_val);
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(func_idx);

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
                if let Some(pos) = self.locals.iter().position(|l| l == name) {
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
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::TypeOf);
            }
            // Logical operators (short-circuit)
            Expr::LogicalAnd(left, right) => {
                // Evaluate left; if falsy, short-circuit (keep left value)
                self.compile_expr(left)?;
                let end_jump = self.emit_jump(Opcode::JumpIfFalse);
                // Left was truthy, discard it and evaluate right
                self.compile_expr(right)?;
                let skip = self.emit_jump(Opcode::Jump);
                self.patch_jump(end_jump);
                // Left was falsy, push false
                let idx = self.chunk.add_constant(Value::Boolean(false));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
                self.patch_jump(skip);
            }
            Expr::LogicalOr(left, right) => {
                // Evaluate left; if truthy, short-circuit (keep left value)
                self.compile_expr(left)?;
                let end_jump = self.emit_jump(Opcode::JumpIfTrue);
                // Left was falsy, discard it and evaluate right
                self.compile_expr(right)?;
                let skip = self.emit_jump(Opcode::Jump);
                self.patch_jump(end_jump);
                // Left was truthy, push true
                let idx = self.chunk.add_constant(Value::Boolean(true));
                self.chunk.write_opcode(Opcode::Constant);
                self.chunk.write_u16(idx);
                self.patch_jump(skip);
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
                    self.compile_expr(key)?;
                    self.compile_expr(val)?;
                    count += 1;
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
            Expr::PostIncrement(inner) => {
                self.compile_expr(inner)?;
                // Duplicate: keep old value on stack below
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::Increment);
                self.compile_store(inner)?;
                // Pop the incremented value, keep original
                self.chunk.write_opcode(Opcode::Pop);
            }
            // Postfix decrement: x--
            Expr::PostDecrement(inner) => {
                self.compile_expr(inner)?;
                self.compile_expr(inner)?;
                self.chunk.write_opcode(Opcode::Decrement);
                self.compile_store(inner)?;
                self.chunk.write_opcode(Opcode::Pop);
            }
            // Assignment to property: obj.key = val, obj[i] = val
            Expr::Assign(left, right) => match &**left {
                Expr::Var(name, ..) => {
                    self.compile_expr(right)?;
                    if let Some(pos) = self.locals.iter().position(|l| l == name) {
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
                _ => {
                    return Err(crate::raise_syntax_error!("Invalid assignment target for VM"));
                }
            },
            // Arrow function: (params) => body
            Expr::ArrowFunction(params, body) => {
                let jump_over = self.emit_jump(Opcode::Jump);
                let func_ip = self.chunk.code.len();

                let old_locals = std::mem::take(&mut self.locals);
                let old_depth = self.scope_depth;
                let old_loops = std::mem::take(&mut self.loop_stack);
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
                    if name == "Error" {
                        // new Error(msg) → VmObject { message: msg }
                        // Evaluate message arg or default to ""
                        if let Some(arg) = args.first() {
                            self.compile_expr(arg)?;
                        } else {
                            let idx = self.chunk.add_constant(Value::String(Vec::new()));
                            self.chunk.write_opcode(Opcode::Constant);
                            self.chunk.write_u16(idx);
                        }
                        // Use NewError opcode
                        self.chunk.write_opcode(Opcode::NewError);
                    } else {
                        return Err(crate::raise_syntax_error!(format!("Unimplemented new target for VM: {name}")));
                    }
                } else {
                    return Err(crate::raise_syntax_error!(format!(
                        "Unimplemented new expression for VM: {constructor:?}"
                    )));
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
            _ => {
                return Err(crate::raise_syntax_error!(format!(
                    "Unimplemented expression type for VM: {expr:?}"
                )));
            }
        }
        Ok(())
    }

    /// Write-back helper for increment/decrement: store the top-of-stack value
    /// back into the variable that `expr` represents.
    fn compile_store(&mut self, expr: &Expr) -> Result<(), JSError> {
        match expr {
            Expr::Var(name, ..) => {
                if let Some(pos) = self.locals.iter().position(|l| l == name) {
                    self.chunk.write_opcode(Opcode::SetLocal);
                    self.chunk.write_byte(pos as u8);
                } else {
                    let name_u16 = crate::unicode::utf8_to_utf16(name);
                    let name_idx = self.chunk.add_constant(Value::String(name_u16));
                    self.chunk.write_opcode(Opcode::SetGlobal);
                    self.chunk.write_u16(name_idx);
                }
            }
            _ => {
                return Err(crate::raise_syntax_error!("Invalid increment/decrement target for VM"));
            }
        }
        Ok(())
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

    /// Compile a function body (shared between FunctionDeclaration, ArrowFunction, and Function expression)
    fn compile_function_body(&mut self, params: &[DestructuringElement], body: &[Statement]) -> Result<(), JSError> {
        let jump_over = self.emit_jump(Opcode::Jump);
        let func_ip = self.chunk.code.len();

        let old_locals = std::mem::take(&mut self.locals);
        let old_depth = self.scope_depth;
        let old_loops = std::mem::take(&mut self.loop_stack);
        let old_label = self.pending_label.take();
        self.scope_depth = 1;
        for param in params {
            if let DestructuringElement::Variable(param_name, _) = param {
                self.locals.push(param_name.clone());
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

        let func_val = Value::VmFunction(func_ip, params.len() as u8);
        let func_idx = self.chunk.add_constant(func_val);
        self.chunk.write_opcode(Opcode::Constant);
        self.chunk.write_u16(func_idx);
        Ok(())
    }
}
