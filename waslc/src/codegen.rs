//! Generation of the core WebAssembly module of a world (§ 12, § 12).

use std::borrow::Cow;
use std::collections::HashMap;

use wasm_encoder::{
    ArrayType, BlockType, CodeSection, CompositeInnerType, CompositeType, ConstExpr,
    DataCountSection, DataSection, EntityType, ExportKind, ExportSection, FieldType, Function,
    FunctionSection, GlobalSection, GlobalType, HeapType, Ieee32, Ieee64, ImportSection,
    Instruction, MemorySection, MemoryType, Module, RefType, StorageType, StructType, SubType,
    TypeSection, ValType,
};

use crate::ast::{BinOp, UnaryOp};
use crate::diag::{Code, Diagnostic, Result};
use crate::hir::{
    Arith, Builtin, Const, Expr, ExprKind, FuncDef, FuncId, FuncKind, GlobalId, MatchArm, Place,
    Program, Stmt, WorldDef, visit_expr, visit_stmts,
};
use crate::types::{DeclId, DeclKind, Prim, Type, TypeKind, Types};
use crate::wit::{InterfaceName, kebab};

mod boundary;
mod helpers;

use boundary::{Boundary, EndRt, WaitRt};
use helpers::Helper;

/// String literals longer than this are built from a data segment rather than with
/// `array.new_fixed` (§ 12.8).
const MAX_FIXED: usize = 1000;

/// Produces the core module of world `world`: its exported functions, the functions they
/// call transitively, the globals they use and the helpers they need.
///
/// `interface_names` gives the WIT names of each interface of the WIT.
pub fn generate(
    program: &Program,
    world: usize,
    interface_names: &[Option<InterfaceName>],
) -> Result<Vec<u8>> {
    let (used_funcs, imported) = program.reach(world);
    let world = &program.worlds[world];
    let types = &program.types;

    // Globals used by the functions reachable from the exports.
    let mut used_globals = vec![false; program.globals.len()];
    for (f, func) in program.funcs.iter().enumerate() {
        if used_funcs[f] {
            visit_stmts(&func.body, &mut |e| {
                if let ExprKind::Global(g) = e.kind {
                    used_globals[g] = true;
                }
            });
            mark_assigned_globals(&func.body, &mut used_globals);
        }
    }

    // The resources of the exported interfaces, which other components reach through
    // handles (§ 13.11).
    let resources: Vec<DeclId> = world
        .exports
        .iter()
        .flat_map(|&i| program.interfaces[i].types.iter().copied())
        .filter(|&d| matches!(types.decls[d].kind, DeclKind::Resource(_)))
        .collect();

    // Handles (§ 12.10): the resources of the interfaces of other packages and of the
    // interfaces the world imports. Each one that the module uses needs its
    // `resource.drop`.
    let handle_ifaces: Vec<bool> = program
        .interfaces
        .iter()
        .enumerate()
        .map(|(i, iface)| iface.foreign.is_some() || world.imports.contains(&i))
        .collect();
    let mut handles: Vec<DeclId> = Vec::new();
    {
        let mut note = |ty: Type| {
            for d in types.reachable_decls(ty) {
                if matches!(types.decls[d].kind, DeclKind::Resource(_))
                    && handle_ifaces[types.decls[d].interface]
                    && !handles.contains(&d)
                {
                    handles.push(d);
                }
            }
        };
        for (f, func) in program.funcs.iter().enumerate() {
            if used_funcs[f] || imported.contains(&f) {
                func.locals
                    .iter()
                    .chain(func.params.iter().map(|(_, t)| t))
                    .chain(&func.ret)
                    .for_each(|&t| note(t));
                visit_stmts(&func.body, &mut |e| {
                    if let Some(t) = e.ty {
                        note(t)
                    }
                });
            }
        }
    }
    handles.sort();

    // Streams and futures (§ 12.12): the stream and future types that the module uses,
    // by element type. Each one needs the intrinsics of its type.
    let mut ends: Vec<(bool, Type)> = Vec::new();
    for (f, func) in program.funcs.iter().enumerate() {
        if used_funcs[f] || imported.contains(&f) {
            let mut tys: Vec<Type> = func
                .locals
                .iter()
                .chain(func.params.iter().map(|(_, t)| t))
                .chain(&func.ret)
                .copied()
                .collect();
            visit_stmts(&func.body, &mut |e| tys.extend(e.ty));
            // The types of the package can be recursive (§ 7.10): each one is seen once.
            let mut seen = Vec::new();
            while let Some(ty) = tys.pop() {
                if seen.contains(&ty) {
                    continue;
                }
                seen.push(ty);
                if let Some((kind, elem)) = types.end(ty)
                    && !ends.contains(&(kind.is_stream(), elem))
                {
                    ends.push((kind.is_stream(), elem));
                }
                tys.extend(types.children(ty));
            }
        }
    }
    let carriers = ends
        .iter()
        .map(|&key| carrier(program, world, interface_names, key))
        .collect::<Result<Vec<_>>>()?;

    let exported: Vec<FuncId> = world
        .exports
        .iter()
        .flat_map(|&i| program.interfaces[i].funcs.iter().copied())
        .collect();
    let async_exports: Vec<FuncId> = exported
        .iter()
        .copied()
        .filter(|&f| program.funcs[f].is_async)
        .collect();

    // Imported functions come first in the function index space (§ 13.10), then the
    // intrinsics of the resources: `new`, `rep` and `drop`, then the `drop` of the
    // handles, then the `task.return` of the async exports (§ 13.13), then the
    // intrinsics of the streams and futures and those of the waitable set (§ 12.12).
    let waitable_count = if ends.is_empty() {
        0
    } else {
        WAITABLE_INTRINSICS.len()
    };
    let import_count = (imported.len()
        + 3 * resources.len()
        + handles.len()
        + async_exports.len()
        + END_INTRINSICS.len() * ends.len()
        + waitable_count) as u32;
    let mut func_index = index_map(&used_funcs);
    for index in func_index.iter_mut().flatten() {
        *index += import_count;
    }
    for (k, &f) in imported.iter().enumerate() {
        func_index[f] = Some(k as u32);
    }
    let defined = used_funcs.iter().filter(|&&u| u).count() as u32;
    let mut cx = ModuleCx::new(program, func_index);
    cx.helper_base = import_count + defined;
    cx.imports = imported.clone();
    cx.handle_ifaces = handle_ifaces;
    let first_drop = (imported.len() + 3 * resources.len()) as u32;
    for (k, &d) in handles.iter().enumerate() {
        cx.drops.insert(d, first_drop + k as u32);
    }
    let first_task_return = first_drop + handles.len() as u32;
    for (k, &f) in async_exports.iter().enumerate() {
        cx.task_returns.insert(f, first_task_return + k as u32);
    }
    let first_end = first_task_return + async_exports.len() as u32;
    for (k, &key) in ends.iter().enumerate() {
        let first = first_end + (END_INTRINSICS.len() * k) as u32;
        cx.ends.insert(
            key,
            EndRt {
                new: first,
                write: first + 1,
                read: first + 2,
                drop_readable: first + 3,
                drop_writable: first + 4,
            },
        );
    }
    let first_waitable = first_end + (END_INTRINSICS.len() * ends.len()) as u32;
    // The operations on streams and futures copy through linear memory.
    let memory = !ends.is_empty()
        || exported
            .iter()
            .any(|&f| boundary::export_needs_memory(types, &program.funcs[f]))
        || imported
            .iter()
            .any(|&f| boundary::import_needs_memory(types, &program.funcs[f]));

    // Shared immutable values (§ 12.8, § 12.6): string literals and cases without
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
    // The top of the scratch buffer (§ 13.7) comes after the program's globals, then the
    // bottom of the scratch memory of the sync tasks (§ 13.13), then the rep tables of
    // the resources.
    cx.scratch_top = memory.then_some(next);
    if memory {
        next += 1;
    }
    if memory && !async_exports.is_empty() {
        cx.scratch_base = Some(next);
        next += 1;
    }
    // The waitable set of the async task (§ 12.12).
    if !ends.is_empty() {
        cx.waitable = Some(WaitRt {
            set_new: first_waitable,
            join: first_waitable + 1,
            wait: first_waitable + 2,
            set_drop: first_waitable + 3,
            set: next,
        });
        next += 1;
    }
    for (k, &decl) in resources.iter().enumerate() {
        let first = imported.len() as u32 + 3 * k as u32;
        cx.resources.insert(
            decl,
            ResourceRt {
                new: first,
                rep: first + 1,
                drop: first + 2,
                reps: next,
                frees: next + 1,
                nfree: next + 2,
                used: next + 3,
            },
        );
        next += 4;
    }

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
    // Core exports (§ 13.8): an adapter for each function that takes or returns a heap
    // value, the function itself otherwise. An async function always has an adapter,
    // with the callback of its lift (§ 13.13).
    let mut core_exports: Vec<(FuncId, u32, Option<u32>)> = Vec::new();
    let mut async_core_exports: Vec<(FuncId, u32, u32)> = Vec::new();
    for &f in &exported {
        let func = &program.funcs[f];
        if func.is_async {
            let adapter = cx.helper(Helper::Boundary(Boundary::AsyncAdapter(f)));
            let callback = cx.helper(Helper::Boundary(Boundary::Callback));
            async_core_exports.push((f, adapter, callback));
        } else if boundary::needs_adapter(types, func) {
            let adapter = cx.helper(Helper::Boundary(Boundary::Adapter(f)));
            let post_return = boundary::result_in_memory(types, func)
                .then(|| cx.helper(Helper::Boundary(Boundary::PostReturn)));
            core_exports.push((f, adapter, post_return));
        } else {
            core_exports.push((f, cx.func_index[f].unwrap(), None));
        }
    }
    let realloc = memory.then(|| cx.helper(Helper::Boundary(Boundary::Realloc)));
    let dtors: Vec<(DeclId, u32)> = resources
        .iter()
        .map(|&d| (d, cx.helper(Helper::Boundary(Boundary::Dtor(d)))))
        .collect();
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
    let wset = cx.waitable.map(|w| w.set);
    for global in [cx.scratch_top, cx.scratch_base, wset]
        .into_iter()
        .flatten()
    {
        debug_assert_eq!(global as usize, globals.len() as usize);
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i32_const(0),
        );
    }
    for &decl in &resources {
        let rt = cx.resources[&decl];
        debug_assert_eq!(rt.reps as usize, globals.len() as usize);
        let object = cx.heap(types.decl_type_of(decl));
        let reps = cx.array(StorageType::Val(ref_type(object, true)));
        let frees = cx.array(StorageType::Val(ValType::I32));
        for array in [reps, frees] {
            globals.global(
                GlobalType {
                    val_type: ref_type(array, true),
                    mutable: true,
                    shared: false,
                },
                &ConstExpr::ref_null(HeapType::Concrete(array)),
            );
        }
        for _ in 0..2 {
            globals.global(
                GlobalType {
                    val_type: ValType::I32,
                    mutable: true,
                    shared: false,
                },
                &ConstExpr::i32_const(0),
            );
        }
    }

    // Imports (§ 13.10), with the standard names of the Component Model.
    let mut import_section = ImportSection::new();
    let mut import_sigs = Vec::new();
    for &f in &imported {
        let func = &program.funcs[f];
        let (params, results) = boundary::import_signature(types, func);
        import_sigs.push(cx.signature(params, results));
    }
    let drop_sig = cx.signature(vec![ValType::I32], vec![]);
    let task_return_sigs: Vec<u32> = async_exports
        .iter()
        .map(|&f| {
            let params = boundary::task_return_params(types, &program.funcs[f]);
            cx.signature(params, Vec::new())
        })
        .collect();
    let end_sigs: Vec<[u32; 5]> = ends
        .iter()
        .map(|&(stream, _)| {
            let copy = if stream { 3 } else { 2 };
            [
                cx.signature(vec![], vec![ValType::I64]),
                cx.signature(vec![ValType::I32; copy], vec![ValType::I32]),
                cx.signature(vec![ValType::I32; copy], vec![ValType::I32]),
                drop_sig,
                drop_sig,
            ]
        })
        .collect();
    let waitable_sigs = [
        cx.signature(vec![], vec![ValType::I32]),
        cx.signature(vec![ValType::I32; 2], vec![]),
        cx.signature(vec![ValType::I32; 2], vec![ValType::I32]),
        drop_sig,
    ];
    let intrinsic_sigs: Vec<[u32; 3]> = resources
        .iter()
        .map(|_| {
            [
                cx.signature(vec![ValType::I32], vec![ValType::I32]),
                cx.signature(vec![ValType::I32], vec![ValType::I32]),
                cx.signature(vec![ValType::I32], vec![]),
            ]
        })
        .collect();

    // Standard Component Model export names (§ 13.3, § 13.6).
    let mut exports = ExportSection::new();
    for (f, index, post_return) in core_exports {
        let func = &program.funcs[f];
        let path = interface_names[func.interface]
            .as_ref()
            .map(|n| &n.canonical)
            .expect("exported interface present in the WIT");
        let name = format!("cm32p2|{path}|{}", wit_func_name(types, func));
        exports.export(&name, ExportKind::Func, index);
        if let Some(post_return) = post_return {
            exports.export(&format!("{name}_post"), ExportKind::Func, post_return);
        }
    }
    // Async exports (§ 13.13) have no standard name yet: they take the names that
    // `wit-component` reads, with the full name of the interface.
    for &(f, adapter, callback) in &async_core_exports {
        let func = &program.funcs[f];
        let path = &interface_names[func.interface]
            .as_ref()
            .expect("exported interface present in the WIT")
            .full;
        let name = format!("{path}#{}", wit_func_name(types, func));
        exports.export(&format!("[async-lift]{name}"), ExportKind::Func, adapter);
        exports.export(
            &format!("[callback][async-lift]{name}"),
            ExportKind::Func,
            callback,
        );
    }
    for &(decl, dtor) in &dtors {
        let path = interface_names[types.decls[decl].interface]
            .as_ref()
            .map(|n| &n.canonical)
            .expect("exported interface present in the WIT");
        let name = kebab(&types.decls[decl].name);
        exports.export(
            &format!("cm32p2|{path}|{name}_dtor"),
            ExportKind::Func,
            dtor,
        );
    }
    let mut memories = MemorySection::new();
    if let Some(realloc) = realloc {
        memories.memory(MemoryType {
            minimum: 0,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        exports.export("cm32p2_memory", ExportKind::Memory, 0);
        exports.export("cm32p2_realloc", ExportKind::Func, realloc);
    }

    // Types: the GC types in one recursion group, then the function types (§ 12.1).
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
    for (&f, sig) in imported.iter().zip(import_sigs) {
        let func = &program.funcs[f];
        let path = interface_names[func.interface]
            .as_ref()
            .map(|n| &n.canonical)
            .expect("imported interface present in the WIT");
        import_section.import(
            &format!("cm32p2|{path}"),
            &wit_func_name(types, func),
            EntityType::Function(gc_types + sig),
        );
    }
    for (&decl, sigs) in resources.iter().zip(intrinsic_sigs) {
        let path = interface_names[types.decls[decl].interface]
            .as_ref()
            .map(|n| &n.canonical)
            .expect("exported interface present in the WIT");
        let name = kebab(&types.decls[decl].name);
        for (suffix, sig) in ["new", "rep", "drop"].iter().zip(sigs) {
            import_section.import(
                &format!("cm32p2|_ex_{path}"),
                &format!("{name}_{suffix}"),
                EntityType::Function(gc_types + sig),
            );
        }
    }
    for &decl in &handles {
        let path = interface_names[types.decls[decl].interface]
            .as_ref()
            .map(|n| &n.canonical)
            .expect("imported interface present in the WIT");
        import_section.import(
            &format!("cm32p2|{path}"),
            &format!("{}_drop", kebab(&types.decls[decl].name)),
            EntityType::Function(gc_types + drop_sig),
        );
    }
    for (&f, sig) in async_exports.iter().zip(task_return_sigs) {
        let func = &program.funcs[f];
        let path = &interface_names[func.interface]
            .as_ref()
            .expect("exported interface present in the WIT")
            .full;
        import_section.import(
            &format!("[export]{path}"),
            &format!("[task-return]{}", wit_func_name(types, func)),
            EntityType::Function(gc_types + sig),
        );
    }
    // The intrinsics of streams and futures take the name of a function of the WIT that
    // carries their type, with its index among the streams and futures of that function
    // (§ 13.13).
    for (((stream, _), carrier), sigs) in ends.iter().zip(&carriers).zip(end_sigs) {
        let kind = if *stream { "stream" } else { "future" };
        let Carrier {
            module,
            func,
            index,
        } = carrier;
        for (name, sig) in END_INTRINSICS.iter().zip(sigs) {
            import_section.import(
                module,
                &format!("{}{kind}-{name}-{index}]{func}", lowering(name)),
                EntityType::Function(gc_types + sig),
            );
        }
    }
    if !ends.is_empty() {
        for (name, sig) in WAITABLE_INTRINSICS.iter().zip(waitable_sigs) {
            import_section.import("$root", name, EntityType::Function(gc_types + sig));
        }
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
    if import_count > 0 {
        module.section(&import_section);
    }
    module.section(&functions);
    if realloc.is_some() {
        module.section(&memories);
    }
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

/// The intrinsics of a stream or future type, in index order: `new`, `write`, `read`,
/// `drop-readable`, `drop-writable` (§ 13.13).
const END_INTRINSICS: [&str; 5] = ["new", "write", "read", "drop-readable", "drop-writable"];

/// The intrinsics of the waitable set, in index order (§ 12.12).
const WAITABLE_INTRINSICS: [&str; 4] = [
    "[waitable-set-new]",
    "[waitable-join]",
    "[waitable-set-wait]",
    "[waitable-set-drop]",
];

/// The start of the name of an intrinsic of streams and futures: the copies are lowered
/// `async`, and wait in the waitable set of the task (§ 12.12).
fn lowering(name: &str) -> &'static str {
    match name {
        "write" | "read" => "[async-lower][",
        _ => "[",
    }
}

/// The stream and future types in `ty`, a type of the WIT and therefore not recursive
/// (E0208), as `(is a stream, element type)`, in the order of `wit-parser`: the elements
/// of an end before the end itself.
fn end_keys(types: &Types, ty: Type) -> Vec<(bool, Type)> {
    let mut keys = Vec::new();
    for child in types.children(ty) {
        keys.extend(end_keys(types, child));
    }
    if let Some((kind, elem)) = types.end(ty) {
        keys.push((kind.is_stream(), elem));
    }
    keys
}

/// A function of the WIT of the world whose signature holds a stream or future type: the
/// module and name under which the core module imports the intrinsics of that type.
struct Carrier {
    module: String,
    func: String,
    index: usize,
}

/// The first function of the world that carries the stream or future type `key`: in an
/// imported interface, then in an exported one. E0223 guarantees there is one.
fn carrier(
    program: &Program,
    world: &WorldDef,
    interface_names: &[Option<InterfaceName>],
    key: (bool, Type),
) -> Result<Carrier> {
    let types = &program.types;
    let imports = world.imports.iter().map(|&i| (i, ""));
    let exports = world.exports.iter().map(|&i| (i, "[export]"));
    for (iface, prefix) in imports.chain(exports) {
        let Some(name) = &interface_names[iface] else {
            continue;
        };
        for &f in &program.interfaces[iface].funcs {
            let func = &program.funcs[f];
            let keys: Vec<(bool, Type)> = func
                .params
                .iter()
                .map(|(_, t)| *t)
                .chain(func.ret)
                .flat_map(|t| end_keys(types, t))
                .collect();
            if let Some(index) = keys.iter().position(|&k| k == key) {
                return Ok(Carrier {
                    module: format!("{prefix}{}", name.full),
                    func: wit_func_name(types, func),
                    index,
                });
            }
        }
    }
    Err(Diagnostic::without_span(
        Code::Internal,
        format!(
            "internal error: no function of world `{}` carries the stream or future type of `{}`",
            world.name,
            types.display(key.1)
        ),
    ))
}

/// The name of a function in the core module (§ 13.3): the WIT name of a free function,
/// `[constructor]r`, `[method]r.m` or `[static]r.s` for the functions of a resource.
fn wit_func_name(types: &Types, func: &FuncDef) -> String {
    let resource = |d: DeclId| kebab(&types.decls[d].name);
    match func.kind {
        FuncKind::Free => kebab(&func.name),
        FuncKind::Constructor(d) => format!("[constructor]{}", resource(d)),
        FuncKind::Method(d) => format!("[method]{}.{}", resource(d), kebab(&func.name)),
        FuncKind::Static(d) => format!("[static]{}.{}", resource(d), kebab(&func.name)),
    }
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
    program: &'a Program,
    types: &'a Types,
    /// The GC types, in one recursion group (§ 12.5).
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
    /// Functions of other components that the module imports, in index order.
    imports: Vec<FuncId>,
    helpers: Vec<Helper>,
    helper_index: HashMap<Helper, u32>,

    /// Globals of the string literals and of the cases without payload: type and
    /// initializer.
    shared_globals: Vec<(ValType, Vec<Instruction<'static>>)>,
    strings: HashMap<String, u32>,
    shared_cases: HashMap<(Type, u32), u32>,
    global_index: Vec<Option<u32>>,
    /// The global `$scratch_top` (§ 13.7), when the module has a memory.
    scratch_top: Option<u32>,
    /// Passive data segments of the long string literals.
    data: Vec<Vec<u8>>,
    data_index: HashMap<String, u32>,
    /// Index of the first helper function.
    helper_base: u32,
    /// The runtime of each exported resource (§ 12.10).
    resources: HashMap<DeclId, ResourceRt>,
    /// The interfaces whose resources are handles in this module: those of other
    /// packages and those the world imports (§ 12.10).
    handle_ifaces: Vec<bool>,
    /// The `resource.drop` import of each handle type used by the module.
    drops: HashMap<DeclId, u32>,
    /// The `task.return` import of each exported async function (§ 13.13).
    task_returns: HashMap<FuncId, u32>,
    /// The global `$scratch_base`, when the module has a memory and async exports: the
    /// bottom of the scratch memory of the sync tasks, above the data of the async task
    /// that waits (§ 13.13).
    scratch_base: Option<u32>,
    /// The intrinsics of each stream or future type the module uses, by `(is a stream,
    /// element type)` (§ 12.12).
    ends: HashMap<(bool, Type), EndRt>,
    /// The waitable set of the async task, when the module uses streams or futures.
    waitable: Option<WaitRt>,
}

/// The functions and globals behind the handles of an exported resource (§ 12.10): the
/// intrinsics `new`, `rep` and `drop` of the Component Model, the table of the objects
/// that have a handle (`reps`), and the stack of its free slots (`frees`, `nfree`).
/// `used` is the number of slots handed out so far.
#[derive(Clone, Copy)]
struct ResourceRt {
    new: u32,
    rep: u32,
    drop: u32,
    reps: u32,
    frees: u32,
    nfree: u32,
    used: u32,
}

impl<'a> ModuleCx<'a> {
    fn new(program: &'a Program, func_index: Vec<Option<u32>>) -> Self {
        ModuleCx {
            program,
            types: &program.types,
            rec: Vec::new(),
            heap: HashMap::new(),
            arrays: HashMap::new(),
            structs: HashMap::new(),
            cases: HashMap::new(),
            signatures: Vec::new(),
            func_index,
            imports: Vec::new(),
            helpers: Vec::new(),
            helper_index: HashMap::new(),
            shared_globals: Vec::new(),
            strings: HashMap::new(),
            shared_cases: HashMap::new(),
            global_index: Vec::new(),
            scratch_top: None,
            data: Vec::new(),
            data_index: HashMap::new(),
            helper_base: 0,
            resources: HashMap::new(),
            handle_ifaces: Vec::new(),
            drops: HashMap::new(),
            task_returns: HashMap::new(),
            scratch_base: None,
            ends: HashMap::new(),
            waitable: None,
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

    /// Wasm value type of a WaSL type (§ 12.5).
    fn val(&mut self, ty: Type) -> ValType {
        let types = self.types;
        match types.kind(ty) {
            TypeKind::Prim(p) => p.val_type(),
            _ if types.is_scalar(ty) || self.is_handle(ty).is_some() || self.is_end(ty) => {
                ValType::I32
            }
            TypeKind::Option(inner) => {
                let index = self.option_heap(*inner);
                ref_type(index, true)
            }
            _ => ref_type(self.heap(ty), false),
        }
    }

    /// Whether `option<inner>` boxes its payload (§ 12.7).
    fn boxed(&self, inner: Type) -> bool {
        self.types.is_scalar(inner)
            || self.is_handle(inner).is_some()
            || self.is_end(inner)
            || matches!(self.types.kind(inner), TypeKind::Option(_))
    }

    /// The resource of `ty`, `own` or `borrow`, when it is a handle in this module: an
    /// `i32` index in the handle table of the instance (§ 12.10).
    fn is_handle(&self, ty: Type) -> Option<DeclId> {
        let (d, _) = self.types.resource(ty)?;
        self.handle_ifaces
            .get(self.types.decls[d].interface)
            .copied()
            .unwrap_or(false)
            .then_some(d)
    }

    /// Whether `ty` is an end of a stream or a future (§ 7.13): an `i32` handle.
    fn is_end(&self, ty: Type) -> bool {
        self.types.end(ty).is_some()
    }

    /// Whether `ty` is an owned handle, which its owner releases: a resource of another
    /// component, or an end of a stream or a future.
    fn owned_handle(&self, ty: Type) -> bool {
        match self.types.kind(ty) {
            TypeKind::Borrow(_) => false,
            TypeKind::End(..) => true,
            _ => self.is_handle(ty).is_some(),
        }
    }

    /// The function that releases a non-zero owned handle of type `ty` (§ 9.9): the
    /// `resource.drop` of its resource, or the helper that releases an end.
    fn release_func(&mut self, ty: Type) -> u32 {
        match self.types.kind(ty) {
            TypeKind::End(..) => self.helper(Helper::Boundary(Boundary::ReleaseEnd(ty))),
            _ => self.drops[&self.is_handle(ty).expect("a handle")],
        }
    }

    /// Whether a value of type `ty` holds owned handles.
    fn has_handles(&self, ty: Type) -> bool {
        let mut seen = Vec::new();
        let mut stack = vec![ty];
        while let Some(t) = stack.pop() {
            if seen.contains(&t) {
                continue;
            }
            seen.push(t);
            if self.owned_handle(t) {
                return true;
            }
            stack.extend(self.types.children(t));
        }
        false
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

    /// Storage type of the elements of a list (§ 12.5).
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
                DeclKind::Record(fields)
                | DeclKind::Resource(crate::types::ResourceDef { fields, .. }) => {
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
            TypeKind::Borrow(inner) => return self.heap(*inner),
            TypeKind::Prim(_) | TypeKind::Char | TypeKind::Option(_) | TypeKind::End(..) => {
                unreachable!("not a heap type")
            }
        };
        self.heap.insert(ty, index);
        index
    }

    /// The types of a variant or result (§ 12.6): the tag struct, then one subtype per
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
            ExprKind::Try { expr, ret } => {
                // `?` on a result returns the `err` of the function's result type: without
                // a payload, its shared instance.
                if let TypeKind::Result { err: None, .. } = self.types.kind(*ret)
                    && let TypeKind::Result { .. } = self.types.kind(expr.value_ty())
                    && !self.shared_cases.contains_key(&(*ret, 1))
                {
                    let base = self.heap(*ret);
                    let init = vec![Instruction::I32Const(1), Instruction::StructNew(base)];
                    let g = self.shared_global(ref_type(base, false), init);
                    self.shared_cases.insert((*ret, 1), g);
                }
            }
            _ => {}
        }
    }

    fn shared_global(&mut self, ty: ValType, init: Vec<Instruction<'static>>) -> u32 {
        self.shared_globals.push((ty, init));
        self.shared_globals.len() as u32 - 1
    }

    /// The function a call to `f` calls: the reverse adapter of an imported function
    /// that needs one (§ 13.10), or of an async import that may let sync tasks run
    /// (§ 13.13); the function itself otherwise.
    fn call_index(&mut self, f: FuncId) -> u32 {
        let func = &self.program.funcs[f];
        if self.imports.contains(&f)
            && (boundary::import_needs_adapter(self.types, func)
                || (func.is_async && self.scratch_base.is_some()))
        {
            return self.helper(Helper::Boundary(Boundary::Import(f)));
        }
        self.func_index[f].expect("used function")
    }

    /// Index of a helper function; its body is generated after the program's functions.
    fn helper(&mut self, helper: Helper) -> u32 {
        if let Some(&i) = self.helper_index.get(&helper) {
            return i;
        }
        let i = self.helper_base + self.helpers.len() as u32;
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
    /// The locals that own a handle, by block, innermost last (§ 9.9).
    owners: Vec<Vec<u32>>,
    /// The number of blocks open at the start of each enclosing loop body.
    loops: Vec<usize>,
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
            owners: Vec::new(),
            loops: Vec::new(),
        };
        // The parameters that own a handle are released on every exit (§ 9.9).
        let params: Vec<u32> = func
            .params
            .iter()
            .enumerate()
            .filter(|(_, (_, t))| cg.cx.owned_handle(*t))
            .map(|(i, _)| i as u32)
            .collect();
        cg.owners.push(params);
        cg.block(&func.body);
        cg.release_from(0);
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
            owners: Vec::new(),
            loops: Vec::new(),
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

    /// Pushes a scratch local holding a value of Wasm type `val`, non-null if `val` is.
    fn get_scratch(&mut self, s: u32, val: ValType) {
        self.emit(Instruction::LocalGet(s));
        if matches!(val, ValType::Ref(r) if !r.nullable) {
            self.emit(Instruction::RefAsNonNull);
        }
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

    /// A block of statements: the handles of its locals are released at its end.
    fn block(&mut self, stmts: &[Stmt]) {
        self.owners.push(Vec::new());
        self.stmts(stmts);
        self.release_from(self.owners.len() - 1);
        self.owners.pop();
    }

    // ---- Handles (§ 9.9, § 12.10) ----

    /// Releases the handle in local `l`, if any, and empties it.
    fn release_local(&mut self, l: u32) {
        let drop = self.cx.release_func(self.local_types[l as usize]);
        self.instrs.extend([
            Instruction::LocalGet(l),
            Instruction::If(BlockType::Empty),
            Instruction::LocalGet(l),
            Instruction::Call(drop),
            Instruction::End,
            Instruction::I32Const(0),
            Instruction::LocalSet(l),
        ]);
    }

    /// Releases the handles of the blocks from `depth` to the innermost, innermost first,
    /// each in the reverse order of its declarations.
    fn release_from(&mut self, depth: usize) {
        let locals: Vec<u32> = self.owners[depth..]
            .iter()
            .rev()
            .flat_map(|b| b.iter().rev().copied())
            .collect();
        for l in locals {
            self.release_local(l);
        }
    }

    /// Records that local `l`, just declared, owns its value if it is a handle.
    fn own(&mut self, l: u32) {
        if self.cx.owned_handle(self.local_types[l as usize]) {
            self.owners.last_mut().expect("open block").push(l);
        }
    }

    /// Releases the handles of the value of type `ty` on the stack, and consumes it.
    fn release_value(&mut self, ty: Type) {
        if self.cx.has_handles(ty) {
            let helper = self.cx.helper(Helper::Boundary(Boundary::Release(ty)));
            self.emit(Instruction::Call(helper));
        } else {
            self.emit(Instruction::Drop);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Local { local, init } => {
                match init {
                    Some(e) => self.expr(e),
                    None => {
                        // A local may be redeclared on every loop iteration: reset it to the
                        // default value of its Wasm type (§ 9.4).
                        let ty = self.cx.val(self.local_types[*local as usize]);
                        self.zero(ty);
                    }
                }
                self.emit(Instruction::LocalSet(*local));
                self.own(*local);
            }
            Stmt::Destructure { locals, init } => {
                let ty = init.value_ty();
                let TypeKind::Tuple(elems) = self.types().kind(ty) else {
                    unreachable!("checked: a tuple")
                };
                let tuple = self.cx.heap(ty);
                let val = self.cx.val(ty);
                let tmp = self.scratch(val);
                self.expr(init);
                self.emit(Instruction::LocalSet(tmp));
                // In order: each element goes to its local, and an element written `_` is
                // released at once (§ 9.4, § 9.9).
                for (k, (local, &elem)) in locals.iter().zip(elems).enumerate() {
                    if local.is_none() && !self.cx.has_handles(elem) {
                        continue;
                    }
                    self.get_scratch(tmp, val);
                    self.emit(Instruction::StructGet {
                        struct_type_index: tuple,
                        field_index: k as u32,
                    });
                    match local {
                        Some(l) => {
                            self.emit(Instruction::LocalSet(*l));
                            self.own(*l);
                        }
                        None => self.release_value(elem),
                    }
                }
            }
            Stmt::Assign { place, value } => match place {
                Place::Local(l) if self.cx.owned_handle(self.local_types[*l as usize]) => {
                    // The old handle is released once the new value is computed.
                    let tmp = self.scratch(ValType::I32);
                    self.expr(value);
                    self.emit(Instruction::LocalSet(tmp));
                    self.release_local(*l);
                    self.emit(Instruction::LocalGet(tmp));
                    self.emit(Instruction::LocalSet(*l));
                }
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
                self.block(then);
                if !else_.is_empty() {
                    self.emit(Instruction::Else);
                    self.block(else_);
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
                self.loops.push(self.owners.len());
                self.block(body);
                self.loops.pop();
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
            Stmt::Break | Stmt::Continue => {
                let depth = *self.loops.last().expect("inside a loop");
                self.release_from(depth);
                self.branch(if matches!(stmt, Stmt::Break) {
                    Frame::LoopExit
                } else {
                    Frame::LoopStart
                });
            }
            Stmt::Return(value) => {
                if let Some(e) = value {
                    self.expr(e);
                }
                self.release_from(0);
                self.emit(Instruction::Return);
            }
            Stmt::Call(e) => {
                self.expr(e);
                if let Some(ty) = e.ty {
                    self.release_value(ty);
                }
            }
        }
    }

    /// `match` (§ 12.6): one block per arm, plus one for `else`, around a `br_table`
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
            } else if self.cx.has_handles(ty) {
                // A case without a name gives up its payload (§ 9.9).
                self.get_scratch(s, val);
                self.release_value(ty);
            }
            self.owners.push(Vec::new());
            if let Some(binding) = arm.binding {
                self.own(binding);
            }
            self.stmts(&arm.body);
            self.release_from(self.owners.len() - 1);
            self.owners.pop();
            self.branch(Frame::MatchEnd);
        }
        if let Some(else_) = else_ {
            self.emit(Instruction::End);
            self.frames.pop();
            if self.cx.has_handles(ty) {
                self.get_scratch(s, val);
                self.release_value(ty);
            }
            self.block(else_);
        }
        self.emit(Instruction::End);
        self.frames.pop();
    }

    /// `e?` (§ 12.11): leaves the payload of `ok` or `some` on the stack, or returns the
    /// `err` or `none` of the function's return type `ret`.
    fn try_(&mut self, e: &Expr, ret: Type) {
        let types = self.types();
        let ty = e.value_ty();
        self.expr(e);
        let val = self.cx.val(ty);
        let s = self.scratch(val);
        self.emit(Instruction::LocalSet(s));
        self.emit(Instruction::LocalGet(s));
        match types.kind(ty) {
            TypeKind::Result { ok, err } => {
                let base = self.cx.heap(ty);
                self.emit(Instruction::StructGet {
                    struct_type_index: base,
                    field_index: 0,
                });
                self.emit(Instruction::If(BlockType::Empty));
                // Tag 1: `err`. The error is rebuilt in the function's result type, which
                // is a different struct family (§ 12.6).
                match err {
                    Some(_) => {
                        let from = self.cx.case_type(ty, 1);
                        let to = self.cx.case_type(ret, 1);
                        self.emit(Instruction::I32Const(1));
                        self.emit(Instruction::LocalGet(s));
                        self.emit(Instruction::RefCastNonNull(HeapType::Concrete(from)));
                        self.emit(Instruction::StructGet {
                            struct_type_index: from,
                            field_index: 1,
                        });
                        self.emit(Instruction::StructNew(to));
                    }
                    None => {
                        let g = self.cx.shared_cases[&(ret, 1)];
                        self.emit(Instruction::GlobalGet(g));
                    }
                }
                self.release_from(0);
                self.emit(Instruction::Return);
                self.emit(Instruction::End);
                if ok.is_some() {
                    let case = self.cx.case_type(ty, 0);
                    self.emit(Instruction::LocalGet(s));
                    self.emit(Instruction::RefCastNonNull(HeapType::Concrete(case)));
                    self.emit(Instruction::StructGet {
                        struct_type_index: case,
                        field_index: 1,
                    });
                }
            }
            TypeKind::Option(inner) => {
                self.emit(Instruction::RefIsNull);
                self.emit(Instruction::If(BlockType::Empty));
                let TypeKind::Option(ret_inner) = types.kind(ret) else {
                    unreachable!("checked: `?` on an option returns an option")
                };
                let none = self.cx.option_heap(*ret_inner);
                self.emit(Instruction::RefNull(HeapType::Concrete(none)));
                self.release_from(0);
                self.emit(Instruction::Return);
                self.emit(Instruction::End);
                self.emit(Instruction::LocalGet(s));
                if self.cx.boxed(*inner) {
                    let boxed = self.cx.option_heap(*inner);
                    self.emit(Instruction::StructGet {
                        struct_type_index: boxed,
                        field_index: 0,
                    });
                } else {
                    self.emit(Instruction::RefAsNonNull);
                }
            }
            _ => unreachable!("checked: `?` applies to a result or an option"),
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
        self.cx.global_index[g].expect("used global")
    }

    /// The default value of a Wasm local of type `ty`: zero, or null for an `option`.
    fn zero(&mut self, ty: ValType) {
        self.emit(match ty {
            ValType::I32 => Instruction::I32Const(0),
            ValType::I64 => Instruction::I64Const(0),
            ValType::F32 => Instruction::F32Const(Ieee32::from(0.0f32)),
            ValType::F64 => Instruction::F64Const(Ieee64::from(0.0f64)),
            ValType::Ref(r) if r.nullable => Instruction::RefNull(r.heap_type),
            _ => unreachable!("locals of non-nullable reference types are initialized"),
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
            ExprKind::Move(l) => {
                self.emit(Instruction::LocalGet(*l));
                if self.cx.owned_handle(self.local_types[*l as usize]) {
                    // The handle leaves the local (§ 9.9).
                    self.emit(Instruction::I32Const(0));
                    self.emit(Instruction::LocalSet(*l));
                }
            }
            ExprKind::Global(g) => self.emit(Instruction::GlobalGet(self.global(*g))),
            ExprKind::Call { func, args } => {
                // A handle created only to be lent is released after the call (§ 9.9).
                let params = &self.cx.program.funcs[*func].params;
                let mut lent = Vec::new();
                for (arg, (_, param)) in args.iter().zip(params) {
                    self.expr(arg);
                    let temporary = !matches!(arg.kind, ExprKind::Local(_));
                    if temporary
                        && matches!(self.types().kind(*param), TypeKind::Borrow(_))
                        && self.cx.is_handle(arg.value_ty()).is_some()
                    {
                        let tmp = self.scratch(ValType::I32);
                        self.emit(Instruction::LocalTee(tmp));
                        lent.push((tmp, self.cx.is_handle(arg.value_ty()).unwrap()));
                    }
                }
                let index = self.cx.call_index(*func);
                self.emit(Instruction::Call(index));
                for (tmp, d) in lent {
                    let drop = self.cx.drops[&d];
                    self.emit(Instruction::LocalGet(tmp));
                    self.emit(Instruction::Call(drop));
                }
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
            ExprKind::Try { expr, ret } => self.try_(expr, *ret),
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
            ExprKind::Builtin { func, args } if func.is_end() => {
                self.end_builtin(*func, args, e.ty)
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
        match func {
            Builtin::Wrapping(Arith::Neg) => {
                self.zero(ty.val_type());
                self.expr(&args[0]);
                self.emit(binary_instr(BinOp::Sub, ty));
                self.normalize(ty);
                return;
            }
            Builtin::Wrapping(op) => {
                self.expr(&args[0]);
                self.expr(&args[1]);
                self.emit(binary_instr(arith_op(op), ty));
                self.normalize(ty);
                return;
            }
            Builtin::Checked(op) => {
                // `option::none` on overflow, `option::some(r)` otherwise.
                let int = args[0].value_ty();
                self.expr(&args[0]);
                self.expr(&args[1]);
                let r = self.arith_overflow(op, int);
                let boxed = self.cx.option_heap(int);
                self.emit(Instruction::If(BlockType::Result(ref_type(boxed, true))));
                self.emit(Instruction::RefNull(HeapType::Concrete(boxed)));
                self.emit(Instruction::Else);
                self.emit(Instruction::LocalGet(r));
                self.emit(Instruction::StructNew(boxed));
                self.emit(Instruction::End);
                return;
            }
            Builtin::WrappingFrom => {
                self.expr(&args[0]);
                self.wrap_int(args[0].value_ty(), ty, true);
                return;
            }
            _ => {}
        }
        for arg in args {
            self.expr(arg);
        }
        let helper = match func {
            Builtin::StringLen | Builtin::ListLen => {
                self.emit(Instruction::ArrayLen);
                return;
            }
            // A string never changes: its bytes are the same array (§ 8.17).
            Builtin::StringBytes => return,
            Builtin::StringFromBytes => Helper::FromBytes,
            Builtin::StringChars => Helper::Chars,
            Builtin::StringFromChars => Helper::FromChars,
            Builtin::CharFromU32 => Helper::CharFromU32,
            Builtin::ListCopy | Builtin::ListFreeze => Helper::Copy(self.cx.heap(ty)),
            Builtin::ListFilled
            | Builtin::Wrapping(_)
            | Builtin::Checked(_)
            | Builtin::WrappingFrom => unreachable!("generated above"),
            Builtin::EndNew
            | Builtin::StreamWrite
            | Builtin::StreamRead
            | Builtin::StreamClose
            | Builtin::FutureWrite
            | Builtin::FutureRead => unreachable!("functions of streams and futures"),
        };
        let index = self.cx.helper(helper);
        self.emit(Instruction::Call(index));
    }

    /// The functions of streams and futures (§ 12.12), whose value has type `ty`.
    fn end_builtin(&mut self, func: Builtin, args: &[Expr], ty: Option<Type>) {
        let helper = match func {
            Builtin::EndNew => Boundary::EndNew(ty.expect("a tuple of ends")),
            // `stream::write` and `stream::read` borrow their end: the helper returns it
            // after the value, `-1` once the other end has left (§ 12.12).
            Builtin::StreamWrite | Builtin::StreamRead => {
                for arg in args {
                    self.expr(arg);
                }
                let helper = match func {
                    Builtin::StreamWrite => Boundary::StreamWrite(args[1].value_ty()),
                    _ => Boundary::StreamRead(ty.expect("an option of a list")),
                };
                let index = self.cx.helper(Helper::Boundary(helper));
                self.emit(Instruction::Call(index));
                match args[0].kind {
                    ExprKind::Local(l) => self.emit(Instruction::LocalSet(l)),
                    // An end created only for this call is released after it.
                    _ => self.release_value(args[0].value_ty()),
                }
                return;
            }
            Builtin::StreamClose => Boundary::ReleaseEnd(args[0].value_ty()),
            Builtin::FutureWrite => Boundary::FutureWrite(args[1].value_ty()),
            Builtin::FutureRead => Boundary::FutureRead(ty.expect("the value of a future")),
            _ => unreachable!("not a function of streams and futures"),
        };
        for arg in args {
            self.expr(arg);
        }
        let index = self.cx.helper(Helper::Boundary(helper));
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
                    self.expr(operand);
                    self.trapping_arith(Arith::Neg, ty);
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
        if ty.is_int() {
            let arith = match op {
                BinOp::Add => Some(Arith::Add),
                BinOp::Sub => Some(Arith::Sub),
                BinOp::Mul => Some(Arith::Mul),
                _ => None,
            };
            if let Some(arith) = arith {
                self.trapping_arith(arith, ty);
                return;
            }
        }
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

    /// `+`, `-`, `*` or unary `-` on an integer type, with the operands on the stack: the
    /// exact result, or a trap (§ 10.2).
    fn trapping_arith(&mut self, op: Arith, ty: Type) {
        let r = self.arith_overflow(op, ty);
        self.trap_if_overflow();
        self.emit(Instruction::LocalGet(r));
    }

    /// Traps with `integer overflow` if the `i32` on the stack is not 0, by dividing
    /// `i32::MIN` by -1 as for the division of § 12.2 (§ 12.9). The trap is inline: a call
    /// to a helper would make the hot path save its registers around the call.
    fn trap_if_overflow(&mut self) {
        self.emit(Instruction::If(BlockType::Empty));
        self.emit(Instruction::I32Const(i32::MIN));
        self.emit(Instruction::I32Const(-1));
        self.emit(Instruction::I32DivS);
        self.emit(Instruction::Drop);
        self.emit(Instruction::End);
    }

    /// Integer arithmetic with the operands on the stack: stores the result modulo 2ⁿ in a
    /// scratch local, which it returns, and leaves on the stack whether the exact result
    /// does not fit in `ty` (§ 12.9).
    fn arith_overflow(&mut self, op: Arith, ty: Type) -> u32 {
        use Instruction as I;
        let vt = ty.val_type();
        if ty.is_small_int() {
            // The operands are normalized `i32`s (§ 12.2), so the exact result fits in an
            // `i32` (read as unsigned for a `u16` product): it overflowed when it is out of
            // the range of the type.
            if op == Arith::Neg {
                let a = self.scratch(ValType::I32);
                self.emit(I::LocalSet(a));
                self.emit(I::I32Const(0));
                self.emit(I::LocalGet(a));
                self.emit(I::I32Sub);
            } else {
                self.emit(binary_instr(arith_op(op), Type::U32));
            }
            let r = self.scratch(ValType::I32);
            self.emit(I::LocalTee(r));
            match ty {
                Type::U8 => self.emit_all([I::I32Const(0xFF), I::I32GtU]),
                Type::U16 => self.emit_all([I::I32Const(0xFFFF), I::I32GtU]),
                Type::S8 => self.emit_all([I::LocalGet(r), I::I32Extend8S, I::I32Ne]),
                _ => self.emit_all([I::LocalGet(r), I::I32Extend16S, I::I32Ne]),
            }
            return r;
        }
        let r = self.scratch(vt);
        let instr = |op| binary_instr(op, ty);
        match op {
            Arith::Mul if vt == ValType::I64 => {
                // When both operands fit in 32 bits (signed or unsigned, as the type), the
                // exact product fits in 64 bits; otherwise a helper divides.
                let (a, b) = (self.scratch(vt), self.scratch(vt));
                self.emit_all([
                    I::LocalSet(b),
                    I::LocalTee(a),
                    I::LocalGet(b),
                    I::I64Mul,
                    I::LocalSet(r),
                    I::LocalGet(a),
                ]);
                if ty.is_signed() {
                    // a + 2³¹ < 2³² (unsigned) exactly when a is in [-2³¹, 2³¹).
                    self.emit_all([
                        I::I64Const(1 << 31),
                        I::I64Add,
                        I::LocalGet(b),
                        I::I64Const(1 << 31),
                        I::I64Add,
                    ]);
                } else {
                    self.emit(I::LocalGet(b));
                }
                let helper = self.cx.helper(Helper::MulOverflow64 {
                    signed: ty.is_signed(),
                });
                self.emit_all([
                    I::I64Or,
                    I::I64Const(32),
                    I::I64ShrU,
                    I::I64Eqz,
                    I::If(BlockType::Result(ValType::I32)),
                    I::I32Const(0),
                    I::Else,
                    I::LocalGet(a),
                    I::LocalGet(b),
                    I::Call(helper),
                    I::End,
                ]);
            }
            Arith::Mul => {
                // The exact product of two 32-bit integers fits in 64 bits.
                let extend = if ty.is_signed() {
                    I::I64ExtendI32S
                } else {
                    I::I64ExtendI32U
                };
                let b = self.scratch(ValType::I32);
                let wide = self.scratch(ValType::I64);
                self.emit_all([
                    I::LocalSet(b),
                    extend.clone(),
                    I::LocalGet(b),
                    extend,
                    I::I64Mul,
                    I::LocalTee(wide),
                    I::I32WrapI64,
                    I::LocalSet(r),
                    I::LocalGet(wide),
                ]);
                if ty.is_signed() {
                    self.emit_all([I::LocalGet(wide), I::I64Extend32S, I::I64Ne]);
                } else {
                    self.emit_all([I::I64Const(32), I::I64ShrU, I::I64Const(0), I::I64Ne]);
                }
            }
            Arith::Add | Arith::Sub => {
                let (a, b) = (self.scratch(vt), self.scratch(vt));
                self.emit_all([
                    I::LocalSet(b),
                    I::LocalTee(a),
                    I::LocalGet(b),
                    instr(arith_op(op)),
                    I::LocalSet(r),
                ]);
                if !ty.is_signed() {
                    // Unsigned: a + b overflowed when r < a, a - b when a < b.
                    let lhs = if op == Arith::Add { r } else { a };
                    let rhs = if op == Arith::Add { a } else { b };
                    self.emit_all([I::LocalGet(lhs), I::LocalGet(rhs), instr(BinOp::Lt)]);
                } else {
                    // Signed: the sign of r is wrong. For a + b, a and b have the same sign
                    // and r has the other: (a ^ r) & (b ^ r) < 0. For a - b, a and b have
                    // different signs and r has the sign of b: (a ^ b) & (a ^ r) < 0.
                    let (x, y) = if op == Arith::Add { (r, b) } else { (b, a) };
                    self.emit_all([
                        I::LocalGet(a),
                        I::LocalGet(r),
                        instr(BinOp::BitXor),
                        I::LocalGet(x),
                        I::LocalGet(y),
                        instr(BinOp::BitXor),
                        instr(BinOp::BitAnd),
                    ]);
                    self.zero(vt);
                    self.emit(instr(BinOp::Lt));
                }
            }
            Arith::Neg => {
                // Signed only: -a overflows for the minimum of the type.
                let a = self.scratch(vt);
                self.emit(I::LocalSet(a));
                self.zero(vt);
                self.emit_all([
                    I::LocalGet(a),
                    instr(BinOp::Sub),
                    I::LocalSet(r),
                    I::LocalGet(a),
                ]);
                self.emit(if vt == ValType::I64 {
                    I::I64Const(i64::MIN)
                } else {
                    I::I32Const(i32::MIN)
                });
                self.emit(instr(BinOp::Eq));
            }
        }
        r
    }

    fn emit_all<const N: usize>(&mut self, instrs: [Instruction<'static>; N]) {
        self.instrs.extend(instrs);
    }

    fn convert(&mut self, from: Type, to: Type) {
        use Instruction as I;
        if from == to {
            return;
        }
        let from_vt = from.val_type();
        let to_vt = to.val_type();
        if from.is_int() && to.is_int() {
            // Once the value is known to fit, it is already normalized for `to` (§ 12.2).
            self.check_fits(from, to);
            self.wrap_int(from, to, false);
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
            // `trunc` traps on NaN and on values out of the range of its result (§ 10.5).
            let f32_src = from == Type::F32;
            if to.is_small_int() {
                self.emit(if f32_src {
                    I::I32TruncF32S
                } else {
                    I::I32TruncF64S
                });
                self.check_fits(Type::S32, to);
            } else {
                let signed = to.is_signed();
                self.emit(match (to_vt, f32_src, signed) {
                    (ValType::I32, true, true) => I::I32TruncF32S,
                    (ValType::I32, true, false) => I::I32TruncF32U,
                    (ValType::I32, false, true) => I::I32TruncF64S,
                    (ValType::I32, false, false) => I::I32TruncF64U,
                    (_, true, true) => I::I64TruncF32S,
                    (_, true, false) => I::I64TruncF32U,
                    (_, false, true) => I::I64TruncF64S,
                    (_, false, false) => I::I64TruncF64U,
                });
            }
        } else if to == Type::F64 {
            self.emit(I::F64PromoteF32);
        } else {
            self.emit(I::F32DemoteF64);
        }
    }

    /// With an integer of type `from` on the stack: traps if its value does not fit in the
    /// integer type `to` (§ 10.5).
    fn check_fits(&mut self, from: Type, to: Type) {
        use Instruction as I;
        let (from_min, from_max) = from.int_range();
        let (to_min, to_max) = to.int_range();
        if to_min <= from_min && from_max <= to_max {
            return;
        }
        let vt = from.val_type();
        let constant = |v: i128| {
            if vt == ValType::I64 {
                I::I64Const(v as i64)
            } else {
                I::I32Const(v as i32)
            }
        };
        let t = self.scratch(vt);
        self.emit(I::LocalTee(t));
        if from.is_signed() && to.is_signed() {
            // t < min || t > max
            self.emit_all([
                constant(to_min),
                binary_instr(BinOp::Lt, from),
                I::LocalGet(t),
                constant(to_max),
                binary_instr(BinOp::Gt, from),
                I::I32Or,
            ]);
        } else {
            // Compared as unsigned, a negative `t` is above the maximum.
            let max = to_max.min(from_max);
            let unsigned = if vt == ValType::I64 {
                Type::U64
            } else {
                Type::U32
            };
            self.emit_all([constant(max), binary_instr(BinOp::Gt, unsigned)]);
        }
        self.trap_if_overflow();
        self.emit(I::LocalGet(t));
    }

    /// Integer to integer conversion keeping the low-order bits (`T::wrapping_from`), with
    /// the normalization of § 12.2 unless the value already fits in `to`.
    fn wrap_int(&mut self, from: Type, to: Type, normalize: bool) {
        use Instruction as I;
        match (from.val_type(), to.val_type()) {
            (ValType::I64, ValType::I32) => self.emit(I::I32WrapI64),
            (ValType::I32, ValType::I64) => self.emit(if from.is_signed() {
                I::I64ExtendI32S
            } else {
                I::I64ExtendI32U
            }),
            _ => {}
        }
        if normalize {
            self.normalize(to);
        }
    }
}

/// The binary operator of an arithmetic operation with two operands.
fn arith_op(op: Arith) -> BinOp {
    match op {
        Arith::Add => BinOp::Add,
        Arith::Sub => BinOp::Sub,
        Arith::Mul => BinOp::Mul,
        Arith::Neg => unreachable!("unary"),
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
