//! Checked HTML/SVG and raster rendering for plot specifications.
#![cfg_attr(
    not(test),
    deny(
        missing_docs,
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unwrap_used
    )
)]

use crate::eval::EvalError;
use crate::value::NumKind;
use crate::value::RecordValue;
use crate::value::TensorData;
use crate::value::Value;
use crate::value::si_unit_text;
use aevia_dim::Dim;
use aevia_dim::UnitDef;
use aevia_dim::UnitTable;
use aevia_syntax::Span;
use aevia_types::adt;
use num_traits::ToPrimitive;
use rssn::io::GifAnimation;
use rssn::io::GifFrame;
use std::fmt::Write as _;
use std::rc::Rc;

const WIDTH: u16 = 800;
const HEIGHT: u16 = 400;
const CHANNELS: usize = 4;
const PALE: [u8; CHANNELS] = [204, 204, 204, 255];
const RED: [u8; CHANNELS] = [192, 57, 43, 255];

type Point = (f64, f64);
type Pixel = (u16, u16);

fn vector_len(
    tensor: &TensorData,
    field: &str,
) -> Result<usize, String> {
    match tensor.shape.as_slice() {
        | &[count] if count == tensor.storage.len() => {
            super::checked_element_count(&[count], Span::default())
                .map_err(|_| format!("{field} exceeds the allocation limit"))
        },
        | _ => Err(format!("{field} must be a vector with matching storage")),
    }
}

fn coordinate(
    tensor: &TensorData,
    dense: Option<&[f64]>,
    index: usize,
    axis: &str,
) -> Result<f64, String> {
    dense
        .and_then(|data| data.get(index).copied())
        .or_else(|| tensor.get_flat(index)?.as_f64())
        .ok_or_else(|| format!("plot {axis} coordinate {index} must be a concrete real number"))
}

#[derive(Clone, Copy)]
enum SeriesKind {
    Line,
    Scatter,
    ErrorBar,
    Histogram,
    Heatmap,
}

impl SeriesKind {
    fn parse(value: Option<&Value>) -> Result<Self, String> {
        match value {
            | None => Ok(Self::Line),
            | Some(Value::Str(name)) => {
                match name.as_ref() {
                    | "line" | "line_with" => Ok(Self::Line),
                    | "scatter" | "scatter_with" => Ok(Self::Scatter),
                    | "errorbar" | "errorbar_with" => Ok(Self::ErrorBar),
                    | "hist" | "hist_with" => Ok(Self::Histogram),
                    | "heatmap" | "heatmap_with" => Ok(Self::Heatmap),
                    | _ => Err("unknown plot series kind".into()),
                }
            },
            | Some(_) => Err("plot series kind must be text".into()),
        }
    }
}

fn tensor_field<'a>(
    record: &'a RecordValue,
    name: &str,
) -> Result<&'a TensorData, String> {
    match record.fields.get(name) {
        | Some(Value::Tensor(tensor)) => Ok(tensor),
        | _ => Err(format!("plot series needs the {name} tensor")),
    }
}

fn option_field<'a>(
    opts: &'a RecordValue,
    key: &str,
) -> Result<Option<&'a Value>, String> {
    match opts.fields.get(key) {
        | None => Ok(None),
        | Some(Value::Variant(opt)) => {
            match (opt.name.as_str(), opt.fields.as_slice()) {
                | ("None", []) => Ok(None),
                | ("Some", [value]) => Ok(Some(value)),
                | _ => Err(format!("plot option {key} must be an Option")),
            }
        },
        | _ => Err(format!("plot option {key} must be an Option")),
    }
}

fn text_option<'a>(
    opts: &'a RecordValue,
    key: &str,
) -> Result<Option<&'a str>, String> {
    match option_field(opts, key)? {
        | None => Ok(None),
        | Some(Value::Str(text)) => Ok(Some(text)),
        | Some(_) => Err(format!("plot option {key} must contain text")),
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "validates CSS input once before choosing renderer-specific output"
)]
fn color(text: &str) -> Result<[u8; CHANNELS], String> {
    let channels = match text {
        | "red" => RED,
        | "blue" => [39, 92, 178, 255],
        | "green" => [20, 123, 73, 255],
        | "orange" => [217, 110, 28, 255],
        | "purple" => [126, 74, 154, 255],
        | "black" => [0, 0, 0, 255],
        | text => {
            let Some(rgb) = text
                .strip_prefix('#')
                .filter(|rgb| rgb.len() == 6 && rgb.bytes().all(|byte| byte.is_ascii_hexdigit()))
            else {
                return Err(
                    "plot color must be a six-digit hex RGB value or a supported name".into(),
                );
            };
            let packed = u32::from_str_radix(rgb, 16)
                .map_err(|_| "plot color has an invalid hex RGB value")?;
            let [_, red, green, blue] = packed.to_be_bytes();
            [red, green, blue, 255]
        },
    };
    Ok(channels)
}

#[derive(Clone, Copy, Default)]
struct AxisOptions<'a> {
    unit: Option<&'a str>,
    log: Option<bool>,
    limits: Option<[f64; 2]>,
}

impl<'a> AxisOptions<'a> {
    fn read(
        opts: &'a RecordValue,
        unit: &str,
        scale: &str,
        lim: &str,
    ) -> Result<Self, String> {
        let log = match opts.fields.get(scale) {
            | None => None,
            | Some(Value::Variant(value)) if value.fields.is_empty() => {
                match value.name.as_str() {
                    | "Linear" => Some(false),
                    | "Log" => Some(true),
                    | _ => return Err(format!("plot option {scale} must be Linear or Log")),
                }
            },
            | _ => return Err(format!("plot option {scale} must be Linear or Log")),
        };
        let limits = match option_field(opts, lim)? {
            | None => None,
            | Some(Value::Record(pair)) => {
                let get = |key| {
                    pair.fields
                        .get(key)
                        .and_then(Value::as_f64)
                        .filter(|x| x.is_finite())
                };
                let (Some(low), Some(high)) = (get("0"), get("1")) else {
                    return Err(format!(
                        "plot option {lim} must contain two finite coordinates"
                    ));
                };
                if low >= high {
                    return Err(format!("plot option {lim} must be increasing"));
                }
                Some([low, high])
            },
            | _ => return Err(format!("plot option {lim} must contain a coordinate pair")),
        };
        Ok(Self {
            unit: text_option(opts, unit)?,
            log,
            limits,
        })
    }
}

struct Style<'a> {
    label: &'a str,
    ink: [u8; CHANNELS],
    x: AxisOptions<'a>,
    y: AxisOptions<'a>,
}

impl<'a> Style<'a> {
    fn read(record: &'a RecordValue) -> Result<Self, String> {
        let label = match record.fields.get("label") {
            | None => None,
            | Some(Value::Str(text)) => Some(text.as_ref()),
            | _ => return Err("plot series label must be text".into()),
        };
        let Some(options) = record.fields.get("options") else {
            return Ok(Self {
                label: label.unwrap_or(""),
                ink: RED,
                x: AxisOptions::default(),
                y: AxisOptions::default(),
            });
        };
        let Value::Record(options) = options else {
            return Err("plot options must be a PlotOpts record".into());
        };
        let option_label = text_option(options, "label")?;
        let ink = text_option(options, "color")?.map_or(Ok(RED), color)?;
        Ok(Self {
            label: label.or(option_label).unwrap_or(""),
            ink,
            x: AxisOptions::read(options, "xunit", "xscale", "xlim")?,
            y: AxisOptions::read(options, "yunit", "yscale", "ylim")?,
        })
    }
}

#[derive(Clone, Copy)]
enum Glyph {
    Segment(Point, Point),
    Whisker(Point, Point),
    Dot(Point),
    Box(Point, Point),
}

#[derive(Default)]
struct FrameAxis {
    dim: Option<Dim>,
    unit: Option<UnitDef>,
    log: Option<bool>,
    limits: Option<[f64; 2]>,
    bounds: Option<Axis>,
}

impl FrameAxis {
    #[allow(
        clippy::float_cmp,
        reason = "series must explicitly agree on axis limits"
    )]
    fn merge(
        &mut self,
        dim: &Dim,
        opts: AxisOptions<'_>,
        units: &UnitTable,
    ) -> Result<(), String> {
        if self.dim.as_ref().is_some_and(|prior| prior != dim) {
            return Err("series sharing an axis must have the same coordinate dimension".into());
        }
        self.dim.get_or_insert_with(|| dim.clone());
        if let Some(name) = opts.unit {
            let unit = units
                .lookup(name)
                .ok_or_else(|| format!("unknown plot unit {name}"))?;
            if &unit.dim != dim || !unit.scale.to_f64().is_finite() || unit.scale.to_f64() <= 0.0 {
                return Err(format!(
                    "plot unit {name} is incompatible with the coordinate dimension"
                ));
            }
            if self.unit.as_ref().is_some_and(|old| old.name != unit.name) {
                return Err("series sharing an axis must agree on the display unit".into());
            }
            self.unit = Some(unit);
        }
        if let Some(log) = opts.log {
            if self.log.is_some_and(|prior| prior != log) {
                return Err("series sharing an axis must agree on the scale".into());
            }
            self.log = Some(log);
        }
        if let Some(limits) = opts.limits {
            if self.limits.is_some_and(|prior| prior != limits) {
                return Err("series sharing an axis must agree on the limits".into());
            }
            self.limits = Some(limits);
        }
        Ok(())
    }

    fn scale(
        &self,
        value: f64,
    ) -> Option<f64> {
        let value = if self.log == Some(true) {
            value.log10()
        } else {
            value
        };
        value.is_finite().then_some(value)
    }

    fn transform(
        &self,
        value: f64,
    ) -> Option<f64> {
        let value = self.unit.as_ref().map_or(value, |unit| {
            let offset = unit.offset.as_ref().map_or(0.0, aevia_dim::Scale::to_f64);
            (value - offset) / unit.scale.to_f64()
        });
        self.scale(value)
    }

    fn extend(
        &mut self,
        value: f64,
    ) {
        if let Some(value) = self.transform(value) {
            self.bounds
                .get_or_insert_with(|| Axis::at(value))
                .include(value);
        }
    }

    fn domain(&self) -> Result<Axis, String> {
        if let Some([lo, hi]) = self.limits {
            let (Some(min), Some(max)) = (self.scale(lo), self.scale(hi)) else {
                return Err("logarithmic plot limits must be positive".into());
            };
            return Ok(Axis { min, max });
        }
        Ok(self.bounds.unwrap_or(Axis { min: 0.0, max: 1.0 }))
    }

    fn label(
        &self,
        name: &str,
    ) -> String {
        let unit = self.unit.as_ref().map_or_else(
            || self.dim.as_ref().map_or_else(String::new, si_unit_text),
            |unit| unit.display.clone(),
        );
        let mut label = if unit.is_empty() {
            name.to_string()
        } else {
            format!("{name} / {unit}")
        };
        if self.log == Some(true) {
            label.push_str(" (log10)");
        }
        label
    }
}

#[derive(Default)]
struct PlotAxes {
    x: FrameAxis,
    y: FrameAxis,
}

fn visit_series(
    spec: &Value,
    mut render: impl FnMut(&RecordValue) -> Result<(), String>,
) -> Result<(), String> {
    if matches!(spec, Value::Unit) {
        return Ok(());
    }
    let Value::Record(spec) = spec else {
        return Err("expected a plot specification".into());
    };
    let Some(Value::Tensor(series)) = spec.fields.get("series") else {
        return Err("a plot specification must contain a series vector".into());
    };
    for index in 0..vector_len(series, "plot series")? {
        let Some(Value::Record(item)) = series.get_flat(index) else {
            return Err(format!("plot series {index} must be a record"));
        };
        render(&item).map_err(|error| format!("plot series {index}: {error}"))?;
    }
    Ok(())
}

impl PlotAxes {
    fn read(
        spec: &Value,
        units: &UnitTable,
    ) -> Result<Self, String> {
        let mut axes = Self::default();
        visit_series(spec, |series| {
            let Style { x, y, .. } = Style::read(series)?;
            let kind = SeriesKind::parse(series.fields.get("kind"))?;
            let dimensionless = Dim::one();
            let (dim_x, dim_y) = if matches!(kind, SeriesKind::Heatmap) {
                (&dimensionless, &dimensionless)
            } else {
                (
                    &tensor_field(series, "x")?.dim,
                    &tensor_field(series, "y")?.dim,
                )
            };
            axes.x.merge(dim_x, x, units)?;
            axes.y.merge(dim_y, y, units)?;
            Ok(())
        })?;
        visit_series(spec, |series| {
            if matches!(
                SeriesKind::parse(series.fields.get("kind"))?,
                SeriesKind::Heatmap
            ) {
                // Retain blank edge cells in the displayed matrix domain.
                let (_, rows, cols, count) = heatmap_shape(series)?;
                if count != 0 {
                    let xmax = grid_coordinate(cols)?;
                    let ymax = grid_coordinate(rows)?;
                    axes.x.extend(0.0);
                    axes.x.extend(xmax);
                    axes.y.extend(0.0);
                    axes.y.extend(ymax);
                }
                return Ok(());
            }
            glyphs(series, RED, |glyph, _| {
                if let Glyph::Box(start, end) = glyph {
                    // A logarithmic baseline lies outside the displayed domain,
                    // but its horizontal bin edge still contributes.
                    if axes.point(end).is_some() {
                        axes.x.extend(start.0);
                        axes.x.extend(end.0);
                        axes.y.extend(start.1);
                        axes.y.extend(end.1);
                    }
                    return Ok(());
                }
                let corners: [Point; 2] = glyph.corners().into();
                // Non-positive points on a logarithmic axis are omitted,
                // so an invalid counterpart must not expand the other axis.
                for (x, y) in corners {
                    if axes.x.transform(x).is_some() && axes.y.transform(y).is_some() {
                        axes.x.extend(x);
                        axes.y.extend(y);
                    }
                }
                Ok(())
            })
        })?;
        Ok(axes)
    }

    fn point(
        &self,
        (x, y): Point,
    ) -> Option<Point> {
        Some((self.x.transform(x)?, self.y.transform(y)?))
    }
}

impl Glyph {
    const fn corners(self) -> (Point, Point) {
        match self {
            | Self::Segment(a, b) | Self::Whisker(a, b) | Self::Box(a, b) => (a, b),
            | Self::Dot(a) => (a, a),
        }
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "dispatches line, scatter and whisker geometry over shared coordinates"
)]
fn path_glyphs(
    series: &RecordValue,
    kind: SeriesKind,
    ink: [u8; CHANNELS],
    mut draw: impl FnMut(Glyph, [u8; CHANNELS]) -> Result<(), String>,
) -> Result<(), String> {
    let x = tensor_field(series, "x")?;
    let y = tensor_field(series, "y")?;
    let count = vector_len(x, "x coordinates")?;
    if vector_len(y, "y coordinates")? != count {
        return Err("different x and y lengths".into());
    }
    let error = if matches!(kind, SeriesKind::ErrorBar) {
        let error = tensor_field(series, "error")?;
        if vector_len(error, "error magnitudes")? != count || error.dim != y.dim {
            return Err("error magnitudes must match y length and dimension".into());
        }
        Some((error, error.dense()))
    } else {
        None
    };
    let (xs, ys) = (x.dense(), y.dense());
    let mut previous = None;
    let mut connected = false;
    for index in 0..count {
        let px = coordinate(x, xs, index, "x")?;
        let py = coordinate(y, ys, index, "y")?;
        if !px.is_finite() || !py.is_finite() {
            continue;
        }
        if let Some((tensor, values)) = error {
            let e = coordinate(tensor, values, index, "error")?;
            if e.is_finite() && e >= 0.0 {
                let (lo, hi) = (py - e, py + e);
                if lo.is_finite() && hi.is_finite() {
                    draw(Glyph::Whisker((px, lo), (px, hi)), ink)?;
                }
            } else if e.is_finite() {
                return Err("error magnitudes must be nonnegative".into());
            }
        }
        let here = (px, py);
        if matches!(kind, SeriesKind::Line) {
            if let Some(start) = previous {
                draw(Glyph::Segment(start, here), ink)?;
                connected = true;
            }
            previous = Some(here);
        } else {
            draw(Glyph::Dot(here), ink)?;
        }
    }
    if matches!(kind, SeriesKind::Line)
        && !connected
        && let Some(point) = previous
    {
        draw(Glyph::Dot(point), ink)?;
    }
    Ok(())
}

#[allow(
    clippy::single_call_fn,
    reason = "validates histogram shape and emits bin rectangles"
)]
fn histogram_glyphs(
    series: &RecordValue,
    ink: [u8; CHANNELS],
    mut draw: impl FnMut(Glyph, [u8; CHANNELS]) -> Result<(), String>,
) -> Result<(), String> {
    let x = tensor_field(series, "x")?;
    let y = tensor_field(series, "y")?;
    let (edges, bins) = (vector_len(x, "bin edges")?, vector_len(y, "bin counts")?);
    if bins.checked_add(1) != Some(edges) {
        return Err("histogram needs one more edge than bin counts".into());
    }
    let (xs, ys) = (x.dense(), y.dense());
    let mut left = coordinate(x, xs, 0, "edge")?;
    for bin in 0..bins {
        let next = bin.checked_add(1).ok_or("histogram edge index overflows")?;
        let right = coordinate(x, xs, next, "edge")?;
        let count = coordinate(y, ys, bin, "count")?;
        if !left.is_finite()
            || !right.is_finite()
            || right < left
            || !count.is_finite()
            || count < 0.0
        {
            return Err("histogram requires finite increasing edges and nonnegative counts".into());
        }
        draw(Glyph::Box((left, 0.0), (right, count)), ink)?;
        left = right;
    }
    Ok(())
}

fn heatmap_shape(series: &RecordValue) -> Result<(&TensorData, usize, usize, usize), String> {
    let tensor = tensor_field(series, "x")?;
    let &[rows, cols] = tensor.shape.as_slice() else {
        return Err("heatmap cells must form a matrix".into());
    };
    let count = super::checked_element_count(&tensor.shape, Span::default())
        .map_err(|_| "heatmap exceeds the allocation limit")?;
    if tensor.storage.len() != count {
        return Err("heatmap shape and storage disagree".into());
    }
    Ok((tensor, rows, cols, count))
}

fn grid_coordinate(index: usize) -> Result<f64, String> {
    u32::try_from(index)
        .map(f64::from)
        .map_err(|_| "heatmap coordinate exceeds the supported range".into())
}

#[allow(
    clippy::single_call_fn,
    reason = "tracks cell bounds and color before emitting rectangles"
)]
fn heatmap_glyphs(
    series: &RecordValue,
    ink: [u8; CHANNELS],
    mut draw: impl FnMut(Glyph, [u8; CHANNELS]) -> Result<(), String>,
) -> Result<(), String> {
    let (tensor, rows, cols, count) = heatmap_shape(series)?;
    if count == 0 {
        return Ok(());
    }
    let values = tensor.dense();
    let mut color_range: Option<Axis> = None;
    for offset in 0..count {
        let cell = coordinate(tensor, values, offset, "cell")?;
        if cell.is_finite() {
            color_range
                .get_or_insert_with(|| Axis::at(cell))
                .include(cell);
        }
    }
    let Some(range) = color_range else {
        return Ok(());
    };
    for offset in 0..count {
        let cell = coordinate(tensor, values, offset, "cell")?;
        if !cell.is_finite() {
            continue;
        }
        let row = offset
            .checked_div(cols)
            .ok_or("heatmap row index overflow")?;
        let col = offset
            .checked_rem(cols)
            .ok_or("heatmap column index overflow")?;
        let top = rows.checked_sub(row).ok_or("heatmap row index overflow")?;
        let x = grid_coordinate(col)?;
        let y = grid_coordinate(top)?;
        let intensity = range.fraction(cell).mul_add(0.85, 0.15);
        let tint = |channel: u8| {
            (f64::from(channel) - 255.0)
                .mul_add(intensity, 255.0)
                .round()
                .to_u8()
                .unwrap_or(channel)
        };
        let fill = ink.map(tint);
        draw(Glyph::Box((x, y - 1.0), (x + 1.0, y)), fill)?;
    }
    Ok(())
}

fn glyphs(
    series: &RecordValue,
    ink: [u8; CHANNELS],
    draw: impl FnMut(Glyph, [u8; CHANNELS]) -> Result<(), String>,
) -> Result<(), String> {
    let kind = SeriesKind::parse(series.fields.get("kind"))?;
    match kind {
        | SeriesKind::Histogram => histogram_glyphs(series, ink, draw),
        | SeriesKind::Heatmap => heatmap_glyphs(series, ink, draw),
        | SeriesKind::Line | SeriesKind::Scatter | SeriesKind::ErrorBar => {
            path_glyphs(series, kind, ink, draw)
        },
    }
}

fn reserve_svg(
    out: &mut String,
    additional: usize,
) -> Result<(), String> {
    let total = out
        .len()
        .checked_add(additional)
        .ok_or("SVG size overflows")?;
    super::checked_element_count(&[total], Span::default())
        .map_err(|_| "SVG exceeds the allocation limit")?;
    out.try_reserve(additional)
        .map_err(|_| "cannot allocate SVG data".into())
}

#[allow(
    clippy::single_call_fn,
    reason = "SVG text has distinct escaping from source and terminal output"
)]
fn write_xml_text(
    out: &mut String,
    text: &str,
) -> Result<(), String> {
    let capacity = text.len().checked_mul(5).ok_or("plot label is too long")?;
    reserve_svg(out, capacity)?;
    for ch in text.chars() {
        match ch {
            | '&' => out.push_str("&amp;"),
            | '<' => out.push_str("&lt;"),
            | '>' => out.push_str("&gt;"),
            | '\t' | '\n' | '\r' => out.push(ch),
            | '\0'..='\u{1f}' | '\u{fffe}' | '\u{ffff}' => out.push('\u{fffd}'),
            | _ => out.push(ch),
        }
    }
    Ok(())
}

fn pixel_count() -> Result<usize, String> {
    usize::from(WIDTH)
        .checked_mul(usize::from(HEIGHT))
        .ok_or_else(|| "plot frame dimensions overflow".to_string())
}

#[allow(
    clippy::single_call_fn,
    reason = "validates pixel coordinates and channel offsets at the buffer boundary"
)]
fn set_pixel(
    buf: &mut [u8],
    x: i32,
    y: i32,
    color: [u8; CHANNELS],
) -> Result<(), String> {
    let index = || {
        let x = u16::try_from(x).ok().filter(|x| *x < WIDTH)?;
        let y = u16::try_from(y).ok().filter(|y| *y < HEIGHT)?;
        let pixel = usize::from(y)
            .checked_mul(usize::from(WIDTH))?
            .checked_add(usize::from(x))?;
        let offset = pixel.checked_mul(CHANNELS)?;
        Some(offset..offset.checked_add(CHANNELS)?)
    };
    let pixel = index()
        .and_then(|range| buf.get_mut(range))
        .ok_or("plot pixel is outside the raster frame")?;
    pixel.copy_from_slice(&color);
    Ok(())
}

fn draw_line(
    buf: &mut [u8],
    from: Pixel,
    to: Pixel,
    color: [u8; CHANNELS],
) -> Result<(), String> {
    let (mut x, mut y) = (i32::from(from.0), i32::from(from.1));
    let (end_x, end_y) = (i32::from(to.0), i32::from(to.1));
    let dx = i32::from(from.0.abs_diff(to.0));
    let dy = i32::from(from.1.abs_diff(to.1)).saturating_neg();
    let step_x = if x < end_x { 1 } else { -1 };
    let step_y = if y < end_y { 1 } else { -1 };
    // The inputs are u16 pixel positions. The Bresenham accumulators fit
    // in i32 across the entire coordinate domain.
    let mut error = dx.saturating_add(dy);
    loop {
        set_pixel(buf, x, y, color)?;
        if x == end_x && y == end_y {
            return Ok(());
        }
        let twice = error.saturating_mul(2);
        if twice >= dy {
            error = error.saturating_add(dy);
            x = x.saturating_add(step_x);
        }
        if twice <= dx {
            error = error.saturating_add(dx);
            y = y.saturating_add(step_y);
        }
    }
}

#[derive(Clone, Copy)]
struct Axis {
    min: f64,
    max: f64,
}

impl Axis {
    const fn at(value: f64) -> Self {
        Self {
            min: value,
            max: value,
        }
    }

    const fn include(
        &mut self,
        value: f64,
    ) {
        self.min = self.min.min(value);
        self.max = self.max.max(value);
    }

    fn fraction(
        self,
        value: f64,
    ) -> f64 {
        let width = self.max - self.min;
        let fraction = if width == 0.0 {
            0.5
        } else if width.is_finite() {
            (value - self.min) / width
        } else {
            // Halve all three finite operands before subtracting to cover
            // endpoints such as -f64::MAX and f64::MAX without overflow.
            let lo = self.min * 0.5;
            let hi = self.max * 0.5;
            value.mul_add(0.5, -lo) / (hi - lo)
        };
        fraction.clamp(0.0, 1.0)
    }
}

struct Viewport {
    x: Axis,
    y: Axis,
}

#[derive(Clone, Copy)]
enum Paint {
    Line(Pixel, Pixel),
    Dot(Pixel),
    Rect(Pixel, Pixel),
}

impl Axis {
    fn contains(
        self,
        value: f64,
    ) -> bool {
        (self.min..=self.max).contains(&value)
    }
}

/// Clip a segment to one axis, interpolating the other coordinate where the
/// segment crosses an edge. It is safe to order endpoints for each pass.
fn clip_axis(
    limit: Axis,
    (a, b): (Point, Point),
) -> Option<(Point, Point)> {
    let (a, b) = if a.0 <= b.0 { (a, b) } else { (b, a) };
    if a.0 > limit.max || b.0 < limit.min {
        return None;
    }
    let lerp = |boundary: f64| {
        let fraction = Axis { min: a.0, max: b.0 }.fraction(boundary);
        a.1.mul_add(1.0 - fraction, fraction * b.1)
    };
    Some((
        if a.0 < limit.min {
            (limit.min, lerp(limit.min))
        } else {
            a
        },
        if b.0 > limit.max {
            (limit.max, lerp(limit.max))
        } else {
            b
        },
    ))
}

impl Viewport {
    fn new(axes: &PlotAxes) -> Result<Self, String> {
        Ok(Self {
            x: axes.x.domain()?,
            y: axes.y.domain()?,
        })
    }

    fn pixel(
        &self,
        (x, y): Point,
    ) -> Result<Pixel, String> {
        let px = self.x.fraction(x).mul_add(720.0, 40.0).round().to_u16();
        let py = self.y.fraction(y).mul_add(-340.0, 360.0).round().to_u16();
        match (px, py) {
            | (Some(x), Some(y)) => Ok((x, y)),
            | _ => Err("cannot project plot coordinate into the frame".into()),
        }
    }

    fn segment(
        &self,
        from: Point,
        to: Point,
    ) -> Result<Option<(Pixel, Pixel)>, String> {
        let transpose = |(x, y): Point| (y, x);
        let Some(((left, bottom), (right, top))) = clip_axis(self.x, (from, to)) else {
            return Ok(None);
        };
        let Some((a, b)) = clip_axis(self.y, ((bottom, left), (top, right))) else {
            return Ok(None);
        };
        Ok(Some((self.pixel(transpose(a))?, self.pixel(transpose(b))?)))
    }

    fn rectangle(
        &self,
        a: Point,
        b: Point,
    ) -> Result<Option<(Pixel, Pixel)>, String> {
        let (left, right) = (a.0.min(b.0), a.0.max(b.0));
        let (bottom, top) = (a.1.min(b.1), a.1.max(b.1));
        if left > self.x.max || right < self.x.min || bottom > self.y.max || top < self.y.min {
            return Ok(None);
        }
        Ok(Some((
            self.pixel((left.max(self.x.min), top.min(self.y.max)))?,
            self.pixel((right.min(self.x.max), bottom.max(self.y.min)))?,
        )))
    }

    fn paint(
        &self,
        axes: &PlotAxes,
        glyph: Glyph,
        mut output: impl FnMut(Paint) -> Result<(), String>,
    ) -> Result<(), String> {
        match glyph {
            | Glyph::Dot(point) => {
                if let Some((x, y)) = axes.point(point)
                    && self.x.contains(x)
                    && self.y.contains(y)
                {
                    output(Paint::Dot(self.pixel((x, y))?))?;
                }
            },
            | Glyph::Segment(a, b) | Glyph::Whisker(a, b) => {
                if let (Some(a), Some(b)) = (axes.point(a), axes.point(b))
                    && let Some((from, to)) = self.segment(a, b)?
                {
                    output(Paint::Line(from, to))?;
                    if matches!(glyph, Glyph::Whisker(..)) {
                        for (x, y) in [from, to] {
                            output(Paint::Line(
                                (x.saturating_sub(3).max(40), y),
                                (x.saturating_add(3).min(760), y),
                            ))?;
                        }
                    }
                }
            },
            | Glyph::Box(a, b) => {
                // A rectangle can start below a logarithmic axis (for example,
                // a histogram bar at zero). Clip that edge to the lower limit.
                let start = |axis: &FrameAxis, bound: Axis, value: f64| {
                    axis.transform(value)
                        .or_else(|| (axis.log == Some(true)).then_some(bound.min))
                };
                let lower = (start(&axes.x, self.x, a.0), start(&axes.y, self.y, a.1));
                let (Some(x), Some(y)) = lower else {
                    return Ok(());
                };
                if let Some(end) = axes.point(b)
                    && let Some((from, to)) = self.rectangle((x, y), end)?
                {
                    output(Paint::Rect(from, to))?;
                }
            },
        }
        Ok(())
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps SVG serialization separate from geometry and escaped labels"
)]
fn write_svg_mark(
    out: &mut String,
    mark: Paint,
    ink: [u8; CHANNELS],
) -> Result<(), String> {
    reserve_svg(out, 160)?;
    let [red, green, blue, _] = ink;
    match mark {
        | Paint::Line((x1, y1), (x2, y2)) => write!(out,
            r##"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="#{red:02x}{green:02x}{blue:02x}" stroke-width="2"/>"##,
        ),
        | Paint::Dot((cx, cy)) => write!(out,
            r##"<circle cx="{cx}" cy="{cy}" r="2" fill="#{red:02x}{green:02x}{blue:02x}"/>"##,
        ),
        | Paint::Rect((x, y), (right, bottom)) => {
            let width = right.saturating_sub(x).max(1);
            let height = bottom.saturating_sub(y).max(1);
            write!(out,
                r##"<rect x="{x}" y="{y}" width="{width}" height="{height}" fill="#{red:02x}{green:02x}{blue:02x}"/>"##,
            )
        },
    }
    .map_err(|_| "cannot render SVG geometry".into())
}

/// Render a standalone SVG or an HTML document containing SVG.
pub(super) fn render_document(
    spec: &Value,
    standalone: bool,
    units: &UnitTable,
    span: Span,
) -> Result<String, EvalError> {
    let diagnostic =
        |error| EvalError::new("E0905", format!("plot rendering failed: {error}")).at(span);
    let axes = PlotAxes::read(spec, units).map_err(diagnostic)?;
    let viewport = Viewport::new(&axes).map_err(diagnostic)?;
    let mut out = String::new();
    if !standalone {
        out.push_str(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>Aevia plot</title></head><body>",
        );
    }
    out.push_str(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 800 400\" width=\"800\" height=\"400\" style=\"background:#fff\"><rect x=\"40\" y=\"20\" width=\"720\" height=\"340\" fill=\"none\" stroke=\"#ccc\"/>",
    );
    visit_series(spec, |series| {
        let style = Style::read(series)?;
        reserve_svg(&mut out, 32)?;
        out.push_str("<g><title>");
        write_xml_text(&mut out, style.label)?;
        out.push_str("</title>");
        glyphs(series, style.ink, |glyph, ink| {
            viewport.paint(&axes, glyph, |mark| write_svg_mark(&mut out, mark, ink))
        })?;
        out.push_str("</g>");
        Ok(())
    })
    .map_err(diagnostic)?;
    for (x, y, label) in [(400, 384, axes.x.label("x")), (10, 18, axes.y.label("y"))] {
        reserve_svg(&mut out, 80).map_err(diagnostic)?;
        write!(out, "<text x=\"{x}\" y=\"{y}\" font-size=\"12\">")
            .map_err(|_| diagnostic("cannot render axis label".into()))?;
        write_xml_text(&mut out, &label).map_err(diagnostic)?;
        out.push_str("</text>");
    }
    reserve_svg(&mut out, 20).map_err(diagnostic)?;
    out.push_str("</svg>");
    if !standalone {
        out.push_str("</body></html>");
    }
    Ok(out)
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps raster mark encoding separate from shared geometry"
)]
fn paint_rgba(
    buf: &mut [u8],
    mark: Paint,
    ink: [u8; CHANNELS],
) -> Result<(), String> {
    match mark {
        | Paint::Line(a, b) => draw_line(buf, a, b, ink),
        | Paint::Dot((x, y)) => {
            for dx in -2i32..=2 {
                for dy in -2i32..=2 {
                    if dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)) <= 4 {
                        set_pixel(
                            buf,
                            i32::from(x).saturating_add(dx),
                            i32::from(y).saturating_add(dy),
                            ink,
                        )?;
                    }
                }
            }
            Ok(())
        },
        | Paint::Rect((left, top), (right, bottom)) => {
            let columns = right.saturating_sub(left).saturating_add(1);
            if columns >= 8 && right < WIDTH && top <= bottom && bottom < HEIGHT {
                let row_stride = usize::from(WIDTH).saturating_mul(CHANNELS);
                let offset = |column, row| {
                    usize::from(row)
                        .saturating_mul(row_stride)
                        .saturating_add(usize::from(column).saturating_mul(CHANNELS))
                };
                let start = offset(left, top);
                let stop = offset(right.saturating_add(1), bottom);
                if let Some(rows) = buf.get_mut(start..stop) {
                    let width = usize::from(columns).saturating_mul(CHANNELS);
                    // Each chunk starts at the same column in the next physical row.
                    for row in rows.chunks_mut(row_stride) {
                        let pixels = row
                            .get_mut(..width)
                            .ok_or("plot pixel is outside the raster frame")?;
                        pixels.as_chunks_mut::<CHANNELS>().0.fill(ink);
                    }
                    return Ok(());
                }
            }
            for y in top..=bottom {
                for x in left..=right {
                    set_pixel(buf, i32::from(x), i32::from(y), ink)?;
                }
            }
            Ok(())
        },
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "renders one RGBA frame independently of its GIF envelope"
)]
fn render_plot_rgba_frame(
    spec: &Value,
    delay_cs: u16,
    units: &UnitTable,
) -> Result<GifFrame, String> {
    let axes = PlotAxes::read(spec, units)?;
    let viewport = Viewport::new(&axes)?;
    let bytes = pixel_count()?
        .checked_mul(CHANNELS)
        .ok_or("plot RGBA buffer size overflows")?;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(bytes)
        .map_err(|_| "cannot allocate plot frame")?;
    rgba.resize(bytes, 255);
    for (from, to) in [
        ((40, 20), (760, 20)),
        ((760, 20), (760, 360)),
        ((760, 360), (40, 360)),
        ((40, 360), (40, 20)),
    ] {
        draw_line(&mut rgba, from, to, PALE)?;
    }
    visit_series(spec, |series| {
        let style = Style::read(series)?;
        glyphs(series, style.ink, |glyph, ink| {
            viewport.paint(&axes, glyph, |mark| paint_rgba(&mut rgba, mark, ink))
        })
    })?;
    Ok(GifFrame {
        width: WIDTH,
        height: HEIGHT,
        delay_cs,
        rgba,
    })
}

/// Render a vector of plot specifications as frames of a fixed-size GIF.
#[allow(
    clippy::single_call_fn,
    reason = "validates and assembles the animation before the builtin builds its IO action"
)]
pub(super) fn render_animation(
    specs: &TensorData,
    delay: Option<&Value>,
    units: &UnitTable,
    span: Span,
) -> Result<GifAnimation, String> {
    let delay_cs = match delay {
        | None => 10,
        | Some(Value::Num(number)) => {
            match &number.kind {
                | NumKind::Int(value) => value.as_i64().and_then(|value| u16::try_from(value).ok()),
                | _ => None,
            }
            .ok_or("plot_save_gif delay_cs must be an Int in 0..=65535")?
        },
        | Some(_) => return Err("plot_save_gif delay_cs must be an Int in 0..=65535".into()),
    };
    let count = vector_len(specs, "plot_save_gif frames")?;
    if count == 0 {
        return Err("plot_save_gif requires at least one plot frame".into());
    }
    // Apply the builtin allocation budget to the total resident frame pixels.
    super::checked_element_count(&[count, pixel_count()?], span)
        .map_err(|_| "plot animation exceeds the raster allocation limit")?;
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(count)
        .map_err(|_| "cannot allocate plot frames")?;
    for index in 0..count {
        let spec = specs
            .get_flat(index)
            .ok_or("plot frame missing from storage")?;
        let frame = render_plot_rgba_frame(&spec, delay_cs, units)
            .map_err(|error| format!("plot frame {index}: {error}"))?;
        frames.push(frame);
    }
    Ok(GifAnimation {
        width: WIDTH,
        height: HEIGHT,
        loop_count: 0,
        frames,
    })
}

#[allow(
    clippy::single_call_fn,
    reason = "validates and filters histogram data before handing it to RSSN"
)]
fn histogram(
    values: &TensorData,
    bins: usize,
    span: Span,
) -> Result<(Vec<f64>, Vec<f64>), EvalError> {
    use std::borrow::Cow;

    let fail = |message: &str| EvalError::new("E0802", message).at(span);
    let count = vector_len(values, "histogram observations").map_err(|error| fail(&error))?;
    let slots = bins
        .checked_mul(2)
        .and_then(|sum| sum.checked_add(1))
        .ok_or_else(|| fail("histogram size overflows"))?;
    super::checked_element_count(&[slots], span)?;
    let dense = values.dense();
    let input =
        if let Some(finite) = dense.filter(|items| items.iter().all(|item| item.is_finite())) {
            Cow::Borrowed(finite)
        } else {
            let mut finite = Vec::new();
            finite
                .try_reserve_exact(count)
                .map_err(|_| fail("cannot allocate histogram observations"))?;
            for index in 0..count {
                let value = coordinate(values, dense, index, "observation")
                    .map_err(|error| fail(&error))?;
                if value.is_finite() {
                    finite.push(value);
                }
            }
            Cow::Owned(finite)
        };
    let (counts, edges) =
        aevia_std::stats::histogram(&input, bins).map_err(|error| fail(&error.to_string()))?;
    let counts = counts
        .into_iter()
        .map(|count| {
            count
                .to_f64()
                .ok_or_else(|| fail("histogram count cannot be represented"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((edges, counts))
}

/// The default styling record exposed by the Plot module and the Prelude.
pub fn default_plot_opts() -> Value {
    let scale = super::make_variant(
        adt::intern(adt::Adt {
            name: "AxisScale".to_owned(),
            params: Vec::new(),
            ctors: Vec::new(),
        }),
        0,
        "Linear",
        Vec::new(),
    );
    super::record_of(vec![
        ("label", super::none()),
        ("color", super::none()),
        ("xunit", super::none()),
        ("yunit", super::none()),
        ("xscale", scale.clone()),
        ("yscale", scale),
        ("xlim", super::none()),
        ("ylim", super::none()),
    ])
}

/// Carry the declared series shape and options to the shared renderer.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "the plot module owns series validation and construction"
    )
)]
pub(super) fn build_series(
    name: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EvalError> {
    let kind = match name {
        | "line" | "line_with" => SeriesKind::Line,
        | "scatter" | "scatter_with" => SeriesKind::Scatter,
        | "errorbar" | "errorbar_with" => SeriesKind::ErrorBar,
        | "hist" | "hist_with" => SeriesKind::Histogram,
        | "heatmap" | "heatmap_with" => SeriesKind::Heatmap,
        | _ => return Err(EvalError::new("E0302", "unknown plot series kind").at(span)),
    };
    let mut fields = indexmap::IndexMap::new();
    fields.insert("kind".to_owned(), Value::string(name));
    let options_index = match kind {
        | SeriesKind::Histogram => {
            let values = super::expect_tensor(args, 0, name, span)?;
            let bins = super::expect_exact_int(args, 1, name, span)?
                .to_usize()
                .filter(|&bins| bins != 0)
                .ok_or_else(|| {
                    EvalError::new("E0802", "histogram needs a positive bin count").at(span)
                })?;
            let (edges, counts) = histogram(&values, bins, span)?;
            fields.insert(
                "x".to_owned(),
                super::float_tensor(vec![edges.len()], edges, values.dim.clone()),
            );
            fields.insert(
                "y".to_owned(),
                super::float_tensor(vec![counts.len()], counts, Dim::one()),
            );
            2
        },
        | SeriesKind::Heatmap => {
            let values = super::expect_tensor(args, 0, name, span)?;
            fields.insert("x".to_owned(), Value::Tensor(values));
            1
        },
        | SeriesKind::Line | SeriesKind::Scatter | SeriesKind::ErrorBar => {
            let x = super::expect_tensor(args, 0, name, span)?;
            let y = super::expect_tensor(args, 1, name, span)?;
            fields.insert("x".to_owned(), Value::Tensor(x));
            fields.insert("y".to_owned(), Value::Tensor(y));
            if matches!(kind, SeriesKind::ErrorBar) {
                let error = super::expect_tensor(args, 2, name, span)?;
                fields.insert("error".to_owned(), Value::Tensor(error));
                3
            } else {
                2
            }
        },
    };
    if name.ends_with("_with") {
        let Some(options @ Value::Record(_)) = args.get(options_index) else {
            return Err(EvalError::new("E0302", "expected a PlotOpts record").at(span));
        };
        fields.insert("options".to_owned(), options.clone());
    }
    Ok(Value::Record(Rc::new(RecordValue { fields })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::RecordValue;
    use crate::value::Storage;
    use aevia_dim::Dim;
    use std::rc::Rc;

    fn vector(values: Vec<f64>) -> Value {
        Value::Tensor(Rc::new(TensorData::new(
            vec![values.len()],
            Storage::Dense(Rc::new(values)),
            Dim::one(),
            None,
        )))
    }

    fn spec(
        xs: Vec<f64>,
        ys: Vec<f64>,
        label: &str,
    ) -> Value {
        let fields = [
            ("x".to_owned(), vector(xs)),
            ("y".to_owned(), vector(ys)),
            ("label".to_owned(), Value::string(label)),
        ];
        let series = Value::Record(Rc::new(RecordValue {
            fields: fields.into_iter().collect(),
        }));
        let tensor = TensorData::new(vec![1], Storage::Boxed(vec![series]), Dim::one(), None);
        Value::Record(Rc::new(RecordValue {
            fields: [("series".to_owned(), Value::Tensor(Rc::new(tensor)))]
                .into_iter()
                .collect(),
        }))
    }

    #[test]
    fn synthetic_plot_vectors_are_rejected_before_traversal() {
        let len = usize::MAX;
        let input = TensorData::new(
            vec![len],
            Storage::View(crate::value::View {
                data: Rc::new(vec![1.0]),
                offset: 0,
                strides: vec![0],
                reversed: vec![false],
                len,
            }),
            Dim::one(),
            None,
        );
        assert!(
            vector_len(&input, "plot x coordinates")
                .unwrap_err()
                .contains("allocation limit")
        );
    }

    #[test]
    fn extreme_finite_ranges_reach_both_sides_of_the_raster() {
        let spec = spec(vec![-f64::MAX, f64::MAX], vec![-f64::MAX, f64::MAX], "");
        let frame =
            render_plot_rgba_frame(&spec, 10, &UnitTable::si()).expect("extreme finite plot");
        let index = ((20 * usize::from(WIDTH)) + 760) * CHANNELS;
        assert_eq!(
            frame.rgba.get(index..index + CHANNELS),
            Some(RED.as_slice())
        );
        assert_eq!(frame.rgba.len(), 800 * 400 * CHANNELS);
    }

    #[test]
    fn raster_rectangles_match_pixel_traversal_and_keep_partial_error_writes() {
        let bytes_per_row = usize::from(WIDTH) * CHANNELS;
        let frame = bytes_per_row * usize::from(HEIGHT);
        for (size, rectangles) in [
            (frame, vec![((0, 0), (799, 399)), ((40, 20), (759, 359))]),
            (frame, vec![((0, 0), (6, 0)), ((792, 399), (799, 399))]),
            (frame, vec![((30, 20), (0, 20)), ((0, 30), (30, 20))]),
            (frame, vec![((790, 0), (800, 0)), ((0, 399), (10, 400))]),
            (bytes_per_row + 13 * CHANNELS, vec![((3, 0), (25, 1))]),
            (bytes_per_row + 13 * CHANNELS, vec![((3, 0), (12, 1))]),
        ] {
            let mut expected = vec![255; size];
            let mut actual = vec![255; size];
            for (index, (from, to)) in rectangles.into_iter().enumerate() {
                let ink = if index == 0 {
                    [27, 43, 173, 5]
                } else {
                    RED
                };
                let reference = (|| {
                    for y in from.1..=to.1 {
                        for x in from.0..=to.0 {
                            set_pixel(&mut expected, i32::from(x), i32::from(y), ink)?;
                        }
                    }
                    Ok(())
                })();
                assert_eq!(
                    paint_rgba(&mut actual, Paint::Rect(from, to), ink),
                    reference
                );
                assert_eq!(actual, expected, "{from:?} to {to:?} in {size} bytes");
            }
        }
    }

    #[test]
    fn both_renderers_validate_coordinate_lengths_and_svg_labels_are_text() {
        let valid = spec(vec![0.0, 1.0], vec![0.0, 1.0], "A & <B>");
        let svg = render_document(&valid, true, &UnitTable::si(), Span::default()).expect("SVG");
        assert!(svg.contains("<title>A &amp; &lt;B&gt;</title>"));
        let invalid = spec(vec![0.0, 1.0], vec![0.0], "");
        assert_eq!(
            render_document(&invalid, true, &UnitTable::si(), Span::default())
                .unwrap_err()
                .code,
            "E0905"
        );
        assert!(
            render_plot_rgba_frame(&invalid, 10, &UnitTable::si())
                .unwrap_err()
                .contains("different x and y lengths")
        );
    }
}
