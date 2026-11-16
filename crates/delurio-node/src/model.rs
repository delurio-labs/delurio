//! model.js: the model checker (model.ts). The model and an exploration stay in the addon; a context crosses as the
//! places of its values in the model's tables (`Ctx`), which js/model.js turns into the values themselves and back,
//! so a context the caller hands in keeps its values, objects included, as the TypeScript arrays kept them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use delurio_core::error::Error as CoreError;
use delurio_core::model::{
    By, Config, Ctx, Exploration, Model, Scalar, Step, check_assertions, cover, explore, final_paths, path_to,
    transitions_of,
};
use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::conv::{json, ok, parse_opt, place_json};
use crate::graph::NativeValue;

#[napi]
pub struct NativeModel {
    pub(crate) m: Model,
}

/// the steps out of a pair: one entry per step in each list, the contexts one after the other
#[napi(object)]
pub struct StepsOut {
    pub event: Vec<Option<String>>,
    pub target: Vec<String>,
    pub taken: Vec<String>,
    pub ctx: Uint16Array,
}

/// contexts, one after the other
#[napi(object)]
pub struct CtxList {
    pub count: u32,
    pub ctx: Uint16Array,
}

fn steps_out(steps: Vec<Step>) -> StepsOut {
    let mut out = StepsOut { event: Vec::new(), target: Vec::new(), taken: Vec::new(), ctx: Uint16Array::new(vec![]) };
    let mut ctx: Vec<u16> = Vec::new();
    for s in steps {
        out.event.push(s.event.map(|e| e.to_string()));
        out.target.push(s.target.to_string());
        out.taken.push(s.taken.to_string());
        ctx.extend_from_slice(&s.ctx);
    }
    out.ctx = Uint16Array::new(ctx);
    out
}

/// contexts given one after the other, `count` of them
pub(crate) fn contexts(flat: &[u16], count: usize, n: usize) -> Vec<Ctx> {
    (0..count).map(|k| flat.get(k * n..(k + 1) * n).unwrap_or(&[]).to_vec().into_boxed_slice()).collect()
}

/// a value of a context as JSON, None for undefined
fn scalar_json(s: &Scalar) -> Result<Option<String>> {
    match s {
        Scalar::Undefined => Ok(None),
        other => json(&other.to_value()).map(Some),
    }
}

#[napi]
impl NativeModel {
    /// the model of a flow the addon holds (js/internal.js reads each flow object once)
    #[napi(constructor)]
    pub fn new(flow: &NativeValue) -> Self {
        NativeModel { m: Model::new(flow.v.clone()) }
    }

    #[napi]
    pub fn vars(&self) -> Vec<String> {
        self.m.vars.clone()
    }

    /// for each variable, the place of each of its values (`flow.context[v].values`) in its table
    #[napi]
    pub fn domain_codes(&self) -> Vec<Vec<u32>> {
        self.m
            .domains
            .iter()
            .enumerate()
            .map(|(i, values)| values.iter().map(|v| self.m.code_of(i, v).map_or(u32::MAX, u32::from)).collect())
            .collect()
    }

    /// the variables that start from any of their values (`initial: any`), by position
    #[napi]
    pub fn any_vars(&self) -> Vec<u32> {
        (0..self.m.vars.len()).filter(|&i| self.m.initial_code(i).is_none()).map(|i| i as u32).collect()
    }

    /// the value at a place of a variable's table, as JSON; undefined as None
    #[napi]
    pub fn value_at(&self, var: u32, code: u32) -> Result<Option<String>> {
        scalar_json(&self.m.value_at(var as usize, code as u16))
    }

    /// the place of a value in a variable's table, added when no table holds it (undefined: None)
    #[napi]
    pub fn code_of(&self, var: u32, value: Option<String>) -> Result<u32> {
        let s = match parse_opt(value)? {
            None => Scalar::Undefined,
            Some(v) => Scalar::of(&v),
        };
        ok(self.m.code_of(var as usize, &s)).map(u32::from)
    }

    #[napi]
    pub fn steps(&self, state: String, ctx: Uint16Array) -> Result<StepsOut> {
        ok(self.m.steps(&state, &ctx)).map(steps_out)
    }

    #[napi]
    pub fn initials(&self) -> CtxList {
        let all = self.m.initials();
        let count = all.len() as u32;
        CtxList { count, ctx: Uint16Array::new(all.into_iter().flat_map(|c| c.into_vec()).collect()) }
    }

    #[napi]
    pub fn test(&self, expr: String, state: String, ctx: Uint16Array) -> Result<bool> {
        ok(self.m.test(&expr, &state, &ctx))
    }

    /// `value(expr, state, ctx)`, as JSON
    #[napi]
    pub fn value(&self, expr: String, state: String, ctx: Uint16Array) -> Result<String> {
        json(&ok(self.m.value(&expr, &state, &ctx))?)
    }

    #[napi]
    pub fn pure(&self, src: String) -> bool {
        self.m.pure(&src)
    }

    /// `inState(src, state)`, as JSON; undefined as None
    #[napi]
    pub fn in_state(&self, src: String, state: String) -> Result<Option<String>> {
        ok(self.m.in_state(&src, &state))?.map(|v| json(&v)).transpose()
    }

    /// what `tester(expr)` throws before it is run: nothing, or the error of an expression that cannot be read
    #[napi]
    pub fn tester(&self, expr: String) -> Result<()> {
        ok(self.m.tester(&expr)).map(|_| ())
    }

    /// the place `placeError` gives an error without one that `steps(state, ctx)` threw, as JSON
    #[napi]
    pub fn place_of(&self, message: String, state: String, ctx: Uint16Array) -> Option<String> {
        let placed = self.m.place_error(CoreError::model(message), &state, &ctx);
        placed.place.as_ref().map(place_json)
    }
}

/// an exploration the addon holds for the JavaScript object explore() gave; V8 is told how much memory it holds, so
/// that an unread exploration is collected as soon as a JavaScript one of its size would be
#[napi(custom_finalize)]
pub struct NativeExploration {
    pub(crate) ex: Exploration,
    bytes: i64,
}

impl NativeExploration {
    fn held(env: &Env, ex: Exploration) -> NativeExploration {
        let bytes = i64::try_from(ex.bytes()).unwrap_or(i64::MAX);
        let _ = env.adjust_external_memory(bytes);
        NativeExploration { ex, bytes }
    }
}

impl ObjectFinalize for NativeExploration {
    fn finalize(self, env: Env) -> Result<()> {
        env.adjust_external_memory(-self.bytes).map(|_| ())
    }
}

/// an exploration's pairs, each list one entry per pair: the state and the event as places in `names` (event 0 is
/// null, k is names[k - 1]), the parent as its position (-1 for a start), the contexts one after the other
#[napi(object)]
pub struct ConfigsOut {
    pub names: Vec<String>,
    pub state: Uint32Array,
    pub parent: Int32Array,
    pub event: Uint32Array,
    pub ctx: Uint16Array,
}

#[napi]
impl NativeExploration {
    #[napi]
    pub fn starts(&self) -> u32 {
        self.ex.starts as u32
    }

    #[napi]
    pub fn size(&self) -> u32 {
        self.ex.len() as u32
    }

    #[napi]
    pub fn taken(&self) -> Vec<String> {
        self.ex.taken.iter().map(|t| t.to_string()).collect()
    }

    /// `atState` as JSON entries
    #[napi]
    pub fn at_state(&self) -> Result<String> {
        let entries: Vec<(&str, usize)> = self.ex.at_state.iter().map(|(k, n)| (&**k, *n)).collect();
        json(&entries)
    }

    /// `branchCounts` as JSON entries
    #[napi]
    pub fn branch_counts(&self) -> Result<String> {
        let entries: Vec<(&str, &Vec<usize>)> = self.ex.branch_counts.iter().map(|(k, n)| (&**k, n)).collect();
        json(&entries)
    }

    #[napi]
    pub fn configs(&self) -> ConfigsOut {
        let mut names: Vec<String> = Vec::new();
        let mut at: HashMap<Rc<str>, u32> = HashMap::new();
        let mut place = |s: &Rc<str>| -> u32 {
            *at.entry(s.clone()).or_insert_with(|| {
                names.push(s.to_string());
                (names.len() - 1) as u32
            })
        };
        let n = self.ex.len();
        let (mut state, mut parent, mut event) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
        let mut ctx: Vec<u16> = Vec::new();
        let mut one: Vec<u16> = Vec::new();
        for i in 0..n {
            state.push(self.ex.state_rc(i).map_or(0, &mut place));
            parent.push(self.ex.parent(i).map_or(-1, |p| p as i32));
            event.push(self.ex.event_rc(i).map_or(0, |e| place(e) + 1));
            self.ex.ctx_into(i, &mut one);
            ctx.extend_from_slice(&one);
        }
        ConfigsOut {
            names,
            state: Uint32Array::new(state),
            parent: Int32Array::new(parent),
            event: Uint32Array::new(event),
            ctx: Uint16Array::new(ctx),
        }
    }
}

/// the arguments of a trapped `keep(at, s)`: whether `at` is another pair than the last call's, its state, parent,
/// event and context, and the step's event, target, context and transition. It answers 1 to keep the step.
pub type KeepArgs =
    FnArgs<(bool, String, i32, Option<String>, Uint16Array, Option<String>, String, Uint16Array, String)>;

/// the pair the last `keep` call was given, to tell the caller whether this one is another
struct Last {
    state: Rc<str>,
    parent: Option<u32>,
    event: Option<Rc<str>>,
    ctx: Ctx,
}

/// the caller's `keep`, as the explorer calls it: each call in a handle scope of its own, so that what it is handed
/// is let go of at once and not when the exploration is done
fn keeper<'a>(env: &'a Env, keep: &'a Function<'a, KeepArgs, u32>) -> impl Fn(&Config, &Step) -> bool + 'a {
    let last: RefCell<Option<Last>> = RefCell::new(None);
    move |at: &Config, s: &Step| {
        let fresh = {
            let mut last = last.borrow_mut();
            let same = last.as_ref().is_some_and(|l| {
                Rc::ptr_eq(&l.state, &at.state) && l.parent == at.parent && l.event == at.event && l.ctx == at.ctx
            });
            if !same {
                *last = Some(Last {
                    state: at.state.clone(),
                    parent: at.parent,
                    event: at.event.clone(),
                    ctx: at.ctx.clone(),
                });
            }
            !same
        };
        let args = (
            fresh,
            at.state.to_string(),
            at.parent.map_or(-1, |p| p as i32),
            at.event.as_ref().map(|e| e.to_string()),
            Uint16Array::new(at.ctx.to_vec()),
            s.event.as_ref().map(|e| e.to_string()),
            s.target.to_string(),
            Uint16Array::new(s.ctx.to_vec()),
            s.taken.to_string(),
        );
        matches!(env.run_in_scope(|| keep.call(args.into())), Ok(1))
    }
}

fn starts_of(m: &Model, from: Option<Uint16Array>, count: Option<u32>) -> Option<Vec<Ctx>> {
    let from = from?;
    Some(contexts(&from, count.unwrap_or(0) as usize, m.vars.len()))
}

/// `explore(m, from, keep)`
#[napi(js_name = "explore")]
pub fn explore_flow(
    env: Env,
    m: &NativeModel,
    from: Option<Uint16Array>,
    count: Option<u32>,
    keep: Option<Function<KeepArgs, u32>>,
) -> Result<NativeExploration> {
    let from = starts_of(&m.m, from, count);
    let ex = match &keep {
        Some(k) => {
            let f = keeper(&env, k);
            explore(&m.m, from, Some(&f))
        }
        None => explore(&m.m, from, None),
    };
    ok(ex).map(|ex| NativeExploration::held(&env, ex))
}

/// `cover(m, by, from, keep)`, as JSON
#[napi(js_name = "cover")]
pub fn cover_flow(
    env: Env,
    m: &NativeModel,
    by: String,
    from: Option<Uint16Array>,
    count: Option<u32>,
    keep: Option<Function<KeepArgs, u32>>,
) -> Result<String> {
    let by = if by == "states" { By::States } else { By::Transitions };
    let from = starts_of(&m.m, from, count);
    let c = match &keep {
        Some(k) => {
            let f = keeper(&env, k);
            cover(&m.m, by, from, Some(&f))
        }
        None => cover(&m.m, by, from, None),
    };
    json(&ok(c)?)
}

/// `pathTo(m, ex, index)`, as JSON
#[napi(js_name = "pathTo")]
pub fn path_to_config(m: &NativeModel, ex: &NativeExploration, index: u32) -> Result<String> {
    json(&path_to(&m.m, &ex.ex, index as usize))
}

/// `checkAssertions(m, ex)`, as JSON
#[napi(js_name = "checkAssertions")]
pub fn check_assertions_of(m: &NativeModel, ex: &NativeExploration) -> Result<String> {
    json(&ok(check_assertions(&m.m, &ex.ex))?)
}

/// `finalPaths(m, ex)`, as JSON
#[napi(js_name = "finalPaths")]
pub fn final_paths_of(m: &NativeModel, ex: &NativeExploration) -> Result<String> {
    json(&final_paths(&m.m, &ex.ex))
}

/// `transitionsOf(flow)`
#[napi(js_name = "transitionsOf")]
pub fn transitions_of_flow(flow: &NativeValue) -> Vec<String> {
    transitions_of(delurio_core::graph::Flow(&flow.v))
}
