//! The component boundary (§ v2.7): layout of the Canonical ABI, adapters of the exported
//! functions, lifting and lowering through linear memory, `realloc` and post-return.
//!
//! Values cross the boundary as deep copies. Lifting reads the flat values and the bytes
//! of linear memory and builds new WasmGC objects; lowering writes them back, in memory
//! allocated by the bump allocator of § v2.7.4.

use wasm_encoder::{BlockType, Function, HeapType, Instruction, MemArg, ValType};

use super::helpers::Helper;
use super::{ModuleCx, ref_type};
use crate::hir::{FuncDef, FuncId};
use crate::types::{DeclKind, Prim, Type, TypeKind, Types};

use Instruction as I;

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
    /// The core export of an exported function (§ v2.7.5).
    Adapter(FuncId),
    /// `(ptr) -> T'`: lifts a value stored at `ptr`.
    LiftMem(Type),
    /// `(T', ptr)`: lowers a value to `ptr`.
    LowerMem(Type),
    /// `(ptr, len) -> T'`: lifts a string or a list.
    LiftList(Type),
    /// `(T') -> (ptr, len)`: lowers a string or a list into new memory.
    LowerList(Type),
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
        TypeKind::Decl(d) => match &types.decls[*d].kind {
            DeclKind::Enum(cases) => discriminant_size(cases.len()),
            DeclKind::Flags(flags) => flags_size(flags.len()),
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
    matches!(types.kind(ty), TypeKind::String | TypeKind::List { .. })
        || types.children(ty).into_iter().any(|t| has_list(types, t))
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

/// Whether an exported function needs linear memory (§ v2.7.3).
pub(super) fn needs_memory(types: &Types, func: &FuncDef) -> bool {
    func.params.iter().any(|(_, t)| has_list(types, *t))
        || params_flat(types, func).len() > MAX_FLAT_PARAMS
        || result_in_memory(types, func)
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
    fn lift_at(&mut self, ty: Type, addr: u32, offset: u32) {
        self.emit(I::LocalGet(addr));
        if self.types.is_scalar(ty) {
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
        if self.types.is_scalar(ty) {
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

    /// Lowers the value on the stack to its single flat value.
    fn lower_flat(&mut self, ty: Type) {
        let types = self.types;
        if types.is_scalar(ty) {
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
                let top = g.scratch_top();
                g.emit(I::I32Const(0));
                g.emit(I::GlobalSet(top));
                g.finish(Vec::new())
            }
            Boundary::Adapter(f) => adapter(cx, f),
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

/// The bump allocator of § v2.7.4. Locals: old_ptr = 0, old_size = 1, align = 2,
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

/// The core export of an exported function (§ v2.7.5): lifts the arguments, frees the
/// scratch buffer, calls the function, lowers the result.
fn adapter(cx: &mut ModuleCx, f: FuncId) -> (Vec<ValType>, Vec<ValType>, Function) {
    let types = cx.types;
    let func = &cx.program.funcs[f];
    let flat_params = params_flat(types, func);
    let in_memory = flat_params.len() > MAX_FLAT_PARAMS;
    let params = if in_memory {
        vec![ValType::I32]
    } else {
        flat_params.clone()
    };
    let target = cx.func_index[f].expect("exported function");
    let mut g = Gen::new(cx, params);

    let param_types: Vec<Type> = func.params.iter().map(|(_, t)| *t).collect();
    if in_memory {
        let (offsets, _, _) = fields_layout(types, &param_types);
        for (&t, &offset) in param_types.iter().zip(&offsets) {
            g.lift_at(t, 0, offset);
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
            pos += flat(types, t).len();
        }
    }
    if let Some(top) = g.cx.scratch_top {
        // The arguments are on the heap now.
        g.emit(I::I32Const(0));
        g.emit(I::GlobalSet(top));
    }
    g.emit(I::Call(target));

    let results = match func.ret {
        None => Vec::new(),
        Some(t) if result_in_memory(types, func) => {
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
            let get = [I::LocalGet(value), I::RefAsNonNull];
            let get: &[Instruction] = if matches!(val, ValType::Ref(r) if !r.nullable) {
                &get
            } else {
                &get[..1]
            };
            g.lower_at(t, get, ptr, 0);
            g.emit(I::LocalGet(ptr));
            vec![ValType::I32]
        }
        Some(t) => {
            g.lower_flat(t);
            flat(types, t)
        }
    };
    g.finish(results)
}
