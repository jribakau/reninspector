//! Object pickle reader for `.rpyc` files.
//!
//! Classes are never imported or called. A global becomes `Value::Object`
//! with the class name, constructor arguments, and the `BUILD` state.
//! Only modules a Ren'Py script pickle actually uses are accepted.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const MAX_DEPTH: usize = 64;
const MAX_OBJECTS: usize = 5_000_000;
const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_STACK: usize = 100_000;
const MAX_MEMO: usize = 2_000_000;
const MAX_LONG: usize = 64;

const MARK: u8 = b'(';
const STOP: u8 = b'.';
const POP: u8 = b'0';
const POP_MARK: u8 = b'1';
const DUP: u8 = b'2';
const FLOAT: u8 = b'F';
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
const BUILD: u8 = b'b';
const GLOBAL: u8 = b'c';
const DICT_OP: u8 = b'd';
const EMPTY_DICT: u8 = b'}';
const APPENDS: u8 = b'e';
const GET: u8 = b'g';
const BINGET: u8 = b'h';
const INST: u8 = b'i';
const LIST_OP: u8 = b'l';
const EMPTY_LIST: u8 = b']';
const OBJ: u8 = b'o';
const PUT: u8 = b'p';
const BINPUT: u8 = b'q';
const LONG_BINPUT: u8 = b'r';
const SETITEM: u8 = b's';
const TUPLE_OP: u8 = b't';
const EMPTY_TUPLE: u8 = b')';
const SETITEMS: u8 = b'u';
const BINFLOAT: u8 = b'G';
const PROTO: u8 = 0x80;
const NEWOBJ: u8 = 0x81;
const TUPLE1: u8 = 0x85;
const TUPLE2: u8 = 0x86;
const TUPLE3: u8 = 0x87;
const NEWTRUE: u8 = 0x88;
const NEWFALSE: u8 = 0x89;
const LONG1: u8 = 0x8a;
const LONG4: u8 = 0x8b;
const SHORT_BINUNICODE: u8 = 0x8c;
const BINUNICODE8: u8 = 0x8d;
const BINBYTES8: u8 = 0x8e;
const EMPTY_SET: u8 = 0x8f;
const ADDITEMS: u8 = 0x90;
const FROZENSET: u8 = 0x91;
const NEWOBJ_EX: u8 = 0x92;
const STACK_GLOBAL: u8 = 0x93;
const MEMOIZE: u8 = 0x94;
const FRAME: u8 = 0x95;
const BINBYTES: u8 = b'B';
const SHORT_BINBYTES: u8 = b'C';

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    None,
    Bool(bool),
    Int(i128),
    Float(f64),
    Bytes(Vec<u8>),
    Str(String),
    List(Vec<Value>),
    Tuple(Vec<Value>),
    Dict(Vec<(Value, Value)>),
    Set(Vec<Value>),
    Object {
        class: String,
        args: Vec<Value>,
        state: Option<Box<Value>>,
    },
}

#[derive(Clone, Debug)]
enum Val {
    None,
    Bool(bool),
    Int(i128),
    Float(f64),
    Bytes(Vec<u8>),
    Str(String),
    List(Rc<RefCell<Vec<Val>>>),
    Tuple(Vec<Val>),
    Dict(Rc<RefCell<Vec<(Val, Val)>>>),
    Set(Rc<RefCell<Vec<Val>>>),
    Object(Rc<RefCell<Obj>>),
    Callable { module: String, name: String },
}

#[derive(Clone, Debug)]
struct Obj {
    class: String,
    args: Vec<Val>,
    state: Option<Val>,
}

#[derive(Debug)]
pub struct UnpickleError(pub String);

impl std::fmt::Display for UnpickleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl UnpickleError {
    fn new(msg: impl Into<String>) -> Self {
        UnpickleError(msg.into())
    }
}

struct Budget {
    objects: usize,
    bytes: usize,
}

impl Budget {
    fn obj(&mut self) -> Result<(), UnpickleError> {
        self.objects += 1;
        if self.objects > MAX_OBJECTS {
            return Err(UnpickleError::new("pickle has too many objects"));
        }
        Ok(())
    }

    fn add_bytes(&mut self, n: usize) -> Result<(), UnpickleError> {
        self.bytes = self.bytes.saturating_add(n);
        if self.bytes > MAX_BYTES {
            return Err(UnpickleError::new("pickle strings are too large"));
        }
        Ok(())
    }
}

enum StackItem {
    Mark,
    Value(Val),
}

struct Parser<'a> {
    data: &'a [u8],
    i: usize,
    stack: Vec<StackItem>,
    memo: HashMap<usize, Val>,
    budget: Budget,
}

pub fn loads(data: &[u8]) -> Result<Value, UnpickleError> {
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
    let mut seen = Vec::new();
    freeze(value, &mut seen, 0)
}

impl<'a> Parser<'a> {
    fn parse(&mut self) -> Result<Val, UnpickleError> {
        while self.i < self.data.len() {
            let op = self.data[self.i];
            self.i += 1;
            self.step(op)?;
            if op == STOP {
                return match self.stack.pop() {
                    Some(StackItem::Value(v)) => Ok(v),
                    _ => Err(UnpickleError::new("pickle STOP with an empty stack")),
                };
            }
        }
        Err(UnpickleError::new("pickle ended without STOP"))
    }

    fn step(&mut self, op: u8) -> Result<(), UnpickleError> {
        match op {
            PROTO => {
                let v = self.u8_arg()?;
                if v > 5 {
                    return Err(UnpickleError::new(format!(
                        "unsupported pickle protocol {v}"
                    )));
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
            EMPTY_SET => {
                self.budget.obj()?;
                self.push(Val::Set(Rc::new(RefCell::new(Vec::new()))))?;
            }
            BININT1 => {
                let n = self.u8_arg()? as i128;
                self.push(Val::Int(n))?;
            }
            BININT2 => {
                let n = self.read(2)?;
                self.push(Val::Int(u16::from_le_bytes([n[0], n[1]]) as i128))?;
            }
            BININT => {
                let n = self.read(4)?;
                self.push(Val::Int(
                    i32::from_le_bytes([n[0], n[1], n[2], n[3]]) as i128
                ))?;
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
            FLOAT => {
                let line = self.line()?.to_string();
                let n = line
                    .parse::<f64>()
                    .map_err(|_| UnpickleError::new(format!("bad pickle float {line:?}")))?;
                self.push(Val::Float(n))?;
            }
            BINFLOAT => {
                let n = self.read(8)?;
                let bits = u64::from_be_bytes([n[0], n[1], n[2], n[3], n[4], n[5], n[6], n[7]]);
                self.push(Val::Float(f64::from_bits(bits)))?;
            }
            SHORT_BINSTRING | SHORT_BINBYTES => {
                let n = self.u8_arg()? as usize;
                self.push_bytes(n)?;
            }
            BINSTRING | BINBYTES => {
                let n = self.u32_len()?;
                self.push_bytes(n)?;
            }
            BINBYTES8 => {
                let n = self.u64_len()?;
                self.push_bytes(n)?;
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
            POP => {
                self.stack
                    .pop()
                    .ok_or_else(|| UnpickleError::new("POP on an empty stack"))?;
            }
            POP_MARK => {
                self.pop_mark()?;
            }
            DUP => {
                let v = match self.stack.last() {
                    Some(StackItem::Value(v)) => v.clone(),
                    _ => return Err(UnpickleError::new("DUP on an empty stack")),
                };
                self.push(v)?;
            }
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
            FROZENSET => {
                let items = self.pop_mark()?;
                self.budget.obj()?;
                self.push(Val::Set(Rc::new(RefCell::new(items))))?;
            }
            APPEND => {
                let v = self.pop_val()?;
                match self.stack.last() {
                    Some(StackItem::Value(Val::List(list))) => list.borrow_mut().push(v),
                    _ => return Err(UnpickleError::new("APPEND without a list")),
                }
            }
            APPENDS => {
                let items = self.pop_mark()?;
                match self.stack.last() {
                    Some(StackItem::Value(Val::List(list))) => list.borrow_mut().extend(items),
                    _ => return Err(UnpickleError::new("APPENDS without a list")),
                }
            }
            ADDITEMS => {
                let items = self.pop_mark()?;
                match self.stack.last() {
                    Some(StackItem::Value(Val::Set(set))) => set.borrow_mut().extend(items),
                    _ => return Err(UnpickleError::new("ADDITEMS without a set")),
                }
            }
            SETITEM => {
                let value = self.pop_val()?;
                let key = self.pop_val()?;
                match self.stack.last() {
                    Some(StackItem::Value(Val::Dict(dict))) => dict.borrow_mut().push((key, value)),
                    _ => return Err(UnpickleError::new("SETITEM without a dict")),
                }
            }
            SETITEMS => {
                let items = self.pop_mark()?;
                let extra = pairs(items)?;
                match self.stack.last() {
                    Some(StackItem::Value(Val::Dict(dict))) => dict.borrow_mut().extend(extra),
                    _ => return Err(UnpickleError::new("SETITEMS without a dict")),
                }
            }
            GLOBAL => {
                let module = self.line()?.to_string();
                let name = self.line()?.to_string();
                self.push_callable(&module, &name)?;
            }
            STACK_GLOBAL => {
                let name = self.expect_text()?;
                let module = self.expect_text()?;
                self.push_callable(&module, &name)?;
            }
            REDUCE => self.reduce()?,
            BUILD => self.build()?,
            NEWOBJ => self.newobj(false)?,
            NEWOBJ_EX => self.newobj(true)?,
            INST => {
                let module = self.line()?.to_string();
                let name = self.line()?.to_string();
                self.push_callable(&module, &name)?;
                let args = self.pop_mark()?;
                let class = format!("{module}.{name}");
                self.make_object(class, args, None)?;
            }
            OBJ => {
                let args = self.pop_mark()?;
                if args.is_empty() {
                    return Err(UnpickleError::new("OBJ without a class"));
                }
                let class = match &args[0] {
                    Val::Callable { module, name } => format!("{module}.{name}"),
                    _ => return Err(UnpickleError::new("OBJ class is not a global")),
                };
                self.make_object(class, args[1..].to_vec(), None)?;
            }
            BINPUT => {
                let idx = self.u8_arg()? as usize;
                self.memo_put(idx)?;
            }
            LONG_BINPUT => {
                let n = self.read(4)?;
                self.memo_put(u32::from_le_bytes([n[0], n[1], n[2], n[3]]) as usize)?;
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
                let n = self.read(4)?;
                self.memo_get(u32::from_le_bytes([n[0], n[1], n[2], n[3]]) as usize)?;
            }
            GET => {
                let idx = parse_usize(self.line()?)?;
                self.memo_get(idx)?;
            }
            STOP => {}
            other => {
                return Err(UnpickleError::new(format!(
                    "unsupported pickle opcode 0x{other:02x}"
                )));
            }
        }
        Ok(())
    }

    fn push_callable(&mut self, module: &str, name: &str) -> Result<(), UnpickleError> {
        if !allowed_module(module) {
            return Err(UnpickleError::new(format!(
                "pickle global {module}.{name} is not allowed"
            )));
        }
        self.push(Val::Callable {
            module: module.to_string(),
            name: name.to_string(),
        })
    }

    fn reduce(&mut self) -> Result<(), UnpickleError> {
        let args = self.pop_val()?;
        let func = self.pop_val()?;
        let Val::Callable { module, name } = func else {
            return Err(UnpickleError::new("REDUCE of a non-callable"));
        };
        let args = match args {
            Val::Tuple(items) => items,
            other => vec![other],
        };
        if module == "_codecs" && name == "encode" {
            return self.reduce_encode(args);
        }
        if name == "_reconstructor" && (module == "copyreg" || module == "copy_reg") {
            let class = match args.first() {
                Some(Val::Callable { module, name }) => format!("{module}.{name}"),
                _ => return Err(UnpickleError::new("reconstructor is missing its class")),
            };
            return self.make_object(class, Vec::new(), None);
        }
        if is_container(&module, &name) {
            return self.reduce_container(&name, args);
        }
        self.make_object(format!("{module}.{name}"), args, None)
    }

    fn reduce_encode(&mut self, items: Vec<Val>) -> Result<(), UnpickleError> {
        if items.len() < 2 {
            return Err(UnpickleError::new(
                "_codecs.encode needs a string and an encoding",
            ));
        }
        let text = as_text(&items[0])
            .ok_or_else(|| UnpickleError::new("_codecs.encode text is not a string"))?;
        let encoding = as_text(&items[1])
            .ok_or_else(|| UnpickleError::new("_codecs.encode encoding is not a string"))?;
        let bytes = decode_encoded(&text, &encoding)?;
        self.budget.add_bytes(bytes.len())?;
        self.push(Val::Bytes(bytes))
    }

    fn reduce_container(&mut self, name: &str, args: Vec<Val>) -> Result<(), UnpickleError> {
        self.budget.obj()?;
        let lower = name.to_ascii_lowercase();
        if lower.contains("dict") || lower == "defaultdict" {
            let dict = Rc::new(RefCell::new(Vec::new()));
            if let Some(Val::Dict(items)) = args.first() {
                dict.borrow_mut().extend(items.borrow().clone());
            }
            self.push(Val::Dict(dict))
        } else if lower.contains("list") {
            let items = match args.first() {
                Some(Val::List(list)) => list.borrow().clone(),
                Some(Val::Tuple(items)) => items.clone(),
                _ => Vec::new(),
            };
            self.push(Val::List(Rc::new(RefCell::new(items))))
        } else if lower.contains("set") {
            let items = match args.first() {
                Some(Val::List(list)) => list.borrow().clone(),
                Some(Val::Tuple(items)) => items.clone(),
                Some(Val::Set(set)) => set.borrow().clone(),
                _ => Vec::new(),
            };
            self.push(Val::Set(Rc::new(RefCell::new(items))))
        } else {
            self.push(Val::Tuple(args))
        }
    }

    fn build(&mut self) -> Result<(), UnpickleError> {
        let state = self.pop_val()?;
        match self.stack.last() {
            Some(StackItem::Value(Val::Object(obj))) => obj.borrow_mut().state = Some(state),
            Some(StackItem::Value(Val::Dict(dict))) => {
                if let Val::Dict(extra) = &state {
                    dict.borrow_mut().extend(extra.borrow().clone());
                }
            }
            _ => return Err(UnpickleError::new("BUILD without an object")),
        }
        Ok(())
    }

    fn newobj(&mut self, with_kwargs: bool) -> Result<(), UnpickleError> {
        if with_kwargs {
            self.pop_val()?;
        }
        let args = self.pop_val()?;
        let class = self.pop_val()?;
        let Val::Callable { module, name } = class else {
            return Err(UnpickleError::new("NEWOBJ class is not a global"));
        };
        let args = match args {
            Val::Tuple(items) => items,
            other => vec![other],
        };
        if is_container(&module, &name) {
            return self.reduce_container(&name, args);
        }
        self.make_object(format!("{module}.{name}"), args, None)
    }

    fn make_object(
        &mut self,
        class: String,
        args: Vec<Val>,
        state: Option<Val>,
    ) -> Result<(), UnpickleError> {
        self.budget.obj()?;
        self.push(Val::Object(Rc::new(RefCell::new(Obj {
            class,
            args,
            state,
        }))))
    }

    fn push_long(&mut self, n: usize) -> Result<(), UnpickleError> {
        if n > MAX_LONG {
            return Err(UnpickleError::new("pickle integer is too big"));
        }
        let bytes = self.read(n)?.to_vec();
        self.push(Val::Int(decode_long(&bytes)?))
    }

    fn push_bytes(&mut self, n: usize) -> Result<(), UnpickleError> {
        let bytes = self.read(n)?.to_vec();
        self.budget.add_bytes(bytes.len())?;
        self.push(Val::Bytes(bytes))
    }

    fn push_utf8(&mut self, n: usize) -> Result<(), UnpickleError> {
        let bytes = self.read(n)?;
        self.budget.add_bytes(bytes.len())?;
        let s = std::str::from_utf8(bytes)
            .map_err(|_| UnpickleError::new("pickle string is not utf-8"))?
            .to_string();
        self.push(Val::Str(s))
    }

    fn push_text_int(&mut self) -> Result<(), UnpickleError> {
        let line = self.line()?;
        if line == "01" {
            return self.push(Val::Bool(true));
        }
        if line == "00" {
            return self.push(Val::Bool(false));
        }
        let n = line
            .parse::<i128>()
            .map_err(|_| UnpickleError::new(format!("bad pickle int {line:?}")))?;
        self.push(Val::Int(n))
    }

    fn push_text_long(&mut self) -> Result<(), UnpickleError> {
        let line = self.line()?;
        let digits = line.strip_suffix('L').unwrap_or(line);
        let n = digits
            .parse::<i128>()
            .map_err(|_| UnpickleError::new(format!("bad pickle long {line:?}")))?;
        self.push(Val::Int(n))
    }

    fn push_tuple(&mut self, n: usize) -> Result<(), UnpickleError> {
        if self.stack.len() < n {
            return Err(UnpickleError::new("pickle stack underflow"));
        }
        let start = self.stack.len() - n;
        let mut items = Vec::with_capacity(n);
        for item in self.stack.drain(start..) {
            match item {
                StackItem::Value(v) => items.push(v),
                StackItem::Mark => return Err(UnpickleError::new("tuple built across a mark")),
            }
        }
        self.budget.obj()?;
        self.push(Val::Tuple(items))
    }

    fn pop_mark(&mut self) -> Result<Vec<Val>, UnpickleError> {
        let mut items = Vec::new();
        loop {
            match self.stack.pop() {
                Some(StackItem::Mark) => {
                    items.reverse();
                    return Ok(items);
                }
                Some(StackItem::Value(v)) => items.push(v),
                None => return Err(UnpickleError::new("pickle mark is missing")),
            }
        }
    }

    fn pop_val(&mut self) -> Result<Val, UnpickleError> {
        match self.stack.pop() {
            Some(StackItem::Value(v)) => Ok(v),
            _ => Err(UnpickleError::new("pickle stack underflow")),
        }
    }

    fn expect_text(&mut self) -> Result<String, UnpickleError> {
        as_text(&self.pop_val()?).ok_or_else(|| UnpickleError::new("global name is not text"))
    }

    fn memo_put(&mut self, idx: usize) -> Result<(), UnpickleError> {
        if self.memo.len() >= MAX_MEMO {
            return Err(UnpickleError::new("pickle memo is too large"));
        }
        let val = match self.stack.last() {
            Some(StackItem::Value(v)) => v.clone(),
            _ => return Err(UnpickleError::new("PUT with an empty stack")),
        };
        self.memo.insert(idx, val);
        Ok(())
    }

    fn memo_get(&mut self, idx: usize) -> Result<(), UnpickleError> {
        let val = self
            .memo
            .get(&idx)
            .cloned()
            .ok_or_else(|| UnpickleError::new(format!("pickle memo {idx} is missing")))?;
        self.push(val)
    }

    fn push(&mut self, v: Val) -> Result<(), UnpickleError> {
        if self.stack.len() >= MAX_STACK {
            return Err(UnpickleError::new("pickle stack is too deep"));
        }
        self.stack.push(StackItem::Value(v));
        Ok(())
    }

    fn push_mark(&mut self) -> Result<(), UnpickleError> {
        if self.stack.len() >= MAX_STACK {
            return Err(UnpickleError::new("pickle stack is too deep"));
        }
        self.stack.push(StackItem::Mark);
        Ok(())
    }

    fn u8_arg(&mut self) -> Result<u8, UnpickleError> {
        Ok(self.read(1)?[0])
    }

    fn u32_len(&mut self) -> Result<usize, UnpickleError> {
        let n = self.read(4)?;
        Ok(u32::from_le_bytes([n[0], n[1], n[2], n[3]]) as usize)
    }

    fn u64_len(&mut self) -> Result<usize, UnpickleError> {
        let n = self.u64_arg()?;
        usize::try_from(n).map_err(|_| UnpickleError::new("pickle length does not fit"))
    }

    fn u64_arg(&mut self) -> Result<u64, UnpickleError> {
        let n = self.read(8)?;
        Ok(u64::from_le_bytes([
            n[0], n[1], n[2], n[3], n[4], n[5], n[6], n[7],
        ]))
    }

    fn read(&mut self, n: usize) -> Result<&'a [u8], UnpickleError> {
        if self.i + n > self.data.len() {
            return Err(UnpickleError::new("pickle ended early"));
        }
        let out = &self.data[self.i..self.i + n];
        self.i += n;
        Ok(out)
    }

    fn line(&mut self) -> Result<&'a str, UnpickleError> {
        let start = self.i;
        while self.i < self.data.len() && self.data[self.i] != b'\n' {
            self.i += 1;
        }
        if self.i >= self.data.len() {
            return Err(UnpickleError::new("pickle line ended early"));
        }
        let bytes = &self.data[start..self.i];
        self.i += 1;
        std::str::from_utf8(bytes).map_err(|_| UnpickleError::new("pickle line is not utf-8"))
    }
}

fn allowed_module(module: &str) -> bool {
    module == "renpy"
        || module.starts_with("renpy.")
        || module == "store"
        || module.starts_with("store.")
        || module == "builtins"
        || module == "__builtin__"
        || module == "_codecs"
        || module == "collections"
        || module == "copyreg"
        || module == "copy_reg"
        || module == "types"
        || module == "__builtin__"
}

fn is_container(module: &str, name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    (module == "collections" && name == "defaultdict")
        || n.contains("revertabledict")
        || n.contains("revertablelist")
        || n.contains("revertableset")
        || ((module == "builtins" || module == "__builtin__")
            && matches!(name, "list" | "dict" | "set" | "frozenset" | "tuple"))
}

fn as_text(v: &Val) -> Option<String> {
    match v {
        Val::Str(s) => Some(s.clone()),
        Val::Bytes(b) => String::from_utf8(b.clone()).ok(),
        _ => None,
    }
}

fn pairs(items: Vec<Val>) -> Result<Vec<(Val, Val)>, UnpickleError> {
    if items.len() % 2 != 0 {
        return Err(UnpickleError::new("pickle dict has an odd number of items"));
    }
    let mut out = Vec::with_capacity(items.len() / 2);
    let mut it = items.into_iter();
    while let Some(k) = it.next() {
        out.push((k, it.next().unwrap()));
    }
    Ok(out)
}

fn decode_long(bytes: &[u8]) -> Result<i128, UnpickleError> {
    if bytes.is_empty() {
        return Ok(0);
    }
    let mut n = 0i128;
    for (i, b) in bytes.iter().enumerate() {
        n |= (*b as i128) << (8 * i);
    }
    if bytes.last().copied().unwrap_or(0) & 0x80 != 0 {
        let bits = bytes.len() * 8;
        if bits < 128 {
            n |= -1i128 << bits;
        }
    }
    Ok(n)
}

fn decode_encoded(text: &str, encoding: &str) -> Result<Vec<u8>, UnpickleError> {
    match encoding.to_ascii_lowercase().as_str() {
        "latin1" | "latin-1" | "iso-8859-1" => Ok(text.chars().map(|c| c as u8).collect()),
        "utf-8" | "utf8" => Ok(text.as_bytes().to_vec()),
        "ascii" => {
            if text.is_ascii() {
                Ok(text.as_bytes().to_vec())
            } else {
                Err(UnpickleError::new(
                    "pickle ascii string has non-ascii bytes",
                ))
            }
        }
        other => Err(UnpickleError::new(format!(
            "pickle encoding {other} is not allowed"
        ))),
    }
}

fn decode_repr(line: &str) -> Result<Vec<u8>, UnpickleError> {
    let line = line.trim();
    let quote = line.chars().next().unwrap_or('\0');
    if quote != '\'' && quote != '"' {
        return Err(UnpickleError::new("pickle string is not quoted"));
    }
    let mut out = Vec::new();
    let mut chars = line[1..].chars().peekable();
    while let Some(c) = chars.next() {
        if c == quote {
            return Ok(out);
        }
        if c != '\\' {
            let mut buf = [0; 4];
            out.extend(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        match chars.next() {
            Some('n') => out.push(b'\n'),
            Some('r') => out.push(b'\r'),
            Some('t') => out.push(b'\t'),
            Some('\\') => out.push(b'\\'),
            Some('\'') => out.push(b'\''),
            Some('"') => out.push(b'"'),
            Some('x') => {
                let h: String = chars.by_ref().take(2).collect();
                let b = u8::from_str_radix(&h, 16)
                    .map_err(|_| UnpickleError::new("bad pickle escape"))?;
                out.push(b);
            }
            _ => return Err(UnpickleError::new("bad pickle escape")),
        }
    }
    Err(UnpickleError::new("pickle string was not closed"))
}

fn decode_raw_unicode(line: &str) -> Result<String, UnpickleError> {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('u') => {
                let h: String = chars.by_ref().take(4).collect();
                let cp = u32::from_str_radix(&h, 16)
                    .map_err(|_| UnpickleError::new("bad unicode escape"))?;
                out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
            }
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => out.push(other),
            None => break,
        }
    }
    Ok(out)
}

fn note_cycle(ptr: *const (), seen: &mut Vec<*const ()>) -> Result<(), UnpickleError> {
    if seen.contains(&ptr) {
        return Err(UnpickleError::new("pickle contains a cycle"));
    }
    seen.push(ptr);
    Ok(())
}

fn freeze(v: Val, seen: &mut Vec<*const ()>, depth: usize) -> Result<Value, UnpickleError> {
    if depth > MAX_DEPTH {
        return Err(UnpickleError::new("pickle nesting is too deep"));
    }
    match v {
        Val::None => Ok(Value::None),
        Val::Bool(b) => Ok(Value::Bool(b)),
        Val::Int(n) => Ok(Value::Int(n)),
        Val::Float(n) => Ok(Value::Float(n)),
        Val::Bytes(b) => Ok(Value::Bytes(b)),
        Val::Str(s) => Ok(Value::Str(s)),
        Val::Tuple(items) => Ok(Value::Tuple(freeze_list(items, seen, depth)?)),
        Val::List(list) => {
            note_cycle(Rc::as_ptr(&list) as *const (), seen)?;
            let items = list.borrow().clone();
            let out = Value::List(freeze_list(items, seen, depth)?);
            seen.pop();
            Ok(out)
        }
        Val::Dict(dict) => {
            note_cycle(Rc::as_ptr(&dict) as *const (), seen)?;
            let pairs = dict.borrow().clone();
            let mut out = Vec::new();
            for (k, v) in pairs {
                out.push((freeze(k, seen, depth + 1)?, freeze(v, seen, depth + 1)?));
            }
            seen.pop();
            Ok(Value::Dict(out))
        }
        Val::Set(set) => {
            note_cycle(Rc::as_ptr(&set) as *const (), seen)?;
            let items = set.borrow().clone();
            let out = Value::Set(freeze_list(items, seen, depth)?);
            seen.pop();
            Ok(out)
        }
        Val::Object(obj) => {
            note_cycle(Rc::as_ptr(&obj) as *const (), seen)?;
            let (class, args, state) = {
                let obj = obj.borrow();
                (obj.class.clone(), obj.args.clone(), obj.state.clone())
            };
            let out = Value::Object {
                class,
                args: freeze_list(args, seen, depth)?,
                state: match state {
                    Some(s) => Some(Box::new(freeze(s, seen, depth + 1)?)),
                    None => None,
                },
            };
            seen.pop();
            Ok(out)
        }
        Val::Callable { module, name } => Ok(Value::Str(format!("{module}.{name}"))),
    }
}

fn freeze_list(
    items: Vec<Val>,
    seen: &mut Vec<*const ()>,
    depth: usize,
) -> Result<Vec<Value>, UnpickleError> {
    items
        .into_iter()
        .map(|v| freeze(v, seen, depth + 1))
        .collect()
}

fn parse_usize(s: &str) -> Result<usize, UnpickleError> {
    s.parse()
        .map_err(|_| UnpickleError::new(format!("bad pickle memo index {s:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_an_object_without_calling_it() {
        // PROTO 2, GLOBAL renpy.ast Say, EMPTY_TUPLE, REDUCE, dict what="Hi", BUILD, STOP
        let mut p = vec![0x80, 2, b'c'];
        p.extend(b"renpy.ast\nSay\n");
        p.push(b')');
        p.push(b'R');
        p.push(b'}');
        p.push(b'X');
        p.extend(4u32.to_le_bytes());
        p.extend(b"what");
        p.push(b'X');
        p.extend(2u32.to_le_bytes());
        p.extend(b"Hi");
        p.push(b's');
        p.push(b'b');
        p.push(b'.');
        let v = loads(&p).unwrap();
        match v {
            Value::Object { class, state, .. } => {
                assert_eq!(class, "renpy.ast.Say");
                let state = state.unwrap();
                match *state {
                    Value::Dict(items) => assert!(items.iter().any(|(k, v)| matches!((k, v), (Value::Str(k), Value::Str(v)) if k == "what" && v == "Hi"))),
                    other => panic!("{other:?}"),
                }
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn truncated_pickle_is_an_error() {
        let err = loads(&[0x80, 2, b'J']).unwrap_err();
        assert!(err.0.contains("ended early"), "{err}");
    }

    #[test]
    fn stack_limit_is_enforced() {
        let mut p = vec![0x80, 2];
        for _ in 0..=MAX_STACK {
            p.push(BININT1);
            p.push(1);
        }
        p.push(b'.');
        let err = loads(&p).unwrap_err();
        assert!(err.0.contains("stack is too deep"), "{err}");
    }

    #[test]
    fn oversized_long_is_rejected() {
        let mut p = vec![0x80, 2, LONG1, (MAX_LONG as u8) + 1];
        p.extend(std::iter::repeat(0u8).take(MAX_LONG + 1));
        p.push(b'.');
        let err = loads(&p).unwrap_err();
        assert!(err.0.contains("integer is too big"), "{err}");
    }

    #[test]
    fn cyclic_object_is_an_error() {
        // REDUCE a Say, memoize it, BUILD its own state from that memo.
        let mut p = vec![0x80, 2, b'c'];
        p.extend(b"renpy.ast\nSay\n");
        p.push(b')');
        p.push(b'R');
        p.push(b'q');
        p.push(1);
        p.push(b'h');
        p.push(1);
        p.push(b'b');
        p.push(b'.');
        let err = loads(&p).unwrap_err();
        assert!(err.0.contains("cycle"), "{err}");
    }

    #[test]
    fn cyclic_set_is_an_error() {
        let p = vec![0x80, 4, EMPTY_SET, b'q', 1, b'(', b'h', 1, ADDITEMS, b'.'];
        let err = loads(&p).unwrap_err();
        assert!(err.0.contains("cycle"), "{err}");
    }

    #[test]
    fn deep_tuples_hit_the_nesting_limit() {
        let mut p = vec![0x80, 2, NONE_OP];
        for _ in 0..=MAX_DEPTH {
            p.push(TUPLE1);
        }
        p.push(STOP);
        let err = loads(&p).unwrap_err();
        assert!(err.0.contains("too deep"), "{err}");
    }

    #[test]
    fn rejects_a_module_outside_the_allow_list() {
        let mut p = vec![0x80, 2, b'c'];
        p.extend(b"os\nsystem\n");
        p.push(b')');
        p.push(b'R');
        p.push(b'.');
        let err = loads(&p).unwrap_err();
        assert!(err.0.contains("not allowed"), "{err}");
    }
}
