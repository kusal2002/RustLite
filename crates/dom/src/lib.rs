//! Arena-based DOM.
//!
//! All nodes of a document live in one `Vec` and refer to each other through
//! [`NodeId`], a 4-byte index (`Option<NodeId>` is also 4 bytes). This gives:
//!
//! - one allocation per node payload at most, no `Rc<RefCell<_>>` chains;
//! - cheap, `Copy` node handles that later stages (style, layout, JS bindings)
//!   can store without borrowing the tree;
//! - stable identity: ids are never reused, so a stale id can never silently
//!   point at a different node.
//!
//! Removed nodes stay in the arena as *detached* nodes (no parent). They can be
//! re-inserted. Memory for detached nodes is reclaimed only when the document is
//! dropped; a compaction pass is future work once scripts can create garbage.

mod dump;
mod node;

pub use dump::dump_tree;
pub use node::{Attribute, ElementData, NodeData, QuirksMode};

use std::num::NonZeroU32;

/// Handle to a node in a [`Document`]. Only meaningful for the document that created it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(NonZeroU32);

impl NodeId {
    fn from_index(index: usize) -> Self {
        let raw = u32::try_from(index + 1).expect("DOM node count exceeds u32::MAX - 1");
        // `index + 1` is at least 1.
        Self(NonZeroU32::new(raw).expect("non-zero"))
    }

    /// Dense index of this node in its document (for side tables such as styles).
    pub fn index(self) -> usize {
        self.0.get() as usize - 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DomError {
    #[error("the operation would create an invalid tree (cycle or misplaced document node)")]
    HierarchyRequest,
    #[error("the reference node is not a child of the parent")]
    NotFound,
}

#[derive(Debug, Clone)]
struct Node {
    parent: Option<NodeId>,
    first_child: Option<NodeId>,
    last_child: Option<NodeId>,
    prev_sibling: Option<NodeId>,
    next_sibling: Option<NodeId>,
    data: NodeData,
}

/// A document and every node created for it.
#[derive(Debug, Clone)]
pub struct Document {
    nodes: Vec<Node>,
    quirks_mode: QuirksMode,
    generation: u64,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        let mut doc = Self {
            nodes: Vec::new(),
            quirks_mode: QuirksMode::NoQuirks,
            generation: 0,
        };
        doc.push(NodeData::Document);
        doc
    }

    /// The document node (always the first node).
    pub fn root(&self) -> NodeId {
        NodeId::from_index(0)
    }

    pub fn quirks_mode(&self) -> QuirksMode {
        self.quirks_mode
    }

    pub fn set_quirks_mode(&mut self, mode: QuirksMode) {
        self.quirks_mode = mode;
    }

    /// Increments on every tree or attribute mutation. Consumers (style, layout)
    /// compare generations to detect that cached results are stale.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Total nodes ever created, including detached ones.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Approximate heap bytes: the node arena plus string payloads
    /// (allocator overhead is not counted).
    pub fn heap_bytes(&self) -> usize {
        let payload: usize = self
            .nodes
            .iter()
            .map(|n| match &n.data {
                NodeData::Document => 0,
                NodeData::Doctype {
                    name,
                    public_id,
                    system_id,
                } => name.capacity() + public_id.capacity() + system_id.capacity(),
                NodeData::Element(e) => {
                    e.name.len()
                        + e.attrs.capacity() * std::mem::size_of::<Attribute>()
                        + e.attrs
                            .iter()
                            .map(|a| a.name.len() + a.value.capacity())
                            .sum::<usize>()
                }
                NodeData::Text(t) | NodeData::Comment(t) => t.capacity(),
            })
            .sum();
        self.nodes.capacity() * std::mem::size_of::<Node>() + payload
    }

    fn push(&mut self, data: NodeData) -> NodeId {
        let id = NodeId::from_index(self.nodes.len());
        self.nodes.push(Node {
            parent: None,
            first_child: None,
            last_child: None,
            prev_sibling: None,
            next_sibling: None,
            data,
        });
        id
    }

    fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    // ----- creation -------------------------------------------------------

    /// Creates a detached element. `name` must already be normalized (HTML
    /// parser lowercases ASCII).
    pub fn create_element(&mut self, name: impl Into<Box<str>>, attrs: Vec<Attribute>) -> NodeId {
        self.push(NodeData::Element(ElementData {
            name: name.into(),
            attrs,
        }))
    }

    pub fn create_text(&mut self, text: impl Into<String>) -> NodeId {
        self.push(NodeData::Text(text.into()))
    }

    pub fn create_comment(&mut self, text: impl Into<String>) -> NodeId {
        self.push(NodeData::Comment(text.into()))
    }

    pub fn create_doctype(
        &mut self,
        name: impl Into<String>,
        public_id: impl Into<String>,
        system_id: impl Into<String>,
    ) -> NodeId {
        self.push(NodeData::Doctype {
            name: name.into(),
            public_id: public_id.into(),
            system_id: system_id.into(),
        })
    }

    // ----- access ---------------------------------------------------------

    pub fn data(&self, id: NodeId) -> &NodeData {
        &self.node(id).data
    }

    pub fn element(&self, id: NodeId) -> Option<&ElementData> {
        match &self.node(id).data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    /// The element's local name, or `None` for non-elements.
    pub fn tag_name(&self, id: NodeId) -> Option<&str> {
        self.element(id).map(|e| &*e.name)
    }

    pub fn is_element_named(&self, id: NodeId, name: &str) -> bool {
        self.tag_name(id) == Some(name)
    }

    pub fn attribute(&self, id: NodeId, name: &str) -> Option<&str> {
        self.element(id)?.attribute(name)
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).parent
    }

    pub fn first_child(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).first_child
    }

    pub fn last_child(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).last_child
    }

    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).next_sibling
    }

    pub fn prev_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).prev_sibling
    }

    pub fn children(&self, id: NodeId) -> Children<'_> {
        Children {
            doc: self,
            next: self.first_child(id),
        }
    }

    /// `id` and all its descendants in tree order (pre-order).
    pub fn descendants(&self, id: NodeId) -> Descendants<'_> {
        Descendants {
            doc: self,
            root: id,
            next: Some(id),
        }
    }

    /// Ancestors of `id`, nearest first, excluding `id` itself.
    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.parent(id), |&n| self.parent(n))
    }

    /// `true` if `ancestor` is `id` or one of its ancestors.
    pub fn is_inclusive_ancestor(&self, ancestor: NodeId, id: NodeId) -> bool {
        ancestor == id || self.ancestors(id).any(|a| a == ancestor)
    }

    /// Concatenated text of all descendant text nodes.
    pub fn text_content(&self, id: NodeId) -> String {
        match &self.node(id).data {
            NodeData::Text(t) | NodeData::Comment(t) => t.clone(),
            _ => {
                let mut out = String::new();
                for n in self.descendants(id) {
                    if let NodeData::Text(t) = &self.node(n).data {
                        out.push_str(t);
                    }
                }
                out
            }
        }
    }

    /// First element in tree order under `root` with the given local name.
    pub fn find_element(&self, root: NodeId, name: &str) -> Option<NodeId> {
        self.descendants(root)
            .find(|&n| self.is_element_named(n, name))
    }

    // ----- mutation -------------------------------------------------------

    fn check_insert(&self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        // A childless node can only be an inclusive ancestor of itself, so the
        // O(depth) ancestor walk is skipped for leaves. The parser only ever
        // inserts leaves, which keeps parsing deeply nested markup linear.
        let creates_cycle = if self.node(child).first_child.is_none() {
            child == parent
        } else {
            self.is_inclusive_ancestor(child, parent)
        };
        if matches!(self.node(child).data, NodeData::Document)
            || !self.node(parent).data.can_have_children()
            || creates_cycle
        {
            return Err(DomError::HierarchyRequest);
        }
        Ok(())
    }

    /// Appends `child` as the last child of `parent`, detaching it first.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        self.insert_before(parent, child, None)
    }

    /// Inserts `child` into `parent` before `reference` (or last if `None`),
    /// detaching it from any previous position first.
    pub fn insert_before(
        &mut self,
        parent: NodeId,
        child: NodeId,
        reference: Option<NodeId>,
    ) -> Result<(), DomError> {
        self.check_insert(parent, child)?;
        if let Some(r) = reference {
            if self.parent(r) != Some(parent) {
                return Err(DomError::NotFound);
            }
            if r == child {
                return Ok(());
            }
        }
        self.detach(child);

        let prev = match reference {
            Some(r) => self.node(r).prev_sibling,
            None => self.node(parent).last_child,
        };
        {
            let c = self.node_mut(child);
            c.parent = Some(parent);
            c.prev_sibling = prev;
            c.next_sibling = reference;
        }
        match prev {
            Some(p) => self.node_mut(p).next_sibling = Some(child),
            None => self.node_mut(parent).first_child = Some(child),
        }
        match reference {
            Some(r) => self.node_mut(r).prev_sibling = Some(child),
            None => self.node_mut(parent).last_child = Some(child),
        }
        self.generation += 1;
        Ok(())
    }

    /// Removes `id` from its parent. The node and its subtree remain valid and
    /// can be re-inserted. No-op for already-detached nodes.
    pub fn detach(&mut self, id: NodeId) {
        let Some(parent) = self.node(id).parent else {
            return;
        };
        let (prev, next) = (self.node(id).prev_sibling, self.node(id).next_sibling);
        match prev {
            Some(p) => self.node_mut(p).next_sibling = next,
            None => self.node_mut(parent).first_child = next,
        }
        match next {
            Some(n) => self.node_mut(n).prev_sibling = prev,
            None => self.node_mut(parent).last_child = prev,
        }
        let n = self.node_mut(id);
        n.parent = None;
        n.prev_sibling = None;
        n.next_sibling = None;
        self.generation += 1;
    }

    /// Moves all children of `from` to the end of `to`, preserving order.
    pub fn reparent_children(&mut self, from: NodeId, to: NodeId) -> Result<(), DomError> {
        while let Some(child) = self.first_child(from) {
            self.append_child(to, child)?;
        }
        Ok(())
    }

    /// Inserts text before `reference` (or at the end), merging with an
    /// adjacent preceding text node as the HTML parser requires.
    pub fn insert_text(
        &mut self,
        parent: NodeId,
        text: &str,
        reference: Option<NodeId>,
    ) -> Result<(), DomError> {
        if text.is_empty() {
            return Ok(());
        }
        let prev = match reference {
            Some(r) => {
                if self.parent(r) != Some(parent) {
                    return Err(DomError::NotFound);
                }
                self.prev_sibling(r)
            }
            None => self.last_child(parent),
        };
        if let Some(p) = prev
            && let NodeData::Text(existing) = &mut self.node_mut(p).data
        {
            existing.push_str(text);
            self.generation += 1;
            return Ok(());
        }
        let node = self.create_text(text);
        self.insert_before(parent, node, reference)
    }

    /// Adds `attr` unless an attribute with the same name already exists.
    /// Returns `true` if it was added. No-op for non-elements.
    pub fn add_attribute_if_missing(&mut self, id: NodeId, attr: Attribute) -> bool {
        let NodeData::Element(e) = &mut self.node_mut(id).data else {
            return false;
        };
        if e.attribute(&attr.name).is_some() {
            return false;
        }
        e.attrs.push(attr);
        self.generation += 1;
        true
    }

    /// Removes an attribute. Returns whether it was present.
    pub fn remove_attribute(&mut self, id: NodeId, name: &str) -> bool {
        let NodeData::Element(e) = &mut self.node_mut(id).data else {
            return false;
        };
        let before = e.attrs.len();
        e.attrs.retain(|a| &*a.name != name);
        let removed = e.attrs.len() != before;
        if removed {
            self.generation += 1;
        }
        removed
    }

    /// DOM `textContent` setter: text and comment nodes get new data;
    /// elements and fragments lose their children and get one text child
    /// (none for empty text). Document and doctype nodes are unchanged.
    pub fn set_text_content(&mut self, id: NodeId, text: &str) {
        match &mut self.node_mut(id).data {
            NodeData::Text(t) | NodeData::Comment(t) => {
                *t = text.to_owned();
                self.generation += 1;
            }
            NodeData::Element(_) => {
                while let Some(child) = self.first_child(id) {
                    self.detach(child);
                }
                if !text.is_empty() {
                    let t = self.create_text(text);
                    // `id` is an element and `t` a fresh leaf: cannot fail.
                    let _ = self.append_child(id, t);
                }
                self.generation += 1;
            }
            NodeData::Document | NodeData::Doctype { .. } => {}
        }
    }

    /// Sets (or replaces) an attribute value. No-op for non-elements.
    pub fn set_attribute(&mut self, id: NodeId, name: &str, value: impl Into<String>) {
        let NodeData::Element(e) = &mut self.node_mut(id).data else {
            return;
        };
        let value = value.into();
        match e.attrs.iter_mut().find(|a| &*a.name == name) {
            Some(a) => a.value = value,
            None => e.attrs.push(Attribute::new(name, value)),
        }
        self.generation += 1;
    }
}

#[derive(Debug)]
pub struct Children<'a> {
    doc: &'a Document,
    next: Option<NodeId>,
}

impl Iterator for Children<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self.doc.next_sibling(current);
        Some(current)
    }
}

/// Non-recursive pre-order traversal, so deep trees cannot overflow the stack.
#[derive(Debug)]
pub struct Descendants<'a> {
    doc: &'a Document,
    root: NodeId,
    next: Option<NodeId>,
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self.doc.first_child(current).or_else(|| {
            let mut n = current;
            loop {
                if n == self.root {
                    return None;
                }
                if let Some(s) = self.doc.next_sibling(n) {
                    return Some(s);
                }
                n = self.doc.parent(n)?;
            }
        });
        Some(current)
    }
}

#[cfg(test)]
mod tests;
