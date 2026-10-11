//! Generation of the core WebAssembly module of a world (§ 12).

use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, ExportKind, ExportSection, Function, FunctionSection,
    GlobalSection, GlobalType, Ieee32, Ieee64, Instruction, Module, TypeSection, ValType,
};

use crate::ast::{BinOp, UnaryOp};
use crate::diag::{Code, Diagnostic, Result};
use crate::hir::{Const, Expr, ExprKind, FuncDef, FuncId, GlobalId, Place, Program, Stmt};
use crate::types::Type;
use crate::wit::kebab;

/// Produces the core module of world `world`: its exported functions, the functions they
/// call transitively and the globals they use.
///
/// `interface_names` gives the canonical WIT name of each exported interface.
pub fn generate(
    program: &Program,
    world: usize,
    interface_names: &[Option<String>],
) -> Result<Vec<u8>> {
    let world = &program.worlds[world];

    // Functions and globals reachable from the exports.
    let mut used_funcs = vec![false; program.funcs.len()];
    let mut used_globals = vec![false; program.globals.len()];
    let mut work: Vec<FuncId> = world
        .exports
        .iter()
        .flat_map(|&i| program.interfaces[i].funcs.iter().copied())
        .collect();
    while let Some(f) = work.pop() {
        if std::mem::replace(&mut used_funcs[f], true) {
            continue;
        }
        visit_stmts(&program.funcs[f].body, &mut |e| match e.kind {
            ExprKind::Call { func, .. } if !used_funcs[func] => work.push(func),
            ExprKind::Global(g) => used_globals[g] = true,
            _ => {}
        });
        mark_assigned_globals(&program.funcs[f].body, &mut used_globals);
    }
    for (id, func) in program.funcs.iter().enumerate() {
        let scalar = func
            .locals
            .iter()
            .chain(&func.ret)
            .all(|t| t.as_prim().is_some())
            && func.params.iter().all(|(_, t)| t.as_prim().is_some());
        if used_funcs[id] && !scalar {
            return Err(Diagnostic::without_span(
                Code::Unsupported,
                format!(
                    "function `{}::{}` uses types other than numbers and `bool`, which cannot be compiled yet",
                    program.interfaces[func.interface].name, func.name
                ),
            ));
        }
    }
    let func_index = index_map(&used_funcs);
    let global_index = index_map(&used_globals);

    let mut types = TypeSection::new();
    let mut signatures: Vec<(Vec<ValType>, Vec<ValType>)> = Vec::new();
    let mut functions = FunctionSection::new();
    let mut code = CodeSection::new();
    for (id, func) in program.funcs.iter().enumerate() {
        if !used_funcs[id] {
            continue;
        }
        let params: Vec<ValType> = func.params.iter().map(|(_, t)| t.val_type()).collect();
        let results: Vec<ValType> = func.ret.iter().map(|t| t.val_type()).collect();
        let sig = (params, results);
        let type_index = match signatures.iter().position(|s| *s == sig) {
            Some(i) => i,
            None => {
                types.ty().function(sig.0.clone(), sig.1.clone());
                signatures.push(sig);
                signatures.len() - 1
            }
        };
        functions.function(type_index as u32);
        code.function(&FuncGen::run(func, &func_index, &global_index));
    }

    let mut globals = GlobalSection::new();
    for (id, global) in program.globals.iter().enumerate() {
        if !used_globals[id] {
            continue;
        }
        let init = match global.init {
            Const::I32(v) => ConstExpr::i32_const(v),
            Const::I64(v) => ConstExpr::i64_const(v),
            Const::F32(v) => ConstExpr::f32_const(Ieee32::from(v)),
            Const::F64(v) => ConstExpr::f64_const(Ieee64::from(v)),
        };
        globals.global(
            GlobalType {
                val_type: global.ty.val_type(),
                mutable: global.mutable,
                shared: false,
            },
            &init,
        );
    }

    // Standard Component Model export names: `cm32p2|<interface>|<function>`.
    let mut exports = ExportSection::new();
    for &iface in &world.exports {
        let iface_def = &program.interfaces[iface];
        let path = interface_names[iface]
            .as_deref()
            .expect("exported interface present in the WIT");
        for &f in &iface_def.funcs {
            let name = format!("cm32p2|{path}|{}", kebab(&program.funcs[f].name));
            exports.export(&name, ExportKind::Func, func_index[f].unwrap());
        }
    }

    let mut module = Module::new();
    module.section(&types);
    module.section(&functions);
    module.section(&globals);
    module.section(&exports);
    module.section(&code);
    Ok(module.finish())
}

/// Maps each used item to its index in the module.
fn index_map(used: &[bool]) -> Vec<Option<u32>> {
    let mut next = 0;
    used.iter()
        .map(|&u| {
            u.then(|| {
                next += 1;
                next - 1
            })
        })
        .collect()
}

fn mark_assigned_globals(stmts: &[Stmt], used: &mut [bool]) {
    for stmt in stmts {
        match stmt {
            Stmt::Assign {
                place: Place::Global(g),
                ..
            } => used[*g] = true,
            Stmt::If { then, else_, .. } => {
                mark_assigned_globals(then, used);
                mark_assigned_globals(else_, used);
            }
            Stmt::While { body, .. } => mark_assigned_globals(body, used),
            _ => {}
        }
    }
}

/// Visits every expression of a list of statements.
fn visit_stmts(stmts: &[Stmt], f: &mut impl FnMut(&Expr)) {
    for stmt in stmts {
        match stmt {
            Stmt::Local { init, .. } => init.iter().for_each(|e| visit_expr(e, f)),
            Stmt::Assign { value, .. } => visit_expr(value, f),
            Stmt::If { cond, then, else_ } => {
                visit_expr(cond, f);
                visit_stmts(then, f);
                visit_stmts(else_, f);
            }
            Stmt::While { cond, body } => {
                visit_expr(cond, f);
                visit_stmts(body, f);
            }
            Stmt::Return(value) => value.iter().for_each(|e| visit_expr(e, f)),
            Stmt::Call(e) => visit_expr(e, f),
            Stmt::Break | Stmt::Continue => {}
        }
    }
}

fn visit_expr(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    match &e.kind {
        ExprKind::Call { args, .. } => args.iter().for_each(|a| visit_expr(a, f)),
        ExprKind::Unary { expr, .. } | ExprKind::Convert(expr) => visit_expr(expr, f),
        ExprKind::Binary { lhs, rhs, .. } => {
            visit_expr(lhs, f);
            visit_expr(rhs, f);
        }
        ExprKind::Const(_) | ExprKind::Local(_) | ExprKind::Global(_) => {}
    }
}

/// Wasm block opened by a statement, used to compute `br` targets.
#[derive(Clone, Copy, PartialEq)]
enum Frame {
    /// `block` around a `while`: target of `break`.
    LoopExit,
    /// `loop` of a `while`: target of `continue`.
    LoopStart,
    If,
}

struct FuncGen<'a> {
    local_types: &'a [Type],
    func_index: &'a [Option<u32>],
    global_index: &'a [Option<u32>],
    instrs: Vec<Instruction<'static>>,
    frames: Vec<Frame>,
    /// Index of the next scratch local (after the variables of the function).
    next_scratch: u32,
    scratch_count: u32,
}

impl<'a> FuncGen<'a> {
    fn run(
        func: &'a FuncDef,
        func_index: &'a [Option<u32>],
        global_index: &'a [Option<u32>],
    ) -> Function {
        let mut cg = FuncGen {
            local_types: &func.locals,
            func_index,
            global_index,
            instrs: Vec::new(),
            frames: Vec::new(),
            next_scratch: func.locals.len() as u32,
            scratch_count: 0,
        };
        cg.stmts(&func.body);
        if func.ret.is_some() {
            // Every path ends with `return` (checked by semantic analysis);
            // the end of the body is unreachable.
            cg.emit(Instruction::Unreachable);
        }
        cg.emit(Instruction::End);

        let mut locals: Vec<(u32, ValType)> = func.locals[func.params.len()..]
            .iter()
            .map(|t| (1, t.val_type()))
            .collect();
        if cg.scratch_count > 0 {
            locals.push((cg.scratch_count, ValType::I32));
        }
        let mut function = Function::new(locals);
        for instr in &cg.instrs {
            function.instruction(instr);
        }
        function
    }

    fn emit(&mut self, instr: Instruction<'static>) {
        self.instrs.push(instr);
    }

    /// Reserves an `i32` scratch local.
    fn scratch(&mut self) -> u32 {
        self.scratch_count += 1;
        self.next_scratch += 1;
        self.next_scratch - 1
    }

    // ---- Statements ----

    fn stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Local { local, init } => {
                match init {
                    Some(e) => self.expr(e),
                    None => {
                        // A local may be redeclared on every loop iteration: reset it to zero.
                        let ty = self.local_types[*local as usize].val_type();
                        self.zero(ty);
                    }
                }
                self.emit(Instruction::LocalSet(*local));
            }
            Stmt::Assign { place, value } => {
                self.expr(value);
                match place {
                    Place::Local(l) => self.emit(Instruction::LocalSet(*l)),
                    Place::Global(g) => self.emit(Instruction::GlobalSet(self.global(*g))),
                }
            }
            Stmt::If { cond, then, else_ } => {
                self.expr(cond);
                self.emit(Instruction::If(BlockType::Empty));
                self.frames.push(Frame::If);
                self.stmts(then);
                if !else_.is_empty() {
                    self.emit(Instruction::Else);
                    self.stmts(else_);
                }
                self.frames.pop();
                self.emit(Instruction::End);
            }
            Stmt::While { cond, body } => {
                self.emit(Instruction::Block(BlockType::Empty));
                self.frames.push(Frame::LoopExit);
                self.emit(Instruction::Loop(BlockType::Empty));
                self.frames.push(Frame::LoopStart);
                self.expr(cond);
                self.emit(Instruction::I32Eqz);
                self.emit(Instruction::BrIf(1));
                self.stmts(body);
                self.emit(Instruction::Br(0));
                self.frames.pop();
                self.emit(Instruction::End);
                self.frames.pop();
                self.emit(Instruction::End);
            }
            Stmt::Break => self.branch(Frame::LoopExit),
            Stmt::Continue => self.branch(Frame::LoopStart),
            Stmt::Return(value) => {
                if let Some(e) = value {
                    self.expr(e);
                }
                self.emit(Instruction::Return);
            }
            Stmt::Call(e) => {
                self.expr(e);
                if e.ty.is_some() {
                    self.emit(Instruction::Drop);
                }
            }
        }
    }

    fn branch(&mut self, target: Frame) {
        let index = self
            .frames
            .iter()
            .rposition(|f| *f == target)
            .expect("break/continue inside a loop");
        self.emit(Instruction::Br((self.frames.len() - 1 - index) as u32));
    }

    fn global(&self, g: GlobalId) -> u32 {
        self.global_index[g].expect("used global")
    }

    fn zero(&mut self, ty: ValType) {
        self.emit(match ty {
            ValType::I32 => Instruction::I32Const(0),
            ValType::I64 => Instruction::I64Const(0),
            ValType::F32 => Instruction::F32Const(Ieee32::from(0.0f32)),
            ValType::F64 => Instruction::F64Const(Ieee64::from(0.0f64)),
            _ => unreachable!(),
        });
    }

    // ---- Expressions ----

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Const(c) => self.emit(match *c {
                Const::I32(v) => Instruction::I32Const(v),
                Const::I64(v) => Instruction::I64Const(v),
                Const::F32(v) => Instruction::F32Const(Ieee32::from(v)),
                Const::F64(v) => Instruction::F64Const(Ieee64::from(v)),
            }),
            ExprKind::Local(l) => self.emit(Instruction::LocalGet(*l)),
            ExprKind::Global(g) => self.emit(Instruction::GlobalGet(self.global(*g))),
            ExprKind::Call { func, args } => {
                for arg in args {
                    self.expr(arg);
                }
                self.emit(Instruction::Call(
                    self.func_index[*func].expect("used function"),
                ));
            }
            ExprKind::Unary { op, expr } => self.unary(*op, expr),
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs),
            ExprKind::Convert(inner) => {
                self.expr(inner);
                self.convert(inner.value_ty(), e.value_ty());
            }
        }
    }

    /// Brings an 8- or 16-bit integer back into its range (§ 12.2).
    fn normalize(&mut self, ty: Type) {
        match ty {
            Type::S8 => self.emit(Instruction::I32Extend8S),
            Type::S16 => self.emit(Instruction::I32Extend16S),
            Type::U8 => {
                self.emit(Instruction::I32Const(0xFF));
                self.emit(Instruction::I32And);
            }
            Type::U16 => {
                self.emit(Instruction::I32Const(0xFFFF));
                self.emit(Instruction::I32And);
            }
            _ => {}
        }
    }

    fn unary(&mut self, op: UnaryOp, operand: &Expr) {
        let ty = operand.value_ty();
        match op {
            UnaryOp::Not => {
                self.expr(operand);
                self.emit(Instruction::I32Eqz);
            }
            UnaryOp::Neg => match ty {
                Type::F32 => {
                    self.expr(operand);
                    self.emit(Instruction::F32Neg);
                }
                Type::F64 => {
                    self.expr(operand);
                    self.emit(Instruction::F64Neg);
                }
                _ => {
                    // -x = 0 - x, wrapping.
                    self.zero(ty.val_type());
                    self.expr(operand);
                    self.emit(if ty.val_type() == ValType::I64 {
                        Instruction::I64Sub
                    } else {
                        Instruction::I32Sub
                    });
                    self.normalize(ty);
                }
            },
        }
    }

    fn binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) {
        match op {
            BinOp::And => {
                self.expr(lhs);
                self.emit(Instruction::If(BlockType::Result(ValType::I32)));
                self.expr(rhs);
                self.emit(Instruction::Else);
                self.emit(Instruction::I32Const(0));
                self.emit(Instruction::End);
                return;
            }
            BinOp::Or => {
                self.expr(lhs);
                self.emit(Instruction::If(BlockType::Result(ValType::I32)));
                self.emit(Instruction::I32Const(1));
                self.emit(Instruction::Else);
                self.expr(rhs);
                self.emit(Instruction::End);
                return;
            }
            _ => {}
        }

        let ty = lhs.value_ty();
        if op == BinOp::Div && matches!(ty, Type::S8 | Type::S16) {
            // `i32.div_s` does not overflow for -128 / -1: the trap is emitted explicitly, by
            // dividing i32::MIN by -1 so that it is reported as an integer overflow.
            let (a, b) = (self.scratch(), self.scratch());
            let min = if ty == Type::S8 { -128 } else { -32768 };
            self.expr(lhs);
            self.emit(Instruction::LocalTee(a));
            self.expr(rhs);
            self.emit(Instruction::LocalTee(b));
            self.emit(Instruction::LocalGet(b));
            self.emit(Instruction::I32Const(-1));
            self.emit(Instruction::I32Eq);
            self.emit(Instruction::LocalGet(a));
            self.emit(Instruction::I32Const(min));
            self.emit(Instruction::I32Eq);
            self.emit(Instruction::I32And);
            self.emit(Instruction::If(BlockType::Empty));
            self.emit(Instruction::I32Const(i32::MIN));
            self.emit(Instruction::I32Const(-1));
            self.emit(Instruction::I32DivS);
            self.emit(Instruction::Drop);
            self.emit(Instruction::End);
            self.emit(Instruction::I32DivS);
            return;
        }

        self.expr(lhs);
        self.expr(rhs);
        if matches!(op, BinOp::Shl | BinOp::Shr) && ty.is_small_int() {
            // Shift amount taken modulo the bit width of the type (§ 10.2).
            self.emit(Instruction::I32Const(ty.bits() as i32 - 1));
            self.emit(Instruction::I32And);
        }
        self.emit(binary_instr(op, ty));
        if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Shl) {
            self.normalize(ty);
        }
    }

    fn convert(&mut self, from: Type, to: Type) {
        use Instruction as I;
        if from == to {
            return;
        }
        let from_vt = from.val_type();
        let to_vt = to.val_type();
        if from.is_int() && to.is_int() {
            match (from_vt, to_vt) {
                (ValType::I64, ValType::I32) => self.emit(I::I32WrapI64),
                (ValType::I32, ValType::I64) => self.emit(if from.is_signed() {
                    I::I64ExtendI32S
                } else {
                    I::I64ExtendI32U
                }),
                _ => {}
            }
            self.normalize(to);
        } else if from.is_int() {
            let signed = from.is_signed();
            self.emit(match (from_vt, to, signed) {
                (ValType::I32, Type::F32, true) => I::F32ConvertI32S,
                (ValType::I32, Type::F32, false) => I::F32ConvertI32U,
                (ValType::I32, _, true) => I::F64ConvertI32S,
                (ValType::I32, _, false) => I::F64ConvertI32U,
                (_, Type::F32, true) => I::F32ConvertI64S,
                (_, Type::F32, false) => I::F32ConvertI64U,
                (_, _, true) => I::F64ConvertI64S,
                (_, _, false) => I::F64ConvertI64U,
            });
        } else if to.is_int() {
            let f32_src = from == Type::F32;
            if to.is_small_int() {
                // Saturate to i32, then clamp to the range of the type.
                self.emit(if f32_src {
                    I::I32TruncSatF32S
                } else {
                    I::I32TruncSatF64S
                });
                let (min, max) = to.int_range();
                self.clamp(min as i32, max as i32);
            } else {
                let signed = to.is_signed();
                self.emit(match (to_vt, f32_src, signed) {
                    (ValType::I32, true, true) => I::I32TruncSatF32S,
                    (ValType::I32, true, false) => I::I32TruncSatF32U,
                    (ValType::I32, false, true) => I::I32TruncSatF64S,
                    (ValType::I32, false, false) => I::I32TruncSatF64U,
                    (_, true, true) => I::I64TruncSatF32S,
                    (_, true, false) => I::I64TruncSatF32U,
                    (_, false, true) => I::I64TruncSatF64S,
                    (_, false, false) => I::I64TruncSatF64U,
                });
            }
        } else if to == Type::F64 {
            self.emit(I::F64PromoteF32);
        } else {
            self.emit(I::F32DemoteF64);
        }
    }

    /// Clamps the `i32` value on top of the stack to `[min, max]`.
    fn clamp(&mut self, min: i32, max: i32) {
        use Instruction as I;
        let t = self.scratch();
        self.emit(I::LocalSet(t));
        // t = t > max ? max : t
        self.emit(I::I32Const(max));
        self.emit(I::LocalGet(t));
        self.emit(I::LocalGet(t));
        self.emit(I::I32Const(max));
        self.emit(I::I32GtS);
        self.emit(I::Select);
        self.emit(I::LocalSet(t));
        // t < min ? min : t
        self.emit(I::I32Const(min));
        self.emit(I::LocalGet(t));
        self.emit(I::LocalGet(t));
        self.emit(I::I32Const(min));
        self.emit(I::I32LtS);
        self.emit(I::Select);
    }
}

/// Wasm instruction of a binary operator, chosen from the operand type (§ 12.3).
fn binary_instr(op: BinOp, ty: Type) -> Instruction<'static> {
    use Instruction as I;
    let signed = ty.is_signed();
    match ty.val_type() {
        ValType::I32 => match op {
            BinOp::Add => I::I32Add,
            BinOp::Sub => I::I32Sub,
            BinOp::Mul => I::I32Mul,
            BinOp::Div if signed => I::I32DivS,
            BinOp::Div => I::I32DivU,
            BinOp::Rem if signed => I::I32RemS,
            BinOp::Rem => I::I32RemU,
            BinOp::BitAnd => I::I32And,
            BinOp::BitOr => I::I32Or,
            BinOp::BitXor => I::I32Xor,
            BinOp::Shl => I::I32Shl,
            BinOp::Shr if signed => I::I32ShrS,
            BinOp::Shr => I::I32ShrU,
            BinOp::Eq => I::I32Eq,
            BinOp::Ne => I::I32Ne,
            BinOp::Lt if signed => I::I32LtS,
            BinOp::Lt => I::I32LtU,
            BinOp::Le if signed => I::I32LeS,
            BinOp::Le => I::I32LeU,
            BinOp::Gt if signed => I::I32GtS,
            BinOp::Gt => I::I32GtU,
            BinOp::Ge if signed => I::I32GeS,
            BinOp::Ge => I::I32GeU,
            BinOp::And | BinOp::Or => unreachable!(),
        },
        ValType::I64 => match op {
            BinOp::Add => I::I64Add,
            BinOp::Sub => I::I64Sub,
            BinOp::Mul => I::I64Mul,
            BinOp::Div if signed => I::I64DivS,
            BinOp::Div => I::I64DivU,
            BinOp::Rem if signed => I::I64RemS,
            BinOp::Rem => I::I64RemU,
            BinOp::BitAnd => I::I64And,
            BinOp::BitOr => I::I64Or,
            BinOp::BitXor => I::I64Xor,
            BinOp::Shl => I::I64Shl,
            BinOp::Shr if signed => I::I64ShrS,
            BinOp::Shr => I::I64ShrU,
            BinOp::Eq => I::I64Eq,
            BinOp::Ne => I::I64Ne,
            BinOp::Lt if signed => I::I64LtS,
            BinOp::Lt => I::I64LtU,
            BinOp::Le if signed => I::I64LeS,
            BinOp::Le => I::I64LeU,
            BinOp::Gt if signed => I::I64GtS,
            BinOp::Gt => I::I64GtU,
            BinOp::Ge if signed => I::I64GeS,
            BinOp::Ge => I::I64GeU,
            BinOp::And | BinOp::Or => unreachable!(),
        },
        ValType::F32 => match op {
            BinOp::Add => I::F32Add,
            BinOp::Sub => I::F32Sub,
            BinOp::Mul => I::F32Mul,
            BinOp::Div => I::F32Div,
            BinOp::Eq => I::F32Eq,
            BinOp::Ne => I::F32Ne,
            BinOp::Lt => I::F32Lt,
            BinOp::Le => I::F32Le,
            BinOp::Gt => I::F32Gt,
            BinOp::Ge => I::F32Ge,
            _ => unreachable!("operator checked by semantic analysis"),
        },
        ValType::F64 => match op {
            BinOp::Add => I::F64Add,
            BinOp::Sub => I::F64Sub,
            BinOp::Mul => I::F64Mul,
            BinOp::Div => I::F64Div,
            BinOp::Eq => I::F64Eq,
            BinOp::Ne => I::F64Ne,
            BinOp::Lt => I::F64Lt,
            BinOp::Le => I::F64Le,
            BinOp::Gt => I::F64Gt,
            BinOp::Ge => I::F64Ge,
            _ => unreachable!("operator checked by semantic analysis"),
        },
        _ => unreachable!(),
    }
}
