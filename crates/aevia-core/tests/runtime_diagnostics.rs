//! All runtime reporting paths retain bounded, original source callsites.
use aevia_core::EvalError;
use aevia_core::eval::Frame;
use aevia_syntax::Span;

#[test]
fn runtime_diagnostic_labels_are_recent_distinct_bounded_and_exclude_the_primary() {
    let primary = Span::new(0, 2, 8);
    let sites: Vec<_> = (0..8)
        .map(|index| {
            Span::new(
                1 + index % 2,
                10 + index as usize * 10,
                15 + index as usize * 10,
            )
        })
        .collect();
    let mut frames: Vec<_> = sites
        .iter()
        .enumerate()
        .map(|(index, span)| {
            Frame {
                function: format!("frame{index}").into(),
                span: *span,
            }
        })
        .collect();
    frames.push(Frame {
        function: "recent_duplicate".into(),
        span: sites[7],
    });
    frames.push(Frame {
        function: "primary_location".into(),
        span: primary,
    });
    frames.push(Frame {
        function: "host_without_source".into(),
        span: Span::default(),
    });
    let mut error = EvalError::new("E0801", "division by zero")
        .at(primary)
        .with_stack(frames);
    error.notes.push("original note".into());
    let diagnostic = error.diagnostic();
    assert_eq!(diagnostic.span, primary);
    assert_eq!(
        diagnostic
            .labels
            .iter()
            .map(|label| label.span)
            .collect::<Vec<_>>(),
        vec![sites[7], sites[6], sites[5], sites[4], sites[3]]
    );
    assert!(diagnostic.labels[0].message.contains("recent_duplicate"));
    assert!(diagnostic.notes.iter().any(|note| note == "original note"));
    assert!(diagnostic.notes.iter().any(|note| note.contains("frame0")));
}

#[test]
fn recursive_frames_keep_a_compact_trace_and_one_callsite_label() {
    let callsite = Span::new(1, 12, 19);
    let frames = vec![
        Frame {
            function: "recursive".into(),
            span: callsite
        };
        100
    ];
    let error = EvalError::new("E0805", "execution budget exhausted")
        .at(Span::new(0, 1, 4))
        .with_stack(frames);
    let diagnostic = error.diagnostic();
    assert_eq!(diagnostic.labels.len(), 1);
    assert_eq!(diagnostic.labels[0].span, callsite);
    assert!(diagnostic.notes.len() < 10);
    assert!(
        diagnostic
            .notes
            .iter()
            .any(|note| note.contains("more recursive frames"))
    );
}
