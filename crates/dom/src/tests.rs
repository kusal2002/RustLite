use super::*;

fn el(doc: &mut Document, name: &str) -> NodeId {
    doc.create_element(name, Vec::new())
}

fn names(doc: &Document, ids: impl Iterator<Item = NodeId>) -> Vec<String> {
    ids.map(|n| match doc.data(n) {
        NodeData::Element(e) => e.name.to_string(),
        NodeData::Text(t) => format!("#{t}"),
        NodeData::Document => "#document".into(),
        other => format!("{other:?}"),
    })
    .collect()
}

#[test]
fn node_id_is_niche_optimized() {
    assert_eq!(std::mem::size_of::<Option<NodeId>>(), 4);
}

#[test]
fn append_and_traverse() {
    let mut doc = Document::new();
    let html = el(&mut doc, "html");
    let body = el(&mut doc, "body");
    let p = el(&mut doc, "p");
    doc.append_child(doc.root(), html).unwrap();
    doc.append_child(html, body).unwrap();
    doc.append_child(body, p).unwrap();
    doc.insert_text(p, "hi", None).unwrap();

    assert_eq!(
        names(&doc, doc.descendants(doc.root())),
        ["#document", "html", "body", "p", "#hi"]
    );
    assert_eq!(names(&doc, doc.ancestors(p)), ["body", "html", "#document"]);
    assert_eq!(doc.text_content(html), "hi");
}

#[test]
fn descendants_of_subtree_stop_at_subtree() {
    let mut doc = Document::new();
    let a = el(&mut doc, "a");
    let b = el(&mut doc, "b");
    let c = el(&mut doc, "c");
    doc.append_child(doc.root(), a).unwrap();
    doc.append_child(a, b).unwrap();
    doc.append_child(doc.root(), c).unwrap();
    assert_eq!(names(&doc, doc.descendants(a)), ["a", "b"]);
}

#[test]
fn insert_before_and_detach_keep_links_consistent() {
    let mut doc = Document::new();
    let parent = el(&mut doc, "ul");
    let [a, b, c] = ["a", "b", "c"].map(|n| el(&mut doc, n));
    doc.append_child(parent, a).unwrap();
    doc.append_child(parent, c).unwrap();
    doc.insert_before(parent, b, Some(c)).unwrap();
    assert_eq!(names(&doc, doc.children(parent)), ["a", "b", "c"]);

    doc.detach(b);
    assert_eq!(names(&doc, doc.children(parent)), ["a", "c"]);
    assert_eq!(doc.next_sibling(a), Some(c));
    assert_eq!(doc.prev_sibling(c), Some(a));
    assert_eq!(doc.parent(b), None);

    // Re-inserting a detached node works; moving a node detaches it first.
    doc.insert_before(parent, b, Some(a)).unwrap();
    doc.append_child(parent, b).unwrap();
    assert_eq!(names(&doc, doc.children(parent)), ["a", "c", "b"]);
    assert_eq!(doc.last_child(parent), Some(b));
    assert_eq!(doc.first_child(parent), Some(a));
}

#[test]
fn cycles_and_bad_references_are_rejected() {
    let mut doc = Document::new();
    let a = el(&mut doc, "a");
    let b = el(&mut doc, "b");
    doc.append_child(a, b).unwrap();
    assert_eq!(doc.append_child(b, a), Err(DomError::HierarchyRequest));
    assert_eq!(doc.append_child(a, a), Err(DomError::HierarchyRequest));
    assert_eq!(
        doc.append_child(a, doc.root()),
        Err(DomError::HierarchyRequest)
    );
    let t = doc.create_text("x");
    assert_eq!(doc.append_child(t, b), Err(DomError::HierarchyRequest));
    let stray = el(&mut doc, "c");
    assert_eq!(
        doc.insert_before(a, stray, Some(stray)),
        Err(DomError::NotFound)
    );
}

#[test]
fn adjacent_text_is_merged() {
    let mut doc = Document::new();
    let p = el(&mut doc, "p");
    doc.insert_text(p, "a", None).unwrap();
    doc.insert_text(p, "b", None).unwrap();
    let span = el(&mut doc, "span");
    doc.append_child(p, span).unwrap();
    doc.insert_text(p, "c", Some(span)).unwrap();
    assert_eq!(names(&doc, doc.children(p)), ["#abc", "span"]);
}

#[test]
fn attributes() {
    let mut doc = Document::new();
    let e = doc.create_element("a", vec![Attribute::new("href", "/x")]);
    assert_eq!(doc.attribute(e, "href"), Some("/x"));
    assert!(!doc.add_attribute_if_missing(e, Attribute::new("href", "/y")));
    assert!(doc.add_attribute_if_missing(e, Attribute::new("id", "k")));
    doc.set_attribute(e, "href", "/z");
    assert_eq!(doc.attribute(e, "href"), Some("/z"));
    assert_eq!(doc.attribute(e, "id"), Some("k"));
}

#[test]
fn generation_tracks_mutations() {
    let mut doc = Document::new();
    let g0 = doc.generation();
    let a = el(&mut doc, "a");
    assert_eq!(
        doc.generation(),
        g0,
        "creating a detached node is not a tree mutation"
    );
    doc.append_child(doc.root(), a).unwrap();
    assert!(doc.generation() > g0);
}

#[test]
fn deep_trees_do_not_overflow_the_stack() {
    let mut doc = Document::new();
    let mut parent = doc.root();
    for _ in 0..200_000 {
        let child = el(&mut doc, "div");
        doc.append_child(parent, child).unwrap();
        parent = child;
    }
    assert_eq!(doc.descendants(doc.root()).count(), 200_001);
}

#[test]
fn dump_handles_deep_trees_iteratively() {
    // Dump output is quadratic in depth (indentation), so keep this moderate;
    // the point is that it does not recurse.
    let mut doc = Document::new();
    let mut parent = doc.root();
    for _ in 0..2_000 {
        let child = el(&mut doc, "div");
        doc.append_child(parent, child).unwrap();
        parent = child;
    }
    assert_eq!(dump_tree(&doc, doc.root()).lines().count(), 2_000);
}

#[test]
fn dump_format() {
    let mut doc = Document::new();
    let dt = doc.create_doctype("html", "", "");
    doc.append_child(doc.root(), dt).unwrap();
    let html = doc.create_element(
        "html",
        vec![Attribute::new("lang", "en"), Attribute::new("dir", "ltr")],
    );
    doc.append_child(doc.root(), html).unwrap();
    let c = doc.create_comment(" hi ");
    doc.append_child(html, c).unwrap();
    doc.insert_text(html, "t", None).unwrap();
    assert_eq!(
        dump_tree(&doc, doc.root()),
        "| <!DOCTYPE html>\n| <html>\n|   dir=\"ltr\"\n|   lang=\"en\"\n|   <!--  hi  -->\n|   \"t\"\n"
    );
}
