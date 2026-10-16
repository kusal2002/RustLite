//! Form controls and keyboard input through the `Page` API: clicks focus
//! and toggle, typing edits values, Enter submits GET forms, scripts see
//! (and can cancel) the events.

use rustlite_browser_core::{Page, PageKey};
use rustlite_dom::NodeId;
use rustlite_fonts::FontContext;
use rustlite_paint::Size;

fn page(html: &str) -> Page {
    // Fontless: control geometry is still exact for these tests' purposes
    // (no text shaping needed to find and click controls).
    let mut page = Page::with_fonts(Size::new(600, 400), FontContext::new());
    page.load_html("http://a.test/dir/page.html", html);
    page
}

fn node(page: &mut Page, id: &str) -> NodeId {
    let (dom, _) = page.layout().unwrap();
    dom.descendants(dom.root())
        .find(|&n| dom.attribute(n, "id") == Some(id))
        .unwrap()
}

fn center(page: &mut Page, id: &str) -> (f32, f32) {
    let n = node(page, id);
    let (_, tree) = page.layout().unwrap();
    let r = rustlite_layout::element_rects(tree)[&n];
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn click(page: &mut Page, id: &str) -> rustlite_browser_core::InputOutcome {
    let (x, y) = center(page, id);
    page.click(x, y)
}

fn value(page: &mut Page, id: &str) -> String {
    let n = node(page, id);
    page.layout().unwrap().0.control_value(n)
}

fn checked(page: &mut Page, id: &str) -> bool {
    let n = node(page, id);
    page.layout().unwrap().0.checked(n)
}

fn typ(page: &mut Page, text: &str) {
    page.key_down(&PageKey::Text(text.into()));
}

const FORM: &str = "<!DOCTYPE html><body style='margin:0'>\
    <form action=search><input id=q name=q value=old>\
    <input id=c type=checkbox name=c checked><input id=r1 type=radio name=r value=one checked>\
    <input id=r2 type=radio name=r value=two><input type=hidden name=h value=secret>\
    <input id=off name=off disabled value=x><button id=go name=b value=go>Go</button></form>\
    <p id=plain>text</p>\
    <script>window.log = [];\
      for (const t of ['input', 'change', 'focus', 'blur', 'submit'])\
        document.addEventListener(t, e => log.push(t + ':' + e.target.id), false);\
      document.querySelector('form').addEventListener('submit', e => log.push('submit-form'));\
    </script>";

#[test]
fn typing_edits_the_focused_field_and_enter_submits() {
    let mut p = page(FORM);
    assert!(click(&mut p, "q").consumed, "clicking a field focuses it");
    assert_eq!(p.focused(), Some(node(&mut p, "q")));
    typ(&mut p, "x");
    p.key_down(&PageKey::Backspace);
    p.key_down(&PageKey::Backspace);
    typ(&mut p, "dy!");
    p.key_down(&PageKey::Home);
    typ(&mut p, ">");
    assert_eq!(value(&mut p, "q"), ">oldy!");
    let out = p.key_down(&PageKey::Enter);
    assert!(out.consumed);
    assert_eq!(
        out.navigate.map(|u| u.to_string()).as_deref(),
        Some("http://a.test/dir/search?q=%3Eoldy%21&c=on&r=one&h=secret"),
        "GET submission: checked boxes, the checked radio, hidden fields; no disabled field, no button"
    );
}

#[test]
fn submit_button_names_itself_and_scripts_can_cancel_submission() {
    let mut p = page(FORM);
    let out = click(&mut p, "go");
    assert_eq!(
        out.navigate
            .map(|u| u.query().unwrap_or("").to_owned())
            .as_deref(),
        Some("q=old&c=on&r=one&b=go&h=secret")
    );
    let mut p = page(&format!(
        "{FORM}<script>document.querySelector('form').addEventListener('submit', e => e.preventDefault())</script>"
    ));
    assert_eq!(click(&mut p, "go").navigate, None);
}

#[test]
fn checkboxes_toggle_and_radios_form_a_group() {
    let mut p = page(FORM);
    click(&mut p, "c");
    assert!(!checked(&mut p, "c"));
    click(&mut p, "c");
    assert!(checked(&mut p, "c"));
    click(&mut p, "r2");
    assert!(checked(&mut p, "r2"));
    assert!(
        !checked(&mut p, "r1"),
        "checking one radio unchecks its group"
    );
    click(&mut p, "r2");
    assert!(
        checked(&mut p, "r2"),
        "clicking a checked radio keeps it checked"
    );
}

#[test]
fn tab_moves_focus_and_disabled_controls_are_skipped() {
    let mut p = page(FORM);
    let order: Vec<NodeId> = (0..7)
        .map(|_| {
            p.key_down(&PageKey::Tab { shift: false });
            p.focused().unwrap()
        })
        .collect();
    let ids = ["q", "c", "r1", "r2", "go", "q", "c"];
    let expected: Vec<NodeId> = ids.iter().map(|id| node(&mut p, id)).collect();
    assert_eq!(order, expected);
    p.key_down(&PageKey::Tab { shift: true });
    assert_eq!(p.focused(), Some(node(&mut p, "q")));
    assert!(!click(&mut p, "plain").consumed);
    assert_eq!(p.focused(), None, "clicking elsewhere blurs");
}

#[test]
fn scripts_see_events_and_can_cancel_keys() {
    let mut p = page(&format!(
        "{FORM}<script>document.getElementById('q').addEventListener('keydown', e => {{\
            if (e.key === 'z') e.preventDefault(); }});\
          document.addEventListener('keyup', e => console.log('up ' + e.key));</script>"
    ));
    click(&mut p, "q");
    p.key_down(&PageKey::End);
    typ(&mut p, "z");
    typ(&mut p, "a");
    p.key_up(&PageKey::Text("a".into()));
    assert_eq!(
        value(&mut p, "q"),
        "olda",
        "the cancelled key typed nothing"
    );
    click(&mut p, "plain");
    assert_eq!(p.console(), ["up a"]);
}

#[test]
fn script_value_and_checked_properties() {
    let mut p = page(&format!(
        "{FORM}<script>const q = document.getElementById('q');\
          window.before = q.value; q.value = 'set by script';\
          document.getElementById('c').checked = false;\
          document.getElementById('r2').checked = true;\
          q.focus(); window.active = document.activeElement.id;\
          console.log(before + '|' + q.value + '|' + q.type + '|' + active);</script>"
    ));
    assert_eq!(p.console(), ["old|set by script|text|q"]);
    assert_eq!(value(&mut p, "q"), "set by script");
    assert!(!checked(&mut p, "c"));
    assert!(!checked(&mut p, "r1") && checked(&mut p, "r2"));
    assert_eq!(p.focused(), Some(node(&mut p, "q")));
}

#[test]
fn input_and_change_events_fire_in_order() {
    let mut p = page(&format!(
        "{FORM}<script>for (const t of ['input', 'change', 'focus', 'blur'])\
           document.getElementById('q').addEventListener(t, () => console.log(t));</script>"
    ));
    click(&mut p, "q");
    typ(&mut p, "a");
    typ(&mut p, "b");
    click(&mut p, "plain");
    assert_eq!(p.console(), ["focus", "input", "input", "change", "blur"]);
}

fn demo_page() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/pages/interactive.html"
    ))
    .unwrap()
}

#[test]
fn demo_page_form_works() {
    let mut p = Page::with_fonts(Size::new(800, 822), FontContext::with_system_fonts());
    p.load_html("file:///D:/x/interactive.html", &demo_page());
    assert!(click(&mut p, "name").consumed);
    for c in ["A", "d", "a"] {
        p.key_down(&PageKey::Text(c.into()));
        // What the window does around each key: a repaint and the release.
        p.display_list();
        p.key_up(&PageKey::Text(c.into()));
    }
    // Timers and animation frames do not disturb form state.
    let later = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while p.wants_animation_frame() {
        p.run_animation_frame(later);
    }
    p.run_timers(later);
    assert_eq!(value(&mut p, "name"), "Ada");
    click(&mut p, "loud");
    assert!(checked(&mut p, "loud"));
    click(&mut p, "name");
    let out = p.key_down(&PageKey::Enter);
    assert_eq!(
        out.navigate, None,
        "the page's submit handler cancels navigation"
    );
    let g = node(&mut p, "greeting");
    assert_eq!(p.layout().unwrap().0.text_content(g), "HELLO, ADA!");
}

#[test]
fn demo_page_form_works_through_a_tab() {
    let path = std::fs::canonicalize(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/pages/interactive.html"
    ))
    .unwrap();
    let page = Page::with_fonts(Size::new(800, 822), FontContext::with_system_fonts());
    let mut t = rustlite_browser_core::Tab::new(page, None);
    t.navigate(&path.to_string_lossy()).unwrap();
    let (x, y) = center(t.page_mut().as_local_mut().unwrap(), "name");
    assert!(t.click(x, y).consumed);
    for c in ["O", "k"] {
        t.key_down(&PageKey::Text(c.into()));
        t.key_up(&PageKey::Text(c.into()));
    }
    assert_eq!(value(t.page_mut().as_local_mut().unwrap(), "name"), "Ok");
}
