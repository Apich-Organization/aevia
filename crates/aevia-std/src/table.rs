//! The data layer: CSV, JSON and `NumPy` files (§8.11).
//!
//! A `Table` at run time is a record — a list of column names, a list of column
//! dtypes, and a tensor of row records — so there is no `Table` value in the
//! runtime and no new variant in `Value`. This module only parses and
//! serialises text and bytes; turning a [`Table`] into a runtime record is
//! `aevia-core`'s job.
//!
//! The CSV reader understands the header form §8.11 specifies: `time [s]` and
//! `velocity [m/s]` name the unit of the column. A column without a unit is
//! dimensionless, and every cell is parsed as a number when it parses as one
//! and kept as text when it does not.

use crate::NumericError;
use std::fmt::Write as _;
use std::str::Chars;

/// A parsed CSV table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Table {
    /// The column names, without their units.
    pub columns: Vec<String>,
    /// The unit of each column, from the header, when it has one.
    pub units: Vec<Option<String>>,
    /// The cells, row-major, as text.
    pub rows: Vec<Vec<String>>,
}

impl Table {
    /// The number of rows.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.rows.len()
    }

    /// True when the table has no rows.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// One column as text.
    ///
    /// # Errors
    ///
    /// [`NumericError::Domain`] when the name is not a column.
    pub fn column(
        &self,
        name: &str,
    ) -> Result<Vec<&str>, NumericError> {
        let index = self
            .columns
            .iter()
            .position(|column| column == name)
            .ok_or_else(|| NumericError::Domain(format!("no column named `{name}`")))?;
        Ok(self
            .rows
            .iter()
            .map(|row| row.get(index).map_or("", String::as_str))
            .collect())
    }

    /// One column as numbers, when every cell parses.
    ///
    /// # Errors
    ///
    /// [`NumericError::Domain`] when the name is not a column or a cell is not
    /// a number.
    pub fn numeric_column(
        &self,
        name: &str,
    ) -> Result<Vec<f64>, NumericError> {
        let cells = self.column(name)?;
        let mut out = Vec::with_capacity(cells.len());
        for cell in cells {
            let parsed = cell.trim().parse::<f64>().map_err(|_| {
                NumericError::Domain(format!("column `{name}` holds `{cell}`, not a number"))
            })?;
            out.push(parsed);
        }
        Ok(out)
    }
}

/// Parses CSV text with a header row and optional column units.
///
/// Unquoted fields are trimmed. Quoted fields preserve whitespace, embedded
/// line breaks, and escaped quotation marks. Records can end with LF, CRLF,
/// or CR. Whitespace-only records without separators are skipped.
///
/// # Errors
/// [`NumericError::Empty`] when there is no header,
/// [`NumericError::Domain`] for malformed quoting, and
/// [`NumericError::ShapeMismatch`] when a row width differs from the header.
pub fn parse_csv(text: &str) -> Result<Table, NumericError> {
    parse_delimited(text, ',')
}

/// Parses tab-separated text with the same quoting and header-unit rules as CSV.
///
/// # Errors
/// Returns the same errors as [`parse_csv`] for missing headers or malformed rows.
pub fn parse_tsv(text: &str) -> Result<Table, NumericError> {
    parse_delimited(text, '\t')
}

fn parse_delimited(
    text: &str,
    delimiter: char,
) -> Result<Table, NumericError> {
    let mut records = text.trim_start_matches('\u{feff}').chars();
    let header = next_csv_record(&mut records, delimiter)?.ok_or(NumericError::Empty)?;
    let (columns, units): (Vec<_>, Vec<_>) = header
        .into_iter()
        .map(|field| {
            if let Some((name, unit)) = field.split_once('[')
                && let Some(unit) = unit.trim_end().strip_suffix(']')
            {
                let unit = unit.trim();
                return (
                    name.trim().to_string(),
                    (!unit.is_empty()).then(|| unit.to_string()),
                );
            }
            (field, None)
        })
        .unzip();
    let width = columns.len();
    let mut rows = Vec::new();
    while let Some(cells) = next_csv_record(&mut records, delimiter)? {
        if cells.len() != width {
            return Err(NumericError::ShapeMismatch(format!(
                "a row has {} cell(s), the header has {width}",
                cells.len()
            )));
        }
        rows.push(cells);
    }
    Ok(Table { columns, units, rows })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CsvQuoting {
    Unquoted,
    Quoted,
    Closed,
}

fn finish_csv_field(
    field: String,
    quoting: CsvQuoting,
) -> String {
    if quoting == CsvQuoting::Unquoted {
        let trimmed = field.trim();
        if trimmed.len() != field.len() {
            return trimmed.to_string();
        }
    }
    field
}

fn next_csv_record(
    characters: &mut Chars<'_>,
    delimiter: char,
) -> Result<Option<Vec<String>>, NumericError> {
    loop {
        let remaining = characters.as_str();
        if remaining.is_empty() {
            return Ok(None);
        }
        // A quote before the first line ending may continue onto another line.
        let line_end = remaining.find(['\r', '\n', '"']).unwrap_or(remaining.len());
        let (line, ending) = remaining.split_at(line_end);
        if ending.starts_with('"') {
            return next_quoted_csv_record(characters, delimiter);
        }
        let following = ending
            .strip_prefix("\r\n")
            .or_else(|| ending.strip_prefix(['\r', '\n']))
            .unwrap_or(ending);
        *characters = following.chars();
        if !line.trim().is_empty() || line.contains(delimiter) {
            return Ok(Some(
                line.split(delimiter)
                    .map(|field| field.trim().to_owned())
                    .collect(),
            ));
        }
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "keeps quoted records and embedded line breaks in the CSV state machine"
)]
fn next_quoted_csv_record(
    characters: &mut Chars<'_>,
    delimiter: char,
) -> Result<Option<Vec<String>>, NumericError> {
    let mut cells = Vec::new();
    let mut field = String::new();
    let mut quoting = CsvQuoting::Unquoted;
    let mut nonblank = false;

    while let Some(character) = characters.next() {
        if quoting == CsvQuoting::Quoted {
            if character != '"' {
                field.push(character);
            } else if let Some(following) = characters.as_str().strip_prefix('"') {
                *characters = following.chars();
                field.push('"');
            } else {
                quoting = CsvQuoting::Closed;
            }
            continue;
        }

        match character {
            | '"' if quoting == CsvQuoting::Unquoted && field.trim().is_empty() => {
                field.clear();
                quoting = CsvQuoting::Quoted;
                nonblank = true;
            },
            | separator if separator == delimiter => {
                cells.push(finish_csv_field(std::mem::take(&mut field), quoting));
                quoting = CsvQuoting::Unquoted;
                nonblank = true;
            },
            | '\r' | '\n' => {
                if character == '\r'
                    && let Some(following) = characters.as_str().strip_prefix('\n')
                {
                    *characters = following.chars();
                }
                if nonblank {
                    cells.push(finish_csv_field(field, quoting));
                    return Ok(Some(cells));
                }
                field.clear();
            },
            | any if quoting == CsvQuoting::Closed && any.is_whitespace() => {},
            | '"' => return Err(NumericError::Domain("unexpected quote in CSV field".into())),
            | _ if quoting == CsvQuoting::Closed => {
                return Err(NumericError::Domain(
                    "unexpected text after quoted CSV field".into(),
                ));
            },
            | any => {
                nonblank |= !any.is_whitespace();
                field.push(any);
            },
        }
    }

    if quoting == CsvQuoting::Quoted {
        return Err(NumericError::Domain("unterminated quoted CSV field".into()));
    }
    if !nonblank {
        return Ok(None);
    }
    cells.push(finish_csv_field(field, quoting));
    Ok(Some(cells))
}

/// Serialises a table as CSV, with the unit back in the header.
#[must_use]
pub fn write_csv(table: &Table) -> String {
    let mut out = String::new();
    for (index, name) in table.columns.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        if let Some(unit) = table.units.get(index).and_then(Option::as_ref) {
            write_csv_field(&format!("{name} [{unit}]"), &mut out);
        } else {
            write_csv_field(name, &mut out);
        }
    }
    out.push('\n');
    for row in &table.rows {
        for (index, cell) in row.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            write_csv_field(cell, &mut out);
        }
        out.push('\n');
    }
    out
}

fn write_csv_field(
    cell: &str,
    out: &mut String,
) {
    if cell.is_empty()
        || cell.trim().len() != cell.len()
        || cell
            .bytes()
            .any(|byte| matches!(byte, b',' | b'"' | b'\r' | b'\n'))
    {
        out.push('"');
        let mut pieces = cell.split('"');
        out.push_str(pieces.next().unwrap_or_default());
        for piece in pieces {
            out.push_str("\"\"");
            out.push_str(piece);
        }
        out.push('"');
    } else {
        out.push_str(cell);
    }
}

/// A JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// A number.
    Number(f64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Self>),
    /// An object, in document order.
    Object(Vec<(String, Self)>),
}

impl Json {
    /// The value under a key, when this is an object.
    #[must_use]
    pub fn get(
        &self,
        key: &str,
    ) -> Option<&Self> {
        match self {
            | Self::Object(fields) => {
                fields
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value)
            },
            | _ => None,
        }
    }
}

/// Parses JSON text, preserving the order of object fields.
///
/// # Errors
/// Returns [`NumericError::Domain`] with the byte offset of invalid syntax,
/// nesting beyond 128 levels, or a number outside the finite Float range.
pub fn parse_json(text: &str) -> Result<Json, NumericError> {
    let mut parser = JsonParser {
        rest: text,
        input_len: text.len(),
    };
    let value = parser.value(128)?;
    parser.space();
    if !parser.rest.is_empty() {
        return Err(parser.error("trailing text"));
    }
    Ok(value)
}

struct JsonParser<'a> {
    rest: &'a str,
    input_len: usize,
}

impl JsonParser<'_> {
    fn error(
        &self,
        reason: &str,
    ) -> NumericError {
        let position = self.input_len.saturating_sub(self.rest.len());
        NumericError::Domain(format!("{reason} at offset {position}"))
    }

    fn space(&mut self) {
        self.rest = self.rest.trim_start_matches([' ', '\n', '\r', '\t']);
    }

    fn take(
        &mut self,
        pattern: &str,
    ) -> bool {
        if let Some(rest) = self.rest.strip_prefix(pattern) {
            self.rest = rest;
            true
        } else {
            false
        }
    }

    fn value(
        &mut self,
        remaining: usize,
    ) -> Result<Json, NumericError> {
        self.space();
        match self.rest.as_bytes().first() {
            | Some(b'n') if self.take("null") => Ok(Json::Null),
            | Some(b't') if self.take("true") => Ok(Json::Bool(true)),
            | Some(b'f') if self.take("false") => Ok(Json::Bool(false)),
            | Some(b'"') => self.string().map(Json::String),
            | Some(b'[' | b'{') if remaining == 0 => {
                Err(self.error("JSON nesting exceeds 128 levels"))
            },
            | Some(b'[') => self.array(remaining.saturating_sub(1)).map(Json::Array),
            | Some(b'{') => self.object(remaining.saturating_sub(1)).map(Json::Object),
            | Some(b'-' | b'0'..=b'9') => self.number().map(Json::Number),
            | _ => Err(self.error("expected a JSON value")),
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "The array grammar is independent of scalar and object parsing."
    )]
    fn array(
        &mut self,
        remaining: usize,
    ) -> Result<Vec<Json>, NumericError> {
        self.take("[");
        self.space();
        let mut values = Vec::new();
        if self.take("]") {
            return Ok(values);
        }
        loop {
            values.push(self.value(remaining)?);
            self.space();
            if self.take("]") {
                return Ok(values);
            }
            if !self.take(",") {
                return Err(self.error("expected `,` or `]`"));
            }
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "The object grammar is independent of scalar and array parsing."
    )]
    fn object(
        &mut self,
        remaining: usize,
    ) -> Result<Vec<(String, Json)>, NumericError> {
        self.take("{");
        self.space();
        let mut fields = Vec::new();
        if self.take("}") {
            return Ok(fields);
        }
        loop {
            self.space();
            let name = self.string()?;
            self.space();
            if !self.take(":") {
                return Err(self.error("expected `:`"));
            }
            fields.push((name, self.value(remaining)?));
            self.space();
            if self.take("}") {
                return Ok(fields);
            }
            if !self.take(",") {
                return Err(self.error("expected `,` or `}`"));
            }
        }
    }

    fn string(&mut self) -> Result<String, NumericError> {
        if !self.take("\"") {
            return Err(self.error("expected a JSON string"));
        }
        let mut out = String::new();
        loop {
            let Some(index) = self.rest.find(|character: char| {
                character == '"' || character == '\\' || character < '\u{20}'
            }) else {
                return Err(self.error("unterminated JSON string"));
            };
            let Some((prefix, rest)) = self.rest.split_at_checked(index) else {
                return Err(self.error("invalid JSON string boundary"));
            };
            self.rest = rest;
            if self.take("\"") {
                // Plain strings can allocate exactly their decoded length.
                // Reserve a growing buffer only after the first escape.
                if out.is_empty() {
                    return Ok(prefix.to_owned());
                }
                out.push_str(prefix);
                return Ok(out);
            }
            if !self.take("\\") {
                return Err(self.error("unescaped control character"));
            }
            out.push_str(prefix);
            out.push(self.escape()?);
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "Separates JSON escapes from ordinary UTF-8 text."
    )]
    fn escape(&mut self) -> Result<char, NumericError> {
        let Some((escape, rest)) = self.rest.split_at_checked(1) else {
            return Err(self.error("invalid string escape"));
        };
        self.rest = rest;
        match escape {
            | "\"" => Ok('"'),
            | "\\" => Ok('\\'),
            | "/" => Ok('/'),
            | "b" => Ok('\u{8}'),
            | "f" => Ok('\u{c}'),
            | "n" => Ok('\n'),
            | "r" => Ok('\r'),
            | "t" => Ok('\t'),
            | "u" => {
                let first = self.hex_unit()?;
                if let Some(character) = char::from_u32(u32::from(first)) {
                    return Ok(character);
                }
                if !self.take("\\u") {
                    return Err(self.error("invalid UTF-16 surrogate pair"));
                }
                char::decode_utf16([first, self.hex_unit()?])
                    .next()
                    .and_then(Result::ok)
                    .ok_or_else(|| self.error("invalid UTF-16 surrogate pair"))
            },
            | _ => Err(self.error("invalid string escape")),
        }
    }

    fn hex_unit(&mut self) -> Result<u16, NumericError> {
        let (hex, rest) = self
            .rest
            .split_at_checked(4)
            .filter(|(hex, _)| hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| self.error("invalid Unicode escape"))?;
        let unit =
            u16::from_str_radix(hex, 16).map_err(|_| self.error("invalid Unicode escape"))?;
        self.rest = rest;
        Ok(unit)
    }

    fn digits(&mut self) -> bool {
        let rest = self
            .rest
            .trim_start_matches(|character: char| character.is_ascii_digit());
        let consumed = rest.len() != self.rest.len();
        self.rest = rest;
        consumed
    }

    #[allow(
        clippy::single_call_fn,
        reason = "Validates JSON number grammar before Float conversion."
    )]
    fn number(&mut self) -> Result<f64, NumericError> {
        let start = self.rest;
        self.take("-");
        if !self.take("0") && !self.digits() {
            return Err(self.error("expected a digit"));
        }
        if self.take(".") && !self.digits() {
            return Err(self.error("expected fractional digits"));
        }
        if self.take("e") || self.take("E") {
            if let Some(rest) = self.rest.strip_prefix(['+', '-']) {
                self.rest = rest;
            }
            if !self.digits() {
                return Err(self.error("expected exponent digits"));
            }
        }
        let width = start.len().saturating_sub(self.rest.len());
        start
            .get(..width)
            .and_then(|number| number.parse::<f64>().ok())
            .filter(|number| number.is_finite())
            .ok_or_else(|| self.error("JSON number exceeds the finite Float range"))
    }
}

/// Serialises JSON compactly. Non-finite Float values are written as `null`.
#[must_use]
pub fn write_json(value: &Json) -> String {
    let mut out = String::new();
    write_json_into(value, &mut out);
    out
}

fn write_json_into(
    value: &Json,
    out: &mut String,
) {
    match value {
        | Json::Bool(true) => out.push_str("true"),
        | Json::Bool(false) => out.push_str("false"),
        | Json::Number(number) if number.is_finite() => {
            let _ = write!(out, "{number}");
        },
        | Json::Null | Json::Number(_) => out.push_str("null"),
        | Json::String(text) => write_json_string(text, out),
        | Json::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json_into(item, out);
            }
            out.push(']');
        },
        | Json::Object(fields) => {
            out.push('{');
            for (index, (name, item)) in fields.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json_string(name, out);
                out.push(':');
                write_json_into(item, out);
            }
            out.push('}');
        },
    }
}

fn write_json_string(
    text: &str,
    out: &mut String,
) {
    const CONTROL_ESCAPES: [&str; 32] = [
        "\\u0000", "\\u0001", "\\u0002", "\\u0003", "\\u0004", "\\u0005", "\\u0006", "\\u0007",
        "\\u0008", "\\t", "\\n", "\\u000b", "\\u000c", "\\r", "\\u000e", "\\u000f", "\\u0010",
        "\\u0011", "\\u0012", "\\u0013", "\\u0014", "\\u0015", "\\u0016", "\\u0017", "\\u0018",
        "\\u0019", "\\u001a", "\\u001b", "\\u001c", "\\u001d", "\\u001e", "\\u001f",
    ];
    out.push('"');
    let mut start = 0;
    let escapes = text
        .bytes()
        .enumerate()
        .filter(|&(_, byte)| byte < b' ' || matches!(byte, b'"' | b'\\'));
    for (end, byte) in escapes {
        // Escape bytes are ASCII, so both sides of each escape are UTF-8 boundaries.
        if start != end {
            out.push_str(text.get(start..end).unwrap_or_default());
        }
        let escape = match byte {
            | b'"' => "\\\"",
            | b'\\' => "\\\\",
            | control => {
                CONTROL_ESCAPES
                    .get(usize::from(control))
                    .copied()
                    .unwrap_or_default()
            },
        };
        out.push_str(escape);
        start = end.saturating_add(1);
    }
    out.push_str(text.get(start..).unwrap_or_default());
    out.push('"');
}

/// Read a `NumPy` Float array in C or Fortran order, preserving its rank.
///
/// # Errors
/// Returns a domain error for unsupported or malformed data.
pub fn read_npy(bytes: &[u8]) -> Result<(Vec<usize>, Vec<f64>), NumericError> {
    crate::npy::decode(bytes).map_err(NumericError::Domain)
}

/// Encode a `NumPy` Float array after validating shape and storage.
///
/// # Errors
/// Returns a domain error if shape and data are inconsistent or unrepresentable.
pub fn try_write_npy(
    shape: &[usize],
    values: &[f64],
) -> Result<Vec<u8>, NumericError> {
    crate::npy::encode(shape, values).map_err(NumericError::Domain)
}

/// Encode a valid `NumPy` Float array.
///
/// # Panics
/// Panics when the array cannot be encoded. [`try_write_npy`] returns the error.
#[must_use]
#[allow(
    clippy::expect_used,
    reason = "Preserves the public infallible wrapper for trusted data."
)]
pub fn write_npy(
    shape: &[usize],
    values: &[f64],
) -> Vec<u8> {
    try_write_npy(shape, values).expect("NPY shape and values must agree")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_writing_preserves_plain_and_quoted_utf8_fields() {
        let table = Table {
            columns: vec!["time".into(), "note,\"lab\"".into(), "observer".into()],
            units: vec![Some("s".into()), None, None],
            rows: vec![
                vec!["0.125".into(), "Δ\"値\"\r\n🙂".into(), "O'Reilly".into()],
                vec!["-0.0".into(), " ".into(), String::new()],
                vec!["2".into(), "α,β".into(), "plain".into()],
            ],
        };
        let written = write_csv(&table);
        assert_eq!(
            written,
            concat!(
                "time [s],\"note,\"\"lab\"\"\",observer\n",
                "0.125,\"Δ\"\"値\"\"\r\n🙂\",O'Reilly\n",
                "-0.0,\" \",\"\"\n",
                "2,\"α,β\",plain\n",
            )
        );
        assert_eq!(parse_csv(&written), Ok(table));
    }

    #[test]
    fn json_writing_keeps_all_ascii_control_escapes_and_adjacent_utf8() {
        let controls: String = (0_u8..32).map(char::from).collect();
        let source = Json::Object(vec![(
            "key'\"🙂".into(),
            Json::String(format!("α/値'\"\\{controls}🙂\u{2028}")),
        )]);
        let written = write_json(&source);
        assert_eq!(
            written,
            concat!(
                r#"{"key'\"🙂":"α/値'\"\\"#,
                r#"\u0000\u0001\u0002\u0003\u0004\u0005\u0006\u0007\u0008\t\n\u000b\u000c\r\u000e\u000f"#,
                r#"\u0010\u0011\u0012\u0013\u0014\u0015\u0016\u0017\u0018\u0019\u001a\u001b\u001c\u001d\u001e\u001f"#,
                "🙂\u{2028}\"}",
            )
        );
        assert_eq!(parse_json(&written), Ok(source));
    }

    #[test]
    fn csv_headers_carry_units() {
        let table = parse_csv("time [s],velocity [m/s]\n0,1.5\n1,3.0\n").unwrap();
        assert_eq!(table.columns, vec!["time", "velocity"]);
        assert_eq!(table.units[0], Some("s".to_string()));
        assert_eq!(table.units[1], Some("m/s".to_string()));
        assert_eq!(table.len(), 2);
        assert_eq!(table.numeric_column("velocity").unwrap(), vec![1.5, 3.0]);
    }

    #[test]
    fn csv_round_trips_through_the_writer() {
        let text = "a,b\n1,2\n3,4\n";
        let table = parse_csv(text).unwrap();
        let written = write_csv(&table);
        let again = parse_csv(&written).unwrap();
        assert_eq!(table, again);
    }

    #[test]
    fn a_quoted_cell_may_contain_a_comma() {
        let table = parse_csv("name,value\n\"a,b\",1\n").unwrap();
        assert_eq!(table.rows[0][0], "a,b");
    }

    #[test]
    fn csv_preserves_quoted_newlines_and_doubled_quotes() {
        let input = "\u{feff}\"label, id\",\"length [m]\"\r\n\" a\"\"b\r\nc \",2\r\n";
        let table = parse_csv(input).unwrap();
        assert_eq!(table.columns, ["label, id", "length"]);
        assert_eq!(table.units[1].as_deref(), Some("m"));
        assert_eq!(table.rows[0][0], " a\"b\r\nc ");
        assert_eq!(parse_csv(&write_csv(&table)).unwrap(), table);
        assert!(parse_csv("x\n\"unterminated\n").is_err());
    }

    #[test]
    fn tsv_distinguishes_tabs_from_commas_and_preserves_quoted_tabs() {
        let text = "label\tlength [m]\tcomment\r\n\"x,y\t z\"\t2\t\r\n";
        let table = parse_tsv(text).unwrap();
        assert_eq!(table.columns, ["label", "length", "comment"]);
        assert_eq!(table.numeric_column("length").unwrap(), [2.0]);
        assert_eq!(table.column("label").unwrap(), ["x,y\t z"]);
        assert_eq!(table.column("comment").unwrap(), [""]);
    }

    #[test]
    fn simple_and_quoted_records_share_whitespace_and_newline_rules() {
        let csv = concat!(
            "\u{feff}\u{feff}\u{2003}\r\nid [m],label,last\r\n",
            "  1  , \u{2003}alpha\u{202f}, \r\n",
            "\u{2002}\r",
            "\u{2003}\"2\"\u{202f},\" keep ,\n spacing \",z\r",
            ",,\n",
            "3,tail,x"
        );
        let table = parse_csv(csv).unwrap();
        assert_eq!(table.columns, ["id", "label", "last"]);
        assert_eq!(table.units, [Some("m".to_owned()), None, None]);
        assert_eq!(
            table.rows,
            [
                ["1", "alpha", ""],
                ["2", " keep ,\n spacing ", "z"],
                ["", "", ""],
                ["3", "tail", "x"],
            ]
        );

        let tsv = concat!(
            "\u{2003}\r\nid [s]\tlabel\tlast\r",
            "\t\t\r\n",
            " 2 \t\"a\tb\"\u{202f}\t z"
        );
        let table = parse_tsv(tsv).unwrap();
        assert_eq!(table.columns, ["id", "label", "last"]);
        assert_eq!(table.units, [Some("s".to_owned()), None, None]);
        assert_eq!(table.rows, [["", "", ""], ["2", "a\tb", "z"]]);
    }

    #[test]
    fn a_quoted_record_reports_lexical_errors_before_its_width() {
        for (field, expected) in [
            ("x\"y", "unexpected quote in CSV field"),
            ("\"x\"y", "unexpected text after quoted CSV field"),
            ("\"x\r\ny", "unterminated quoted CSV field"),
        ] {
            let text = format!("left,right\r\n1,2\r\n{field}");
            assert_eq!(parse_csv(&text), Err(NumericError::Domain(expected.into())));
        }
    }

    #[test]
    fn a_ragged_csv_is_refused() {
        assert!(matches!(
            parse_csv("a,b\n1\n"),
            Err(NumericError::ShapeMismatch(_))
        ));
    }

    #[test]
    fn json_parses_every_value_kind() {
        let value = parse_json(r#"{"a": [1, 2.5, true, null], "b": "x\ny"}"#).unwrap();
        let array = match value.get("a") {
            | Some(Json::Array(items)) => items.clone(),
            | other => panic!("expected an array, found {other:?}"),
        };
        assert_eq!(array[0], Json::Number(1.0));
        assert_eq!(array[1], Json::Number(2.5));
        assert_eq!(array[2], Json::Bool(true));
        assert_eq!(array[3], Json::Null);
        assert_eq!(value.get("b"), Some(&Json::String("x\ny".to_string())));
    }

    #[test]
    fn json_round_trips() {
        let text = r#"{"a":[1,2,3],"b":{"c":"d"}}"#;
        let value = parse_json(text).unwrap();
        assert_eq!(write_json(&value), text);
    }

    #[test]
    fn malformed_json_is_reported() {
        assert!(parse_json("{").is_err());
        assert!(parse_json("[1,]").is_err());
        assert!(parse_json("nul").is_err());
    }

    #[test]
    fn json_handles_unicode_surrogates_and_reports_byte_offsets() {
        let value = parse_json(r#"{"e":"\uD83D\uDE00"}"#).unwrap();
        assert_eq!(value.get("e"), Some(&Json::String("😀".into())));
        assert_eq!(write_json(&value), r#"{"e":"😀"}"#);
        assert!(parse_json(r#""\uD83D\u0041""#).is_err());
        assert!(
            parse_json("\"é\" ?")
                .unwrap_err()
                .to_string()
                .contains("offset 5")
        );
    }

    #[test]
    fn json_string_fields_keep_order_empty_strings_and_unicode() {
        let value = parse_json(
            r#"{"":"", "é😀":"\uD83D\uDE00\/\\\"\u0000\b\f\n\r\t","é😀":"👩‍💻─é","numbers":[-0,5e-324]}"#,
        )
        .expect("valid JSON");
        let Json::Object(fields) = &value else {
            panic!("expected an object");
        };
        assert_eq!(fields[0], (String::new(), Json::String(String::new())));
        assert_eq!(
            fields[1],
            (
                "é😀".into(),
                Json::String("😀/\\\"\0\u{8}\u{c}\n\r\t".into())
            ),
        );
        assert_eq!(fields[2], ("é😀".into(), Json::String("👩‍💻─é".into())));
        let Json::Array(numbers) = &fields[3].1 else {
            panic!("expected an array");
        };
        let [Json::Number(zero), Json::Number(subnormal)] = numbers.as_slice() else {
            panic!("expected two numbers");
        };
        assert_eq!(zero.to_bits(), (-0.0f64).to_bits());
        assert_eq!(subnormal.to_bits(), 1);
        assert_eq!(parse_json(&write_json(&value)).expect("round trip"), value);
    }

    #[test]
    fn json_string_errors_keep_the_original_byte_offsets() {
        for (input, reason, offset) in [
            ("\"é😀\u{1f}\"", "unescaped control character", 7),
            ("\"é😀", "unterminated JSON string", 1),
            ("\"é\\n😀", "unterminated JSON string", 5),
            ("\"é😀\\β\"", "invalid string escape", 8),
            (r#""é😀\q""#, "invalid string escape", 9),
            (r#""é\ud800""#, "invalid UTF-16 surrogate pair", 9),
            (r#""é\ud800\u00q1""#, "invalid Unicode escape", 11),
        ] {
            assert_eq!(
                parse_json(input),
                Err(NumericError::Domain(format!("{reason} at offset {offset}"))),
                "{input:?}",
            );
        }
    }

    #[test]
    fn json_checks_number_syntax_and_nesting() {
        for invalid in ["01", "1.", "-.1", "1e+", "1e999", "\u{a0}0", "\"\u{1f}\""] {
            assert!(parse_json(invalid).is_err(), "accepted {invalid:?}");
        }
        let nested = |depth: usize| format!("{}\"猫\"{}", "[".repeat(depth), "]".repeat(depth));
        assert!(parse_json(&nested(128)).is_ok());
        assert_eq!(
            parse_json(&nested(129)),
            Err(NumericError::Domain(
                "JSON nesting exceeds 128 levels at offset 128".into()
            )),
        );
        assert_eq!(write_json(&Json::Number(f64::INFINITY)), "null");
    }

    #[test]
    fn npy_round_trips_a_two_by_three_array() {
        let shape = vec![2, 3];
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.5, -6.0];
        let bytes = write_npy(&shape, &values);
        // The prologue is aligned to 64 bytes.
        let header_len = usize::from(bytes[8]) | (usize::from(bytes[9]) << 8);
        assert_eq!((10 + header_len) % 64, 0);
        let (read_shape, read_values) = read_npy(&bytes).unwrap();
        assert_eq!(read_shape, shape);
        assert_eq!(read_values, values);
    }

    #[test]
    fn a_foreign_file_is_not_mistaken_for_npy() {
        assert!(read_npy(b"not an array").is_err());
    }
}
