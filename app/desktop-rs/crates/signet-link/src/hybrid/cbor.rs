//! Minimal CBOR (RFC 8949) as CTAP2 uses it. The encoder writes the canonical form (shortest-form lengths, map keys sorted).
//! The decoder is strict like Chromium's components/cbor reader: definite lengths only, shortest-form integers and lengths,
//! no tags, no floats, no duplicate map keys, no trailing bytes.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cbor {
    /// Covers the whole CBOR integer range (-2^64 .. 2^64-1).
    Int(i128),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Cbor>),
    /// Entries in insertion order (decoded: wire order). The encoder sorts them.
    Map(Vec<(Cbor, Cbor)>),
    Bool(bool),
    Null,
    Undefined,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CborError(pub &'static str);

impl std::fmt::Display for CborError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for CborError {}

impl Cbor {
    pub fn int(n: impl Into<i128>) -> Cbor {
        Cbor::Int(n.into())
    }
    pub fn text(s: &str) -> Cbor {
        Cbor::Text(s.to_string())
    }
    pub fn bytes(b: &[u8]) -> Cbor {
        Cbor::Bytes(b.to_vec())
    }
    pub fn map(entries: Vec<(Cbor, Cbor)>) -> Cbor {
        Cbor::Map(entries)
    }

    pub fn as_map(&self) -> Option<&Vec<(Cbor, Cbor)>> {
        if let Cbor::Map(m) = self { Some(m) } else { None }
    }
    pub fn as_array(&self) -> Option<&Vec<Cbor>> {
        if let Cbor::Array(a) = self { Some(a) } else { None }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        if let Cbor::Bytes(b) = self { Some(b) } else { None }
    }
    pub fn as_text(&self) -> Option<&str> {
        if let Cbor::Text(t) = self { Some(t) } else { None }
    }
    pub fn as_int(&self) -> Option<i128> {
        if let Cbor::Int(i) = self { Some(*i) } else { None }
    }
    /// Value of a map entry by key.
    pub fn get(&self, key: &Cbor) -> Option<&Cbor> {
        self.as_map()?.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    /// Value of a map entry by integer key.
    pub fn geti(&self, key: i64) -> Option<&Cbor> {
        self.get(&Cbor::Int(key as i128))
    }
    /// Value of a map entry by text key.
    pub fn gets(&self, key: &str) -> Option<&Cbor> {
        self.get(&Cbor::Text(key.to_string()))
    }
}

fn head(major: u8, value: u64, out: &mut Vec<u8>) {
    let m = major << 5;
    if value < 24 {
        out.push(m | value as u8);
    } else if value < 0x100 {
        out.extend_from_slice(&[m | 24, value as u8]);
    } else if value < 0x1_0000 {
        out.push(m | 25);
        out.extend_from_slice(&(value as u16).to_be_bytes());
    } else if value < 0x1_0000_0000 {
        out.push(m | 26);
        out.extend_from_slice(&(value as u32).to_be_bytes());
    } else {
        out.push(m | 27);
        out.extend_from_slice(&value.to_be_bytes());
    }
}

/// CTAP2 canonical order: shorter encoded keys first, then bytewise.
fn compare_keys(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

fn encode_into(value: &Cbor, out: &mut Vec<u8>) -> Result<(), CborError> {
    match value {
        Cbor::Null => out.push(0xf6),
        Cbor::Undefined => out.push(0xf7),
        Cbor::Bool(true) => out.push(0xf5),
        Cbor::Bool(false) => out.push(0xf4),
        Cbor::Int(n) => {
            if *n >= 0 {
                let v = u64::try_from(*n).map_err(|_| CborError("CBOR: integer out of range"))?;
                head(0, v, out);
            } else {
                let v = u64::try_from(-1 - *n).map_err(|_| CborError("CBOR: integer out of range"))?;
                head(1, v, out);
            }
        }
        Cbor::Text(s) => {
            head(3, s.len() as u64, out);
            out.extend_from_slice(s.as_bytes());
        }
        Cbor::Bytes(b) => {
            head(2, b.len() as u64, out);
            out.extend_from_slice(b);
        }
        Cbor::Array(a) => {
            head(4, a.len() as u64, out);
            for v in a {
                encode_into(v, out)?;
            }
        }
        Cbor::Map(m) => {
            let mut entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(m.len());
            for (k, v) in m {
                let (mut ek, mut ev) = (vec![], vec![]);
                encode_into(k, &mut ek)?;
                encode_into(v, &mut ev)?;
                entries.push((ek, ev));
            }
            entries.sort_by(|a, b| compare_keys(&a.0, &b.0));
            if entries.windows(2).any(|w| w[0].0 == w[1].0) {
                return Err(CborError("CBOR: duplicate map key"));
            }
            head(5, entries.len() as u64, out);
            for (k, v) in entries {
                out.extend_from_slice(&k);
                out.extend_from_slice(&v);
            }
        }
    }
    Ok(())
}

pub fn encode_cbor(value: &Cbor) -> Result<Vec<u8>, CborError> {
    let mut out = vec![];
    encode_into(value, &mut out)?;
    Ok(out)
}

/// Nesting limit (arrays and maps).
const MAX_DEPTH: usize = 16;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn left(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], CborError> {
        if n > self.left() {
            return Err(CborError("CBOR: truncated"));
        }
        let out = &self.bytes[self.at..self.at + n];
        self.at += n;
        Ok(out)
    }

    /// The argument of an initial byte; rejects indefinite lengths and non-shortest forms.
    fn argument(&mut self, info: u8) -> Result<u64, CborError> {
        if info < 24 {
            return Ok(info as u64);
        }
        if info > 27 {
            return Err(CborError("CBOR: indefinite length or reserved value"));
        }
        let size = 1usize << (info - 24);
        let b = self.take(size)?;
        let v = b.iter().fold(0u64, |acc, x| (acc << 8) | *x as u64);
        let min: u64 = match info {
            24 => 24,
            25 => 0x100,
            26 => 0x1_0000,
            _ => 0x1_0000_0000,
        };
        if v < min {
            return Err(CborError("CBOR: not the shortest form"));
        }
        Ok(v)
    }

    /// A length that must fit in what is left (`unit`: minimum bytes of each element).
    fn length(&mut self, info: u8, unit: u64) -> Result<usize, CborError> {
        let n = self.argument(info)?;
        if (n as u128) * (unit as u128) > self.left() as u128 {
            return Err(CborError("CBOR: length beyond the input"));
        }
        Ok(n as usize)
    }

    fn value(&mut self, depth: usize) -> Result<Cbor, CborError> {
        let initial = self.take(1)?[0];
        let major = initial >> 5;
        let info = initial & 31;
        match major {
            0 => Ok(Cbor::Int(self.argument(info)? as i128)),
            1 => Ok(Cbor::Int(-1 - self.argument(info)? as i128)),
            2 => {
                let n = self.length(info, 1)?;
                Ok(Cbor::Bytes(self.take(n)?.to_vec()))
            }
            3 => {
                let n = self.length(info, 1)?;
                let s = std::str::from_utf8(self.take(n)?).map_err(|_| CborError("CBOR: invalid UTF-8"))?;
                Ok(Cbor::Text(s.to_string()))
            }
            4 => {
                if depth >= MAX_DEPTH {
                    return Err(CborError("CBOR: too deeply nested"));
                }
                let n = self.length(info, 1)?;
                let mut out = Vec::with_capacity(n.min(1024));
                for _ in 0..n {
                    out.push(self.value(depth + 1)?);
                }
                Ok(Cbor::Array(out))
            }
            5 => {
                if depth >= MAX_DEPTH {
                    return Err(CborError("CBOR: too deeply nested"));
                }
                let n = self.length(info, 2)?;
                let mut out: Vec<(Cbor, Cbor)> = Vec::with_capacity(n.min(1024));
                for _ in 0..n {
                    let key = self.value(depth + 1)?;
                    if !matches!(key, Cbor::Int(_) | Cbor::Text(_)) {
                        return Err(CborError("CBOR: unsupported map key"));
                    }
                    if out.iter().any(|(k, _)| *k == key) {
                        return Err(CborError("CBOR: duplicate map key"));
                    }
                    let v = self.value(depth + 1)?;
                    out.push((key, v));
                }
                Ok(Cbor::Map(out))
            }
            6 => Err(CborError("CBOR: tags are not supported")),
            _ => match info {
                20 => Ok(Cbor::Bool(false)),
                21 => Ok(Cbor::Bool(true)),
                22 => Ok(Cbor::Null),
                23 => Ok(Cbor::Undefined),
                _ => Err(CborError("CBOR: unsupported simple value or float")),
            },
        }
    }
}

/// Decodes the first value of `input` and says how many bytes it took (COSE keys sit in front of other data).
pub fn decode_cbor_prefix(input: &[u8]) -> Result<(Cbor, usize), CborError> {
    let mut r = Reader { bytes: input, at: 0 };
    let v = r.value(0)?;
    Ok((v, r.at))
}

/// Decodes exactly one value; errors on malformed input or trailing bytes.
pub fn decode_cbor(input: &[u8]) -> Result<Cbor, CborError> {
    let (v, len) = decode_cbor_prefix(input)?;
    if len != input.len() {
        return Err(CborError("CBOR: trailing bytes"));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hx(v: &Cbor) -> String {
        hex::encode(encode_cbor(v).unwrap())
    }
    fn dec(h: &str) -> Result<Cbor, CborError> {
        decode_cbor(&hex::decode(h).unwrap())
    }
    fn i(n: i128) -> Cbor {
        Cbor::Int(n)
    }

    #[test]
    fn encodes_integers() {
        for (v, e) in [
            (0i128, "00"),
            (23, "17"),
            (24, "1818"),
            (255, "18ff"),
            (256, "190100"),
            (65535, "19ffff"),
            (65536, "1a00010000"),
            (1 << 32, "1b0000000100000000"),
            (0xffff_ffff_ffff_ffff, "1bffffffffffffffff"),
            (-1, "20"),
            (-24, "37"),
            (-25, "3818"),
            (-256, "38ff"),
            (-257, "390100"),
        ] {
            assert_eq!(hx(&i(v)), e, "{v}");
        }
    }

    #[test]
    fn encodes_byte_and_text_strings() {
        assert_eq!(hx(&Cbor::bytes(&[])), "40");
        assert_eq!(hx(&Cbor::bytes(&[1, 2, 3])), "43010203");
        assert_eq!(hx(&Cbor::bytes(&[0; 24])), format!("5818{}", "00".repeat(24)));
        assert_eq!(hx(&Cbor::bytes(&[0; 256])), format!("590100{}", "00".repeat(256)));
        assert_eq!(hx(&Cbor::text("")), "60");
        assert_eq!(hx(&Cbor::text("ga")), "626761");
        assert_eq!(hx(&Cbor::text("é")), "62c3a9");
        assert_eq!(hx(&Cbor::text(&"a".repeat(24))), format!("7818{}", "61".repeat(24)));
        assert_eq!(hx(&Cbor::text(&"a".repeat(300))), format!("79012c{}", "61".repeat(300)));
    }

    #[test]
    fn encodes_arrays_and_sorts_map_keys() {
        assert_eq!(hx(&Cbor::Array(vec![])), "80");
        assert_eq!(hx(&Cbor::Array(vec![i(1), Cbor::Array(vec![i(2), i(3)])])), "8201820203");
        let m = Cbor::map(vec![(i(2), Cbor::text("b")), (i(0), Cbor::text("a")), (i(1), Cbor::Bool(true))]);
        assert_eq!(hx(&m), format!("a3{}{}{}{}{}{}", "00", "6161", "01", "f5", "02", "6162"));
        assert_eq!(hx(&Cbor::map(vec![(i(24), i(1)), (i(23), i(2))])), "a2170218180".to_string() + "1");
        assert_eq!(hx(&Cbor::map(vec![])), "a0");
    }

    #[test]
    fn rejects_duplicate_keys_and_unencodable_values() {
        assert!(encode_cbor(&Cbor::map(vec![(i(1), i(1)), (i(1), i(2))])).is_err());
        assert!(encode_cbor(&i(1i128 << 64)).is_err());
        assert!(encode_cbor(&i(-(1i128 << 64) - 1)).is_err());
        assert_eq!(hx(&Cbor::Bool(false)), "f4");
        assert_eq!(hx(&Cbor::Null), "f6");
    }

    #[test]
    fn decodes_simple_values() {
        for (h, v) in [
            ("00", i(0)),
            ("17", i(23)),
            ("1818", i(24)),
            ("1903e8", i(1000)),
            ("1a000f4240", i(1_000_000)),
            ("20", i(-1)),
            ("3863", i(-100)),
            ("3903e7", i(-1000)),
            ("1b0000000100000000", i(1 << 32)),
            ("1bffffffffffffffff", i(0xffff_ffff_ffff_ffff)),
            ("3bffffffffffffffff", i(-(1i128 << 64))),
            ("f4", Cbor::Bool(false)),
            ("f5", Cbor::Bool(true)),
            ("f6", Cbor::Null),
            ("f7", Cbor::Undefined),
        ] {
            assert_eq!(dec(h).unwrap(), v, "{h}");
        }
    }

    #[test]
    fn decodes_strings_arrays_and_maps() {
        assert_eq!(dec("40").unwrap(), Cbor::bytes(&[]));
        assert_eq!(dec("4401020304").unwrap(), Cbor::bytes(&[1, 2, 3, 4]));
        assert_eq!(dec("60").unwrap(), Cbor::text(""));
        assert_eq!(dec("62c3a9").unwrap(), Cbor::text("é"));
        assert_eq!(dec("83010203").unwrap(), Cbor::Array(vec![i(1), i(2), i(3)]));
        assert_eq!(dec("a0").unwrap(), Cbor::Map(vec![]));
        let m = dec("a22001020 3".replace(' ', "").as_str()).unwrap();
        assert_eq!(m.geti(-1), Some(&i(1)));
        assert_eq!(m.geti(2), Some(&i(3)));
        let t = dec("a2616101626263 02".replace(' ', "").as_str()).unwrap();
        assert_eq!(t.gets("a"), Some(&i(1)));
        assert_eq!(t.gets("bc"), Some(&i(2)));
    }

    #[test]
    fn round_trips_with_the_encoder() {
        let value = Cbor::map(vec![
            (i(1), Cbor::bytes(&[1, 2])),
            (i(2), Cbor::map(vec![(Cbor::text("rk"), Cbor::Bool(true)), (Cbor::text("x"), Cbor::Array(vec![i(1), i(-1), Cbor::text("é"), Cbor::Null]))])),
            (i(-3), i(0xffff_ffff_ffff_ffff)),
            (Cbor::text("fmt"), Cbor::text("none")),
        ]);
        let back = decode_cbor(&encode_cbor(&value).unwrap()).unwrap();
        // Canonical encoding is the same whatever the entry order.
        assert_eq!(encode_cbor(&back).unwrap(), encode_cbor(&value).unwrap());
    }

    #[test]
    fn decodes_a_prefix_and_says_how_long_it_was() {
        let (v, len) = decode_cbor_prefix(&hex::decode("a10102ff").unwrap()).unwrap();
        assert_eq!(v, Cbor::map(vec![(i(1), i(2))]));
        assert_eq!(len, 3);
    }

    #[test]
    fn rejects_malformed_input() {
        for (name, input) in [
            ("empty input", ""),
            ("truncated integer", "19ff"),
            ("truncated byte string", "4401"),
            ("byte string longer than the input", "5b00000000ffffffff"),
            ("array longer than the input", "9b00000000ffffffff"),
            ("indefinite byte string", "5f4101ff"),
            ("indefinite array", "9f01ff"),
            ("indefinite map", "bf0102ff"),
            ("reserved additional information", "1c"),
            ("non-shortest integer", "1800"),
            ("non-shortest 16-bit integer", "190017"),
            ("non-shortest length", "780161"),
            ("tag", "c101"),
            ("float 16", "f93c00"),
            ("float 64", "fb3ff0000000000000"),
            ("simple value 16", "f0"),
            ("simple value in the next byte", "f820"),
            ("break outside of anything", "ff"),
            ("invalid UTF-8", "62c328"),
            ("duplicate map keys", "a201010102"),
            ("duplicate text map keys", "a2616101616102"),
            ("byte string map key", "a1410101"),
            ("array map key", "a1810101"),
            ("trailing bytes", "0000"),
            ("map missing a value", "a101"),
            ("huge array", "9a7fffffff"),
            ("huge map", "ba7fffffff"),
        ] {
            assert!(dec(input).is_err(), "{name}");
        }
    }

    #[test]
    fn limits_nesting_to_16() {
        assert!(dec(&format!("{}00", "81".repeat(16))).is_ok());
        assert!(dec(&format!("{}00", "81".repeat(17))).is_err());
    }
}
