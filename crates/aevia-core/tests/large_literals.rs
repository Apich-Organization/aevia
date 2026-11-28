//! Tensor literals unify one fresh representation (and dimension) variable per
//! element, in a chain. Resolution used to stop after 128 links, so a literal
//! with more elements kept an unresolved variable and failed at run time with
//! `E0304` ("a numeric literal requires concrete runtime type arguments").

fn run(source: &str) -> String {
    aevia_core::run(source, "large.aev").expect("runs")
}

fn list(
    count: usize,
    item: impl Fn(usize) -> String,
) -> String {
    (0..count).map(item).collect::<Vec<_>>().join(", ")
}

#[test]
fn a_dense_literal_longer_than_the_old_bound() {
    let source = format!(
        "fn main(): IO Unit = println(sum([{}]))\n",
        list(1000, |i| i.to_string())
    );
    assert_eq!(run(&source).trim(), "499500");
}

#[test]
fn a_quantity_literal_longer_than_the_old_bound() {
    let source = format!(
        "fn main(): IO Unit = println(sum([{}]))\n",
        list(300, |_| "1.0 m".to_string())
    );
    assert_eq!(run(&source).trim(), "300.0 m");
}

#[test]
fn a_sparse_literal_with_many_coordinates() {
    let source = format!(
        "fn main(): IO Unit = println(nnz(sparse_coo([300, 300], [{}], [{}])))\n",
        list(300, |i| format!("({i}, {i})")),
        list(300, |_| "2.0".to_string())
    );
    assert_eq!(run(&source).trim(), "300");
}
