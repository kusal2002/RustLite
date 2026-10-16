/// An element attribute. Names are stored as the parser normalized them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name: Box<str>,
    pub value: String,
}

impl Attribute {
    pub fn new(name: impl Into<Box<str>>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// Element payload. Only the HTML namespace is supported so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementData {
    pub name: Box<str>,
    pub attrs: Vec<Attribute>,
}

impl ElementData {
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| &*a.name == name)
            .map(|a| a.value.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeData {
    Document,
    Doctype {
        name: String,
        public_id: String,
        system_id: String,
    },
    Element(ElementData),
    Text(String),
    Comment(String),
}

impl NodeData {
    pub(crate) fn can_have_children(&self) -> bool {
        matches!(self, NodeData::Document | NodeData::Element(_))
    }
}

/// Document rendering mode, determined by the doctype.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuirksMode {
    #[default]
    NoQuirks,
    LimitedQuirks,
    Quirks,
}
