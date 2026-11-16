//! The model checker (SPEC.md §2, §10): every (state, context) pair a flow can reach, found breadth first, and the
//! guards, effects and assertions it is held to. The port of model.ts.
//!
//! A context is held as one small number per variable: its value's place in that variable's table of values (`Ctx`).
//! The largest flows reach 17 million pairs of two dozen variables, which a value per variable would not fit in memory
//! for. The model reads a context's values back (`value_at`, `context`), and `Shown` writes a structure that holds
//! contexts with their values, in the shape the TypeScript objects have.

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};

use crate::error::{Error, ErrorKind, ErrorPlace, Result};
use crate::expr::{Expr, Op, parse_expr};
use crate::graph::{Flow, Transition, alternatives, transition_path};
use crate::js::{self, Object, Value};

/// `new ModelError(message, place)`: a flow that cannot be explored; `place` says where in flow.yaml, when it is known
/// (SPEC.md §10)
pub fn model_error(message: impl Into<String>, place: Option<ErrorPlace>) -> Error {
    Error { kind: ErrorKind::Model, message: message.into(), place, line_pos: None, code: None }
}

fn in_flow(path: Vec<String>) -> ErrorPlace {
    ErrorPlace { file: "flow.yaml".into(), path: Some(path), line: None, column: None }
}

fn path_of(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// a value a context holds, as JavaScript holds it: a list or a map read from flow.yaml is equal (`===`) only to
/// itself, as an object is
#[derive(Debug, Clone)]
pub enum Scalar {
    /// a variable that has no `initial`: JavaScript's undefined
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(Rc<str>),
    Other(Rc<Value>),
}

impl Scalar {
    /// a value read from a graph file; a list or a map becomes a value of its own, equal to no other
    pub fn of(v: &Value) -> Scalar {
        match v {
            Value::Null => Scalar::Null,
            Value::Bool(b) => Scalar::Bool(*b),
            Value::Number(n) => Scalar::Number(*n),
            Value::String(s) => Scalar::String(Rc::from(s.as_str())),
            other => Scalar::Other(Rc::new(other.clone())),
        }
    }
    /// `x[key]` read as a value: undefined when the key is not there
    pub fn of_option(v: Option<&Value>) -> Scalar {
        v.map_or(Scalar::Undefined, Scalar::of)
    }
    fn prim(&self) -> Prim<'_> {
        match self {
            Scalar::Undefined => Prim::Undefined,
            Scalar::Null => Prim::Null,
            Scalar::Bool(b) => Prim::Bool(*b),
            Scalar::Number(n) => Prim::Number(*n),
            Scalar::String(s) => Prim::Str(s),
            Scalar::Other(v) => Prim::Other(Rc::as_ptr(v)),
        }
    }
    /// `a === b`
    pub fn strict_eq(&self, other: &Scalar) -> bool {
        self.prim().strict_eq(&other.prim())
    }
    /// `this === v`, `v` a value of a graph file: a list or a map of a file is never this one
    pub fn is(&self, v: &Value) -> bool {
        self.prim().eq_value(v)
    }
    /// SameValueZero, the equality of `includes` and of a Map's keys: `===`, and NaN is NaN
    pub fn same_value_zero(&self, v: &Value) -> bool {
        match (self, v) {
            (Scalar::Number(a), Value::Number(b)) => a == b || (a.is_nan() && b.is_nan()),
            _ => self.is(v),
        }
    }
    /// JavaScript truthiness
    pub fn truthy(&self) -> bool {
        self.prim().truthy()
    }
    /// the value as a graph file holds it; undefined, which no file holds, is null
    pub fn to_value(&self) -> Value {
        match self {
            Scalar::Undefined | Scalar::Null => Value::Null,
            Scalar::Bool(b) => Value::Bool(*b),
            Scalar::Number(n) => Value::Number(*n),
            Scalar::String(s) => Value::String(s.to_string()),
            Scalar::Other(v) => (**v).clone(),
        }
    }
    /// `String(v)`, and a template literal's `${v}`
    pub fn text(&self) -> String {
        match self {
            Scalar::Undefined => "undefined".to_string(),
            Scalar::String(s) => s.to_string(),
            other => js::to_js_string(&other.to_value()),
        }
    }
    /// the value as `join` writes it: null and undefined are empty
    pub fn written(&self) -> String {
        match self {
            Scalar::Undefined | Scalar::Null => String::new(),
            other => other.text(),
        }
    }
    /// SameValue: the identity a context keeps (NaN is NaN, 0 is not -0)
    fn same(&self) -> Same {
        match self {
            Scalar::Undefined => Same::Undefined,
            Scalar::Null => Same::Null,
            Scalar::Bool(b) => Same::Bool(*b),
            Scalar::Number(n) => Same::Number(if n.is_nan() { f64::NAN.to_bits() } else { n.to_bits() }),
            Scalar::String(s) => Same::String(s.clone()),
            Scalar::Other(v) => Same::Other(Rc::as_ptr(v) as usize),
        }
    }
}

impl Serialize for Scalar {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Scalar::Undefined | Scalar::Null => s.serialize_unit(),
            Scalar::Bool(b) => s.serialize_bool(*b),
            Scalar::Number(n) => Value::Number(*n).serialize(s),
            Scalar::String(x) => s.serialize_str(x),
            Scalar::Other(v) => v.serialize(s),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Same {
    Undefined,
    Null,
    Bool(bool),
    Number(u64),
    String(Rc<str>),
    Other(usize),
}

/// a value as `===` and truthiness read it
#[derive(Debug, Clone, Copy)]
enum Prim<'a> {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    Str(&'a str),
    Other(*const Value),
    /// a list an expression builds: a new array, equal to nothing
    List,
}

impl Prim<'_> {
    fn strict_eq(&self, other: &Prim<'_>) -> bool {
        match (self, other) {
            (Prim::Undefined, Prim::Undefined) | (Prim::Null, Prim::Null) => true,
            (Prim::Bool(a), Prim::Bool(b)) => a == b,
            (Prim::Number(a), Prim::Number(b)) => a == b,
            (Prim::Str(a), Prim::Str(b)) => a == b,
            (Prim::Other(a), Prim::Other(b)) => std::ptr::eq(*a, *b),
            _ => false,
        }
    }
    fn eq_value(&self, v: &Value) -> bool {
        match (self, v) {
            (Prim::Null, Value::Null) => true,
            (Prim::Bool(a), Value::Bool(b)) => a == b,
            (Prim::Number(a), Value::Number(b)) => a == b,
            (Prim::Str(a), Value::String(b)) => *a == b.as_str(),
            _ => false,
        }
    }
    fn truthy(&self) -> bool {
        match self {
            Prim::Undefined | Prim::Null => false,
            Prim::Bool(b) => *b,
            Prim::Number(n) => *n != 0.0 && !n.is_nan(),
            Prim::Str(s) => !s.is_empty(),
            Prim::Other(_) | Prim::List => true,
        }
    }
}

/// a context: for each variable of the model, its value's place in the variable's table (`Model::value_at`)
pub type Ctx = Box<[u16]>;

/// one step out of a (state, context) pair
#[derive(Debug, Clone)]
pub struct Step {
    pub event: Option<Rc<str>>,
    pub target: Rc<str>,
    pub ctx: Ctx,
    /// `${state}|${event or "always"}|${alternative index}`
    pub taken: Rc<str>,
}

/// a (state, context) pair the exploration reached: `parent` is the pair it was reached from (None for a start),
/// `event` the event that led here
#[derive(Debug, Clone)]
pub struct Config {
    pub state: Rc<str>,
    pub ctx: Ctx,
    pub parent: Option<u32>,
    pub event: Option<Rc<str>>,
    /// the state's position in flow.states (NO_STATE when it is not one)
    pub(crate) at: u32,
}

const NO_STATE: u32 = u32::MAX;

impl Config {
    /// `parent` as an index into `Exploration::configs`
    pub fn parent_index(&self) -> Option<usize> {
        self.parent.map(|p| p as usize)
    }
}

// an expression made ready to run against one (state, context) pair: what evaluate(parseExpr(src), lookup) gave, with
// the same errors, raised only when the name is reached
#[derive(Debug)]
enum Compiled {
    Lit(Scalar),
    State,
    Var(usize),
    /// a guard read by name, compiled when it is first reached, as the lookup read it
    Guard(Box<Named>),
    Unknown {
        name: String,
        place: Option<ErrorPlace>,
    },
    TooDeep {
        name: String,
        place: Option<ErrorPlace>,
    },
    List(Vec<Compiled>),
    Not(Box<Compiled>),
    And(Box<Compiled>, Box<Compiled>),
    Or(Box<Compiled>, Box<Compiled>),
    Eq(Box<Compiled>, Box<Compiled>),
    Ne(Box<Compiled>, Box<Compiled>),
    /// `in` / `not in` a list of literals, the usual right side: the same list every time, made once
    InFixed {
        l: Box<Compiled>,
        fixed: Vec<Scalar>,
        negate: bool,
    },
    In {
        l: Box<Compiled>,
        r: Box<Compiled>,
        negate: bool,
    },
    /// a comparison of one variable with literals (`x == 'a'`, `x != 1`, `x in ['a', 'b']`), the usual guard: its
    /// answer for each value of the variable's table, read once, and the comparison itself for a value no table holds
    VarIs {
        var: usize,
        hits: Vec<bool>,
        slow: Box<Compiled>,
    },
}

/// a guard an expression names: its source, how deep it is read, its place in flow.yaml, and what it compiles to
#[derive(Debug)]
struct Named {
    src: Value,
    depth: usize,
    at: ErrorPlace,
    inner: OnceCell<Result<Rc<Compiled>>>,
}

/// a value an expression gives: a context's value, the state, a boolean, or a list it builds
#[derive(Debug, Clone)]
enum Val<'a> {
    Ref(&'a Scalar),
    Own(Scalar),
    State(&'a str),
    Bool(bool),
    List(Vec<Val<'a>>),
}

impl Val<'_> {
    fn prim(&self) -> Prim<'_> {
        match self {
            Val::Ref(s) => s.prim(),
            Val::Own(s) => s.prim(),
            Val::State(s) => Prim::Str(s),
            Val::Bool(b) => Prim::Bool(*b),
            Val::List(_) => Prim::List,
        }
    }
    fn truthy(&self) -> bool {
        self.prim().truthy()
    }
    fn strict_eq(&self, other: &Val<'_>) -> bool {
        self.prim().strict_eq(&other.prim())
    }
    /// `Array.isArray(this) && this.some((y) => y === x)`
    fn holds(&self, x: &Val<'_>) -> bool {
        let x = x.prim();
        match self {
            Val::List(items) => items.iter().any(|y| y.prim().strict_eq(&x)),
            Val::Ref(Scalar::Other(v)) | Val::Own(Scalar::Other(v)) => {
                v.as_array().is_some_and(|items| items.iter().any(|y| x.eq_value(y)))
            }
            _ => false,
        }
    }
    fn to_value(&self) -> Value {
        match self {
            Val::Ref(s) => s.to_value(),
            Val::Own(s) => s.to_value(),
            Val::State(s) => Value::String(s.to_string()),
            Val::Bool(b) => Value::Bool(*b),
            Val::List(items) => Value::Array(items.iter().map(Val::to_value).collect()),
        }
    }
    fn to_scalar(&self) -> Scalar {
        match self {
            Val::Ref(s) => (*s).clone(),
            Val::Own(s) => s.clone(),
            Val::State(s) => Scalar::String(Rc::from(*s)),
            Val::Bool(b) => Scalar::Bool(*b),
            Val::List(_) => Scalar::Other(Rc::new(self.to_value())),
        }
    }
}

/// the expression a graph gives as a value. The TS parser read whatever it was handed: a number or a boolean has no
/// length, so no token was read and "unexpected end" was thrown; null threw a TypeError, worded here as Node words it;
/// a list or a map, which it read in ways of its own, is read here as "unexpected end"
fn parse_source(v: Option<&Value>) -> std::result::Result<Rc<Expr>, String> {
    match v {
        Some(Value::String(s)) => parse_expr(s),
        None => Err("Cannot read properties of undefined (reading 'length')".to_string()),
        Some(Value::Null) => Err("Cannot read properties of null (reading 'length')".to_string()),
        Some(other) => Err(format!("unexpected end in: {}", js::to_js_string(other))),
    }
}

// an effect made ready to apply: the variables it sets, and the ones it lets take any value, by position
#[derive(Debug)]
struct Ready {
    set: Vec<(usize, u16)>,
    havoc: Vec<(usize, Vec<u16>)>,
}

/// a transition's guard, read the first time it is tried, as before: a guard never tried is never parsed
#[derive(Debug)]
struct Guard {
    src: Value,
    compiled: OnceCell<Result<Rc<Compiled>>>,
}

// one way out of a state: an event's alternative, or an `always` branch
#[derive(Debug)]
struct Way {
    event: Option<Rc<str>>,
    target: Rc<str>,
    target_at: u32,
    guard: Option<Guard>,
    /// the effects it applies, in order; one that the flow does not have is its name
    effects: Vec<std::result::Result<Rc<Ready>, String>>,
    taken: Rc<str>,
    /// its place among every way of the flow
    id: usize,
    /// its alternative index
    alt: usize,
    /// where it is written in flow.yaml
    path: Vec<String>,
}

// a state's ways out: `always` as one group tried in order, else one group per event
#[derive(Debug, Default)]
struct Ways {
    always: bool,
    groups: Vec<Vec<Way>>,
}

/// a variable's values, each once (SameValue), and their places
#[derive(Debug, Default)]
struct Table {
    values: Vec<Scalar>,
    index: HashMap<Same, u16>,
}

const MOST_VALUES: usize = u16::MAX as usize + 1;

impl Table {
    fn find(&self, s: &Scalar) -> Option<u16> {
        self.index.get(&s.same()).copied()
    }
    /// the value's place, added when it is new; None when the table is full
    fn intern(&mut self, s: Scalar, base: usize) -> Option<u16> {
        if let Some(&i) = self.index.get(&s.same()) {
            return Some(i);
        }
        let code = base + self.values.len();
        if code >= MOST_VALUES {
            return None;
        }
        self.index.insert(s.same(), code as u16);
        self.values.push(s);
        Some(code as u16)
    }
}

// The model explores millions of (state, context) pairs on a large flow (QAuth's consent: 5.4 million), so every guard,
// assertion and effect is read once and kept ready, instead of being looked up by name at every pair.
#[derive(Debug)]
pub struct Model {
    pub flow: Value,
    pub vars: Vec<String>,
    pub domains: Vec<Vec<Scalar>>,
    /// a variable's position in a context
    at: HashMap<String, usize>,
    /// each variable's values as places in its table
    domain_codes: Vec<Vec<u16>>,
    /// each variable's starting value, None for `any`
    initial_codes: Vec<Option<u16>>,
    tables: Vec<Table>,
    /// values a caller gives that no table holds, placed after the table's own
    extra: RefCell<Vec<Table>>,
    state_names: Vec<Rc<str>>,
    state_at: HashMap<String, u32>,
    ways: Vec<Ways>,
    /// every way's `taken`, by its id
    way_names: Vec<Rc<str>>,
    effects: HashMap<String, Rc<Ready>>,
    compiled: RefCell<HashMap<String, Rc<Compiled>>>,
    /// a variable with more values than a context tells apart, which no exploration can hold
    full: Option<String>,
}

/// a transition's effect names: the list, each name as a property key reads it; a string is iterated as its
/// characters, as `for..of` iterates it; anything else is no list
fn effect_names(t: &Transition<'_>) -> Vec<String> {
    let Transition::Full(v) = t else { return Vec::new() };
    match v.get("effect") {
        Some(Value::Array(items)) => items.iter().map(js::to_js_string).collect(),
        Some(Value::String(s)) => s.chars().map(String::from).collect(),
        _ => Vec::new(),
    }
}

impl Model {
    pub fn new(flow: Value) -> Model {
        let f = Flow(&flow);
        let vars: Vec<String> = f.context().map(|(k, _)| k.to_string()).collect();
        let at: HashMap<String, usize> = vars.iter().enumerate().map(|(i, v)| (v.clone(), i)).collect();
        let mut tables: Vec<Table> = vars.iter().map(|_| Table::default()).collect();
        let mut full: Option<String> = None;
        let mut intern = |tables: &mut Vec<Table>, i: usize, s: Scalar| -> u16 {
            match tables[i].intern(s, 0) {
                Some(c) => c,
                None => {
                    full.get_or_insert_with(|| vars[i].clone());
                    u16::MAX
                }
            }
        };
        let mut domains = Vec::with_capacity(vars.len());
        let mut domain_codes = Vec::with_capacity(vars.len());
        let mut initial_codes = Vec::with_capacity(vars.len());
        for (i, (_, cv)) in f.context().enumerate() {
            let values: Vec<Scalar> = cv.values().into_iter().map(Scalar::of).collect();
            domain_codes.push(values.iter().map(|s| intern(&mut tables, i, s.clone())).collect::<Vec<u16>>());
            domains.push(values);
            let initial = cv.initial();
            initial_codes.push(if initial.and_then(Value::as_str) == Some("any") {
                None
            } else {
                Some(intern(&mut tables, i, Scalar::of_option(initial)))
            });
        }
        // every effect, made ready: a name that is not a variable sets nothing a context holds, and a havoc of one is
        // left out (the TS crashed on it)
        let mut effects = HashMap::new();
        for (name, effect) in f.effects() {
            let mut set = Vec::new();
            for (k, v) in effect.set().into_iter().flat_map(|o| o.iter()) {
                if let Some(&i) = at.get(k.as_str()) {
                    set.push((i, intern(&mut tables, i, Scalar::of(v))));
                }
            }
            let havoc = effect
                .havoc()
                .into_iter()
                .filter_map(|h| vars.iter().position(|v| v == h))
                .map(|i| (i, domain_codes[i].clone()))
                .collect();
            effects.insert(name.to_string(), Rc::new(Ready { set, havoc }));
        }
        let state_names: Vec<Rc<str>> = f.state_names().into_iter().map(Rc::from).collect();
        let state_at: HashMap<String, u32> =
            state_names.iter().enumerate().map(|(i, s)| (s.to_string(), i as u32)).collect();
        let mut m = Model {
            flow: Value::Null,
            vars,
            domains,
            at,
            domain_codes,
            initial_codes,
            extra: RefCell::new(tables.iter().map(|_| Table::default()).collect()),
            tables,
            state_names,
            state_at,
            ways: Vec::new(),
            way_names: Vec::new(),
            effects,
            compiled: RefCell::new(HashMap::new()),
            full,
        };
        let (ways, way_names) = m.read_ways(Flow(&flow));
        m.ways = ways;
        m.way_names = way_names;
        m.flow = flow;
        m
    }

    /// the flow, read through its view
    pub fn flow(&self) -> Flow<'_> {
        Flow(&self.flow)
    }

    // every state's ways out, read once: `always` as one group tried in order, else one group per event
    fn read_ways(&self, flow: Flow<'_>) -> (Vec<Ways>, Vec<Rc<str>>) {
        let mut names: Vec<Rc<str>> = Vec::new();
        let mut all = Vec::with_capacity(self.state_names.len());
        for (state, st) in flow.states() {
            let mut way = |event: Option<&str>, a: Transition<'_>, i: usize| -> Way {
                let target = a.target_text();
                let target_at = self.state_at.get(&target).copied().unwrap_or(NO_STATE);
                let taken: Rc<str> = Rc::from(format!("{state}|{}|{i}", event.unwrap_or("always")));
                names.push(taken.clone());
                let guard = match a {
                    Transition::Full(v) => v.get("guard").map(|g| Guard { src: g.clone(), compiled: OnceCell::new() }),
                    Transition::Target(_) => None,
                };
                Way {
                    event: event.map(Rc::from),
                    target: if target_at == NO_STATE {
                        Rc::from(target)
                    } else {
                        self.state_names[target_at as usize].clone()
                    },
                    target_at,
                    guard,
                    effects: effect_names(&a).into_iter().map(|n| self.effects.get(&n).cloned().ok_or(n)).collect(),
                    taken,
                    id: names.len() - 1,
                    alt: i,
                    path: transition_path(state, st, event, i),
                }
            };
            // an `always` that is no list, or a state that is no map, crashed the TS model: read as having no `always`
            let ways = if st.is_final() {
                Ways::default()
            } else if let Some(always) = st.0.get_array("always") {
                let group = always.iter().enumerate().map(|(i, a)| way(None, Transition::of(a), i)).collect();
                Ways { always: true, groups: vec![group] }
            } else {
                let groups = st
                    .on()
                    .into_iter()
                    .map(|(event, spec)| {
                        alternatives(spec).into_iter().enumerate().map(|(i, a)| way(Some(event), a, i)).collect()
                    })
                    .collect();
                Ways { always: false, groups }
            };
            all.push(ways);
        }
        (all, names)
    }

    /// the position of a state in flow.states
    fn state_index(&self, state: &str) -> u32 {
        self.state_at.get(state).copied().unwrap_or(NO_STATE)
    }

    /// the value a context holds for a variable, by its place in the variable's table; undefined for a place that
    /// holds none
    pub fn value_at(&self, var: usize, code: u16) -> Scalar {
        let Some(t) = self.tables.get(var) else { return Scalar::Undefined };
        let code = code as usize;
        if let Some(v) = t.values.get(code) {
            return v.clone();
        }
        self.extra
            .borrow()
            .get(var)
            .and_then(|x| x.values.get(code - t.values.len()).cloned())
            .unwrap_or(Scalar::Undefined)
    }

    /// a context's values, in the variables' order
    pub fn context(&self, ctx: &[u16]) -> Vec<Scalar> {
        ctx.iter().enumerate().map(|(i, &c)| self.value_at(i, c)).collect()
    }

    /// the place of a value in a variable's table, added when no table holds it yet
    pub fn code_of(&self, var: usize, v: &Scalar) -> Result<u16> {
        let Some(t) = self.tables.get(var) else { return Err(model_error("no such variable", None)) };
        if let Some(c) = t.find(v) {
            return Ok(c);
        }
        let mut extra = self.extra.borrow_mut();
        extra.get_mut(var).and_then(|x| x.intern(v.clone(), t.values.len())).ok_or_else(|| {
            model_error(format!("variable {} holds more values than a context tells apart", self.vars[var]), None)
        })
    }

    /// a context of the given values, one per variable: a missing one is undefined, one past the variables is left out
    pub fn encode(&self, values: &[Scalar]) -> Result<Ctx> {
        (0..self.vars.len()).map(|i| self.code_of(i, values.get(i).unwrap_or(&Scalar::Undefined))).collect()
    }

    /// `ctx[var] === v`, `v` a value of a graph file
    pub fn holds(&self, ctx: &[u16], var: usize, v: &Value) -> bool {
        ctx.get(var).map_or(Scalar::Undefined, |&c| self.value_at(var, c)).is(v)
    }

    /// `a[var] === b[var]`
    pub fn same_at(&self, var: usize, a: u16, b: u16) -> bool {
        let x = self.value_at(var, a);
        if a == b {
            // the same value: equal unless it is NaN
            return !matches!(x, Scalar::Number(n) if n.is_nan());
        }
        x.strict_eq(&self.value_at(var, b))
    }

    /// `ctx.join(',')`
    pub fn joined(&self, ctx: &[u16]) -> String {
        let parts: Vec<String> = ctx.iter().enumerate().map(|(i, &c)| self.value_at(i, c).written()).collect();
        parts.join(",")
    }

    /// the starting value of a variable, None when it starts from any of its values
    pub fn initial_code(&self, var: usize) -> Option<u16> {
        self.initial_codes.get(var).copied().flatten()
    }

    fn var<'a>(&'a self, i: usize, ctx: &[u16]) -> Val<'a> {
        let Some(&code) = ctx.get(i) else { return Val::Own(Scalar::Undefined) };
        match self.tables.get(i).and_then(|t| t.values.get(code as usize)) {
            Some(v) => Val::Ref(v),
            None => Val::Own(self.value_at(i, code)),
        }
    }

    // what evaluate(parseExpr(src), lookup) gave, with the same errors, raised only when the name is reached
    // `place`: the named guard being read, so that a name in it that is not one is placed there
    fn compile(&self, e: &Expr, depth: usize, place: Option<&ErrorPlace>) -> Compiled {
        match e {
            Expr::Lit { v } => Compiled::Lit(Scalar::of(v)),
            Expr::Id { name } => {
                if name == "state" {
                    return Compiled::State;
                }
                if let Some(&i) = self.at.get(name.as_str()) {
                    return Compiled::Var(i);
                }
                let Some(guard) = self.flow().guard(name) else {
                    return Compiled::Unknown { name: name.clone(), place: place.cloned() };
                };
                if depth > 8 {
                    return Compiled::TooDeep { name: name.clone(), place: place.cloned() };
                }
                Compiled::Guard(Box::new(Named {
                    src: guard.clone(),
                    depth,
                    at: in_flow(path_of(&["guards", name])),
                    inner: OnceCell::new(),
                }))
            }
            Expr::List { items } => Compiled::List(items.iter().map(|x| self.compile(x, depth, place)).collect()),
            Expr::Not { e } => Compiled::Not(Box::new(self.compile(e, depth, place))),
            Expr::And { l, r } => {
                Compiled::And(Box::new(self.compile(l, depth, place)), Box::new(self.compile(r, depth, place)))
            }
            Expr::Or { l, r } => {
                Compiled::Or(Box::new(self.compile(l, depth, place)), Box::new(self.compile(r, depth, place)))
            }
            Expr::Cmp { op, l, r } => {
                let full = self.compile_cmp(*op, l, r, depth, place);
                self.var_is(full)
            }
        }
    }

    /// `full`, a comparison, answered by the value's place when it compares one variable with literals: neither side
    /// can throw then, and nothing else than the variable's value decides it
    fn var_is(&self, full: Compiled) -> Compiled {
        let var = match &full {
            Compiled::Eq(l, r) | Compiled::Ne(l, r) => match (&**l, &**r) {
                (Compiled::Var(i), Compiled::Lit(_)) | (Compiled::Lit(_), Compiled::Var(i)) => Some(*i),
                _ => None,
            },
            Compiled::InFixed { l, .. } => match &**l {
                Compiled::Var(i) => Some(*i),
                _ => None,
            },
            _ => None,
        };
        let Some(var) = var else { return full };
        let Some(table) = self.tables.get(var) else { return full };
        let mut probe = vec![0u16; self.vars.len()];
        let mut hits = Vec::with_capacity(table.values.len());
        for code in 0..table.values.len() {
            probe[var] = code as u16;
            match self.eval(&full, "", &probe) {
                Ok(v) => hits.push(v.truthy()),
                Err(_) => return full,
            }
        }
        Compiled::VarIs { var, hits, slow: Box::new(full) }
    }

    // a comparison made ready, as compile makes it
    fn compile_cmp(&self, op: Op, l: &Expr, r: &Expr, depth: usize, place: Option<&ErrorPlace>) -> Compiled {
        let lc = Box::new(self.compile(l, depth, place));
        match op {
            Op::Eq => Compiled::Eq(lc, Box::new(self.compile(r, depth, place))),
            Op::Ne => Compiled::Ne(lc, Box::new(self.compile(r, depth, place))),
            Op::In | Op::NotIn => {
                let negate = op == Op::NotIn;
                // a list of literals, the usual right side, is the same list every time: made once
                if let Expr::List { items } = r
                    && items.iter().all(|x| matches!(x, Expr::Lit { .. }))
                {
                    let fixed = items
                        .iter()
                        .filter_map(|x| match x {
                            Expr::Lit { v } => Some(Scalar::of(v)),
                            _ => None,
                        })
                        .collect();
                    return Compiled::InFixed { l: lc, fixed, negate };
                }
                Compiled::In { l: lc, r: Box::new(self.compile(r, depth, place)), negate }
            }
        }
    }

    fn eval<'a>(&'a self, c: &'a Compiled, state: &'a str, ctx: &[u16]) -> Result<Val<'a>> {
        Ok(match c {
            Compiled::Lit(v) => Val::Ref(v),
            Compiled::State => Val::State(state),
            Compiled::Var(i) => self.var(*i, ctx),
            Compiled::Guard(named) => self.eval(self.named(named)?, state, ctx)?,
            Compiled::Unknown { name, place } => {
                return Err(model_error(format!("unknown name \"{name}\" (state {state})"), place.clone()));
            }
            Compiled::TooDeep { name, place } => {
                return Err(model_error(format!("guard {name} nests too deep"), place.clone()));
            }
            Compiled::List(items) => {
                Val::List(items.iter().map(|x| self.eval(x, state, ctx)).collect::<Result<Vec<_>>>()?)
            }
            Compiled::Not(e) => Val::Bool(!self.eval(e, state, ctx)?.truthy()),
            Compiled::And(l, r) => Val::Bool(self.eval(l, state, ctx)?.truthy() && self.eval(r, state, ctx)?.truthy()),
            Compiled::Or(l, r) => Val::Bool(self.eval(l, state, ctx)?.truthy() || self.eval(r, state, ctx)?.truthy()),
            Compiled::Eq(l, r) => {
                let a = self.eval(l, state, ctx)?;
                Val::Bool(a.strict_eq(&self.eval(r, state, ctx)?))
            }
            Compiled::Ne(l, r) => {
                let a = self.eval(l, state, ctx)?;
                Val::Bool(!a.strict_eq(&self.eval(r, state, ctx)?))
            }
            Compiled::InFixed { l, fixed, negate } => {
                let x = self.eval(l, state, ctx)?;
                let x = x.prim();
                let in_list = fixed.iter().any(|y| y.prim().strict_eq(&x));
                Val::Bool(in_list != *negate)
            }
            Compiled::In { l, r, negate } => {
                let x = self.eval(l, state, ctx)?;
                let list = self.eval(r, state, ctx)?;
                Val::Bool(list.holds(&x) != *negate)
            }
            Compiled::VarIs { var, hits, slow } => match ctx.get(*var).and_then(|&c| hits.get(c as usize)) {
                Some(&hit) => Val::Bool(hit),
                None => self.eval(slow, state, ctx)?,
            },
        })
    }

    /// a named guard, compiled the first time it is reached; one that does not parse throws each time it is
    fn named<'a>(&self, n: &'a Named) -> Result<&'a Compiled> {
        let ready = n.inner.get_or_init(|| match parse_source(Some(&n.src)) {
            Ok(e) => Ok(Rc::new(self.compile(&e, n.depth + 1, Some(&n.at)))),
            Err(message) => Err(model_error(message, Some(n.at.clone()))),
        });
        match ready {
            Ok(inner) => Ok(&**inner),
            Err(e) => Err(e.clone()),
        }
    }

    /// `Boolean(eval(c))`, without the values a comparison or a connective gives on the way
    fn truth(&self, c: &Compiled, state: &str, ctx: &[u16]) -> Result<bool> {
        Ok(match c {
            Compiled::VarIs { var, hits, slow } => match ctx.get(*var).and_then(|&c| hits.get(c as usize)) {
                Some(&hit) => hit,
                None => self.eval(slow, state, ctx)?.truthy(),
            },
            Compiled::Not(e) => !self.truth(e, state, ctx)?,
            Compiled::And(l, r) => self.truth(l, state, ctx)? && self.truth(r, state, ctx)?,
            Compiled::Or(l, r) => self.truth(l, state, ctx)? || self.truth(r, state, ctx)?,
            Compiled::Var(i) => self.var(*i, ctx).truthy(),
            Compiled::Guard(named) => self.truth(self.named(named)?, state, ctx)?,
            other => self.eval(other, state, ctx)?.truthy(),
        })
    }

    fn expr(&self, src: &str) -> Result<Rc<Compiled>> {
        if let Some(c) = self.compiled.borrow().get(src) {
            return Ok(c.clone());
        }
        let e = parse_expr(src).map_err(|m| model_error(m, None))?;
        let c = Rc::new(self.compile(&e, 0, None));
        self.compiled.borrow_mut().insert(src.to_string(), c.clone());
        Ok(c)
    }

    /// an expression a graph gives as a value (a guard, an assertion, a node's `when`), made ready
    fn expr_of(&self, src: Option<&Value>) -> Result<Rc<Compiled>> {
        match src {
            Some(Value::String(s)) => self.expr(s),
            other => Err(model_error(parse_source(other).err().unwrap_or_default(), None)),
        }
    }

    pub fn test(&self, expr: &str, state: &str, ctx: &[u16]) -> Result<bool> {
        let f = self.expr(expr)?;
        self.truth(&f, state, ctx)
    }

    /// `test` of an expression a graph gives as a value
    pub fn test_value(&self, expr: Option<&Value>, state: &str, ctx: &[u16]) -> Result<bool> {
        let f = self.expr_of(expr)?;
        self.truth(&f, state, ctx)
    }

    /// Whether evaluating `src` can never throw: it names only the state, variables, and guards that parse and nest no
    /// deeper than evaluation allows. Only then may its pairs be tested in another order than the exploration's.
    pub fn pure(&self, src: &str) -> bool {
        fn ok(m: &Model, e: &Expr, depth: usize, seen: &[&str]) -> bool {
            match e {
                Expr::Lit { .. } => true,
                Expr::Id { name } => {
                    if name == "state" || m.at.contains_key(name.as_str()) {
                        return true;
                    }
                    let Some(guard) = m.flow().guard(name) else { return false };
                    if depth > 8 || seen.contains(&name.as_str()) {
                        return false;
                    }
                    match parse_source(Some(guard)) {
                        Ok(g) => {
                            let mut seen2 = seen.to_vec();
                            seen2.push(name);
                            ok(m, &g, depth + 1, &seen2)
                        }
                        Err(_) => false,
                    }
                }
                Expr::List { items } => items.iter().all(|x| ok(m, x, depth, seen)),
                Expr::Not { e } => ok(m, e, depth, seen),
                Expr::And { l, r } | Expr::Or { l, r } | Expr::Cmp { l, r, .. } => {
                    ok(m, l, depth, seen) && ok(m, r, depth, seen)
                }
            }
        }
        match parse_expr(src) {
            Ok(e) => ok(self, &e, 0, &[]),
            Err(_) => false,
        }
    }

    /// The value of `src` in `state` whatever the context holds, when the state alone decides it (`state in [...] and
    /// ...` outside those states); None when the context matters. Folds only what evaluation would compute the same
    /// way: literals, the state, and the side of an `and` / `or` that decides it before the other is read.
    pub fn in_state(&self, src: &str, state: &str) -> Result<Option<Value>> {
        fn fold(e: &Expr, state: &str) -> Option<Value> {
            match e {
                Expr::Lit { v } => Some(v.clone()),
                Expr::Id { name } => (name == "state").then(|| Value::from(state)),
                Expr::List { items } => {
                    items.iter().map(|x| fold(x, state)).collect::<Option<Vec<_>>>().map(Value::Array)
                }
                Expr::Not { e } => fold(e, state).map(|x| Value::Bool(!x.truthy())),
                Expr::And { l, r } | Expr::Or { l, r } => {
                    let is_and = matches!(e, Expr::And { .. });
                    let l = fold(l, state)?;
                    if if is_and { !l.truthy() } else { l.truthy() } {
                        return Some(Value::Bool(!is_and));
                    }
                    fold(r, state).map(|r| Value::Bool(r.truthy()))
                }
                Expr::Cmp { op, l, r } => {
                    let l = fold(l, state)?;
                    let r = fold(r, state)?;
                    Some(Value::Bool(match op {
                        Op::Eq => l.strict_eq(&r),
                        Op::Ne => !l.strict_eq(&r),
                        Op::In | Op::NotIn => {
                            let in_list = r.as_array().is_some_and(|items| items.iter().any(|y| y.strict_eq(&l)));
                            if *op == Op::In { in_list } else { !in_list }
                        }
                    }))
                }
            }
        }
        let e = parse_expr(src).map_err(|m| model_error(m, None))?;
        Ok(fold(&e, state))
    }

    /// `test` for one expression at many pairs: read once, then run at each
    pub fn tester(&self, expr: &str) -> Result<impl Fn(&str, &[u16]) -> Result<bool> + '_> {
        let f = self.expr(expr)?;
        Ok(move |state: &str, ctx: &[u16]| self.truth(&f, state, ctx))
    }

    /// the value of an expression in a pair; undefined, which a value of a graph file never is, is null
    pub fn value(&self, expr: &str, state: &str, ctx: &[u16]) -> Result<Value> {
        let f = self.expr(expr)?;
        Ok(self.eval(&f, state, ctx)?.to_value())
    }

    /// `value`, undefined kept: a list it builds is a value of its own, as a new array is
    pub fn scalar_value(&self, expr: &str, state: &str, ctx: &[u16]) -> Result<Scalar> {
        let f = self.expr(expr)?;
        Ok(self.eval(&f, state, ctx)?.to_scalar())
    }

    // the contexts an alternative's effects leave, `n` values each, one after the other in `out`: every combination of
    // the havoc variables' values, the first variable slowest, as the nested lists gave them
    fn apply(
        &self,
        effects: &[std::result::Result<Rc<Ready>, String>],
        ctx: &[u16],
        out: &mut Vec<u16>,
        tmp: &mut Vec<u16>,
    ) -> Result<usize> {
        let n = ctx.len();
        out.clear();
        out.extend_from_slice(ctx);
        let mut count = 1;
        for e in effects {
            let ready = match e {
                Ok(r) => r,
                Err(name) => return Err(model_error(format!("unknown effect {name}"), None)),
            };
            tmp.clear();
            let mut next = 0usize;
            for b in 0..count {
                let start = tmp.len();
                tmp.extend_from_slice(&out[b * n..(b + 1) * n]);
                for &(i, v) in &ready.set {
                    if i < n {
                        tmp[start + i] = v;
                    }
                }
                if ready.havoc.is_empty() {
                    next += 1;
                    continue;
                }
                let total = ready.havoc.iter().try_fold(1usize, |t, (_, values)| t.checked_mul(values.len()));
                let Some(total) = total else {
                    return Err(model_error("an effect lets more contexts out than can be counted", None));
                };
                let c: Vec<u16> = tmp[start..].to_vec();
                tmp.truncate(start);
                let mut picks = vec![0usize; ready.havoc.len()];
                for k in 0..total {
                    let mut rest = k;
                    for j in (0..ready.havoc.len()).rev() {
                        let len = ready.havoc[j].1.len();
                        picks[j] = rest % len;
                        rest /= len;
                    }
                    let at = tmp.len();
                    tmp.extend_from_slice(&c);
                    for (j, (i, values)) in ready.havoc.iter().enumerate() {
                        if *i < n {
                            tmp[at + i] = values[picks[j]];
                        }
                    }
                }
                next += total;
            }
            std::mem::swap(out, tmp);
            count = next;
        }
        Ok(count)
    }

    fn passes(&self, w: &Way, state: &str, ctx: &[u16]) -> Result<bool> {
        let Some(g) = &w.guard else { return Ok(true) };
        let f = g.compiled.get_or_init(|| self.expr_of(Some(&g.src)));
        match f {
            Ok(f) => self.truth(f, state, ctx),
            Err(e) => Err(e.clone()),
        }
    }

    // the steps out of a pair, in order, each handed to `each` with the way it takes and the context it leaves;
    // returns the way of the first, for `always` counts
    fn each_step(
        &self,
        at: u32,
        state: &str,
        ctx: &[u16],
        scratch: &mut (Vec<u16>, Vec<u16>),
        each: &mut dyn FnMut(&Way, &[u16]),
    ) -> Result<()> {
        if let Some(var) = &self.full {
            return Err(model_error(format!("variable {var} holds more values than a context tells apart"), None));
        }
        let Some(ways) = self.ways.get(at as usize) else { return Ok(()) };
        let n = ctx.len();
        for alts in &ways.groups {
            let mut chosen = false;
            for a in alts {
                if self.passes(a, state, ctx)? {
                    let (out, tmp) = scratch;
                    let count = self.apply(&a.effects, ctx, out, tmp)?;
                    for k in 0..count {
                        each(a, &out[k * n..(k + 1) * n]);
                    }
                    chosen = true;
                    break;
                }
            }
            if ways.always && !chosen {
                let mut o = Object::new();
                for (i, v) in self.vars.iter().enumerate() {
                    let x = ctx.get(i).map_or(Scalar::Undefined, |&c| self.value_at(i, c));
                    // JSON.stringify leaves out a key whose value is undefined
                    if !matches!(x, Scalar::Undefined) {
                        o.insert(v.clone(), x.to_value());
                    }
                }
                return Err(model_error(
                    format!("`always` in {state} is not exhaustive for {}", js::stringify(&Value::Object(o))),
                    Some(in_flow(path_of(&["states", state, "always"]))),
                ));
            }
        }
        Ok(())
    }

    pub fn steps(&self, state: &str, ctx: &[u16]) -> Result<Vec<Step>> {
        let mut out = Vec::new();
        let mut scratch = (Vec::new(), Vec::new());
        self.each_step(self.state_index(state), state, ctx, &mut scratch, &mut |a, c| {
            out.push(Step { event: a.event.clone(), target: a.target.clone(), ctx: c.into(), taken: a.taken.clone() });
        })?;
        Ok(out)
    }

    /// `err`, returned by `steps(state, ctx)`, given the place in flow.yaml of the transition that threw it when it has
    /// none: the ways are tried again as `steps` tries them, and the one whose guard or effect throws is the place; else
    /// the state. Read only once a step has thrown, so the exploration itself pays nothing for it.
    pub fn place_error(&self, err: Error, state: &str, ctx: &[u16]) -> Error {
        let at = self.state_index(state);
        if err.place.is_some() || at == NO_STATE {
            return err;
        }
        let mut scratch = (Vec::new(), Vec::new());
        for alts in &self.ways[at as usize].groups {
            for a in alts {
                let tried = self.passes(a, state, ctx).and_then(|pass| {
                    if pass {
                        self.apply(&a.effects, ctx, &mut scratch.0, &mut scratch.1).map(|_| true)
                    } else {
                        Ok(false)
                    }
                });
                match tried {
                    Err(_) => return err.placed(in_flow(a.path.clone())),
                    Ok(true) => break,
                    Ok(false) => continue,
                }
            }
        }
        err.placed(in_flow(path_of(&["states", state])))
    }

    pub fn initials(&self) -> Vec<Ctx> {
        // every combination of the starting values, the first variable slowest: the order the nested lists gave
        let choices: Vec<Vec<u16>> = (0..self.vars.len())
            .map(|i| match self.initial_codes[i] {
                None => self.domain_codes[i].clone(),
                Some(c) => vec![c],
            })
            .collect();
        let Some(total) = choices.iter().try_fold(1usize, |n, c| n.checked_mul(c.len())) else { return Vec::new() };
        let mut out = Vec::with_capacity(total);
        for k in 0..total {
            let mut c = vec![0u16; choices.len()];
            let mut rest = k;
            for i in (0..choices.len()).rev() {
                c[i] = choices[i][rest % choices[i].len()];
                rest /= choices[i].len();
            }
            out.push(c.into_boxed_slice());
        }
        out
    }

    /// the values of the variables that start from any value: the starting situation a context is
    pub fn scenario(&self, ctx: &[u16]) -> Object {
        let mut out = Object::new();
        for (i, v) in self.vars.iter().enumerate() {
            if self.initial_codes[i].is_none() {
                out.insert(v.clone(), ctx.get(i).map_or(Scalar::Undefined, |&c| self.value_at(i, c)).to_value());
            }
        }
        out
    }

    /// a context of another length than the variables, cut or filled with undefined
    fn fit(&self, ctx: Ctx) -> Ctx {
        if ctx.len() == self.vars.len() {
            return ctx;
        }
        (0..self.vars.len())
            .map(|i| ctx.get(i).copied().unwrap_or_else(|| self.code_of(i, &Scalar::Undefined).unwrap_or(u16::MAX)))
            .collect()
    }

    /// a structure that holds contexts, ready to be written with their values
    pub fn show<'a, T: ?Sized>(&'a self, value: &'a T) -> Shown<'a, T> {
        Shown { model: self, value }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Exploration {
    pub configs: Vec<Config>,
    pub starts: usize,
    pub taken: IndexSet<Rc<str>>,
    pub at_state: IndexMap<Rc<str>, usize>,
    pub branch_counts: IndexMap<Rc<str>, Vec<usize>>,
}

/// murmur3's finalizer: the mixed-radix numbers of the pairs differ in their high digits as much as in their low ones
fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

/// the numbers of the pairs seen: open addressing, each number in its own slot (as the number + 1; 0 is a free slot),
/// so that a lookup reads one place in memory where a general hash set reads two. On the largest flows the lookups
/// are most of the exploration's time.
struct NumberSet {
    slots: Vec<u64>,
    len: usize,
}

impl NumberSet {
    fn new() -> Self {
        NumberSet { slots: vec![0; 1 << 10], len: 0 }
    }

    /// whether the number is new, kept when it is; a number is below 2^53
    fn insert(&mut self, key: u64) -> bool {
        if (self.len + 1) * 2 > self.slots.len() {
            self.grow();
        }
        let mask = self.slots.len() - 1;
        let k = key + 1;
        let mut i = mix(key) as usize & mask;
        loop {
            match self.slots[i] {
                0 => {
                    self.slots[i] = k;
                    self.len += 1;
                    return true;
                }
                s if s == k => return false,
                _ => i = (i + 1) & mask,
            }
        }
    }

    fn grow(&mut self) {
        let size = self.slots.len() * 2;
        let old = std::mem::replace(&mut self.slots, vec![0; size]);
        let mask = self.slots.len() - 1;
        for k in old.into_iter().filter(|&k| k != 0) {
            let mut i = mix(k - 1) as usize & mask;
            while self.slots[i] != 0 {
                i = (i + 1) & mask;
            }
            self.slots[i] = k;
        }
    }
}

// A (state, context) pair as one number: the state's position, then each variable's value by its position in the
// variable's values, as digits of a mixed radix. A Set keeps numbers far faster than the strings they replace.
// Two pairs are the same exactly when their old keys, `${state}|${ctx.join(',')}`, are: a value is placed by its text,
// as join wrote it. A flow whose pairs do not fit below 2^53, a value or state name that would make the old key
// ambiguous, or a value outside its variable's values, keeps the old key.
struct Keyer {
    numbers: Option<Numbers>,
    seen_numbers: NumberSet,
    seen_texts: HashSet<String>,
}

struct Numbers {
    radix: Vec<u64>,
    /// for each variable, the digit of each place in its table (the first of its values with the same text)
    digits: Vec<Vec<Option<u64>>>,
    by_text: Vec<HashMap<String, u64>>,
}

const MAX_SAFE_INTEGER: u128 = (1u128 << 53) - 1;

impl Keyer {
    fn new(m: &Model) -> Keyer {
        let names = &m.state_names;
        let size = m
            .domains
            .iter()
            .try_fold(names.len().max(1) as u128, |n, values| n.checked_mul(values.len().max(1) as u128));
        let fits = size.is_some_and(|s| s <= MAX_SAFE_INTEGER);
        let numbers = if !fits
            || names.iter().any(|s| s.contains('|'))
            || m.domains.iter().any(|values| values.iter().any(|v| v.written().contains(',')))
        {
            None
        } else {
            let by_text: Vec<HashMap<String, u64>> = m
                .domains
                .iter()
                .map(|values| {
                    let mut d = HashMap::new();
                    for (i, v) in values.iter().enumerate() {
                        d.entry(v.written()).or_insert(i as u64);
                    }
                    d
                })
                .collect();
            let extra = m.extra.borrow();
            let digits = (0..m.vars.len())
                .map(|i| {
                    let count = m.tables[i].values.len() + extra[i].values.len();
                    (0..count).map(|c| by_text[i].get(&m.value_at(i, c as u16).written()).copied()).collect()
                })
                .collect();
            let radix = m.domains.iter().map(|values| values.len().max(1) as u64).collect();
            Some(Numbers { radix, digits, by_text })
        };
        Keyer { numbers, seen_numbers: NumberSet::new(), seen_texts: HashSet::new() }
    }

    fn text(m: &Model, state: &str, ctx: &[u16]) -> String {
        format!("{state}|{}", m.joined(ctx))
    }

    /// whether the pair is new, kept as seen when it is
    fn add(&mut self, m: &Model, at: u32, state: &str, ctx: &[u16]) -> bool {
        if let Some(n) = &self.numbers
            && at != NO_STATE
            && ctx.len() == n.radix.len()
        {
            let mut key = at as u64;
            let mut whole = true;
            for (i, &code) in ctx.iter().enumerate() {
                let d = match n.digits[i].get(code as usize) {
                    Some(d) => *d,
                    None => n.by_text[i].get(&m.value_at(i, code).written()).copied(),
                };
                let Some(d) = d else {
                    whole = false;
                    break;
                };
                key = key * n.radix[i] + d;
            }
            if whole {
                return self.seen_numbers.insert(key);
            }
        }
        self.seen_texts.insert(Keyer::text(m, state, ctx))
    }
}

/// `keep(at, step)`: whether the exploration takes the step from the situation it is at
pub type Keep<'a> = &'a dyn Fn(&Config, &Step) -> bool;

// Every reachable (state, context) pair, breadth first, so the first hit of a state is a shortest path to it.
// `from` and `keep` narrow the search: the replay starts only from situations the app is bound to and takes only steps it can drive.
pub fn explore(m: &Model, from: Option<Vec<Ctx>>, keep: Option<Keep<'_>>) -> Result<Exploration> {
    let from = from.unwrap_or_else(|| m.initials());
    let mut key = Keyer::new(m);
    let mut configs: Vec<Config> = Vec::new();
    let mut taken = IndexSet::new();
    let mut taken_seen = vec![false; m.way_names.len()];
    let mut at_state: IndexMap<Rc<str>, usize> = IndexMap::new();
    let mut at_slot: Vec<Option<usize>> = vec![None; m.state_names.len()];
    let mut branch_counts: IndexMap<Rc<str>, Vec<usize>> = IndexMap::new();
    let initial = m.flow().initial().map_or_else(
        || m.flow.get("initial").map_or_else(|| "undefined".to_string(), js::to_js_string),
        str::to_string,
    );
    let initial_at = m.state_index(&initial);
    let initial_name: Rc<str> =
        if initial_at == NO_STATE { Rc::from(initial.as_str()) } else { m.state_names[initial_at as usize].clone() };
    for c in from {
        let c = m.fit(c);
        if key.add(m, initial_at, &initial_name, &c) {
            configs.push(Config { state: initial_name.clone(), ctx: c, parent: None, event: None, at: initial_at });
        }
    }
    let starts = configs.len();
    let mut scratch = (Vec::new(), Vec::new());
    let mut cur: Vec<u16> = Vec::new();
    let mut i = 0;
    while i < configs.len() {
        let at = configs[i].at;
        let state = configs[i].state.clone();
        let slot = at_slot.get(at as usize).copied().flatten();
        let pos = match slot {
            Some(p) => p,
            None => {
                let (p, _) = at_state.insert_full(state.clone(), 0);
                if let Some(s) = at_slot.get_mut(at as usize) {
                    *s = Some(p);
                }
                p
            }
        };
        if let Some((_, n)) = at_state.get_index_mut(pos) {
            *n += 1;
        }
        cur.clear();
        cur.extend_from_slice(&configs[i].ctx);
        let mut first: Option<usize> = None;
        let parent = i as u32;
        let stepped = m.each_step(at, &state, &cur, &mut scratch, &mut |a, c| {
            if first.is_none() {
                first = Some(a.alt);
            }
            if let Some(keep) = keep {
                let s =
                    Step { event: a.event.clone(), target: a.target.clone(), ctx: c.into(), taken: a.taken.clone() };
                if !keep(&configs[parent as usize], &s) {
                    return;
                }
            }
            if !taken_seen[a.id] {
                taken_seen[a.id] = true;
                taken.insert(a.taken.clone());
            }
            if key.add(m, a.target_at, &a.target, c) {
                configs.push(Config {
                    state: a.target.clone(),
                    ctx: c.into(),
                    parent: Some(parent),
                    event: a.event.clone(),
                    at: a.target_at,
                });
            }
        });
        if let Err(err) = stepped {
            // where in flow.yaml the step that threw is written (SPEC.md §10)
            return Err(m.place_error(err, &configs[i].state, &configs[i].ctx));
        }
        if let (Some(alt), Some(ways)) = (first, m.ways.get(at as usize))
            && ways.always
        {
            let counts = branch_counts.entry(state).or_insert_with(|| vec![0; ways.groups[0].len()]);
            if let Some(n) = counts.get_mut(alt) {
                *n += 1;
            }
        }
        i += 1;
    }
    Ok(Exploration { configs, starts, taken, at_state, branch_counts })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PathResult {
    pub events: Vec<String>,
    #[serde(serialize_with = "object")]
    pub start: Object,
}

fn object<S: Serializer>(o: &Object, s: S) -> std::result::Result<S::Ok, S::Error> {
    let mut map = s.serialize_map(Some(o.len()))?;
    for (k, v) in o.iter() {
        map.serialize_entry(k, v)?;
    }
    map.end()
}

pub fn path_to(m: &Model, ex: &Exploration, index: usize) -> PathResult {
    let mut events = Vec::new();
    let Some(mut c) = ex.configs.get(index) else { return PathResult { events, start: Object::new() } };
    while let Some(p) = c.parent_index() {
        if let Some(e) = &c.event {
            events.push(e.to_string());
        }
        c = &ex.configs[p];
    }
    events.reverse();
    PathResult { events, start: m.scenario(&c.ctx) }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Violation {
    pub events: Vec<String>,
    #[serde(serialize_with = "object")]
    pub start: Object,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AssertionResult {
    pub id: String,
    pub never: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub violation: Option<Violation>,
}

// the first pair, in exploration order, where `src` holds; None when none does. An assertion that cannot throw is tested
// only in the states where the state alone does not rule it out: the first hit of each such state, the earliest of them.
fn first_hit<'e>(
    m: &Model,
    ex: &'e Exploration,
    src: &str,
    by_state: &mut dyn FnMut() -> &'e IndexMap<Rc<str>, Vec<usize>>,
) -> Result<Option<usize>> {
    // read where the exploration reads it, so an expression that throws throws at the same pair as before: with no
    // pair, it is never read
    if !m.pure(src) {
        if ex.configs.is_empty() {
            return Ok(None);
        }
        let test = m.tester(src)?;
        for (i, c) in ex.configs.iter().enumerate() {
            if test(&c.state, &c.ctx)? {
                return Ok(Some(i));
            }
        }
        return Ok(None);
    }
    let test = m.tester(src)?;
    let mut hit: Option<usize> = None;
    for (state, at) in by_state() {
        let fixed = m.in_state(src, state)?;
        if fixed.as_ref().is_some_and(|f| !f.truthy()) {
            continue;
        }
        for &i in at {
            if hit.is_some_and(|h| i > h) {
                break;
            }
            if fixed.is_some() || test(state, &ex.configs[i].ctx)? {
                hit = Some(i);
                break;
            }
        }
    }
    Ok(hit)
}

pub fn check_assertions(m: &Model, ex: &Exploration) -> Result<Vec<AssertionResult>> {
    let groups: OnceCell<IndexMap<Rc<str>, Vec<usize>>> = OnceCell::new();
    let mut by_state = || {
        groups.get_or_init(|| {
            let mut g: IndexMap<Rc<str>, Vec<usize>> = IndexMap::new();
            for (i, c) in ex.configs.iter().enumerate() {
                g.entry(c.state.clone()).or_default().push(i);
            }
            g
        })
    };
    let mut out = Vec::new();
    for (i, a) in m.flow().assertions().into_iter().enumerate() {
        let place = || in_flow(vec!["assertions".into(), i.to_string(), "never".into()]);
        let never = a.get("never");
        let hit = match never {
            Some(Value::String(src)) => first_hit(m, ex, src, &mut by_state),
            // an expression that is no string is read at the first pair, as the TS model read it
            _ if ex.configs.is_empty() => Ok(None),
            other => Err(model_error(parse_source(other).err().unwrap_or_default(), None)),
        };
        let hit = hit.map_err(|err| err.placed(place()))?;
        let id = a.get("id").map_or_else(|| "undefined".to_string(), js::to_js_string);
        let never = never.map_or_else(|| "undefined".to_string(), js::to_js_string);
        let say = crate::graph::given(a, "say").map(js::to_js_string);
        let violation = hit.map(|h| {
            let p = path_to(m, ex, h);
            Violation { events: p.events, start: p.start, state: ex.configs[h].state.to_string() }
        });
        out.push(AssertionResult { id, never, say, violation });
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FinalPath {
    pub state: String,
    /// position in Exploration.configs, for building a narrative
    pub index: usize,
    pub events: Vec<String>,
    #[serde(serialize_with = "object")]
    pub start: Object,
}

pub fn final_paths(m: &Model, ex: &Exploration) -> Vec<FinalPath> {
    let mut out = Vec::new();
    for (k, (name, st)) in m.flow().states().enumerate() {
        if !st.is_final() {
            continue;
        }
        let at = k as u32;
        if let Some(hit) = ex.configs.iter().position(|c| c.at == at || (c.at == NO_STATE && &*c.state == name)) {
            let p = path_to(m, ex, hit);
            out.push(FinalPath { state: name.to_string(), index: hit, events: p.events, start: p.start });
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoverPath {
    /// a hash of its start and of the transitions it takes: the same path has the same id in every run
    pub id: String,
    /// the starting situation, as pathTo gives it
    #[serde(serialize_with = "object")]
    pub start: Object,
    /// the events to send, in order; a transition with no event (`always`) is taken on its own
    pub events: Vec<String>,
    /// the states it passes, in order, the initial one first
    pub states: Vec<String>,
    /// the transitions it takes, in order, each as `<state>|<event or always>|<alternative>`
    pub transitions: Vec<String>,
    /// the states (or transitions) it is kept for: those no path before it passes (or takes)
    pub covers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum By {
    States,
    Transitions,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cover {
    pub by: By,
    pub paths: Vec<CoverPath>,
    /// the states (or transitions) of the flow that no reachable situation passes (or takes)
    pub unreached: Vec<String>,
}

// every transition of the flow, named as a Step's `taken` names it
pub fn transitions_of(flow: Flow<'_>) -> Vec<String> {
    let mut out = Vec::new();
    for (state, st) in flow.states() {
        if st.is_final() {
            continue;
        }
        if let Some(always) = st.0.get_array("always") {
            out.extend((0..always.len()).map(|i| format!("{state}|always|{i}")));
        } else {
            for (event, spec) in st.on() {
                out.extend((0..alternatives(spec).len()).map(|i| format!("{state}|{event}|{i}")));
            }
        }
    }
    out
}

// 53 bits of a string, as 14 hex digits (cyrb53): a stable id with no dependency on a platform's crypto
fn hash(text: &str) -> String {
    let mut h1: u32 = 0xdead_beef;
    let mut h2: u32 = 0x41c6_ce57;
    for c in text.encode_utf16() {
        let c = u32::from(c);
        h1 = (h1 ^ c).wrapping_mul(2_654_435_761);
        h2 = (h2 ^ c).wrapping_mul(1_597_334_677);
    }
    h1 = (h1 ^ (h1 >> 16)).wrapping_mul(2_246_822_507) ^ (h2 ^ (h2 >> 13)).wrapping_mul(3_266_489_909);
    h2 = (h2 ^ (h2 >> 16)).wrapping_mul(2_246_822_507) ^ (h1 ^ (h1 >> 13)).wrapping_mul(3_266_489_909);
    let n = 4_294_967_296u64 * u64::from(2_097_151 & h2) + u64::from(h1);
    format!("{n:014x}")
}

/// Event paths that together pass every reachable state (`states`) or take every reachable transition (`transitions`)
/// of the flow (#4), from the same starts and through the same steps as `explore(m, from, keep)`. Each is a shortest
/// way to the state or transition it is first kept for, so it repeats a state only on the way to something not yet
/// covered. The longest are kept first and a path that covers nothing new is left out: a small set, never every path.
/// The same flow gives the same paths, in the same order, with the same ids.
pub fn cover(m: &Model, by: By, from: Option<Vec<Ctx>>, keep: Option<Keep<'_>>) -> Result<Cover> {
    let ex = explore(m, from, keep)?;
    let steps_of = |c: &Config| -> Result<Vec<Step>> {
        let mut s = m.steps(&c.state, &c.ctx)?;
        if let Some(keep) = keep {
            s.retain(|x| keep(c, x));
        }
        Ok(s)
    };
    // the transition that made a situation: the first step of its parent that leads to it, as explore took it
    let mut via: HashMap<usize, String> = HashMap::new();
    let mut made = |i: usize| -> Result<String> {
        if let Some(t) = via.get(&i) {
            return Ok(t.clone());
        }
        let c = &ex.configs[i];
        let p = &ex.configs[c.parent_index().unwrap_or(0)];
        let s = steps_of(p)?.into_iter().find(|x| {
            x.target == c.state
                && x.event == c.event
                && x.ctx.iter().enumerate().all(|(j, &v)| c.ctx.get(j).is_some_and(|&w| m.same_at(j, v, w)))
        });
        let t = match s {
            Some(s) => s.taken.to_string(),
            None => format!("{}|{}|?", p.state, c.event.as_deref().unwrap_or("always")),
        };
        via.insert(i, t.clone());
        Ok(t)
    };
    let chain = |i: usize| -> Vec<usize> {
        let mut out = vec![i];
        let mut k = i;
        while let Some(p) = ex.configs[k].parent_index() {
            out.push(p);
            k = p;
        }
        out.reverse();
        out
    };
    struct Candidate {
        /// the situation the path reaches, or the one it takes its last step from
        at: usize,
        /// for a transition: the step taken from there
        last: Option<Step>,
        depth: usize,
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    if by == By::States {
        let mut first: IndexMap<&str, usize> = IndexMap::new();
        for (i, c) in ex.configs.iter().enumerate() {
            first.entry(&c.state).or_insert(i);
        }
        for &i in first.values() {
            candidates.push(Candidate { at: i, last: None, depth: chain(i).len() });
        }
    } else {
        // the first situation, in exploration order, at which each transition is taken: the end of a shortest way to it
        let mut seen: HashSet<Rc<str>> = HashSet::new();
        let mut i = 0;
        while i < ex.configs.len() && seen.len() < ex.taken.len() {
            for s in steps_of(&ex.configs[i])? {
                if seen.contains(&s.taken) {
                    continue;
                }
                seen.insert(s.taken.clone());
                let depth = chain(i).len() + 1;
                candidates.push(Candidate { at: i, last: Some(s), depth });
            }
            i += 1;
        }
    }
    let order = |c: &Candidate| -> usize { c.at * 2 + usize::from(c.last.is_some()) };
    let mut sorted: Vec<&Candidate> = candidates.iter().collect();
    sorted.sort_by(|a, b| b.depth.cmp(&a.depth).then(order(a).cmp(&order(b))));
    let mut covered: HashSet<String> = HashSet::new();
    let mut paths: Vec<(CoverPath, usize)> = Vec::new();
    for c in sorted {
        let links = chain(c.at);
        let mut states: Vec<String> = links.iter().map(|&k| ex.configs[k].state.to_string()).collect();
        let mut transitions: Vec<String> = links[1..].iter().map(|&k| made(k)).collect::<Result<_>>()?;
        let mut events: Vec<String> =
            links[1..].iter().filter_map(|&k| ex.configs[k].event.as_ref().map(|e| e.to_string())).collect();
        if let Some(last) = &c.last {
            states.push(last.target.to_string());
            transitions.push(last.taken.to_string());
            if let Some(e) = &last.event {
                events.push(e.to_string());
            }
        }
        let mut covers: Vec<String> = Vec::new();
        for x in if by == By::States { &states } else { &transitions } {
            if !covered.contains(x) && !covers.contains(x) {
                covers.push(x.clone());
            }
        }
        if covers.is_empty() {
            continue;
        }
        covered.extend(covers.iter().cloned());
        let start = m.scenario(&ex.configs[links[0]].ctx);
        let id_text = Value::Array(vec![
            Value::Object(start.clone()),
            Value::Array(transitions.iter().map(|t| Value::from(t.as_str())).collect()),
        ]);
        let id = hash(&js::stringify(&id_text));
        paths.push((CoverPath { id, start, events, states, transitions, covers }, order(c)));
    }
    paths.sort_by_key(|(_, o)| *o);
    let all = if by == By::States {
        m.flow().state_names().into_iter().map(str::to_string).collect()
    } else {
        transitions_of(m.flow())
    };
    Ok(Cover {
        by,
        paths: paths.into_iter().map(|(p, _)| p).collect(),
        unreached: all.into_iter().filter(|x| !covered.contains(x)).collect(),
    })
}

/// a structure that holds contexts (a configuration, a step, an exploration, a story's steps), written with each
/// context's values: the shape the TypeScript objects have, for the Node bindings to hand JavaScript
pub struct Shown<'a, T: ?Sized> {
    pub model: &'a Model,
    pub value: &'a T,
}

struct CtxShown<'a>(&'a Model, &'a [u16]);

impl Serialize for CtxShown<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.1.len()))?;
        for (i, &c) in self.1.iter().enumerate() {
            seq.serialize_element(&self.0.value_at(i, c))?;
        }
        seq.end()
    }
}

impl Serialize for Shown<'_, [u16]> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        CtxShown(self.model, self.value).serialize(s)
    }
}

impl Serialize for Shown<'_, Config> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let c = self.value;
        let mut st = s.serialize_struct("Config", 4)?;
        st.serialize_field("state", &*c.state)?;
        st.serialize_field("ctx", &CtxShown(self.model, &c.ctx))?;
        st.serialize_field("parent", &c.parent.map_or(-1, i64::from))?;
        st.serialize_field("event", &c.event.as_deref())?;
        st.end()
    }
}

impl Serialize for Shown<'_, Step> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let x = self.value;
        let mut st = s.serialize_struct("Step", 4)?;
        st.serialize_field("event", &x.event.as_deref())?;
        st.serialize_field("target", &*x.target)?;
        st.serialize_field("ctx", &CtxShown(self.model, &x.ctx))?;
        st.serialize_field("taken", &*x.taken)?;
        st.end()
    }
}

impl Serialize for Shown<'_, [Step]> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_seq(self.value.iter().map(|x| self.model.show(x)))
    }
}

impl Serialize for Shown<'_, [Config]> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_seq(self.value.iter().map(|x| self.model.show(x)))
    }
}

impl Serialize for Shown<'_, Exploration> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let ex = self.value;
        let mut st = s.serialize_struct("Exploration", 5)?;
        st.serialize_field("configs", &self.model.show(&ex.configs[..]))?;
        st.serialize_field("starts", &ex.starts)?;
        st.serialize_field("taken", &Names(&ex.taken))?;
        st.serialize_field("atState", &Counts(&ex.at_state))?;
        st.serialize_field("branchCounts", &Counts(&ex.branch_counts))?;
        st.end()
    }
}

/// a Set of names, written as a list in its order
struct Names<'a>(&'a IndexSet<Rc<str>>);

impl Serialize for Names<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_seq(self.0.iter().map(|x| &**x))
    }
}

/// a Map from names, written as a map in its order
struct Counts<'a, V>(&'a IndexMap<Rc<str>, V>);

impl<V: Serialize> Serialize for Counts<'_, V> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_map(self.0.iter().map(|(k, v)| (&**k, v)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The flows of these tests are written as the TS Model takes them (no load), and every expected value is what
    // explore, checkAssertions and cover of dist/model.js gave on the same flow (node, scratch tricky.mjs), each
    // configuration written as `${state}|${JSON.stringify(ctx)}|${parent}|${event}`.
    fn model(json: &str) -> Model {
        Model::new(js::json_parse(json).expect("the test flow is JSON"))
    }

    fn configs(m: &Model, ex: &Exploration) -> Vec<String> {
        ex.configs
            .iter()
            .map(|c| {
                let ctx = Value::Array(m.context(&c.ctx).iter().map(Scalar::to_value).collect());
                let parent = c.parent.map_or(-1, i64::from);
                format!("{}|{}|{parent}|{}", c.state, js::stringify(&ctx), c.event.as_deref().unwrap_or("null"))
            })
            .collect()
    }

    fn cover_ids(m: &Model) -> Vec<String> {
        let c = cover(m, By::Transitions, None, None).expect("covers");
        c.paths.iter().map(|p| format!("{} {}", p.id, p.transitions.join(" "))).collect()
    }

    fn failure(json: &str) -> (String, Vec<String>) {
        let m = model(json);
        let e = explore(&m, None, None).expect_err("the exploration fails");
        (e.message, e.place.and_then(|p| p.path).unwrap_or_default())
    }

    #[test]
    fn a_value_is_placed_by_its_text() {
        // the string 'true' an effect sets is the situation the boolean true is: the first one reached is kept
        let m = model(
            r#"{"id":"t","initial":"a","context":{"x":{"values":[true,"y"],"initial":"any"}},"guards":{},
            "effects":{"s":{"set":{"x":"true"}}},
            "states":{"a":{"on":{"GO":{"target":"b","effect":["s"]}}},"b":{"on":{"BACK":"a"}}}}"#,
        );
        let ex = explore(&m, None, None).unwrap();
        assert_eq!(configs(&m, &ex), ["a|[true]|-1|null", "a|[\"y\"]|-1|null", "b|[\"true\"]|0|GO"]);
        assert_eq!(ex.taken.iter().map(|t| &**t).collect::<Vec<_>>(), ["a|GO|0", "b|BACK|0"]);
        assert_eq!(ex.at_state.iter().map(|(k, n)| (&**k, *n)).collect::<Vec<_>>(), [("a", 2), ("b", 1)]);
        assert_eq!(cover_ids(&m), ["0ae9428bbb02a5 a|GO|0 b|BACK|0"]);
    }

    #[test]
    fn a_value_with_a_comma_keeps_the_text_key() {
        let m = model(
            r#"{"id":"c","initial":"a","context":{"x":{"values":["a,b","c"],"initial":"any"}},"guards":{},"effects":{},
            "states":{"a":{"on":{"GO":"b"}},"b":{"type":"final"}}}"#,
        );
        let ex = explore(&m, None, None).unwrap();
        assert!(Keyer::new(&m).numbers.is_none());
        assert_eq!(
            configs(&m, &ex),
            ["a|[\"a,b\"]|-1|null", "a|[\"c\"]|-1|null", "b|[\"a,b\"]|0|GO", "b|[\"c\"]|1|GO"]
        );
        assert_eq!(cover_ids(&m), ["0b0fa650b530cd a|GO|0"]);
    }

    #[test]
    fn havoc_gives_every_combination_the_first_variable_slowest() {
        let m = model(
            r#"{"id":"h","initial":"a","context":{"p":{"values":[1,2],"initial":1},"q":{"values":["x","y","z"],"initial":"x"}},
            "guards":{},"effects":{"any":{"havoc":["q","p"]}},
            "states":{"a":{"on":{"GO":{"target":"b","effect":["any"]}}},"b":{"type":"final"}}}"#,
        );
        let ex = explore(&m, None, None).unwrap();
        assert_eq!(
            configs(&m, &ex),
            [
                "a|[1,\"x\"]|-1|null",
                "b|[1,\"x\"]|0|GO",
                "b|[2,\"x\"]|0|GO",
                "b|[1,\"y\"]|0|GO",
                "b|[2,\"y\"]|0|GO",
                "b|[1,\"z\"]|0|GO",
                "b|[2,\"z\"]|0|GO"
            ]
        );
        assert_eq!(cover_ids(&m), ["079b5e214cd613 a|GO|0"]);
    }

    #[test]
    fn a_list_of_the_flow_is_equal_only_to_itself() {
        let m = model(
            r#"{"id":"i","initial":"a","context":{"v":{"values":[[1],[2]],"initial":"any"}},
            "guards":{"same":"v == v","inLit":"v in [[1]]"},"effects":{},
            "states":{"a":{"always":[{"target":"b","guard":"same and not inLit"},{"target":"c"}]},"b":{"type":"final"},"c":{"type":"final"}}}"#,
        );
        let ex = explore(&m, None, None).unwrap();
        assert_eq!(configs(&m, &ex), ["a|[[1]]|-1|null", "a|[[2]]|-1|null", "b|[[1]]|0|null", "b|[[2]]|1|null"]);
        assert_eq!(ex.branch_counts.get("a"), Some(&vec![2, 0]));
        assert_eq!(cover_ids(&m), ["187af100616059 a|always|0"]);
    }

    #[test]
    fn errors_are_worded_and_placed_as_the_ts_model_does() {
        let at = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            failure(
                r#"{"id":"n","initial":"a","context":{"x":{"values":["p","q"],"initial":"any"},"7":{"values":[0],"initial":0}},
                "guards":{},"effects":{},"states":{"a":{"always":[{"target":"b","guard":"x == 'p'"}]},"b":{"type":"final"}}}"#
            ),
            ("`always` in a is not exhaustive for {\"7\":0,\"x\":\"q\"}".to_string(), at(&["states", "a", "always"]))
        );
        assert_eq!(
            failure(
                r#"{"id":"u","initial":"a","context":{},"guards":{"g":"nope"},"effects":{},
                "states":{"a":{"on":{"GO":[{"target":"b","guard":"g"}]}},"b":{"type":"final"}}}"#
            ),
            ("unknown name \"nope\" (state a)".to_string(), at(&["guards", "g"]))
        );
        assert_eq!(
            failure(
                r#"{"id":"u2","initial":"a","context":{},"guards":{},"effects":{},
                "states":{"a":{"on":{"GO":{"target":"b","guard":"nope"}}},"b":{"type":"final"}}}"#
            ),
            ("unknown name \"nope\" (state a)".to_string(), at(&["states", "a", "on", "GO"]))
        );
        assert_eq!(
            failure(
                r#"{"id":"d","initial":"a","context":{},"guards":{"g":"g or true"},"effects":{},
                "states":{"a":{"on":{"GO":{"target":"b","guard":"g"}}},"b":{"type":"final"}}}"#
            ),
            ("guard g nests too deep".to_string(), at(&["guards", "g"]))
        );
        assert_eq!(
            failure(
                r#"{"id":"bg","initial":"a","context":{},"guards":{"g":"a =="},"effects":{},
                "states":{"a":{"on":{"GO":{"target":"b","guard":"g"}}},"b":{"type":"final"}}}"#
            ),
            ("unexpected end in: a ==".to_string(), at(&["guards", "g"]))
        );
        assert_eq!(
            failure(
                r#"{"id":"e","initial":"a","context":{},"guards":{},"effects":{},
                "states":{"a":{"on":{"GO":[{"target":"b","guard":"false"},{"target":"b","effect":["missing"]}]}},"b":{"type":"final"}}}"#
            ),
            ("unknown effect missing".to_string(), at(&["states", "a", "on", "GO", "1"]))
        );
    }

    #[test]
    fn assertions_are_found_at_their_first_pair_and_their_errors_placed() {
        let flow = |assertions: &str| {
            model(&format!(
                r#"{{"id":"as","initial":"a","context":{{"x":{{"values":["p","q"],"initial":"any"}}}},"guards":{{}},
                "effects":{{"q":{{"set":{{"x":"q"}}}}}},"assertions":{assertions},
                "states":{{"a":{{"on":{{"GO":"b","SET":{{"target":"a","effect":["q"]}}}}}},"b":{{"on":{{"BACK":"a"}}}}}}}}"#
            ))
        };
        let m = flow(
            r#"[{"id":"one","never":"state == 'b' and x == 'q'","say":"b is never q"},{"id":"two","never":"state == 'z'"}]"#,
        );
        let ex = explore(&m, None, None).unwrap();
        let a = check_assertions(&m, &ex).unwrap();
        let one = a[0].violation.as_ref().expect("one is broken");
        assert_eq!((one.state.as_str(), one.events.clone()), ("b", vec!["GO".to_string()]));
        assert_eq!(js::stringify(&Value::Object(one.start.clone())), r#"{"x":"q"}"#);
        assert_eq!(a[0].say.as_deref(), Some("b is never q"));
        assert!(a[1].violation.is_none() && a[1].say.is_none());
        assert_eq!(cover_ids(&m), ["12932fc902092f a|SET|0", "1cccf0dbdc7318 a|GO|0 b|BACK|0"]);
        let m = flow(r#"[{"id":"three","never":"missing"}]"#);
        let ex = explore(&m, None, None).unwrap();
        let e = check_assertions(&m, &ex).unwrap_err();
        assert_eq!(e.message, "unknown name \"missing\" (state a)");
        // the TS flow had it third, at assertions.2.never; alone here, it is the first
        assert_eq!(e.place.and_then(|p| p.path).unwrap(), ["assertions", "0", "never"]);
    }

    #[test]
    fn cyrb53_as_the_ts_hash_gives_it() {
        // the function hash of dist/model.js, run by node on each text
        assert_eq!(hash(""), "0bdcb81aee8d83");
        assert_eq!(hash("a"), "1c2ba782c97901");
        assert_eq!(hash("[{},[\"s|GO|0\"]]"), "06cde877c332d5");
        assert_eq!(hash("ğü😀"), "1046cef17b72c9");
        assert_eq!(hash("[{\"a\":true,\"b\":\"x\"},[\"s|always|0\",\"t|GO|1\"]]"), "131fde23e6ce9e");
    }
}
