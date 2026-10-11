//! Generation of the core WebAssembly module of a world (§ 12, § v2.6).

use std::borrow::Cow;
use std::collections::HashMap;

use wasm_encoder::{
    ArrayType, BlockType, CodeSection, CompositeInnerType, CompositeType, ConstExpr,
    DataCountSection, DataSection, ExportKind, ExportSection, FieldType, Function, FunctionSection,
    GlobalSection, GlobalType, HeapType, Ieee32, Ieee64, Instruction, Module, RefType, StorageType,
    StructType, SubType, TypeSection, ValType,
};

use crate::ast::{BinOp, UnaryOp};
use crate::diag::{Code, Diagnostic, Result};
use crate::hir::{
    Builtin, Const, Expr, ExprKind, FuncDef, FuncId, GlobalId, MatchArm, Place, Program, Stmt,
};
use crate::types::{DeclKind, Prim, Type, TypeKind, Types};
use crate::wit::kebab;

mod helpers;

use helpers::Helper;

/// String literals longer than this are built from a data segment rather than with
/// `array.new_fixed` (§ v2.6.4).
const MAX_FIXED: usize = 1000;

/// Produces the core module of world `world`: its exported functions, the functions they
/// call transitively, the globals they use and the helpers they need.
///
/// `interface_names` gives the canonical WIT name of each exported interface.
pub fn generate(
    program: &Program,
    world: usize,
    interface_names: &[Option<String>],
) -> Result<Vec<u8>> {
    let world = &program.worlds[world];
    let types = &program.types;

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

    // Values that cross the boundary without an adapter: scalars (§ v2.7).
    for &iface in &world.exports {
        for &f in &program.interfaces[iface].funcs {
            let func = &program.funcs[f];
            let mut crossing = func.params.iter().map(|(_, t)| *t).chain(func.ret);
            if let Some(ty) = crossing.find(|&t| !types.is_scalar(t)) {
                return Err(Diagnostic::without_span(
                    Code::Unsupported,
                    format!(
                        "exported function `{}::{}` takes or returns a `{}`, which cannot cross the component boundary yet",
                        program.interfaces[iface].name,
                        func.name,
                        types.display(ty)
                    ),
                ));
            }
        }
    }

    let func_index = index_map(&used_funcs);
    let mut cx = ModuleCx::new(types, func_index);

    // Shared immutable values (§ v2.6.4, § v2.6.2): string literals and cases without
    // payload. Their globals come first, so that the initializers of the program's
    // globals can read them.
    for (id, func) in program.funcs.iter().enumerate() {
        if used_funcs[id] {
            visit_stmts(&func.body, &mut |e| cx.collect_shared(e, false));
        }
    }
    for (id, global) in program.globals.iter().enumerate() {
        if used_globals[id] {
            visit_expr(&global.init, &mut |e| cx.collect_shared(e, true));
        }
    }
    let mut next = cx.shared_globals.len() as u32;
    cx.global_index = used_globals
        .iter()
        .map(|&u| {
            u.then(|| {
                next += 1;
                next - 1
            })
        })
        .collect();

    let mut bodies = Vec::new();
    let mut signatures = Vec::new();
    for (id, func) in program.funcs.iter().enumerate() {
        if !used_funcs[id] {
            continue;
        }
        let params: Vec<ValType> = func.params.iter().map(|(_, t)| cx.val(*t)).collect();
        let results: Vec<ValType> = func.ret.iter().map(|t| cx.val(*t)).collect();
        signatures.push(cx.signature(params, results));
        bodies.push(FuncGen::run(&mut cx, func));
    }
    // Helpers can request other helpers.
    let mut done = 0;
    while done < cx.helpers.len() {
        let helper = cx.helpers[done];
        let (params, results, body) = helper.generate(&mut cx);
        signatures.push(cx.signature(params, results));
        bodies.push(body);
        done += 1;
    }

    let mut globals = GlobalSection::new();
    for (val_type, init) in std::mem::take(&mut cx.shared_globals) {
        globals.global(
            GlobalType {
                val_type,
                mutable: false,
                shared: false,
            },
            &ConstExpr::extended(init),
        );
    }
    for (id, global) in program.globals.iter().enumerate() {
        if !used_globals[id] {
            continue;
        }
        let val_type = cx.val(global.ty);
        let init = FuncGen::constant(&mut cx, &global.init);
        globals.global(
            GlobalType {
                val_type,
                mutable: global.mutable,
                shared: false,
            },
            &ConstExpr::extended(init),
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
            exports.export(&name, ExportKind::Func, cx.func_index[f].unwrap());
        }
    }

    // Types: the GC types in one recursion group, then the function types (§ v2.6.5).
    let mut type_section = TypeSection::new();
    let gc_types = cx.rec.len() as u32;
    if gc_types > 0 {
        type_section.ty().rec(cx.rec.iter().cloned());
    }
    for (params, results) in &cx.signatures {
        type_section
            .ty()
            .function(params.iter().copied(), results.iter().copied());
    }
    let mut functions = FunctionSection::new();
    for sig in signatures {
        functions.function(gc_types + sig);
    }
    let mut code = CodeSection::new();
    for body in &bodies {
        code.function(body);
    }

    let mut module = Module::new();
    module.section(&type_section);
    module.section(&functions);
    module.section(&globals);
    module.section(&exports);
    if !cx.data.is_empty() {
        module.section(&DataCountSection {
            count: cx.data.len() as u32,
        });
    }
    module.section(&code);
    if !cx.data.is_empty() {
        let mut data = DataSection::new();
        for bytes in &cx.data {
            data.passive(bytes.iter().copied());
        }
        module.section(&data);
    }
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
            Stmt::Match { arms, else_, .. } => {
                for arm in arms {
                    mark_assigned_globals(&arm.body, used);
                }
                if let Some(else_) = else_ {
                    mark_assigned_globals(else_, used);
                }
            }
            _ => {}
        }
    }
}

/// Visits every expression of a list of statements.
fn visit_stmts(stmts: &[Stmt], f: &mut impl FnMut(&Expr)) {
    for stmt in stmts {
        match stmt {
            Stmt::Local { init, .. } => init.iter().for_each(|e| visit_expr(e, f)),
            Stmt::Assign { place, value } => {
                match place {
                    Place::Local(_) | Place::Global(_) => {}
                    Place::Field { record, .. } => visit_expr(record, f),
                    Place::Index { list, index } => {
                        visit_expr(list, f);
                        visit_expr(index, f);
                    }
                }
                visit_expr(value, f);
            }
            Stmt::Match {
                scrutinee,
                arms,
                else_,
            } => {
                visit_expr(scrutinee, f);
                for arm in arms {
                    visit_stmts(&arm.body, f);
                }
                if let Some(else_) = else_ {
                    visit_stmts(else_, f);
                }
            }
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
        ExprKind::Call { args, .. }
        | ExprKind::Builtin { args, .. }
        | ExprKind::Record(args)
        | ExprKind::List(args)
        | ExprKind::Tuple(args) => args.iter().for_each(|a| visit_expr(a, f)),
        ExprKind::Unary { expr, .. }
        | ExprKind::Convert(expr)
        | ExprKind::Some(expr)
        | ExprKind::Field { expr, .. }
        | ExprKind::Elem { expr, .. }
        | ExprKind::Flag { expr, .. } => visit_expr(expr, f),
        ExprKind::Binary { lhs: a, rhs: b, .. } | ExprKind::Index { expr: a, index: b } => {
            visit_expr(a, f);
            visit_expr(b, f);
        }
        ExprKind::Case { payload, .. } => payload.iter().for_each(|p| visit_expr(p, f)),
        ExprKind::Const(_)
        | ExprKind::Local(_)
        | ExprKind::Global(_)
        | ExprKind::Str(_)
        | ExprKind::None => {}
    }
}

fn ref_type(index: u32, nullable: bool) -> ValType {
    ValType::Ref(RefType {
        nullable,
        heap_type: HeapType::Concrete(index),
    })
}

fn composite(inner: CompositeInnerType) -> CompositeType {
    CompositeType {
        inner,
        shared: false,
        descriptor: None,
        describes: None,
    }
}

fn struct_type(fields: Vec<FieldType>, is_final: bool, supertype: Option<u32>) -> SubType {
    SubType {
        is_final,
        supertype_idxs: supertype.into_iter().collect(),
        composite_type: composite(CompositeInnerType::Struct(StructType {
            fields: fields.into(),
        })),
    }
}

fn field(val: ValType, mutable: bool) -> FieldType {
    FieldType {
        element_type: StorageType::Val(val),
        mutable,
    }
}

/// State shared by the functions of the module: types, helpers, shared globals.
struct ModuleCx<'a> {
    types: &'a Types,
    /// The GC types, in one recursion group (§ v2.6.1).
    rec: Vec<SubType>,
    /// Struct or array type of each heap type other than an option.
    heap: HashMap<Type, u32>,
    arrays: HashMap<StorageType, u32>,
    /// Immutable structs, shared by structure: tuples and boxes.
    structs: HashMap<Vec<FieldType>, u32>,
    /// Struct type of each case with a payload of a variant or result type.
    cases: HashMap<(Type, u32), u32>,
    /// Function types, after the recursion group.
    signatures: Vec<(Vec<ValType>, Vec<ValType>)>,

    func_index: Vec<Option<u32>>,
    helpers: Vec<Helper>,
    helper_index: HashMap<Helper, u32>,

    /// Globals of the string literals and of the cases without payload: type and
    /// initializer.
    shared_globals: Vec<(ValType, Vec<Instruction<'static>>)>,
    strings: HashMap<String, u32>,
    shared_cases: HashMap<(Type, u32), u32>,
    global_index: Vec<Option<u32>>,
    /// Passive data segments of the long string literals.
    data: Vec<Vec<u8>>,
    data_index: HashMap<String, u32>,
}

impl<'a> ModuleCx<'a> {
    fn new(types: &'a Types, func_index: Vec<Option<u32>>) -> Self {
        ModuleCx {
            types,
            rec: Vec::new(),
            heap: HashMap::new(),
            arrays: HashMap::new(),
            structs: HashMap::new(),
            cases: HashMap::new(),
            signatures: Vec::new(),
            func_index,
            helpers: Vec::new(),
            helper_index: HashMap::new(),
            shared_globals: Vec::new(),
            strings: HashMap::new(),
            shared_cases: HashMap::new(),
            global_index: Vec::new(),
            data: Vec::new(),
            data_index: HashMap::new(),
        }
    }

    /// Index of a function type, without the offset of the GC types.
    fn signature(&mut self, params: Vec<ValType>, results: Vec<ValType>) -> u32 {
        let sig = (params, results);
        match self.signatures.iter().position(|s| *s == sig) {
            Some(i) => i as u32,
            None => {
                self.signatures.push(sig);
                self.signatures.len() as u32 - 1
            }
        }
    }

    // ---- Types ----

    /// Wasm value type of a WaSL type (§ v2.6.1).
    fn val(&mut self, ty: Type) -> ValType {
        let types = self.types;
        match types.kind(ty) {
            TypeKind::Prim(p) => p.val_type(),
            _ if types.is_scalar(ty) => ValType::I32,
            TypeKind::Option(inner) => {
                let index = self.option_heap(*inner);
                ref_type(index, true)
            }
            _ => ref_type(self.heap(ty), false),
        }
    }

    /// Whether `option<inner>` boxes its payload (§ v2.6.3).
    fn boxed(&self, inner: Type) -> bool {
        self.types.is_scalar(inner) || matches!(self.types.kind(inner), TypeKind::Option(_))
    }

    /// The type `X` of the `(ref null X)` of `option<inner>`.
    fn option_heap(&mut self, inner: Type) -> u32 {
        if self.boxed(inner) {
            let val = self.val(inner);
            self.immutable_struct(vec![field(val, false)])
        } else {
            self.heap(inner)
        }
    }

    /// Storage type of the elements of a list (§ v2.6.1).
    fn storage(&mut self, elem: Type) -> StorageType {
        match self.types.kind(elem) {
            TypeKind::Prim(Prim::Bool | Prim::S8 | Prim::U8) => StorageType::I8,
            TypeKind::Prim(Prim::S16 | Prim::U16) => StorageType::I16,
            _ => StorageType::Val(self.val(elem)),
        }
    }

    fn array(&mut self, storage: StorageType) -> u32 {
        if let Some(&i) = self.arrays.get(&storage) {
            return i;
        }
        let i = self.rec.len() as u32;
        self.rec.push(SubType {
            is_final: true,
            supertype_idxs: Vec::new(),
            composite_type: composite(CompositeInnerType::Array(ArrayType(FieldType {
                element_type: storage,
                mutable: true,
            }))),
        });
        self.arrays.insert(storage, i);
        i
    }

    fn immutable_struct(&mut self, fields: Vec<FieldType>) -> u32 {
        if let Some(&i) = self.structs.get(&fields) {
            return i;
        }
        let i = self.rec.len() as u32;
        self.rec.push(struct_type(fields.clone(), true, None));
        self.structs.insert(fields, i);
        i
    }

    /// Reserves the slot of a declared type in the recursion group, filled once its
    /// components are known: they can refer to it.
    fn reserve(&mut self, ty: Type) -> u32 {
        let i = self.rec.len() as u32;
        self.rec.push(struct_type(Vec::new(), true, None));
        self.heap.insert(ty, i);
        i
    }

    /// The struct or array type of a heap type other than an option.
    fn heap(&mut self, ty: Type) -> u32 {
        if let Some(&i) = self.heap.get(&ty) {
            return i;
        }
        let types = self.types;
        let index = match types.kind(ty) {
            TypeKind::String => self.array(StorageType::I8),
            TypeKind::List { elem, .. } => {
                let storage = self.storage(*elem);
                self.array(storage)
            }
            TypeKind::Tuple(elems) => {
                let fields = elems.iter().map(|&t| field(self.val(t), false)).collect();
                self.immutable_struct(fields)
            }
            TypeKind::Result { ok, err } => return self.variant(ty, &[*ok, *err]),
            TypeKind::Decl(d) => match &types.decls[*d].kind {
                DeclKind::Record(fields) => {
                    let i = self.reserve(ty);
                    let fields = fields
                        .iter()
                        .map(|f| field(self.val(f.ty), f.mutable))
                        .collect();
                    self.rec[i as usize] = struct_type(fields, true, None);
                    return i;
                }
                DeclKind::Variant(cases) => {
                    let payloads: Vec<Option<Type>> = cases.iter().map(|c| c.payload).collect();
                    return self.variant(ty, &payloads);
                }
                DeclKind::Enum(_) | DeclKind::Flags(_) => unreachable!("scalar type"),
            },
            TypeKind::Prim(_) | TypeKind::Char | TypeKind::Option(_) => {
                unreachable!("not a heap type")
            }
        };
        self.heap.insert(ty, index);
        index
    }

    /// The types of a variant or result (§ v2.6.2): the tag struct, then one subtype per
    /// case with a payload.
    fn variant(&mut self, ty: Type, payloads: &[Option<Type>]) -> u32 {
        let base = self.reserve(ty);
        self.rec[base as usize] = struct_type(vec![field(ValType::I32, false)], false, None);
        for (case, payload) in payloads.iter().enumerate() {
            if let Some(p) = payload {
                let val = self.val(*p);
                let i = self.rec.len() as u32;
                self.rec.push(struct_type(
                    vec![field(ValType::I32, false), field(val, false)],
                    true,
                    Some(base),
                ));
                self.cases.insert((ty, case as u32), i);
            }
        }
        base
    }

    /// The struct type of a case with a payload.
    fn case_type(&mut self, ty: Type, case: u32) -> u32 {
        self.heap(ty);
        self.cases[&(ty, case)]
    }

    // ---- Shared values ----

    /// Creates the globals of the string literals and cases without payload of `e`.
    /// `constant`: `e` is part of a global's initializer, which cannot use a data
    /// segment.
    fn collect_shared(&mut self, e: &Expr, constant: bool) {
        match &e.kind {
            ExprKind::Str(text) if constant || text.len() <= MAX_FIXED => {
                if !self.strings.contains_key(text) {
                    let bytes = self.heap(Type::STRING);
                    let mut init: Vec<Instruction<'static>> = text
                        .bytes()
                        .map(|b| Instruction::I32Const(b as i32))
                        .collect();
                    init.push(Instruction::ArrayNewFixed {
                        array_type_index: bytes,
                        array_size: text.len() as u32,
                    });
                    let g = self.shared_global(ref_type(bytes, false), init);
                    self.strings.insert(text.clone(), g);
                }
            }
            ExprKind::Str(text) => {
                if !self.data_index.contains_key(text) {
                    self.data_index.insert(text.clone(), self.data.len() as u32);
                    self.data.push(text.as_bytes().to_vec());
                }
            }
            ExprKind::Case {
                case,
                payload: None,
            } => {
                let ty = e.value_ty();
                if !self.shared_cases.contains_key(&(ty, *case)) {
                    let base = self.heap(ty);
                    let init = vec![
                        Instruction::I32Const(*case as i32),
                        Instruction::StructNew(base),
                    ];
                    let g = self.shared_global(ref_type(base, false), init);
                    self.shared_cases.insert((ty, *case), g);
                }
            }
            _ => {}
        }
    }

    fn shared_global(&mut self, ty: ValType, init: Vec<Instruction<'static>>) -> u32 {
        self.shared_globals.push((ty, init));
        self.shared_globals.len() as u32 - 1
    }

    /// Index of a helper function; its body is generated after the program's functions.
    fn helper(&mut self, helper: Helper) -> u32 {
        if let Some(&i) = self.helper_index.get(&helper) {
            return i;
        }
        let users = self.func_index.iter().flatten().count() as u32;
        let i = users + self.helpers.len() as u32;
        self.helpers.push(helper);
        self.helper_index.insert(helper, i);
        i
    }
}

/// Wasm block opened by a statement, used to compute `br` targets.
#[derive(Clone, Copy, PartialEq)]
enum Frame {
    /// `block` around a `while`: target of `break`.
    LoopExit,
    /// `loop` of a `while`: target of `continue`.
    LoopStart,
    /// The outer `block` of a `match`: where each arm ends.
    MatchEnd,
    /// Any other block.
    Other,
}

/// How a `match` reads the case of its scrutinee.
#[derive(Clone, Copy)]
enum Scrutinee {
    Enum,
    /// A variant or a result, with its tag struct.
    Variant,
    /// An option, with its box type if the payload is boxed.
    Option(Option<u32>),
}

struct FuncGen<'g, 'a> {
    cx: &'g mut ModuleCx<'a>,
    local_types: &'a [Type],
    instrs: Vec<Instruction<'static>>,
    frames: Vec<Frame>,
    /// Index of the first scratch local (after the variables of the function).
    first_scratch: u32,
    scratch: Vec<ValType>,
    /// Generating the initializer of a global: string literals always come from their
    /// global.
    constant: bool,
}

impl<'g, 'a> FuncGen<'g, 'a> {
    fn run(cx: &'g mut ModuleCx<'a>, func: &'a FuncDef) -> Function {
        let mut cg = FuncGen {
            cx,
            local_types: &func.locals,
            instrs: Vec::new(),
            frames: Vec::new(),
            first_scratch: func.locals.len() as u32,
            scratch: Vec::new(),
            constant: false,
        };
        cg.stmts(&func.body);
        if func.ret.is_some() {
            // Every path ends with `return` (checked by semantic analysis);
            // the end of the body is unreachable.
            cg.emit(Instruction::Unreachable);
        }
        cg.emit(Instruction::End);

        let mut locals: Vec<ValType> = func.locals[func.params.len()..]
            .iter()
            .map(|t| cg.cx.val(*t))
            .collect();
        locals.extend(&cg.scratch);
        let mut function = Function::new_with_locals_types(locals);
        for instr in &cg.instrs {
            function.instruction(instr);
        }
        function
    }

    /// Instructions of the constant initializer of a global.
    fn constant(cx: &'g mut ModuleCx<'a>, init: &Expr) -> Vec<Instruction<'static>> {
        let mut cg = FuncGen {
            cx,
            local_types: &[],
            instrs: Vec::new(),
            frames: Vec::new(),
            first_scratch: 0,
            scratch: Vec::new(),
            constant: true,
        };
        cg.expr(init);
        cg.instrs
    }

    fn emit(&mut self, instr: Instruction<'static>) {
        self.instrs.push(instr);
    }

    /// Reserves a scratch local. Reference scratch locals are nullable, so that Wasm
    /// does not track their initialization.
    fn scratch(&mut self, ty: ValType) -> u32 {
        let ty = match ty {
            ValType::Ref(r) => ValType::Ref(RefType {
                nullable: true,
                ..r
            }),
            t => t,
        };
        self.scratch.push(ty);
        self.first_scratch + self.scratch.len() as u32 - 1
    }

    fn types(&self) -> &'a Types {
        self.cx.types
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
                        let ty = self.cx.val(self.local_types[*local as usize]);
                        self.zero(ty);
                    }
                }
                self.emit(Instruction::LocalSet(*local));
            }
            Stmt::Assign { place, value } => match place {
                Place::Local(l) => {
                    self.expr(value);
                    self.emit(Instruction::LocalSet(*l));
                }
                Place::Global(g) => {
                    self.expr(value);
                    self.emit(Instruction::GlobalSet(self.global(*g)));
                }
                Place::Field { record, field } => {
                    let index = self.cx.heap(record.value_ty());
                    self.expr(record);
                    self.expr(value);
                    self.emit(Instruction::StructSet {
                        struct_type_index: index,
                        field_index: *field,
                    });
                }
                Place::Index { list, index } => {
                    let array = self.cx.heap(list.value_ty());
                    self.expr(list);
                    self.expr(index);
                    self.expr(value);
                    self.emit(Instruction::ArraySet(array));
                }
            },
            Stmt::If { cond, then, else_ } => {
                self.expr(cond);
                self.emit(Instruction::If(BlockType::Empty));
                self.frames.push(Frame::Other);
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
            Stmt::Match {
                scrutinee,
                arms,
                else_,
            } => self.match_(scrutinee, arms, else_.as_deref()),
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

    /// `match` (§ v2.6.2): one block per arm, plus one for `else`, around a `br_table`
    /// on the case of the scrutinee. Arm `i` runs after the end of the `i`-th
    /// innermost block, then leaves the outer block.
    fn match_(&mut self, scrutinee: &Expr, arms: &[MatchArm], else_: Option<&[Stmt]>) {
        let types = self.types();
        let ty = scrutinee.value_ty();
        let (shape, cases) = match types.kind(ty) {
            TypeKind::Option(inner) => {
                let boxed = self.cx.boxed(*inner).then(|| self.cx.option_heap(*inner));
                (Scrutinee::Option(boxed), 2)
            }
            TypeKind::Result { .. } => (Scrutinee::Variant, 2),
            TypeKind::Decl(d) => match &types.decls[*d].kind {
                DeclKind::Enum(cases) => (Scrutinee::Enum, cases.len()),
                DeclKind::Variant(cases) => (Scrutinee::Variant, cases.len()),
                _ => unreachable!("checked"),
            },
            _ => unreachable!("checked"),
        };
        self.expr(scrutinee);
        let val = self.cx.val(ty);
        let s = self.scratch(val);
        self.emit(Instruction::LocalSet(s));

        self.emit(Instruction::Block(BlockType::Empty));
        self.frames.push(Frame::MatchEnd);
        if else_.is_some() {
            self.emit(Instruction::Block(BlockType::Empty));
            self.frames.push(Frame::Other);
        }
        for _ in arms {
            self.emit(Instruction::Block(BlockType::Empty));
            self.frames.push(Frame::Other);
        }

        self.emit(Instruction::LocalGet(s));
        match shape {
            Scrutinee::Enum => {}
            Scrutinee::Variant => {
                let base = self.cx.heap(ty);
                self.emit(Instruction::StructGet {
                    struct_type_index: base,
                    field_index: 0,
                });
            }
            Scrutinee::Option(_) => {
                // none = 0, some = 1
                self.emit(Instruction::RefIsNull);
                self.emit(Instruction::I32Eqz);
            }
        }
        let else_depth = arms.len() as u32;
        let targets: Vec<u32> = (0..cases as u32)
            .map(|c| match arms.iter().position(|a| a.case == c) {
                Some(i) => i as u32,
                None => else_depth,
            })
            .collect();
        let default = *targets.last().expect("a type has at least one case");
        self.emit(Instruction::BrTable(Cow::Owned(targets), default));

        for arm in arms {
            self.emit(Instruction::End);
            self.frames.pop();
            if let Some(binding) = arm.binding {
                self.emit(Instruction::LocalGet(s));
                match shape {
                    Scrutinee::Enum => unreachable!("enum cases have no payload"),
                    Scrutinee::Variant => {
                        let case = self.cx.case_type(ty, arm.case);
                        self.emit(Instruction::RefCastNonNull(HeapType::Concrete(case)));
                        self.emit(Instruction::StructGet {
                            struct_type_index: case,
                            field_index: 1,
                        });
                    }
                    Scrutinee::Option(Some(boxed)) => self.emit(Instruction::StructGet {
                        struct_type_index: boxed,
                        field_index: 0,
                    }),
                    Scrutinee::Option(None) => self.emit(Instruction::RefAsNonNull),
                }
                self.emit(Instruction::LocalSet(binding));
            }
            self.stmts(&arm.body);
            self.branch(Frame::MatchEnd);
        }
        if let Some(else_) = else_ {
            self.emit(Instruction::End);
            self.frames.pop();
            self.stmts(else_);
        }
        self.emit(Instruction::End);
        self.frames.pop();
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
        self.cx.global_index[g].expect("used global")
    }

    fn zero(&mut self, ty: ValType) {
        self.emit(match ty {
            ValType::I32 => Instruction::I32Const(0),
            ValType::I64 => Instruction::I64Const(0),
            ValType::F32 => Instruction::F32Const(Ieee32::from(0.0f32)),
            ValType::F64 => Instruction::F64Const(Ieee64::from(0.0f64)),
            _ => unreachable!("locals of heap types are initialized"),
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
                let index = self.cx.func_index[*func].expect("used function");
                self.emit(Instruction::Call(index));
            }
            ExprKind::Unary { op, expr } => self.unary(*op, expr),
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs),
            ExprKind::Convert(inner) => {
                self.expr(inner);
                let from = inner.value_ty();
                // `u32(c)` on a char is its code point.
                if from != Type::CHAR {
                    self.convert(from, e.value_ty());
                }
            }
            ExprKind::Str(text) => match self.cx.strings.get(text) {
                Some(&g) if self.constant || text.len() <= MAX_FIXED => {
                    self.emit(Instruction::GlobalGet(g))
                }
                _ => {
                    let bytes = self.cx.heap(Type::STRING);
                    self.emit(Instruction::I32Const(0));
                    self.emit(Instruction::I32Const(text.len() as i32));
                    self.emit(Instruction::ArrayNewData {
                        array_type_index: bytes,
                        array_data_index: self.cx.data_index[text],
                    });
                }
            },
            ExprKind::Record(values) | ExprKind::Tuple(values) => {
                for v in values {
                    self.expr(v);
                }
                let index = self.cx.heap(e.value_ty());
                self.emit(Instruction::StructNew(index));
            }
            ExprKind::List(values) => {
                for v in values {
                    self.expr(v);
                }
                let index = self.cx.heap(e.value_ty());
                self.emit(Instruction::ArrayNewFixed {
                    array_type_index: index,
                    array_size: values.len() as u32,
                });
            }
            ExprKind::Case { case, payload } => {
                let ty = e.value_ty();
                match payload {
                    Some(p) => {
                        let index = self.cx.case_type(ty, *case);
                        self.emit(Instruction::I32Const(*case as i32));
                        self.expr(p);
                        self.emit(Instruction::StructNew(index));
                    }
                    None => {
                        let g = self.cx.shared_cases[&(ty, *case)];
                        self.emit(Instruction::GlobalGet(g));
                    }
                }
            }
            ExprKind::Some(value) => {
                self.expr(value);
                let inner = value.value_ty();
                if self.cx.boxed(inner) {
                    let index = self.cx.option_heap(inner);
                    self.emit(Instruction::StructNew(index));
                }
            }
            ExprKind::None => {
                let TypeKind::Option(inner) = self.types().kind(e.value_ty()) else {
                    unreachable!("option::none is an option")
                };
                let index = self.cx.option_heap(*inner);
                self.emit(Instruction::RefNull(HeapType::Concrete(index)));
            }
            ExprKind::Field { expr, field } => {
                let index = self.cx.heap(expr.value_ty());
                self.expr(expr);
                self.emit(Instruction::StructGet {
                    struct_type_index: index,
                    field_index: *field,
                });
            }
            ExprKind::Elem { expr, index } => {
                let tuple = self.cx.heap(expr.value_ty());
                self.expr(expr);
                self.emit(Instruction::StructGet {
                    struct_type_index: tuple,
                    field_index: *index,
                });
            }
            ExprKind::Flag { expr, flag } => {
                self.expr(expr);
                self.emit(Instruction::I32Const((1u32 << flag) as i32));
                self.emit(Instruction::I32And);
                self.emit(Instruction::I32Const(0));
                self.emit(Instruction::I32Ne);
            }
            ExprKind::Index { expr, index } => {
                let list = expr.value_ty();
                self.expr(expr);
                self.expr(index);
                self.array_get(list);
            }
            ExprKind::Builtin { func, args } => self.builtin(*func, args, e.value_ty()),
        }
    }

    /// Reads an element of a list: the list and the index are on the stack.
    fn array_get(&mut self, list: Type) {
        let array = self.cx.heap(list);
        let TypeKind::List { elem, .. } = self.types().kind(list) else {
            unreachable!("indexing a list")
        };
        self.emit(match *elem {
            Type::S8 | Type::S16 => Instruction::ArrayGetS(array),
            Type::BOOL | Type::U8 | Type::U16 => Instruction::ArrayGetU(array),
            _ => Instruction::ArrayGet(array),
        });
    }

    fn builtin(&mut self, func: Builtin, args: &[Expr], ty: Type) {
        if func == Builtin::ListFilled {
            // The count is evaluated first, but `array.new` takes it last.
            self.expr(&args[0]);
            let n = self.scratch(ValType::I32);
            self.emit(Instruction::LocalSet(n));
            self.expr(&args[1]);
            self.emit(Instruction::LocalGet(n));
            let array = self.cx.heap(ty);
            self.emit(Instruction::ArrayNew(array));
            return;
        }
        for arg in args {
            self.expr(arg);
        }
        let helper = match func {
            Builtin::StringLen | Builtin::ListLen => {
                self.emit(Instruction::ArrayLen);
                return;
            }
            // A string never changes: its bytes are the same array (§ v2.4.11).
            Builtin::StringBytes => return,
            Builtin::StringFromBytes => Helper::FromBytes,
            Builtin::StringChars => Helper::Chars,
            Builtin::StringFromChars => Helper::FromChars,
            Builtin::CharFromU32 => Helper::CharFromU32,
            Builtin::ListCopy | Builtin::ListFreeze => Helper::Copy(self.cx.heap(ty)),
            Builtin::ListFilled => unreachable!(),
        };
        let index = self.cx.helper(helper);
        self.emit(Instruction::Call(index));
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
        if ty == Type::STRING {
            self.expr(lhs);
            self.expr(rhs);
            let helper = if op == BinOp::Add {
                Helper::Concat
            } else {
                Helper::StrEq
            };
            let index = self.cx.helper(helper);
            self.emit(Instruction::Call(index));
            if op == BinOp::Ne {
                self.emit(Instruction::I32Eqz);
            }
            return;
        }
        // Chars, enums and flags compare and combine as unsigned 32-bit integers.
        let ty = if ty.as_prim().is_some() {
            ty
        } else {
            Type::U32
        };

        if op == BinOp::Div && matches!(ty, Type::S8 | Type::S16) {
            // `i32.div_s` does not overflow for -128 / -1: the trap is emitted explicitly, by
            // dividing i32::MIN by -1 so that it is reported as an integer overflow.
            let (a, b) = (self.scratch(ValType::I32), self.scratch(ValType::I32));
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
        let t = self.scratch(ValType::I32);
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
