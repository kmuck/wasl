//! Interfaces of other packages (§ 2.5): their functions and types, read from the WIT of
//! the dependencies (§ 3.5) and converted into WaSL types, with WaSL names (§ 13.2).
//!
//! Resources become resource declarations whose constructor, methods and static
//! functions are imported functions (§ 7.12).
//!
//! An item that WaSL cannot use (a name that is not a WaSL identifier, a type WaSL does
//! not have yet) is not an error by itself: it is recorded with the reason, reported when
//! the code names it (E0215).

use std::collections::HashMap;

use wit_parser::{FunctionKind, InterfaceId, Resolve, TypeDefKind, TypeId, TypeOwner};

use super::InterfaceScope;
use crate::hir::{self, Foreign};
use crate::lexer::is_reserved;
use crate::types::{
    CaseDef, DeclKind, EndKind, FieldDef, Prim, ResourceDef, Type, TypeDecl, TypeKind, Types,
};

/// The WaSL name of a WIT name: `output-stream` → `output_stream`, `%from` → `from`,
/// `DNS-timeout` → `dns_timeout`. `None` when it gives no valid WaSL identifier (§ 13.2).
pub(crate) fn wasl_name(wit: &str) -> Option<String> {
    let name = wit
        .strip_prefix('%')
        .unwrap_or(wit)
        .replace('-', "_")
        .to_ascii_lowercase();
    let valid = !name.is_empty()
        && name
            .split('_')
            .all(|w| w.starts_with(|c: char| c.is_ascii_lowercase()))
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        && !is_reserved(&name);
    valid.then_some(name)
}

/// Why an item cannot be used from WaSL.
type Reason = String;

/// Converts the items of the interfaces of other packages. The interfaces it meets get
/// the HIR ids `first`, `first + 1`…, in the order of `ifaces`.
pub(super) struct Converter<'r> {
    resolve: &'r Resolve,
    pub ifaces: Vec<Foreign>,
    first: usize,
    ids: HashMap<InterfaceId, usize>,
    memo: HashMap<TypeId, Result<Type, Reason>>,
}

impl<'r> Converter<'r> {
    pub fn new(resolve: &'r Resolve, first: usize) -> Self {
        Converter {
            resolve,
            ifaces: Vec::new(),
            first,
            ids: HashMap::new(),
            memo: HashMap::new(),
        }
    }

    /// The HIR id of an interface of another package, added if needed.
    pub fn iface(&mut self, id: InterfaceId) -> usize {
        if let Some(&i) = self.ids.get(&id) {
            return i;
        }
        let iface = &self.resolve.interfaces[id];
        let package = &self.resolve.packages[iface.package.expect("interface of a package")];
        let i = self.first + self.ifaces.len();
        self.ifaces.push(Foreign {
            id,
            package: format!("{}:{}", package.name.namespace, package.name.name),
            name: iface.name.clone().expect("named interface"),
            version: package.name.version.as_ref().map(|v| v.to_string()),
        });
        self.ids.insert(id, i);
        i
    }

    /// The name code gives to the interface with HIR id `i`: the last part of its name.
    pub fn short_name(&self, i: usize) -> String {
        let name = &self.ifaces[i - self.first].name;
        wasl_name(name).unwrap_or_else(|| name.clone())
    }

    /// The functions and types of an interface that code names, with its scope. The
    /// functions get the ids `first_func`, `first_func + 1`…
    pub fn scope(
        &mut self,
        types: &mut Types,
        id: InterfaceId,
        first_func: usize,
    ) -> (InterfaceScope, Vec<hir::FuncDef>) {
        let hir_id = self.iface(id);
        let iface = &self.resolve.interfaces[id];
        let mut scope = InterfaceScope::default();
        let mut funcs = Vec::new();
        for (wit, &ty) in &iface.types {
            let Some(name) = wasl_name(wit) else { continue };
            match self.def(types, ty) {
                Ok(t) => match types.kind(t) {
                    TypeKind::Decl(d) => {
                        scope.types.insert(name, *d);
                    }
                    _ => {
                        scope.aliases.insert(name, t);
                    }
                },
                Err(reason) => {
                    scope.unavailable.insert(name, reason);
                }
            }
        }
        for (wit, func) in &iface.functions {
            let name = match func.kind {
                FunctionKind::Constructor(_) => Some("constructor".to_string()),
                _ => wasl_name(func.item_name()),
            };
            let Some(name) = name else { continue };
            let resource = match func.kind {
                FunctionKind::Constructor(r)
                | FunctionKind::Method(r)
                | FunctionKind::Static(r)
                | FunctionKind::AsyncMethod(r)
                | FunctionKind::AsyncStatic(r) => match self.def(types, r) {
                    Ok(t) => types.resource(t).map(|(d, _)| d),
                    Err(_) => continue,
                },
                _ => None,
            };
            let is_async = matches!(
                func.kind,
                FunctionKind::AsyncFreestanding
                    | FunctionKind::AsyncMethod(_)
                    | FunctionKind::AsyncStatic(_)
            );
            let converted = match func.kind {
                FunctionKind::Freestanding
                | FunctionKind::AsyncFreestanding
                | FunctionKind::Constructor(_)
                | FunctionKind::Static(_)
                | FunctionKind::AsyncStatic(_) => self.signature(types, &func.params, func.result),
                // `self` is the first parameter, whose WIT name is not a WaSL name.
                FunctionKind::Method(_) | FunctionKind::AsyncMethod(_) => self
                    .signature(types, &func.params[1..], func.result)
                    .and_then(|(params, ret)| {
                        let this = self.ty(types, func.params[0].ty)?;
                        let mut all = vec![("self".to_string(), this)];
                        all.extend(params);
                        Ok((all, ret))
                    }),
                #[allow(unreachable_patterns)]
                _ => Err(format!(
                    "`{wit}` is a kind of function that WaSL does not support yet"
                )),
            };
            let id = first_func + funcs.len();
            let kind = match (func.kind.clone(), resource) {
                (FunctionKind::Constructor(_), Some(d)) => hir::FuncKind::Constructor(d),
                (FunctionKind::Method(_) | FunctionKind::AsyncMethod(_), Some(d)) => {
                    hir::FuncKind::Method(d)
                }
                (FunctionKind::Static(_) | FunctionKind::AsyncStatic(_), Some(d)) => {
                    hir::FuncKind::Static(d)
                }
                _ => hir::FuncKind::Free,
            };
            match converted {
                Ok((params, ret)) => {
                    match kind {
                        hir::FuncKind::Free => {
                            scope.funcs.insert(name.clone(), id);
                        }
                        hir::FuncKind::Constructor(d)
                        | hir::FuncKind::Method(d)
                        | hir::FuncKind::Static(d) => {
                            let DeclKind::Resource(def) = &mut types.decls[d].kind else {
                                unreachable!("a resource")
                            };
                            match kind {
                                hir::FuncKind::Constructor(_) => def.constructor = Some(id),
                                hir::FuncKind::Method(_) => def.methods.push((name.clone(), id)),
                                _ => def.statics.push((name.clone(), id)),
                            }
                        }
                    }
                    funcs.push(hir::FuncDef {
                        name,
                        wit: Some(func.item_name().to_string()),
                        kind,
                        is_async,
                        delivers: false,
                        interface: hir_id,
                        params,
                        ret,
                        locals: Vec::new(),
                        body: Vec::new(),
                    });
                }
                Err(reason) if kind == hir::FuncKind::Free => {
                    scope.unavailable.insert(name, reason);
                }
                // A member of a resource that WaSL cannot use is left out: naming it is
                // E0308.
                Err(_) => {}
            }
        }
        (scope, funcs)
    }

    #[allow(clippy::type_complexity)]
    fn signature(
        &mut self,
        types: &mut Types,
        params: &[wit_parser::Param],
        result: Option<wit_parser::Type>,
    ) -> Result<(Vec<(String, Type)>, Option<Type>), Reason> {
        let mut out = Vec::new();
        for p in params {
            let name = wasl_name(&p.name)
                .ok_or_else(|| format!("its parameter `{}` has no WaSL name", p.name))?;
            out.push((name, self.ty(types, p.ty)?));
        }
        let ret = result.map(|t| self.ty(types, t)).transpose()?;
        Ok((out, ret))
    }

    fn ty(&mut self, types: &mut Types, ty: wit_parser::Type) -> Result<Type, Reason> {
        use wit_parser::Type as W;
        let prim = |p| Ok(Type::prim(p));
        match ty {
            W::Bool => prim(Prim::Bool),
            W::U8 => prim(Prim::U8),
            W::U16 => prim(Prim::U16),
            W::U32 => prim(Prim::U32),
            W::U64 => prim(Prim::U64),
            W::S8 => prim(Prim::S8),
            W::S16 => prim(Prim::S16),
            W::S32 => prim(Prim::S32),
            W::S64 => prim(Prim::S64),
            W::F32 => prim(Prim::F32),
            W::F64 => prim(Prim::F64),
            W::Char => Ok(Type::CHAR),
            W::String => Ok(Type::STRING),
            W::ErrorContext => {
                Err("it uses `error-context`, which WaSL does not support yet".into())
            }
            W::Id(id) => self.def(types, id),
        }
    }

    /// The WaSL type of a type definition of the WIT.
    fn def(&mut self, types: &mut Types, id: TypeId) -> Result<Type, Reason> {
        if let Some(done) = self.memo.get(&id) {
            return done.clone();
        }
        let result = self.convert(types, id);
        self.memo.insert(id, result.clone());
        result
    }

    fn convert(&mut self, types: &mut Types, id: TypeId) -> Result<Type, Reason> {
        let def = &self.resolve.types[id];
        let kind = match &def.kind {
            TypeDefKind::Type(inner) => return self.ty(types, *inner),
            TypeDefKind::List(elem) => TypeKind::List {
                mutable: false,
                elem: self.ty(types, *elem)?,
            },
            TypeDefKind::Option(inner) => TypeKind::Option(self.ty(types, *inner)?),
            TypeDefKind::Result(r) => TypeKind::Result {
                ok: r.ok.map(|t| self.ty(types, t)).transpose()?,
                err: r.err.map(|t| self.ty(types, t)).transpose()?,
            },
            TypeDefKind::Tuple(t) => TypeKind::Tuple(
                t.types
                    .iter()
                    .map(|&t| self.ty(types, t))
                    .collect::<Result<_, _>>()?,
            ),
            TypeDefKind::Handle(wit_parser::Handle::Own(r)) => return self.def(types, *r),
            TypeDefKind::Handle(wit_parser::Handle::Borrow(r)) => {
                TypeKind::Borrow(self.def(types, *r)?)
            }
            TypeDefKind::Stream(Some(elem)) => {
                TypeKind::End(EndKind::Stream, self.ty(types, *elem)?)
            }
            TypeDefKind::Future(Some(value)) => {
                TypeKind::End(EndKind::Future, self.ty(types, *value)?)
            }
            TypeDefKind::Stream(None) | TypeDefKind::Future(None) => {
                return Err(format!(
                    "it uses a `{}` without an element type, which WaSL does not support",
                    def.kind.as_str()
                ));
            }
            TypeDefKind::Map(..) | TypeDefKind::FixedLengthList(..) | TypeDefKind::Unknown => {
                return Err(format!(
                    "it uses a `{}`, which WaSL does not support yet",
                    def.kind.as_str()
                ));
            }
            TypeDefKind::Resource
            | TypeDefKind::Record(_)
            | TypeDefKind::Variant(_)
            | TypeDefKind::Enum(_)
            | TypeDefKind::Flags(_) => {
                return self.decl(types, id);
            }
        };
        Ok(types.intern(kind))
    }

    /// A record, variant, enum, flags or resource type, as a declared type of its interface.
    fn decl(&mut self, types: &mut Types, id: TypeId) -> Result<Type, Reason> {
        let def = &self.resolve.types[id];
        let wit = def.name.clone().expect("named type");
        let name = wasl_name(&wit).ok_or_else(|| format!("`{wit}` has no WaSL name"))?;
        let member = |n: &str, what: &str| {
            wasl_name(n).ok_or_else(|| format!("the {what} `{n}` of `{wit}` has no WaSL name"))
        };
        let kind = match &def.kind {
            TypeDefKind::Record(r) => {
                let mut fields = Vec::new();
                for f in &r.fields {
                    fields.push(FieldDef {
                        name: member(&f.name, "field")?,
                        mutable: false,
                        ty: self.ty(types, f.ty)?,
                    });
                }
                DeclKind::Record(fields)
            }
            TypeDefKind::Variant(v) => {
                let mut cases = Vec::new();
                for c in &v.cases {
                    cases.push(CaseDef {
                        name: member(&c.name, "case")?,
                        payload: c.ty.map(|t| self.ty(types, t)).transpose()?,
                    });
                }
                DeclKind::Variant(cases)
            }
            TypeDefKind::Enum(e) => DeclKind::Enum(
                e.cases
                    .iter()
                    .map(|c| member(&c.name, "case"))
                    .collect::<Result<_, _>>()?,
            ),
            TypeDefKind::Flags(f) => DeclKind::Flags(
                f.flags
                    .iter()
                    .map(|c| member(&c.name, "flag"))
                    .collect::<Result<_, _>>()?,
            ),
            TypeDefKind::Resource => DeclKind::Resource(ResourceDef {
                fields: Vec::new(),
                constructor: None,
                methods: Vec::new(),
                statics: Vec::new(),
            }),
            _ => unreachable!("a declared type"),
        };
        let TypeOwner::Interface(owner) = def.owner else {
            return Err(format!("`{wit}` is not declared in an interface"));
        };
        let interface = self.iface(owner);
        let decl = types.decls.len();
        types.decls.push(TypeDecl {
            name,
            wit: Some(wit),
            interface,
            kind,
        });
        Ok(types.decl_type(decl))
    }
}
