//! DOM bindings: prototypes, accessors and methods exposed to scripts.

use std::collections::HashMap;

use boa_engine::native_function::NativeFunctionPointer;
use boa_engine::object::FunctionObjectBuilder;
use boa_engine::object::builtins::JsArray;
use boa_engine::property::{Attribute as PropAttr, PropertyDescriptor};
use boa_engine::{
    Context, JsError, JsNativeError, JsObject, JsResult, JsString, JsValue, NativeFunction, Source,
    js_string,
};
use rustlite_dom::{Attribute, Document, NodeData, NodeId};

use crate::{DispatchFlags, HostState, Target, serialize};

// ----- helpers -----------------------------------------------------------

pub(crate) fn host(ctx: &mut Context) -> JsResult<&mut HostState> {
    ctx.host_defined_mut()
        .get_mut::<HostState>()
        .ok_or_else(|| JsNativeError::error().with_message("no document").into())
}

fn type_error(message: &str) -> JsError {
    JsNativeError::typ().with_message(message.to_owned()).into()
}

fn syntax_error(message: &str) -> JsError {
    JsNativeError::syntax()
        .with_message(message.to_owned())
        .into()
}

fn arg(args: &[JsValue], i: usize) -> JsValue {
    args.get(i).cloned().unwrap_or_default()
}

fn arg_string(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<String> {
    Ok(arg(args, i).to_string(ctx)?.to_std_string_escaped())
}

fn string(s: &str) -> JsValue {
    JsString::from(s).into()
}

/// The node a wrapper stands for; `TypeError` for anything else.
fn node_of(value: &JsValue, ctx: &mut Context) -> JsResult<NodeId> {
    let obj = value.as_object().ok_or_else(|| type_error("not a Node"))?;
    host(ctx)?
        .nodes
        .get(&obj)
        .copied()
        .ok_or_else(|| type_error("not a Node"))
}

fn this_node(this: &JsValue, ctx: &mut Context) -> JsResult<NodeId> {
    node_of(this, ctx)
}

fn this_element(this: &JsValue, ctx: &mut Context) -> JsResult<NodeId> {
    let n = node_of(this, ctx)?;
    match host(ctx)?.dom.data(n) {
        NodeData::Element(_) => Ok(n),
        _ => Err(type_error("not an Element")),
    }
}

/// The (unique) wrapper for `node`.
pub(crate) fn wrap(node: NodeId, ctx: &mut Context) -> JsResult<JsValue> {
    let (existing, proto) = {
        let s = host(ctx)?;
        let proto = match s.dom.data(node) {
            NodeData::Document => s.document_proto.clone(),
            NodeData::Element(_) => s.element_proto.clone(),
            _ => s.node_proto.clone(),
        };
        (s.wrappers.get(&node).cloned(), proto)
    };
    if let Some(obj) = existing {
        return Ok(obj.into());
    }
    let obj = JsObject::with_object_proto(ctx.intrinsics());
    obj.set_prototype(Some(proto));
    let s = host(ctx)?;
    s.wrappers.insert(node, obj.clone());
    s.nodes.insert(obj.clone(), node);
    Ok(obj.into())
}

fn wrap_opt(node: Option<NodeId>, ctx: &mut Context) -> JsResult<JsValue> {
    match node {
        Some(n) => wrap(n, ctx),
        None => Ok(JsValue::null()),
    }
}

fn wrap_all(nodes: Vec<NodeId>, ctx: &mut Context) -> JsResult<JsValue> {
    let mut values = Vec::with_capacity(nodes.len());
    for n in nodes {
        values.push(wrap(n, ctx)?);
    }
    Ok(JsArray::from_iter(values, ctx).into())
}

fn function(ctx: &mut Context, name: &str, length: usize, f: NativeFunctionPointer) -> JsObject {
    FunctionObjectBuilder::new(ctx.realm(), NativeFunction::from_fn_ptr(f))
        .name(JsString::from(name))
        .length(length)
        .build()
        .into()
}

fn method(ctx: &mut Context, obj: &JsObject, name: &str, length: usize, f: NativeFunctionPointer) {
    let func = function(ctx, name, length, f);
    obj.define_property_or_throw(
        JsString::from(name),
        PropertyDescriptor::builder()
            .value(func)
            .writable(true)
            .enumerable(false)
            .configurable(true),
        ctx,
    )
    .expect("defining a method on a fresh object cannot fail");
}

fn accessor(
    ctx: &mut Context,
    obj: &JsObject,
    name: &str,
    get: NativeFunctionPointer,
    set: Option<NativeFunctionPointer>,
) {
    let getter = function(ctx, &format!("get {name}"), 0, get);
    let mut desc = PropertyDescriptor::builder()
        .get(getter)
        .enumerable(true)
        .configurable(true);
    desc = match set {
        Some(s) => desc.set(function(ctx, &format!("set {name}"), 1, s)),
        None => desc.set(JsValue::undefined()),
    };
    obj.define_property_or_throw(JsString::from(name), desc, ctx)
        .expect("defining an accessor on a fresh object cannot fail");
}

fn with_dom<R>(ctx: &mut Context, f: impl FnOnce(&Document) -> R) -> JsResult<R> {
    Ok(f(&host(ctx)?.dom))
}

fn with_dom_mut<R>(ctx: &mut Context, f: impl FnOnce(&mut Document) -> R) -> JsResult<R> {
    Ok(f(&mut host(ctx)?.dom))
}

fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

// ----- installation ------------------------------------------------------

pub(crate) fn install(ctx: &mut Context, url: &str) {
    let node_proto = JsObject::with_object_proto(ctx.intrinsics());
    let element_proto = JsObject::with_object_proto(ctx.intrinsics());
    element_proto.set_prototype(Some(node_proto.clone()));
    let document_proto = JsObject::with_object_proto(ctx.intrinsics());
    document_proto.set_prototype(Some(node_proto.clone()));

    // Node
    accessor(ctx, &node_proto, "nodeType", node_type, None);
    accessor(ctx, &node_proto, "nodeName", node_name, None);
    accessor(
        ctx,
        &node_proto,
        "textContent",
        text_content,
        Some(set_text_content),
    );
    accessor(ctx, &node_proto, "parentNode", parent_node, None);
    accessor(ctx, &node_proto, "parentElement", parent_element, None);
    accessor(ctx, &node_proto, "firstChild", first_child, None);
    accessor(ctx, &node_proto, "lastChild", last_child, None);
    accessor(ctx, &node_proto, "nextSibling", next_sibling, None);
    accessor(ctx, &node_proto, "previousSibling", previous_sibling, None);
    accessor(ctx, &node_proto, "childNodes", child_nodes, None);
    accessor(ctx, &node_proto, "isConnected", is_connected, None);
    method(ctx, &node_proto, "appendChild", 1, append_child);
    method(ctx, &node_proto, "removeChild", 1, remove_child);
    method(ctx, &node_proto, "insertBefore", 2, insert_before);
    method(ctx, &node_proto, "contains", 1, contains);
    method(ctx, &node_proto, "addEventListener", 2, add_event_listener);
    method(
        ctx,
        &node_proto,
        "removeEventListener",
        2,
        remove_event_listener,
    );

    // Element
    accessor(ctx, &element_proto, "tagName", tag_name, None);
    accessor(ctx, &element_proto, "id", id, Some(set_id));
    accessor(
        ctx,
        &element_proto,
        "className",
        class_name,
        Some(set_class_name),
    );
    accessor(
        ctx,
        &element_proto,
        "innerHTML",
        inner_html,
        Some(set_inner_html),
    );
    accessor(
        ctx,
        &element_proto,
        "innerText",
        text_content,
        Some(set_text_content),
    );
    accessor(ctx, &element_proto, "children", children, None);
    accessor(
        ctx,
        &element_proto,
        "firstElementChild",
        first_element_child,
        None,
    );
    accessor(ctx, &element_proto, "classList", class_list, None);
    accessor(ctx, &element_proto, "onclick", onclick, Some(set_onclick));
    method(ctx, &element_proto, "getAttribute", 1, get_attribute);
    method(ctx, &element_proto, "setAttribute", 2, set_attribute);
    method(ctx, &element_proto, "removeAttribute", 1, remove_attribute);
    method(ctx, &element_proto, "hasAttribute", 1, has_attribute);
    method(ctx, &element_proto, "remove", 0, remove);
    method(ctx, &element_proto, "querySelector", 1, query_selector);
    method(
        ctx,
        &element_proto,
        "querySelectorAll",
        1,
        query_selector_all,
    );
    method(
        ctx,
        &element_proto,
        "getElementsByTagName",
        1,
        get_elements_by_tag_name,
    );
    method(ctx, &element_proto, "matches", 1, matches);
    method(ctx, &element_proto, "closest", 1, closest);
    method(ctx, &element_proto, "click", 0, click);
    accessor(ctx, &element_proto, "value", value, Some(set_value));
    accessor(ctx, &element_proto, "checked", checked, Some(set_checked));
    accessor(ctx, &element_proto, "type", control_type, None);
    accessor(ctx, &element_proto, "name", name, None);
    method(ctx, &element_proto, "focus", 0, focus);
    method(ctx, &element_proto, "blur", 0, blur);

    accessor(ctx, &element_proto, "style", style, Some(set_style));

    // CSSStyleDeclaration (element.style)
    let style_proto = JsObject::with_object_proto(ctx.intrinsics());
    for &name in crate::style_attr::PROPERTIES {
        let get = NativeFunction::from_copy_closure(move |this, _, ctx| style_get(this, name, ctx));
        let set = NativeFunction::from_copy_closure(move |this, args, ctx| {
            style_set(this, name, args, ctx)
        });
        let getter: JsObject = FunctionObjectBuilder::new(ctx.realm(), get)
            .length(0)
            .build()
            .into();
        let setter: JsObject = FunctionObjectBuilder::new(ctx.realm(), set)
            .length(1)
            .build()
            .into();
        let camel = crate::style_attr::camel_case(name);
        let keys = if camel == name {
            vec![camel]
        } else {
            vec![camel, name.to_owned()]
        };
        for key in keys {
            style_proto
                .define_property_or_throw(
                    JsString::from(key.as_str()),
                    PropertyDescriptor::builder()
                        .get(getter.clone())
                        .set(setter.clone())
                        .enumerable(true)
                        .configurable(true),
                    ctx,
                )
                .expect("fresh object");
        }
    }
    accessor(ctx, &style_proto, "cssText", css_text, Some(set_css_text));
    method(ctx, &style_proto, "getPropertyValue", 1, get_property_value);
    method(ctx, &style_proto, "setProperty", 3, set_property);
    method(ctx, &style_proto, "removeProperty", 1, remove_property);

    // Document
    accessor(ctx, &document_proto, "title", title, Some(set_title));
    accessor(ctx, &document_proto, "body", body, None);
    accessor(ctx, &document_proto, "activeElement", active_element, None);
    accessor(ctx, &document_proto, "head", head, None);
    accessor(
        ctx,
        &document_proto,
        "documentElement",
        document_element,
        None,
    );
    method(ctx, &document_proto, "getElementById", 1, get_element_by_id);
    method(ctx, &document_proto, "querySelector", 1, query_selector);
    method(
        ctx,
        &document_proto,
        "querySelectorAll",
        1,
        query_selector_all,
    );
    method(
        ctx,
        &document_proto,
        "getElementsByTagName",
        1,
        get_elements_by_tag_name,
    );
    method(ctx, &document_proto, "createElement", 1, create_element);
    method(ctx, &document_proto, "createTextNode", 1, create_text_node);

    ctx.insert_data(HostState {
        dom: Document::new(),
        wrappers: HashMap::new(),
        nodes: HashMap::new(),
        listeners: HashMap::new(),
        property_handlers: HashMap::new(),
        attribute_handlers: HashMap::new(),
        node_proto,
        style_objects: HashMap::new(),
        style_nodes: HashMap::new(),
        style_proto,
        element_proto,
        document_proto,
        dispatch: DispatchFlags::default(),
        dispatching: false,
        console: Vec::new(),
        origin: std::time::Instant::now(),
        timers: crate::timers::Timers::default(),
    });

    // Globals. `document` is the wrapper of the root node, created lazily
    // through an accessor because no document is lent at install time.
    let global = ctx.global_object();
    accessor(ctx, &global, "document", global_document, None);
    let window = global.clone();
    ctx.register_global_property(js_string!("window"), window.clone(), PropAttr::READONLY)
        .expect("fresh global");
    ctx.register_global_property(js_string!("self"), window, PropAttr::READONLY)
        .expect("fresh global");
    method(
        ctx,
        &global,
        "addEventListener",
        2,
        window_add_event_listener,
    );
    method(
        ctx,
        &global,
        "removeEventListener",
        2,
        window_remove_event_listener,
    );
    method(ctx, &global, "setTimeout", 2, crate::timers::set_timeout);
    method(ctx, &global, "setInterval", 2, crate::timers::set_interval);
    method(ctx, &global, "clearTimeout", 1, crate::timers::clear_timer);
    method(ctx, &global, "clearInterval", 1, crate::timers::clear_timer);
    method(
        ctx,
        &global,
        "requestAnimationFrame",
        1,
        crate::timers::request_animation_frame,
    );
    method(
        ctx,
        &global,
        "cancelAnimationFrame",
        1,
        crate::timers::cancel_animation_frame,
    );
    let performance = JsObject::with_object_proto(ctx.intrinsics());
    method(ctx, &performance, "now", 0, crate::timers::performance_now);
    ctx.register_global_property(js_string!("performance"), performance, PropAttr::READONLY)
        .expect("fresh global");

    let console = JsObject::with_object_proto(ctx.intrinsics());
    for name in ["log", "info", "warn", "error", "debug"] {
        method(ctx, &console, name, 0, console_log);
    }
    ctx.register_global_property(js_string!("console"), console, PropAttr::all())
        .expect("fresh global");

    let location = JsObject::with_object_proto(ctx.intrinsics());
    location
        .define_property_or_throw(
            js_string!("href"),
            PropertyDescriptor::builder()
                .value(string(url))
                .writable(false)
                .enumerable(true)
                .configurable(false),
            ctx,
        )
        .expect("fresh object");
    ctx.register_global_property(js_string!("location"), location, PropAttr::READONLY)
        .expect("fresh global");
}

// ----- globals -----------------------------------------------------------

fn global_document(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let root = with_dom(ctx, Document::root)?;
    wrap(root, ctx)
}

fn console_log(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let mut parts = Vec::with_capacity(args.len());
    for a in args {
        parts.push(match a.to_string(ctx) {
            Ok(s) => s.to_std_string_escaped(),
            Err(_) => "[object]".to_owned(),
        });
    }
    host(ctx)?.log(parts.join(" "));
    Ok(JsValue::undefined())
}

// ----- Node --------------------------------------------------------------

fn node_type(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let t = with_dom(ctx, |d| match d.data(n) {
        NodeData::Element(_) => 1,
        NodeData::Text(_) => 3,
        NodeData::Comment(_) => 8,
        NodeData::Document => 9,
        NodeData::Doctype { .. } => 10,
    })?;
    Ok(JsValue::from(t))
}

fn node_name(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let name = with_dom(ctx, |d| match d.data(n) {
        NodeData::Element(e) => e.name.to_ascii_uppercase(),
        NodeData::Text(_) => "#text".to_owned(),
        NodeData::Comment(_) => "#comment".to_owned(),
        NodeData::Document => "#document".to_owned(),
        NodeData::Doctype { name, .. } => name.clone(),
    })?;
    Ok(string(&name))
}

fn text_content(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let text = with_dom(ctx, |d| match d.data(n) {
        NodeData::Document | NodeData::Doctype { .. } => None,
        _ => Some(d.text_content(n)),
    })?;
    Ok(text.map_or(JsValue::null(), |t| string(&t)))
}

fn set_text_content(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let value = arg(args, 0);
    let text = if value.is_null_or_undefined() {
        String::new()
    } else {
        value.to_string(ctx)?.to_std_string_escaped()
    };
    with_dom_mut(ctx, |d| d.set_text_content(n, &text))?;
    Ok(JsValue::undefined())
}

fn parent_node(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let p = with_dom(ctx, |d| d.parent(n))?;
    wrap_opt(p, ctx)
}

fn parent_element(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let p = with_dom(ctx, |d| d.parent(n).filter(|&p| d.element(p).is_some()))?;
    wrap_opt(p, ctx)
}

fn first_child(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let c = with_dom(ctx, |d| d.first_child(n))?;
    wrap_opt(c, ctx)
}

fn last_child(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let c = with_dom(ctx, |d| d.last_child(n))?;
    wrap_opt(c, ctx)
}

fn next_sibling(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let c = with_dom(ctx, |d| d.next_sibling(n))?;
    wrap_opt(c, ctx)
}

fn previous_sibling(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let c = with_dom(ctx, |d| d.prev_sibling(n))?;
    wrap_opt(c, ctx)
}

fn child_nodes(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let kids = with_dom(ctx, |d| d.children(n).collect())?;
    wrap_all(kids, ctx)
}

fn is_connected(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let connected = with_dom(ctx, |d| d.is_inclusive_ancestor(d.root(), n))?;
    Ok(JsValue::from(connected))
}

fn append_child(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let parent = this_node(this, ctx)?;
    let child = node_of(&arg(args, 0), ctx)?;
    with_dom_mut(ctx, |d| d.append_child(parent, child))?
        .map_err(|_| type_error("HierarchyRequestError: the node cannot be inserted here"))?;
    Ok(arg(args, 0))
}

fn insert_before(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let parent = this_node(this, ctx)?;
    let child = node_of(&arg(args, 0), ctx)?;
    let reference = match arg(args, 1) {
        v if v.is_null_or_undefined() => None,
        v => Some(node_of(&v, ctx)?),
    };
    if let Some(r) = reference
        && with_dom(ctx, |d| d.parent(r))? != Some(parent)
    {
        return Err(type_error(
            "NotFoundError: the reference node is not a child of this node",
        ));
    }
    with_dom_mut(ctx, |d| d.insert_before(parent, child, reference))?
        .map_err(|_| type_error("HierarchyRequestError: the node cannot be inserted here"))?;
    Ok(arg(args, 0))
}

fn remove_child(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let parent = this_node(this, ctx)?;
    let child = node_of(&arg(args, 0), ctx)?;
    if with_dom(ctx, |d| d.parent(child))? != Some(parent) {
        return Err(type_error(
            "NotFoundError: the node is not a child of this node",
        ));
    }
    with_dom_mut(ctx, |d| d.detach(child))?;
    Ok(arg(args, 0))
}

fn contains(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let other = match arg(args, 0) {
        v if v.is_null_or_undefined() => return Ok(JsValue::from(false)),
        v => node_of(&v, ctx)?,
    };
    Ok(JsValue::from(with_dom(ctx, |d| {
        d.is_inclusive_ancestor(n, other)
    })?))
}

fn listener_args(args: &[JsValue], ctx: &mut Context) -> JsResult<Option<(String, JsObject)>> {
    let kind = arg_string(args, 0, ctx)?;
    Ok(arg(args, 1).as_callable().map(|f| (kind, f)))
}

fn add_listener(target: Target, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    if let Some((kind, f)) = listener_args(args, ctx)? {
        let list = host(ctx)?.listeners.entry((target, kind)).or_default();
        // Adding the same listener twice is a no-op (DOM §2.7).
        if !list.contains(&f) {
            list.push(f);
        }
    }
    Ok(JsValue::undefined())
}

fn remove_listener(target: Target, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    if let Some((kind, f)) = listener_args(args, ctx)?
        && let Some(list) = host(ctx)?.listeners.get_mut(&(target, kind))
    {
        list.retain(|l| *l != f);
    }
    Ok(JsValue::undefined())
}

fn add_event_listener(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    add_listener(Target::Node(n), args, ctx)
}

fn remove_event_listener(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    remove_listener(Target::Node(n), args, ctx)
}

fn window_add_event_listener(
    _: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    add_listener(Target::Window, args, ctx)
}

fn window_remove_event_listener(
    _: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    remove_listener(Target::Window, args, ctx)
}

// ----- Element -----------------------------------------------------------

fn tag_name(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let name = with_dom(ctx, |d| d.tag_name(n).unwrap_or("").to_ascii_uppercase())?;
    Ok(string(&name))
}

fn attr_or_empty(this: &JsValue, ctx: &mut Context, name: &str) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let v = with_dom(ctx, |d| d.attribute(n, name).unwrap_or("").to_owned())?;
    Ok(string(&v))
}

fn set_attr(this: &JsValue, args: &[JsValue], ctx: &mut Context, name: &str) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let v = arg_string(args, 0, ctx)?;
    with_dom_mut(ctx, |d| d.set_attribute(n, name, v))?;
    Ok(JsValue::undefined())
}

fn id(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    attr_or_empty(this, ctx, "id")
}

fn set_id(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    set_attr(this, args, ctx, "id")
}

fn class_name(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    attr_or_empty(this, ctx, "class")
}

fn set_class_name(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    set_attr(this, args, ctx, "class")
}

fn inner_html(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let html = with_dom(ctx, |d| serialize::serialize_children(d, n))?;
    Ok(string(&html))
}

fn set_inner_html(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let html = arg_string(args, 0, ctx)?;
    with_dom_mut(ctx, |d| serialize::set_inner_html(d, n, &html))?;
    Ok(JsValue::undefined())
}

fn element_children(d: &Document, n: NodeId) -> Vec<NodeId> {
    d.children(n).filter(|&c| d.element(c).is_some()).collect()
}

fn children(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let kids = with_dom(ctx, |d| element_children(d, n))?;
    wrap_all(kids, ctx)
}

fn first_element_child(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let c = with_dom(ctx, |d| element_children(d, n).first().copied())?;
    wrap_opt(c, ctx)
}

fn get_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let name = arg_string(args, 0, ctx)?.to_ascii_lowercase();
    let v = with_dom(ctx, |d| d.attribute(n, &name).map(str::to_owned))?;
    Ok(v.map_or(JsValue::null(), |v| string(&v)))
}

fn set_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let name = arg_string(args, 0, ctx)?.to_ascii_lowercase();
    if !is_valid_name(&name) {
        return Err(type_error("InvalidCharacterError: invalid attribute name"));
    }
    let value = arg_string(args, 1, ctx)?;
    with_dom_mut(ctx, |d| d.set_attribute(n, &name, value))?;
    Ok(JsValue::undefined())
}

fn remove_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let name = arg_string(args, 0, ctx)?.to_ascii_lowercase();
    with_dom_mut(ctx, |d| d.remove_attribute(n, &name))?;
    Ok(JsValue::undefined())
}

fn has_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let name = arg_string(args, 0, ctx)?.to_ascii_lowercase();
    Ok(JsValue::from(with_dom(ctx, |d| {
        d.attribute(n, &name).is_some()
    })?))
}

fn remove(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    with_dom_mut(ctx, |d| d.detach(n))?;
    Ok(JsValue::undefined())
}

fn selectors(args: &[JsValue], ctx: &mut Context) -> JsResult<rustlite_css::SelectorList> {
    let text = arg_string(args, 0, ctx)?;
    rustlite_style::parse_selectors(&text)
        .ok_or_else(|| syntax_error(&format!("'{text}' is not a valid selector")))
}

fn query_selector(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let list = selectors(args, ctx)?;
    let found = with_dom(ctx, |d| {
        d.descendants(n)
            .skip(1)
            .find(|&e| rustlite_style::element_matches(d, &list, e))
    })?;
    wrap_opt(found, ctx)
}

fn query_selector_all(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let list = selectors(args, ctx)?;
    let found = with_dom(ctx, |d| rustlite_style::query_selector_all(d, n, &list))?;
    wrap_all(found, ctx)
}

fn get_elements_by_tag_name(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let n = this_node(this, ctx)?;
    let name = arg_string(args, 0, ctx)?.to_ascii_lowercase();
    let found = with_dom(ctx, |d| {
        d.descendants(n)
            .skip(1)
            .filter(|&e| d.tag_name(e).is_some_and(|t| name == "*" || t == name))
            .collect()
    })?;
    wrap_all(found, ctx)
}

fn matches(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let list = selectors(args, ctx)?;
    Ok(JsValue::from(with_dom(ctx, |d| {
        rustlite_style::element_matches(d, &list, n)
    })?))
}

fn closest(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let list = selectors(args, ctx)?;
    let found = with_dom(ctx, |d| {
        std::iter::once(n)
            .chain(d.ancestors(n))
            .find(|&e| rustlite_style::element_matches(d, &list, e))
    })?;
    wrap_opt(found, ctx)
}

fn click(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    // Synthetic clicks from inside a listener are ignored rather than
    // re-entering dispatch (dispatch flags are not a stack yet).
    if !host(ctx)?.dispatching {
        let (_, errors) = dispatch(ctx, Some(n), "click");
        for e in errors {
            tracing::warn!(target: "console", "uncaught {e}");
        }
    }
    Ok(JsValue::undefined())
}

fn onclick(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let f = host(ctx)?
        .property_handlers
        .get(&(n, "click".to_owned()))
        .cloned();
    Ok(f.map_or(JsValue::null(), JsValue::from))
}

fn set_onclick(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let handlers = &mut host(ctx)?.property_handlers;
    match arg(args, 0).as_callable() {
        Some(f) => handlers.insert((n, "click".to_owned()), f),
        None => handlers.remove(&(n, "click".to_owned())),
    };
    Ok(JsValue::undefined())
}

// classList: a fresh object per access whose methods close over the node id
// (a `Copy` capture, so no traced data lives outside the GC).

fn class_tokens(d: &Document, n: NodeId) -> Vec<String> {
    d.attribute(n, "class")
        .unwrap_or("")
        .split_ascii_whitespace()
        .map(str::to_owned)
        .collect()
}

fn class_list(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let list = JsObject::with_object_proto(ctx.intrinsics());
    let ops: [(&str, ClassOp); 4] = [
        ("add", ClassOp::Add),
        ("remove", ClassOp::Remove),
        ("toggle", ClassOp::Toggle),
        ("contains", ClassOp::Contains),
    ];
    for (name, op) in ops {
        let f = NativeFunction::from_copy_closure(move |_, args, ctx| class_op(n, op, args, ctx));
        let func: JsObject = FunctionObjectBuilder::new(ctx.realm(), f)
            .name(JsString::from(name))
            .length(1)
            .build()
            .into();
        list.set(JsString::from(name), func, false, ctx)?;
    }
    let tokens = with_dom(ctx, |d| class_tokens(d, n))?;
    list.set(js_string!("length"), tokens.len(), false, ctx)?;
    Ok(list.into())
}

#[derive(Debug, Clone, Copy)]
enum ClassOp {
    Add,
    Remove,
    Toggle,
    Contains,
}

fn class_op(n: NodeId, op: ClassOp, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let mut names = Vec::new();
    for i in 0..args.len().max(1) {
        let name = arg_string(args, i, ctx)?;
        if name.is_empty() || name.contains(|c: char| c.is_ascii_whitespace()) {
            return Err(syntax_error(
                "class names must be non-empty and contain no spaces",
            ));
        }
        names.push(name);
    }
    let mut tokens = with_dom(ctx, |d| class_tokens(d, n))?;
    let result = match op {
        ClassOp::Contains => return Ok(JsValue::from(tokens.contains(&names[0]))),
        ClassOp::Add => {
            for name in names {
                if !tokens.contains(&name) {
                    tokens.push(name);
                }
            }
            JsValue::undefined()
        }
        ClassOp::Remove => {
            tokens.retain(|t| !names.contains(t));
            JsValue::undefined()
        }
        ClassOp::Toggle => {
            let name = &names[0];
            let present = tokens.contains(name);
            if present {
                tokens.retain(|t| t != name);
            } else {
                tokens.push(name.clone());
            }
            JsValue::from(!present)
        }
    };
    let joined = tokens.join(" ");
    with_dom_mut(ctx, |d| d.set_attribute(n, "class", joined))?;
    Ok(result)
}

// ----- form controls -----------------------------------------------------

fn value(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let v = with_dom(ctx, |d| match d.control_kind(n) {
        Some(rustlite_dom::ControlKind::Text { .. }) => d.control_value(n),
        Some(rustlite_dom::ControlKind::Checkbox | rustlite_dom::ControlKind::Radio) => {
            d.attribute(n, "value").unwrap_or("on").to_owned()
        }
        _ => d.attribute(n, "value").unwrap_or("").to_owned(),
    })?;
    Ok(string(&v))
}

fn set_value(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let v = arg_string(args, 0, ctx)?;
    with_dom_mut(ctx, |d| match d.control_kind(n) {
        Some(rustlite_dom::ControlKind::Text { .. }) => d.set_control_value(n, &v),
        _ => d.set_attribute(n, "value", v),
    })?;
    Ok(JsValue::undefined())
}

fn checked(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    Ok(JsValue::from(with_dom(ctx, |d| d.checked(n))?))
}

fn set_checked(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let on = arg(args, 0).to_boolean();
    with_dom_mut(ctx, |d| {
        if on && d.control_kind(n) == Some(rustlite_dom::ControlKind::Radio) {
            uncheck_radio_group(d, n);
        }
        d.set_checked(n, on);
    })?;
    Ok(JsValue::undefined())
}

/// Unchecks the other radios with `n`'s name in the same form (or, outside
/// forms, the same document), HTML §4.10.5.1.16.
pub fn uncheck_radio_group(d: &mut Document, n: NodeId) {
    let Some(name) = d
        .attribute(n, "name")
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    let form_of = |d: &Document, x: NodeId| d.ancestors(x).find(|&a| d.is_element_named(a, "form"));
    let form = form_of(d, n);
    let group: Vec<NodeId> = d
        .descendants(d.root())
        .filter(|&x| {
            x != n
                && d.control_kind(x) == Some(rustlite_dom::ControlKind::Radio)
                && d.attribute(x, "name") == Some(name.as_str())
                && form_of(d, x) == form
        })
        .collect();
    for x in group {
        d.set_checked(x, false);
    }
}

fn control_type(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let t = with_dom(ctx, |d| match d.tag_name(n) {
        Some("input") => {
            let t = d
                .attribute(n, "type")
                .unwrap_or("text")
                .trim()
                .to_ascii_lowercase();
            if t.is_empty() { "text".to_owned() } else { t }
        }
        Some("button") => d
            .attribute(n, "type")
            .map(|t| t.trim().to_ascii_lowercase())
            .filter(|t| matches!(t.as_str(), "button" | "reset"))
            .unwrap_or_else(|| "submit".to_owned()),
        _ => String::new(),
    })?;
    Ok(string(&t))
}

fn name(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    attr_or_empty(this, ctx, "name")
}

fn focus(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    with_dom_mut(ctx, |d| {
        if d.control_kind(n).is_some() {
            d.set_focused(Some(n));
        }
    })?;
    Ok(JsValue::undefined())
}

fn blur(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    with_dom_mut(ctx, |d| {
        if d.focused() == Some(n) {
            d.set_focused(None);
        }
    })?;
    Ok(JsValue::undefined())
}

fn active_element(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = with_dom(ctx, |d| {
        d.focused().or_else(|| d.find_element(d.root(), "body"))
    })?;
    wrap_opt(n, ctx)
}

// ----- element.style -----------------------------------------------------

fn style(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let (existing, proto) = {
        let s = host(ctx)?;
        (s.style_objects.get(&n).cloned(), s.style_proto.clone())
    };
    if let Some(obj) = existing {
        return Ok(obj.into());
    }
    let obj = JsObject::with_object_proto(ctx.intrinsics());
    obj.set_prototype(Some(proto));
    let s = host(ctx)?;
    s.style_objects.insert(n, obj.clone());
    s.style_nodes.insert(obj.clone(), n);
    Ok(obj.into())
}

/// `el.style = "color: red"` sets the whole attribute, like `cssText`.
fn set_style(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = this_element(this, ctx)?;
    let text = arg_string(args, 0, ctx)?;
    with_dom_mut(ctx, |d| d.set_attribute(n, "style", text))?;
    Ok(JsValue::undefined())
}

fn style_node(this: &JsValue, ctx: &mut Context) -> JsResult<NodeId> {
    let obj = this
        .as_object()
        .ok_or_else(|| type_error("not a CSSStyleDeclaration"))?;
    host(ctx)?
        .style_nodes
        .get(&obj)
        .copied()
        .ok_or_else(|| type_error("not a CSSStyleDeclaration"))
}

fn style_text(n: NodeId, ctx: &mut Context) -> JsResult<String> {
    with_dom(ctx, |d| d.attribute(n, "style").unwrap_or("").to_owned())
}

fn style_get(this: &JsValue, name: &str, ctx: &mut Context) -> JsResult<JsValue> {
    let n = style_node(this, ctx)?;
    let text = style_text(n, ctx)?;
    Ok(string(&crate::style_attr::get(&text, name)))
}

fn write_style(
    n: NodeId,
    name: &str,
    value: Option<&str>,
    important: bool,
    ctx: &mut Context,
) -> JsResult<()> {
    if value.is_some_and(|v| !crate::style_attr::acceptable_value(v)) {
        return Ok(());
    }
    let text = style_text(n, ctx)?;
    let updated = crate::style_attr::set(&text, name, value, important);
    with_dom_mut(ctx, |d| {
        if updated.is_empty() {
            d.remove_attribute(n, "style");
        } else {
            d.set_attribute(n, "style", updated);
        }
    })
}

fn style_set(this: &JsValue, name: &str, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = style_node(this, ctx)?;
    let value = match arg(args, 0) {
        v if v.is_null_or_undefined() => None,
        v => Some(v.to_string(ctx)?.to_std_string_escaped()),
    };
    write_style(n, name, value.as_deref(), false, ctx)?;
    Ok(JsValue::undefined())
}

fn property_name(args: &[JsValue], ctx: &mut Context) -> JsResult<String> {
    let name = arg_string(args, 0, ctx)?;
    Ok(if name.starts_with("--") {
        name
    } else {
        name.to_ascii_lowercase()
    })
}

fn get_property_value(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let name = property_name(args, ctx)?;
    style_get(this, &name, ctx)
}

fn set_property(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = style_node(this, ctx)?;
    let name = property_name(args, ctx)?;
    let value = arg_string(args, 1, ctx)?;
    let important = arg_string(args, 2, ctx)?.eq_ignore_ascii_case("important");
    write_style(n, &name, Some(&value), important, ctx)?;
    Ok(JsValue::undefined())
}

fn remove_property(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = style_node(this, ctx)?;
    let name = property_name(args, ctx)?;
    let text = style_text(n, ctx)?;
    let old = crate::style_attr::get(&text, &name);
    write_style(n, &name, None, false, ctx)?;
    Ok(string(&old))
}

fn css_text(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = style_node(this, ctx)?;
    let text = style_text(n, ctx)?;
    Ok(string(&crate::style_attr::serialize(
        &crate::style_attr::parse(&text),
    )))
}

fn set_css_text(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = style_node(this, ctx)?;
    let text = arg_string(args, 0, ctx)?;
    with_dom_mut(ctx, |d| d.set_attribute(n, "style", text))?;
    Ok(JsValue::undefined())
}

// ----- Document ----------------------------------------------------------

fn title(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let text = with_dom(ctx, |d| {
        d.find_element(d.root(), "title")
            .map(|t| {
                d.text_content(t)
                    .split_ascii_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default()
    })?;
    Ok(string(&text))
}

fn set_title(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let text = arg_string(args, 0, ctx)?;
    with_dom_mut(ctx, |d| {
        let title = match d.find_element(d.root(), "title") {
            Some(t) => t,
            None => {
                let t = d.create_element("title", Vec::new());
                let parent = d
                    .find_element(d.root(), "head")
                    .or_else(|| d.find_element(d.root(), "html"))
                    .unwrap_or(d.root());
                let _ = d.append_child(parent, t);
                t
            }
        };
        d.set_text_content(title, &text);
    })?;
    Ok(JsValue::undefined())
}

fn body(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let b = with_dom(ctx, |d| d.find_element(d.root(), "body"))?;
    wrap_opt(b, ctx)
}

fn head(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let h = with_dom(ctx, |d| d.find_element(d.root(), "head"))?;
    wrap_opt(h, ctx)
}

fn document_element(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let e = with_dom(ctx, |d| {
        d.children(d.root()).find(|&c| d.element(c).is_some())
    })?;
    wrap_opt(e, ctx)
}

fn get_element_by_id(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = arg_string(args, 0, ctx)?;
    let found = with_dom(ctx, |d| {
        d.descendants(d.root())
            .find(|&n| d.attribute(n, "id") == Some(id.as_str()))
    })?;
    wrap_opt(found, ctx)
}

fn create_element(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let name = arg_string(args, 0, ctx)?.to_ascii_lowercase();
    if !is_valid_name(&name) {
        return Err(type_error("InvalidCharacterError: invalid element name"));
    }
    let n = with_dom_mut(ctx, |d| d.create_element(name, Vec::<Attribute>::new()))?;
    wrap(n, ctx)
}

fn create_text_node(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let text = arg_string(args, 0, ctx)?;
    let n = with_dom_mut(ctx, |d| d.create_text(text))?;
    wrap(n, ctx)
}

// ----- events ------------------------------------------------------------

fn prevent_default(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    host(ctx)?.dispatch.default_prevented = true;
    Ok(JsValue::undefined())
}

fn stop_propagation(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    host(ctx)?.dispatch.stop_propagation = true;
    Ok(JsValue::undefined())
}

fn stop_immediate_propagation(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let d = &mut host(ctx)?.dispatch;
    d.stop_propagation = true;
    d.stop_immediate = true;
    Ok(JsValue::undefined())
}

fn default_prevented(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(host(ctx)?.dispatch.default_prevented))
}

/// Compiles (or reuses) the `on{kind}` attribute handler of `n`, if any.
fn attribute_handler(n: NodeId, kind: &str, ctx: &mut Context) -> Option<JsObject> {
    let source = host(ctx)
        .ok()?
        .dom
        .attribute(n, &format!("on{kind}"))?
        .to_owned();
    let key = (n, kind.to_owned());
    if let Some((cached_source, f)) = host(ctx).ok()?.attribute_handlers.get(&key)
        && *cached_source == source
    {
        return Some(f.clone());
    }
    let wrapped = format!("(function (event) {{\n{source}\n}})");
    match ctx.eval(Source::from_bytes(wrapped.as_bytes())) {
        Ok(v) => {
            let f = v.as_callable()?;
            host(ctx)
                .ok()?
                .attribute_handlers
                .insert(key, (source, f.clone()));
            Some(f)
        }
        Err(e) => {
            tracing::warn!(target: "console", "on{kind} handler: {e}");
            None
        }
    }
}

/// Dispatches `kind` at `target` (bubbling through ancestors to the window)
/// or at the window alone. Returns (default prevented, uncaught errors).
pub(crate) fn dispatch(
    ctx: &mut Context,
    target: Option<NodeId>,
    kind: &str,
) -> (bool, Vec<String>) {
    dispatch_init(ctx, target, kind, &crate::EventInit::default())
}

/// Dispatches with explicit options: non-bubbling events (`focus`, `blur`,
/// `change` is bubbling) stop at the target; keyboard events carry `key`.
pub(crate) fn dispatch_init(
    ctx: &mut Context,
    target: Option<NodeId>,
    kind: &str,
    init: &crate::EventInit,
) -> (bool, Vec<String>) {
    let mut errors = Vec::new();
    let Ok(state) = host(ctx) else {
        return (false, errors);
    };
    state.dispatch = DispatchFlags::default();
    state.dispatching = true;
    let mut path: Vec<Target> = match target {
        Some(t) => std::iter::once(t)
            .chain(state.dom.ancestors(t))
            .map(Target::Node)
            .collect(),
        None => Vec::new(),
    };
    if init.no_bubble {
        path.truncate(1);
    }
    if !init.no_bubble || target.is_none() {
        path.push(Target::Window);
    }

    let event = JsObject::with_object_proto(ctx.intrinsics());
    let target_value = match target {
        Some(t) => wrap(t, ctx).unwrap_or_default(),
        None => ctx.global_object().into(),
    };
    let setup: JsResult<()> = (|| {
        event.set(js_string!("type"), string(kind), false, ctx)?;
        event.set(js_string!("target"), target_value.clone(), false, ctx)?;
        event.set(js_string!("bubbles"), !init.no_bubble, false, ctx)?;
        if let Some(key) = &init.key {
            event.set(js_string!("key"), string(key), false, ctx)?;
        }
        event.set(js_string!("cancelable"), true, false, ctx)?;
        for (name, f) in [
            ("preventDefault", prevent_default as NativeFunctionPointer),
            ("stopPropagation", stop_propagation),
            ("stopImmediatePropagation", stop_immediate_propagation),
        ] {
            let func = function(ctx, name, 0, f);
            event.set(JsString::from(name), func, false, ctx)?;
        }
        let getter = function(ctx, "get defaultPrevented", 0, default_prevented);
        event.define_property_or_throw(
            js_string!("defaultPrevented"),
            PropertyDescriptor::builder()
                .get(getter)
                .set(JsValue::undefined())
                .enumerable(true)
                .configurable(true),
            ctx,
        )?;
        Ok(())
    })();
    if let Err(e) = setup {
        errors.push(e.to_string());
    }

    for current in path {
        let current_value = match current {
            Target::Node(n) => wrap(n, ctx).unwrap_or_default(),
            Target::Window => ctx.global_object().into(),
        };
        let _ = event.set(
            js_string!("currentTarget"),
            current_value.clone(),
            false,
            ctx,
        );
        // (callback, is an `on*` event handler)
        let mut handlers: Vec<(JsObject, bool)> = Vec::new();
        if let Ok(state) = host(ctx) {
            handlers.extend(
                state
                    .listeners
                    .get(&(current, kind.to_owned()))
                    .into_iter()
                    .flatten()
                    .map(|f| (f.clone(), false)),
            );
        }
        if let Target::Node(n) = current {
            // A handler property replaces the attribute's handler (HTML §8.1.8).
            let property = host(ctx)
                .ok()
                .and_then(|s| s.property_handlers.get(&(n, kind.to_owned())).cloned());
            match property {
                Some(f) => handlers.push((f, true)),
                None => handlers.extend(attribute_handler(n, kind, ctx).map(|f| (f, true))),
            }
        }
        for (f, is_handler) in handlers {
            match f.call(&current_value, &[event.clone().into()], ctx) {
                Ok(v) => {
                    // `return false` from an event handler cancels the event.
                    if is_handler
                        && v == JsValue::from(false)
                        && let Ok(s) = host(ctx)
                    {
                        s.dispatch.default_prevented = true;
                    }
                }
                Err(e) => errors.push(format!("{kind} listener: {e}")),
            }
            if let Err(e) = ctx.run_jobs() {
                errors.push(format!("{kind} listener (promise job): {e}"));
            }
            if host(ctx).is_ok_and(|s| s.dispatch.stop_immediate) {
                break;
            }
        }
        if host(ctx).is_ok_and(|s| s.dispatch.stop_propagation) {
            break;
        }
    }
    let prevented = host(ctx).is_ok_and(|s| s.dispatch.default_prevented);
    if let Ok(s) = host(ctx) {
        s.dispatching = false;
    }
    (prevented, errors)
}
