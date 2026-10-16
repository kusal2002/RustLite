use rustlite_dom::Document;

use super::*;

fn page(html: &str) -> (ScriptHost, Document) {
    let doc = rustlite_html::parse_document(html).document;
    (ScriptHost::new("https://a.test/"), doc)
}

fn eval(host: &mut ScriptHost, doc: &mut Document, expr: &str) -> String {
    eval_to_string(host, doc, expr).unwrap_or_else(|e| panic!("{expr}: {e}"))
}

fn by_id(doc: &Document, id: &str) -> NodeId {
    doc.descendants(doc.root())
        .find(|&n| doc.attribute(n, "id") == Some(id))
        .unwrap()
}

#[test]
fn scripts_read_and_change_the_document() {
    let (mut h, mut d) = page("<title>Old</title><p id=a class='x y'>hi <b>there</b></p>");
    assert_eq!(eval(&mut h, &mut d, "document.title"), "Old");
    assert_eq!(
        eval(&mut h, &mut d, "document.getElementById('a').textContent"),
        "hi there"
    );
    let out = h.run_script(
        &mut d,
        "document.title = 'New';\
         const p = document.querySelector('p.x');\
         p.textContent = 'changed';\
         p.setAttribute('data-k', '1');\
         const li = document.createElement('li');\
         li.appendChild(document.createTextNode('item'));\
         document.body.appendChild(li);",
        "inline",
    );
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    assert!(out.mutated);
    assert_eq!(eval(&mut h, &mut d, "document.title"), "New");
    let a = by_id(&d, "a");
    assert_eq!(d.text_content(a), "changed");
    assert_eq!(d.attribute(a, "data-k"), Some("1"));
    assert_eq!(
        eval(&mut h, &mut d, "document.body.lastChild.tagName"),
        "LI"
    );
}

#[test]
fn wrappers_are_unique_and_cannot_be_forged() {
    let (mut h, mut d) = page("<p id=a>x</p>");
    assert_eq!(
        eval(
            &mut h,
            &mut d,
            "document.getElementById('a') === document.querySelector('#a')"
        ),
        "true"
    );
    let out = h.run_script(
        &mut d,
        "const fake = Object.create(Object.getPrototypeOf(document.body)); fake.textContent;",
        "forge",
    );
    assert!(
        out.errors.iter().any(|e| e.contains("not a Node")),
        "{:?}",
        out.errors
    );
}

#[test]
fn query_helpers_and_inner_html() {
    let (mut h, mut d) = page("<ul><li class=a>1</li><li>2</li><li class=a>3</li></ul>");
    assert_eq!(
        eval(&mut h, &mut d, "document.querySelectorAll('li.a').length"),
        "2"
    );
    assert_eq!(
        eval(
            &mut h,
            &mut d,
            "document.querySelector('li:last-child').closest('ul').children.length"
        ),
        "3"
    );
    let out = h.run_script(&mut d, "document.querySelector('[')", "bad");
    assert!(
        out.errors.iter().any(|e| e.contains("SyntaxError")),
        "{:?}",
        out.errors
    );
    h.run_script(
        &mut d,
        "document.querySelector('ul').innerHTML = '<li id=n>a &amp; b</li><script>window.ran = 1</script>';",
        "inner",
    );
    assert_eq!(
        eval(&mut h, &mut d, "document.querySelector('ul').innerHTML"),
        "<li id=\"n\">a &amp; b</li><script>window.ran = 1</script>"
    );
    assert_eq!(
        eval(&mut h, &mut d, "typeof window.ran"),
        "undefined",
        "scripts inserted through innerHTML do not run"
    );
}

#[test]
fn class_list_and_attributes() {
    let (mut h, mut d) = page("<p id=a class='one'>x</p>");
    h.run_script(
        &mut d,
        "const c = document.getElementById('a').classList;\
         c.add('two'); c.remove('one'); window.t = c.toggle('three'); window.has = c.contains('two');",
        "classes",
    );
    assert_eq!(d.attribute(by_id(&d, "a"), "class"), Some("two three"));
    assert_eq!(eval(&mut h, &mut d, "String(t) + String(has)"), "truetrue");
}

#[test]
fn click_listeners_bubble_and_can_cancel() {
    let (mut h, mut d) = page(
        "<div id=outer><a id=link href=/x onclick=\"window.attr = (window.attr || 0) + 1\">go</a></div>",
    );
    h.run_script(
        &mut d,
        "window.order = [];\
         document.getElementById('link').addEventListener('click', e => order.push('link:' + e.target.id));\
         document.getElementById('outer').addEventListener('click', e => { order.push('outer'); e.preventDefault(); });\
         window.addEventListener('click', () => order.push('window'));",
        "listeners",
    );
    let link = by_id(&d, "link");
    assert!(h.has_listeners(&d, link, "click"));
    let out = h.dispatch_event(&mut d, link, "click");
    assert!(out.default_prevented);
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    assert_eq!(
        eval(&mut h, &mut d, "order.join(',') + ' ' + attr"),
        "link:link,outer,window 1"
    );
}

#[test]
fn stop_propagation_and_handler_return_false() {
    let (mut h, mut d) = page("<div id=outer><button id=b onclick='return false'>b</button></div>");
    h.run_script(
        &mut d,
        "window.outer = 0;\
         document.getElementById('outer').addEventListener('click', () => outer++);\
         document.getElementById('b').addEventListener('click', e => e.stopPropagation());",
        "listeners",
    );
    let b = by_id(&d, "b");
    let out = h.dispatch_event(&mut d, b, "click");
    assert!(out.default_prevented, "return false from onclick cancels");
    assert_eq!(eval(&mut h, &mut d, "outer"), "0");
}

#[test]
fn runaway_scripts_are_stopped() {
    let (mut h, mut d) = page("<p>x</p>");
    let out = h.run_script(&mut d, "while (true) {}", "loop");
    assert_eq!(out.errors.len(), 1, "{:?}", out.errors);
    let out = h.run_script(&mut d, "function f() { return f(); } f();", "recursion");
    assert_eq!(out.errors.len(), 1, "{:?}", out.errors);
    // The realm keeps working after a stopped script.
    assert_eq!(eval(&mut h, &mut d, "1 + 1"), "2");
}

#[test]
fn console_output_is_captured() {
    let (mut h, mut d) = page("<p>x</p>");
    h.run_script(&mut d, "console.log('a', 1, {}); console.error('b')", "log");
    assert_eq!(h.console(), ["a 1 [object Object]", "b"]);
}

#[test]
fn script_type_filter() {
    assert!(is_classic_script_type(None));
    assert!(is_classic_script_type(Some("")));
    assert!(is_classic_script_type(Some("text/javascript")));
    assert!(is_classic_script_type(Some(
        "Application/JavaScript; charset=utf-8"
    )));
    assert!(!is_classic_script_type(Some("module")));
    assert!(!is_classic_script_type(Some("application/json")));
}
