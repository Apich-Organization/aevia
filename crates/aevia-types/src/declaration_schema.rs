//! Collision-free declaration identities, independent of allocation and source revision.
//!
//! Only the reachable declaration graph enters the schema. Names are length
//! framed, dimension binders are numbered by parameter position, and nominal
//! edges use a sorted graph index. Recursive graphs therefore terminate without
//! hashing an entire source file or retaining process-global `AdtId`s.
use crate::ModulePath;
use crate::Nat;
use crate::Rep;
use crate::Shape;
use crate::Type;
use crate::adt;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

/// An immutable, canonical declaration schema. This is a local interface format,
/// not a versioned registry or serialized `.aevi` protocol.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeclarationSchemaKey(pub(crate) Arc<str>);

struct Encoder {
    text: String,
    nodes: HashMap<adt::AdtId, usize>,
    dimensions: HashMap<u32, u32>,
}
impl Encoder {
    fn part(
        &mut self,
        value: impl AsRef<str>,
    ) {
        let value = value.as_ref();
        self.text.push_str(&value.len().to_string());
        self.text.push(':');
        self.text.push_str(value);
    }

    fn number(
        &mut self,
        value: impl ToString,
    ) {
        self.part(value.to_string());
    }

    fn module(
        &mut self,
        module: &ModulePath,
    ) {
        self.number(module.0.len());
        for component in &module.0 {
            self.part(component);
        }
        if let Some(origin) = module.origin() {
            self.part("origin");
            self.part(&origin.package);
            self.part(&origin.source);
        } else {
            self.part("no-origin");
        }
    }

    fn dim(
        &mut self,
        dim: &aevia_dim::Dim,
    ) -> Option<()> {
        let mut factors = BTreeMap::new();
        for (atom, power) in dim.factors() {
            let atom = match atom {
                | aevia_dim::Atom::Base(name) => (0_u8, name.to_string()),
                | aevia_dim::Atom::Var(id) => (1, self.dimensions.get(id)?.to_string()),
            };
            factors.insert(atom, power);
        }
        self.number(factors.len());
        for ((kind, name), power) in factors {
            self.number(kind);
            self.part(name);
            self.number(power.numer());
            self.number(power.denom());
        }
        Some(())
    }

    fn rep(
        &mut self,
        rep: Rep,
    ) -> Option<()> {
        if matches!(rep, Rep::Var(_)) {
            return None;
        }
        self.part(rep.name());
        Some(())
    }

    fn nat(
        &mut self,
        nat: &Nat,
    ) -> Option<()> {
        if let Some(value) = nat.as_const() {
            self.part("lit");
            self.number(value);
            return Some(());
        }
        match nat {
            | Nat::Lit(n) => {
                self.part("lit");
                self.number(n);
            },
            | Nat::Add(a, b) => {
                self.part("add");
                self.nat(a)?;
                self.nat(b)?;
            },
            | Nat::Mul(a, b) => {
                self.part("mul");
                self.nat(a)?;
                self.nat(b)?;
            },
            | Nat::Var(_) => return None,
        }
        Some(())
    }

    fn ty(
        &mut self,
        ty: &Type,
    ) -> Option<()> {
        match ty {
            | Type::Scalar(rep, dim) => {
                self.part("scalar");
                self.rep(*rep)?;
                self.dim(dim)?;
            },
            | Type::Bool => self.part("bool"),
            | Type::Str => self.part("string"),
            | Type::Prop => self.part("prop"),
            | Type::Unit => self.part("unit"),
            | Type::Point(dim) => {
                self.part("point");
                self.dim(dim)?;
            },
            | Type::UnitOf(dim) => {
                self.part("unit-of");
                self.dim(dim)?;
            },
            | Type::Dyn(rep) => {
                self.part("dyn");
                self.rep(*rep)?;
            },
            | Type::Tensor { shape, elem } => {
                self.part("tensor");
                match shape {
                    | Shape::Rank(rank) => {
                        self.part("rank");
                        self.number(rank);
                    },
                    | Shape::Refined(items) if items.iter().all(Option::is_none) => {
                        self.part("rank");
                        self.number(items.len());
                    },
                    | Shape::Refined(items) => {
                        self.part("shape");
                        self.number(items.len());
                        for item in items {
                            if let Some(n) = item {
                                self.part("extent");
                                self.nat(n)?;
                            } else {
                                self.part("unknown");
                            }
                        }
                    },
                }
                self.ty(elem)?;
            },
            | Type::Fn(args, result) => {
                self.part("function");
                self.number(args.len());
                for arg in args {
                    self.ty(arg)?;
                }
                self.ty(result)?;
            },
            | Type::Record(row) => {
                if row.tail_var().is_some() {
                    return None;
                }
                self.part("record");
                let mut fields = row.fields();
                fields.sort_by(|a, b| a.0.cmp(&b.0));
                self.number(fields.len());
                for (name, ty) in fields {
                    self.part(name);
                    self.ty(&ty)?;
                }
            },
            | Type::Adt(id, args) => {
                self.part("adt");
                self.number(*self.nodes.get(id)?);
                self.number(args.len());
                for arg in args {
                    self.ty(arg)?;
                }
            },
            | Type::Io(inner) => {
                self.part("io");
                self.ty(inner)?;
            },
            | Type::Param(index) => {
                self.part("parameter");
                self.number(index);
            },
            | Type::TensorIntrinsic(intrinsic) => {
                self.part("tensor-intrinsic");
                self.part(intrinsic.kind.name());
                if let Some(rep) = intrinsic.element {
                    self.part("element");
                    self.rep(rep)?;
                } else {
                    self.part("polymorphic");
                }
            },
            | Type::ToCallbackIntrinsic => self.part("to-callback"),
            | Type::TypeRepIntrinsic(inner) | Type::FromValueIntrinsic(inner) => {
                self.part(if matches!(ty, Type::TypeRepIntrinsic(_)) {
                    "type-rep"
                } else {
                    "from-value"
                });
                if let Some(inner) = inner {
                    self.part("specialized");
                    self.ty(inner)?;
                } else {
                    self.part("polymorphic");
                }
            },
            | Type::Var(_) | Type::Error => return None,
        }
        Some(())
    }
}
fn edges(
    ty: &Type,
    out: &mut Vec<adt::AdtId>,
) {
    match ty {
        | Type::Adt(id, args) => {
            out.push(*id);
            for arg in args {
                edges(arg, out);
            }
        },
        | Type::Tensor { elem, .. } | Type::Io(elem) => edges(elem, out),
        | Type::Fn(args, result) => {
            for arg in args {
                edges(arg, out);
            }
            edges(result, out);
        },
        | Type::Record(row) => {
            for (_, ty) in row.fields() {
                edges(&ty, out);
            }
        },
        | Type::TypeRepIntrinsic(Some(inner)) | Type::FromValueIntrinsic(Some(inner)) => {
            edges(inner, out)
        },
        | _ => {},
    }
}
pub(crate) fn build(root: adt::AdtId) -> Option<DeclarationSchemaKey> {
    let mut pending = vec![root];
    let mut visited = HashSet::new();
    let mut nodes = BTreeMap::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let declaration = adt::get(id)?;
        let module = adt::module_of(id);
        if !module.is_prelude() && !adt::is_complete(id) {
            return None;
        }
        // An intentionally mixed graph containing different revisions of one
        // nominal declaration is conservatively unshareable.
        if nodes
            .insert(
                (module.clone(), declaration.name.clone()),
                (id, declaration.clone()),
            )
            .is_some()
        {
            return None;
        }
        if !module.is_prelude() {
            for ctor in &declaration.ctors {
                for field in &ctor.fields {
                    edges(field, &mut pending);
                }
            }
        }
    }
    let mut encoder = Encoder {
        text: String::new(),
        nodes: nodes
            .values()
            .enumerate()
            .map(|(index, (id, _))| (*id, index))
            .collect(),
        dimensions: HashMap::new(),
    };
    encoder.part("aevia-declaration-schema-v1");
    encoder.number(*encoder.nodes.get(&root)?);
    encoder.number(nodes.len());
    for ((module, name), (id, declaration)) in nodes {
        encoder.module(&module);
        encoder.part(name);
        if module.is_prelude() {
            encoder.part("host");
            continue;
        }
        encoder.part("source");
        encoder.dimensions.clear();
        let kinds = adt::param_kinds_of(id);
        encoder.number(kinds.len());
        for (index, kind) in kinds.iter().enumerate() {
            match kind {
                | adt::ParamKind::Type => encoder.part("type"),
                | adt::ParamKind::Dim(id) => {
                    encoder.part("dimension");
                    encoder.dimensions.insert(*id, u32::try_from(index).ok()?);
                },
            }
        }
        encoder.number(declaration.ctors.len());
        for ctor in declaration.ctors {
            encoder.part(ctor.name);
            encoder.number(ctor.fields.len());
            for field in ctor.fields {
                encoder.ty(&field)?;
            }
        }
    }
    Some(DeclarationSchemaKey(encoder.text.into()))
}
