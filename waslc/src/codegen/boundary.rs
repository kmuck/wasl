//! The component boundary (§ 13): layout of the Canonical ABI, adapters of the exported
//! functions, lifting and lowering through linear memory, `realloc` and post-return.
//!
//! Values cross the boundary as deep copies. Lifting reads the flat values and the bytes
//! of linear memory and builds new WasmGC objects; lowering writes them back, in memory
//! allocated by the bump allocator of § 13.7.

use wasm_encoder::{BlockType, Function, HeapType, Instruction, MemArg, ValType};

use super::helpers::Helper;
use super::{ModuleCx, ref_type};
use crate::hir::{FuncDef, FuncId};
use crate::types::{DeclId, DeclKind, Prim, Type, TypeKind, Types};

use Instruction as I;

mod ends;

pub(super) use ends::{EndRt, WaitRt};

/// Flat values beyond which the parameters go through memory.
const MAX_FLAT_PARAMS: usize = 16;
/// Flat values beyond which the result goes through memory.
const MAX_FLAT_RESULTS: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Boundary {
    /// `cm32p2_realloc(old_ptr, old_size, align, new_size) -> ptr`
    Realloc,
    /// Post-return of every function whose result is in memory: frees the scratch buffer.
    PostReturn,
    /// The core export of an exported function (§ 13.8).
    Adapter(FuncId),
    /// The core export of an exported async function (§ 13.13): lifts the arguments,
    /// calls the function, gives its result to `task.return`, and returns `EXIT`.
    AsyncAdapter(FuncId),
    /// The callback of the async exports, never called in 0.4 (§ 13.13).
    Callback,
    /// The reverse adapter of an imported function (§ 13.10): lowers the arguments,
    /// calls the import, lifts the result.
    Import(FuncId),
    /// `(ptr) -> T'`: lifts a value stored at `ptr`.
    LiftMem(Type),
    /// `(T', ptr)`: lowers a value to `ptr`.
    LowerMem(Type),
    /// `(ptr, len) -> T'`: lifts a string or a list.
    LiftList(Type),
    /// `(T') -> (ptr, len)`: lowers a string or a list into new memory.
    LowerList(Type),
    /// `(handle) -> r`: takes the object of an `own` handle of an exported resource,
    /// and drops the handle (§ 12.10).
    LiftOwn(DeclId),
    /// `(rep) -> r`: the object of a `borrow` of an exported resource, which the
    /// Component Model passes as its rep.
    LiftBorrow(DeclId),
    /// `(r) -> handle`: stores the object in a free slot of the table and creates an
    /// `own` handle for it.
    LowerOwn(DeclId),
    /// `(rep)`: the destructor of an exported resource, called when the last handle is
    /// dropped: frees the slot.
    Dtor(DeclId),
    /// `(T')`: releases every handle that a value holds (§ 9.9).
    Release(Type),
    /// `(end)`: releases an end of a stream or a future, unless it is `0` or `-1`
    /// (§ 12.12).
    ReleaseEnd(Type),
    /// `(end) -> code`: the wait of a task for the event of an end (§ 12.12).
    Wait,
    /// `() -> tuple<R, W>`: `stream::new()` or `future::new()`, of the tuple type.
    EndNew(Type),
    /// `(tx, xs) -> (count, tx)`: `stream::write`, of the list type.
    StreamWrite(Type),
    /// `(rx, n) -> (option<list<T>>, rx)`: `stream::read`, of the option type.
    StreamRead(Type),
    /// `(tx, v) -> bool`: `future::write`, of the element type.
    FutureWrite(Type),
    /// `(rx) -> T`: `future::read`, of the element type.
    FutureRead(Type),
}

// ---- Layout of the Canonical ABI ----

fn align_to(n: u32, align: u32) -> u32 {
    n.div_ceil(align) * align
}

/// Size in bytes of the discriminant of a type with `cases` cases.
fn discriminant_size(cases: usize) -> u32 {
    match cases {
        0..=256 => 1,
        257..=65536 => 2,
        _ => 4,
    }
}

fn flags_size(flags: usize) -> u32 {
    match flags {
        0..=8 => 1,
        9..=16 => 2,
        _ => 4,
    }
}

/// The payloads of a variant, `option` or `result`, in case order.
fn cases(types: &Types, ty: Type) -> Option<Vec<Option<Type>>> {
    match types.kind(ty) {
        TypeKind::Option(inner) => Some(vec![None, Some(*inner)]),
        TypeKind::Result { ok, err } => Some(vec![*ok, *err]),
        TypeKind::Decl(d) => match &types.decls[*d].kind {
            DeclKind::Variant(cases) => Some(cases.iter().map(|c| c.payload).collect()),
            _ => None,
        },
        _ => None,
    }
}

/// The field types of a record or tuple.
fn fields(types: &Types, ty: Type) -> Option<Vec<Type>> {
    match types.kind(ty) {
        TypeKind::Tuple(elems) => Some(elems.clone()),
        TypeKind::Decl(d) => match &types.decls[*d].kind {
            DeclKind::Record(fields) => Some(fields.iter().map(|f| f.ty).collect()),
            _ => None,
        },
        _ => None,
    }
}

/// Element type of a string or list.
fn element(types: &Types, ty: Type) -> Type {
    match types.kind(ty) {
        TypeKind::String => Type::U8,
        TypeKind::List { elem, .. } => *elem,
        _ => unreachable!("a string or a list"),
    }
}

/// Offsets of a sequence of fields, then the size and alignment of the whole.
fn fields_layout(types: &Types, tys: &[Type]) -> (Vec<u32>, u32, u32) {
    let mut offsets = Vec::new();
    let mut size = 0;
    let mut align = 1;
    for &t in tys {
        let (s, a) = size_align(types, t);
        size = align_to(size, a);
        offsets.push(size);
        size += s;
        align = align.max(a);
    }
    (offsets, align_to(size, align), align)
}

/// Discriminant size, payload offset, size and alignment of a variant.
fn variant_layout(types: &Types, payloads: &[Option<Type>]) -> (u32, u32, u32, u32) {
    let disc = discriminant_size(payloads.len());
    let (mut case_size, mut case_align) = (0, 1);
    for p in payloads.iter().flatten() {
        let (s, a) = size_align(types, *p);
        case_size = case_size.max(s);
        case_align = case_align.max(a);
    }
    let offset = align_to(disc, case_align);
    let align = disc.max(case_align);
    (disc, offset, align_to(offset + case_size, align), align)
}

/// Size and alignment of a type in linear memory.
pub(super) fn size_align(types: &Types, ty: Type) -> (u32, u32) {
    if let Some(payloads) = cases(types, ty) {
        let (_, _, size, align) = variant_layout(types, &payloads);
        return (size, align);
    }
    if let Some(fields) = fields(types, ty) {
        let (_, size, align) = fields_layout(types, &fields);
        return (size, align);
    }
    let size = match types.kind(ty) {
        TypeKind::Prim(p) => match p {
            Prim::Bool | Prim::S8 | Prim::U8 => 1,
            Prim::S16 | Prim::U16 => 2,
            Prim::S32 | Prim::U32 | Prim::F32 => 4,
            Prim::S64 | Prim::U64 | Prim::F64 => 8,
        },
        TypeKind::Char => 4,
        TypeKind::String | TypeKind::List { .. } => return (8, 4),
        TypeKind::Borrow(_) | TypeKind::End(..) => 4,
        TypeKind::Decl(d) => match &types.decls[*d].kind {
            DeclKind::Enum(cases) => discriminant_size(cases.len()),
            DeclKind::Flags(flags) => flags_size(flags.len()),
            DeclKind::Resource(_) => 4,
            _ => unreachable!("handled above"),
        },
        _ => unreachable!("handled above"),
    };
    (size, size)
}

/// The flat core values of a type (Canonical ABI flattening).
pub(super) fn flat(types: &Types, ty: Type) -> Vec<ValType> {
    if let Some(payloads) = cases(types, ty) {
        let mut joined: Vec<ValType> = Vec::new();
        for p in payloads.iter().flatten() {
            for (i, t) in flat(types, *p).into_iter().enumerate() {
                match joined.get_mut(i) {
                    Some(j) if *j == t => {}
                    Some(j)
                        if matches!(
                            (*j, t),
                            (ValType::I32, ValType::F32) | (ValType::F32, ValType::I32)
                        ) =>
                    {
                        *j = ValType::I32
                    }
                    Some(j) => *j = ValType::I64,
                    None => joined.push(t),
                }
            }
        }
        let mut out = vec![ValType::I32];
        out.extend(joined);
        return out;
    }
    if let Some(fields) = fields(types, ty) {
        return fields.iter().flat_map(|&t| flat(types, t)).collect();
    }
    match types.kind(ty) {
        TypeKind::Prim(p) => vec![p.val_type()],
        TypeKind::String | TypeKind::List { .. } => vec![ValType::I32, ValType::I32],
        _ => vec![ValType::I32],
    }
}

/// Whether a value of this type holds a string or a list.
fn has_list(types: &Types, ty: Type) -> bool {
    match types.kind(ty) {
        TypeKind::String | TypeKind::List { .. } => true,
        // The elements of a stream or a future do not cross with the handle.
        TypeKind::End(..) => false,
        _ => types.children(ty).into_iter().any(|t| has_list(types, t)),
    }
}

/// Whether an exported function needs an adapter: it takes or returns a heap value.
pub(super) fn needs_adapter(types: &Types, func: &FuncDef) -> bool {
    func.params
        .iter()
        .map(|(_, t)| *t)
        .chain(func.ret)
        .any(|t| !types.is_scalar(t))
}

fn params_flat(types: &Types, func: &FuncDef) -> Vec<ValType> {
    func.params
        .iter()
        .flat_map(|(_, t)| flat(types, *t))
        .collect()
}

/// Whether the result of an exported function is written in memory, at an address
/// returned by its adapter.
pub(super) fn result_in_memory(types: &Types, func: &FuncDef) -> bool {
    func.ret
        .is_some_and(|t| has_list(types, t) || flat(types, t).len() > MAX_FLAT_RESULTS)
}

/// Whether a call to an imported function goes through a reverse adapter (§ 13.10).
pub(super) fn import_needs_adapter(types: &Types, func: &FuncDef) -> bool {
    needs_adapter(types, func) || import_needs_memory(types, func)
}

/// The core type of an imported function, following the Canonical ABI: its flat
/// parameters (or a pointer to them), then a pointer to the return area if the result
/// goes through memory; its flat result otherwise.
pub(super) fn import_signature(types: &Types, func: &FuncDef) -> (Vec<ValType>, Vec<ValType>) {
    let mut params = params_flat(types, func);
    if params.len() > MAX_FLAT_PARAMS {
        params = vec![ValType::I32];
    }
    let results = match func.ret {
        Some(_) if result_in_memory(types, func) => {
            params.push(ValType::I32);
            Vec::new()
        }
        Some(t) => flat(types, t),
        None => Vec::new(),
    };
    (params, results)
}

/// Whether an exported function needs linear memory (§ 13.6). The result of an async
/// function goes to `task.return`, in memory only beyond 16 flat values (§ 13.13).
pub(super) fn export_needs_memory(types: &Types, func: &FuncDef) -> bool {
    if !func.is_async {
        return import_needs_memory(types, func);
    }
    params_need_memory(types, func)
        || func
            .ret
            .is_some_and(|t| has_list(types, t) || task_return_in_memory(types, func))
}

/// Whether an imported function needs linear memory (§ 13.6). An async import is
/// lowered without `async`, like the others (§ 13.13).
pub(super) fn import_needs_memory(types: &Types, func: &FuncDef) -> bool {
    params_need_memory(types, func) || result_in_memory(types, func)
}

fn params_need_memory(types: &Types, func: &FuncDef) -> bool {
    func.params.iter().any(|(_, t)| has_list(types, *t))
        || params_flat(types, func).len() > MAX_FLAT_PARAMS
}

/// Whether the `task.return` of an exported async function takes its result in memory:
/// beyond 16 flat values, as for parameters (§ 13.13).
fn task_return_in_memory(types: &Types, func: &FuncDef) -> bool {
    func.ret
        .is_some_and(|t| flat(types, t).len() > MAX_FLAT_PARAMS)
}

/// The core type of the `task.return` of an exported async function: its result as
/// flat parameters, or a pointer to it (§ 13.13).
pub(super) fn task_return_params(types: &Types, func: &FuncDef) -> Vec<ValType> {
    match func.ret {
        None => Vec::new(),
        Some(_) if task_return_in_memory(types, func) => vec![ValType::I32],
        Some(t) => flat(types, t),
    }
}

// ---- Code generation ----

fn mem(offset: u32, align: u32) -> MemArg {
    MemArg {
        offset: offset.into(),
        align,
        memory_index: 0,
    }
}

/// Load and store of a scalar type, with its size in bytes.
fn scalar_access(types: &Types, ty: Type) -> (Vec<Instruction<'static>>, Instruction<'static>) {
    let (size, _) = size_align(types, ty);
    let int = |size: u32, signed: bool| -> (Instruction<'static>, Instruction<'static>) {
        match (size, signed) {
            (1, false) => (I::I32Load8U(mem(0, 0)), I::I32Store8(mem(0, 0))),
            (1, true) => (I::I32Load8S(mem(0, 0)), I::I32Store8(mem(0, 0))),
            (2, false) => (I::I32Load16U(mem(0, 1)), I::I32Store16(mem(0, 1))),
            (2, true) => (I::I32Load16S(mem(0, 1)), I::I32Store16(mem(0, 1))),
            _ => (I::I32Load(mem(0, 2)), I::I32Store(mem(0, 2))),
        }
    };
    let (load, store) = match types.kind(ty) {
        TypeKind::Prim(Prim::Bool) => {
            // Any non-zero byte is `true`.
            let (load, store) = int(1, false);
            return (vec![load, I::I32Const(0), I::I32Ne], store);
        }
        TypeKind::Prim(Prim::S64 | Prim::U64) => (I::I64Load(mem(0, 3)), I::I64Store(mem(0, 3))),
        TypeKind::Prim(Prim::F32) => (I::F32Load(mem(0, 2)), I::F32Store(mem(0, 2))),
        TypeKind::Prim(Prim::F64) => (I::F64Load(mem(0, 3)), I::F64Store(mem(0, 3))),
        _ => int(size, ty.is_signed()),
    };
    (vec![load], store)
}

/// Adds `offset` to the memory argument of a load or store.
fn at(instr: &Instruction<'static>, offset: u32) -> Instruction<'static> {
    let shift = |m: &MemArg| mem(m.offset as u32 + offset, m.align);
    match instr {
        I::I32Load(m) => I::I32Load(shift(m)),
        I::I32Load8U(m) => I::I32Load8U(shift(m)),
        I::I32Load8S(m) => I::I32Load8S(shift(m)),
        I::I32Load16U(m) => I::I32Load16U(shift(m)),
        I::I32Load16S(m) => I::I32Load16S(shift(m)),
        I::I64Load(m) => I::I64Load(shift(m)),
        I::F32Load(m) => I::F32Load(shift(m)),
        I::F64Load(m) => I::F64Load(shift(m)),
        I::I32Store(m) => I::I32Store(shift(m)),
        I::I32Store8(m) => I::I32Store8(shift(m)),
        I::I32Store16(m) => I::I32Store16(shift(m)),
        I::I64Store(m) => I::I64Store(shift(m)),
        I::F32Store(m) => I::F32Store(shift(m)),
        I::F64Store(m) => I::F64Store(shift(m)),
        other => other.clone(),
    }
}

/// A function being generated.
struct Gen<'c, 'a> {
    cx: &'c mut ModuleCx<'a>,
    types: &'a Types,
    params: Vec<ValType>,
    locals: Vec<ValType>,
    code: Vec<Instruction<'static>>,
}

impl<'c, 'a> Gen<'c, 'a> {
    fn new(cx: &'c mut ModuleCx<'a>, params: Vec<ValType>) -> Self {
        let types = cx.types;
        Gen {
            cx,
            types,
            params,
            locals: Vec::new(),
            code: Vec::new(),
        }
    }

    fn emit(&mut self, instr: Instruction<'static>) {
        self.code.push(instr);
    }

    fn local(&mut self, ty: ValType) -> u32 {
        // Reference locals are nullable: Wasm does not track their initialization.
        let ty = match ty {
            ValType::Ref(r) => ref_type(
                match r.heap_type {
                    HeapType::Concrete(i) => i,
                    _ => unreachable!(),
                },
                true,
            ),
            t => t,
        };
        self.locals.push(ty);
        (self.params.len() + self.locals.len() - 1) as u32
    }

    fn finish(mut self, results: Vec<ValType>) -> (Vec<ValType>, Vec<ValType>, Function) {
        self.emit(I::End);
        let mut function = Function::new_with_locals_types(self.locals);
        for instr in &self.code {
            function.instruction(instr);
        }
        (self.params, results, function)
    }

    fn call(&mut self, helper: Boundary) {
        let index = self.cx.helper(Helper::Boundary(helper));
        self.emit(I::Call(index));
    }

    fn scratch_top(&self) -> u32 {
        self.cx.scratch_top.expect("module with a memory")
    }

    /// Gives back the scratch memory of a sync task (§ 13.7): everything above
    /// `$scratch_base`, which keeps the data of the async task that waits, if any
    /// (§ 13.13).
    fn free_scratch(&mut self) {
        let Some(top) = self.cx.scratch_top else {
            return;
        };
        match self.cx.scratch_base {
            Some(base) => self.code.extend([I::GlobalGet(base), I::GlobalSet(top)]),
            None => self.code.extend([I::I32Const(0), I::GlobalSet(top)]),
        }
    }

    /// `if`/`else` chain on the discriminant in local `disc`: `case(k)` pushes the value
    /// of case `k`, of type `result`.
    fn dispatch(
        &mut self,
        disc: u32,
        cases: usize,
        result: ValType,
        mut case: impl FnMut(&mut Self, u32),
    ) {
        for k in 0..cases - 1 {
            self.emit(I::LocalGet(disc));
            self.emit(I::I32Const(k as i32));
            self.emit(I::I32Eq);
            self.emit(I::If(BlockType::Result(result)));
            case(self, k as u32);
            self.emit(I::Else);
        }
        case(self, cases as u32 - 1);
        for _ in 0..cases - 1 {
            self.emit(I::End);
        }
    }

    // ---- Lifting ----

    /// Converts a flat value of the joined type `from` to the type `to` of a case.
    fn coerce(&mut self, from: ValType, to: ValType) {
        match (from, to) {
            _ if from == to => {}
            (ValType::I32, ValType::F32) => self.emit(I::F32ReinterpretI32),
            (ValType::I64, ValType::I32) => self.emit(I::I32WrapI64),
            (ValType::I64, ValType::F32) => {
                self.emit(I::I32WrapI64);
                self.emit(I::F32ReinterpretI32);
            }
            (ValType::I64, ValType::F64) => self.emit(I::F64ReinterpretI64),
            _ => unreachable!("joined flat types"),
        }
    }

    /// Lifts a value of type `ty` from flat values: `src` lists the locals that hold
    /// them, with their types, and the value starts at `src[pos]`.
    fn lift_flat(&mut self, ty: Type, src: &[(u32, ValType)], pos: usize) {
        let types = self.types;
        let read = |g: &mut Self, k: usize, want: ValType| {
            let (local, have) = src[k];
            g.emit(I::LocalGet(local));
            g.coerce(have, want);
        };
        if let Some(fields) = fields(types, ty) {
            let mut pos = pos;
            for &f in &fields {
                self.lift_flat(f, src, pos);
                pos += flat(types, f).len();
            }
            let index = self.cx.heap(ty);
            self.emit(I::StructNew(index));
        } else if let Some(payloads) = cases(types, ty) {
            let disc = self.local(ValType::I32);
            read(self, pos, ValType::I32);
            self.emit(I::LocalSet(disc));
            let result = self.cx.val(ty);
            self.dispatch(disc, payloads.len(), result, |g, k| {
                g.case(ty, k, payloads[k as usize], |g, p| {
                    g.lift_flat(p, src, pos + 1)
                })
            });
        } else if matches!(types.kind(ty), TypeKind::String | TypeKind::List { .. }) {
            read(self, pos, ValType::I32);
            read(self, pos + 1, ValType::I32);
            self.call(Boundary::LiftList(ty));
        } else if let Some(lift) = lift_handle(self.cx, ty) {
            read(self, pos, ValType::I32);
            self.call(lift);
        } else {
            let want = flat(types, ty)[0];
            read(self, pos, want);
        }
    }

    /// Pushes case `k` of the variant, `option` or `result` type `ty`; `payload` pushes
    /// the lifted payload.
    fn case(
        &mut self,
        ty: Type,
        k: u32,
        payload: Option<Type>,
        mut lift: impl FnMut(&mut Self, Type),
    ) {
        if let TypeKind::Option(inner) = self.types.kind(ty) {
            let inner = *inner;
            let index = self.cx.option_heap(inner);
            if k == 0 {
                self.emit(I::RefNull(HeapType::Concrete(index)));
            } else {
                lift(self, inner);
                if self.cx.boxed(inner) {
                    self.emit(I::StructNew(index));
                }
            }
            return;
        }
        self.emit(I::I32Const(k as i32));
        match payload {
            Some(p) => {
                lift(self, p);
                let index = self.cx.case_type(ty, k);
                self.emit(I::StructNew(index));
            }
            None => {
                let base = self.cx.heap(ty);
                self.emit(I::StructNew(base));
            }
        }
    }

    /// Lifts a value of type `ty` stored at `addr + offset`, `addr` being a local.
    /// Whether a value of type `ty` is a single `i32` or number that crosses the
    /// boundary as is: a scalar, or a handle of another component.
    fn scalar(&self, ty: Type) -> bool {
        self.types.is_scalar(ty) || self.cx.is_handle(ty).is_some() || self.cx.is_end(ty)
    }

    fn lift_at(&mut self, ty: Type, addr: u32, offset: u32) {
        self.emit(I::LocalGet(addr));
        if self.scalar(ty) {
            let (load, _) = scalar_access(self.types, ty);
            let mut load = load.into_iter();
            let first = load.next().unwrap();
            self.emit(at(&first, offset));
            self.code.extend(load);
        } else {
            if offset > 0 {
                self.emit(I::I32Const(offset as i32));
                self.emit(I::I32Add);
            }
            self.call(Boundary::LiftMem(ty));
        }
    }

    // ---- Lowering ----

    /// Lowers the value pushed by `value`, of type `ty`, to `addr + offset`.
    fn lower_at(&mut self, ty: Type, value: &[Instruction<'static>], addr: u32, offset: u32) {
        if self.scalar(ty) {
            let (_, store) = scalar_access(self.types, ty);
            self.emit(I::LocalGet(addr));
            self.code.extend(value.iter().cloned());
            self.emit(at(&store, offset));
        } else {
            self.code.extend(value.iter().cloned());
            self.emit(I::LocalGet(addr));
            if offset > 0 {
                self.emit(I::I32Const(offset as i32));
                self.emit(I::I32Add);
            }
            self.call(Boundary::LowerMem(ty));
        }
    }

    /// Pushes the value of local `local`, of Wasm type `val`, as a non-null reference
    /// when its type is one.
    fn get(&mut self, local: u32, val: ValType) {
        self.emit(I::LocalGet(local));
        if matches!(val, ValType::Ref(r) if !r.nullable) {
            self.emit(I::RefAsNonNull);
        }
    }

    /// Lowers the value pushed by `value`, of type `ty`, to its flat values, pushed in
    /// order (Canonical ABI flattening).
    fn lower_to_flat(&mut self, ty: Type, value: &[Instruction<'static>]) {
        let types = self.types;
        if self.scalar(ty) {
            self.code.extend(value.iter().cloned());
            return;
        }
        if matches!(types.kind(ty), TypeKind::String | TypeKind::List { .. }) {
            self.code.extend(value.iter().cloned());
            self.call(Boundary::LowerList(ty));
            return;
        }
        if let Some((d, _)) = types.resource(ty) {
            self.code.extend(value.iter().cloned());
            self.call(Boundary::LowerOwn(d));
            return;
        }
        let val = self.cx.val(ty);
        let v = self.local(val);
        self.code.extend(value.iter().cloned());
        self.emit(I::LocalSet(v));
        if let Some(fields) = fields(types, ty) {
            let index = self.cx.heap(ty);
            for (k, &f) in fields.iter().enumerate() {
                let value = [
                    I::LocalGet(v),
                    I::StructGet {
                        struct_type_index: index,
                        field_index: k as u32,
                    },
                ];
                self.lower_to_flat(f, &value);
            }
            return;
        }
        // A variant, `option` or `result`: the discriminant, then the joined payloads.
        let payloads = cases(types, ty).expect("a variant");
        let joined = flat(types, ty)[1..].to_vec();
        let slots: Vec<u32> = joined.iter().map(|&t| self.local(t)).collect();
        let disc = self.local(ValType::I32);
        let lower_case = |g: &mut Self, payload: Type, value: Vec<Instruction<'static>>| {
            g.lower_to_flat(payload, &value);
            let have = flat(types, payload);
            for (i, &t) in have.iter().enumerate().rev() {
                g.widen(t, joined[i]);
                g.emit(I::LocalSet(slots[i]));
            }
        };
        if let TypeKind::Option(inner) = types.kind(ty) {
            let inner = *inner;
            let boxed = self.cx.boxed(inner).then(|| self.cx.option_heap(inner));
            self.emit(I::LocalGet(v));
            self.emit(I::RefIsNull);
            self.emit(I::I32Eqz);
            self.emit(I::LocalTee(disc));
            self.emit(I::If(BlockType::Empty));
            let value = match boxed {
                Some(index) => vec![
                    I::LocalGet(v),
                    I::StructGet {
                        struct_type_index: index,
                        field_index: 0,
                    },
                ],
                None => vec![I::LocalGet(v), I::RefAsNonNull],
            };
            lower_case(self, inner, value);
            self.emit(I::End);
        } else {
            let base = self.cx.heap(ty);
            self.emit(I::LocalGet(v));
            self.emit(I::StructGet {
                struct_type_index: base,
                field_index: 0,
            });
            self.emit(I::LocalSet(disc));
            for (k, payload) in payloads.iter().enumerate() {
                let Some(p) = payload else { continue };
                let case = self.cx.case_type(ty, k as u32);
                self.emit(I::LocalGet(disc));
                self.emit(I::I32Const(k as i32));
                self.emit(I::I32Eq);
                self.emit(I::If(BlockType::Empty));
                let value = vec![
                    I::LocalGet(v),
                    I::RefCastNonNull(HeapType::Concrete(case)),
                    I::StructGet {
                        struct_type_index: case,
                        field_index: 1,
                    },
                ];
                lower_case(self, *p, value);
                self.emit(I::End);
            }
        }
        self.emit(I::LocalGet(disc));
        for slot in slots {
            self.emit(I::LocalGet(slot));
        }
    }

    /// Converts a flat value of a case, of type `from`, to the joined type `to`.
    fn widen(&mut self, from: ValType, to: ValType) {
        match (from, to) {
            _ if from == to => {}
            (ValType::F32, ValType::I32) => self.emit(I::I32ReinterpretF32),
            (ValType::I32, ValType::I64) => self.emit(I::I64ExtendI32U),
            (ValType::F32, ValType::I64) => {
                self.emit(I::I32ReinterpretF32);
                self.emit(I::I64ExtendI32U);
            }
            (ValType::F64, ValType::I64) => self.emit(I::I64ReinterpretF64),
            _ => unreachable!("joined flat types"),
        }
    }

    /// Lowers the value on the stack to its single flat value.
    fn lower_flat(&mut self, ty: Type) {
        let types = self.types;
        if self.scalar(ty) {
            return;
        }
        if let Some((d, _)) = types.resource(ty) {
            self.call(Boundary::LowerOwn(d));
            return;
        }
        if let Some(fields) = fields(types, ty) {
            // A single flat value: a single field.
            let index = self.cx.heap(ty);
            self.emit(I::StructGet {
                struct_type_index: index,
                field_index: 0,
            });
            self.lower_flat(fields[0]);
        } else {
            // A variant or result without payloads: its tag.
            let base = self.cx.heap(ty);
            self.emit(I::StructGet {
                struct_type_index: base,
                field_index: 0,
            });
        }
    }
}

impl Boundary {
    pub(super) fn generate(self, cx: &mut ModuleCx) -> (Vec<ValType>, Vec<ValType>, Function) {
        let types = cx.types;
        match self {
            Boundary::Realloc => realloc(cx),
            Boundary::PostReturn => {
                let mut g = Gen::new(cx, vec![ValType::I32]);
                g.free_scratch();
                g.finish(Vec::new())
            }
            Boundary::Adapter(f) => adapter(cx, f),
            Boundary::AsyncAdapter(f) => async_adapter(cx, f),
            Boundary::Callback => {
                let mut g = Gen::new(cx, vec![ValType::I32; 3]);
                g.emit(I::Unreachable);
                g.finish(vec![ValType::I32])
            }
            Boundary::Import(f) => import_adapter(cx, f),
            Boundary::LiftMem(ty) => {
                let mut g = Gen::new(cx, vec![ValType::I32]);
                let result = g.cx.val(ty);
                if let Some(fields) = fields(types, ty) {
                    let (offsets, _, _) = fields_layout(types, &fields);
                    for (&f, &offset) in fields.iter().zip(&offsets) {
                        g.lift_at(f, 0, offset);
                    }
                    let index = g.cx.heap(ty);
                    g.emit(I::StructNew(index));
                } else if let Some(payloads) = cases(types, ty) {
                    let (disc_size, offset, _, _) = variant_layout(types, &payloads);
                    let disc = g.local(ValType::I32);
                    g.emit(I::LocalGet(0));
                    g.emit(match disc_size {
                        1 => I::I32Load8U(mem(0, 0)),
                        2 => I::I32Load16U(mem(0, 1)),
                        _ => I::I32Load(mem(0, 2)),
                    });
                    g.emit(I::LocalSet(disc));
                    g.dispatch(disc, payloads.len(), result, |g, k| {
                        g.case(ty, k, payloads[k as usize], |g, p| g.lift_at(p, 0, offset))
                    });
                } else if let Some(lift) = lift_handle(g.cx, ty) {
                    g.emit(I::LocalGet(0));
                    g.emit(I::I32Load(mem(0, 2)));
                    g.call(lift);
                } else {
                    // A string or a list: (ptr, len).
                    g.emit(I::LocalGet(0));
                    g.emit(I::I32Load(mem(0, 2)));
                    g.emit(I::LocalGet(0));
                    g.emit(I::I32Load(mem(4, 2)));
                    g.call(Boundary::LiftList(ty));
                }
                g.finish(vec![result])
            }
            Boundary::LowerMem(ty) => {
                let val = cx.val(ty);
                let mut g = Gen::new(cx, vec![val, ValType::I32]);
                if let Some(fields) = fields(types, ty) {
                    let (offsets, _, _) = fields_layout(types, &fields);
                    let index = g.cx.heap(ty);
                    for (k, (&f, &offset)) in fields.iter().zip(&offsets).enumerate() {
                        let value = [
                            I::LocalGet(0),
                            I::StructGet {
                                struct_type_index: index,
                                field_index: k as u32,
                            },
                        ];
                        g.lower_at(f, &value, 1, offset);
                    }
                } else if let Some(payloads) = cases(types, ty) {
                    lower_variant(&mut g, ty, &payloads);
                } else if let Some((d, _)) = types.resource(ty) {
                    g.emit(I::LocalGet(1));
                    g.emit(I::LocalGet(0));
                    g.call(Boundary::LowerOwn(d));
                    g.emit(I::I32Store(mem(0, 2)));
                } else {
                    // A string or a list: (ptr, len).
                    let (ptr, len) = (g.local(ValType::I32), g.local(ValType::I32));
                    g.emit(I::LocalGet(0));
                    g.call(Boundary::LowerList(ty));
                    g.emit(I::LocalSet(len));
                    g.emit(I::LocalSet(ptr));
                    g.emit(I::LocalGet(1));
                    g.emit(I::LocalGet(ptr));
                    g.emit(I::I32Store(mem(0, 2)));
                    g.emit(I::LocalGet(1));
                    g.emit(I::LocalGet(len));
                    g.emit(I::I32Store(mem(4, 2)));
                }
                g.finish(Vec::new())
            }
            Boundary::LiftList(ty) => lift_list(cx, ty),
            Boundary::LowerList(ty) => lower_list(cx, ty),
            Boundary::LiftOwn(d) => lift_own(cx, d),
            Boundary::LiftBorrow(d) => {
                let rt = cx.resources[&d];
                let (object, reps) = handle_types(cx, d);
                let mut g = Gen::new(cx, vec![ValType::I32]);
                g.emit(I::GlobalGet(rt.reps));
                g.emit(I::LocalGet(0));
                g.emit(I::ArrayGet(reps));
                g.emit(I::RefAsNonNull);
                g.finish(vec![ref_type(object, false)])
            }
            Boundary::LowerOwn(d) => lower_own(cx, d),
            Boundary::Dtor(d) => dtor(cx, d),
            Boundary::Release(ty) => release(cx, ty),
            Boundary::ReleaseEnd(ty) => ends::release_end(cx, ty),
            Boundary::Wait => ends::wait(cx),
            Boundary::EndNew(ty) => ends::end_new(cx, ty),
            Boundary::StreamWrite(ty) => ends::stream_write(cx, ty),
            Boundary::StreamRead(ty) => ends::stream_read(cx, ty),
            Boundary::FutureWrite(ty) => ends::future_write(cx, ty),
            Boundary::FutureRead(ty) => ends::future_read(cx, ty),
        }
    }
}

/// Writes the discriminant, then the payload of the case, if any.
fn lower_variant(g: &mut Gen, ty: Type, payloads: &[Option<Type>]) {
    let types = g.types;
    let (disc_size, offset, _, _) = variant_layout(types, payloads);
    let store = match disc_size {
        1 => I::I32Store8(mem(0, 0)),
        2 => I::I32Store16(mem(0, 1)),
        _ => I::I32Store(mem(0, 2)),
    };
    if let TypeKind::Option(inner) = types.kind(ty) {
        let inner = *inner;
        let boxed = g.cx.boxed(inner).then(|| g.cx.option_heap(inner));
        // tag = 1 if the reference is not null
        let tag = g.local(ValType::I32);
        g.emit(I::LocalGet(0));
        g.emit(I::RefIsNull);
        g.emit(I::I32Eqz);
        g.emit(I::LocalSet(tag));
        g.emit(I::LocalGet(1));
        g.emit(I::LocalGet(tag));
        g.emit(store);
        g.emit(I::LocalGet(tag));
        g.emit(I::If(BlockType::Empty));
        let value = match boxed {
            Some(index) => vec![
                I::LocalGet(0),
                I::StructGet {
                    struct_type_index: index,
                    field_index: 0,
                },
            ],
            None => vec![I::LocalGet(0), I::RefAsNonNull],
        };
        g.lower_at(inner, &value, 1, offset);
        g.emit(I::End);
        return;
    }
    let base = g.cx.heap(ty);
    let tag = g.local(ValType::I32);
    g.emit(I::LocalGet(0));
    g.emit(I::StructGet {
        struct_type_index: base,
        field_index: 0,
    });
    g.emit(I::LocalSet(tag));
    g.emit(I::LocalGet(1));
    g.emit(I::LocalGet(tag));
    g.emit(store);
    for (k, payload) in payloads.iter().enumerate() {
        let Some(p) = payload else { continue };
        let case = g.cx.case_type(ty, k as u32);
        g.emit(I::LocalGet(tag));
        g.emit(I::I32Const(k as i32));
        g.emit(I::I32Eq);
        g.emit(I::If(BlockType::Empty));
        let value = [
            I::LocalGet(0),
            I::RefCastNonNull(HeapType::Concrete(case)),
            I::StructGet {
                struct_type_index: case,
                field_index: 1,
            },
        ];
        g.lower_at(*p, &value, 1, offset);
        g.emit(I::End);
    }
}

/// `array.get` for the elements of a list type.
fn array_get(elem: Type, array: u32) -> Instruction<'static> {
    match elem {
        Type::S8 | Type::S16 => I::ArrayGetS(array),
        Type::BOOL | Type::U8 | Type::U16 => I::ArrayGetU(array),
        _ => I::ArrayGet(array),
    }
}

/// `(ptr, len) -> T'`: a new array whose element `k` is lifted from `ptr + k × size`.
/// Locals: ptr = 0, len = 1.
fn lift_list(cx: &mut ModuleCx, ty: Type) -> (Vec<ValType>, Vec<ValType>, Function) {
    let types = cx.types;
    let elem = element(types, ty);
    let (size, _) = size_align(types, elem);
    let array = cx.heap(ty);
    let result = ref_type(array, false);
    let elem_val = cx.val(elem);
    let defaultable = !matches!(elem_val, ValType::Ref(r) if !r.nullable);
    let mut g = Gen::new(cx, vec![ValType::I32, ValType::I32]);
    let (r, i, addr) = (
        g.local(result),
        g.local(ValType::I32),
        g.local(ValType::I32),
    );
    if defaultable {
        g.emit(I::LocalGet(1));
        g.emit(I::ArrayNewDefault(array));
    } else {
        // No default value: the array is created with its first element.
        g.emit(I::LocalGet(1));
        g.emit(I::I32Eqz);
        g.emit(I::If(BlockType::Empty));
        g.emit(I::ArrayNewFixed {
            array_type_index: array,
            array_size: 0,
        });
        g.emit(I::Return);
        g.emit(I::End);
        g.lift_at(elem, 0, 0);
        g.emit(I::LocalGet(1));
        g.emit(I::ArrayNew(array));
        g.emit(I::I32Const(1));
        g.emit(I::LocalSet(i));
    }
    g.emit(I::LocalSet(r));
    g.emit(I::Block(BlockType::Empty));
    g.emit(I::Loop(BlockType::Empty));
    g.emit(I::LocalGet(i));
    g.emit(I::LocalGet(1));
    g.emit(I::I32GeU);
    g.emit(I::BrIf(1));
    // addr = ptr + i × size
    g.emit(I::LocalGet(0));
    g.emit(I::LocalGet(i));
    g.emit(I::I32Const(size as i32));
    g.emit(I::I32Mul);
    g.emit(I::I32Add);
    g.emit(I::LocalSet(addr));
    g.emit(I::LocalGet(r));
    g.emit(I::LocalGet(i));
    g.lift_at(elem, addr, 0);
    g.emit(I::ArraySet(array));
    g.emit(I::LocalGet(i));
    g.emit(I::I32Const(1));
    g.emit(I::I32Add);
    g.emit(I::LocalSet(i));
    g.emit(I::Br(0));
    g.emit(I::End);
    g.emit(I::End);
    g.emit(I::LocalGet(r));
    g.emit(I::RefAsNonNull);
    g.finish(vec![result])
}

/// `(T') -> (ptr, len)`: allocates `len × size` bytes, then lowers each element.
/// Locals: list = 0.
fn lower_list(cx: &mut ModuleCx, ty: Type) -> (Vec<ValType>, Vec<ValType>, Function) {
    let types = cx.types;
    let elem = element(types, ty);
    let (size, align) = size_align(types, elem);
    let array = cx.heap(ty);
    let mut g = Gen::new(cx, vec![ref_type(array, false)]);
    let (n, ptr, i, addr) = (
        g.local(ValType::I32),
        g.local(ValType::I32),
        g.local(ValType::I32),
        g.local(ValType::I32),
    );
    g.emit(I::LocalGet(0));
    g.emit(I::ArrayLen);
    g.emit(I::LocalSet(n));
    g.emit(I::I32Const(0));
    g.emit(I::I32Const(0));
    g.emit(I::I32Const(align as i32));
    g.emit(I::LocalGet(n));
    g.emit(I::I32Const(size as i32));
    g.emit(I::I32Mul);
    g.call(Boundary::Realloc);
    g.emit(I::LocalSet(ptr));
    g.emit(I::Block(BlockType::Empty));
    g.emit(I::Loop(BlockType::Empty));
    g.emit(I::LocalGet(i));
    g.emit(I::LocalGet(n));
    g.emit(I::I32GeU);
    g.emit(I::BrIf(1));
    g.emit(I::LocalGet(ptr));
    g.emit(I::LocalGet(i));
    g.emit(I::I32Const(size as i32));
    g.emit(I::I32Mul);
    g.emit(I::I32Add);
    g.emit(I::LocalSet(addr));
    let value = [I::LocalGet(0), I::LocalGet(i), array_get(elem, array)];
    g.lower_at(elem, &value, addr, 0);
    g.emit(I::LocalGet(i));
    g.emit(I::I32Const(1));
    g.emit(I::I32Add);
    g.emit(I::LocalSet(i));
    g.emit(I::Br(0));
    g.emit(I::End);
    g.emit(I::End);
    g.emit(I::LocalGet(ptr));
    g.emit(I::LocalGet(n));
    g.finish(vec![ValType::I32, ValType::I32])
}

/// The bump allocator of § 13.7. Locals: old_ptr = 0, old_size = 1, align = 2,
/// new_size = 3, ptr = 4.
fn realloc(cx: &mut ModuleCx) -> (Vec<ValType>, Vec<ValType>, Function) {
    let mut g = Gen::new(cx, vec![ValType::I32; 4]);
    let top = g.scratch_top();
    let ptr = g.local(ValType::I32);
    g.code.extend([
        // ptr = (top + align - 1) & -align
        I::GlobalGet(top),
        I::LocalGet(2),
        I::I32Add,
        I::I32Const(1),
        I::I32Sub,
        I::I32Const(0),
        I::LocalGet(2),
        I::I32Sub,
        I::I32And,
        I::LocalTee(ptr),
        I::LocalGet(3),
        I::I32Add,
        I::GlobalSet(top),
        // Grow the memory to hold `top` bytes.
        I::GlobalGet(top),
        I::MemorySize(0),
        I::I32Const(16),
        I::I32Shl,
        I::I32GtU,
        I::If(BlockType::Empty),
        I::GlobalGet(top),
        I::MemorySize(0),
        I::I32Const(16),
        I::I32Shl,
        I::I32Sub,
        I::I32Const(0xFFFF),
        I::I32Add,
        I::I32Const(16),
        I::I32ShrU,
        I::MemoryGrow(0),
        I::I32Const(-1),
        I::I32Eq,
        I::If(BlockType::Empty),
        I::Unreachable,
        I::End,
        I::End,
        // Copy the old bytes, if any.
        I::LocalGet(0),
        I::If(BlockType::Empty),
        I::LocalGet(ptr),
        I::LocalGet(0),
        I::LocalGet(1),
        I::LocalGet(3),
        I::LocalGet(1),
        I::LocalGet(3),
        I::I32LtU,
        I::Select,
        I::MemoryCopy {
            src_mem: 0,
            dst_mem: 0,
        },
        I::End,
        I::LocalGet(ptr),
    ]);
    g.finish(vec![ValType::I32])
}

/// The core parameters of an exported function: its flat parameters, or a pointer to
/// them beyond 16 (§ 13.6).
fn adapter_params(types: &Types, func: &FuncDef) -> Vec<ValType> {
    let flat_params = params_flat(types, func);
    if flat_params.len() > MAX_FLAT_PARAMS {
        vec![ValType::I32]
    } else {
        flat_params
    }
}

/// Lifts the arguments of an exported function onto the stack. Returns the locals that
/// keep the handles lent to it, with their `resource.drop`: they are given back before
/// the function delivers its result (§ 9.9).
fn lift_args(g: &mut Gen, func: &FuncDef) -> Vec<(u32, u32)> {
    let types = g.types;
    let flat_params = params_flat(types, func);
    let param_types: Vec<Type> = func.params.iter().map(|(_, t)| *t).collect();
    let mut lent = Vec::new();
    let mut keep_lent = |g: &mut Gen, t: Type| {
        if let (TypeKind::Borrow(_), Some(d)) = (types.kind(t), g.cx.is_handle(t)) {
            let tmp = g.local(ValType::I32);
            g.emit(I::LocalTee(tmp));
            lent.push((tmp, g.cx.drops[&d]));
        }
    };
    if flat_params.len() > MAX_FLAT_PARAMS {
        let (offsets, _, _) = fields_layout(types, &param_types);
        for (&t, &offset) in param_types.iter().zip(&offsets) {
            g.lift_at(t, 0, offset);
            keep_lent(g, t);
        }
    } else {
        let src: Vec<(u32, ValType)> = flat_params
            .iter()
            .enumerate()
            .map(|(i, &t)| (i as u32, t))
            .collect();
        let mut pos = 0;
        for &t in &param_types {
            g.lift_flat(t, &src, pos);
            keep_lent(g, t);
            pos += flat(types, t).len();
        }
    }
    lent
}

/// Lowers the value of type `t` on the stack into new scratch memory, and leaves its
/// address on the stack.
fn lower_to_memory(g: &mut Gen, t: Type) {
    let types = g.types;
    let (size, align) = size_align(types, t);
    let val = g.cx.val(t);
    let (value, ptr) = (g.local(val), g.local(ValType::I32));
    g.emit(I::LocalSet(value));
    g.emit(I::I32Const(0));
    g.emit(I::I32Const(0));
    g.emit(I::I32Const(align as i32));
    g.emit(I::I32Const(size as i32));
    g.call(Boundary::Realloc);
    g.emit(I::LocalSet(ptr));
    g.lower_at(t, &getter(value, val), ptr, 0);
    g.emit(I::LocalGet(ptr));
}

/// The instructions that push local `local`, of Wasm type `val`, as a non-null reference
/// when its type is one.
fn getter(local: u32, val: ValType) -> Vec<Instruction<'static>> {
    if matches!(val, ValType::Ref(r) if !r.nullable) {
        vec![I::LocalGet(local), I::RefAsNonNull]
    } else {
        vec![I::LocalGet(local)]
    }
}

/// The core export of an exported function (§ 13.8): lifts the arguments, frees the
/// scratch buffer, calls the function, lowers the result.
fn adapter(cx: &mut ModuleCx, f: FuncId) -> (Vec<ValType>, Vec<ValType>, Function) {
    let types = cx.types;
    let func = &cx.program.funcs[f];
    let target = cx.func_index[f].expect("exported function");
    let mut g = Gen::new(cx, adapter_params(types, func));
    let lent = lift_args(&mut g, func);
    // The arguments are on the heap now.
    g.free_scratch();
    g.emit(I::Call(target));
    for (tmp, drop) in lent {
        g.emit(I::LocalGet(tmp));
        g.emit(I::Call(drop));
    }

    let results = match func.ret {
        None => Vec::new(),
        Some(t) if result_in_memory(types, func) => {
            lower_to_memory(&mut g, t);
            vec![ValType::I32]
        }
        Some(t) => {
            g.lower_flat(t);
            flat(types, t)
        }
    };
    g.finish(results)
}

/// The core export of an exported async function, lifted with a callback (§ 13.13).
/// The whole body runs in this call, blocking where it waits (§ 12.12); its result goes
/// to `task.return`, then the scratch buffer is freed and the code `EXIT` (`0`) returned.
fn async_adapter(cx: &mut ModuleCx, f: FuncId) -> (Vec<ValType>, Vec<ValType>, Function) {
    let types = cx.types;
    let func = &cx.program.funcs[f];
    let target = cx.func_index[f].expect("exported function");
    let task_return = cx.task_returns[&f];
    let mut g = Gen::new(cx, adapter_params(types, func));
    let lent = lift_args(&mut g, func);
    // The arguments are on the heap now. No other task runs when an async task starts
    // (§ 13.13): `$scratch_base` is 0.
    g.free_scratch();
    g.emit(I::Call(target));
    // Lent handles are given back before the task delivers its result.
    for (tmp, drop) in lent {
        g.emit(I::LocalGet(tmp));
        g.emit(I::Call(drop));
    }
    match func.ret {
        None => {}
        Some(t) if task_return_in_memory(types, func) => lower_to_memory(&mut g, t),
        Some(t) => {
            let val = g.cx.val(t);
            let value = g.local(val);
            g.emit(I::LocalSet(value));
            g.lower_to_flat(t, &getter(value, val));
        }
    }
    g.emit(I::Call(task_return));
    // The waitable set of the task, if it has waited (§ 12.12).
    if let Some(w) = g.cx.waitable {
        g.code.extend([
            I::GlobalGet(w.set),
            I::If(BlockType::Empty),
            I::GlobalGet(w.set),
            I::Call(w.set_drop),
            I::I32Const(0),
            I::GlobalSet(w.set),
            I::End,
        ]);
    }
    for global in [g.cx.scratch_base, g.cx.scratch_top].into_iter().flatten() {
        g.emit(I::I32Const(0));
        g.emit(I::GlobalSet(global));
    }
    g.emit(I::I32Const(0));
    g.finish(vec![ValType::I32])
}

/// The reverse adapter of an imported function (§ 13.10): lowers the arguments, calls the
/// import, lifts the result, then sets `$scratch_top` back to where it was.
fn import_adapter(cx: &mut ModuleCx, f: FuncId) -> (Vec<ValType>, Vec<ValType>, Function) {
    let types = cx.types;
    let func = &cx.program.funcs[f];
    let params: Vec<ValType> = func.params.iter().map(|(_, t)| cx.val(*t)).collect();
    let ret_val = func.ret.map(|t| cx.val(t));
    let target = cx.func_index[f].expect("imported function");
    let mut g = Gen::new(cx, params.clone());
    let saved = g.cx.scratch_top.map(|top| {
        let saved = g.local(ValType::I32);
        g.emit(I::GlobalGet(top));
        g.emit(I::LocalSet(saved));
        saved
    });

    let param_types: Vec<Type> = func.params.iter().map(|(_, t)| *t).collect();
    if params_flat(types, func).len() > MAX_FLAT_PARAMS {
        let (offsets, size, align) = fields_layout(types, &param_types);
        let area = g.local(ValType::I32);
        g.emit(I::I32Const(0));
        g.emit(I::I32Const(0));
        g.emit(I::I32Const(align as i32));
        g.emit(I::I32Const(size as i32));
        g.call(Boundary::Realloc);
        g.emit(I::LocalSet(area));
        for (k, (&t, &offset)) in param_types.iter().zip(&offsets).enumerate() {
            g.lower_at(t, &[I::LocalGet(k as u32)], area, offset);
        }
        g.emit(I::LocalGet(area));
    } else {
        for (k, &t) in param_types.iter().enumerate() {
            g.lower_to_flat(t, &[I::LocalGet(k as u32)]);
        }
    }
    let ret_area = match func.ret {
        Some(t) if result_in_memory(types, func) => {
            let (size, align) = size_align(types, t);
            let ptr = g.local(ValType::I32);
            g.emit(I::I32Const(0));
            g.emit(I::I32Const(0));
            g.emit(I::I32Const(align as i32));
            g.emit(I::I32Const(size as i32));
            g.call(Boundary::Realloc);
            g.emit(I::LocalTee(ptr));
            Some(ptr)
        }
        _ => None,
    };
    if let (true, Some(base), Some(top)) = (func.is_async, g.cx.scratch_base, g.cx.scratch_top) {
        // The async task waits in this call: the sync tasks that run meanwhile keep its
        // data, up to here (§ 13.13).
        g.emit(I::GlobalGet(top));
        g.emit(I::GlobalSet(base));
    }
    g.emit(I::Call(target));

    let results = match (func.ret, ret_val) {
        (Some(t), Some(val)) => {
            match ret_area {
                Some(ptr) => g.lift_at(t, ptr, 0),
                None => {
                    let flat_ty = flat(types, t)[0];
                    let r = g.local(flat_ty);
                    g.emit(I::LocalSet(r));
                    g.lift_flat(t, &[(r, flat_ty)], 0);
                }
            }
            if let (Some(saved), Some(top)) = (saved, g.cx.scratch_top) {
                let value = g.local(val);
                g.emit(I::LocalSet(value));
                g.emit(I::LocalGet(saved));
                g.emit(I::GlobalSet(top));
                g.get(value, val);
            }
            vec![val]
        }
        _ => {
            if let (Some(saved), Some(top)) = (saved, g.cx.scratch_top) {
                g.emit(I::LocalGet(saved));
                g.emit(I::GlobalSet(top));
            }
            Vec::new()
        }
    };
    g.finish(results)
}

// ---- Resources (§ 12.10) ----

/// The helper that lifts a handle of an exported resource, `own` or `borrow`.
fn lift_handle(cx: &ModuleCx, ty: Type) -> Option<Boundary> {
    let (d, _) = cx.types.resource(ty)?;
    if !cx.resources.contains_key(&d) {
        return None;
    }
    let types = cx.types;
    Some(match types.kind(ty) {
        TypeKind::Borrow(_) => Boundary::LiftBorrow(d),
        _ => Boundary::LiftOwn(d),
    })
}

/// The struct type of a resource and the array type of its table.
fn handle_types(cx: &mut ModuleCx, d: DeclId) -> (u32, u32) {
    let object = cx.heap(cx.types.decl_type_of(d));
    let reps = cx.array(wasm_encoder::StorageType::Val(ref_type(object, true)));
    (object, reps)
}

/// `(handle) -> r`: `rep(handle)` gives the slot of the object; dropping the handle
/// then runs the destructor, which frees the slot.
fn lift_own(cx: &mut ModuleCx, d: DeclId) -> (Vec<ValType>, Vec<ValType>, Function) {
    let rt = cx.resources[&d];
    let (object, reps) = handle_types(cx, d);
    let mut g = Gen::new(cx, vec![ValType::I32]);
    let result = g.local(ref_type(object, false));
    g.emit(I::GlobalGet(rt.reps));
    g.emit(I::LocalGet(0));
    g.emit(I::Call(rt.rep));
    g.emit(I::ArrayGet(reps));
    g.emit(I::LocalSet(result));
    g.emit(I::LocalGet(0));
    g.emit(I::Call(rt.drop));
    g.emit(I::LocalGet(result));
    g.emit(I::RefAsNonNull);
    g.finish(vec![ref_type(object, false)])
}

/// `(r) -> handle`: takes a free slot (the last freed one, or a new one, growing the
/// table and the stack of free slots together), stores the object there and creates a
/// handle whose rep is the slot. Locals: object = 0.
fn lower_own(cx: &mut ModuleCx, d: DeclId) -> (Vec<ValType>, Vec<ValType>, Function) {
    let rt = cx.resources[&d];
    let (object, reps) = handle_types(cx, d);
    let frees = cx.array(wasm_encoder::StorageType::Val(ValType::I32));
    let mut g = Gen::new(cx, vec![ref_type(object, false)]);
    let (slot, cap) = (g.local(ValType::I32), g.local(ValType::I32));
    let (new_reps, new_frees) = (
        g.local(ref_type(reps, true)),
        g.local(ref_type(frees, true)),
    );
    g.code.extend([
        I::GlobalGet(rt.nfree),
        I::If(BlockType::Empty),
        // slot = frees[--nfree]
        I::GlobalGet(rt.nfree),
        I::I32Const(1),
        I::I32Sub,
        I::GlobalSet(rt.nfree),
        I::GlobalGet(rt.frees),
        I::GlobalGet(rt.nfree),
        I::ArrayGet(frees),
        I::LocalSet(slot),
        I::Else,
        // slot = used++
        I::GlobalGet(rt.used),
        I::LocalTee(slot),
        I::I32Const(1),
        I::I32Add,
        I::GlobalSet(rt.used),
        // cap = reps == null ? 0 : len(reps)
        I::GlobalGet(rt.reps),
        I::RefIsNull,
        I::If(BlockType::Result(ValType::I32)),
        I::I32Const(0),
        I::Else,
        I::GlobalGet(rt.reps),
        I::ArrayLen,
        I::End,
        I::LocalTee(cap),
        I::LocalGet(slot),
        I::I32Eq,
        I::If(BlockType::Empty),
        // Grow both arrays to max(8, 2 × cap).
        I::LocalGet(cap),
        I::I32Const(1),
        I::I32Shl,
        I::I32Const(8),
        I::LocalGet(cap),
        I::I32Const(4),
        I::I32GeU,
        I::Select,
        I::LocalTee(cap),
        I::ArrayNewDefault(reps),
        I::LocalSet(new_reps),
        I::LocalGet(cap),
        I::ArrayNewDefault(frees),
        I::LocalSet(new_frees),
        I::LocalGet(slot),
        I::If(BlockType::Empty),
        I::LocalGet(new_reps),
        I::I32Const(0),
        I::GlobalGet(rt.reps),
        I::I32Const(0),
        I::LocalGet(slot),
        I::ArrayCopy {
            array_type_index_dst: reps,
            array_type_index_src: reps,
        },
        I::End,
        I::LocalGet(new_reps),
        I::GlobalSet(rt.reps),
        I::LocalGet(new_frees),
        I::GlobalSet(rt.frees),
        I::End,
        I::End,
        // reps[slot] = object
        I::GlobalGet(rt.reps),
        I::LocalGet(slot),
        I::LocalGet(0),
        I::ArraySet(reps),
        I::LocalGet(slot),
        I::Call(rt.new),
    ]);
    g.finish(vec![ValType::I32])
}

/// `(rep)`: empties the slot and pushes it on the stack of free slots, which is as large
/// as the table. Locals: rep = 0.
fn dtor(cx: &mut ModuleCx, d: DeclId) -> (Vec<ValType>, Vec<ValType>, Function) {
    let rt = cx.resources[&d];
    let (object, reps) = handle_types(cx, d);
    let frees = cx.array(wasm_encoder::StorageType::Val(ValType::I32));
    let mut g = Gen::new(cx, vec![ValType::I32]);
    g.code.extend([
        I::GlobalGet(rt.reps),
        I::LocalGet(0),
        I::RefNull(HeapType::Concrete(object)),
        I::ArraySet(reps),
        I::GlobalGet(rt.frees),
        I::GlobalGet(rt.nfree),
        I::LocalGet(0),
        I::ArraySet(frees),
        I::GlobalGet(rt.nfree),
        I::I32Const(1),
        I::I32Add,
        I::GlobalSet(rt.nfree),
    ]);
    g.finish(Vec::new())
}

/// `(T')`: releases the handles of a value (§ 9.9): a handle itself, the elements of a
/// tuple, or the payload of the case of an `option`, a `result` or a variant. Records
/// and lists hold no handles (E0220). Locals: value = 0.
fn release(cx: &mut ModuleCx, ty: Type) -> (Vec<ValType>, Vec<ValType>, Function) {
    let types = cx.types;
    let val = cx.val(ty);
    let mut g = Gen::new(cx, vec![val]);
    if g.cx.owned_handle(ty) {
        let drop = g.cx.release_func(ty);
        g.code.extend([
            I::LocalGet(0),
            I::If(BlockType::Empty),
            I::LocalGet(0),
            I::Call(drop),
            I::End,
        ]);
    } else if let TypeKind::Option(inner) = types.kind(ty) {
        let inner = *inner;
        g.emit(I::LocalGet(0));
        g.emit(I::RefIsNull);
        g.emit(I::I32Eqz);
        g.emit(I::If(BlockType::Empty));
        g.emit(I::LocalGet(0));
        if g.cx.boxed(inner) {
            let boxed = g.cx.option_heap(inner);
            g.emit(I::StructGet {
                struct_type_index: boxed,
                field_index: 0,
            });
        } else {
            g.emit(I::RefAsNonNull);
        }
        g.call(Boundary::Release(inner));
        g.emit(I::End);
    } else if let TypeKind::Tuple(elems) = types.kind(ty) {
        let tuple = g.cx.heap(ty);
        for (k, &elem) in elems.iter().enumerate() {
            if !g.cx.has_handles(elem) {
                continue;
            }
            g.code.extend([
                I::LocalGet(0),
                I::StructGet {
                    struct_type_index: tuple,
                    field_index: k as u32,
                },
            ]);
            g.call(Boundary::Release(elem));
        }
    } else if let Some(payloads) = cases(types, ty) {
        let base = g.cx.heap(ty);
        for (k, payload) in payloads.iter().enumerate() {
            let Some(p) = *payload else { continue };
            if !g.cx.has_handles(p) {
                continue;
            }
            let case = g.cx.case_type(ty, k as u32);
            g.code.extend([
                I::LocalGet(0),
                I::StructGet {
                    struct_type_index: base,
                    field_index: 0,
                },
                I::I32Const(k as i32),
                I::I32Eq,
                I::If(BlockType::Empty),
                I::LocalGet(0),
                I::RefCastNonNull(HeapType::Concrete(case)),
                I::StructGet {
                    struct_type_index: case,
                    field_index: 1,
                },
            ]);
            g.call(Boundary::Release(p));
            g.emit(I::End);
        }
    }
    g.finish(Vec::new())
}
