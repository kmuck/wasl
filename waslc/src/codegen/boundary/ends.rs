//! Streams and futures (§ 12.12): the helpers behind their built-in functions, the wait
//! of a task, and the release of an end.

use wasm_encoder::{BlockType, HeapType, Instruction as I, ValType};

use super::{Boundary, Gen, element, getter, mem, size_align};
use crate::codegen::{ModuleCx, ref_type};
use crate::types::{Type, TypeKind};
use wasm_encoder::Function;

/// The code `BLOCKED` of an operation lowered `async`.
const BLOCKED: i32 = -1;
/// The status `DROPPED` of a copy, in the low 4 bits of its result.
const DROPPED: i32 = 1;

/// The intrinsics of the Component Model for the streams or the futures of one type
/// (§ 13.13): indices of the imported functions.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EndRt {
    pub new: u32,
    pub write: u32,
    pub read: u32,
    pub drop_readable: u32,
    pub drop_writable: u32,
}

/// The waitable set of the async task (§ 12.12): the intrinsics, and the global that
/// keeps the set, `0` until the first wait.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WaitRt {
    pub set_new: u32,
    pub join: u32,
    pub wait: u32,
    pub set_drop: u32,
    pub set: u32,
}

/// The intrinsics of the stream or future type of the end type `ty`.
fn rt(cx: &ModuleCx, ty: Type) -> EndRt {
    let (kind, elem) = cx.types.end(ty).expect("an end");
    cx.ends[&(kind.is_stream(), elem)]
}

/// Pushes whether the end in local `end` has been closed by the other side: `-1`
/// (§ 12.12). The empty handle `0` goes to the Component Model, which traps (§ 9.9).
fn closed(g: &mut Gen, end: u32) {
    g.code.extend([I::LocalGet(end), I::I32Const(-1), I::I32Eq]);
}

/// Calls the wait of the task when the result code in local `code` is `BLOCKED`, and
/// puts the code of the event in `code` (§ 12.12).
fn wait_if_blocked(g: &mut Gen, end: u32, code: u32) {
    g.code.extend([
        I::LocalGet(code),
        I::I32Const(BLOCKED),
        I::I32Eq,
        I::If(BlockType::Empty),
        I::LocalGet(end),
    ]);
    g.call(Boundary::Wait);
    g.code.extend([I::LocalSet(code), I::End]);
}

/// Pushes whether the result code in local `code` reports `DROPPED`.
fn dropped(g: &mut Gen, code: u32) {
    g.code.extend([
        I::LocalGet(code),
        I::I32Const(0xF),
        I::I32And,
        I::I32Const(DROPPED),
        I::I32Eq,
    ]);
}

/// Allocates `size` bytes aligned to `align` in the scratch buffer, into a new local.
fn alloc(g: &mut Gen, align: u32, size: &[I<'static>]) -> u32 {
    let ptr = g.local(ValType::I32);
    g.code
        .extend([I::I32Const(0), I::I32Const(0), I::I32Const(align as i32)]);
    g.code.extend(size.iter().cloned());
    g.call(Boundary::Realloc);
    g.emit(I::LocalSet(ptr));
    ptr
}

/// Saves `$scratch_top` in a new local: the data of an operation is given back when it
/// ends.
fn save_top(g: &mut Gen) -> u32 {
    let top = g.scratch_top();
    let saved = g.local(ValType::I32);
    g.code.extend([I::GlobalGet(top), I::LocalSet(saved)]);
    saved
}

fn restore_top(g: &mut Gen, saved: u32) {
    let top = g.scratch_top();
    g.code.extend([I::LocalGet(saved), I::GlobalSet(top)]);
}

/// `(end)`: releases an end, unless it is `0` or `-1` (§ 12.12).
pub(super) fn release_end(cx: &mut ModuleCx, ty: Type) -> (Vec<ValType>, Vec<ValType>, Function) {
    let rt = rt(cx, ty);
    let (kind, _) = cx.types.end(ty).expect("an end");
    let drop = if kind.readable() {
        rt.drop_readable
    } else {
        rt.drop_writable
    };
    let mut g = Gen::new(cx, vec![ValType::I32]);
    // Neither `0` nor `-1`.
    g.code.extend([
        I::LocalGet(0),
        I::I32Const(1),
        I::I32Add,
        I::I32Const(1),
        I::I32GtU,
        I::If(BlockType::Empty),
        I::LocalGet(0),
        I::Call(drop),
        I::End,
    ]);
    g.finish(Vec::new())
}

/// `(end) -> code`: waits for the event of the end, in the waitable set of the task, and
/// returns its result code (§ 12.12). The data of the task stays in the scratch buffer
/// while it waits: the sync tasks that run meanwhile allocate above it (§ 13.13).
pub(super) fn wait(cx: &mut ModuleCx) -> (Vec<ValType>, Vec<ValType>, Function) {
    let w = cx.waitable.expect("a module that waits");
    let mut g = Gen::new(cx, vec![ValType::I32]);
    g.code.extend([
        I::GlobalGet(w.set),
        I::I32Eqz,
        I::If(BlockType::Empty),
        I::Call(w.set_new),
        I::GlobalSet(w.set),
        I::End,
        I::LocalGet(0),
        I::GlobalGet(w.set),
        I::Call(w.join),
    ]);
    // The event: the waitable, then the result code.
    let event = alloc(&mut g, 4, &[I::I32Const(8)]);
    if let (Some(base), Some(top)) = (g.cx.scratch_base, g.cx.scratch_top) {
        g.code.extend([I::GlobalGet(top), I::GlobalSet(base)]);
    }
    g.code.extend([
        I::GlobalGet(w.set),
        I::LocalGet(event),
        I::Call(w.wait),
        I::Drop,
        I::LocalGet(0),
        I::I32Const(0),
        I::Call(w.join),
        I::LocalGet(event),
        I::I32Load(mem(4, 2)),
    ]);
    g.finish(vec![ValType::I32])
}

/// `() -> tuple<R, W>`: a new stream or future, its readable end then its writable end
/// (§ 12.12). `stream.new` returns them in one `i64`, the readable end in the low bits.
pub(super) fn end_new(cx: &mut ModuleCx, ty: Type) -> (Vec<ValType>, Vec<ValType>, Function) {
    let TypeKind::Tuple(elems) = cx.types.kind(ty) else {
        unreachable!("a tuple of two ends")
    };
    let rt = rt(cx, elems[0]);
    let tuple = cx.heap(ty);
    let mut g = Gen::new(cx, Vec::new());
    let both = g.local(ValType::I64);
    g.code.extend([
        I::Call(rt.new),
        I::LocalSet(both),
        I::LocalGet(both),
        I::I32WrapI64,
        I::LocalGet(both),
        I::I64Const(32),
        I::I64ShrU,
        I::I32WrapI64,
        I::StructNew(tuple),
    ]);
    g.finish(vec![ref_type(tuple, false)])
}

/// `(tx, xs) -> (count, tx)`: `stream::write` (§ 12.12). Lowers the elements of `xs`, then
/// writes until the reader has taken them all, waiting when the write is blocked. When
/// the reader has left, the end is released and becomes `-1`, and the writes stop.
pub(super) fn stream_write(
    cx: &mut ModuleCx,
    list: Type,
) -> (Vec<ValType>, Vec<ValType>, Function) {
    let types = cx.types;
    let elem = element(types, list);
    let (size, _) = size_align(types, elem);
    let rt = cx.ends[&(true, elem)];
    let array = cx.heap(list);
    let mut g = Gen::new(cx, vec![ValType::I32, ref_type(array, false)]);
    let (ptr, len, total, code) = (
        g.local(ValType::I32),
        g.local(ValType::I32),
        g.local(ValType::I32),
        g.local(ValType::I32),
    );
    closed(&mut g, 0);
    g.code.extend([
        I::If(BlockType::Empty),
        I::I32Const(0),
        I::LocalGet(0),
        I::Return,
        I::End,
    ]);
    let saved = save_top(&mut g);
    g.emit(I::LocalGet(1));
    g.call(Boundary::LowerList(list));
    g.code.extend([
        I::LocalSet(len),
        I::LocalSet(ptr),
        I::Block(BlockType::Empty),
        I::Loop(BlockType::Empty),
        I::LocalGet(total),
        I::LocalGet(len),
        I::I32GeU,
        I::BrIf(1),
        I::LocalGet(0),
        I::LocalGet(ptr),
        I::LocalGet(total),
        I::I32Const(size as i32),
        I::I32Mul,
        I::I32Add,
        I::LocalGet(len),
        I::LocalGet(total),
        I::I32Sub,
        I::Call(rt.write),
        I::LocalSet(code),
    ]);
    wait_if_blocked(&mut g, 0, code);
    g.code.extend([
        I::LocalGet(total),
        I::LocalGet(code),
        I::I32Const(4),
        I::I32ShrU,
        I::I32Add,
        I::LocalSet(total),
    ]);
    dropped(&mut g, code);
    g.code.extend([
        I::If(BlockType::Empty),
        I::LocalGet(0),
        I::Call(rt.drop_writable),
        I::I32Const(-1),
        I::LocalSet(0),
        I::Br(2),
        I::End,
        I::Br(0),
        I::End,
        I::End,
    ]);
    restore_top(&mut g, saved);
    g.code.extend([I::LocalGet(total), I::LocalGet(0)]);
    g.finish(vec![ValType::I32, ValType::I32])
}

/// `(rx, n) -> (option<list<T>>, rx)`: `stream::read` (§ 12.12). Reads at most `n`
/// elements, waiting when the read is blocked; `none` when the writer has left and
/// nothing came, after which the end is `-1` and every read returns `none`.
pub(super) fn stream_read(
    cx: &mut ModuleCx,
    result: Type,
) -> (Vec<ValType>, Vec<ValType>, Function) {
    let types = cx.types;
    let TypeKind::Option(list) = *types.kind(result) else {
        unreachable!("an option of a list")
    };
    let elem = element(types, list);
    let (size, align) = size_align(types, elem);
    let rt = cx.ends[&(true, elem)];
    let array = cx.heap(list);
    let value = cx.val(result);
    let mut g = Gen::new(cx, vec![ValType::I32, ValType::I32]);
    let (code, count, out) = (g.local(ValType::I32), g.local(ValType::I32), g.local(value));
    closed(&mut g, 0);
    g.code.extend([
        I::If(BlockType::Empty),
        I::RefNull(HeapType::Concrete(array)),
        I::LocalGet(0),
        I::Return,
        I::End,
    ]);
    let saved = save_top(&mut g);
    let ptr = alloc(
        &mut g,
        align,
        &[I::LocalGet(1), I::I32Const(size as i32), I::I32Mul],
    );
    g.code.extend([
        I::LocalGet(0),
        I::LocalGet(ptr),
        I::LocalGet(1),
        I::Call(rt.read),
        I::LocalSet(code),
    ]);
    wait_if_blocked(&mut g, 0, code);
    g.code.extend([
        I::LocalGet(code),
        I::I32Const(4),
        I::I32ShrU,
        I::LocalSet(count),
    ]);
    dropped(&mut g, code);
    g.code.extend([
        I::If(BlockType::Empty),
        I::LocalGet(0),
        I::Call(rt.drop_readable),
        I::I32Const(-1),
        I::LocalSet(0),
        I::End,
        // Nothing read from a stream that has ended: `none`.
        I::LocalGet(count),
        I::I32Eqz,
        I::LocalGet(0),
        I::I32Const(-1),
        I::I32Eq,
        I::I32And,
        I::If(BlockType::Result(ref_type(array, true))),
        I::RefNull(HeapType::Concrete(array)),
        I::Else,
        I::LocalGet(ptr),
        I::LocalGet(count),
    ]);
    g.call(Boundary::LiftList(list));
    g.code.extend([I::End, I::LocalSet(out)]);
    restore_top(&mut g, saved);
    g.code.extend([I::LocalGet(out), I::LocalGet(0)]);
    g.finish(vec![value, ValType::I32])
}

/// `(tx, v) -> bool`: `future::write` (§ 12.12). Lowers `v`, writes it, waits when the
/// write is blocked, and releases the end. `false` when the reader has left: the handles
/// of `v` are then released.
pub(super) fn future_write(
    cx: &mut ModuleCx,
    elem: Type,
) -> (Vec<ValType>, Vec<ValType>, Function) {
    let types = cx.types;
    let (size, align) = size_align(types, elem);
    let rt = cx.ends[&(false, elem)];
    let val = cx.val(elem);
    let handles = cx.has_handles(elem);
    let mut g = Gen::new(cx, vec![ValType::I32, val]);
    let code = g.local(ValType::I32);
    let saved = save_top(&mut g);
    let ptr = alloc(&mut g, align, &[I::I32Const(size as i32)]);
    g.lower_at(elem, &[I::LocalGet(1)], ptr, 0);
    g.code.extend([
        I::LocalGet(0),
        I::LocalGet(ptr),
        I::Call(rt.write),
        I::LocalSet(code),
    ]);
    wait_if_blocked(&mut g, 0, code);
    g.code.extend([I::LocalGet(0), I::Call(rt.drop_writable)]);
    restore_top(&mut g, saved);
    dropped(&mut g, code);
    g.emit(I::If(BlockType::Result(ValType::I32)));
    if handles {
        // Nobody took the value: its handles are still this component's.
        g.emit(I::LocalGet(1));
        g.call(Boundary::Release(elem));
    }
    g.code
        .extend([I::I32Const(0), I::Else, I::I32Const(1), I::End]);
    g.finish(vec![ValType::I32])
}

/// `(rx) -> T`: `future::read` (§ 12.12). Reads the value, waiting when the read is
/// blocked, and releases the end. A future whose writer has left without a value has
/// trapped in the writer; here it is `unreachable`.
pub(super) fn future_read(cx: &mut ModuleCx, elem: Type) -> (Vec<ValType>, Vec<ValType>, Function) {
    let types = cx.types;
    let (size, align) = size_align(types, elem);
    let rt = cx.ends[&(false, elem)];
    let val = cx.val(elem);
    let mut g = Gen::new(cx, vec![ValType::I32]);
    let (code, value) = (g.local(ValType::I32), g.local(val));
    let saved = save_top(&mut g);
    let ptr = alloc(&mut g, align, &[I::I32Const(size as i32)]);
    g.code.extend([
        I::LocalGet(0),
        I::LocalGet(ptr),
        I::Call(rt.read),
        I::LocalSet(code),
    ]);
    wait_if_blocked(&mut g, 0, code);
    g.code.extend([I::LocalGet(0), I::Call(rt.drop_readable)]);
    dropped(&mut g, code);
    g.code
        .extend([I::If(BlockType::Empty), I::Unreachable, I::End]);
    g.lift_at(elem, ptr, 0);
    g.emit(I::LocalSet(value));
    restore_top(&mut g, saved);
    g.code.extend(getter(value, val));
    g.finish(vec![val])
}
