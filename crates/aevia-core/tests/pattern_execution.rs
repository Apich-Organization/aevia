//! Nominal, transactional pattern execution and zero-copy vector tails.
use aevia_core::Compilation;
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::ExactRep;
use aevia_core::value::Scope;
use aevia_core::value::Storage;
use aevia_core::value::TensorData;
use aevia_core::value::Value;
use aevia_core::value::make_variant;
use aevia_syntax::Span;
use num_bigint::BigInt;
use num_rational::BigRational;
use std::rc::Rc;
fn checked(source: &str) -> Compilation {
    let c = compile("pattern_execution.aev", source);
    assert!(
        !c.diagnostics.has_errors(),
        "{source}\n{:?}",
        c.diagnostics.items
    );
    c
}
fn installed(
    c: &Compilation,
    mode: Mode,
) -> Interp {
    let mut i = Interp::new(&c.checked).with_mode(mode);
    let diagnostics = i.load(c.module.as_ref().unwrap());
    assert!(
        !diagnostics.has_errors(),
        "{mode:?}: {:?}",
        diagnostics.items
    );
    i
}
fn apply(
    i: &mut Interp,
    name: &str,
    value: Value,
) -> Result<Value, aevia_core::EvalError> {
    let function = i.eval_toplevel(name).unwrap();
    i.apply(function, vec![value], Span::default())
}
#[test]
fn fixed_vector_patterns_require_exact_length_in_match_let_and_parameters() {
    let c = checked(
        r#"fn choose(xs: Tensor<1, Int>): Int = match xs with
| [] -> 0
| [one] -> one
| [one, two] -> one + two
| _ -> 99
fn single([one]: Tensor<1, Int>): Int = one
let empty = choose([])
let one = choose([7])
let two = choose([7, 8])
let three = choose([7, 8, 9])
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut i = installed(&c, mode);
        for (name, expected) in [("empty", 0.), ("one", 7.), ("two", 15.), ("three", 99.)] {
            assert_eq!(
                i.eval_toplevel(name).unwrap().as_f64(),
                Some(expected),
                "{mode:?}: {name}"
            );
        }
        let xs = Value::Tensor(Rc::new(TensorData::new(
            vec![2],
            Storage::TypedExact(
                vec![
                    BigRational::from_integer(1.into()),
                    BigRational::from_integer(2.into()),
                ],
                ExactRep::Int,
            ),
            aevia_dim::Dim::one(),
            None,
        )));
        assert_eq!(apply(&mut i, "single", xs).unwrap_err().code, "E0308");
    }
}
#[test]
fn a_failed_pattern_commits_no_prefix_bindings() {
    let c = checked("let [first, 0] = [1, 2]\n");
    let aevia_syntax::DeclKind::Let { pattern, .. } = &c.module.as_ref().unwrap().decls[0].kind
    else {
        panic!("let")
    };
    let mut i = Interp::new(&c.checked);
    let scope = Scope::root();
    scope
        .vars
        .borrow_mut()
        .insert("first".into(), Value::int(99));
    let value = Value::Tensor(Rc::new(TensorData::new(
        vec![2],
        Storage::TypedExact(
            vec![
                BigRational::from_integer(1.into()),
                BigRational::from_integer(2.into()),
            ],
            ExactRep::Int,
        ),
        aevia_dim::Dim::one(),
        None,
    )));
    assert_eq!(
        i.bind_pattern_entry(pattern, &value, &scope)
            .unwrap_err()
            .code,
        "E0308"
    );
    assert_eq!(scope.lookup("first").unwrap().as_f64(), Some(99.));
    assert_eq!(scope.vars.borrow().len(), 1);
}
#[test]
fn constructor_patterns_require_the_checked_type_index_name_and_arity() {
    let c = checked(
        "type Actual = | Same(Int) | Other(Int)\nfn read(x: Actual): Int = match x with\n| Same(n) -> n\n| Other(n) -> n + 1\n",
    );
    let id = match c.checked.exports["Actual"] {
        | aevia_types::Export::Type { adt } => adt,
        | _ => panic!("type"),
    };
    for mode in [Mode::Ast, Mode::Ir] {
        let mut i = installed(&c, mode);
        for value in [
            make_variant(aevia_types::adt::OPTION_ID, 1, "Same", vec![Value::int(7)]),
            make_variant(id, 1, "Same", vec![Value::int(7)]),
            make_variant(id, 0, "Same", Vec::new()),
        ] {
            assert_eq!(
                apply(&mut i, "read", value).unwrap_err().code,
                "E0309",
                "{mode:?}"
            );
        }
        assert_eq!(
            apply(
                &mut i,
                "read",
                make_variant(id, 0, "Same", vec![Value::int(7)])
            )
            .unwrap()
            .as_f64(),
            Some(7.)
        );
    }
}
fn tail_storage(
    storage: Storage,
    expected: Vec<Value>,
) {
    let source = Rc::new(TensorData::new(
        vec![expected.len()],
        storage,
        aevia_dim::Dim::one(),
        None,
    ));
    let mut tail = source.clone();
    for offset in 1..=expected.len() {
        tail = tail.tail_view(1).unwrap();
        let Storage::SharedSlice(window) = &tail.storage else {
            panic!("shared suffix")
        };
        assert!(Rc::ptr_eq(window.source(), &source));
        assert_eq!(window.offset(), offset);
        assert_eq!(
            tail.elements()
                .iter()
                .map(aevia_core::value::repr)
                .collect::<Vec<_>>(),
            expected[offset..]
                .iter()
                .map(aevia_core::value::repr)
                .collect::<Vec<_>>()
        );
    }
}
#[test]
fn suffixes_share_all_storage_classes_without_building_a_view_chain() {
    tail_storage(
        Storage::Dense(Rc::new(vec![1., 2., 3.])),
        vec![Value::float(1.), Value::float(2.), Value::float(3.)],
    );
    let exact = vec![
        BigRational::new(1.into(), 3.into()),
        BigRational::new(2.into(), 3.into()),
    ];
    tail_storage(
        Storage::Exact(exact.clone()),
        exact
            .iter()
            .cloned()
            .map(|n| Value::Num(aevia_core::value::NumVal::exact(n, aevia_dim::Dim::one())))
            .collect(),
    );
    let ints = vec![BigInt::from(1), BigInt::from(1) << 100usize];
    tail_storage(
        Storage::TypedExact(
            ints.iter()
                .cloned()
                .map(BigRational::from_integer)
                .collect(),
            ExactRep::Int,
        ),
        ints.into_iter()
            .map(|value| Value::Num(aevia_core::value::NumVal::int(value)))
            .collect(),
    );
    tail_storage(
        Storage::Bool(vec![true, false]),
        vec![Value::Bool(true), Value::Bool(false)],
    );
    tail_storage(
        Storage::Boxed(vec![Value::string("a"), Value::string("b")]),
        vec![Value::string("a"), Value::string("b")],
    );
}
#[test]
fn pattern_tails_preserve_empty_nominal_schema_and_float_or_integer_representation() {
    let c = checked(
        r#"type Token = | Token(Int)
fn tail(xs: Tensor<1, Token>) = match xs with
| [] -> xs
| [first, ..rest] -> rest
let empty = tail([Token(1)])
let roundtrip = from_value<Tensor<1, Token>>(to_value(empty))
fn floats(xs: Tensor<1, Float>) = match xs with
| [] -> xs
| [first, ..rest] -> rest
let float_tail = floats([1.0, 2.0])
fn ints(xs: Tensor<1, Int>) = match xs with
| [] -> xs
| [first, ..rest] -> rest
let int_tail = ints([1, 9007199254740993])
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut i = installed(&c, mode);
        let Value::Variant(result) = i.eval_toplevel("roundtrip").unwrap() else {
            panic!("Result")
        };
        assert_eq!(result.name, "Ok", "{mode:?}: {result:?}");
        let Value::Tensor(tail) = i.eval_toplevel("int_tail").unwrap() else {
            panic!("tensor")
        };
        assert_eq!(
            aevia_core::value::repr(&tail.get_flat(0).unwrap()),
            "9007199254740993"
        );
        let Value::Tensor(tail) = i.eval_toplevel("float_tail").unwrap() else {
            panic!("tensor")
        };
        assert_eq!(tail.dense_flat().unwrap().as_ref(), &[2.]);
    }
}
#[test]
fn recursive_consumption_uses_flat_views_for_linear_tail_work() {
    let c = checked(
        "fn count(xs: Tensor<1, Int>, n: Int): Int = match xs with\n| [] -> n\n| [head, ..tail] -> count(tail, n + 1)\nfn start(xs: Tensor<1, Int>): Int = count(xs, 0)\n",
    );
    for mode in [Mode::Ast, Mode::Ir] {
        // AST deliberately retains its source recursion guard; use small input
        // there, while IR exercises the full constant-stack tail path.
        let size = if mode == Mode::Ast {
            64
        } else {
            20_000
        };
        let mut i = installed(&c, mode);
        let xs = Value::Tensor(Rc::new(TensorData::new(
            vec![size],
            Storage::TypedExact(
                (0..size)
                    .map(|n| BigRational::from_integer(n.into()))
                    .collect(),
                ExactRep::Int,
            ),
            aevia_dim::Dim::one(),
            None,
        )));
        assert_eq!(
            apply(&mut i, "start", xs).unwrap().as_f64(),
            Some(size as f64),
            "{mode:?}"
        );
    }
}

#[test]
fn dimension_kinded_constructor_fields_keep_independent_instantiations() {
    let c = checked(
        r#"type Quantity<d: Dim> = | Quantity(Float[d])
fn length(q: Quantity<Length>): Float[Length] = match q with
| Quantity(value) -> value
fn time(q: Quantity<Time>): Float[Time] = match q with
| Quantity(value) -> value
let distance = length(Quantity(2.0 m))
let duration = time(Quantity(3.0 s))
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut i = installed(&c, mode);
        let distance = i.eval_toplevel("distance").unwrap();
        let duration = i.eval_toplevel("duration").unwrap();
        assert_eq!(distance.as_f64(), Some(2.));
        assert_eq!(duration.as_f64(), Some(3.));
        assert_eq!(
            aevia_core::value::value_dim(&distance),
            aevia_dim::Dim::base("Length")
        );
        assert_eq!(
            aevia_core::value::value_dim(&duration),
            aevia_dim::Dim::base("Time")
        );
    }
}
#[test]
fn constructor_schema_changes_are_rejected_while_unchanged_rechecks_match() {
    let first = checked(
        "type Item = | Build(Int)\nfn read(x: Item): Int = match x with\n| Build(n) -> n\n",
    );
    let same = checked(
        "type Item = | Build(Int)\nfn read(x: Item): Int = match x with\n| Build(n) -> n\n",
    );
    let changed = checked(
        "type Item = | Build(Bool)\nfn read(x: Item): Int = match x with\n| Build(n) -> if n then 1 else 0\n",
    );
    let aevia_types::Export::Type { adt: id } = first.checked.exports["Item"] else {
        panic!("type")
    };
    for mode in [Mode::Ast, Mode::Ir] {
        let old = make_variant(id, 0, "Build", vec![Value::int(7)]);
        assert_eq!(
            apply(&mut installed(&same, mode), "read", old.clone())
                .unwrap()
                .as_f64(),
            Some(7.)
        );
        assert_eq!(
            apply(&mut installed(&changed, mode), "read", old)
                .unwrap_err()
                .code,
            "E0309"
        );
    }
}
#[test]
fn private_constructor_names_cannot_be_recovered_from_an_inferred_imported_type() {
    let directory =
        std::env::temp_dir().join(format!("aevia-pattern-visibility-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let library = directory.join("Library.aev");
    let entry = directory.join("main.aev");
    std::fs::write(
        &library,
        "module Library\nprivate type Secret = | Sealed(Int)\nfn make() = Sealed(7)\n",
    )
    .unwrap();
    std::fs::write(
        &entry,
        "import Library\nfn read(): Int = match Library.make() with\n| Sealed(n) -> n\n",
    )
    .unwrap();
    let private = aevia_core::driver::load(&entry);
    assert!(
        private.diagnostics.items.iter().any(|d| d.code == "E0307"),
        "{:?}",
        private.diagnostics.items
    );
    std::fs::write(
        &library,
        "module Library\ntype Secret = | Sealed(Int)\nfn make() = Sealed(7)\n",
    )
    .unwrap();
    let public = aevia_core::driver::load(&entry);
    assert!(
        !public.diagnostics.has_errors(),
        "{:?}",
        public.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut i = Interp::new(&public.entry().unwrap().checked).with_mode(mode);
        let diagnostics = aevia_core::driver::run_modules(&mut i, &public);
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
        let function = i.eval_toplevel("read").unwrap();
        assert_eq!(
            i.apply(function, vec![], Span::default()).unwrap().as_f64(),
            Some(7.)
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn logical_suffixes_compose_with_reversed_strided_views() {
    let root = Rc::new(TensorData::new(
        vec![5],
        Storage::Dense(Rc::new(vec![1., 2., 3., 4., 5.])),
        aevia_dim::Dim::base("Length"),
        None,
    ));
    let reversed = Rc::new(root.reversed_view(0).unwrap());
    let tail = reversed.tail_view(2).unwrap();
    assert_eq!(tail.dense_flat().unwrap().as_ref(), &[3., 2., 1.]);
    let reverse_again = tail.reversed_view(0).unwrap();
    assert_eq!(reverse_again.dense_flat().unwrap().as_ref(), &[1., 2., 3.]);
    assert_eq!(
        aevia_core::value::value_dim(&tail.get_flat(0).unwrap()),
        root.dim
    );
}

#[test]
fn logical_suffixes_reshape_transpose_and_slice_with_correct_layouts() {
    let root = Rc::new(TensorData::new(
        vec![5],
        Storage::Dense(Rc::new(vec![0., 1., 2., 3., 4.])),
        aevia_dim::Dim::one(),
        None,
    ));
    for source in [root.clone(), Rc::new(root.reversed_view(0).unwrap())] {
        let tail = source.tail_view(1).unwrap();
        let values = tail.dense_flat().unwrap().into_owned();
        let storage = tail.reshape_storage(&[2, 2]).unwrap();
        let matrix = TensorData::new(vec![2, 2], storage, aevia_dim::Dim::one(), None);
        if Rc::ptr_eq(&source, &root) {
            assert!(Rc::ptr_eq(matrix.buffer().unwrap(), root.buffer().unwrap()));
        }
        let transpose = matrix.transposed_view().unwrap();
        assert_eq!(
            transpose.dense_flat().unwrap().as_ref(),
            &[values[0], values[2], values[1], values[3]]
        );
        let slice = transpose.slice_view(&[(0, 1), (0, 2)]).unwrap();
        assert_eq!(
            slice.dense_flat().unwrap().as_ref(),
            &[values[0], values[2]]
        );
    }
    // Public construction can reuse the logical storage directly under a new
    // shape too. Layout helpers must not return rank-one strides for rank two.
    let tail = root.tail_view(1).unwrap();
    let matrix = TensorData::new(
        vec![2, 2],
        tail.storage.clone(),
        aevia_dim::Dim::one(),
        None,
    );
    assert_eq!(
        matrix
            .transposed_view()
            .unwrap()
            .dense_flat()
            .unwrap()
            .as_ref(),
        &[1., 3., 2., 4.]
    );
}

#[test]
fn reshaping_empty_suffixes_preserves_every_representation_and_nominal_schema() {
    for storage in [
        Storage::Dense(Rc::new(vec![1.])),
        Storage::Exact(vec![BigRational::from_integer(1.into())]),
        Storage::TypedExact(vec![BigRational::from_integer(1.into())], ExactRep::Int),
        Storage::Bool(vec![true]),
        Storage::Boxed(vec![Value::string("a")]),
    ] {
        let source = Rc::new(TensorData::new(
            vec![1],
            storage,
            aevia_dim::Dim::one(),
            None,
        ));
        let tail = source.tail_view(1).unwrap();
        let reshaped = TensorData::new(
            vec![0, 2],
            tail.reshape_storage(&[0, 2]).unwrap(),
            aevia_dim::Dim::one(),
            None,
        );
        assert!(reshaped.storage.is_empty());
        assert_eq!(
            std::mem::discriminant(&reshaped.storage),
            std::mem::discriminant(&source.storage)
        );
    }
    let c = checked(
        r#"type Token = | Token(Int)
fn tail(xs: Tensor<1, Token>) = match xs with
| [] -> xs
| [first, ..rest] -> rest
let empty = tail([Token(1)])
let reshaped = reshape<[0, 2]>(empty)
let roundtrip = match reshaped with
| Ok(values) -> from_value<Tensor<2, Token>>(to_value(values))
| Err(_) -> from_value<Tensor<2, Token>>(to_value(empty))
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut i = installed(&c, mode);
        let Value::Variant(result) = i.eval_toplevel("roundtrip").unwrap() else {
            panic!("Result")
        };
        assert_eq!(result.name, "Ok", "{mode:?}: {result:?}");
    }
}

#[test]
fn empty_suffix_arithmetic_preserves_integer_and_real_element_representations() {
    let c = checked(
        r#"fn integers(xs: Tensor<1, Int>) = match xs with
| [] -> xs
| [head, ..tail] -> tail
fn reals(xs: Tensor<1, Real>) = match xs with
| [] -> xs
| [head, ..tail] -> tail
let empty_int = integers([1])
let empty_real = reals([1/3])
let sum = from_value<Tensor<1, Int>>(to_value(empty_int + empty_int))
let product = from_value<Tensor<1, Real>>(to_value(empty_real * empty_real))
"#,
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut i = installed(&c, mode);
        for name in ["sum", "product"] {
            let Value::Variant(result) = i.eval_toplevel(name).unwrap() else {
                panic!("Result")
            };
            assert_eq!(result.name, "Ok", "{mode:?}: {name}: {result:?}");
        }
    }
}

#[test]
fn npy_encoding_checks_only_logical_float_suffix_elements() {
    let root = Rc::new(TensorData::new(
        vec![3],
        Storage::Boxed(vec![
            Value::string("not in the suffix"),
            Value::float(1.5),
            Value::float(2.5),
        ]),
        aevia_dim::Dim::one(),
        None,
    ));
    let tail = root.tail_view(1).unwrap();
    let bytes = aevia_core::builtins::npy_encode(&tail).unwrap();
    let decoded = aevia_core::builtins::npy_decode(&bytes).unwrap();
    assert_eq!(decoded.shape, vec![2]);
    assert_eq!(decoded.dense_flat().unwrap().as_ref(), &[1.5, 2.5]);
    assert!(aevia_core::builtins::npy_encode(&root).is_err());
    // Empty boxed data has no Float proof merely because a preceding element
    // happened to be Float; exact data is never silently converted either.
    assert!(aevia_core::builtins::npy_encode(&root.tail_view(3).unwrap()).is_err());
    let exact = Rc::new(TensorData::new(
        vec![2],
        Storage::TypedExact(
            vec![
                BigRational::from_integer(1.into()),
                BigRational::from_integer(2.into()),
            ],
            ExactRep::Int,
        ),
        aevia_dim::Dim::one(),
        None,
    ));
    assert!(aevia_core::builtins::npy_encode(&exact.tail_view(1).unwrap()).is_err());
}
