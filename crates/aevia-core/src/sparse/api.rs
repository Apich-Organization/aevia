//! The language-facing surface of sparse tensors: the builtins, the operator
//! dispatch (`+ - * / @ == !=`), indexing, equality and rendering.

#![deny(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
#![allow(
    clippy::single_call_fn,
    reason = "Each built-in and arithmetic rule has a named handler at the dispatch boundary."
)]

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
use crate::builtin_ids::BuiltinId;
use crate::eval::Interp;
use crate::eval::build_tensor;
use crate::eval::normalize_index;
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
use num_traits::ToPrimitive;
use std::alloc::Layout;
use std::borrow::Cow;
use std::collections::HashSet;
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

const fn zero_rank(data: &SparseData) -> u8 {
    kind_rank(&data.zero.kind)
}

/// Like [`wrap`], keeping the block shape of a `Bcsr` operand when it still
/// divides the result (transposes try the swapped block).
fn wrap_like<T: Store>(
    span: Span,
    like: &SparseData,
    shape: Vec<usize>,
    dim: Dim,
    display: Option<Rc<UnitDef>>,
    zero_rank: u8,
    entries: Entries<T>,
) -> R<Value> {
    if like.format == SparseFormat::Bcsr
        && let [rows, cols] = shape.as_slice()
    {
        let (br, bc) = like.block;
        for (r, c) in [(br, bc), (bc, br)] {
            if r != 0 && c != 0 && rows.is_multiple_of(r) && cols.is_multiple_of(c) {
                check_block_capacity::<T>(*rows, *cols, (r, c), &entries, span)?;
                return Ok(Value::Sparse(Rc::new(
                    SparseData::from_entries_blocked(
                        shape,
                        dim,
                        display,
                        zero_rank,
                        (r, c),
                        entries,
                    )
                    .map_err(|message| err("E0802", message, span))?,
                )));
            }
        }
    }
    wrap(span, like.format, shape, dim, display, zero_rank, entries)
}

fn wrap<T: Store>(
    span: Span,
    format: SparseFormat,
    shape: Vec<usize>,
    dim: Dim,
    display: Option<Rc<UnitDef>>,
    zero_rank: u8,
    entries: Entries<T>,
) -> R<Value> {
    check_format_capacity(format, &shape, span)?;
    Ok(Value::Sparse(Rc::new(
        SparseData::from_entries(format, shape, dim, display, zero_rank, entries)
            .map_err(|message| err("E0802", message, span))?,
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
        | Some(Value::Sparse(data)) => {
            if !matches!(
                name,
                "get" | "nnz" | "density" | "format_of" | "shape" | "rank" | "block_size"
            ) {
                check_sparse_layout(data, span)?;
            }
            Ok(data)
        },
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
                format!("`{name}` is missing argument {}", index.saturating_add(1)),
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
        | Some(Value::Tensor(data)) => {
            check_dense_layout(data, span)?;
            Ok(data)
        },
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
                format!("`{name}` is missing argument {}", index.saturating_add(1)),
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
    let value = args.get(index).ok_or_else(|| {
        err(
            "E0315",
            format!("`{name}` is missing argument {}", index.saturating_add(1)),
            span,
        )
    })?;
    index_of(value, name, span)
}

fn rank2(
    data: &SparseData,
    what: &str,
    span: Span,
) -> R<(usize, usize)> {
    if let [rows, cols] = data.shape.as_slice() {
        Ok((*rows, *cols))
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

fn check_dense_layout(
    data: &TensorData,
    span: Span,
) -> R<()> {
    if data.checked_element_count() == Some(data.storage.len()) {
        Ok(())
    } else {
        Err(err(
            "E0802",
            "tensor storage does not match its shape",
            span,
        ))
    }
}

fn materialized_values(
    data: &TensorData,
    span: Span,
) -> R<Vec<Value>> {
    check_dense_layout(data, span)?;
    let values = data.elements();
    if values.len() == data.storage.len() {
        Ok(values)
    } else {
        Err(err(
            "E0802",
            "tensor view cannot provide all its elements",
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

const fn index_buffer_fits(count: usize) -> bool {
    Layout::array::<usize>(count).is_ok()
}

fn pointer_buffer_fits(lines: usize) -> bool {
    lines.checked_add(1).is_some_and(index_buffer_fits)
}

fn check_format_capacity(
    format: SparseFormat,
    shape: &[usize],
    span: Span,
) -> R<()> {
    check_format_rank(format, shape.len(), span)?;
    let fits = match (format, shape) {
        | (SparseFormat::Csr | SparseFormat::Bcsr, [rows, _]) => pointer_buffer_fits(*rows),
        | (SparseFormat::Csc, [_, cols]) => pointer_buffer_fits(*cols),
        | _ => true,
    };
    if fits {
        Ok(())
    } else {
        Err(err(
            "E0802",
            format!(
                "{} index storage is too large for shape {shape:?}",
                format.name()
            ),
            span,
        ))
    }
}

fn check_block_capacity<T>(
    rows: usize,
    cols: usize,
    block: (usize, usize),
    entries: &Entries<T>,
    span: Span,
) -> R<()> {
    let (br, bc) = block;
    let error = || err("E0802", "block CSR storage is too large", span);
    let area = br.checked_mul(bc).ok_or_else(error)?;
    if !rows.checked_div(br).is_some_and(pointer_buffer_fits) || cols.checked_div(bc).is_none() {
        return Err(error());
    }
    let buffers_fit = |blocks: usize| {
        blocks
            .checked_mul(area)
            .is_some_and(|slots| Layout::array::<T>(slots).is_ok())
    };
    if !buffers_fit(entries.len()) {
        // Several nonzeros can share a block. Count them exactly before rejecting a large request.
        let blocks: Option<HashSet<_>> = entries
            .coords
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[row, col]| row.checked_div(br).zip(col.checked_div(bc)))
            .collect();
        if !blocks.is_some_and(|blocks| buffers_fit(blocks.len())) {
            return Err(error());
        }
    }
    Ok(())
}

/// Checks the public metadata before the private storage is decoded.
fn sparse_layout_is_consistent(data: &SparseData) -> bool {
    let slots = data.stored_len();
    let compressed = |ptr: &[usize], idx: &[usize], outer: usize, inner: usize| {
        outer.checked_add(1) == Some(ptr.len())
            && ptr.first() == Some(&0)
            && ptr.last() == Some(&idx.len())
            && ptr.array_windows::<2>().all(|[start, end]| start <= end)
            && idx.iter().all(|index| *index < inner)
    };
    match (data.format, &data.pattern, data.shape.as_slice()) {
        | (SparseFormat::Coo, super::Pattern::Coords(coords), []) => {
            coords.is_empty() && slots <= 1
        },
        | (SparseFormat::Coo, super::Pattern::Coords(coords), shape) => {
            shape.len().checked_mul(slots) == Some(coords.len())
                && coords
                    .chunks_exact(shape.len())
                    .all(|coordinate| outside_axis(coordinate, shape).is_none())
        },
        | (SparseFormat::Csr, super::Pattern::Compressed { ptr, idx }, [rows, cols]) => {
            idx.len() == slots && compressed(ptr, idx, *rows, *cols)
        },
        | (SparseFormat::Csc, super::Pattern::Compressed { ptr, idx }, [rows, cols]) => {
            idx.len() == slots && compressed(ptr, idx, *cols, *rows)
        },
        | (SparseFormat::Bcsr, super::Pattern::Blocked { ptr, idx }, [rows, cols]) => {
            let (br, bc) = data.block;
            br != 0
                && bc != 0
                && rows.is_multiple_of(br)
                && cols.is_multiple_of(bc)
                && idx
                    .len()
                    .checked_mul(br)
                    .and_then(|count| count.checked_mul(bc))
                    == Some(slots)
                && br.checked_mul(bc).is_some()
                && rows
                    .checked_div(br)
                    .zip(cols.checked_div(bc))
                    .is_some_and(|(rows, cols)| compressed(ptr, idx, rows, cols))
        },
        | _ => false,
    }
}

fn outside_axis(
    coordinate: &[usize],
    shape: &[usize],
) -> Option<usize> {
    coordinate
        .iter()
        .zip(shape)
        .position(|(index, extent)| index >= extent)
}

fn check_sparse_layout(
    data: &SparseData,
    span: Span,
) -> R<()> {
    if sparse_layout_is_consistent(data) {
        Ok(())
    } else {
        Err(err(
            "E0802",
            "sparse storage does not match its shape or format",
            span,
        ))
    }
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
        | TyKind::Named { path, args } if args.is_empty() => path.last().map(String::as_str),
        | TyKind::Var(name) => Some(name.as_str()),
        | _ => None,
    };
    let name = name.ok_or_else(|| {
        err(
            "E0310",
            "expected a sparse format name (Csr, Csc, Coo or Bcsr)",
            span,
        )
    })?;
    SparseFormat::parse(name, span)
}

/// A builtin called with explicit type arguments (`from_dense<Coo>(A)`).
pub fn call_typed(
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
pub fn call(
    interp: &mut Interp,
    id: BuiltinId,
    name: &str,
    args: &[Value],
    span: Span,
) -> Option<R<Value>> {
    use BuiltinId as B;
    let always = matches!(
        id,
        B::FromDense
            | B::ToDense
            | B::Nnz
            | B::Density
            | B::FormatOf
            | B::Convert
            | B::ToBcsr
            | B::BlockSize
            | B::SparseCoo
            | B::SparseCsr
            | B::Speye
            | B::Spdiag
            | B::Spsolve
            | B::Cg
            | B::CgWith
            | B::GmresWith
            | B::BicgstabWith
            | B::Kron
            | B::Contract
            | B::MapNonzero
            | B::Nonzeros
    );
    let polymorphic = matches!(
        id,
        B::Transpose | B::Matmul | B::Sum | B::Get | B::Shape | B::Rank
    ) && matches!(args.first(), Some(Value::Sparse(_)));
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
        | "density" => Ok(Value::float(density(sparse_arg(args, 0, name, span)?))),
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
            for item in args.iter().skip(1) {
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
        | "cg_with" | "gmres_with" | "bicgstab_with" => iterative_with(name, args, span),
        | "map_nonzero" => map_nonzero(interp, args, span),
        | "nonzeros" => Ok(nonzeros(sparse_arg(args, 0, name, span)?)),
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

fn density(data: &SparseData) -> f64 {
    if data.shape.contains(&0) {
        return 0.0;
    }
    let total = data
        .shape
        .iter()
        .map(|extent| extent.to_f64().unwrap_or(f64::INFINITY))
        .product::<f64>();
    data.nnz().to_f64().unwrap_or(f64::INFINITY) / total
}

fn signed_index(
    value: &Value,
    span: Span,
) -> R<i128> {
    let coordinate = match value.as_num().map(|number| &number.kind) {
        | Some(NumKind::Int(integer)) => integer.to_i128(),
        | Some(NumKind::Exact(rational)) if rational.is_integer() => {
            rational.to_integer().to_i128()
        },
        | Some(NumKind::Float(number)) if number.fract() == 0.0 => number.to_i128(),
        | _ => None,
    };
    coordinate.ok_or_else(|| err("E0802", "a sparse index must be an Int", span))
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

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
    let mut coord = vec![0; rank];
    if let Some(data) = tensor.dense_flat() {
        let mut entries: Entries<f64> = Entries::empty(rank);
        for (flat, value) in data.iter().enumerate() {
            if *value != 0.0 {
                kernels::unflatten(flat, &tensor.shape, &mut coord);
                entries.push(&coord, *value);
            }
        }
        return wrap(
            span,
            format,
            tensor.shape.clone(),
            tensor.dim.clone(),
            tensor.display.clone(),
            2,
            entries,
        );
    }
    let mut entries: Entries<Value> = Entries::empty(rank);
    let mut base = if tensor.storage.is_empty() {
        2
    } else {
        0
    };
    for (flat, element) in materialized_values(tensor, span)?.into_iter().enumerate() {
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
            kernels::unflatten(flat, &tensor.shape, &mut coord);
            entries.push(&coord, element);
        }
    }
    wrap(
        span,
        format,
        tensor.shape.clone(),
        tensor.dim.clone(),
        tensor.display.clone(),
        base,
        entries,
    )
}

fn dense_slot<'a, T>(
    output: &'a mut [T],
    coordinate: &[usize],
    shape: &[usize],
    span: Span,
) -> R<&'a mut T> {
    kernels::flat_index(coordinate, shape)
        .and_then(|offset| output.get_mut(offset))
        .ok_or_else(|| err("E0802", "sparse coordinate is out of bounds", span))
}

fn dense_slots<T: Clone>(
    count: usize,
    fill: T,
    span: Span,
) -> R<Vec<T>> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| err("E0802", "dense result storage is too large", span))?;
    output.resize(count, fill);
    Ok(output)
}

fn to_dense(
    data: &SparseData,
    span: Span,
) -> R<Value> {
    let count = element_count(&data.shape, span)?;
    if let Some(entries) = data.entries::<f64>() {
        let mut out = dense_slots(count, 0.0, span)?;
        for (i, &value) in entries.vals.iter().enumerate() {
            *dense_slot(&mut out, entries.coord(i), &data.shape, span)? = value;
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
    let mut out = dense_slots(count, zero, span)?;
    for (i, value) in entries.vals.iter().enumerate() {
        *dense_slot(&mut out, entries.coord(i), &data.shape, span)? = value.clone();
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
    let (rows, cols) = rank2(data, "`to_bcsr`", span)?;
    if br == 0 || bc == 0 || !rows.is_multiple_of(br) || !cols.is_multiple_of(bc) {
        return Err(err(
            "E0802",
            format!(
                "a {br}x{bc} block does not tile a {rows}x{cols} matrix (the block extents must be positive divisors of the shape)"
            ),
            span,
        ));
    }
    if br.checked_mul(bc).is_none() {
        return Err(err("E0802", "the block area is too large", span));
    }
    if data.format == SparseFormat::Bcsr && data.block == (br, bc) {
        return Ok(Value::Sparse(Rc::new(data.clone())));
    }
    let block = (br, bc);
    Ok(if let Some(entries) = data.entries::<f64>() {
        check_block_capacity(rows, cols, block, &entries, span)?;
        Value::Sparse(Rc::new(
            SparseData::from_entries_blocked(
                data.shape.clone(),
                data.dim.clone(),
                data.display.clone(),
                2,
                block,
                entries,
            )
            .map_err(|message| err("E0802", message, span))?,
        ))
    } else {
        let entries = data
            .entries::<Value>()
            .ok_or_else(|| kernels::class_error(span))?;
        check_block_capacity(rows, cols, block, &entries, span)?;
        Value::Sparse(Rc::new(
            SparseData::from_entries_blocked(
                data.shape.clone(),
                data.dim.clone(),
                data.display.clone(),
                zero_rank(data),
                block,
                entries,
            )
            .map_err(|message| err("E0802", message, span))?,
        ))
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
    if let Some(entries) = data.entries::<f64>() {
        wrap(
            span,
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
            span,
            target,
            data.shape.clone(),
            data.dim.clone(),
            data.display.clone(),
            zero_rank(data),
            entries,
        )
    }
}

/// The element values of a 1-D tensor, and the representation class they
/// start the implicit zero at.
fn tensor_values(
    tensor: &TensorData,
    span: Span,
) -> R<(Vec<Value>, u8)> {
    let items = materialized_values(tensor, span)?;
    let mut rank = if items.is_empty() {
        match &tensor.storage {
            | Storage::TypedExact(_, crate::value::ExactRep::Int) => 0,
            | Storage::Exact(_) | Storage::TypedExact(..) => 1,
            | _ => 2,
        }
    } else {
        0
    };
    for item in &items {
        match item {
            | Value::Num(num) => rank = rank.max(kind_rank(&num.kind)),
            | other => {
                return Err(err(
                    "E0302",
                    format!("sparse values must be numbers, found {}", other.type_name()),
                    span,
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
        return wrap(
            span,
            format,
            shape,
            values.dim.clone(),
            values.display.clone(),
            2,
            entries,
        );
    }
    let (items, base) = tensor_values(values, span)?;
    let mut ring = ValueRing { interp, span };
    let entries = kernels::assemble(rank, coords, items, &mut ring)?;
    wrap(
        span,
        format,
        shape,
        values.dim.clone(),
        values.display.clone(),
        base,
        entries,
    )
}

fn coordinate_buffer(
    entries: usize,
    rank: usize,
    message: &str,
    span: Span,
) -> R<Vec<usize>> {
    let count = entries
        .checked_mul(rank)
        .ok_or_else(|| err("E0802", message, span))?;
    let mut coordinates = Vec::new();
    coordinates
        .try_reserve_exact(count)
        .map_err(|_| err("E0802", message, span))?;
    Ok(coordinates)
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
    let mut coords = coordinate_buffer(nnz, rank, "too many sparse coordinates", span)?;
    match indices.shape.as_slice() {
        | [entries, width] => {
            if *width != rank || *entries != nnz {
                return Err(err(
                    "E0802",
                    format!(
                        "indices have shape {:?}, but {nnz} value(s) of a rank-{rank} tensor need [{nnz}, {rank}]",
                        indices.shape
                    ),
                    span,
                ));
            }
            for item in materialized_values(indices, span)? {
                coords.push(index_of(&item, "sparse_coo index", span)?);
            }
        },
        | [entries] => {
            if *entries != nnz {
                return Err(err(
                    "E0802",
                    format!("{entries} index entr(ies) for {nnz} value(s)"),
                    span,
                ));
            }
            for item in materialized_values(indices, span)? {
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
        | _ => {
            return Err(err(
                "E0802",
                format!(
                    "indices must be an [nnz, rank] matrix or a list of tuples, found rank {}",
                    indices.rank(),
                ),
                span,
            ));
        },
    }
    for (entry, coord) in coords.chunks_exact(rank).enumerate() {
        if let Some(axis) = outside_axis(coord, &shape) {
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
    let pointer_count = rows
        .checked_add(1)
        .ok_or_else(|| err("E0802", "too many CSR rows", span))?;
    if ptr.len() != pointer_count || ptr.first() != Some(&0) || ptr.last() != Some(&nnz) {
        return Err(err(
            "E0802",
            format!("row_ptr must have {pointer_count} entries, start at 0 and end at nnz = {nnz}"),
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
    let mut coords = coordinate_buffer(nnz, 2, "too many CSR coordinates", span)?;
    for (row, &[start, end]) in ptr.array_windows::<2>().enumerate() {
        if start > end {
            return Err(err(
                "E0802",
                format!("row_ptr decreases at row {row}"),
                span,
            ));
        }
        let columns = idx.get(start..end).ok_or_else(|| {
            err(
                "E0802",
                format!("row_ptr points beyond nnz = {nnz} at row {row}"),
                span,
            )
        })?;
        let mut previous = None;
        for &column in columns {
            if column >= cols {
                return Err(err(
                    "E0802",
                    format!(
                        "column index {column} is out of bounds for {cols} column(s) (row {row})"
                    ),
                    span,
                ));
            }
            if previous.is_some_and(|previous| previous >= column) {
                return Err(err(
                    "E0802",
                    format!("column indices of row {row} must be strictly increasing"),
                    span,
                ));
            }
            previous = Some(column);
            coords.extend_from_slice(&[row, column]);
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
    entries.coords = coordinate_buffer(n, 2, "identity matrix is too large", span)?;
    entries
        .vals
        .try_reserve_exact(n)
        .map_err(|_| err("E0802", "identity matrix is too large", span))?;
    for i in 0..n {
        entries.push(&[i, i], 1.0);
    }
    wrap(span, format, vec![n, n], Dim::one(), None, 2, entries)
}

fn spdiag(
    diagonal: &TensorData,
    format: SparseFormat,
    span: Span,
) -> R<Value> {
    check_format_rank(format, 2, span)?;
    let [n] = diagonal.shape.as_slice() else {
        return Err(err("E0802", "`spdiag` expects a rank-1 tensor", span));
    };
    let n = *n;
    if let Some(data) = diagonal.dense_flat() {
        let mut entries: Entries<f64> = Entries::empty(2);
        for (i, v) in data.iter().enumerate() {
            if *v != 0.0 {
                entries.push(&[i, i], *v);
            }
        }
        return wrap(
            span,
            format,
            vec![n, n],
            diagonal.dim.clone(),
            diagonal.display.clone(),
            2,
            entries,
        );
    }
    let (items, base) = tensor_values(diagonal, span)?;
    let mut entries: Entries<Value> = Entries::empty(2);
    for (i, v) in items.into_iter().enumerate() {
        if !matches!(&v, Value::Num(num) if num_is_zero(num)) {
            entries.push(&[i, i], v);
        }
    }
    wrap(
        span,
        format,
        vec![n, n],
        diagonal.dim.clone(),
        diagonal.display.clone(),
        base,
        entries,
    )
}

// ---------------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------------

fn transpose(
    data: &SparseData,
    span: Span,
) -> R<Value> {
    let (rows, cols) = rank2(data, "`transpose`", span)?;
    let shape = vec![cols, rows];
    if let Some(entries) = data.entries::<f64>() {
        let t = kernels::transpose(&entries, cols).map_err(|error| error.at(span))?;
        wrap_like(
            span,
            data,
            shape,
            data.dim.clone(),
            data.display.clone(),
            2,
            t,
        )
    } else {
        let entries = data
            .entries::<Value>()
            .ok_or_else(|| kernels::class_error(span))?;
        let t = kernels::transpose(&entries, cols).map_err(|error| error.at(span))?;
        wrap_like(
            span,
            data,
            shape,
            data.dim.clone(),
            data.display.clone(),
            zero_rank(data),
            t,
        )
    }
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
                let mid = lo.midpoint(hi);
                let start = mid.checked_mul(rank)?;
                let end = start.checked_add(rank)?;
                match coords.get(start..end)?.cmp(coord) {
                    | std::cmp::Ordering::Equal => return Some(mid),
                    | std::cmp::Ordering::Less => lo = mid.saturating_add(1),
                    | std::cmp::Ordering::Greater => hi = mid,
                }
            }
            None
        },
        | super::Pattern::Blocked { ptr, idx } => {
            let (br, bc) = data.block;
            let [row, col] = coord else { return None };
            let block_row = row.checked_div(br)?;
            let block_col = col.checked_div(bc)?;
            let [start, end] = *ptr.get(block_row..)?.first_chunk::<2>()?;
            let relative = idx.get(start..end)?.binary_search(&block_col).ok()?;
            let block = start.checked_add(relative)?;
            let slot = block
                .checked_mul(br)?
                .checked_add(row.checked_rem(br)?)?
                .checked_mul(bc)?
                .checked_add(col.checked_rem(bc)?)?;
            // Padding zeros are not stored entries.
            let live = match &data.values {
                | super::SpValues::F64(items) => *items.get(slot)? != 0.0,
                | super::SpValues::Boxed(items) => {
                    !matches!(items.get(slot)?, Value::Num(n) if num_is_zero(n))
                },
            };
            live.then_some(slot)
        },
        | super::Pattern::Compressed { ptr, idx } => {
            let [row, col] = *coord else { return None };
            let (line, other) = if data.format == SparseFormat::Csc {
                (col, row)
            } else {
                (row, col)
            };
            let [start, end] = *ptr.get(line..)?.first_chunk::<2>()?;
            let relative = idx.get(start..end)?.binary_search(&other).ok()?;
            start.checked_add(relative)
        },
    }
}

/// `A[i, j, …]`: the stored element, or the implicit zero.
pub fn index(
    data: &SparseData,
    coordinates: &[i128],
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
        let Some(normalized) = normalize_index(*c, *extent) else {
            return Err(err(
                "E0802",
                format!("index {c} is out of bounds on axis {axis} for extent {extent}"),
                span,
            ));
        };
        coord.push(normalized);
    }
    let mut value = match position(data, &coord) {
        | Some(k) => {
            match &data.values {
                | SpValues::F64(items) => {
                    items
                        .get(k)
                        .map(|value| Value::Num(NumVal::float(*value, data.dim.clone())))
                },
                | SpValues::Boxed(items) => items.get(k).cloned(),
            }
            .ok_or_else(|| err("E0802", "sparse position has no stored value", span))?
        },
        | None => Value::Num(data.zero.clone()),
    };
    if let (Value::Num(num), Some(display)) = (&mut value, &data.display) {
        num.display = Some(display.clone());
    }
    Ok(value)
}

fn nonzeros(data: &SparseData) -> Value {
    let items: Vec<Value> = data
        .stored()
        .into_iter()
        .map(|(coord, value)| tuple_of(vec![int_tensor(&coord), value]))
        .collect();
    let len = items.len();
    build_tensor(items, vec![len], Dim::one())
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
    for (i, value) in entries.vals.iter().enumerate() {
        let mapped = interp.apply(function.clone(), vec![value.clone()], span)?;
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
    wrap_like(
        span,
        data,
        data.shape.clone(),
        dim.unwrap_or_else(|| data.dim.clone()),
        None,
        base,
        out,
    )
}

pub fn negate(
    interp: &mut Interp,
    data: &SparseData,
    span: Span,
) -> R<Value> {
    check_sparse_layout(data, span)?;
    if let Some(entries) = data.entries::<f64>() {
        let mapped = kernels::map_values(&entries, &mut FloatRing, |_, v| Ok(-*v))?;
        return wrap_like(
            span,
            data,
            data.shape.clone(),
            data.dim.clone(),
            data.display.clone(),
            2,
            mapped,
        );
    }
    let entries = data
        .entries::<Value>()
        .ok_or_else(|| kernels::class_error(span))?;
    let mut ring = ValueRing { interp, span };
    let mapped = kernels::map_values(&entries, &mut ring, |ring, v| {
        let zero = ring.zero_like(v);
        ring.sub(&zero, v)
    })?;
    wrap_like(
        span,
        data,
        data.shape.clone(),
        data.dim.clone(),
        data.display.clone(),
        zero_rank(data),
        mapped,
    )
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
    if let Some(axis) = axis
        && axis >= data.rank()
    {
        return Err(err(
            "E0802",
            format!(
                "axis {axis} is out of range for a rank-{} tensor",
                data.rank()
            ),
            span,
        ));
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
                *shape
                    .get_mut(axis)
                    .ok_or_else(|| err("E0802", "reduction axis out of range", span))? = 1;
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
            *shape
                .get_mut(axis)
                .ok_or_else(|| err("E0802", "reduction axis out of range", span))? = 1;
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
        return wrap_like(span, a, a.shape.clone(), dim, display, 2, merged);
    }
    let (x, y) = (boxed(a), boxed(b));
    let (Some(ex), Some(ey)) = (x.entries::<Value>(), y.entries::<Value>()) else {
        return Err(kernels::class_error(span));
    };
    let mut ring = ValueRing { interp, span };
    let merged = kernels::merge(&ex, &ey, mode, &mut ring)?;
    wrap_like(
        span,
        a,
        a.shape.clone(),
        dim,
        display,
        zero_rank(a).max(zero_rank(b)),
        merged,
    )
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
        return wrap_like(span, a, a.shape.clone(), dim, None, 2, out);
    }
    let b = boxed(a);
    let e = b
        .entries::<Value>()
        .ok_or_else(|| kernels::class_error(span))?;
    let dense = materialized_values(x, span)?;
    let mut ring = ValueRing { interp, span };
    let out = kernels::hadamard_dense(&e, &a.shape, &dense, &mut ring)?;
    wrap_like(span, a, a.shape.clone(), dim, None, zero_rank(a), out)
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
        return wrap_like(span, a, a.shape.clone(), dim, display, 2, mapped);
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
    wrap_like(
        span,
        a,
        a.shape.clone(),
        dim,
        display,
        zero_rank(a).max(kind_rank(&s.kind)),
        mapped,
    )
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
    matrix: &SparseData,
    operand: &TensorData,
    span: Span,
) -> R<Value> {
    let (rows, inner) = rank2(matrix, "`@`", span)?;
    let (operand_inner, cols, out_shape) = match operand.shape.as_slice() {
        | [length] => (*length, 1, vec![rows]),
        | [length, cols] => (*length, *cols, vec![rows, *cols]),
        | _ => {
            return Err(err(
                "E0802",
                format!(
                    "`@` is not defined for a sparse matrix and a rank-{} tensor",
                    operand.rank()
                ),
                span,
            ));
        },
    };
    if inner != operand_inner {
        return Err(err(
            "E0802",
            format!(
                "shape mismatch in `matmul`: left shape {:?}, right shape {:?}",
                matrix.shape, operand.shape
            ),
            span,
        )
        .with_note("matmul requires left columns = right rows"));
    }
    check_dense_layout(operand, span)?;
    let dim = matrix.dim.mul(&operand.dim);
    if matrix.is_f64()
        && let Some(dense) = operand.dense_flat()
    {
        if let (SparseFormat::Bcsr, super::Pattern::Blocked { ptr, idx }, SpValues::F64(vals)) =
            (matrix.format, &matrix.pattern, &matrix.values)
        {
            let out = kernels::bsr_spmm_f64(ptr, idx, vals, matrix.block, rows, &dense, cols)
                .map_err(|error| error.at(span))?;
            return Ok(dense_value_f64(out_shape, out, dim, None));
        }
        let csr = kernels::csr_parts::<f64>(matrix, span)?;
        let out = kernels::spmm_f64(&csr, &dense, cols).map_err(|error| error.at(span))?;
        return Ok(dense_value_f64(
            out_shape,
            out.into_iter().map(|v| v.unwrap_or(0.0)).collect(),
            dim,
            None,
        ));
    }
    let boxed_matrix = boxed(matrix);
    let csr = kernels::csr_parts::<Value>(&boxed_matrix, span)?;
    let dense = materialized_values(operand, span)?;
    let mut ring = ValueRing { interp, span };
    let out = kernels::spmm(&csr, &dense, cols, &mut ring)?;
    Ok(dense_from_options(out_shape, out, zero_rank(matrix), dim))
}

/// `dense @ sparse`.
fn dense_sparse_matmul(
    interp: &mut Interp,
    operand: &TensorData,
    matrix: &SparseData,
    span: Span,
) -> R<Value> {
    let (inner, cols) = rank2(matrix, "`@`", span)?;
    let (rows, operand_inner, out_shape) = match operand.shape.as_slice() {
        | [length] => (1, *length, vec![cols]),
        | [rows, length] => (*rows, *length, vec![*rows, cols]),
        | _ => {
            return Err(err(
                "E0802",
                format!(
                    "`@` is not defined for a rank-{} tensor and a sparse matrix",
                    operand.rank()
                ),
                span,
            ));
        },
    };
    if inner != operand_inner {
        return Err(err(
            "E0802",
            format!(
                "shape mismatch in `matmul`: left shape {:?}, right shape {:?}",
                operand.shape, matrix.shape
            ),
            span,
        )
        .with_note("matmul requires left columns = right rows"));
    }
    check_dense_layout(operand, span)?;
    let dim = operand.dim.mul(&matrix.dim);
    if matrix.is_f64()
        && let Some(dense) = operand.dense_flat()
    {
        let csr = kernels::csr_parts::<f64>(matrix, span)?;
        let out = kernels::dense_spmm(&dense, rows, &csr, &mut FloatRing)?;
        return Ok(dense_value_f64(
            out_shape,
            out.into_iter().map(|v| v.unwrap_or(0.0)).collect(),
            dim,
            None,
        ));
    }
    let boxed_matrix = boxed(matrix);
    let csr = kernels::csr_parts::<Value>(&boxed_matrix, span)?;
    let dense = materialized_values(operand, span)?;
    let mut ring = ValueRing { interp, span };
    let out = kernels::dense_spmm(&dense, rows, &csr, &mut ring)?;
    Ok(dense_from_options(out_shape, out, zero_rank(matrix), dim))
}

/// `sparse @ sparse`: a CSR matrix.
fn sparse_sparse_matmul(
    interp: &mut Interp,
    a: &SparseData,
    b: &SparseData,
    span: Span,
) -> R<Value> {
    let (left_rows, left_cols) = rank2(a, "`@`", span)?;
    let (right_rows, right_cols) = rank2(b, "`@`", span)?;
    if left_cols != right_rows {
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
    let shape = vec![left_rows, right_cols];
    let dim = a.dim.mul(&b.dim);
    if a.is_f64() && b.is_f64() {
        let (x, y) = (
            kernels::csr_parts::<f64>(a, span)?,
            kernels::csr_parts::<f64>(b, span)?,
        );
        let product = kernels::spgemm(&x, &y, &mut FloatRing)?;
        return wrap(
            span,
            SparseFormat::Csr,
            shape,
            dim,
            None,
            2,
            kernels::entries_of_csr(&product).map_err(|error| error.at(span))?,
        );
    }
    let (ba, bb) = (boxed(a), boxed(b));
    let (x, y) = (
        kernels::csr_parts::<Value>(&ba, span)?,
        kernels::csr_parts::<Value>(&bb, span)?,
    );
    let mut ring = ValueRing { interp, span };
    let product = kernels::spgemm(&x, &y, &mut ring)?;
    wrap(
        span,
        SparseFormat::Csr,
        shape,
        dim,
        None,
        zero_rank(a).max(zero_rank(b)),
        kernels::entries_of_csr(&product).map_err(|error| error.at(span))?,
    )
}

fn kron(
    interp: &mut Interp,
    a: &SparseData,
    b: &SparseData,
    span: Span,
) -> R<Value> {
    let (left_rows, left_cols) = rank2(a, "`kron`", span)?;
    let (right_rows, right_cols) = rank2(b, "`kron`", span)?;
    let (Some(rows), Some(cols)) = (
        left_rows.checked_mul(right_rows),
        left_cols.checked_mul(right_cols),
    ) else {
        return Err(err("E0802", "`kron` output dimensions overflow", span));
    };
    let shape = vec![rows, cols];
    let dim = a.dim.mul(&b.dim);
    if a.is_f64() && b.is_f64() {
        let (x, y) = (
            kernels::csr_parts::<f64>(a, span)?,
            kernels::csr_parts::<f64>(b, span)?,
        );
        let product = kernels::kron(&x, &y, &mut FloatRing)?;
        return wrap(
            span,
            SparseFormat::Csr,
            shape,
            dim,
            None,
            2,
            kernels::entries_of_csr(&product).map_err(|error| error.at(span))?,
        );
    }
    let (ba, bb) = (boxed(a), boxed(b));
    let (x, y) = (
        kernels::csr_parts::<Value>(&ba, span)?,
        kernels::csr_parts::<Value>(&bb, span)?,
    );
    let mut ring = ValueRing { interp, span };
    let product = kernels::kron(&x, &y, &mut ring)?;
    wrap(
        span,
        SparseFormat::Csr,
        shape,
        dim,
        None,
        zero_rank(a).max(zero_rank(b)),
        kernels::entries_of_csr(&product).map_err(|error| error.at(span))?,
    )
}

/// The operators on a sparse operand.
pub fn binary(
    interp: &mut Interp,
    op: BinOp,
    left: Value,
    right: Value,
    span: Span,
) -> R<Value> {
    for value in [&left, &right] {
        if let Value::Sparse(data) = value {
            check_sparse_layout(data, span)?;
        }
    }
    match (&left, &right, op) {
        | (Value::Sparse(a), Value::Sparse(b), BinOp::Add) => combine(interp, a, b, Merge::Add, span),
        | (Value::Sparse(a), Value::Sparse(b), BinOp::Sub) => combine(interp, a, b, Merge::Sub, span),
        | (Value::Sparse(a), Value::Sparse(b), BinOp::Mul) => combine(interp, a, b, Merge::Mul, span),
        | (Value::Sparse(a), Value::Sparse(b), BinOp::MatMul) => sparse_sparse_matmul(interp, a, b, span),
        | (Value::Sparse(a), Value::Sparse(b), BinOp::Eq) => Ok(Value::Bool(equal_entries(a, b))),
        | (Value::Sparse(a), Value::Sparse(b), BinOp::Ne) => Ok(Value::Bool(!equal_entries(a, b))),
        // Sparse ± dense is dense (§13.9.5: conversions are never implicit, so
        // the result of mixing the two layouts is the layout that holds it).
        | (Value::Sparse(a), Value::Tensor(_), BinOp::Add | BinOp::Sub) => {
            let dense = to_dense(a, span)?;
            interp.binary(op, dense, right, span)
        },
        | (Value::Tensor(_), Value::Sparse(b), BinOp::Add | BinOp::Sub) => {
            let dense = to_dense(b, span)?;
            interp.binary(op, left, dense, span)
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
    let (n, cols) = rank2(a, "`spsolve`", span)?;
    if cols != n {
        return Err(err(
            "E0802",
            format!("`spsolve` needs a square matrix, found {:?}", a.shape),
            span,
        ));
    }
    if b.shape.as_slice() != [n] {
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
    if a.is_f64()
        && let Some(rhs) = b.dense_flat()
    {
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
    let ba = boxed(a);
    let csr = kernels::csr_parts::<Value>(&ba, span)?;
    let rhs = materialized_values(b, span)?;
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
    let (n, cols) = rank2(a, "`cg`", span)?;
    if cols != n || b.shape.as_slice() != [n] {
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

fn preconditioner(
    value: &Value,
    name: &str,
    span: Span,
) -> R<aevia_std::sparse::PrecondKind> {
    use aevia_std::sparse::PrecondKind;
    let Value::Variant(variant) = value else {
        return Err(err(
            "E0302",
            format!(
                "`{name}`: `precond` must be a `Precond`, found {}",
                value.type_name()
            ),
            span,
        ));
    };
    Ok(match variant.name.as_str() {
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
    })
}

/// `cg_with(A, b, { tol, maxiter, precond })`: preconditioned conjugate
/// gradients; `gmres_with(A, b, { tol, maxiter, restart, precond })` and
/// `bicgstab_with(A, b, { tol, maxiter, precond })`: the Krylov solvers for
/// general (nonsymmetric) matrices. `precond` is a `Precond` constructor
/// (`Identity`, `Jacobi`, `BlockJacobi`, `Ilu0`, `Ic0`, `Auto`); the result
/// also reports the preconditioner that was really applied.
fn iterative_with(
    name: &str,
    args: &[Value],
    span: Span,
) -> R<Value> {
    use aevia_std::sparse::Krylov;
    let expected = match name {
        | "gmres_with" => "{ tol, maxiter, restart, precond }",
        | _ => "{ tol, maxiter, precond }",
    };
    let a = sparse_arg(args, 0, name, span)?;
    let b = tensor_arg(args, 1, name, span)?;
    let Some(Value::Record(options)) = args.get(2) else {
        return Err(err(
            "E0302",
            format!("`{name}` expects an options record `{expected}`"),
            span,
        ));
    };
    let field = |field_name: &str| {
        options.fields.get(field_name).ok_or_else(|| {
            err(
                "E0302",
                format!("`{name}` options lack the field `{field_name}`"),
                span,
            )
        })
    };
    let tolerance = field("tol")?
        .as_f64()
        .ok_or_else(|| err("E0302", format!("`{name}`: `tol` must be a Float"), span))?;
    let max_iterations = index_of(field("maxiter")?, "maxiter", span)?;
    let kind = preconditioner(field("precond")?, name, span)?;
    let method = match name {
        | "gmres_with" => {
            let restart = index_of(field("restart")?, "restart", span)?;
            if restart == 0 {
                return Err(err(
                    "E0802",
                    "`gmres_with`: `restart` must be positive",
                    span,
                ));
            }
            Some(Krylov::Gmres { restart })
        },
        | "bicgstab_with" => Some(Krylov::Bicgstab),
        | _ => None,
    };
    let (n, cols) = rank2(a, &format!("`{name}`"), span)?;
    if cols != n || b.shape.as_slice() != [n] {
        return Err(err(
            "E0802",
            format!("`{name}` needs a square matrix and a right-hand side of shape [{n}]"),
            span,
        ));
    }
    let (true, Some(rhs)) = (a.is_f64(), b.dense_flat()) else {
        return Err(err(
            "E0302",
            format!("`{name}` iterates in floating point: the matrix and the right-hand side must be Float"),
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
    let result = method
        .map_or_else(
            || aevia_std::sparse::cg_precond(&view, &rhs, tolerance, max_iterations, kind, block),
            |method| {
                aevia_std::sparse::krylov_precond(
                    &view,
                    &rhs,
                    tolerance,
                    max_iterations,
                    method,
                    kind,
                    block,
                )
            },
        )
        .map_err(|e| err("E0806", format!("`{name}`: {e}"), span))?;
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
pub enum SliceAxis {
    /// `A[…, i, …]`: keep the entries at `i`; the axis disappears.
    At(usize),
    /// `A[…, a..b, …]` or `a:b:s`: `count` positions from `start` in steps of
    /// `step`; the axis stays with extent `count`.
    Walk {
        start: usize,
        count: usize,
        step: i128,
    },
}

fn slice_position(
    coordinate: usize,
    start: usize,
    count: usize,
    step: i128,
) -> Option<usize> {
    let distance = if step < 0 {
        start.checked_sub(coordinate)?
    } else {
        coordinate.checked_sub(start)?
    };
    if distance == 0 {
        return (count != 0 && step != 0).then_some(0);
    }
    let magnitude = usize::try_from(step.unsigned_abs()).ok()?;
    let position = distance.checked_div(magnitude)?;
    (distance.is_multiple_of(magnitude) && position < count).then_some(position)
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
    'entry: for (i, value) in entries.vals.iter().enumerate() {
        coord.clear();
        for (sel, &c) in axes.iter().zip(entries.coord(i)) {
            match *sel {
                | SliceAxis::At(at) => {
                    if c != at {
                        continue 'entry;
                    }
                },
                | SliceAxis::Walk { start, count, step } => {
                    let Some(position) = slice_position(c, start, count, step) else {
                        continue 'entry;
                    };
                    coord.push(position);
                },
            }
        }
        out.push(&coord, value.clone());
    }
    if axes
        .iter()
        .any(|a| matches!(a, SliceAxis::Walk { step, .. } if *step < 0))
    {
        // A backwards walk reverses the order: restore the sorted form.
        let mut order: Vec<_> = out.vals.iter().enumerate().collect();
        order.sort_by(|(left, _), (right, _)| out.coord(*left).cmp(out.coord(*right)));
        let mut sorted: Entries<T> = Entries::empty(out_rank);
        for (index, value) in order {
            sorted.push(out.coord(index), value.clone());
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
pub fn slice(
    data: &SparseData,
    axes: &[SliceAxis],
    span: Span,
) -> R<Value> {
    check_sparse_layout(data, span)?;
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
    if let Some(entries) = data.entries::<f64>() {
        let sliced = slice_entries(&entries, axes);
        wrap(
            span,
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
            span,
            format,
            shape,
            data.dim.clone(),
            data.display.clone(),
            zero_rank(data),
            sliced,
        )
    }
}

// ---------------------------------------------------------------------------
// Contraction
// ---------------------------------------------------------------------------

/// A tensor's nonzero entries (a dense operand viewed sparsely).
fn dense_entries<T: Store>(
    tensor: &TensorData,
    class: impl Fn(&TensorData) -> R<Vec<T>>,
    is_zero: impl Fn(&T) -> bool,
) -> R<Entries<T>> {
    let values = class(tensor)?;
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

fn contraction_shape(
    shape_a: &[usize],
    shape_b: &[usize],
    axes_a: &[usize],
    axes_b: &[usize],
    span: Span,
) -> R<Vec<usize>> {
    let mut seen_a = HashSet::with_capacity(axes_a.len());
    let mut seen_b = HashSet::with_capacity(axes_b.len());
    for (x, y) in axes_a.iter().zip(axes_b) {
        let (Some(left_extent), Some(right_extent)) = (shape_a.get(*x), shape_b.get(*y)) else {
            return Err(err("E0802", "a contraction axis is out of range", span));
        };
        if !seen_a.insert(*x) || !seen_b.insert(*y) {
            return Err(err(
                "E0802",
                "a contraction axis appears more than once on the same operand",
                span,
            ));
        }
        if left_extent != right_extent {
            return Err(err(
                "E0802",
                format!(
                    "cannot contract axis {x} (extent {left_extent}) with axis {y} (extent {right_extent})"
                ),
                span,
            ));
        }
    }
    let free = |shape: &[usize], axes: &HashSet<usize>| -> Vec<usize> {
        shape
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(i, extent)| (!axes.contains(&i)).then_some(extent))
            .collect()
    };
    let mut out_shape = free(shape_a, &seen_a);
    out_shape.extend(free(shape_b, &seen_b));
    Ok(out_shape)
}

fn contract(
    interp: &mut Interp,
    args: &[Value],
    span: Span,
) -> R<Value> {
    // Operands: sparse, or dense viewed sparsely.
    enum Operand<'a> {
        Sparse(&'a SparseData),
        Dense(&'a TensorData),
    }
    let axes_a = index_list(args.get(2).unwrap_or(&Value::Unit), "contract axes", span)?;
    let axes_b = index_list(args.get(3).unwrap_or(&Value::Unit), "contract axes", span)?;
    if axes_a.len() != axes_b.len() {
        return Err(err(
            "E0802",
            "`contract` needs the same number of axes on both sides",
            span,
        ));
    }
    let operand = |index: usize| -> R<Operand<'_>> {
        match args.get(index) {
            | Some(Value::Sparse(data)) => {
                check_sparse_layout(data, span)?;
                Ok(Operand::Sparse(data))
            },
            | Some(Value::Tensor(data)) => {
                check_dense_layout(data, span)?;
                Ok(Operand::Dense(data))
            },
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
        | Operand::Sparse(d) => (d.shape.as_slice(), &d.dim, zero_rank(d)),
        | Operand::Dense(d) => (d.shape.as_slice(), &d.dim, 2),
    };
    let (shape_b, dim_b, rank_b) = match &right {
        | Operand::Sparse(d) => (d.shape.as_slice(), &d.dim, zero_rank(d)),
        | Operand::Dense(d) => (d.shape.as_slice(), &d.dim, 2),
    };
    let out_shape = contraction_shape(shape_a, shape_b, &axes_a, &axes_b, span)?;
    let dim = dim_a.mul(dim_b);
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
            x.is_f64() && matches!(y.storage.backing(), Storage::Dense(_) | Storage::View(_))
        },
        | _ => false,
    };
    // The result entries (sorted), in the element class of the operands.
    let copy_numbers = |tensor: &TensorData| -> R<Vec<f64>> {
        tensor
            .dense_flat()
            .map(Cow::into_owned)
            .ok_or_else(|| err("E0802", "tensor view cannot provide all its elements", span))
    };
    if float_class {
        let ea: Entries<f64> = match &left {
            | Operand::Sparse(d) => {
                d.entries::<f64>()
                    .ok_or_else(|| kernels::class_error(span))?
            },
            | Operand::Dense(t) => dense_entries(t, copy_numbers, |v| *v == 0.0)?,
        };
        let eb: Entries<f64> = match &right {
            | Operand::Sparse(d) => {
                d.entries::<f64>()
                    .ok_or_else(|| kernels::class_error(span))?
            },
            | Operand::Dense(t) => dense_entries(t, copy_numbers, |v| *v == 0.0)?,
        };
        let out = kernels::contract(&ea, &eb, &axes_a, &axes_b, &mut FloatRing)?;
        return finish_contract_f64(out, out_shape, dim, result_sparse, span);
    }
    let boxed_entries = |op: &Operand<'_>| -> R<Entries<Value>> {
        match op {
            | Operand::Sparse(d) => Ok(d.entries_with(d.boxed_values())),
            | Operand::Dense(t) => {
                dense_entries(
                    t,
                    |t| materialized_values(t, span),
                    |v| matches!(v, Value::Num(n) if num_is_zero(n)),
                )
            },
        }
    };
    let (ea, eb) = (boxed_entries(&left)?, boxed_entries(&right)?);
    let mut ring = ValueRing { interp, span };
    let out = kernels::contract(&ea, &eb, &axes_a, &axes_b, &mut ring)?;
    finish_contract_boxed(out, out_shape, dim, rank_a.max(rank_b), result_sparse, span)
}

fn finish_contract_boxed(
    entries: Entries<Value>,
    shape: Vec<usize>,
    dim: Dim,
    base: u8,
    sparse: bool,
    span: Span,
) -> R<Value> {
    if shape.is_empty() {
        return Ok(entries
            .vals
            .into_iter()
            .next()
            .unwrap_or_else(|| Value::Num(zero_num(base, &dim))));
    }
    if sparse {
        return wrap(span, SparseFormat::Coo, shape, dim, None, base, entries);
    }
    let count = element_count(&shape, span)?;
    let mut slots = dense_slots(count, None, span)?;
    for (i, value) in entries.vals.iter().enumerate() {
        *dense_slot(&mut slots, entries.coord(i), &shape, span)? = Some(value.clone());
    }
    Ok(dense_from_options(shape, slots, base, dim))
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
        return wrap(span, SparseFormat::Coo, shape, dim, None, 2, entries);
    }
    let count = element_count(&shape, span)?;
    let mut out = dense_slots(count, 0.0, span)?;
    for (i, &value) in entries.vals.iter().enumerate() {
        *dense_slot(&mut out, entries.coord(i), &shape, span)? = value;
    }
    Ok(dense_value_f64(shape, out, dim, None))
}

// ---------------------------------------------------------------------------
// Equality and rendering
// ---------------------------------------------------------------------------

/// Logical equality: the same shape and the same entries, whatever the
/// formats. Elements compare as numbers do (`1` equals `1.0`), exactly as
/// dense tensors compare them.
pub fn equal_data(
    a: &SparseData,
    b: &SparseData,
) -> bool {
    sparse_layout_is_consistent(a) && sparse_layout_is_consistent(b) && equal_entries(a, b)
}

fn equal_entries(
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
    if let [index] = coord {
        return index.to_string();
    }
    let parts: Vec<String> = coord.iter().map(ToString::to_string).collect();
    format!("({})", parts.join(", "))
}

/// `show` / `repr` of a sparse tensor.
///
/// `repr` is an expression that rebuilds the same value:
/// `convert<Csr>(sparse_coo(shape, indices, values))`.
pub fn render(
    data: &SparseData,
    repr: bool,
) -> String {
    const LIMIT: usize = 64;
    if !sparse_layout_is_consistent(data) {
        return "sparse(<invalid storage>)".to_string();
    }
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
    let mut body: Vec<String> = stored
        .iter()
        .take(LIMIT)
        .map(|(c, v)| format!("{} = {}", coord_text(c), text(v)))
        .collect();
    if let Some(extra) = stored.len().checked_sub(LIMIT).filter(|extra| *extra != 0) {
        body.push(format!("... ({extra} more)"));
    }
    format!(
        "sparse({}, {}, {{ {} }})",
        data.format.name(),
        show_shape(&data.shape),
        body.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_overshooting_row_pointer_is_a_shape_error() {
        let compiled = crate::compile("csr_bounds.aev", "let unused = 0\n");
        let mut interp = Interp::new(&compiled.checked);
        let args = [
            int_value(2),
            int_value(1),
            int_tensor(&[0, 2, 1]),
            int_tensor(&[0]),
            dense_value_f64(vec![1], vec![1.0], Dim::one(), None),
        ];
        let result = sparse_csr(&mut interp, &args, Span::default());
        assert!(matches!(result, Err(error) if error.code == "E0802"
            && error.message.contains("beyond nnz")));
    }

    #[test]
    fn transpose_rejects_unrepresentable_compressed_output() -> R<()> {
        let span = Span::default();
        let source = wrap(
            span,
            SparseFormat::Csr,
            vec![1, usize::MAX],
            Dim::one(),
            None,
            2,
            Entries::<f64>::empty(2),
        )?;
        assert!(matches!(source, Value::Sparse(data)
            if matches!(transpose(&data, span), Err(error) if error.code == "E0802")));
        Ok(())
    }

    #[test]
    fn sparse_shape_extents_round_trip_as_exact_integers() {
        let span = Span::default();
        assert_eq!(
            index_of(&int_value(usize::MAX), "extent", span).ok(),
            Some(usize::MAX)
        );
        assert!(index_of(&Value::float(f64::MAX), "extent", span).is_err());
        assert_eq!(element_count(&[usize::MAX, 0], span).ok(), Some(0));
    }
}
