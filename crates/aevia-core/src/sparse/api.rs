//! The language-facing surface of sparse tensors: the builtins, the operator
//! dispatch (`+ - * / @ == !=`), indexing, equality and rendering.

use super::Entries;
use super::R;
use super::SpValues;
use super::SparseData;
use super::SparseFormat;
use super::Store;
use super::element_count;
use super::err;
use super::index_list;
use super::index_of;
use super::int_tensor;
use super::int_value;
use super::kernels;
use super::kernels::FloatRing;
use super::kernels::Merge;
use super::kernels::Ring;
use super::kernels::ValueRing;
use super::kind_rank;
use super::num_is_zero;
use super::record_of;
use super::tuple_of;
use super::zero_num;
use crate::eval::EvalError;
use crate::eval::Interp;
use crate::eval::build_tensor;
use crate::value::NumKind;
use crate::value::NumVal;
use crate::value::Storage;
use crate::value::TensorData;
use crate::value::Value;
use aevia_dim::Dim;
use aevia_dim::UnitDef;
use aevia_syntax::Span;
use aevia_syntax::ast::BinOp;
use aevia_syntax::ast::Ty;
use aevia_syntax::ast::TyKind;
use std::borrow::Cow;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Class helpers
// ---------------------------------------------------------------------------

/// The same tensor with boxed values (a copy only when it was `f64`).
fn boxed(data: &SparseData) -> Cow<'_, SparseData> {
    match &data.values {
        | SpValues::Boxed(_) => Cow::Borrowed(data),
        | SpValues::F64(_) => {
            let mut copy = data.clone();
            copy.values = SpValues::Boxed(data.boxed_values());
            Cow::Owned(copy)
        },
    }
}

fn zero_rank(data: &SparseData) -> u8 {
    kind_rank(&data.zero.kind)
}

/// Like [`wrap`], keeping the block shape of a `Bcsr` operand when it still
/// divides the result (transposes try the swapped block).
fn wrap_like<T: Store>(
    like: &SparseData,
    shape: Vec<usize>,
    dim: Dim,
    display: Option<Rc<UnitDef>>,
    zero_rank: u8,
    entries: Entries<T>,
) -> Value {
    if like.format == SparseFormat::Bcsr && shape.len() == 2 {
        let (br, bc) = like.block;
        for (r, c) in [(br, bc), (bc, br)] {
            if shape[0] % r == 0 && shape[1] % c == 0 {
                return Value::Sparse(Rc::new(SparseData::from_entries_blocked(
                    shape,
                    dim,
                    display,
                    zero_rank,
                    (r, c),
                    entries,
                )));
            }
        }
    }
    wrap(like.format, shape, dim, display, zero_rank, entries)
}

fn wrap<T: Store>(
    format: SparseFormat,
    shape: Vec<usize>,
    dim: Dim,
    display: Option<Rc<UnitDef>>,
    zero_rank: u8,
    entries: Entries<T>,
) -> Value {
    Value::Sparse(Rc::new(SparseData::from_entries(
        format, shape, dim, display, zero_rank, entries,
    )))
}

fn dense_value_f64(
    shape: Vec<usize>,
    data: Vec<f64>,
    dim: Dim,
    display: Option<Rc<UnitDef>>,
) -> Value {
    Value::Tensor(Rc::new(TensorData {
        verified_element: None,
        shape,
        storage: Storage::Dense(Rc::new(data)),
        dim,
        display,
    }))
}

/// A dense tensor from accumulated values (`None` is the zero).
fn dense_from_options(
    shape: Vec<usize>,
    out: Vec<Option<Value>>,
    base_rank: u8,
    dim: Dim,
) -> Value {
    let mut rank = base_rank;
    for item in out.iter().flatten() {
        if let Value::Num(num) = item {
            rank = rank.max(kind_rank(&num.kind));
        }
    }
    let zero = Value::Num(zero_num(rank, &dim));
    let elements = out
        .into_iter()
        .map(|item| item.unwrap_or_else(|| zero.clone()))
        .collect();
    build_tensor(elements, shape, dim)
}

fn sparse_arg<'a>(
    args: &'a [Value],
    index: usize,
    name: &str,
    span: Span,
) -> R<&'a Rc<SparseData>> {
    match args.get(index) {
        | Some(Value::Sparse(data)) => Ok(data),
        | Some(other) => {
            Err(err(
                "E0302",
                format!(
                    "`{name}` expects a sparse tensor, found {}",
                    other.type_name()
                ),
                span,
            ))
        },
        | None => {
            Err(err(
                "E0315",
                format!("`{name}` is missing argument {}", index + 1),
                span,
            ))
        },
    }
}

fn tensor_arg<'a>(
    args: &'a [Value],
    index: usize,
    name: &str,
    span: Span,
) -> R<&'a Rc<TensorData>> {
    match args.get(index) {
        | Some(Value::Tensor(data)) => Ok(data),
        | Some(other) => {
            Err(err(
                "E0302",
                format!("`{name}` expects a tensor, found {}", other.type_name()),
                span,
            ))
        },
        | None => {
            Err(err(
                "E0315",
                format!("`{name}` is missing argument {}", index + 1),
                span,
            ))
        },
    }
}

fn int_arg(
    args: &[Value],
    index: usize,
    name: &str,
    span: Span,
) -> R<usize> {
    match args.get(index) {
        | Some(value) => index_of(value, name, span),
        | None => {
            Err(err(
                "E0315",
                format!("`{name}` is missing argument {}", index + 1),
                span,
            ))
        },
    }
}

fn rank2<'a>(
    data: &'a SparseData,
    what: &str,
    span: Span,
) -> R<&'a SparseData> {
    if data.rank() == 2 {
        Ok(data)
    } else {
        Err(err(
            "E0802",
            format!(
                "{what} needs a rank-2 sparse matrix, found rank {}",
                data.rank()
            ),
            span,
        ))
    }
}

fn check_format_rank(
    format: SparseFormat,
    rank: usize,
    span: Span,
) -> R<()> {
    if format != SparseFormat::Coo && rank != 2 {
        return Err(err(
            "E0802",
            format!(
                "{} stores matrices (rank 2), but this tensor has rank {rank}; use Coo for a sparse tensor",
                format.name()
            ),
            span,
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Builtins
// ---------------------------------------------------------------------------

/// The format named by a type argument (`from_dense<Coo>`).
fn format_of_type(
    ty: &Ty,
    span: Span,
) -> R<SparseFormat> {
    let name = match &ty.kind {
        | TyKind::Named { path, args } if args.is_empty() => path.last().cloned(),
        | TyKind::Var(name) => Some(name.clone()),
        | _ => None,
    };
    match name {
        | Some(name) => SparseFormat::parse(&name, span),
        | None => {
            Err(err(
                "E0310",
                "expected a sparse format name (Csr, Csc, Coo or Bcsr)",
                span,
            ))
        },
    }
}

/// A builtin called with explicit type arguments (`from_dense<Coo>(A)`).
pub(crate) fn call_typed(
    interp: &mut Interp,
    name: &str,
    types: &[Ty],
    args: &[Value],
    span: Span,
) -> R<Value> {
    let format = match types.first() {
        | Some(ty) => Some(format_of_type(ty, span)?),
        | None => None,
    };
    run(interp, name, format, args, span)
}

/// A builtin called without type arguments. `None` means the name (or its
/// argument types) is not one the sparse runtime serves.
pub(crate) fn call(
    interp: &mut Interp,
    name: &str,
    args: &[Value],
    span: Span,
) -> Option<R<Value>> {
    let sparse_first = matches!(args.first(), Some(Value::Sparse(_)));
    let always = matches!(
        name,
        "from_dense"
            | "to_dense"
            | "nnz"
            | "density"
            | "format_of"
            | "convert"
            | "to_bcsr"
            | "block_size"
            | "sparse_coo"
            | "sparse_csr"
            | "speye"
            | "spdiag"
            | "spsolve"
            | "cg"
            | "cg_with"
            | "kron"
            | "contract"
            | "map_nonzero"
            | "nonzeros"
    );
    let polymorphic = matches!(
        name,
        "transpose" | "matmul" | "sum" | "get" | "shape" | "rank"
    ) && sparse_first;
    if !always && !polymorphic {
        return None;
    }
    Some(run(interp, name, None, args, span))
}

fn run(
    interp: &mut Interp,
    name: &str,
    format: Option<SparseFormat>,
    args: &[Value],
    span: Span,
) -> R<Value> {
    match name {
        | "from_dense" => {
            let tensor = tensor_arg(args, 0, name, span)?;
            from_dense(format.unwrap_or(SparseFormat::Csr), tensor, span)
        },
        | "to_dense" => to_dense(sparse_arg(args, 0, name, span)?, span),
        | "nnz" => Ok(int_value(sparse_arg(args, 0, name, span)?.nnz())),
        | "density" => {
            let data = sparse_arg(args, 0, name, span)?;
            let total = data.shape.iter().map(|e| *e as f64).product::<f64>();
            let value = if total == 0.0 {
                0.0
            } else {
                data.nnz() as f64 / total
            };
            Ok(Value::float(value))
        },
        | "format_of" => {
            Ok(Value::string(
                sparse_arg(args, 0, name, span)?.format.name(),
            ))
        },
        | "shape" => Ok(int_tensor(&sparse_arg(args, 0, name, span)?.shape)),
        | "rank" => Ok(int_value(sparse_arg(args, 0, name, span)?.rank())),
        | "convert" => {
            let data = sparse_arg(args, 0, name, span)?;
            let Some(target) = format else {
                return Err(err(
                    "E0310",
                    "`convert` needs its target format, as in `convert<Coo>(A)`",
                    span,
                ));
            };
            convert(data, target, span)
        },
        | "to_bcsr" => {
            let data = sparse_arg(args, 0, name, span)?;
            let br = int_arg(args, 1, name, span)?;
            let bc = int_arg(args, 2, name, span)?;
            to_bcsr(data, br, bc, span)
        },
        | "block_size" => {
            let (br, bc) = sparse_arg(args, 0, name, span)?.block;
            Ok(tuple_of(vec![int_value(br), int_value(bc)]))
        },
        | "sparse_coo" => sparse_coo(interp, args, span),
        | "sparse_csr" => sparse_csr(interp, args, span),
        | "speye" => {
            speye(
                int_arg(args, 0, name, span)?,
                format.unwrap_or(SparseFormat::Csr),
                span,
            )
        },
        | "spdiag" => {
            let tensor = tensor_arg(args, 0, name, span)?;
            spdiag(tensor, format.unwrap_or(SparseFormat::Csr), span)
        },
        | "transpose" => transpose(sparse_arg(args, 0, name, span)?, span),
        | "matmul" => {
            let (left, right) = (
                args.first().cloned().unwrap_or(Value::Unit),
                args.get(1).cloned().unwrap_or(Value::Unit),
            );
            binary(interp, BinOp::MatMul, left, right, span)
        },
        | "sum" => sum(interp, args, span),
        | "get" => {
            let data = sparse_arg(args, 0, name, span)?;
            let mut coord = Vec::new();
            for item in &args[1..] {
                coord.push(signed_index(item, span)?);
            }
            index(data, &coord, span)
        },
        | "kron" => {
            kron(
                interp,
                sparse_arg(args, 0, name, span)?,
                sparse_arg(args, 1, name, span)?,
                span,
            )
        },
        | "spsolve" => {
            let data = sparse_arg(args, 0, name, span)?;
            let rhs = tensor_arg(args, 1, name, span)?;
            spsolve(interp, data, rhs, span)
        },
        | "cg" => cg(args, span),
        | "cg_with" => cg_with(args, span),
        | "map_nonzero" => map_nonzero(interp, args, span),
        | "nonzeros" => nonzeros(sparse_arg(args, 0, name, span)?),
        | "contract" => contract(interp, args, span),
        | other => {
            Err(err(
                "E1001",
                format!("`{other}` is not a sparse builtin"),
                span,
            ))
        },
    }
}

fn signed_index(
    value: &Value,
    span: Span,
) -> R<isize> {
    value
        .as_f64()
        .filter(|f| f.fract() == 0.0)
        .map(|f| f as isize)
        .ok_or_else(|| err("E0802", "a sparse index must be an Int", span))
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

fn unflatten_into(
    flat: usize,
    shape: &[usize],
    out: &mut Vec<usize>,
) {
    let start = out.len();
    out.resize(start + shape.len(), 0);
    kernels::unflatten(flat, shape, &mut out[start..]);
}

fn from_dense(
    format: SparseFormat,
    tensor: &TensorData,
    span: Span,
) -> R<Value> {
    let rank = tensor.rank();
    if rank == 0 {
        return Err(err(
            "E0802",
            "`from_dense` needs a tensor of rank 1 or more",
            span,
        ));
    }
    check_format_rank(format, rank, span)?;
    if let Some(data) = tensor.dense_flat() {
        let mut entries: Entries<f64> = Entries::empty(rank);
        for (flat, value) in data.iter().enumerate() {
            if *value != 0.0 {
                let mut coord = Vec::with_capacity(rank);
                unflatten_into(flat, &tensor.shape, &mut coord);
                entries.push(&coord, *value);
            }
        }
        return Ok(wrap(
            format,
            tensor.shape.clone(),
            tensor.dim.clone(),
            tensor.display.clone(),
            2,
            entries,
        ));
    }
    let mut entries: Entries<Value> = Entries::empty(rank);
    let mut base = if tensor.storage.is_empty() {
        2
    } else {
        0
    };
    let mut coord = Vec::with_capacity(rank);
    for (flat, element) in tensor.elements().into_iter().enumerate() {
        let Value::Num(num) = &element else {
            return Err(err(
                "E0302",
                format!(
                    "`from_dense` needs numeric elements, found {}",
                    element.type_name()
                ),
                span,
            ));
        };
        base = base.max(kind_rank(&num.kind));
        if !num_is_zero(num) {
            coord.clear();
            unflatten_into(flat, &tensor.shape, &mut coord);
            entries.push(&coord, element);
        }
    }
    Ok(wrap(
        format,
        tensor.shape.clone(),
        tensor.dim.clone(),
        tensor.display.clone(),
        base,
        entries,
    ))
}

fn to_dense(
    data: &SparseData,
    span: Span,
) -> R<Value> {
    let count = element_count(&data.shape, span)?;
    if let Some(entries) = data.entries::<f64>() {
        let mut out = vec![0.0; count];
        for i in 0..entries.len() {
            out[kernels::flat_index(entries.coord(i), &data.shape)] = entries.vals[i];
        }
        return Ok(dense_value_f64(
            data.shape.clone(),
            out,
            data.dim.clone(),
            data.display.clone(),
        ));
    }
    let entries = data
        .entries::<Value>()
        .ok_or_else(|| kernels::class_error(span))?;
    let zero = Value::Num(data.zero.clone());
    let mut out = vec![zero; count];
    for i in 0..entries.len() {
        out[kernels::flat_index(entries.coord(i), &data.shape)] = entries.vals[i].clone();
    }
    let result = build_tensor(out, data.shape.clone(), data.dim.clone());
    match (&result, &data.display) {
        | (Value::Tensor(tensor), Some(display)) => {
            let mut copy = (**tensor).clone();
            copy.display = Some(display.clone());
            Ok(Value::Tensor(Rc::new(copy)))
        },
        | _ => Ok(result),
    }
}

/// `to_bcsr(A, br, bc)`: block CSR with an explicit `br x bc` block.
fn to_bcsr(
    data: &SparseData,
    br: usize,
    bc: usize,
    span: Span,
) -> R<Value> {
    check_format_rank(SparseFormat::Bcsr, data.rank(), span)?;
    if br == 0 || bc == 0 || data.shape[0] % br != 0 || data.shape[1] % bc != 0 {
        return Err(err(
            "E0802",
            format!(
                "a {br}x{bc} block does not tile a {}x{} matrix (the block extents must be                  positive divisors of the shape)",
                data.shape[0], data.shape[1]
            ),
            span,
        ));
    }
    if data.format == SparseFormat::Bcsr && data.block == (br, bc) {
        return Ok(Value::Sparse(Rc::new(data.clone())));
    }
    let block = (br, bc);
    Ok(if let Some(entries) = data.entries::<f64>() {
        Value::Sparse(Rc::new(SparseData::from_entries_blocked(
            data.shape.clone(),
            data.dim.clone(),
            data.display.clone(),
            2,
            block,
            entries,
        )))
    } else {
        let entries = data
            .entries::<Value>()
            .ok_or_else(|| kernels::class_error(span))?;
        Value::Sparse(Rc::new(SparseData::from_entries_blocked(
            data.shape.clone(),
            data.dim.clone(),
            data.display.clone(),
            zero_rank(data),
            block,
            entries,
        )))
    })
}

fn convert(
    data: &SparseData,
    target: SparseFormat,
    span: Span,
) -> R<Value> {
    check_format_rank(target, data.rank(), span)?;
    if data.format == target {
        return Ok(Value::Sparse(Rc::new(data.clone())));
    }
    Ok(if let Some(entries) = data.entries::<f64>() {
        wrap(
            target,
            data.shape.clone(),
            data.dim.clone(),
            data.display.clone(),
            2,
            entries,
        )
    } else {
        let entries = data
            .entries::<Value>()
            .ok_or_else(|| kernels::class_error(span))?;
        wrap(
            target,
            data.shape.clone(),
            data.dim.clone(),
            data.display.clone(),
            zero_rank(data),
            entries,
        )
    })
}

/// The element values of a 1-D tensor, and the representation class they
/// start the implicit zero at.
fn tensor_values(tensor: &TensorData) -> R<(Vec<Value>, u8)> {
    let items = tensor.elements();
    let mut rank = match &tensor.storage {
        | Storage::TypedExact(_, crate::value::ExactRep::Int) => 0,
        | Storage::Exact(_) | Storage::TypedExact(..) => 1,
        | _ => 2,
    };
    if !items.is_empty() {
        rank = 0;
    }
    for item in &items {
        match item {
            | Value::Num(num) => rank = rank.max(kind_rank(&num.kind)),
            | other => {
                return Err(EvalError::new(
                    "E0302",
                    format!("sparse values must be numbers, found {}", other.type_name()),
                ));
            },
        }
    }
    Ok((items, rank))
}

/// Runs a constructor generically over the value class of `values`.
fn build_from_triplets(
    interp: &mut Interp,
    format: SparseFormat,
    shape: Vec<usize>,
    coords: &[usize],
    values: &TensorData,
    span: Span,
) -> R<Value> {
    let rank = shape.len();
    if values.rank() != 1 {
        return Err(err("E0802", "the values must be a rank-1 tensor", span));
    }
    if let Some(data) = values.dense_flat() {
        let entries = kernels::assemble(rank, coords, data.to_vec(), &mut FloatRing)?;
        return Ok(wrap(
            format,
            shape,
            values.dim.clone(),
            values.display.clone(),
            2,
            entries,
        ));
    }
    let (items, base) = tensor_values(values).map_err(|e| e.at(span))?;
    let mut ring = ValueRing { interp, span };
    let entries = kernels::assemble(rank, coords, items, &mut ring)?;
    Ok(wrap(
        format,
        shape,
        values.dim.clone(),
        values.display.clone(),
        base,
        entries,
    ))
}

fn sparse_coo(
    interp: &mut Interp,
    args: &[Value],
    span: Span,
) -> R<Value> {
    let shape = index_list(
        args.first().unwrap_or(&Value::Unit),
        "sparse_coo shape",
        span,
    )?;
    if shape.is_empty() {
        return Err(err(
            "E0802",
            "a sparse tensor needs a shape of at least one extent",
            span,
        ));
    }
    let rank = shape.len();
    let indices = tensor_arg(args, 1, "sparse_coo", span)?;
    let values = tensor_arg(args, 2, "sparse_coo", span)?;
    let nnz = values.storage.len();
    // The index list: an `nnz × rank` matrix, a list of tuples, or (rank 1)
    // a list of Ints.
    let mut coords: Vec<usize> = Vec::with_capacity(nnz * rank);
    match indices.rank() {
        | 2 => {
            if indices.shape[1] != rank || indices.shape[0] != nnz {
                return Err(err(
                    "E0802",
                    format!(
                        "indices have shape {:?}, but {nnz} value(s) of a rank-{rank} tensor need [{nnz}, {rank}]",
                        indices.shape
                    ),
                    span,
                ));
            }
            for item in indices.elements() {
                coords.push(index_of(&item, "sparse_coo index", span)?);
            }
        },
        | 1 => {
            if indices.shape[0] != nnz {
                return Err(err(
                    "E0802",
                    format!("{} index entr(ies) for {nnz} value(s)", indices.shape[0]),
                    span,
                ));
            }
            for item in indices.elements() {
                match &item {
                    | Value::Record(record) => {
                        if record.fields.len() != rank {
                            return Err(err(
                                "E0802",
                                format!(
                                    "an index tuple has {} component(s), the tensor has rank {rank}",
                                    record.fields.len()
                                ),
                                span,
                            ));
                        }
                        for component in record.fields.values() {
                            coords.push(index_of(component, "sparse_coo index", span)?);
                        }
                    },
                    | other => {
                        if rank != 1 {
                            return Err(err(
                                "E0802",
                                format!(
                                    "a rank-{rank} tensor needs index tuples, found {}",
                                    other.type_name()
                                ),
                                span,
                            ));
                        }
                        coords.push(index_of(other, "sparse_coo index", span)?);
                    },
                }
            }
        },
        | other => {
            return Err(err(
                "E0802",
                format!(
                    "indices must be an [nnz, rank] matrix or a list of tuples, found rank {other}"
                ),
                span,
            ));
        },
    }
    for entry in 0..nnz {
        let coord = &coords[entry * rank..(entry + 1) * rank];
        if let Some(axis) = (0..rank).find(|a| coord[*a] >= shape[*a]) {
            return Err(err(
                "E0802",
                format!(
                    "index {coord:?} is out of bounds on axis {axis} for shape {shape:?} (entry {entry})"
                ),
                span,
            ));
        }
    }
    build_from_triplets(interp, SparseFormat::Coo, shape, &coords, values, span)
}

fn sparse_csr(
    interp: &mut Interp,
    args: &[Value],
    span: Span,
) -> R<Value> {
    let rows = int_arg(args, 0, "sparse_csr rows", span)?;
    let cols = int_arg(args, 1, "sparse_csr cols", span)?;
    let ptr = index_list(
        args.get(2).unwrap_or(&Value::Unit),
        "sparse_csr row_ptr",
        span,
    )?;
    let idx = index_list(
        args.get(3).unwrap_or(&Value::Unit),
        "sparse_csr col_indices",
        span,
    )?;
    let values = tensor_arg(args, 4, "sparse_csr", span)?;
    let nnz = values.storage.len();
    if ptr.len() != rows + 1 || ptr.first() != Some(&0) || ptr.last() != Some(&nnz) {
        return Err(err(
            "E0802",
            format!(
                "row_ptr must have {} entries, start at 0 and end at nnz = {nnz}",
                rows + 1
            ),
            span,
        ));
    }
    if idx.len() != nnz {
        return Err(err(
            "E0802",
            format!("{} column index(es) for {nnz} value(s)", idx.len()),
            span,
        ));
    }
    let mut coords = Vec::with_capacity(nnz * 2);
    for r in 0..rows {
        if ptr[r] > ptr[r + 1] {
            return Err(err("E0802", format!("row_ptr decreases at row {r}"), span));
        }
        for k in ptr[r]..ptr[r + 1] {
            if idx[k] >= cols {
                return Err(err(
                    "E0802",
                    format!(
                        "column index {} is out of bounds for {cols} column(s) (row {r})",
                        idx[k]
                    ),
                    span,
                ));
            }
            if k > ptr[r] && idx[k - 1] >= idx[k] {
                return Err(err(
                    "E0802",
                    format!("column indices of row {r} must be strictly increasing"),
                    span,
                ));
            }
            coords.push(r);
            coords.push(idx[k]);
        }
    }
    build_from_triplets(
        interp,
        SparseFormat::Csr,
        vec![rows, cols],
        &coords,
        values,
        span,
    )
}

fn speye(
    n: usize,
    format: SparseFormat,
    span: Span,
) -> R<Value> {
    check_format_rank(format, 2, span)?;
    let mut entries: Entries<f64> = Entries::empty(2);
    for i in 0..n {
        entries.push(&[i, i], 1.0);
    }
    Ok(wrap(format, vec![n, n], Dim::one(), None, 2, entries))
}

fn spdiag(
    diagonal: &TensorData,
    format: SparseFormat,
    span: Span,
) -> R<Value> {
    check_format_rank(format, 2, span)?;
    if diagonal.rank() != 1 {
        return Err(err("E0802", "`spdiag` expects a rank-1 tensor", span));
    }
    let n = diagonal.shape[0];
    if let Some(data) = diagonal.dense_flat() {
        let mut entries: Entries<f64> = Entries::empty(2);
        for (i, v) in data.iter().enumerate() {
            if *v != 0.0 {
                entries.push(&[i, i], *v);
            }
        }
        return Ok(wrap(
            format,
            vec![n, n],
            diagonal.dim.clone(),
            diagonal.display.clone(),
            2,
            entries,
        ));
    }
    let (items, base) = tensor_values(diagonal).map_err(|e| e.at(span))?;
    let mut entries: Entries<Value> = Entries::empty(2);
    for (i, v) in items.into_iter().enumerate() {
        if !matches!(&v, Value::Num(num) if num_is_zero(num)) {
            entries.push(&[i, i], v);
        }
    }
    Ok(wrap(
        format,
        vec![n, n],
        diagonal.dim.clone(),
        diagonal.display.clone(),
        base,
        entries,
    ))
}

// ---------------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------------

fn transpose(
    data: &SparseData,
    span: Span,
) -> R<Value> {
    rank2(data, "`transpose`", span)?;
    let shape = vec![data.shape[1], data.shape[0]];
    Ok(if let Some(entries) = data.entries::<f64>() {
        let t = kernels::transpose(&entries, data.shape[1]);
        wrap_like(data, shape, data.dim.clone(), data.display.clone(), 2, t)
    } else {
        let entries = data
            .entries::<Value>()
            .ok_or_else(|| kernels::class_error(span))?;
        let t = kernels::transpose(&entries, data.shape[1]);
        wrap_like(
            data,
            shape,
            data.dim.clone(),
            data.display.clone(),
            zero_rank(data),
            t,
        )
    })
}

/// The storage position of `coord`, if it is stored.
fn position(
    data: &SparseData,
    coord: &[usize],
) -> Option<usize> {
    match &data.pattern {
        | super::Pattern::Coords(coords) => {
            let rank = data.rank();
            let n = data.nnz();
            let (mut lo, mut hi) = (0, n);
            while lo < hi {
                let mid = (lo + hi) / 2;
                match coords[mid * rank..(mid + 1) * rank].cmp(coord) {
                    | std::cmp::Ordering::Equal => return Some(mid),
                    | std::cmp::Ordering::Less => lo = mid + 1,
                    | std::cmp::Ordering::Greater => hi = mid,
                }
            }
            None
        },
        | super::Pattern::Blocked { ptr, idx } => {
            let (br, bc) = data.block;
            let ib = coord[0] / br;
            let slice = &idx[ptr[ib]..ptr[ib + 1]];
            let k = ptr[ib] + slice.binary_search(&(coord[1] / bc)).ok()?;
            let slot = k * br * bc + (coord[0] % br) * bc + coord[1] % bc;
            // Padding zeros are not stored entries.
            let live = match &data.values {
                | super::SpValues::F64(items) => items[slot] != 0.0,
                | super::SpValues::Boxed(items) => {
                    !matches!(&items[slot], Value::Num(n) if num_is_zero(n))
                },
            };
            live.then_some(slot)
        },
        | super::Pattern::Compressed { ptr, idx } => {
            let (line, other) = if data.format == SparseFormat::Csc {
                (coord[1], coord[0])
            } else {
                (coord[0], coord[1])
            };
            let slice = &idx[ptr[line]..ptr[line + 1]];
            slice.binary_search(&other).ok().map(|k| ptr[line] + k)
        },
    }
}

/// `A[i, j, …]`: the stored element, or the implicit zero.
pub(crate) fn index(
    data: &SparseData,
    coordinates: &[isize],
    span: Span,
) -> R<Value> {
    if coordinates.len() != data.rank() {
        return Err(err(
            "E0802",
            format!(
                "a rank-{} sparse tensor needs {} index(es), found {}",
                data.rank(),
                data.rank(),
                coordinates.len()
            ),
            span,
        ));
    }
    let mut coord = Vec::with_capacity(coordinates.len());
    for (axis, (c, extent)) in coordinates.iter().zip(&data.shape).enumerate() {
        let extent_i = isize::try_from(*extent).unwrap_or(isize::MAX);
        let normalized = if *c < 0 { extent_i + c } else { *c };
        if normalized < 0 || normalized >= extent_i {
            return Err(err(
                "E0802",
                format!("index {c} is out of bounds on axis {axis} for extent {extent}"),
                span,
            ));
        }
        coord.push(normalized as usize);
    }
    let mut value = match position(data, &coord) {
        | Some(k) => {
            match &data.values {
                | SpValues::F64(items) => Value::Num(NumVal::float(items[k], data.dim.clone())),
                | SpValues::Boxed(items) => items[k].clone(),
            }
        },
        | None => Value::Num(data.zero.clone()),
    };
    if let (Value::Num(num), Some(display)) = (&mut value, &data.display) {
        num.display = Some(display.clone());
    }
    Ok(value)
}

fn nonzeros(data: &SparseData) -> R<Value> {
    let items: Vec<Value> = data
        .stored()
        .into_iter()
        .map(|(coord, value)| tuple_of(vec![int_tensor(&coord), value]))
        .collect();
    let len = items.len();
    Ok(build_tensor(items, vec![len], Dim::one()))
}

fn map_nonzero(
    interp: &mut Interp,
    args: &[Value],
    span: Span,
) -> R<Value> {
    let function = args.first().cloned().ok_or_else(|| {
        err(
            "E0315",
            "`map_nonzero` takes a function and a sparse tensor",
            span,
        )
    })?;
    let data = sparse_arg(args, 1, "map_nonzero", span)?;
    let entries = data.entries_with(data.boxed_values());
    let mut out: Entries<Value> = Entries::empty(entries.rank);
    let mut dim: Option<Dim> = None;
    let mut rank = 0u8;
    for i in 0..entries.len() {
        let mapped = interp.apply(function.clone(), vec![entries.vals[i].clone()], span)?;
        let Value::Num(num) = &mapped else {
            return Err(err(
                "E0302",
                format!(
                    "`map_nonzero` must return a number, found {}",
                    mapped.type_name()
                ),
                span,
            ));
        };
        match &dim {
            | None => dim = Some(num.dim.clone()),
            | Some(d) if *d != num.dim => {
                return Err(err(
                    "E0412",
                    "`map_nonzero` returned elements of different dimensions",
                    span,
                ));
            },
            | _ => {},
        }
        rank = rank.max(kind_rank(&num.kind));
        if !num_is_zero(num) {
            out.push(entries.coord(i), mapped);
        }
    }
    let base = if entries.len() == 0 {
        zero_rank(data)
    } else {
        rank
    };
    Ok(wrap_like(
        data,
        data.shape.clone(),
        dim.unwrap_or_else(|| data.dim.clone()),
        None,
        base,
        out,
    ))
}

pub(crate) fn negate(
    interp: &mut Interp,
    data: &SparseData,
    span: Span,
) -> R<Value> {
    if let Some(entries) = data.entries::<f64>() {
        let mapped = kernels::map_values(&entries, &mut FloatRing, |_, v| Ok(-*v))?;
        return Ok(wrap_like(
            data,
            data.shape.clone(),
            data.dim.clone(),
            data.display.clone(),
            2,
            mapped,
        ));
    }
    let entries = data
        .entries::<Value>()
        .ok_or_else(|| kernels::class_error(span))?;
    let mut ring = ValueRing { interp, span };
    let mapped = kernels::map_values(&entries, &mut ring, |ring, v| {
        let zero = ring.zero_like(v);
        ring.sub(&zero, v)
    })?;
    Ok(wrap_like(
        data,
        data.shape.clone(),
        data.dim.clone(),
        data.display.clone(),
        zero_rank(data),
        mapped,
    ))
}

// ---------------------------------------------------------------------------
// Reductions
// ---------------------------------------------------------------------------

fn sum(
    interp: &mut Interp,
    args: &[Value],
    span: Span,
) -> R<Value> {
    let data = sparse_arg(args, 0, "sum", span)?;
    let axis = match args.get(1) {
        | Some(value) => Some(index_of(value, "sum axis", span)?),
        | None => None,
    };
    if let Some(axis) = axis {
        if axis >= data.rank() {
            return Err(err(
                "E0802",
                format!(
                    "axis {axis} is out of range for a rank-{} tensor",
                    data.rank()
                ),
                span,
            ));
        }
    }
    if let Some(entries) = data.entries::<f64>() {
        return Ok(match axis {
            | None => {
                let total = kernels::sum_all(&entries, &mut FloatRing)?.unwrap_or(0.0);
                Value::Num(NumVal::float(total, data.dim.clone()))
            },
            | Some(axis) => {
                let out = kernels::sum_axis(&entries, &data.shape, axis, &mut FloatRing)?;
                let mut shape = data.shape.clone();
                shape[axis] = 1;
                dense_value_f64(
                    shape,
                    out.into_iter().map(|v| v.unwrap_or(0.0)).collect(),
                    data.dim.clone(),
                    None,
                )
            },
        });
    }
    let entries = data
        .entries::<Value>()
        .ok_or_else(|| kernels::class_error(span))?;
    let mut ring = ValueRing { interp, span };
    Ok(match axis {
        | None => {
            let total = kernels::sum_all(&entries, &mut ring)?;
            total.unwrap_or_else(|| Value::Num(data.zero.clone()))
        },
        | Some(axis) => {
            let out = kernels::sum_axis(&entries, &data.shape, axis, &mut ring)?;
            let mut shape = data.shape.clone();
            shape[axis] = 1;
            dense_from_options(shape, out, zero_rank(data), data.dim.clone())
        },
    })
}

// ---------------------------------------------------------------------------
// Operators
// ---------------------------------------------------------------------------

fn dims_equal(
    a: &SparseData,
    b_dim: &Dim,
    span: Span,
) -> R<()> {
    if a.dim == *b_dim {
        Ok(())
    } else {
        Err(err(
            "E0412",
            format!(
                "the operands have different element dimensions ({} and {})",
                a.dim, b_dim
            ),
            span,
        ))
    }
}

fn shapes_equal(
    a: &[usize],
    b: &[usize],
    op: &str,
    span: Span,
) -> R<()> {
    if a == b {
        Ok(())
    } else {
        Err(err(
            "E0802",
            format!("shape mismatch in `{op}`: left shape {a:?}, right shape {b:?}"),
            span,
        )
        .with_note("sparse operands must have equal shapes"))
    }
}

/// `+`, `-` or `∘` of two sparse tensors of the same shape. The result keeps
/// the left operand's format.
fn combine(
    interp: &mut Interp,
    a: &SparseData,
    b: &SparseData,
    mode: Merge,
    span: Span,
) -> R<Value> {
    let name = match mode {
        | Merge::Add => "+",
        | Merge::Sub => "-",
        | Merge::Mul => "*",
    };
    shapes_equal(&a.shape, &b.shape, name, span)?;
    let dim = if mode == Merge::Mul {
        a.dim.mul(&b.dim)
    } else {
        dims_equal(a, &b.dim, span)?;
        a.dim.clone()
    };
    let display = if mode == Merge::Mul {
        None
    } else {
        a.display.clone().or_else(|| b.display.clone())
    };
    if let (Some(x), Some(y)) = (a.entries::<f64>(), b.entries::<f64>()) {
        let merged = kernels::merge(&x, &y, mode, &mut FloatRing)?;
        return Ok(wrap_like(a, a.shape.clone(), dim, display, 2, merged));
    }
    let (x, y) = (boxed(a), boxed(b));
    let (Some(ex), Some(ey)) = (x.entries::<Value>(), y.entries::<Value>()) else {
        return Err(kernels::class_error(span));
    };
    let mut ring = ValueRing { interp, span };
    let merged = kernels::merge(&ex, &ey, mode, &mut ring)?;
    Ok(wrap_like(
        a,
        a.shape.clone(),
        dim,
        display,
        zero_rank(a).max(zero_rank(b)),
        merged,
    ))
}

/// A dense operand's elements as `f64`s or boxed values.
fn dense_boxed(tensor: &TensorData) -> R<Vec<Value>> {
    Ok(tensor.elements())
}

fn hadamard_dense(
    interp: &mut Interp,
    a: &SparseData,
    x: &TensorData,
    span: Span,
) -> R<Value> {
    shapes_equal(&a.shape, &x.shape, "*", span)?;
    let dim = a.dim.mul(&x.dim);
    if let (Some(e), Some(dense)) = (a.entries::<f64>(), x.dense_flat()) {
        let out = kernels::hadamard_dense(&e, &a.shape, &dense, &mut FloatRing)?;
        return Ok(wrap_like(a, a.shape.clone(), dim, None, 2, out));
    }
    let b = boxed(a);
    let e = b
        .entries::<Value>()
        .ok_or_else(|| kernels::class_error(span))?;
    let dense = dense_boxed(x)?;
    let mut ring = ValueRing { interp, span };
    let out = kernels::hadamard_dense(&e, &a.shape, &dense, &mut ring)?;
    Ok(wrap_like(a, a.shape.clone(), dim, None, zero_rank(a), out))
}

fn scalar_f64(num: &NumVal) -> Option<f64> {
    match &num.kind {
        | NumKind::Float(f) => Some(*f),
        | NumKind::Int(_) | NumKind::Exact(_) => num.to_f64(),
        | _ => None,
    }
}

/// `A * s`, `s * A`, `A / s`.
fn scale(
    interp: &mut Interp,
    a: &SparseData,
    s: &NumVal,
    divide: bool,
    display: Option<Rc<UnitDef>>,
    span: Span,
) -> R<Value> {
    let dim = if divide {
        a.dim.div(&s.dim)
    } else {
        a.dim.mul(&s.dim)
    };
    if let (Some(entries), Some(factor)) = (a.entries::<f64>(), scalar_f64(s)) {
        let mapped = kernels::map_values(&entries, &mut FloatRing, |_, v| {
            Ok(if divide {
                v / factor
            } else {
                v * factor
            })
        })?;
        return Ok(wrap_like(a, a.shape.clone(), dim, display, 2, mapped));
    }
    let b = boxed(a);
    let entries = b
        .entries::<Value>()
        .ok_or_else(|| kernels::class_error(span))?;
    let factor = Value::Num(s.clone());
    let mut ring = ValueRing { interp, span };
    let mapped = kernels::map_values(&entries, &mut ring, |ring, v| {
        if divide {
            ring.div(v, &factor)
        } else {
            ring.mul(v, &factor)
        }
    })?;
    Ok(wrap_like(
        a,
        a.shape.clone(),
        dim,
        display,
        zero_rank(a).max(kind_rank(&s.kind)),
        mapped,
    ))
}

fn unit_scalar(
    unit: &UnitDef,
    float: bool,
    span: Span,
) -> R<NumVal> {
    if unit.affine {
        return Err(err(
            "E0405",
            "an affine unit cannot scale a sparse tensor",
            span,
        ));
    }
    let mut scale = crate::eval::unit_scale_value(unit, float);
    scale.dim = unit.dim.clone();
    Ok(scale)
}

/// `sparse @ dense`.
fn sparse_dense_matmul(
    interp: &mut Interp,
    a: &SparseData,
    x: &TensorData,
    span: Span,
) -> R<Value> {
    rank2(a, "`@`", span)?;
    let (m, k) = (a.shape[0], a.shape[1]);
    let (xk, n, out_shape) = match x.rank() {
        | 1 => (x.shape[0], 1, vec![m]),
        | 2 => (x.shape[0], x.shape[1], vec![m, x.shape[1]]),
        | other => {
            return Err(err(
                "E0802",
                format!("`@` is not defined for a sparse matrix and a rank-{other} tensor"),
                span,
            ));
        },
    };
    if k != xk {
        return Err(err(
            "E0802",
            format!(
                "shape mismatch in `matmul`: left shape {:?}, right shape {:?}",
                a.shape, x.shape
            ),
            span,
        )
        .with_note("matmul requires left columns = right rows"));
    }
    let dim = a.dim.mul(&x.dim);
    if a.is_f64() {
        if let Some(dense) = x.dense_flat() {
            if let (SparseFormat::Bcsr, super::Pattern::Blocked { ptr, idx }, SpValues::F64(vals)) =
                (a.format, &a.pattern, &a.values)
            {
                let out = kernels::bsr_spmm_f64(ptr, idx, vals, a.block, m, &dense, n);
                return Ok(dense_value_f64(out_shape, out, dim, None));
            }
            let csr = kernels::csr_parts::<f64>(a, span)?;
            let out = kernels::spmm(&csr, &dense, n, &mut FloatRing)?;
            return Ok(dense_value_f64(
                out_shape,
                out.into_iter().map(|v| v.unwrap_or(0.0)).collect(),
                dim,
                None,
            ));
        }
    }
    let b = boxed(a);
    let csr = kernels::csr_parts::<Value>(&b, span)?;
    let dense = dense_boxed(x)?;
    let mut ring = ValueRing { interp, span };
    let out = kernels::spmm(&csr, &dense, n, &mut ring)?;
    Ok(dense_from_options(out_shape, out, zero_rank(a), dim))
}

/// `dense @ sparse`.
fn dense_sparse_matmul(
    interp: &mut Interp,
    x: &TensorData,
    b: &SparseData,
    span: Span,
) -> R<Value> {
    rank2(b, "`@`", span)?;
    let (k, n) = (b.shape[0], b.shape[1]);
    let (p, xk, out_shape) = match x.rank() {
        | 1 => (1, x.shape[0], vec![n]),
        | 2 => (x.shape[0], x.shape[1], vec![x.shape[0], n]),
        | other => {
            return Err(err(
                "E0802",
                format!("`@` is not defined for a rank-{other} tensor and a sparse matrix"),
                span,
            ));
        },
    };
    if k != xk {
        return Err(err(
            "E0802",
            format!(
                "shape mismatch in `matmul`: left shape {:?}, right shape {:?}",
                x.shape, b.shape
            ),
            span,
        )
        .with_note("matmul requires left columns = right rows"));
    }
    let dim = x.dim.mul(&b.dim);
    if b.is_f64() {
        if let Some(dense) = x.dense_flat() {
            let csr = kernels::csr_parts::<f64>(b, span)?;
            let out = kernels::dense_spmm(&dense, p, &csr, &mut FloatRing)?;
            return Ok(dense_value_f64(
                out_shape,
                out.into_iter().map(|v| v.unwrap_or(0.0)).collect(),
                dim,
                None,
            ));
        }
    }
    let bb = boxed(b);
    let csr = kernels::csr_parts::<Value>(&bb, span)?;
    let dense = dense_boxed(x)?;
    let mut ring = ValueRing { interp, span };
    let out = kernels::dense_spmm(&dense, p, &csr, &mut ring)?;
    Ok(dense_from_options(out_shape, out, zero_rank(b), dim))
}

/// `sparse @ sparse`: a CSR matrix.
fn sparse_sparse_matmul(
    interp: &mut Interp,
    a: &SparseData,
    b: &SparseData,
    span: Span,
) -> R<Value> {
    rank2(a, "`@`", span)?;
    rank2(b, "`@`", span)?;
    if a.shape[1] != b.shape[0] {
        return Err(err(
            "E0802",
            format!(
                "shape mismatch in `matmul`: left shape {:?}, right shape {:?}",
                a.shape, b.shape
            ),
            span,
        )
        .with_note("matmul requires left columns = right rows"));
    }
    let shape = vec![a.shape[0], b.shape[1]];
    let dim = a.dim.mul(&b.dim);
    if a.is_f64() && b.is_f64() {
        let (x, y) = (
            kernels::csr_parts::<f64>(a, span)?,
            kernels::csr_parts::<f64>(b, span)?,
        );
        let product = kernels::spgemm(&x, &y, &mut FloatRing)?;
        return Ok(wrap(
            SparseFormat::Csr,
            shape,
            dim,
            None,
            2,
            kernels::entries_of_csr(&product),
        ));
    }
    let (ba, bb) = (boxed(a), boxed(b));
    let (x, y) = (
        kernels::csr_parts::<Value>(&ba, span)?,
        kernels::csr_parts::<Value>(&bb, span)?,
    );
    let mut ring = ValueRing { interp, span };
    let product = kernels::spgemm(&x, &y, &mut ring)?;
    Ok(wrap(
        SparseFormat::Csr,
        shape,
        dim,
        None,
        zero_rank(a).max(zero_rank(b)),
        kernels::entries_of_csr(&product),
    ))
}

fn kron(
    interp: &mut Interp,
    a: &SparseData,
    b: &SparseData,
    span: Span,
) -> R<Value> {
    rank2(a, "`kron`", span)?;
    rank2(b, "`kron`", span)?;
    let shape = vec![a.shape[0] * b.shape[0], a.shape[1] * b.shape[1]];
    let dim = a.dim.mul(&b.dim);
    if a.is_f64() && b.is_f64() {
        let (x, y) = (
            kernels::csr_parts::<f64>(a, span)?,
            kernels::csr_parts::<f64>(b, span)?,
        );
        let product = kernels::kron(&x, &y, &mut FloatRing)?;
        return Ok(wrap(
            SparseFormat::Csr,
            shape,
            dim,
            None,
            2,
            kernels::entries_of_csr(&product),
        ));
    }
    let (ba, bb) = (boxed(a), boxed(b));
    let (x, y) = (
        kernels::csr_parts::<Value>(&ba, span)?,
        kernels::csr_parts::<Value>(&bb, span)?,
    );
    let mut ring = ValueRing { interp, span };
    let product = kernels::kron(&x, &y, &mut ring)?;
    Ok(wrap(
        SparseFormat::Csr,
        shape,
        dim,
        None,
        zero_rank(a).max(zero_rank(b)),
        kernels::entries_of_csr(&product),
    ))
}

/// The operators on a sparse operand.
pub(crate) fn binary(
    interp: &mut Interp,
    op: BinOp,
    left: Value,
    right: Value,
    span: Span,
) -> R<Value> {
    match (&left, &right, op) {
        | (Value::Sparse(a), Value::Sparse(b), BinOp::Add) => combine(interp, a, b, Merge::Add, span),
        | (Value::Sparse(a), Value::Sparse(b), BinOp::Sub) => combine(interp, a, b, Merge::Sub, span),
        | (Value::Sparse(a), Value::Sparse(b), BinOp::Mul) => combine(interp, a, b, Merge::Mul, span),
        | (Value::Sparse(a), Value::Sparse(b), BinOp::MatMul) => sparse_sparse_matmul(interp, a, b, span),
        | (Value::Sparse(a), Value::Sparse(b), BinOp::Eq) => Ok(Value::Bool(equal(a, b, interp, span)?)),
        | (Value::Sparse(a), Value::Sparse(b), BinOp::Ne) => Ok(Value::Bool(!equal(a, b, interp, span)?)),
        // Sparse ± dense is dense (§13.9.5: conversions are never implicit, so
        // the result of mixing the two layouts is the layout that holds it).
        | (Value::Sparse(a), Value::Tensor(_), BinOp::Add | BinOp::Sub) => {
            let dense = to_dense(a, span)?;
            interp.binary(op, dense, right.clone(), span)
        },
        | (Value::Tensor(_), Value::Sparse(b), BinOp::Add | BinOp::Sub) => {
            let dense = to_dense(b, span)?;
            interp.binary(op, left.clone(), dense, span)
        },
        | (Value::Sparse(a), Value::Tensor(x), BinOp::Mul) => hadamard_dense(interp, a, x, span),
        | (Value::Tensor(x), Value::Sparse(b), BinOp::Mul) => hadamard_dense(interp, b, x, span),
        | (Value::Sparse(a), Value::Tensor(x), BinOp::MatMul) => sparse_dense_matmul(interp, a, x, span),
        | (Value::Tensor(x), Value::Sparse(b), BinOp::MatMul) => dense_sparse_matmul(interp, x, b, span),
        | (Value::Sparse(a), Value::Num(s), BinOp::Mul | BinOp::Div) => {
            scale(interp, a, s, op == BinOp::Div, None, span)
        },
        | (Value::Num(s), Value::Sparse(a), BinOp::Mul) => scale(interp, a, s, false, None, span),
        | (Value::Sparse(a), Value::UnitOf(unit), BinOp::Mul | BinOp::Div) => {
            let s = unit_scalar(unit, a.is_f64(), span)?;
            let display = (op == BinOp::Mul).then(|| unit.clone());
            scale(interp, a, &s, op == BinOp::Div, display, span)
        },
        | (Value::UnitOf(unit), Value::Sparse(a), BinOp::Mul) => {
            let s = unit_scalar(unit, a.is_f64(), span)?;
            scale(interp, a, &s, false, Some(unit.clone()), span)
        },
        | _ => {
            Err(err(
                "E0302",
                format!(
                    "`{}` is not defined for {} and {}",
                    op.as_str(),
                    left.type_name(),
                    right.type_name()
                ),
                span,
            )
            .with_note("convert explicitly with `to_dense` / `from_dense`; sparse and dense never mix implicitly"))
        },
    }
}

// ---------------------------------------------------------------------------
// Solvers
// ---------------------------------------------------------------------------

fn spsolve(
    interp: &mut Interp,
    a: &SparseData,
    b: &TensorData,
    span: Span,
) -> R<Value> {
    rank2(a, "`spsolve`", span)?;
    let n = a.shape[0];
    if a.shape[1] != n {
        return Err(err(
            "E0802",
            format!("`spsolve` needs a square matrix, found {:?}", a.shape),
            span,
        ));
    }
    if b.rank() != 1 || b.shape[0] != n {
        return Err(err(
            "E0802",
            format!(
                "`spsolve`: the right-hand side must have shape [{n}], found {:?}",
                b.shape
            ),
            span,
        ));
    }
    let dim = b.dim.div(&a.dim);
    if a.is_f64() {
        if let Some(rhs) = b.dense_flat() {
            let csr = kernels::csr_parts::<f64>(a, span)?;
            let view = aevia_std::sparse::Csr {
                rows: n,
                cols: n,
                row_ptr: &csr.ptr,
                col_idx: &csr.idx,
                values: &csr.vals,
            };
            // A symmetric positive-definite matrix goes through sparse
            // Cholesky; anything else (or a breakdown) through sparse LU.
            let (x, _) = aevia_std::sparse::solve_auto(&view, &rhs)
                .map_err(|e| err("E0806", format!("`spsolve`: {e}"), span))?;
            return Ok(dense_value_f64(vec![n], x, dim, None));
        }
    }
    let ba = boxed(a);
    let csr = kernels::csr_parts::<Value>(&ba, span)?;
    let rhs = dense_boxed(b)?;
    let mut ring = ValueRing { interp, span };
    let Some(x) = kernels::solve_elimination(&csr, &rhs, &mut ring)? else {
        return Err(err("E0806", "`spsolve`: the matrix is singular", span));
    };
    let dim = x
        .iter()
        .find_map(|v| {
            match v {
                | Value::Num(num) => Some(num.dim.clone()),
                | _ => None,
            }
        })
        .unwrap_or(dim);
    Ok(build_tensor(x, vec![n], dim))
}

fn cg(
    args: &[Value],
    span: Span,
) -> R<Value> {
    let a = sparse_arg(args, 0, "cg", span)?;
    let b = tensor_arg(args, 1, "cg", span)?;
    let tolerance = args
        .get(2)
        .and_then(Value::as_f64)
        .ok_or_else(|| err("E0302", "`cg` expects a Float tolerance", span))?;
    let max_iterations = int_arg(args, 3, "cg max iterations", span)?;
    rank2(a, "`cg`", span)?;
    let n = a.shape[0];
    if a.shape[1] != n || b.rank() != 1 || b.shape[0] != n {
        return Err(err(
            "E0802",
            format!("`cg` needs a square matrix and a right-hand side of shape [{n}]"),
            span,
        ));
    }
    let (true, Some(rhs)) = (a.is_f64(), b.dense_flat()) else {
        return Err(err(
            "E0302",
            "`cg` iterates in floating point: the matrix and the right-hand side must be Float",
            span,
        )
        .with_note("use `spsolve` for exact elements"));
    };
    let csr = kernels::csr_parts::<f64>(a, span)?;
    let view = aevia_std::sparse::Csr {
        rows: n,
        cols: n,
        row_ptr: &csr.ptr,
        col_idx: &csr.idx,
        values: &csr.vals,
    };
    let result = aevia_std::sparse::cg(&view, &rhs, tolerance, max_iterations)
        .map_err(|e| err("E0806", format!("`cg`: {e}"), span))?;
    Ok(record_of(vec![
        (
            "x",
            dense_value_f64(vec![n], result.x, b.dim.div(&a.dim), None),
        ),
        ("iterations", int_value(result.iterations)),
        ("residual", Value::float(result.residual)),
        ("converged", Value::Bool(result.converged)),
    ]))
}

/// `cg_with(A, b, { tol, maxiter, precond })`: preconditioned conjugate
/// gradients. `precond` is a `Precond` constructor (`Identity`, `Jacobi`,
/// `BlockJacobi`, `Ilu0`, `Ic0`, `Auto`); the result also reports the
/// preconditioner that was really applied.
fn cg_with(
    args: &[Value],
    span: Span,
) -> R<Value> {
    use aevia_std::sparse::PrecondKind;
    let a = sparse_arg(args, 0, "cg_with", span)?;
    let b = tensor_arg(args, 1, "cg_with", span)?;
    let Some(Value::Record(options)) = args.get(2) else {
        return Err(err(
            "E0302",
            "`cg_with` expects an options record `{ tol, maxiter, precond }`",
            span,
        ));
    };
    let field = |name: &str| {
        options.fields.get(name).ok_or_else(|| {
            err(
                "E0302",
                format!("`cg_with` options lack the field `{name}`"),
                span,
            )
        })
    };
    let tolerance = field("tol")?
        .as_f64()
        .ok_or_else(|| err("E0302", "`cg_with`: `tol` must be a Float", span))?;
    let max_iterations = index_of(field("maxiter")?, "cg_with maxiter", span)?;
    let kind = match field("precond")? {
        | Value::Variant(variant) => {
            match variant.name.as_str() {
                | "Identity" => PrecondKind::Identity,
                | "Jacobi" => PrecondKind::Jacobi,
                | "BlockJacobi" => PrecondKind::BlockJacobi,
                | "Ilu0" => PrecondKind::Ilu0,
                | "Ic0" => PrecondKind::Ic0,
                | "Auto" => PrecondKind::Auto,
                | other => {
                    return Err(err(
                        "E0302",
                        format!(
                            "`{other}` is not a `Precond` (Identity, Jacobi, BlockJacobi, Ilu0, Ic0, Auto)"
                        ),
                        span,
                    ));
                },
            }
        },
        | other => {
            return Err(err(
                "E0302",
                format!(
                    "`cg_with`: `precond` must be a `Precond`, found {}",
                    other.type_name()
                ),
                span,
            ));
        },
    };
    rank2(a, "`cg_with`", span)?;
    let n = a.shape[0];
    if a.shape[1] != n || b.rank() != 1 || b.shape[0] != n {
        return Err(err(
            "E0802",
            format!("`cg_with` needs a square matrix and a right-hand side of shape [{n}]"),
            span,
        ));
    }
    let (true, Some(rhs)) = (a.is_f64(), b.dense_flat()) else {
        return Err(err(
            "E0302",
            "`cg_with` iterates in floating point: the matrix and the right-hand side must be Float",
            span,
        )
        .with_note("use `spsolve` for exact elements"));
    };
    let csr = kernels::csr_parts::<f64>(a, span)?;
    let view = aevia_std::sparse::Csr {
        rows: n,
        cols: n,
        row_ptr: &csr.ptr,
        col_idx: &csr.idx,
        values: &csr.vals,
    };
    let block = (a.format == SparseFormat::Bcsr).then_some(a.block);
    let result = aevia_std::sparse::cg_precond(&view, &rhs, tolerance, max_iterations, kind, block)
        .map_err(|e| err("E0806", format!("`cg_with`: {e}"), span))?;
    Ok(record_of(vec![
        (
            "x",
            dense_value_f64(vec![n], result.x, b.dim.div(&a.dim), None),
        ),
        ("iterations", int_value(result.iterations)),
        ("residual", Value::float(result.residual)),
        ("converged", Value::Bool(result.converged)),
        ("precond", Value::string(result.precond)),
    ]))
}

// ---------------------------------------------------------------------------
// Slicing
// ---------------------------------------------------------------------------

/// One axis of a sparse slice, already resolved against the extent.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SliceAxis {
    /// `A[…, i, …]`: keep the entries at `i`; the axis disappears.
    At(usize),
    /// `A[…, a..b, …]` or `a:b:s`: `count` positions from `start` in steps of
    /// `step`; the axis stays with extent `count`.
    Walk {
        start: usize,
        count: usize,
        step: isize,
    },
}

/// The entries of `entries` that fall inside `axes`, re-addressed.
fn slice_entries<T: Clone>(
    entries: &Entries<T>,
    axes: &[SliceAxis],
) -> Entries<T> {
    let out_rank = axes
        .iter()
        .filter(|a| matches!(a, SliceAxis::Walk { .. }))
        .count();
    let mut out: Entries<T> = Entries::empty(out_rank);
    let mut coord = Vec::with_capacity(out_rank);
    'entry: for i in 0..entries.len() {
        coord.clear();
        for (axis, sel) in axes.iter().enumerate() {
            let c = entries.coord(i)[axis];
            match *sel {
                | SliceAxis::At(at) => {
                    if c != at {
                        continue 'entry;
                    }
                },
                | SliceAxis::Walk { start, count, step } => {
                    let d = c as isize - start as isize;
                    if d % step != 0 {
                        continue 'entry;
                    }
                    let k = d / step;
                    if k < 0 || k as usize >= count {
                        continue 'entry;
                    }
                    coord.push(k as usize);
                },
            }
        }
        out.push(&coord, entries.vals[i].clone());
    }
    if axes
        .iter()
        .any(|a| matches!(a, SliceAxis::Walk { step, .. } if *step < 0))
    {
        // A backwards walk reverses the order: restore the sorted form.
        let mut order: Vec<usize> = (0..out.len()).collect();
        order.sort_by(|x, y| out.coord(*x).cmp(out.coord(*y)));
        let mut sorted: Entries<T> = Entries::empty(out_rank);
        for k in order {
            sorted.push(out.coord(k), out.vals[k].clone());
        }
        return sorted;
    }
    out
}

/// `A[r0..r1, c0..c1]`, `A[i, :]`, `A[:, j]`, … on a sparse tensor. Nothing
/// is densified: the stored entries are filtered and re-addressed.
///
/// An integer axis disappears (as for dense tensors), a range keeps its axis.
/// A result of rank 2 keeps the format of `data`; any other rank is `Coo`.
pub(crate) fn slice(
    data: &SparseData,
    axes: &[SliceAxis],
    span: Span,
) -> R<Value> {
    if axes.len() != data.rank() {
        return Err(err(
            "E0802",
            format!(
                "a rank-{} sparse tensor is sliced by {} selector(s), found {}",
                data.rank(),
                data.rank(),
                axes.len()
            ),
            span,
        ));
    }
    let shape: Vec<usize> = axes
        .iter()
        .filter_map(|a| {
            match a {
                | SliceAxis::Walk { count, .. } => Some(*count),
                | SliceAxis::At(_) => None,
            }
        })
        .collect();
    let format = if shape.len() == 2 && data.format != SparseFormat::Coo {
        data.format
    } else {
        SparseFormat::Coo
    };
    Ok(if let Some(entries) = data.entries::<f64>() {
        let sliced = slice_entries(&entries, axes);
        wrap(
            format,
            shape,
            data.dim.clone(),
            data.display.clone(),
            2,
            sliced,
        )
    } else {
        let entries = data
            .entries::<Value>()
            .ok_or_else(|| kernels::class_error(span))?;
        let sliced = slice_entries(&entries, axes);
        wrap(
            format,
            shape,
            data.dim.clone(),
            data.display.clone(),
            zero_rank(data),
            sliced,
        )
    })
}

// ---------------------------------------------------------------------------
// Contraction
// ---------------------------------------------------------------------------

/// A tensor's nonzero entries (a dense operand viewed sparsely).
fn dense_entries<T: Store>(
    tensor: &TensorData,
    class: impl Fn(&TensorData) -> Option<Vec<T>>,
    is_zero: impl Fn(&T) -> bool,
    span: Span,
) -> R<Entries<T>> {
    let values = class(tensor).ok_or_else(|| kernels::class_error(span))?;
    let mut out = Entries::empty(tensor.rank());
    let mut coord = vec![0usize; tensor.rank()];
    for (flat, v) in values.into_iter().enumerate() {
        if !is_zero(&v) {
            kernels::unflatten(flat, &tensor.shape, &mut coord);
            out.push(&coord, v);
        }
    }
    Ok(out)
}

fn contract(
    interp: &mut Interp,
    args: &[Value],
    span: Span,
) -> R<Value> {
    let axes_a = index_list(args.get(2).unwrap_or(&Value::Unit), "contract axes", span)?;
    let axes_b = index_list(args.get(3).unwrap_or(&Value::Unit), "contract axes", span)?;
    if axes_a.len() != axes_b.len() {
        return Err(err(
            "E0802",
            "`contract` needs the same number of axes on both sides",
            span,
        ));
    }
    // Operands: sparse, or dense viewed sparsely.
    enum Operand<'a> {
        Sparse(&'a SparseData),
        Dense(&'a TensorData),
    }
    let operand = |index: usize| -> R<Operand<'_>> {
        match args.get(index) {
            | Some(Value::Sparse(data)) => Ok(Operand::Sparse(data)),
            | Some(Value::Tensor(data)) => Ok(Operand::Dense(data)),
            | _ => {
                Err(err(
                    "E0302",
                    "`contract` expects tensors or sparse tensors",
                    span,
                ))
            },
        }
    };
    let (left, right) = (operand(0)?, operand(1)?);
    let (shape_a, dim_a, rank_a) = match &left {
        | Operand::Sparse(d) => (d.shape.clone(), d.dim.clone(), zero_rank(d)),
        | Operand::Dense(d) => (d.shape.clone(), d.dim.clone(), 2),
    };
    let (shape_b, dim_b, rank_b) = match &right {
        | Operand::Sparse(d) => (d.shape.clone(), d.dim.clone(), zero_rank(d)),
        | Operand::Dense(d) => (d.shape.clone(), d.dim.clone(), 2),
    };
    for (x, y) in axes_a.iter().zip(&axes_b) {
        if *x >= shape_a.len() || *y >= shape_b.len() {
            return Err(err("E0802", "a contraction axis is out of range", span));
        }
        if shape_a[*x] != shape_b[*y] {
            return Err(err(
                "E0802",
                format!(
                    "cannot contract axis {x} (extent {}) with axis {y} (extent {})",
                    shape_a[*x], shape_b[*y]
                ),
                span,
            ));
        }
    }
    let free_a: Vec<usize> = (0..shape_a.len()).filter(|x| !axes_a.contains(x)).collect();
    let free_b: Vec<usize> = (0..shape_b.len()).filter(|x| !axes_b.contains(x)).collect();
    let out_shape: Vec<usize> = free_a
        .iter()
        .map(|x| shape_a[*x])
        .chain(free_b.iter().map(|x| shape_b[*x]))
        .collect();
    let dim = dim_a.mul(&dim_b);
    let both_dense = matches!((&left, &right), (Operand::Dense(_), Operand::Dense(_)));
    if both_dense {
        return Err(err(
            "E0302",
            "`contract` needs at least one sparse operand",
            span,
        ));
    }
    let result_sparse = matches!((&left, &right), (Operand::Sparse(_), Operand::Sparse(_)));
    let float_class = match (&left, &right) {
        | (Operand::Sparse(x), Operand::Sparse(y)) => x.is_f64() && y.is_f64(),
        | (Operand::Sparse(x), Operand::Dense(y)) | (Operand::Dense(y), Operand::Sparse(x)) => {
            x.is_f64() && y.dense_flat().is_some()
        },
        | _ => false,
    };
    // The result entries (sorted), in the element class of the operands.
    let finish_f64 = |entries: Entries<f64>| -> R<Value> {
        finish_contract_f64(entries, out_shape.clone(), dim.clone(), result_sparse, span)
    };
    if float_class {
        let ea: Entries<f64> = match &left {
            | Operand::Sparse(d) => {
                d.entries::<f64>()
                    .ok_or_else(|| kernels::class_error(span))?
            },
            | Operand::Dense(t) => {
                dense_entries(
                    t,
                    |t| t.dense_flat().map(|c| c.into_owned()),
                    |v| *v == 0.0,
                    span,
                )?
            },
        };
        let eb: Entries<f64> = match &right {
            | Operand::Sparse(d) => {
                d.entries::<f64>()
                    .ok_or_else(|| kernels::class_error(span))?
            },
            | Operand::Dense(t) => {
                dense_entries(
                    t,
                    |t| t.dense_flat().map(|c| c.into_owned()),
                    |v| *v == 0.0,
                    span,
                )?
            },
        };
        let out = kernels::contract(&ea, &eb, &axes_a, &axes_b, &mut FloatRing)?;
        return finish_f64(out);
    }
    let boxed_entries = |op: &Operand<'_>| -> R<Entries<Value>> {
        match op {
            | Operand::Sparse(d) => Ok(d.entries_with(d.boxed_values())),
            | Operand::Dense(t) => {
                dense_entries(
                    t,
                    |t| Some(t.elements()),
                    |v| matches!(v, Value::Num(n) if num_is_zero(n)),
                    span,
                )
            },
        }
    };
    let (ea, eb) = (boxed_entries(&left)?, boxed_entries(&right)?);
    let mut ring = ValueRing { interp, span };
    let out = kernels::contract(&ea, &eb, &axes_a, &axes_b, &mut ring)?;
    let base = rank_a.max(rank_b);
    if out_shape.is_empty() {
        return Ok(out
            .vals
            .into_iter()
            .next()
            .unwrap_or_else(|| Value::Num(zero_num(base, &dim))));
    }
    if result_sparse {
        return Ok(wrap(SparseFormat::Coo, out_shape, dim, None, base, out));
    }
    let count = element_count(&out_shape, span)?;
    let mut slots: Vec<Option<Value>> = vec![None; count];
    for i in 0..out.len() {
        slots[kernels::flat_index(out.coord(i), &out_shape)] = Some(out.vals[i].clone());
    }
    Ok(dense_from_options(out_shape, slots, base, dim))
}

fn finish_contract_f64(
    entries: Entries<f64>,
    shape: Vec<usize>,
    dim: Dim,
    sparse: bool,
    span: Span,
) -> R<Value> {
    if shape.is_empty() {
        let total = entries.vals.first().copied().unwrap_or(0.0);
        return Ok(Value::Num(NumVal::float(total, dim)));
    }
    if sparse {
        return Ok(wrap(SparseFormat::Coo, shape, dim, None, 2, entries));
    }
    let count = element_count(&shape, span)?;
    let mut out = vec![0.0; count];
    for i in 0..entries.len() {
        out[kernels::flat_index(entries.coord(i), &shape)] = entries.vals[i];
    }
    Ok(dense_value_f64(shape, out, dim, None))
}

// ---------------------------------------------------------------------------
// Equality and rendering
// ---------------------------------------------------------------------------

/// Logical equality: the same shape and the same entries, whatever the
/// formats. Elements compare as numbers do (`1` equals `1.0`), exactly as
/// dense tensors compare them.
pub(crate) fn equal(
    a: &SparseData,
    b: &SparseData,
    _interp: &mut Interp,
    _span: Span,
) -> R<bool> {
    Ok(equal_data(a, b))
}

pub(crate) fn equal_data(
    a: &SparseData,
    b: &SparseData,
) -> bool {
    if a.shape != b.shape || a.nnz() != b.nnz() {
        return false;
    }
    let ea = a.entries_with(a.boxed_values());
    let eb = b.entries_with(b.boxed_values());
    ea.coords == eb.coords
        && ea
            .vals
            .iter()
            .zip(&eb.vals)
            .all(|(x, y)| crate::value::equal(x, y).unwrap_or(false))
}

fn show_shape(shape: &[usize]) -> String {
    let parts: Vec<String> = shape.iter().map(ToString::to_string).collect();
    format!("[{}]", parts.join(", "))
}

fn coord_text(coord: &[usize]) -> String {
    if coord.len() == 1 {
        return coord[0].to_string();
    }
    let parts: Vec<String> = coord.iter().map(ToString::to_string).collect();
    format!("({})", parts.join(", "))
}

/// `show` / `repr` of a sparse tensor.
///
/// `repr` is an expression that rebuilds the same value:
/// `convert<Csr>(sparse_coo(shape, indices, values))`.
pub(crate) fn render(
    data: &SparseData,
    repr: bool,
) -> String {
    let mut stored = data.stored();
    if let Some(display) = &data.display {
        for (_, value) in &mut stored {
            if let Value::Num(num) = value {
                num.display = Some(display.clone());
            }
        }
    }
    let text = |value: &Value| {
        if repr {
            crate::value::repr(value)
        } else {
            crate::value::show(value)
        }
    };
    if repr {
        let indices: Vec<String> = stored.iter().map(|(c, _)| coord_text(c)).collect();
        let values: Vec<String> = stored.iter().map(|(_, v)| text(v)).collect();
        let base = format!(
            "sparse_coo({}, [{}], [{}])",
            show_shape(&data.shape),
            indices.join(", "),
            values.join(", ")
        );
        return if data.format == SparseFormat::Coo {
            base
        } else if data.format == SparseFormat::Bcsr {
            format!("to_bcsr({base}, {}, {})", data.block.0, data.block.1)
        } else {
            format!("convert<{}>({base})", data.format.name())
        };
    }
    const LIMIT: usize = 64;
    let mut body: Vec<String> = stored
        .iter()
        .take(LIMIT)
        .map(|(c, v)| format!("{} = {}", coord_text(c), text(v)))
        .collect();
    if stored.len() > LIMIT {
        body.push(format!("... ({} more)", stored.len() - LIMIT));
    }
    format!(
        "sparse({}, {}, {{ {} }})",
        data.format.name(),
        show_shape(&data.shape),
        body.join(", ")
    )
}
