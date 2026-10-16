use std::collections::HashMap;

use rustlite_dom::{Document, NodeId};
use rustlite_fonts::FontContext;

use super::*;

struct Laid {
    doc: Document,
    tree: LayoutTree,
}

impl Laid {
    fn rect(&self, id: &str) -> RectF {
        let rects: HashMap<NodeId, RectF> = element_rects(&self.tree);
        let node = self
            .doc
            .descendants(self.doc.root())
            .find(|&n| self.doc.attribute(n, "id") == Some(id))
            .unwrap_or_else(|| panic!("no element #{id}"));
        rects.get(&node).copied().unwrap_or_default()
    }
}

fn lay_out_with(html: &str, fonts: &mut FontContext) -> Laid {
    let parsed = rustlite_html::parse_document(html);
    let doc = parsed.document;
    let stylist = rustlite_style::stylist_for(&doc);
    let (styles, _) = rustlite_style::compute_styles(&doc, &stylist, (800.0, 600.0));
    let tree = layout_document(&doc, &styles, fonts, (800.0, 600.0));
    Laid { doc, tree }
}

/// Layout without fonts: fine for boxes that contain no text.
fn lay_out(html: &str) -> Laid {
    lay_out_with(html, &mut FontContext::new())
}

fn r(x: f32, y: f32, w: f32, h: f32) -> RectF {
    RectF::new(x, y, w, h)
}

const RESET: &str = "<!DOCTYPE html><style>body { margin: 0 }</style>";

#[test]
fn block_widths_heights_and_box_model() {
    let l = lay_out(&format!(
        "{RESET}<div id=a style='height: 50px; padding: 10px; border: 5px solid; margin: 20px'></div>\
         <div id=b style='width: 200px; height: 10px; margin: 0 auto'></div>\
         <div id=c style='width: 50%; height: 10px; margin-left: 10%'></div>"
    ));
    // a: margin-top 20 (root/body margins are 0 and html is a BFC, so a's top
    // margin escapes body but stays inside html).
    assert_eq!(l.rect("a"), r(20.0, 20.0, 760.0, 80.0));
    // b: centered; a's bottom margin 20 separates it.
    assert_eq!(l.rect("b"), r(300.0, 120.0, 200.0, 10.0));
    assert_eq!(l.rect("c"), r(80.0, 130.0, 400.0, 10.0));
}

#[test]
fn sibling_and_parent_child_margins_collapse() {
    let l = lay_out(&format!(
        "{RESET}<div id=p style='margin-top: 10px'>\
           <div id=c1 style='margin-top: 30px; margin-bottom: 20px; height: 10px'></div>\
           <div id=c2 style='margin-top: 5px; height: 10px'></div>\
         </div>\
         <div id=after style='height: 1px'></div>"
    ));
    // p's top margin (10) collapses with c1's (30): both at y = 30.
    assert_eq!(l.rect("p").y, 30.0);
    assert_eq!(l.rect("c1").y, 30.0);
    // Siblings: max(20, 5) = 20.
    assert_eq!(l.rect("c2").y, 60.0);
    assert_eq!(l.rect("p").height, 40.0);
    assert_eq!(l.rect("after").y, 70.0);
}

#[test]
fn borders_padding_and_bfc_prevent_collapsing() {
    let l = lay_out(&format!(
        "{RESET}<div id=p1 style='border-top: 1px solid'><div id=c1 style='margin-top: 10px; height: 5px'></div></div>\
         <div id=p2 style='overflow: hidden'><div id=c2 style='margin-top: 10px; height: 5px'></div></div>"
    ));
    assert_eq!(l.rect("p1").y, 0.0);
    assert_eq!(l.rect("c1").y, 11.0);
    assert_eq!(l.rect("p1").height, 16.0);
    assert_eq!(l.rect("p2").y, 16.0);
    assert_eq!(l.rect("c2").y, 26.0);
}

#[test]
fn empty_blocks_collapse_through_and_negative_margins() {
    let l = lay_out(&format!(
        "{RESET}<div id=a style='height: 10px; margin-bottom: 10px'></div>\
         <div id=empty style='margin-top: 30px; margin-bottom: 5px'></div>\
         <div id=b style='height: 10px; margin-top: -20px'></div>"
    ));
    // Adjoining: 10, 30, 5, -20 → max positive 30 + most negative -20 = 10.
    assert_eq!(l.rect("b").y, 20.0);
}

#[test]
fn display_none_and_body_margin() {
    let l = lay_out(
        "<!DOCTYPE html><div id=x style='height: 10px'></div><div style='display:none'><div id=hidden style='height:99px'></div></div>",
    );
    assert_eq!(l.rect("x"), r(8.0, 8.0, 784.0, 10.0));
    assert_eq!(l.rect("hidden"), RectF::default());
    // html: 8 + 10 + 8.
    assert_eq!(l.tree.document_height, 26.0);
}

#[test]
fn deep_nesting_is_bounded() {
    let html = format!("{RESET}{}x", "<div>".repeat(5_000));
    let l = lay_out(&html);
    assert!(l.tree.root.is_some());
}

fn fonts() -> Option<FontContext> {
    let f = FontContext::with_system_fonts();
    if f.face_count() == 0 {
        eprintln!("note: no system fonts; skipping text layout test");
        return None;
    }
    Some(f)
}

fn line_count(l: &Laid, id: &str) -> usize {
    let node = l
        .doc
        .descendants(l.doc.root())
        .find(|&n| l.doc.attribute(n, "id") == Some(id))
        .unwrap();
    let mut stack: Vec<&BlockBox> = l.tree.root.iter().collect();
    while let Some(b) = stack.pop() {
        if b.node == Some(node) {
            return match &b.children {
                BlockChildren::Inline(i) => i.lines.len(),
                _ => 0,
            };
        }
        if let BlockChildren::Blocks(k) = &b.children {
            stack.extend(k.iter());
        }
    }
    0
}

#[test]
fn text_wraps_into_lines() {
    let Some(mut f) = fonts() else { return };
    let words = "lorem ipsum dolor sit amet ".repeat(20);
    let l = lay_out_with(
        &format!("{RESET}<p id=p style='width: 200px; margin: 0'>{words}</p>"),
        &mut f,
    );
    let lines = line_count(&l, "p");
    assert!(lines > 5, "expected wrapping, got {lines} lines");
    let p = l.rect("p");
    assert!(p.height > 5.0 * 10.0);
    assert_eq!(p.width, 200.0);
}

#[test]
fn line_height_and_br() {
    let Some(mut f) = fonts() else { return };
    let l = lay_out_with(
        &format!("{RESET}<p id=p style='line-height: 30px; margin: 0'>a<br>b<br><br>c</p>"),
        &mut f,
    );
    assert_eq!(line_count(&l, "p"), 4);
    assert_eq!(l.rect("p").height, 120.0);
}

#[test]
fn inline_elements_get_fragment_union_rects() {
    let Some(mut f) = fonts() else { return };
    let l = lay_out_with(
        &format!("{RESET}<p style='margin:0'>aaa <b id=b>bold</b> ccc</p>"),
        &mut f,
    );
    let b = l.rect("b");
    assert!(b.x > 0.0 && b.width > 0.0 && b.height > 0.0, "{b:?}");
}

#[test]
fn text_align_center_and_right() {
    let Some(mut f) = fonts() else { return };
    let l = lay_out_with(
        &format!(
            "{RESET}<p style='text-align:center;margin:0'><span id=c>x</span></p><p style='text-align:right;margin:0'><span id=r>x</span></p>"
        ),
        &mut f,
    );
    let c = l.rect("c");
    let rr = l.rect("r");
    assert!((c.x + c.width / 2.0 - 400.0).abs() < 1.0, "{c:?}");
    assert!((rr.right() - 800.0).abs() < 0.01, "{rr:?}");
}

#[test]
fn painting_produces_backgrounds_borders_and_text() {
    let Some(mut f) = fonts() else { return };
    let l = lay_out_with(
        &format!(
            "{RESET}<body style='background: rgb(1,2,3)'><div style='background: red; border: 2px solid blue; height: 20px'>Hi</div>"
        ),
        &mut f,
    );
    let mut list = rustlite_paint::DisplayList::new();
    paint(&l.tree, 0.0, 1.0, &mut list);
    let items = list.items();
    // Canvas background propagated from body.
    assert!(
        matches!(items[0], rustlite_paint::DisplayItem::SolidRect { color, .. } if color == rustlite_paint::Color::rgb(1, 2, 3))
    );
    assert!(items.iter().any(|i| matches!(i, rustlite_paint::DisplayItem::SolidRect { color, .. } if *color == rustlite_paint::Color::rgb(255, 0, 0))));
    assert!(items.iter().any(|i| matches!(i, rustlite_paint::DisplayItem::SolidRect { color, .. } if *color == rustlite_paint::Color::rgb(0, 0, 255))));
    assert!(
        items
            .iter()
            .any(|i| matches!(i, rustlite_paint::DisplayItem::Text { .. }))
    );
}

#[test]
fn marker_text_for_counter_styles() {
    use crate::build::marker_text;
    use rustlite_style::properties::ListStyleType as T;
    assert_eq!(marker_text(T::Disc, 1).as_deref(), Some("\u{2022} "));
    assert_eq!(marker_text(T::Decimal, 12).as_deref(), Some("12. "));
    assert_eq!(marker_text(T::LowerAlpha, 28).as_deref(), Some("ab. "));
    assert_eq!(
        marker_text(T::UpperRoman, 1994).as_deref(),
        Some("MCMXCIV. ")
    );
    assert_eq!(
        marker_text(T::LowerRoman, 0).as_deref(),
        Some("0. "),
        "out of range falls back to decimal"
    );
    assert_eq!(marker_text(T::None, 1), None);
}

fn markers(l: &Laid) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<&BlockBox> = l.tree.root.iter().collect();
    while let Some(b) = stack.pop() {
        if let Some(m) = &b.marker {
            out.push(m.text.clone());
        }
        if let BlockChildren::Blocks(k) = &b.children {
            stack.extend(k.iter().rev());
        }
    }
    out
}

#[test]
fn list_ordinals_respect_start_and_value() {
    let l = lay_out(&format!(
        "{RESET}<ol start=5><li>a<li>b<li value=10>c<li>d</ol><ul><li>x<ul><li>y</ul></ul><ol style='list-style-type: upper-alpha'><li>q</ol>"
    ));
    assert_eq!(
        markers(&l),
        [
            "5. ",
            "6. ",
            "10. ",
            "11. ",
            "\u{2022} ",
            "\u{25E6} ",
            "A. "
        ]
    );
}

#[test]
fn marker_sits_left_of_content_on_first_baseline() {
    let Some(mut f) = fonts() else { return };
    let l = lay_out_with(
        &format!("{RESET}<ul style='margin:0'><li id=li>item</li></ul>"),
        &mut f,
    );
    let li = l.rect("li");
    let mut stack: Vec<&BlockBox> = l.tree.root.iter().collect();
    while let Some(b) = stack.pop() {
        if let (Some(m), BlockChildren::Inline(inline)) = (&b.marker, &b.children) {
            let width: f32 = m.runs.iter().map(|(_, r)| r.glyphs.len() as f32).sum();
            assert!(width > 0.0, "marker has glyphs");
            assert!(m.x < li.x, "marker is outside the content box");
            assert_eq!(m.baseline, inline.lines[0].fragments[0].baseline);
            return;
        }
        if let BlockChildren::Blocks(k) = &b.children {
            stack.extend(k.iter());
        }
    }
    panic!("no marker found");
}

#[test]
fn hit_testing_finds_innermost_element() {
    let Some(mut f) = fonts() else { return };
    let l = lay_out_with(
        &format!(
            "{RESET}<div id=outer style='padding: 20px'><p id=p style='margin:0'>plain <a id=link href=x><b id=bold>bold link</b></a></p></div><div id=below style='height: 50px'></div>"
        ),
        &mut f,
    );
    let id_of = |n: Option<rustlite_dom::NodeId>| {
        n.and_then(|n| l.doc.attribute(n, "id")).map(str::to_owned)
    };
    let bold = l.rect("bold");
    assert_eq!(
        id_of(hit_test(&l.tree, bold.x + 2.0, bold.y + 2.0)).as_deref(),
        Some("bold")
    );
    // Text directly in the paragraph hits the paragraph.
    let p = l.rect("p");
    assert_eq!(
        id_of(hit_test(&l.tree, p.x + 2.0, p.y + 2.0)).as_deref(),
        Some("p")
    );
    // Padding area hits the outer block.
    assert_eq!(id_of(hit_test(&l.tree, 5.0, 5.0)).as_deref(), Some("outer"));
    let below = l.rect("below");
    assert_eq!(
        id_of(hit_test(&l.tree, 10.0, below.y + 10.0)).as_deref(),
        Some("below")
    );
}

#[test]
fn min_and_max_sizes_clamp_blocks() {
    let l = lay_out(&format!(
        "{RESET}<div id=a style='max-width: 300px; margin: 0 auto; height: 10px'></div>\
         <div id=b style='width: 50px; min-width: 120px; height: 10px'></div>\
         <div id=c style='height: 500px; max-height: 40px'></div>\
         <div id=d style='min-height: 25px'></div>"
    ));
    assert_eq!(
        l.rect("a"),
        r(250.0, 0.0, 300.0, 10.0),
        "max-width with auto margins centers"
    );
    assert_eq!(l.rect("b").width, 120.0);
    assert_eq!(l.rect("c").height, 40.0);
    assert_eq!(l.rect("d").height, 25.0);
}
