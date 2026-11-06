//! Validated NumPy float-array codec shared by the language and Rust API.

const NPY_MAGIC: &[u8] = b"\x93NUMPY";

/// Check that the element count and row-major suffix strides fit `usize`.
/// Empty arrays still require representable strides for downstream tensor views.
#[must_use]
pub fn checked_element_count(shape: &[usize]) -> Option<usize> {
    let count = shape
        .iter()
        .try_fold(1usize, |n, extent| n.checked_mul(*extent))?;
    shape
        .iter()
        .rev()
        .try_fold(1usize, |n, extent| n.checked_mul(*extent))?;
    Some(count)
}

/// Encode a dimensionless row-major Float array as NPY 1.0.
///
/// # Errors
/// Rejects inconsistent or unrepresentable shape and header sizes.
pub fn encode(
    shape: &[usize],
    values: &[f64],
) -> Result<Vec<u8>, String> {
    let count = checked_element_count(shape)
        .ok_or_else(|| "E0902: NPY tensor shape or strides overflow".to_string())?;
    if values.len() != count {
        return Err("E0902: NPY tensor storage does not match its shape".to_string());
    }
    let shape = if shape.is_empty() {
        "()".to_string()
    } else {
        let rank = shape.len();
        let mut shape = shape
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        if rank == 1 {
            shape.push(',');
        }
        format!("({shape})")
    };
    let mut header = format!("{{'descr': '<f8', 'fortran_order': False, 'shape': {shape}, }}");
    // magic(6) + version(2) + header length(2) + header must be a multiple of 64.
    let padding = (64 - ((10 + header.len() + 1) % 64)) % 64;
    header.push_str(&" ".repeat(padding));
    header.push('\n');
    let header_len = u16::try_from(header.len())
        .map_err(|_| "E0903: the NPY header is too large".to_string())?;
    let mut bytes = Vec::with_capacity(10 + header.len() + values.len() * 8);
    bytes.extend_from_slice(NPY_MAGIC);
    bytes.push(1); // major
    bytes.push(0); // minor
    bytes.extend_from_slice(&header_len.to_le_bytes());
    bytes.extend_from_slice(header.as_bytes());
    for value in values.iter() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    Ok(bytes)
}

// NPY uses a Python-literal dictionary, not JSON. Parse the supported
// primitive header grammar directly; never execute or substring-search it.
struct NpyHeader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> NpyHeader<'a> {
    fn invalid() -> String {
        "E0903: malformed NPY header dictionary".to_string()
    }

    fn ws(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn take(
        &mut self,
        byte: u8,
    ) -> bool {
        self.ws();
        if self.bytes.get(self.at) == Some(&byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn string(&mut self) -> Result<&'a str, String> {
        self.ws();
        let quote = *self.bytes.get(self.at).ok_or_else(Self::invalid)?;
        if quote != b'\'' && quote != b'"' {
            return Err(Self::invalid());
        }
        self.at += 1;
        let start = self.at;
        while let Some(&byte) = self.bytes.get(self.at) {
            if byte == quote {
                let text = std::str::from_utf8(&self.bytes[start..self.at])
                    .map_err(|_| Self::invalid())?;
                self.at += 1;
                return Ok(text);
            }
            if byte == b'\\' || byte.is_ascii_control() {
                return Err(Self::invalid());
            }
            self.at += 1;
        }
        Err(Self::invalid())
    }

    fn boolean(&mut self) -> Result<bool, String> {
        self.ws();
        if self.bytes[self.at..].starts_with(b"True") {
            self.at += 4;
            Ok(true)
        } else if self.bytes[self.at..].starts_with(b"False") {
            self.at += 5;
            Ok(false)
        } else {
            Err(Self::invalid())
        }
    }

    fn shape(&mut self) -> Result<Vec<usize>, String> {
        if !self.take(b'(') {
            return Err(Self::invalid());
        }
        let mut shape = Vec::new();
        if self.take(b')') {
            return Ok(shape);
        }
        loop {
            self.ws();
            let start = self.at;
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
            }
            if start == self.at {
                return Err(Self::invalid());
            }
            let extent = std::str::from_utf8(&self.bytes[start..self.at])
                .map_err(|_| Self::invalid())?
                .parse::<usize>()
                .map_err(|_| "E0903: NPY shape entry overflows".to_string())?;
            shape.push(extent);
            if self.take(b',') {
                if self.take(b')') {
                    return Ok(shape);
                }
            } else if self.take(b')') && shape.len() > 1 {
                return Ok(shape);
            } else {
                return Err(Self::invalid());
            }
        }
    }

    fn parse(header: &'a str) -> Result<(String, bool, Vec<usize>), String> {
        let mut parser = Self {
            bytes: header.as_bytes(),
            at: 0,
        };
        if !parser.take(b'{') {
            return Err(Self::invalid());
        }
        let (mut descr, mut order, mut shape) = (None, None, None);
        if !parser.take(b'}') {
            loop {
                let key = parser.string()?;
                if !parser.take(b':') {
                    return Err(Self::invalid());
                }
                match key {
                    | "descr" if descr.is_none() => descr = Some(parser.string()?.to_string()),
                    | "fortran_order" if order.is_none() => order = Some(parser.boolean()?),
                    | "shape" if shape.is_none() => shape = Some(parser.shape()?),
                    | _ => return Err(Self::invalid()),
                }
                if parser.take(b',') {
                    if parser.take(b'}') {
                        break;
                    }
                } else if parser.take(b'}') {
                    break;
                } else {
                    return Err(Self::invalid());
                }
            }
        }
        parser.ws();
        if parser.at != parser.bytes.len() {
            return Err(Self::invalid());
        }
        Ok((
            descr.ok_or_else(Self::invalid)?,
            order.ok_or_else(Self::invalid)?,
            shape.ok_or_else(Self::invalid)?,
        ))
    }
}

/// Decodes a `.npy` file (§8.11).
///
/// # Errors
///
/// `E0903` for malformed/truncated data, `E0904` for an unsupported dtype. The message carries the code, because the language-level
/// result is `Result<Tensor<…>, DataError>`.
pub fn decode(bytes: &[u8]) -> Result<(Vec<usize>, Vec<f64>), String> {
    if bytes.len() < 10 || &bytes[..6] != NPY_MAGIC {
        return Err("E0903: not a NumPy `.npy` file (bad magic)".to_string());
    }
    let major = bytes[6];
    if bytes[7] != 0 {
        return Err(format!(
            "E0904: unsupported NPY version {major}.{}",
            bytes[7]
        ));
    }
    let header_len = match major {
        | 1 => usize::from(u16::from_le_bytes([bytes[8], bytes[9]])),
        | 2 | 3 => {
            if bytes.len() < 12 {
                return Err("E0903: truncated NPY header".to_string());
            }
            usize::try_from(u32::from_le_bytes([
                bytes[8], bytes[9], bytes[10], bytes[11],
            ]))
            .unwrap_or(0)
        },
        | _ => {
            return Err(format!(
                "E0904: unsupported NPY version {major}.{}",
                bytes[7]
            ));
        },
    };
    if header_len > 10 * 1024 {
        return Err("E0903: the NPY header exceeds 10 KiB".to_string());
    }
    let start = if major == 1 { 10 } else { 12 };
    if bytes.len() < start + header_len {
        return Err("E0903: the NPY header is truncated".to_string());
    }
    let header = std::str::from_utf8(&bytes[start..start + header_len])
        .map_err(|_| "E0903: the NPY header is not UTF-8".to_string())?;

    let (descr, fortran, shape) = NpyHeader::parse(header)?;
    let (width, big_endian) = match descr.as_str() {
        | "<f8" | "|f8" => (8usize, false),
        | "<f4" | "|f4" => (4usize, false),
        | ">f8" => (8usize, true),
        | ">f4" => (4usize, true),
        | "=f8" => (8usize, cfg!(target_endian = "big")),
        | "=f4" => (4usize, cfg!(target_endian = "big")),
        | _ => {
            return Err(format!(
                "E0904: unsupported NPY dtype `{descr}` (only float32/float64)"
            ));
        },
    };
    let count = checked_element_count(&shape)
        .ok_or_else(|| "E0903: NPY shape or strides overflow".to_string())?;
    let data = &bytes[start + header_len..];
    let expected = count
        .checked_mul(width)
        .ok_or_else(|| "E0903: NPY shape overflows".to_string())?;
    if data.len() != expected {
        return Err(format!(
            "E0903: NPY data is {} byte(s), the shape needs {expected}",
            data.len()
        ));
    }
    let decode = |chunk: &[u8]| -> f64 {
        if width == 8 {
            let bytes = <[u8; 8]>::try_from(chunk).expect("eight bytes");
            if big_endian {
                f64::from_be_bytes(bytes)
            } else {
                f64::from_le_bytes(bytes)
            }
        } else {
            let bytes = <[u8; 4]>::try_from(chunk).expect("four bytes");
            f64::from(if big_endian {
                f32::from_be_bytes(bytes)
            } else {
                f32::from_le_bytes(bytes)
            })
        }
    };
    let mut values = Vec::with_capacity(count);
    if fortran && shape.len() > 1 && count > 0 {
        let mut strides = Vec::with_capacity(shape.len());
        let mut stride = 1usize;
        for &extent in &shape {
            strides.push(stride);
            stride = stride
                .checked_mul(extent)
                .ok_or_else(|| "E0903: NPY shape overflows".to_string())?;
        }
        for index in 0..count {
            let mut remainder = index;
            let mut source = 0usize;
            for (&extent, &stride) in shape.iter().zip(&strides).rev() {
                let coordinate = remainder % extent;
                remainder /= extent;
                source += coordinate * stride;
            }
            values.push(decode(&data[source * width..(source + 1) * width]));
        }
    } else {
        values.extend(data.chunks_exact(width).map(decode));
    }
    Ok((shape, values))
}
