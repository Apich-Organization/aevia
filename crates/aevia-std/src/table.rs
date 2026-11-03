//! The data layer: CSV, JSON and NumPy files (§8.11).
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

/// A parsed CSV table.
#[derive(Clone, Debug, PartialEq)]
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
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// True when the table has no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// One column as text.
    ///
    /// # Errors
    ///
    /// [`NumericError::Domain`] when the name is not a column.
    pub fn column(&self, name: &str) -> Result<Vec<&str>, NumericError> {
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
    pub fn numeric_column(&self, name: &str) -> Result<Vec<f64>, NumericError> {
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

/// Parses CSV text with a header row.
///
/// # Errors
///
/// [`NumericError::Empty`] when there is no header, and
/// [`NumericError::ShapeMismatch`] when a row has a different width from the
/// header.
pub fn parse_csv(text: &str) -> Result<Table, NumericError> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header = lines.next().ok_or(NumericError::Empty)?;
    let mut columns = Vec::new();
    let mut units = Vec::new();
    for field in split_csv(header) {
        let (name, unit) = match field.find('[') {
            Some(open) if field.trim_end().ends_with(']') => {
                let name = field[..open].trim().to_string();
                let unit = field[open + 1..].trim_end_matches(']').trim().to_string();
                (name, if unit.is_empty() { None } else { Some(unit) })
            }
            _ => (field.trim().to_string(), None),
        };
        columns.push(name);
        units.push(unit);
    }
    let width = columns.len();
    let mut rows = Vec::new();
    for line in lines {
        let cells: Vec<String> = split_csv(line);
        if cells.len() != width {
            return Err(NumericError::ShapeMismatch(format!(
                "a row has {} cell(s), the header has {width}",
                cells.len()
            )));
        }
        rows.push(cells);
    }
    Ok(Table {
        columns,
        units,
        rows,
    })
}

/// Splits one CSV line, honouring double quotes.
fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in line.chars() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                out.push(current.trim().to_string());
                current = String::new();
            }
            other => current.push(other),
        }
    }
    out.push(current.trim().to_string());
    out
}

/// Serialises a table as CSV, with the unit back in the header.
#[must_use]
pub fn write_csv(table: &Table) -> String {
    let mut out = String::new();
    for (index, name) in table.columns.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(name);
        if let Some(unit) = table.units.get(index).and_then(Option::as_ref) {
            out.push_str(" [");
            out.push_str(unit);
            out.push(']');
        }
    }
    out.push('\n');
    for row in &table.rows {
        for (index, cell) in row.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            if cell.contains(',') || cell.contains('"') {
                out.push('"');
                out.push_str(&cell.replace('"', "\"\""));
                out.push('"');
            } else {
                out.push_str(cell);
            }
        }
        out.push('\n');
    }
    out
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
    Array(Vec<Json>),
    /// An object, in document order.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// The value under a key, when this is an object.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(fields) => fields
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }
}

/// Parses JSON text.
///
/// # Errors
///
/// [`NumericError::Domain`] with the byte offset of the first syntax error.
pub fn parse_json(text: &str) -> Result<Json, NumericError> {
    let bytes: Vec<char> = text.chars().collect();
    let mut position = 0usize;
    let value = parse_value(&bytes, &mut position)?;
    skip_space(&bytes, &mut position);
    if position != bytes.len() {
        return Err(NumericError::Domain(format!(
            "trailing text at offset {position}"
        )));
    }
    Ok(value)
}

fn skip_space(text: &[char], position: &mut usize) {
    while *position < text.len() && text[*position].is_whitespace() {
        *position += 1;
    }
}

fn parse_value(text: &[char], position: &mut usize) -> Result<Json, NumericError> {
    skip_space(text, position);
    let Some(character) = text.get(*position).copied() else {
        return Err(NumericError::Domain("unexpected end of input".to_string()));
    };
    match character {
        'n' => expect_word(text, position, "null").map(|()| Json::Null),
        't' => expect_word(text, position, "true").map(|()| Json::Bool(true)),
        'f' => expect_word(text, position, "false").map(|()| Json::Bool(false)),
        '"' => parse_string(text, position).map(Json::String),
        '[' => {
            *position += 1;
            let mut items = Vec::new();
            skip_space(text, position);
            if text.get(*position) == Some(&']') {
                *position += 1;
                return Ok(Json::Array(items));
            }
            loop {
                items.push(parse_value(text, position)?);
                skip_space(text, position);
                match text.get(*position) {
                    Some(',') => *position += 1,
                    Some(']') => {
                        *position += 1;
                        return Ok(Json::Array(items));
                    }
                    _ => {
                        return Err(NumericError::Domain(format!(
                            "expected `,` or `]` at offset {position}"
                        )))
                    }
                }
            }
        }
        '{' => {
            *position += 1;
            let mut fields = Vec::new();
            skip_space(text, position);
            if text.get(*position) == Some(&'}') {
                *position += 1;
                return Ok(Json::Object(fields));
            }
            loop {
                skip_space(text, position);
                let key = parse_string(text, position)?;
                skip_space(text, position);
                if text.get(*position) != Some(&':') {
                    return Err(NumericError::Domain(format!(
                        "expected `:` at offset {position}"
                    )));
                }
                *position += 1;
                let value = parse_value(text, position)?;
                fields.push((key, value));
                skip_space(text, position);
                match text.get(*position) {
                    Some(',') => *position += 1,
                    Some('}') => {
                        *position += 1;
                        return Ok(Json::Object(fields));
                    }
                    _ => {
                        return Err(NumericError::Domain(format!(
                            "expected `,` or `}}` at offset {position}"
                        )))
                    }
                }
            }
        }
        _ => parse_number(text, position),
    }
}

fn expect_word(text: &[char], position: &mut usize, word: &str) -> Result<(), NumericError> {
    for expected in word.chars() {
        if text.get(*position) != Some(&expected) {
            return Err(NumericError::Domain(format!(
                "expected `{word}` at offset {position}"
            )));
        }
        *position += 1;
    }
    Ok(())
}

fn parse_string(text: &[char], position: &mut usize) -> Result<String, NumericError> {
    if text.get(*position) != Some(&'"') {
        return Err(NumericError::Domain(format!(
            "expected a string at offset {position}"
        )));
    }
    *position += 1;
    let mut out = String::new();
    while let Some(character) = text.get(*position).copied() {
        *position += 1;
        match character {
            '"' => return Ok(out),
            '\\' => {
                let escape = text.get(*position).copied().ok_or_else(|| {
                    NumericError::Domain("unterminated escape".to_string())
                })?;
                *position += 1;
                match escape {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'u' => {
                        let mut code = 0u32;
                        for _ in 0..4 {
                            let digit = text.get(*position).copied().ok_or_else(|| {
                                NumericError::Domain("truncated \\u escape".to_string())
                            })?;
                            *position += 1;
                            code = code * 16
                                + digit.to_digit(16).ok_or_else(|| {
                                    NumericError::Domain("bad \\u escape".to_string())
                                })?;
                        }
                        out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                    }
                    other => {
                        return Err(NumericError::Domain(format!(
                            "unknown escape `\\{other}`"
                        )))
                    }
                }
            }
            other => out.push(other),
        }
    }
    Err(NumericError::Domain("unterminated string".to_string()))
}

fn parse_number(text: &[char], position: &mut usize) -> Result<Json, NumericError> {
    let start = *position;
    if text.get(*position) == Some(&'-') {
        *position += 1;
    }
    while let Some(character) = text.get(*position).copied() {
        if character.is_ascii_digit() || matches!(character, '.' | 'e' | 'E' | '+' | '-') {
            *position += 1;
        } else {
            break;
        }
    }
    let literal: String = text[start..*position].iter().collect();
    literal
        .parse::<f64>()
        .map(Json::Number)
        .map_err(|_| NumericError::Domain(format!("bad number `{literal}`")))
}

/// Serialises JSON, compactly.
#[must_use]
pub fn write_json(value: &Json) -> String {
    let mut out = String::new();
    write_json_into(value, &mut out);
    out
}

fn write_json_into(value: &Json, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Number(number) => out.push_str(&format_json_number(*number)),
        Json::String(text) => {
            out.push('"');
            for character in text.chars() {
                match character {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    other if (other as u32) < 0x20 => {
                        out.push_str(&format!("\\u{:04x}", other as u32));
                    }
                    other => out.push(other),
                }
            }
            out.push('"');
        }
        Json::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json_into(item, out);
            }
            out.push(']');
        }
        Json::Object(fields) => {
            out.push('{');
            for (index, (name, item)) in fields.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json_into(&Json::String(name.clone()), out);
                out.push(':');
                write_json_into(item, out);
            }
            out.push('}');
        }
    }
}

fn format_json_number(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// Reads a NumPy `.npy` array of `float64` or `float32` in C order.
///
/// # Errors
///
/// [`NumericError::Domain`] when the header is not a v1.0 NumPy header, names
/// another dtype, or describes a Fortran-ordered array.
pub fn read_npy(bytes: &[u8]) -> Result<(Vec<usize>, Vec<f64>), NumericError> {
    if bytes.len() < 10 || &bytes[0..6] != b"\x93NUMPY" {
        return Err(NumericError::Domain("not a `.npy` file".to_string()));
    }
    let major = bytes[6];
    if major != 1 {
        return Err(NumericError::Domain(format!(
            "`.npy` version {major}.{} is not supported",
            bytes[7]
        )));
    }
    let header_len = usize::from(bytes[8]) | (usize::from(bytes[9]) << 8);
    let start = 10;
    let end = start + header_len;
    if bytes.len() < end {
        return Err(NumericError::Domain("truncated `.npy` header".to_string()));
    }
    let header = std::str::from_utf8(&bytes[start..end])
        .map_err(|_| NumericError::Domain("the `.npy` header is not UTF-8".to_string()))?;
    if header.contains("'fortran_order': True") {
        return Err(NumericError::Domain(
            "a Fortran-ordered `.npy` array is not supported".to_string(),
        ));
    }
    let float32 = if header.contains("'<f8'") || header.contains("'|f8'") {
        false
    } else if header.contains("'<f4'") || header.contains("'|f4'") {
        true
    } else {
        return Err(NumericError::Domain(
            "only `float64` and `float32` arrays are supported".to_string(),
        ));
    };
    let shape = parse_npy_shape(header)?;
    let count: usize = shape.iter().product();
    let width = if float32 { 4 } else { 8 };
    let body = &bytes[end..];
    if body.len() < count * width {
        return Err(NumericError::Domain(format!(
            "the `.npy` body holds {} byte(s), {count} values need {}",
            body.len(),
            count * width
        )));
    }
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        let chunk = &body[index * width..(index + 1) * width];
        let value = if float32 {
            f64::from(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        } else {
            f64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ])
        };
        values.push(value);
    }
    Ok((shape, values))
}

fn parse_npy_shape(header: &str) -> Result<Vec<usize>, NumericError> {
    let open = header
        .find("'shape':")
        .map(|at| at + "'shape':".len())
        .ok_or_else(|| NumericError::Domain("the `.npy` header has no shape".to_string()))?;
    let rest = &header[open..];
    let start = rest
        .find('(')
        .ok_or_else(|| NumericError::Domain("malformed `.npy` shape".to_string()))?;
    let stop = rest
        .find(')')
        .ok_or_else(|| NumericError::Domain("malformed `.npy` shape".to_string()))?;
    let inner = &rest[start + 1..stop];
    let mut shape = Vec::new();
    for part in inner.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let value: usize = part
            .parse()
            .map_err(|_| NumericError::Domain(format!("bad `.npy` extent `{part}`")))?;
        shape.push(value);
    }
    if shape.is_empty() {
        shape.push(1);
    }
    Ok(shape)
}

/// Writes a NumPy v1.0 `.npy` array of `float64` in C order.
#[must_use]
pub fn write_npy(shape: &[usize], values: &[f64]) -> Vec<u8> {
    let shape_text = if shape.len() == 1 {
        format!("({},)", shape[0])
    } else {
        let parts: Vec<String> = shape.iter().map(usize::to_string).collect();
        format!("({})", parts.join(", "))
    };
    let mut header = format!("{{'descr': '<f8', 'fortran_order': False, 'shape': {shape_text}, }}");
    // The header plus the 10-byte prologue must be a multiple of 64 bytes and
    // end with a newline.
    let padding = 64 - ((10 + header.len() + 1) % 64);
    for _ in 0..padding {
        header.push(' ');
    }
    header.push('\n');
    let mut out = Vec::with_capacity(10 + header.len() + values.len() * 8);
    out.extend_from_slice(b"\x93NUMPY");
    out.push(1);
    out.push(0);
    let length = u16::try_from(header.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Some(Json::Array(items)) => items.clone(),
            other => panic!("expected an array, found {other:?}"),
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
