//! Bounds-checked little-endian reader/writer. Every read from network data goes
//! through `Reader`, which returns an error instead of panicking on bad input.

use std::fmt;

#[derive(Debug, PartialEq)]
pub enum WireError {
    Truncated,
    Trailing(usize),
    BadMagic,
    BadVersion(u8),
    BadTag(&'static str, u8),
    BadString,
    BadValue(&'static str),
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "truncated message"),
            Self::Trailing(n) => write!(f, "{n} trailing bytes"),
            Self::BadMagic => write!(f, "bad magic"),
            Self::BadVersion(v) => write!(f, "unsupported protocol version {v}"),
            Self::BadTag(what, t) => write!(f, "unknown {what} tag {t}"),
            Self::BadString => write!(f, "bad string"),
            Self::BadValue(what) => write!(f, "bad value: {what}"),
        }
    }
}

pub type Result<T> = std::result::Result<T, WireError>;

#[derive(Default)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }
    pub fn i8(&mut self, v: i8) -> &mut Self {
        self.u8(v as u8)
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }
    pub fn i16(&mut self, v: i16) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }
    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }
    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }
    pub fn bytes(&mut self, v: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(v);
        self
    }
    /// Length-prefixed (u8) UTF-8 string. Callers keep strings short (see `Reader::str`).
    pub fn str(&mut self, v: &str) -> &mut Self {
        let v = &v.as_bytes()[..v.len().min(255)];
        self.u8(v.len() as u8).bytes(v)
    }
}

pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(WireError::Truncated)?;
        let out = self.data.get(self.pos..end).ok_or(WireError::Truncated)?;
        self.pos = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.array()?))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    /// A finite f32 with |v| <= `limit`.
    pub fn f32(&mut self, limit: f32) -> Result<f32> {
        let v = f32::from_le_bytes(self.array()?);
        if v.is_finite() && v.abs() <= limit {
            Ok(v)
        } else {
            Err(WireError::BadValue("float out of range"))
        }
    }

    /// UTF-8 string of at most `max` bytes with no control characters.
    pub fn str(&mut self, max: usize) -> Result<String> {
        let len = self.u8()? as usize;
        if len > max {
            return Err(WireError::BadString);
        }
        let s = std::str::from_utf8(self.take(len)?).map_err(|_| WireError::BadString)?;
        if s.chars().any(char::is_control) {
            return Err(WireError::BadString);
        }
        Ok(s.to_string())
    }

    pub fn rest(&mut self) -> &'a [u8] {
        let out = &self.data[self.pos..];
        self.pos = self.data.len();
        out
    }

    /// Fails if any bytes are left unread.
    pub fn finish(&self) -> Result<()> {
        match self.data.len() - self.pos {
            0 => Ok(()),
            n => Err(WireError::Trailing(n)),
        }
    }
}
