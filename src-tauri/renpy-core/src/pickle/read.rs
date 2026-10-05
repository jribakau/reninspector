//! One pickle parser for archive indexes, compiled scripts and saves.
//!
//! Containers are reference-counted, so a memo `GET` clones a pointer rather
//! than the value. `freeze` still expands shared structure into an owned tree,
//! and stops when that output passes the policy's value budget.
//!
//! Dropping a deep value must not recurse: a million nested tuples would
//! overflow the stack. Every container type ([`Seq`], [`Pairs`], [`Obj`])
//! therefore implements `Drop` by moving its children onto a worklist, so a
//! value is freed iteratively wherever it is dropped (`POP`, an error path,
//! a replaced memo entry). Cycles never reach a reference count of zero, so
//! the parser also force-empties whatever it still holds when it is dropped.

use std::cell::RefCell;
use std::collections::HashMap;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use super::opcode::*;
use super::policy::Policy;
use super::text::{decode_encoded, decode_raw_unicode, decode_repr};
use super::{PickleError, Value};

pub(crate) const MAX_DEPTH: usize = 64;
const MAX_STACK: usize = 100_000;
const MAX_MEMO: usize = 2_000_000;
/// Slack the dense memo table may carry beyond twice the stored entries. An
/// index past that goes in the sparse map, so a few hostile `PUT`s cannot make
/// the table allocate far more empty slots than there are real entries.
const MEMO_DENSE_SLACK: usize = 1024;
const MAX_LONG: usize = 16;

struct Budget {
    objects: usize,
    bytes: usize,
    values: usize,
    out_bytes: usize,
    max_objects: usize,
    max_bytes: usize,
    max_values: usize,
    max_output_bytes: usize,
}

impl Budget {
    fn new(policy: &Policy) -> Self {
        Self {
            objects: 0,
            bytes: 0,
            values: 0,
            out_bytes: 0,
            max_objects: policy.limits.max_objects,
            max_bytes: policy.limits.max_bytes,
            max_values: policy.limits.max_values,
            max_output_bytes: policy.limits.max_output_bytes,
        }
    }

    fn obj(&mut self) -> Result<(), PickleError> {
        self.objects += 1;
        if self.objects > self.max_objects {
            return Err(PickleError::new("pickle has too many objects"));
        }
        Ok(())
    }

    fn add_bytes(&mut self, n: usize) -> Result<(), PickleError> {
        self.bytes = self.bytes.saturating_add(n);
        if self.bytes > self.max_bytes {
            return Err(PickleError::new("pickle strings are too large"));
        }
        Ok(())
    }

    fn charge_values(&mut self, n: usize) -> Result<(), PickleError> {
        self.values = self.values.saturating_add(n);
        if self.values > self.max_values {
            return Err(PickleError::new("pickle has too many values"));
        }
        Ok(())
    }

    fn add_output(&mut self, n: usize) -> Result<(), PickleError> {
        self.out_bytes = self.out_bytes.saturating_add(n);
        if self.out_bytes > self.max_output_bytes {
            return Err(PickleError::new("pickle strings are too large"));
        }
        Ok(())
    }
}

/// A value while it is still being built. Strings, bytes and containers are
/// shared with the memo. Tuples use a `RefCell` too, so teardown can empty a
/// tuple that another value still holds (a list of tuples that points back).
#[derive(Clone)]
enum Val {
    None,
    Bool(bool),
    Int(i128),
    Float(f64),
    Bytes(Rc<[u8]>),
    Str(Rc<str>),
    List(Rc<RefCell<Seq>>),
    Tuple(Rc<RefCell<Seq>>),
    Dict(Rc<RefCell<Pairs>>),
    Set(Rc<RefCell<Seq>>),
    Object(Rc<RefCell<Obj>>),
    Callable(Rc<CallableInfo>),
}

/// Decided once, when the global is first read. Later objects reuse it from the memo.
struct CallableInfo {
    full: Rc<str>,
    kind: CallKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CallKind {
    Encode,
    Reconstructor,
    Container(ContainerKind),
    Plain,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContainerKind {
    Dict,
    List,
    Set,
    Tuple,
}

/// The items of a list, tuple or set. Not `Clone`: copy `.0` when a copy is meant.
struct Seq(Vec<Val>);

/// The key and value pairs of a dict.
struct Pairs(Vec<(Val, Val)>);

#[derive(Clone)]
struct Obj {
    class: Rc<str>,
    args: Vec<Val>,
    state: Option<Val>,
}

#[cfg(test)]
thread_local! {
    static LIVE: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

/// Containers created and not yet dropped on this thread. Tests use it to
/// check that cyclic and abandoned structures are really freed.
#[cfg(test)]
pub(crate) fn live_containers() -> isize {
    LIVE.with(|c| c.get())
}

fn track(_delta: isize) {
    #[cfg(test)]
    LIVE.with(|c| c.set(c.get() + _delta));
}

impl Seq {
    fn new(items: Vec<Val>) -> Self {
        track(1);
        Seq(items)
    }
}

impl Pairs {
    fn new(items: Vec<(Val, Val)>) -> Self {
        track(1);
        Pairs(items)
    }
}

impl Deref for Seq {
    type Target = Vec<Val>;
    fn deref(&self) -> &Vec<Val> {
        &self.0
    }
}

impl DerefMut for Seq {
    fn deref_mut(&mut self) -> &mut Vec<Val> {
        &mut self.0
    }
}

impl Deref for Pairs {
    type Target = Vec<(Val, Val)>;
    fn deref(&self) -> &Vec<(Val, Val)> {
        &self.0
    }
}

impl DerefMut for Pairs {
    fn deref_mut(&mut self) -> &mut Vec<(Val, Val)> {
        &mut self.0
    }
}

impl Drop for Seq {
    fn drop(&mut self) {
        track(-1);
        if !self.0.is_empty() {
            dismantle(mem::take(&mut self.0), false);
        }
    }
}

impl Drop for Pairs {
    fn drop(&mut self) {
        track(-1);
        if !self.0.is_empty() {
            dismantle(flatten(mem::take(&mut self.0)), false);
        }
    }
}

impl Drop for Obj {
    fn drop(&mut self) {
        if self.args.is_empty() && self.state.is_none() {
            return;
        }
        let mut items = mem::take(&mut self.args);
        items.extend(self.state.take());
        dismantle(items, false);
    }
}

fn seq(items: Vec<Val>) -> Rc<RefCell<Seq>> {
    Rc::new(RefCell::new(Seq::new(items)))
}

fn dict_cell(items: Vec<(Val, Val)>) -> Rc<RefCell<Pairs>> {
    Rc::new(RefCell::new(Pairs::new(items)))
}

fn flatten(pairs: Vec<(Val, Val)>) -> Vec<Val> {
    let mut out = Vec::with_capacity(pairs.len() * 2);
    for (k, v) in pairs {
        out.push(k);
        out.push(v);
    }
    out
}

enum StackItem {
    Mark,
    Value(Val),
}

struct Parser<'a> {
    data: &'a [u8],
    i: usize,
    stack: Vec<StackItem>,
    /// Dense memo. `None` is an unused slot. Indices far past the end go in `memo_sparse`.
    memo: Vec<Option<Val>>,
    memo_sparse: HashMap<usize, Val>,
    /// Number of stored entries. Matches what `HashMap::len` used to return, so
    /// `MEMOIZE` keeps the same indices.
    memo_count: usize,
    budget: Budget,
    policy: &'a Policy,
}

pub(crate) fn parse(data: &[u8], policy: &Policy) -> Result<Value, PickleError> {
    let mut parser = Parser {
        data,
        i: 0,
        stack: Vec::new(),
        memo: Vec::new(),
        memo_sparse: HashMap::new(),
        memo_count: 0,
        budget: Budget::new(policy),
        policy,
    };
    let root = parser.parse()?;
    let mut seen = Vec::new();
    let cut_cycles = parser.policy.cut_cycles();
    // Absorb the root before propagating, so a deep or cyclic graph is freed
    // by `Parser`'s worklist and not by recursive `Drop`.
    let frozen = freeze(&root, &mut parser.budget, cut_cycles, &mut seen, 0);
    parser.absorb(root);
    frozen
}

impl Drop for Parser<'_> {
    fn drop(&mut self) {
        let mut items = Vec::new();
        for item in mem::take(&mut self.stack) {
            if let StackItem::Value(v) = item {
                items.push(v);
            }
        }
        items.extend(mem::take(&mut self.memo).into_iter().flatten());
        items.extend(mem::take(&mut self.memo_sparse).into_values());
        // Nothing the parser holds is used again. Force-empty it so a cycle,
        // which never reaches a reference count of zero, is freed too.
        dismantle(items, true);
    }
}

/// Free values without recursing. Each container's children go onto `stack`
/// and the container is left empty, so dropping it afterwards is shallow.
///
/// Without `force`, only the last owner of a container takes its children;
/// anything still shared is left to whoever else holds it. With `force`,
/// every container is emptied regardless of its count.
fn dismantle(mut stack: Vec<Val>, force: bool) {
    while let Some(v) = stack.pop() {
        match v {
            Val::List(rc) | Val::Tuple(rc) | Val::Set(rc) => {
                if force || Rc::strong_count(&rc) == 1 {
                    if let Ok(mut cell) = rc.try_borrow_mut() {
                        stack.append(&mut cell.0);
                    }
                }
            }
            Val::Dict(rc) => {
                if force || Rc::strong_count(&rc) == 1 {
                    if let Ok(mut cell) = rc.try_borrow_mut() {
                        for (k, v) in mem::take(&mut cell.0) {
                            stack.push(k);
                            stack.push(v);
                        }
                    }
                }
            }
            Val::Object(rc) => {
                if force || Rc::strong_count(&rc) == 1 {
                    if let Ok(mut obj) = rc.try_borrow_mut() {
                        stack.append(&mut obj.args);
                        if let Some(state) = obj.state.take() {
                            stack.push(state);
                        }
                    }
                }
            }
            Val::None
            | Val::Bool(_)
            | Val::Int(_)
            | Val::Float(_)
            | Val::Bytes(_)
            | Val::Str(_)
            | Val::Callable(_) => {}
        }
    }
}

impl<'a> Parser<'a> {
    fn absorb(&mut self, root: Val) {
        self.stack.push(StackItem::Value(root));
    }

    fn parse(&mut self) -> Result<Val, PickleError> {
        while self.i < self.data.len() {
            let op = self.data[self.i];
            self.i += 1;
            self.step(op)?;
            if op == STOP {
                return match self.stack.pop() {
                    Some(StackItem::Value(v)) => Ok(v),
                    _ => Err(PickleError::new("pickle STOP with an empty stack")),
                };
            }
        }
        Err(PickleError::new("pickle ended without STOP"))
    }

    fn step(&mut self, op: u8) -> Result<(), PickleError> {
        match op {
            PROTO => {
                let v = self.u8_arg()?;
                if v > 5 {
                    return Err(PickleError::new(format!("unsupported pickle protocol {v}")));
                }
            }
            FRAME => {
                let _len = self.u64_arg()?;
            }
            NONE_OP => self.push(Val::None)?,
            NEWTRUE => self.push(Val::Bool(true))?,
            NEWFALSE => self.push(Val::Bool(false))?,
            EMPTY_TUPLE => self.push(self.empty_tuple())?,
            EMPTY_LIST => {
                self.budget.obj()?;
                self.push(Val::List(seq(Vec::new())))?;
            }
            EMPTY_DICT => {
                self.budget.obj()?;
                self.push(Val::Dict(dict_cell(Vec::new())))?;
            }
            EMPTY_SET => {
                self.not_in_archive(op)?;
                self.budget.obj()?;
                self.push(Val::Set(seq(Vec::new())))?;
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
                self.not_in_archive(op)?;
                let line = self.line()?.to_string();
                let n = line
                    .parse::<f64>()
                    .map_err(|_| PickleError::new(format!("bad pickle float {line:?}")))?;
                self.push(Val::Float(n))?;
            }
            BINFLOAT => {
                self.not_in_archive(op)?;
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
                let line = self.line_latin1()?;
                let s = decode_repr(&line)?;
                self.budget.add_bytes(s.len())?;
                self.push(Val::Bytes(Rc::from(s)))?;
            }
            UNICODE => {
                let line = self.line_bytes()?;
                let s = decode_raw_unicode(line)?;
                self.budget.add_bytes(s.len())?;
                self.push(Val::Str(Rc::from(s)))?;
            }
            MARK => self.push_mark()?,
            POP => {
                self.not_in_archive(op)?;
                self.stack
                    .pop()
                    .ok_or_else(|| PickleError::new("POP on an empty stack"))?;
            }
            POP_MARK => {
                self.not_in_archive(op)?;
                self.pop_mark()?;
            }
            DUP => {
                self.not_in_archive(op)?;
                let v = match self.stack.last() {
                    Some(StackItem::Value(v)) => v.clone(),
                    _ => return Err(PickleError::new("DUP on an empty stack")),
                };
                self.push(v)?;
            }
            TUPLE1 => self.push_tuple(1)?,
            TUPLE2 => self.push_tuple(2)?,
            TUPLE3 => self.push_tuple(3)?,
            TUPLE_OP => {
                let items = self.pop_mark()?;
                self.budget.obj()?;
                self.push(self.tuple(items))?;
            }
            LIST_OP => {
                let items = self.pop_mark()?;
                self.budget.obj()?;
                self.push(Val::List(seq(items)))?;
            }
            DICT_OP => {
                let items = self.pop_mark()?;
                self.push(Val::Dict(dict_cell(pairs(items)?)))?;
            }
            FROZENSET => {
                self.not_in_archive(op)?;
                let items = self.pop_mark()?;
                self.budget.obj()?;
                self.push(Val::Set(seq(items)))?;
            }
            APPEND => self.append_one()?,
            APPENDS => self.append_many()?,
            ADDITEMS => {
                self.not_in_archive(op)?;
                self.add_items()?;
            }
            SETITEM => self.set_one()?,
            SETITEMS => self.set_many()?,
            GLOBAL => {
                let module = self.line()?;
                let name = self.line()?;
                self.push_callable(module, name)?;
            }
            STACK_GLOBAL => {
                let name = self.expect_text()?;
                let module = self.expect_text()?;
                self.push_callable(&module, &name)?;
            }
            REDUCE => self.reduce()?,
            BUILD => {
                self.not_in_archive(op)?;
                self.build()?;
            }
            NEWOBJ => {
                self.not_in_archive(op)?;
                self.newobj(false)?;
            }
            NEWOBJ_EX => {
                self.not_in_archive(op)?;
                self.newobj(true)?;
            }
            INST => {
                self.not_in_archive(op)?;
                let module = self.line()?;
                let name = self.line()?;
                self.push_callable(module, name)?;
                let class = match self.stack.last() {
                    Some(StackItem::Value(Val::Callable(info))) => Rc::clone(&info.full),
                    _ => return Err(PickleError::new("INST without a class")),
                };
                let args = self.pop_mark()?;
                self.make_object(class, args, None)?;
            }
            OBJ => {
                self.not_in_archive(op)?;
                let args = self.pop_mark()?;
                if args.is_empty() {
                    return Err(PickleError::new("OBJ without a class"));
                }
                let class = match &args[0] {
                    Val::Callable(info) => Rc::clone(&info.full),
                    _ => return Err(PickleError::new("OBJ class is not a global")),
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
                let idx = self.memo_count;
                self.memo_put(idx)?;
            }
            BINGET => {
                let idx = self.u8_arg()? as usize;
                self.memo_get(idx)?;
            }
            LONG_BINGET => {
                let n = self.read(4)?;
                self.memo_get(u32::from_le_bytes([n[0], n[1], n[2], n[3]]) as usize)?;
            }
            GET => {
                let idx = parse_usize(self.line()?)?;
                self.memo_get(idx)?;
            }
            STOP => {}
            other => {
                return Err(PickleError::new(format!(
                    "unsupported pickle opcode 0x{other:02x}"
                )));
            }
        }
        Ok(())
    }

    fn not_in_archive(&self, op: u8) -> Result<(), PickleError> {
        if self.policy.encode_only() {
            Err(PickleError::new(format!(
                "unsupported pickle opcode 0x{op:02x}"
            )))
        } else {
            Ok(())
        }
    }

    fn push_callable(&mut self, module: &str, name: &str) -> Result<(), PickleError> {
        match self.policy.globals {
            super::policy::Globals::EncodeOnly => {
                if module != "_codecs" || name != "encode" {
                    return Err(PickleError::new(format!(
                        "pickle global {module}.{name} is not allowed"
                    )));
                }
            }
            super::policy::Globals::Allowlist => {
                if !allowed_module(module) {
                    return Err(PickleError::new(format!(
                        "pickle global {module}.{name} is not allowed"
                    )));
                }
            }
            super::policy::Globals::Inert => {}
        }
        let full: Rc<str> = Rc::from(format!("{module}.{name}"));
        self.push(Val::Callable(Rc::new(CallableInfo {
            kind: classify(module, name),
            full,
        })))
    }

    fn reduce(&mut self) -> Result<(), PickleError> {
        let args = self.pop_val()?;
        let func = self.pop_val()?;
        let Val::Callable(func) = func else {
            return Err(PickleError::new("REDUCE of a non-callable"));
        };
        let args = self.tuple_items(args)?;
        if func.kind == CallKind::Encode {
            return self.reduce_encode(args);
        }
        if self.policy.encode_only() {
            return Err(PickleError::new(format!(
                "pickle global {} is not allowed",
                func.full
            )));
        }
        match func.kind {
            CallKind::Reconstructor => {
                let class = match args.first() {
                    Some(Val::Callable(class)) => Rc::clone(&class.full),
                    _ => return Err(PickleError::new("reconstructor is missing its class")),
                };
                self.make_object(class, Vec::new(), None)
            }
            CallKind::Container(kind) => self.reduce_container(kind, args),
            CallKind::Encode | CallKind::Plain => {
                self.make_object(Rc::clone(&func.full), args, None)
            }
        }
    }

    /// Archive indexes only ever REDUCE a tuple. Scripts sometimes pass one value.
    fn tuple_items(&mut self, args: Val) -> Result<Vec<Val>, PickleError> {
        match args {
            Val::Tuple(items) => self.take_shared(&items),
            _ if self.policy.encode_only() => {
                Err(PickleError::new("_codecs.encode arguments are not a tuple"))
            }
            other => Ok(vec![other]),
        }
    }

    fn take_shared(&mut self, items: &Rc<RefCell<Seq>>) -> Result<Vec<Val>, PickleError> {
        if Rc::strong_count(items) == 1 {
            return Ok(mem::take(&mut items.borrow_mut().0));
        }
        let cloned = items.borrow().0.clone();
        self.budget.charge_values(cloned.len())?;
        Ok(cloned)
    }

    fn reduce_encode(&mut self, items: Vec<Val>) -> Result<(), PickleError> {
        if items.len() < 2 {
            return Err(PickleError::new(
                "_codecs.encode needs a string and an encoding",
            ));
        }
        let text = as_text(&items[0])
            .ok_or_else(|| PickleError::new("_codecs.encode text is not a string"))?;
        let encoding = as_text(&items[1])
            .ok_or_else(|| PickleError::new("_codecs.encode encoding is not a string"))?;
        let bytes = decode_encoded(&text, &encoding)?;
        self.budget.add_bytes(bytes.len())?;
        self.push(Val::Bytes(Rc::from(bytes)))
    }

    fn reduce_container(&mut self, kind: ContainerKind, args: Vec<Val>) -> Result<(), PickleError> {
        self.budget.obj()?;
        match kind {
            ContainerKind::Dict => {
                let dict = dict_cell(Vec::new());
                if let Some(Val::Dict(items)) = args.first() {
                    let cloned = items.borrow().0.clone();
                    self.budget.charge_values(cloned.len())?;
                    dict.borrow_mut().extend(cloned);
                }
                self.push(Val::Dict(dict))
            }
            ContainerKind::List => {
                let items = self.clone_seq(args.first())?;
                self.push(Val::List(seq(items)))
            }
            ContainerKind::Set => {
                let items = self.clone_seq(args.first())?;
                self.push(Val::Set(seq(items)))
            }
            ContainerKind::Tuple => self.push(self.tuple(args)),
        }
    }

    fn clone_seq(&mut self, v: Option<&Val>) -> Result<Vec<Val>, PickleError> {
        let Some(v) = v else {
            return Ok(Vec::new());
        };
        let cloned = match v {
            Val::List(list) => list.borrow().0.clone(),
            Val::Tuple(items) => items.borrow().0.clone(),
            Val::Set(set) => set.borrow().0.clone(),
            _ => return Ok(Vec::new()),
        };
        self.budget.charge_values(cloned.len())?;
        Ok(cloned)
    }

    fn build(&mut self) -> Result<(), PickleError> {
        let state = self.pop_val()?;
        let inert = self.policy.inert();
        match self.stack.last() {
            Some(StackItem::Value(Val::Object(obj))) => obj.borrow_mut().state = Some(state),
            Some(StackItem::Value(Val::Dict(dict))) => {
                if let Val::Dict(extra) = &state {
                    let cloned = extra.borrow().0.clone();
                    self.budget.charge_values(cloned.len())?;
                    dict.borrow_mut().extend(cloned);
                }
            }
            Some(StackItem::Value(_)) if inert => {}
            _ => return Err(PickleError::new("BUILD without an object")),
        }
        Ok(())
    }

    fn newobj(&mut self, with_kwargs: bool) -> Result<(), PickleError> {
        if with_kwargs {
            self.pop_val()?;
        }
        let args = self.pop_val()?;
        let class = self.pop_val()?;
        let Val::Callable(class) = class else {
            return Err(PickleError::new("NEWOBJ class is not a global"));
        };
        let args = self.tuple_items(args)?;
        if let CallKind::Container(kind) = class.kind {
            return self.reduce_container(kind, args);
        }
        self.make_object(Rc::clone(&class.full), args, None)
    }

    fn make_object(
        &mut self,
        class: Rc<str>,
        args: Vec<Val>,
        state: Option<Val>,
    ) -> Result<(), PickleError> {
        self.budget.obj()?;
        self.push(Val::Object(Rc::new(RefCell::new(Obj {
            class,
            args,
            state,
        }))))
    }

    fn append_one(&mut self) -> Result<(), PickleError> {
        let v = self.pop_val()?;
        let inert = self.policy.inert();
        match self.stack.last() {
            Some(StackItem::Value(Val::List(list))) => list.borrow_mut().push(v),
            Some(StackItem::Value(Val::Object(obj))) if inert => obj.borrow_mut().args.push(v),
            _ => return Err(PickleError::new("APPEND without a list")),
        }
        Ok(())
    }

    fn append_many(&mut self) -> Result<(), PickleError> {
        let items = self.pop_mark()?;
        let inert = self.policy.inert();
        match self.stack.last() {
            Some(StackItem::Value(Val::List(list))) => list.borrow_mut().extend(items),
            Some(StackItem::Value(Val::Object(obj))) if inert => {
                obj.borrow_mut().args.extend(items)
            }
            _ => return Err(PickleError::new("APPENDS without a list")),
        }
        Ok(())
    }

    fn add_items(&mut self) -> Result<(), PickleError> {
        let items = self.pop_mark()?;
        let inert = self.policy.inert();
        match self.stack.last() {
            Some(StackItem::Value(Val::Set(set))) => set.borrow_mut().extend(items),
            Some(StackItem::Value(Val::Object(obj))) if inert => {
                obj.borrow_mut().args.extend(items)
            }
            _ => return Err(PickleError::new("ADDITEMS without a set")),
        }
        Ok(())
    }

    fn set_one(&mut self) -> Result<(), PickleError> {
        let value = self.pop_val()?;
        let key = self.pop_val()?;
        if matches!(self.stack.last(), Some(StackItem::Value(Val::Dict(_)))) {
            if let Some(StackItem::Value(Val::Dict(dict))) = self.stack.last() {
                dict.borrow_mut().push((key, value));
            }
            return Ok(());
        }
        if self.policy.inert()
            && matches!(self.stack.last(), Some(StackItem::Value(Val::Object(_))))
        {
            let pair = self.tuple(vec![key, value]);
            if let Some(StackItem::Value(Val::Object(obj))) = self.stack.last() {
                obj.borrow_mut().args.push(pair);
            }
            return Ok(());
        }
        Err(PickleError::new("SETITEM without a dict"))
    }

    fn set_many(&mut self) -> Result<(), PickleError> {
        let items = self.pop_mark()?;
        let extra = pairs(items)?;
        if matches!(self.stack.last(), Some(StackItem::Value(Val::Dict(_)))) {
            if let Some(StackItem::Value(Val::Dict(dict))) = self.stack.last() {
                dict.borrow_mut().extend(extra);
            }
            return Ok(());
        }
        if self.policy.inert()
            && matches!(self.stack.last(), Some(StackItem::Value(Val::Object(_))))
        {
            let pairs = extra
                .into_iter()
                .map(|(k, v)| self.tuple(vec![k, v]))
                .collect::<Vec<_>>();
            if let Some(StackItem::Value(Val::Object(obj))) = self.stack.last() {
                obj.borrow_mut().args.extend(pairs);
            }
            return Ok(());
        }
        Err(PickleError::new("SETITEMS without a dict"))
    }

    fn push_long(&mut self, n: usize) -> Result<(), PickleError> {
        if n > MAX_LONG {
            return Err(PickleError::new("pickle integer is too big"));
        }
        let bytes = self.read(n)?;
        self.push(Val::Int(decode_long(bytes)?))
    }

    fn push_bytes(&mut self, n: usize) -> Result<(), PickleError> {
        let bytes = self.read(n)?;
        self.budget.add_bytes(bytes.len())?;
        self.push(Val::Bytes(Rc::from(bytes)))
    }

    fn push_utf8(&mut self, n: usize) -> Result<(), PickleError> {
        let bytes = self.read(n)?;
        self.budget.add_bytes(bytes.len())?;
        let s = std::str::from_utf8(bytes)
            .map_err(|_| PickleError::new("pickle string is not utf-8"))?;
        self.push(Val::Str(Rc::from(s)))
    }

    fn push_text_int(&mut self) -> Result<(), PickleError> {
        let line = self.line()?;
        if line == "01" {
            return self.push(Val::Bool(true));
        }
        if line == "00" {
            return self.push(Val::Bool(false));
        }
        let n = line
            .parse::<i128>()
            .map_err(|_| PickleError::new(format!("bad pickle int {line:?}")))?;
        self.push(Val::Int(n))
    }

    fn push_text_long(&mut self) -> Result<(), PickleError> {
        let line = self.line()?;
        let digits = line.strip_suffix('L').unwrap_or(line);
        let n = digits
            .parse::<i128>()
            .map_err(|_| PickleError::new(format!("bad pickle long {line:?}")))?;
        self.push(Val::Int(n))
    }

    fn push_tuple(&mut self, n: usize) -> Result<(), PickleError> {
        if self.stack.len() < n {
            return Err(PickleError::new("pickle stack underflow"));
        }
        let start = self.stack.len() - n;
        let mut items = Vec::with_capacity(n);
        for item in self.stack.drain(start..) {
            match item {
                StackItem::Value(v) => items.push(v),
                StackItem::Mark => return Err(PickleError::new("tuple built across a mark")),
            }
        }
        self.budget.obj()?;
        self.push(self.tuple(items))
    }

    fn empty_tuple(&self) -> Val {
        self.tuple(Vec::new())
    }

    fn tuple(&self, items: Vec<Val>) -> Val {
        Val::Tuple(seq(items))
    }

    fn pop_mark(&mut self) -> Result<Vec<Val>, PickleError> {
        let mut items = Vec::new();
        loop {
            match self.stack.pop() {
                Some(StackItem::Mark) => {
                    items.reverse();
                    return Ok(items);
                }
                Some(StackItem::Value(v)) => items.push(v),
                None => return Err(PickleError::new("pickle mark is missing")),
            }
        }
    }

    fn pop_val(&mut self) -> Result<Val, PickleError> {
        match self.stack.pop() {
            Some(StackItem::Value(v)) => Ok(v),
            _ => Err(PickleError::new("pickle stack underflow")),
        }
    }

    fn expect_text(&mut self) -> Result<String, PickleError> {
        as_text(&self.pop_val()?).ok_or_else(|| PickleError::new("global name is not text"))
    }

    fn memo_put(&mut self, idx: usize) -> Result<(), PickleError> {
        if self.memo_count >= MAX_MEMO {
            return Err(PickleError::new("pickle memo is too large"));
        }
        let val = match self.stack.last() {
            Some(StackItem::Value(v)) => v.clone(),
            _ => return Err(PickleError::new("PUT with an empty stack")),
        };
        if idx < self.memo.len() {
            self.store_dense(idx, val);
            return Ok(());
        }
        let dense_limit = self
            .memo_count
            .saturating_mul(2)
            .saturating_add(MEMO_DENSE_SLACK);
        if idx < MAX_MEMO && idx <= dense_limit {
            let new_len = idx + 1;
            self.memo.resize_with(new_len, || None);
            self.migrate_sparse(new_len);
            self.store_dense(idx, val);
            return Ok(());
        }
        if self.memo_sparse.insert(idx, val).is_none() {
            self.memo_count += 1;
        }
        Ok(())
    }

    fn store_dense(&mut self, idx: usize, val: Val) {
        let replaced = self.memo[idx].is_some()
            || (!self.memo_sparse.is_empty() && self.memo_sparse.remove(&idx).is_some());
        if !replaced {
            self.memo_count += 1;
        }
        self.memo[idx] = Some(val);
    }

    /// Move sparse entries that the dense table now covers, so a later get
    /// finds them in the vec. Their count was already recorded.
    fn migrate_sparse(&mut self, new_len: usize) {
        if self.memo_sparse.is_empty() {
            return;
        }
        let covered: Vec<usize> = self
            .memo_sparse
            .keys()
            .copied()
            .filter(|k| *k < new_len)
            .collect();
        for k in covered {
            if let Some(existing) = self.memo_sparse.remove(&k) {
                self.memo[k] = Some(existing);
            }
        }
    }

    fn memo_get(&mut self, idx: usize) -> Result<(), PickleError> {
        if let Some(val) = self.memo.get(idx).and_then(Clone::clone) {
            return self.push(val);
        }
        let val = self
            .memo_sparse
            .get(&idx)
            .cloned()
            .ok_or_else(|| PickleError::new(format!("pickle memo {idx} is missing")))?;
        self.push(val)
    }

    fn push(&mut self, v: Val) -> Result<(), PickleError> {
        if self.stack.len() >= MAX_STACK {
            return Err(PickleError::new("pickle stack is too deep"));
        }
        self.stack.push(StackItem::Value(v));
        Ok(())
    }

    fn push_mark(&mut self) -> Result<(), PickleError> {
        if self.stack.len() >= MAX_STACK {
            return Err(PickleError::new("pickle stack is too deep"));
        }
        self.stack.push(StackItem::Mark);
        Ok(())
    }

    fn u8_arg(&mut self) -> Result<u8, PickleError> {
        Ok(self.read(1)?[0])
    }

    fn u32_len(&mut self) -> Result<usize, PickleError> {
        let n = self.read(4)?;
        let n = u32::from_le_bytes([n[0], n[1], n[2], n[3]]) as usize;
        if n > self.budget.max_bytes {
            return Err(PickleError::new("pickle string is too large"));
        }
        Ok(n)
    }

    fn u64_len(&mut self) -> Result<usize, PickleError> {
        let n = self.u64_arg()?;
        if n > self.budget.max_bytes as u64 {
            return Err(PickleError::new("pickle string is too large"));
        }
        usize::try_from(n).map_err(|_| PickleError::new("pickle length does not fit"))
    }

    fn u64_arg(&mut self) -> Result<u64, PickleError> {
        let n = self.read(8)?;
        Ok(u64::from_le_bytes([
            n[0], n[1], n[2], n[3], n[4], n[5], n[6], n[7],
        ]))
    }

    fn read(&mut self, n: usize) -> Result<&'a [u8], PickleError> {
        let end = self
            .i
            .checked_add(n)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| PickleError::new("pickle ended early"))?;
        let out = &self.data[self.i..end];
        self.i = end;
        Ok(out)
    }

    fn line_bytes(&mut self) -> Result<&'a [u8], PickleError> {
        let start = self.i;
        while self.i < self.data.len() && self.data[self.i] != b'\n' {
            self.i += 1;
        }
        if self.i >= self.data.len() {
            return Err(PickleError::new("pickle line ended early"));
        }
        let bytes = &self.data[start..self.i];
        self.i += 1;
        Ok(bytes)
    }

    fn line(&mut self) -> Result<&'a str, PickleError> {
        let bytes = self.line_bytes()?;
        std::str::from_utf8(bytes).map_err(|_| PickleError::new("pickle text is not utf-8"))
    }

    fn line_latin1(&mut self) -> Result<String, PickleError> {
        let bytes = self.line_bytes()?;
        Ok(bytes.iter().map(|b| char::from(*b)).collect())
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
}

fn classify(module: &str, name: &str) -> CallKind {
    if module == "_codecs" && name == "encode" {
        return CallKind::Encode;
    }
    if name == "_reconstructor" && (module == "copyreg" || module == "copy_reg") {
        return CallKind::Reconstructor;
    }
    let lower = name.to_ascii_lowercase();
    let container = (module == "collections" && name == "defaultdict")
        || lower.contains("revertabledict")
        || lower.contains("revertablelist")
        || lower.contains("revertableset")
        || ((module == "builtins" || module == "__builtin__")
            && matches!(name, "list" | "dict" | "set" | "frozenset" | "tuple"));
    if !container {
        return CallKind::Plain;
    }
    if lower.contains("dict") || lower == "defaultdict" {
        CallKind::Container(ContainerKind::Dict)
    } else if lower.contains("list") {
        CallKind::Container(ContainerKind::List)
    } else if lower.contains("set") {
        CallKind::Container(ContainerKind::Set)
    } else {
        CallKind::Container(ContainerKind::Tuple)
    }
}

fn as_text(v: &Val) -> Option<String> {
    match v {
        Val::Str(s) => Some(s.to_string()),
        Val::Bytes(b) => String::from_utf8(b.to_vec()).ok(),
        _ => None,
    }
}

fn pairs(items: Vec<Val>) -> Result<Vec<(Val, Val)>, PickleError> {
    if items.len() % 2 != 0 {
        return Err(PickleError::new("pickle dict has an odd number of items"));
    }
    let mut out = Vec::with_capacity(items.len() / 2);
    let mut it = items.into_iter();
    while let Some(k) = it.next() {
        let Some(v) = it.next() else {
            break;
        };
        out.push((k, v));
    }
    Ok(out)
}

fn decode_long(bytes: &[u8]) -> Result<i128, PickleError> {
    if bytes.is_empty() {
        return Ok(0);
    }
    if bytes.len() > MAX_LONG {
        return Err(PickleError::new("pickle integer is too big"));
    }
    let mut acc: i128 = 0;
    for (i, b) in bytes.iter().enumerate() {
        acc |= i128::from(*b) << (8 * i);
    }
    let bits = bytes.len() * 8;
    if bits < 128 && bytes.last().copied().unwrap_or(0) & 0x80 != 0 {
        let shift = 128 - bits;
        acc = (acc << shift) >> shift;
    }
    Ok(acc)
}

fn parse_usize(s: &str) -> Result<usize, PickleError> {
    s.parse()
        .map_err(|_| PickleError::new(format!("bad pickle memo index {s:?}")))
}

fn is_back_edge(ptr: *const (), seen: &mut Vec<*const ()>) -> bool {
    if seen.contains(&ptr) {
        return true;
    }
    seen.push(ptr);
    false
}

fn freeze(
    v: &Val,
    budget: &mut Budget,
    cut_cycles: bool,
    seen: &mut Vec<*const ()>,
    depth: usize,
) -> Result<Value, PickleError> {
    if depth > MAX_DEPTH {
        return Err(PickleError::new("pickle nesting is too deep"));
    }
    budget.charge_values(1)?;
    match v {
        Val::None => Ok(Value::None),
        Val::Bool(b) => Ok(Value::Bool(*b)),
        Val::Int(n) => Ok(Value::Int(*n)),
        Val::Float(n) => Ok(Value::Float(*n)),
        Val::Bytes(b) => {
            budget.add_output(b.len())?;
            Ok(Value::Bytes(b.to_vec()))
        }
        Val::Str(s) => {
            budget.add_output(s.len())?;
            Ok(Value::Str(s.to_string()))
        }
        Val::Tuple(items) => freeze_seq(items, "tuple", budget, cut_cycles, seen, depth),
        Val::List(items) => freeze_seq(items, "list", budget, cut_cycles, seen, depth),
        Val::Set(items) => freeze_seq(items, "set", budget, cut_cycles, seen, depth),
        Val::Dict(dict) => {
            let ptr = Rc::as_ptr(dict) as *const ();
            if is_back_edge(ptr, seen) {
                return cut_or_cycle(cut_cycles, "dict");
            }
            let pairs = dict.borrow();
            let mut out = Vec::with_capacity(pairs.len());
            for (k, child) in pairs.iter() {
                out.push((
                    freeze(k, budget, cut_cycles, seen, depth + 1)?,
                    freeze(child, budget, cut_cycles, seen, depth + 1)?,
                ));
            }
            drop(pairs);
            seen.pop();
            Ok(Value::Dict(out))
        }
        Val::Object(obj) => {
            let ptr = Rc::as_ptr(obj) as *const ();
            if is_back_edge(ptr, seen) {
                return cut_or_cycle(cut_cycles, "object");
            }
            let obj = obj.borrow();
            let class = obj.class.to_string();
            let mut args = Vec::with_capacity(obj.args.len());
            for arg in &obj.args {
                args.push(freeze(arg, budget, cut_cycles, seen, depth + 1)?);
            }
            let state = match &obj.state {
                Some(s) => Some(Box::new(freeze(s, budget, cut_cycles, seen, depth + 1)?)),
                None => None,
            };
            drop(obj);
            seen.pop();
            Ok(Value::Object { class, args, state })
        }
        Val::Callable(info) => {
            if cut_cycles {
                Ok(Value::Str(info.full.to_string()))
            } else {
                Err(PickleError::new(format!(
                    "pickle global {} was not called",
                    info.full
                )))
            }
        }
    }
}

fn freeze_seq(
    items: &Rc<RefCell<Seq>>,
    kind: &str,
    budget: &mut Budget,
    cut_cycles: bool,
    seen: &mut Vec<*const ()>,
    depth: usize,
) -> Result<Value, PickleError> {
    let ptr = Rc::as_ptr(items) as *const ();
    if is_back_edge(ptr, seen) {
        return cut_or_cycle(cut_cycles, kind);
    }
    let borrowed = items.borrow();
    let mut out = Vec::with_capacity(borrowed.len());
    for child in borrowed.iter() {
        out.push(freeze(child, budget, cut_cycles, seen, depth + 1)?);
    }
    drop(borrowed);
    seen.pop();
    Ok(match kind {
        "tuple" => Value::Tuple(out),
        "set" => Value::Set(out),
        _ => Value::List(out),
    })
}

fn cut_or_cycle(cut_cycles: bool, kind: &str) -> Result<Value, PickleError> {
    if cut_cycles {
        Ok(Value::None)
    } else {
        Err(PickleError::new(format!("pickle {kind} is cyclic")))
    }
}

#[cfg(test)]
mod memo_tests {
    use super::*;

    #[test]
    fn hostile_indices_do_not_inflate_the_dense_table() {
        let policy = &Policy::SCRIPT;
        let mut p = Parser {
            data: &[],
            i: 0,
            stack: vec![StackItem::Value(Val::None)],
            memo: Vec::new(),
            memo_sparse: HashMap::new(),
            memo_count: 0,
            budget: Budget::new(policy),
            policy,
        };
        // Each index sits one step past the last, as a hostile pickle would.
        for k in 1..=1500usize {
            p.memo_put(k * 1024).unwrap();
        }
        assert!(
            p.memo.len() <= p.memo_count * 2 + MEMO_DENSE_SLACK + 1,
            "dense table has {} slots for {} entries",
            p.memo.len(),
            p.memo_count
        );
        for k in 1..=1500usize {
            p.memo_get(k * 1024).unwrap();
        }
    }
}
