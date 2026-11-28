//! Checked instance dispatch must use the defining context across revisions.

use aevia_core::Compilation;
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile_with_source_identity;
use aevia_core::source::SourceIdentity;
use aevia_core::value::Value;
use aevia_syntax::Span;
use std::fmt::Write as _;

fn compile_instances(
    base: usize,
    identity: &SourceIdentity,
) -> Compilation {
    let mut source = "class EvidenceProbe a where\n    read_probe(x: a): Int\n".to_string();
    for index in 0..80 {
        writeln!(
            source,
            "type Head{index:02} = | Head{index:02}(Int)\ninstance EvidenceProbe for Head{index:02} where\n    read_probe(x) = {}",
            base + index,
        )
        .unwrap();
    }
    source.push_str(
        "fn first(): Int = read_probe(Head00(0))\nfn last(): Int = read_probe(Head79(0))\n",
    );
    let compilation = compile_with_source_identity("instance-index.aev", &source, identity);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    compilation
}

fn load(
    interp: &mut Interp,
    compilation: &Compilation,
) -> [Value; 2] {
    interp.prepare_module(&compilation.checked);
    let diagnostics = interp.load(compilation.module.as_ref().unwrap());
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    ["first", "last"].map(|name| interp.eval_toplevel(name).unwrap())
}

fn values(
    interp: &mut Interp,
    methods: &[Value; 2],
) -> [f64; 2] {
    methods.each_ref().map(|value| {
        interp
            .apply(value.clone(), vec![], Span::default())
            .unwrap()
            .as_f64()
            .unwrap()
    })
}

#[test]
fn indexed_methods_share_a_name_without_sharing_their_head_or_checked_context() {
    let first_identity = SourceIdentity::virtual_source("aevia-test://evidence/first");
    let second_identity = SourceIdentity::virtual_source("aevia-test://evidence/second");
    let first = compile_instances(100, &first_identity);
    let revision = compile_instances(200, &first_identity);
    let other = compile_instances(300, &second_identity);
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&first.checked).with_mode(mode);
        let original = load(&mut interp, &first);
        assert_eq!(values(&mut interp, &original), [100.0, 179.0], "{mode:?}");
        let refreshed = load(&mut interp, &revision);
        assert_eq!(values(&mut interp, &refreshed), [200.0, 279.0], "{mode:?}");
        let separate = load(&mut interp, &other);
        for _ in 0..3 {
            assert_eq!(values(&mut interp, &separate), [300.0, 379.0], "{mode:?}");
            assert_eq!(values(&mut interp, &original), [100.0, 179.0], "{mode:?}");
            assert_eq!(values(&mut interp, &refreshed), [200.0, 279.0], "{mode:?}");
        }
    }
}
