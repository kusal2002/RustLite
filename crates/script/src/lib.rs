//! JavaScript for RustLite: the Boa engine bound to a subset of the DOM.
//!
//! Ownership model: a page owns its [`Document`]. While a script runs (or an
//! event handler is called), the document is *lent* to the engine: moved into
//! the context's host data, then moved back when the call returns. Bindings
//! therefore never alias the page's DOM and no `RefCell` or `unsafe` is
//! needed. JavaScript wrapper objects map to nodes through a table keyed by
//! object identity, so scripts cannot forge a reference to a node.
//!
//! Exposed to pages: `window`/`globalThis`, `document`, `console`, a node
//! and element subset (see docs/compatibility.md), events with capture-free
//! bubbling, and `on*` handler attributes for dispatched events. Nothing
//! privileged is exposed: no file system, network, timers or browser UI.
//!
//! Limits: per-loop iteration and recursion limits stop runaway scripts;
//! there is no total time or heap limit yet (see docs/security.md).

mod bindings;
mod serialize;
mod style_attr;
mod timers;

use std::collections::HashMap;
use std::time::Instant;

use boa_engine::{Context, JsObject, JsValue, Source};
use rustlite_dom::{Document, NodeId};

pub use bindings::uncheck_radio_group;
pub use serialize::serialize_children;

/// Iterations one loop may run before the script is stopped.
pub const LOOP_ITERATION_LIMIT: u64 = 10_000_000;
/// Maximum JavaScript call depth.
pub const RECURSION_LIMIT: usize = 400;

/// Where an event listener is registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Target {
    Node(NodeId),
    Window,
}

/// Flags of the event being dispatched (dispatch is not re-entrant yet).
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct DispatchFlags {
    pub default_prevented: bool,
    pub stop_propagation: bool,
    pub stop_immediate: bool,
}

/// Engine-side state, kept in the Boa context's host data.
#[derive(Debug)]
pub(crate) struct HostState {
    /// The lent document (an empty placeholder between calls).
    pub dom: Document,
    pub wrappers: HashMap<NodeId, JsObject>,
    pub nodes: HashMap<JsObject, NodeId>,
    pub listeners: HashMap<(Target, String), Vec<JsObject>>,
    /// Handlers set through properties such as `el.onclick = f`.
    pub property_handlers: HashMap<(NodeId, String), JsObject>,
    /// Compiled `on*` attribute handlers, keyed by node, event and source.
    pub attribute_handlers: HashMap<(NodeId, String), (String, JsObject)>,
    pub node_proto: JsObject,
    /// `element.style` objects, one per element, and their elements.
    pub style_objects: HashMap<NodeId, JsObject>,
    pub style_nodes: HashMap<JsObject, NodeId>,
    pub style_proto: JsObject,
    pub element_proto: JsObject,
    pub document_proto: JsObject,
    pub dispatch: DispatchFlags,
    /// A dispatch is in progress (synthetic `click()` inside it is ignored).
    pub dispatching: bool,
    /// `console.*` output, newest last (bounded).
    pub console: Vec<String>,
    /// Time origin for timers and `performance.now()`.
    pub origin: Instant,
    pub timers: timers::Timers,
}

/// Cap on retained console lines.
const CONSOLE_LINES: usize = 200;

impl HostState {
    pub fn log(&mut self, line: String) {
        tracing::info!(target: "console", "{line}");
        if self.console.len() == CONSOLE_LINES {
            self.console.remove(0);
        }
        self.console.push(line);
    }
}

/// Options for [`ScriptHost::dispatch_event_init`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventInit {
    /// The event stops at its target (e.g. `focus`, `blur`).
    pub no_bubble: bool,
    /// `event.key` for keyboard events.
    pub key: Option<String>,
}

/// Result of running scripts or dispatching an event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The document changed (restyle and relayout needed).
    pub mutated: bool,
    /// Uncaught exceptions and limit errors, as messages.
    pub errors: Vec<String>,
    /// For dispatches: a listener called `preventDefault()`.
    pub default_prevented: bool,
}

/// One JavaScript realm bound to one document.
pub struct ScriptHost {
    context: Context,
}

impl std::fmt::Debug for ScriptHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptHost").finish_non_exhaustive()
    }
}

impl ScriptHost {
    /// A realm for the document at `url`, with the DOM bindings installed.
    pub fn new(url: &str) -> Self {
        let mut context = Context::default();
        let limits = context.runtime_limits_mut();
        limits.set_loop_iteration_limit(LOOP_ITERATION_LIMIT);
        limits.set_recursion_limit(RECURSION_LIMIT);
        bindings::install(&mut context, url);
        Self { context }
    }

    fn state(&mut self) -> &mut HostState {
        self.context
            .host_defined_mut()
            .get_mut::<HostState>()
            .expect("installed by ScriptHost::new")
    }

    /// Lends `dom` to the engine for the duration of `f`.
    fn with_document<R>(&mut self, dom: &mut Document, f: impl FnOnce(&mut Context) -> R) -> R {
        std::mem::swap(&mut self.state().dom, dom);
        let result = f(&mut self.context);
        std::mem::swap(&mut self.state().dom, dom);
        result
    }

    /// Runs a classic script against `dom`. `name` labels errors.
    pub fn run_script(&mut self, dom: &mut Document, source: &str, name: &str) -> Outcome {
        let generation = (dom.generation(), dom.form_generation());
        let errors = self.with_document(dom, |ctx| {
            let mut errors = Vec::new();
            if let Err(e) = ctx.eval(Source::from_bytes(source.as_bytes())) {
                errors.push(format!("{name}: {e}"));
            }
            if let Err(e) = ctx.run_jobs() {
                errors.push(format!("{name} (promise job): {e}"));
            }
            errors
        });
        for e in &errors {
            tracing::warn!(target: "console", "uncaught {e}");
        }
        Outcome {
            mutated: (dom.generation(), dom.form_generation()) != generation,
            errors,
            default_prevented: false,
        }
    }

    /// Dispatches a bubbling event of type `kind` at `target` (then its
    /// ancestors, the document and the window).
    pub fn dispatch_event(&mut self, dom: &mut Document, target: NodeId, kind: &str) -> Outcome {
        let generation = (dom.generation(), dom.form_generation());
        let (prevented, errors) =
            self.with_document(dom, |ctx| bindings::dispatch(ctx, Some(target), kind));
        Outcome {
            mutated: (dom.generation(), dom.form_generation()) != generation,
            errors,
            default_prevented: prevented,
        }
    }

    /// Dispatches `kind` at `target` with explicit options.
    pub fn dispatch_event_init(
        &mut self,
        dom: &mut Document,
        target: NodeId,
        kind: &str,
        init: &EventInit,
    ) -> Outcome {
        let before = (dom.generation(), dom.form_generation());
        let (prevented, errors) = self.with_document(dom, |ctx| {
            bindings::dispatch_init(ctx, Some(target), kind, init)
        });
        Outcome {
            mutated: (dom.generation(), dom.form_generation()) != before,
            errors,
            default_prevented: prevented,
        }
    }

    /// Dispatches an event at the window only (e.g. `load`).
    pub fn dispatch_window_event(&mut self, dom: &mut Document, kind: &str) -> Outcome {
        let generation = (dom.generation(), dom.form_generation());
        let (prevented, errors) =
            self.with_document(dom, |ctx| bindings::dispatch(ctx, None, kind));
        Outcome {
            mutated: (dom.generation(), dom.form_generation()) != generation,
            errors,
            default_prevented: prevented,
        }
    }

    /// Does any listener, handler property or `on*` attribute exist for
    /// `kind` on `target` or its ancestors? Lets callers skip dispatch work.
    pub fn has_listeners(&mut self, dom: &Document, target: NodeId, kind: &str) -> bool {
        let attr = format!("on{kind}");
        let state = self.state();
        let window = state
            .listeners
            .contains_key(&(Target::Window, kind.to_owned()));
        window
            || std::iter::once(target)
                .chain(dom.ancestors(target))
                .any(|n| {
                    state
                        .listeners
                        .contains_key(&(Target::Node(n), kind.to_owned()))
                        || state.property_handlers.contains_key(&(n, kind.to_owned()))
                        || dom.attribute(n, &attr).is_some()
                })
    }

    /// When the earliest timer is due, if any timer is active.
    pub fn next_timer_due(&mut self) -> Option<Instant> {
        let state = self.state();
        let due = state
            .timers
            .active
            .values()
            .map(|t| t.due)
            .min_by(f64::total_cmp)?;
        Some(state.origin + std::time::Duration::from_secs_f64(due.max(0.0) / 1000.0))
    }

    /// Runs timer callbacks due at `now` (deadline order; at most 1,000 per
    /// call so a page cannot starve the browser).
    pub fn run_due_timers(&mut self, dom: &mut Document, now: Instant) -> Outcome {
        let generation = (dom.generation(), dom.form_generation());
        let errors = self.with_document(dom, |ctx| timers::run_due(ctx, now));
        Outcome {
            mutated: (dom.generation(), dom.form_generation()) != generation,
            errors,
            default_prevented: false,
        }
    }

    /// Is a `requestAnimationFrame` callback waiting for the next frame?
    pub fn wants_animation_frame(&mut self) -> bool {
        !self.state().timers.frames.is_empty()
    }

    /// Runs the waiting animation frame callbacks (before painting a frame).
    pub fn run_animation_frames(&mut self, dom: &mut Document, now: Instant) -> Outcome {
        let generation = (dom.generation(), dom.form_generation());
        let errors = self.with_document(dom, |ctx| timers::run_frames(ctx, now));
        Outcome {
            mutated: (dom.generation(), dom.form_generation()) != generation,
            errors,
            default_prevented: false,
        }
    }

    /// Console output so far (bounded).
    pub fn console(&mut self) -> &[String] {
        &self.state().console
    }
}

/// Is `type_attr` (a `<script type>` value) a classic JavaScript type?
/// Modules and data blocks are not run.
pub fn is_classic_script_type(type_attr: Option<&str>) -> bool {
    let Some(t) = type_attr.map(str::trim) else {
        return true;
    };
    if t.is_empty() {
        return true;
    }
    let essence = t
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        essence.as_str(),
        "text/javascript"
            | "application/javascript"
            | "application/ecmascript"
            | "application/x-javascript"
            | "text/ecmascript"
            | "text/jscript"
            | "text/x-javascript"
    )
}

/// Convenience used by tests and tools: the value of a global expression as
/// a string, evaluated against `dom`.
pub fn eval_to_string(
    host: &mut ScriptHost,
    dom: &mut Document,
    expr: &str,
) -> Result<String, String> {
    host.with_document(dom, |ctx| {
        let v: JsValue = ctx
            .eval(Source::from_bytes(expr.as_bytes()))
            .map_err(|e| e.to_string())?;
        v.to_string(ctx)
            .map(|s| s.to_std_string_escaped())
            .map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests;
