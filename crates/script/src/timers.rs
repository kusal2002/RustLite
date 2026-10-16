//! Timers (`setTimeout`, `setInterval`), animation frames and
//! `performance.now()` (HTML §8.6, §8.10, High Resolution Time).
//!
//! Times are milliseconds since the realm's time origin. The embedder asks
//! for the next deadline ([`crate::ScriptHost::next_timer_due`]) and calls
//! back with the current time; nothing here sleeps or spawns threads.

use std::collections::BTreeMap;
use std::time::Instant;

use boa_engine::{Context, JsObject, JsResult, JsValue, Source};

use crate::HostState;
use crate::bindings::host;

/// Active timers per realm; more `setTimeout` calls return 0 and do nothing,
/// so a page cannot grow the queue without bound.
pub const MAX_TIMERS: usize = 10_000;
/// Callbacks run per [`run_due`] call before yielding to the embedder.
const MAX_RUNS_PER_TURN: usize = 1_000;
/// Nesting level after which delays are at least 4 ms (HTML §8.6 step 11).
const NESTING_CLAMP_LEVEL: u32 = 5;
const MAX_DELAY_MS: f64 = 2_147_483_647.0;

#[derive(Debug)]
pub(crate) enum Callback {
    Function(JsObject),
    Code(String),
}

#[derive(Debug)]
pub(crate) struct Timer {
    pub due: f64,
    /// Insertion order, so timers with equal deadlines run in call order.
    pub seq: u64,
    pub callback: Callback,
    pub args: Vec<JsValue>,
    pub interval: Option<f64>,
    pub nesting: u32,
}

#[derive(Debug, Default)]
pub(crate) struct Timers {
    pub active: BTreeMap<u32, Timer>,
    pub next_id: u32,
    pub next_seq: u64,
    /// Nesting level of the timer callback now running (0 outside timers).
    pub nesting: u32,
    pub frames: Vec<(u32, JsObject)>,
    pub next_frame_id: u32,
}

/// Milliseconds since the realm's origin, coarsened to 0.1 ms like browsers
/// (precise timers help timing side channels).
pub(crate) fn now_ms(origin: Instant, now: Instant) -> f64 {
    let ms = now.saturating_duration_since(origin).as_secs_f64() * 1000.0;
    (ms * 10.0).floor() / 10.0
}

fn delay_ms(value: &JsValue, ctx: &mut Context) -> JsResult<f64> {
    let d = value.to_number(ctx)?;
    Ok(if d.is_finite() {
        d.clamp(0.0, MAX_DELAY_MS)
    } else {
        0.0
    })
}

fn schedule(args: &[JsValue], ctx: &mut Context, repeat: bool) -> JsResult<JsValue> {
    let callback = match args.first() {
        Some(v) if v.as_callable().is_some() => {
            Callback::Function(v.as_callable().expect("checked"))
        }
        Some(v) => Callback::Code(v.to_string(ctx)?.to_std_string_escaped()),
        None => return Ok(JsValue::from(0)),
    };
    let mut delay = delay_ms(&args.get(1).cloned().unwrap_or_default(), ctx)?;
    let extra = args.get(2..).map(<[JsValue]>::to_vec).unwrap_or_default();
    let state: &mut HostState = host(ctx)?;
    if state.timers.active.len() >= MAX_TIMERS {
        tracing::warn!(target: "console", "timer limit ({MAX_TIMERS}) reached; timer ignored");
        return Ok(JsValue::from(0));
    }
    let nesting = state.timers.nesting + 1;
    if nesting > NESTING_CLAMP_LEVEL {
        delay = delay.max(4.0);
    }
    let now = now_ms(state.origin, Instant::now());
    state.timers.next_id = state.timers.next_id.wrapping_add(1).max(1);
    let id = state.timers.next_id;
    state.timers.next_seq += 1;
    let seq = state.timers.next_seq;
    state.timers.active.insert(
        id,
        Timer {
            due: now + delay,
            seq,
            callback,
            args: extra,
            interval: repeat.then_some(delay),
            nesting,
        },
    );
    Ok(JsValue::from(id))
}

pub(crate) fn set_timeout(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    schedule(args, ctx, false)
}

pub(crate) fn set_interval(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    schedule(args, ctx, true)
}

pub(crate) fn clear_timer(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = args.first().cloned().unwrap_or_default().to_number(ctx)?;
    if id.is_finite() && id >= 1.0 && id <= f64::from(u32::MAX) {
        host(ctx)?.timers.active.remove(&(id as u32));
    }
    Ok(JsValue::undefined())
}

pub(crate) fn request_animation_frame(
    _: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(f) = args.first().and_then(JsValue::as_callable) else {
        return Err(boa_engine::JsNativeError::typ()
            .with_message("requestAnimationFrame needs a function")
            .into());
    };
    let t = &mut host(ctx)?.timers;
    if t.frames.len() >= MAX_TIMERS {
        return Ok(JsValue::from(0));
    }
    t.next_frame_id = t.next_frame_id.wrapping_add(1).max(1);
    let id = t.next_frame_id;
    t.frames.push((id, f));
    Ok(JsValue::from(id))
}

pub(crate) fn cancel_animation_frame(
    _: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let id = args.first().cloned().unwrap_or_default().to_number(ctx)?;
    host(ctx)?
        .timers
        .frames
        .retain(|(i, _)| f64::from(*i) != id);
    Ok(JsValue::undefined())
}

pub(crate) fn performance_now(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let origin = host(ctx)?.origin;
    Ok(JsValue::from(now_ms(origin, Instant::now())))
}

/// Runs timers due at `now` in deadline order. Returns uncaught errors.
pub(crate) fn run_due(ctx: &mut Context, now: Instant) -> Vec<String> {
    let mut errors = Vec::new();
    for _ in 0..MAX_RUNS_PER_TURN {
        let Ok(state) = host(ctx) else { break };
        let now_ms = now_ms(state.origin, now);
        let next = state
            .timers
            .active
            .iter()
            .filter(|(_, t)| t.due <= now_ms)
            .min_by(|a, b| a.1.due.total_cmp(&b.1.due).then(a.1.seq.cmp(&b.1.seq)))
            .map(|(&id, _)| id);
        let Some(id) = next else { break };
        let Some(timer) = state.timers.active.remove(&id) else {
            break;
        };
        state.timers.nesting = timer.nesting;
        // Re-arm intervals before running, so `clearInterval` inside the
        // callback cancels the next run.
        if let Some(interval) = timer.interval {
            let interval = if timer.nesting >= NESTING_CLAMP_LEVEL {
                interval.max(4.0)
            } else {
                interval.max(1.0)
            };
            state.timers.next_seq += 1;
            let seq = state.timers.next_seq;
            state.timers.active.insert(
                id,
                Timer {
                    due: now_ms + interval,
                    seq,
                    callback: match &timer.callback {
                        Callback::Function(f) => Callback::Function(f.clone()),
                        Callback::Code(c) => Callback::Code(c.clone()),
                    },
                    args: timer.args.clone(),
                    interval: Some(interval),
                    nesting: timer.nesting + 1,
                },
            );
        }
        let global: JsValue = ctx.global_object().into();
        let result = match &timer.callback {
            Callback::Function(f) => f.call(&global, &timer.args, ctx).map(drop),
            Callback::Code(code) => ctx.eval(Source::from_bytes(code.as_bytes())).map(drop),
        };
        if let Err(e) = result {
            errors.push(format!("timer callback: {e}"));
        }
        if let Err(e) = ctx.run_jobs() {
            errors.push(format!("timer (promise job): {e}"));
        }
        if let Ok(state) = host(ctx) {
            state.timers.nesting = 0;
        }
    }
    errors
}

/// Runs the animation frame callbacks registered before this call.
pub(crate) fn run_frames(ctx: &mut Context, now: Instant) -> Vec<String> {
    let mut errors = Vec::new();
    let Ok(state) = host(ctx) else {
        return errors;
    };
    let timestamp = now_ms(state.origin, now);
    let frames = std::mem::take(&mut state.timers.frames);
    let global: JsValue = ctx.global_object().into();
    for (_, f) in frames {
        if let Err(e) = f.call(&global, &[JsValue::from(timestamp)], ctx) {
            errors.push(format!("animation frame callback: {e}"));
        }
        if let Err(e) = ctx.run_jobs() {
            errors.push(format!("animation frame (promise job): {e}"));
        }
    }
    errors
}
