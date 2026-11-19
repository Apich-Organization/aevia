//! Public plot constructors agree on geometry across interpreter modes.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::builtins::call_simple;
use aevia_core::compile;
use aevia_core::driver;
use aevia_core::value::IoAction;
use aevia_core::value::NumVal;
use aevia_core::value::Storage;
use aevia_core::value::TensorData;
use aevia_core::value::Value;
use aevia_dim::Dim;
use std::rc::Rc;

fn plot_svg(spec: Value) -> String {
    let Value::Io(action) = call_simple("save", vec![Value::string("result.SVG"), spec]).unwrap()
    else {
        panic!("expected IO");
    };
    let IoAction::Save { html, .. } = &*action else {
        panic!("expected save")
    };
    html.clone()
}

#[test]
fn public_constructors_render_their_geometry_in_both_engines() {
    let source = r##"
import Plot as Plots
import FFT { Log }
let xs: Tensor<1, Float> = [1.0, 2.0, 4.0]
let ys: Tensor<1, Float> = [3.0, 5.0, 6.0]
let errors: Tensor<1, Float> = [0.5, 0.75, 1.0]
let opts = { Plots.default_plot_opts with label = Some("A & <B>"), color = Some("#2255aa"), xscale = Log, xlim = Some((1.0, 8.0)) }
let ln = Plots.combine([Plots.line_with(xs, ys, opts)])
let sc = Plots.combine([Plots.scatter_with(xs, ys, opts)])
let er = Plots.combine([Plots.errorbar_with(xs, ys, errors, opts)])
let hi = Plots.combine([Plots.hist_with(xs, 2, Plots.default_plot_opts)])
let hm = Plots.combine([Plots.heatmap_with([[1.0, 2.0], [3.0, 4.0]], Plots.default_plot_opts)])
let exported_animation = Plots.plot_save_gif("result.gif", [ln, sc, er, hi, hm], 12)
let prelude_animation = plot_save_gif("default.gif", [ln])
let prelude_override = plot_save_gif("manual.gif", [ln], 23)
"##;
    let path = std::env::temp_dir().join(format!("aevia-plot-shapes-{}.aev", std::process::id()));
    let c = driver::load_with_source(&path, source);
    assert!(
        !c.diagnostics.has_errors(),
        "{}",
        c.diagnostics.render(&c.sources)
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&c.entry().unwrap().checked).with_mode(mode);
        let loaded = driver::run_modules(&mut interp, &c);
        assert!(!loaded.has_errors(), "{:?}", loaded.items);
        let (ln, sc, er, hi, hm) = (
            interp.eval_toplevel("ln").unwrap(),
            interp.eval_toplevel("sc").unwrap(),
            interp.eval_toplevel("er").unwrap(),
            interp.eval_toplevel("hi").unwrap(),
            interp.eval_toplevel("hm").unwrap(),
        );
        let svg = plot_svg(ln);
        assert!(svg.contains("<title>A &amp; &lt;B&gt;</title>"));
        assert!(svg.contains("x (log10)"));
        assert_eq!(svg.matches("<line ").count(), 2);
        assert!(svg.contains("stroke=\"#2255aa\""));
        assert_eq!(plot_svg(sc).matches("<circle ").count(), 3);
        let errors = plot_svg(er);
        assert_eq!(errors.matches("<line ").count(), 9);
        assert_eq!(errors.matches("<circle ").count(), 3);
        assert_eq!(plot_svg(hi).matches("<rect ").count(), 3);
        assert_eq!(plot_svg(hm).matches("<rect ").count(), 5);
        let Value::Io(action) = interp.eval_toplevel("exported_animation").unwrap() else {
            panic!("expected GIF IO")
        };
        let IoAction::WriteGif { animation, .. } = &*action else {
            panic!("expected animation, got {action:?}")
        };
        assert_eq!(animation.frames.len(), 5);
        assert!(animation.frames.iter().all(|f| {
            f.delay_cs == 12
                && f.rgba.len() == 800 * 400 * 4
                && f.rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|&color| color != [255, 255, 255, 255] && color != [204, 204, 204, 255])
        }));
        for (name, delay) in [("prelude_animation", 10), ("prelude_override", 23)] {
            let Value::Io(action) = interp.eval_toplevel(name).unwrap() else {
                panic!("expected GIF IO")
            };
            let IoAction::WriteGif { animation, .. } = &*action else {
                panic!("expected animation, got {action:?}")
            };
            assert_eq!(animation.frames.len(), 1);
            assert_eq!(animation.frames[0].delay_cs, delay);
        }
        let bytes = rssn::io::encode_gif(animation).unwrap();
        let decoded = rssn::io::decode_gif(&bytes).unwrap();
        assert_eq!(
            (decoded.width, decoded.height, decoded.frames.len()),
            (800, 400, 5)
        );
    }
}

#[test]
fn wrong_option_shape_reports_a_type_error() {
    let c = compile(
        "bad-plot.aev",
        "let bad = line_with([1.0], [2.0], { color = 2 })\n",
    );
    assert!(c.diagnostics.has_errors());
}

#[test]
fn histogram_rejects_invalid_and_unbounded_bin_counts() {
    let coordinates = Value::Tensor(Rc::new(TensorData::new(
        vec![2],
        Storage::Dense(Rc::new(vec![1.0, 2.0])),
        Dim::one(),
        None,
    )));
    let too_large = aevia_core::int::Int::from(i128::MAX);
    for count in [
        Value::int(0),
        Value::int(-1),
        Value::int(i64::MAX),
        Value::Num(NumVal::int(too_large)),
    ] {
        let error = call_simple("hist", vec![coordinates.clone(), count]).unwrap_err();
        assert_eq!(error.code, "E0802");
    }
}
