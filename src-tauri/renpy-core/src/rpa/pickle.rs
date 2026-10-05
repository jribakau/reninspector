//! A pickle reader limited to what a Ren'Py archive index contains, and a
//! protocol-2 writer for the indexes we emit.
//!
//! Protocols 0 through 5 are accepted. The only global that may be called is
//! `_codecs.encode`, which is how Python 3 writes `bytes` at protocol 2.
//! Everything else (other globals, `REDUCE` of anything else, `BUILD`,
//! `INST`, `NEWOBJ`, persistent ids) is an error.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::format::RpaError;

const MAX_DEPTH: usize = 64;
const MAX_OBJECTS: usize = 5_000_000;
const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_STACK: usize = 100_000;
const MAX_MEMO: usize = 2_000_000;
const MAX_LONG: usize = 16;

const MARK: u8 = b'(';
const STOP: u8 = b'.';
const INT: u8 = b'I';
const BININT: u8 = b'J';
const BININT1: u8 = b'K';
const LONG: u8 = b'L';
const BININT2: u8 = b'M';
const NONE_OP: u8 = b'N';
const REDUCE: u8 = b'R';
const STRING: u8 = b'S';
const BINSTRING: u8 = b'T';
const SHORT_BINSTRING: u8 = b'U';
const UNICODE: u8 = b'V';
const BINUNICODE: u8 = b'X';
const APPEND: u8 = b'a';
const GLOBAL: u8 = b'c';
const DICT_OP: u8 = b'd';
const EMPTY_DICT: u8 = b'}';
const APPENDS: u8 = b'e';
const GET: u8 = b'g';
const BINGET: u8 = b'h';
const LIST_OP: u8 = b'l';
const EMPTY_LIST: u8 = b']';
const PUT: u8 = b'p';
const BINPUT: u8 = b'q';
const LONG_BINPUT: u8 = b'r';
const SETITEM: u8 = b's';
const TUPLE_OP: u8 = b't';
const EMPTY_TUPLE: u8 = b')';
const SETITEMS: u8 = b'u';
const PROTO: u8 = 0x80;
const TUPLE1: u8 = 0x85;
const TUPLE2: u8 = 0x86;
const TUPLE3: u8 = 0x87;
const NEWTRUE: u8 = 0x88;
const NEWFALSE: u8 = 0x89;
const LONG1: u8 = 0x8a;
const LONG4: u8 = 0x8b;
const BINBYTES: u8 = b'B';
const SHORT_BINBYTES: u8 = b'C';
const SHORT_BINUNICODE: u8 = 0x8c;
const BINUNICODE8: u8 = 0x8d;
const BINBYTES8: u8 = 0x8e;
const STACK_GLOBAL: u8 = 0x93;
const MEMOIZE: u8 = 0x94;
const FRAME: u8 = 0x95;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    None,
    Bool(bool),
    Int(i128),
    Bytes(Vec<u8>),
    Str(String),
    List(Vec<Value>),
    Tuple(Vec<Value>),
    Dict(Vec<(Value, Value)>),
}

/// One `(offset, length)` pair stored under a name. Offsets are already the
/// on-disk values the caller wants pickled (XOR them first for RPA-3.0).
pub struct IndexPair {
    pub offset: u64,
    pub len: u64,
}

/// Protocol-2 pickle of `{name: [(offset, len), ...]}`.
pub fn dump_index<S: AsRef<str>>(entries: &[(S, Vec<IndexPair>)]) -> Vec<u8> {
    let mut out = vec![PROTO, 2, EMPTY_DICT, MARK];
    for (name, pairs) in entries {
        write_binunicode(&mut out, name.as_ref());
        out.push(EMPTY_LIST);
        if !pairs.is_empty() {
            out.push(MARK);
            for pair in pairs {
                write_int(&mut out, pair.offset);
                write_int(&mut out, pair.len);
                out.push(TUPLE2);
            }
            out.push(APPENDS);
        }
    }
    out.push(SETITEMS);
    out.push(STOP);
    out
}

fn write_binunicode(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    out.push(BINUNICODE);
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}

fn write_int(out: &mut Vec<u8>, n: u64) {
    if n <= 0xff {
        out.push(BININT1);
        out.push(n as u8);
    } else if n <= 0xffff {
        out.push(BININT2);
        out.extend_from_slice(&(n as u16).to_le_bytes());
    } else if n <= i32::MAX as u64 {
        out.push(BININT);
        out.extend_from_slice(&(n as i32).to_le_bytes());
    } else {
        let bytes = long_bytes(n as i128);
        out.push(LONG1);
        out.push(bytes.len() as u8);
        out.extend_from_slice(&bytes);
    }
}

/// Minimal little-endian two's complement, with the sign bit in the last byte.
fn long_bytes(n: i128) -> Vec<u8> {
    if n == 0 {
        return vec![0];
    }
    let mut bytes = Vec::new();
    if n > 0 {
        let mut v = n;
        while v > 0 {
            bytes.push((v & 0xff) as u8);
            v >>= 8;
        }
        if bytes.last().copied().unwrap_or(0) & 0x80 != 0 {
            bytes.push(0);
        }
    } else {
        let mut v = n;
        loop {
            bytes.push((v & 0xff) as u8);
            v >>= 8;
            if v == -1 && bytes.last().copied().unwrap_or(0) & 0x80 != 0 {
                break;
            }
        }
    }
    bytes
}

#[derive(Clone)]
enum Val {
    None,
    Bool(bool),
    Int(i128),
    Bytes(Vec<u8>),
    Str(String),
    List(Rc<RefCell<Vec<Val>>>),
    Tuple(Vec<Val>),
    Dict(Rc<RefCell<Vec<(Val, Val)>>>),
    Callable { module: String, name: String },
}

struct Budget {
    objects: usize,
    bytes: usize,
}

impl Budget {
    fn obj(&mut self) -> Result<(), RpaError> {
        self.objects += 1;
        if self.objects > MAX_OBJECTS {
            return Err(RpaError::new("pickle has too many objects"));
        }
        Ok(())
    }

    fn add_bytes(&mut self, n: usize) -> Result<(), RpaError> {
        self.bytes = self.bytes.saturating_add(n);
        if self.bytes > MAX_BYTES {
            return Err(RpaError::new("pickle strings are too large"));
        }
        Ok(())
    }
}

struct Parser<'a> {
    data: &'a [u8],
    i: usize,
    stack: Vec<StackItem>,
    memo: HashMap<usize, Val>,
    budget: Budget,
}

enum StackItem {
    Mark,
    Value(Val),
}

pub fn loads(data: &[u8]) -> Result<Value, RpaError> {
    let mut p = Parser {
        data,
        i: 0,
        stack: Vec::new(),
        memo: HashMap::new(),
        budget: Budget {
            objects: 0,
            bytes: 0,
        },
    };
    let value = p.parse()?;
    freeze(value, &mut Vec::new())
}

impl<'a> Parser<'a> {
    fn parse(&mut self) -> Result<Val, RpaError> {
        while self.i < self.data.len() {
            let op = self.data[self.i];
            self.i += 1;
            self.step(op)?;
            if op == STOP {
                return match self.stack.pop() {
                    Some(StackItem::Value(v)) => Ok(v),
                    _ => Err(RpaError::new("pickle STOP with an empty stack")),
                };
            }
        }
        Err(RpaError::new("pickle ended without STOP"))
    }

    fn step(&mut self, op: u8) -> Result<(), RpaError> {
        match op {
            PROTO => {
                let v = self.u8_arg()?;
                if v > 5 {
                    return Err(RpaError::new(format!("unsupported pickle protocol {v}")));
                }
            }
            FRAME => {
                let _len = self.u64_arg()?;
            }
            NONE_OP => self.push(Val::None)?,
            NEWTRUE => self.push(Val::Bool(true))?,
            NEWFALSE => self.push(Val::Bool(false))?,
            EMPTY_TUPLE => self.push(Val::Tuple(Vec::new()))?,
            EMPTY_LIST => {
                self.budget.obj()?;
                self.push(Val::List(Rc::new(RefCell::new(Vec::new()))))?;
            }
            EMPTY_DICT => {
                self.budget.obj()?;
                self.push(Val::Dict(Rc::new(RefCell::new(Vec::new()))))?;
            }
            BININT1 => {
                let n = self.u8_arg()? as i128;
                self.push(Val::Int(n))?;
            }
            BININT2 => {
                let n = self.read(2)?;
                let n = u16::from_le_bytes([n[0], n[1]]) as i128;
                self.push(Val::Int(n))?;
            }
            BININT => {
                let n = self.read(4)?;
                let n = i32::from_le_bytes([n[0], n[1], n[2], n[3]]) as i128;
                self.push(Val::Int(n))?;
            }
            LONG1 => {
                let n = self.u8_arg()? as usize;
                self.push_long(n)?;
            }
            LONG4 => {
                let n = self.u32_len()?;
                self.push_long(n)?;
            }
            INT => self.push_text_int()?,
            LONG => self.push_text_long()?,
            SHORT_BINSTRING | SHORT_BINBYTES => {
                let n = self.u8_arg()? as usize;
                let bytes = self.read(n)?.to_vec();
                self.budget.add_bytes(bytes.len())?;
                self.push(Val::Bytes(bytes))?;
            }
            BINSTRING | BINBYTES => {
                let n = self.u32_len()?;
                let bytes = self.read(n)?.to_vec();
                self.budget.add_bytes(bytes.len())?;
                self.push(Val::Bytes(bytes))?;
            }
            BINBYTES8 => {
                let n = self.u64_len()?;
                let bytes = self.read(n)?.to_vec();
                self.budget.add_bytes(bytes.len())?;
                self.push(Val::Bytes(bytes))?;
            }
            SHORT_BINUNICODE => {
                let n = self.u8_arg()? as usize;
                self.push_utf8(n)?;
            }
            BINUNICODE => {
                let n = self.u32_len()?;
                self.push_utf8(n)?;
            }
            BINUNICODE8 => {
                let n = self.u64_len()?;
                self.push_utf8(n)?;
            }
            STRING => {
                let line = self.line()?;
                let s = decode_repr(line)?;
                self.budget.add_bytes(s.len())?;
                self.push(Val::Bytes(s))?;
            }
            UNICODE => {
                let line = self.line()?;
                let s = decode_raw_unicode(line)?;
                self.budget.add_bytes(s.len())?;
                self.push(Val::Str(s))?;
            }
            MARK => self.push_mark()?,
            TUPLE1 => self.push_tuple(1)?,
            TUPLE2 => self.push_tuple(2)?,
            TUPLE3 => self.push_tuple(3)?,
            TUPLE_OP => {
                let items = self.pop_mark()?;
                self.budget.obj()?;
                self.push(Val::Tuple(items))?;
            }
            LIST_OP => {
                let items = self.pop_mark()?;
                self.budget.obj()?;
                self.push(Val::List(Rc::new(RefCell::new(items))))?;
            }
            DICT_OP => {
                let items = self.pop_mark()?;
                self.push(Val::Dict(Rc::new(RefCell::new(pairs(items)?))))?;
            }
            APPEND => {
                let v = self.pop_val()?;
                match self.stack.last() {
                    Some(StackItem::Value(Val::List(list))) => list.borrow_mut().push(v),
                    _ => return Err(RpaError::new("APPEND without a list")),
                }
            }
            APPENDS => {
                let items = self.pop_mark()?;
                match self.stack.last() {
                    Some(StackItem::Value(Val::List(list))) => list.borrow_mut().extend(items),
                    _ => return Err(RpaError::new("APPENDS without a list")),
                }
            }
            SETITEM => {
                let value = self.pop_val()?;
                let key = self.pop_val()?;
                match self.stack.last() {
                    Some(StackItem::Value(Val::Dict(dict))) => dict.borrow_mut().push((key, value)),
                    _ => return Err(RpaError::new("SETITEM without a dict")),
                }
            }
            SETITEMS => {
                let items = self.pop_mark()?;
                let extra = pairs(items)?;
                match self.stack.last() {
                    Some(StackItem::Value(Val::Dict(dict))) => dict.borrow_mut().extend(extra),
                    _ => return Err(RpaError::new("SETITEMS without a dict")),
                }
            }
            GLOBAL => {
                let module = self.line()?.to_string();
                let name = self.line()?.to_string();
                self.push_callable(module, name)?;
            }
            STACK_GLOBAL => {
                let name = self.expect_str()?;
                let module = self.expect_str()?;
                self.push_callable(module, name)?;
            }
            REDUCE => self.reduce()?,
            BINPUT => {
                let idx = self.u8_arg()? as usize;
                self.memo_put(idx)?;
            }
            LONG_BINPUT => {
                let n = self.read(4)?;
                let idx = u32::from_le_bytes([n[0], n[1], n[2], n[3]]) as usize;
                self.memo_put(idx)?;
            }
            PUT => {
                let idx = parse_usize(self.line()?)?;
                self.memo_put(idx)?;
            }
            MEMOIZE => {
                let idx = self.memo.len();
                self.memo_put(idx)?;
            }
            BINGET => {
                let idx = self.u8_arg()? as usize;
                self.memo_get(idx)?;
            }
            b'j' => {
                // LONG_BINGET
                let n = self.read(4)?;
                let idx = u32::from_le_bytes([n[0], n[1], n[2], n[3]]) as usize;
                self.memo_get(idx)?;
            }
            GET => {
                let idx = parse_usize(self.line()?)?;
                self.memo_get(idx)?;
            }
            STOP => {}
            other => {
                return Err(RpaError::new(format!(
                    "unsupported pickle opcode 0x{other:02x}"
                )));
            }
        }
        Ok(())
    }

    fn push_callable(&mut self, module: String, name: String) -> Result<(), RpaError> {
        if module != "_codecs" || name != "encode" {
            return Err(RpaError::new(format!(
                "pickle global {module}.{name} is not allowed"
            )));
        }
        self.push(Val::Callable { module, name })
    }

    fn reduce(&mut self) -> Result<(), RpaError> {
        let args = self.pop_val()?;
        let func = self.pop_val()?;
        let Val::Callable { module, name } = func else {
            return Err(RpaError::new("REDUCE of a non-callable"));
        };
        if module != "_codecs" || name != "encode" {
            return Err(RpaError::new(format!(
                "pickle global {module}.{name} is not allowed"
            )));
        }
        let Val::Tuple(items) = args else {
            return Err(RpaError::new("_codecs.encode arguments are not a tuple"));
        };
        if items.len() < 2 {
            return Err(RpaError::new(
                "_codecs.encode needs a string and an encoding",
            ));
        }
        let text = match &items[0] {
            Val::Str(s) => s.clone(),
            Val::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            _ => return Err(RpaError::new("_codecs.encode text is not a string")),
        };
        let encoding = match &items[1] {
            Val::Str(s) => s.clone(),
            Val::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            _ => return Err(RpaError::new("_codecs.encode encoding is not a string")),
        };
        let bytes = decode_encoded(&text, &encoding)?;
        self.budget.add_bytes(bytes.len())?;
        self.push(Val::Bytes(bytes))
    }

    fn push_long(&mut self, n: usize) -> Result<(), RpaError> {
        if n > MAX_LONG {
            return Err(RpaError::new("pickle integer is too big"));
        }
        let bytes = self.read(n)?.to_vec();
        self.push(Val::Int(decode_long(&bytes)?))
    }

    fn push_utf8(&mut self, n: usize) -> Result<(), RpaError> {
        let bytes = self.read(n)?;
        self.budget.add_bytes(bytes.len())?;
        let s = std::str::from_utf8(bytes)
            .map_err(|_| RpaError::new("pickle string is not utf-8"))?
            .to_string();
        self.push(Val::Str(s))
    }

    fn push_text_int(&mut self) -> Result<(), RpaError> {
        let line = self.line()?;
        if line == "01" {
            return self.push(Val::Bool(true));
        }
        if line == "00" {
            return self.push(Val::Bool(false));
        }
        let n = line
            .parse::<i128>()
            .map_err(|_| RpaError::new(format!("bad pickle int {line:?}")))?;
        self.push(Val::Int(n))
    }

    fn push_text_long(&mut self) -> Result<(), RpaError> {
        let line = self.line()?;
        let digits = line.strip_suffix('L').unwrap_or(line);
        let n = digits
            .parse::<i128>()
            .map_err(|_| RpaError::new(format!("bad pickle long {line:?}")))?;
        self.push(Val::Int(n))
    }

    fn push_tuple(&mut self, n: usize) -> Result<(), RpaError> {
        if self.stack.len() < n {
            return Err(RpaError::new("pickle stack underflow"));
        }
        let start = self.stack.len() - n;
        let mut items = Vec::with_capacity(n);
        for item in self.stack.drain(start..) {
            match item {
                StackItem::Value(v) => items.push(v),
                StackItem::Mark => return Err(RpaError::new("tuple built across a mark")),
            }
        }
        self.budget.obj()?;
        self.push(Val::Tuple(items))
    }

    fn pop_mark(&mut self) -> Result<Vec<Val>, RpaError> {
        let mut items = Vec::new();
        loop {
            match self.stack.pop() {
                Some(StackItem::Mark) => {
                    items.reverse();
                    return Ok(items);
                }
                Some(StackItem::Value(v)) => items.push(v),
                None => return Err(RpaError::new("pickle mark is missing")),
            }
        }
    }

    fn pop_val(&mut self) -> Result<Val, RpaError> {
        match self.stack.pop() {
            Some(StackItem::Value(v)) => Ok(v),
            _ => Err(RpaError::new("pickle stack underflow")),
        }
    }

    fn expect_str(&mut self) -> Result<String, RpaError> {
        match self.pop_val()? {
            Val::Str(s) => Ok(s),
            Val::Bytes(b) => {
                String::from_utf8(b).map_err(|_| RpaError::new("global name is not text"))
            }
            _ => Err(RpaError::new("global name is not text")),
        }
    }

    fn memo_put(&mut self, idx: usize) -> Result<(), RpaError> {
        if self.memo.len() >= MAX_MEMO {
            return Err(RpaError::new("pickle memo is too large"));
        }
        let val = match self.stack.last() {
            Some(StackItem::Value(v)) => v.clone(),
            _ => return Err(RpaError::new("PUT with an empty stack")),
        };
        self.memo.insert(idx, val);
        Ok(())
    }

    fn memo_get(&mut self, idx: usize) -> Result<(), RpaError> {
        let val = self
            .memo
            .get(&idx)
            .cloned()
            .ok_or_else(|| RpaError::new(format!("pickle memo {idx} is missing")))?;
        self.push(val)
    }

    fn push(&mut self, v: Val) -> Result<(), RpaError> {
        if self.stack.len() >= MAX_STACK {
            return Err(RpaError::new("pickle stack is too deep"));
        }
        self.stack.push(StackItem::Value(v));
        Ok(())
    }

    fn push_mark(&mut self) -> Result<(), RpaError> {
        if self.stack.len() >= MAX_STACK {
            return Err(RpaError::new("pickle stack is too deep"));
        }
        self.stack.push(StackItem::Mark);
        Ok(())
    }

    fn u8_arg(&mut self) -> Result<u8, RpaError> {
        Ok(self.read(1)?[0])
    }

    fn u32_len(&mut self) -> Result<usize, RpaError> {
        let n = self.read(4)?;
        let n = u32::from_le_bytes([n[0], n[1], n[2], n[3]]) as usize;
        if n > MAX_BYTES {
            return Err(RpaError::new("pickle string is too large"));
        }
        Ok(n)
    }

    fn u64_len(&mut self) -> Result<usize, RpaError> {
        let n = self.u64_arg()?;
        if n > MAX_BYTES as u64 {
            return Err(RpaError::new("pickle string is too large"));
        }
        Ok(n as usize)
    }

    fn u64_arg(&mut self) -> Result<u64, RpaError> {
        let n = self.read(8)?;
        let mut b = [0u8; 8];
        b.copy_from_slice(n);
        Ok(u64::from_le_bytes(b))
    }

    fn read(&mut self, n: usize) -> Result<&'a [u8], RpaError> {
        let end = self
            .i
            .checked_add(n)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| RpaError::new("pickle ended early"))?;
        let s = &self.data[self.i..end];
        self.i = end;
        Ok(s)
    }

    fn line(&mut self) -> Result<&str, RpaError> {
        let start = self.i;
        while self.i < self.data.len() && self.data[self.i] != b'\n' {
            self.i += 1;
        }
        if self.i >= self.data.len() {
            return Err(RpaError::new("pickle ended early"));
        }
        let line = std::str::from_utf8(&self.data[start..self.i])
            .map_err(|_| RpaError::new("pickle text is not utf-8"))?;
        self.i += 1;
        Ok(line)
    }
}

fn pairs(items: Vec<Val>) -> Result<Vec<(Val, Val)>, RpaError> {
    if items.len() % 2 != 0 {
        return Err(RpaError::new("pickle dict has a dangling key"));
    }
    let mut out = Vec::with_capacity(items.len() / 2);
    let mut it = items.into_iter();
    while let Some(k) = it.next() {
        let v = it.next().expect("even length");
        out.push((k, v));
    }
    Ok(out)
}

fn parse_usize(s: &str) -> Result<usize, RpaError> {
    s.parse::<usize>()
        .map_err(|_| RpaError::new(format!("bad pickle memo index {s:?}")))
}

fn decode_long(bytes: &[u8]) -> Result<i128, RpaError> {
    if bytes.is_empty() {
        return Ok(0);
    }
    if bytes.len() > MAX_LONG {
        return Err(RpaError::new("pickle integer is too big"));
    }
    let mut acc: i128 = 0;
    for (i, b) in bytes.iter().enumerate() {
        acc |= (*b as i128) << (8 * i);
    }
    let bits = bytes.len() * 8;
    if bits < 128 && bytes.last().copied().unwrap_or(0) & 0x80 != 0 {
        let shift = 128 - bits;
        acc = (acc << shift) >> shift;
    }
    Ok(acc)
}

fn decode_encoded(text: &str, encoding: &str) -> Result<Vec<u8>, RpaError> {
    match encoding.to_ascii_lowercase().as_str() {
        "latin1" | "latin-1" | "iso-8859-1" | "iso8859-1" => latin1_bytes(text),
        "utf-8" | "utf8" => Ok(text.as_bytes().to_vec()),
        "ascii" if text.is_ascii() => Ok(text.as_bytes().to_vec()),
        "ascii" => Err(RpaError::new("ascii encoding contains non-ascii text")),
        other => Err(RpaError::new(format!(
            "pickle encoding {other} is not allowed"
        ))),
    }
}

pub fn latin1_bytes(text: &str) -> Result<Vec<u8>, RpaError> {
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let c = ch as u32;
        if c > 255 {
            return Err(RpaError::new("text is not latin-1"));
        }
        out.push(c as u8);
    }
    Ok(out)
}

fn decode_repr(line: &str) -> Result<Vec<u8>, RpaError> {
    let mut chars = line.chars().peekable();
    let quote = chars
        .next()
        .ok_or_else(|| RpaError::new("empty pickle string"))?;
    if quote != '\'' && quote != '"' {
        return Err(RpaError::new("pickle string is not quoted"));
    }
    let mut out = Vec::new();
    while let Some(c) = chars.next() {
        if c == quote {
            if chars.peek().is_some() {
                return Err(RpaError::new("trailing data in pickle string"));
            }
            return Ok(out);
        }
        if c != '\\' {
            latin1_push(&mut out, c)?;
            continue;
        }
        let esc = chars
            .next()
            .ok_or_else(|| RpaError::new("truncated pickle escape"))?;
        match esc {
            '\\' => out.push(b'\\'),
            '\'' => out.push(b'\''),
            '"' => out.push(b'"'),
            'n' => out.push(b'\n'),
            'r' => out.push(b'\r'),
            't' => out.push(b'\t'),
            'x' => {
                let h1 = chars
                    .next()
                    .ok_or_else(|| RpaError::new("truncated \\x escape"))?;
                let h2 = chars
                    .next()
                    .ok_or_else(|| RpaError::new("truncated \\x escape"))?;
                let hex = format!("{h1}{h2}");
                let b =
                    u8::from_str_radix(&hex, 16).map_err(|_| RpaError::new("bad \\x escape"))?;
                out.push(b);
            }
            other => latin1_push(&mut out, other)?,
        }
    }
    Err(RpaError::new("unterminated pickle string"))
}

fn latin1_push(out: &mut Vec<u8>, c: char) -> Result<(), RpaError> {
    let u = c as u32;
    if u > 255 {
        return Err(RpaError::new("pickle string is not latin-1"));
    }
    out.push(u as u8);
    Ok(())
}

fn decode_raw_unicode(line: &str) -> Result<String, RpaError> {
    let mut chars = line.chars().peekable();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let esc = chars
            .next()
            .ok_or_else(|| RpaError::new("truncated unicode escape"))?;
        match esc {
            '\\' => out.push('\\'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                let mut hex = String::new();
                for _ in 0..4 {
                    hex.push(
                        chars
                            .next()
                            .ok_or_else(|| RpaError::new("truncated \\u escape"))?,
                    );
                }
                let cp =
                    u32::from_str_radix(&hex, 16).map_err(|_| RpaError::new("bad \\u escape"))?;
                out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

fn freeze(v: Val, stack: &mut Vec<usize>) -> Result<Value, RpaError> {
    if stack.len() > MAX_DEPTH {
        return Err(RpaError::new("pickle nesting is too deep"));
    }
    Ok(match v {
        Val::None => Value::None,
        Val::Bool(b) => Value::Bool(b),
        Val::Int(n) => Value::Int(n),
        Val::Bytes(b) => Value::Bytes(b),
        Val::Str(s) => Value::Str(s),
        Val::Tuple(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(freeze(item, stack)?);
            }
            Value::Tuple(out)
        }
        Val::List(rc) => {
            let ptr = Rc::as_ptr(&rc) as usize;
            if stack.contains(&ptr) {
                return Err(RpaError::new("pickle list is cyclic"));
            }
            stack.push(ptr);
            let items = rc.borrow().clone();
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(freeze(item, stack)?);
            }
            stack.pop();
            Value::List(out)
        }
        Val::Dict(rc) => {
            let ptr = Rc::as_ptr(&rc) as usize;
            if stack.contains(&ptr) {
                return Err(RpaError::new("pickle dict is cyclic"));
            }
            stack.push(ptr);
            let items = rc.borrow().clone();
            let mut out = Vec::with_capacity(items.len());
            for (k, v) in items {
                out.push((freeze(k, stack)?, freeze(v, stack)?));
            }
            stack.pop();
            Value::Dict(out)
        }
        Val::Callable { module, name } => {
            return Err(RpaError::new(format!(
                "pickle global {module}.{name} was not called"
            )));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_protocol2_index() {
        let entries = vec![(
            "script.rpy",
            vec![
                IndexPair {
                    offset: 34,
                    len: 12,
                },
                IndexPair {
                    offset: 5_000_000_000,
                    len: 3,
                },
            ],
        )];
        let bytes = dump_index(&entries);
        let v = loads(&bytes).unwrap();
        match v {
            Value::Dict(pairs) => {
                assert_eq!(pairs.len(), 1);
                assert_eq!(pairs[0].0, Value::Str("script.rpy".into()));
                match &pairs[0].1 {
                    Value::List(items) => {
                        assert_eq!(items.len(), 2);
                        assert_eq!(items[0], Value::Tuple(vec![Value::Int(34), Value::Int(12)]));
                        assert_eq!(
                            items[1],
                            Value::Tuple(vec![Value::Int(5_000_000_000), Value::Int(3)])
                        );
                    }
                    other => panic!("list expected, got {other:?}"),
                }
            }
            other => panic!("dict expected, got {other:?}"),
        }
    }

    #[test]
    fn codecs_encode_is_the_only_global() {
        // PROTO 2, GLOBAL _codecs encode, BINUNICODE "AB", BINUNICODE "latin1", TUPLE2, REDUCE, STOP
        let mut p = vec![0x80, 2, b'c'];
        p.extend(b"_codecs\nencode\n");
        p.push(b'X');
        p.extend_from_slice(&2u32.to_le_bytes());
        p.extend(b"AB");
        p.push(b'X');
        p.extend_from_slice(&6u32.to_le_bytes());
        p.extend(b"latin1");
        p.push(0x86); // TUPLE2
        p.push(b'R');
        p.push(b'.');
        let v = loads(&p).unwrap();
        assert_eq!(v, Value::Bytes(b"AB".to_vec()));

        let bad = b"cos\nsystem\n.".to_vec();
        let err = loads(&bad).unwrap_err();
        assert!(err.to_string().contains("not allowed"), "{err}");
    }

    #[test]
    fn truncated_and_unknown_opcode_are_errors() {
        assert!(loads(&[0x80, 2]).is_err());
        assert!(loads(&[0x80, 2, b'F', b'.']).is_err());
        assert!(loads(&[]).is_err());
    }
}
