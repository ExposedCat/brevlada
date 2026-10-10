//! An HTML parser that builds the same trees as html5ever does for scraper.
mod doctype;
mod entities;
#[cfg(test)]
mod parity;
mod serialize;
mod tokenizer;
mod tree_builder;

use std::{borrow::Cow, iter};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Namespace {
    Html,
    Svg,
    MathMl,
}

/// Namespaced attribute kinds, ordered like html5ever orders attribute
/// prefixes: none, the empty prefix of `xmlns`, then `xlink`, `xml`, `xmlns`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Prefix {
    None,
    Xmlns,
    Xlink,
    Xml,
    XmlnsColon,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Attribute<'a> {
    prefix: Prefix,
    name: Cow<'a, str>,
    value: Cow<'a, str>,
}

struct Element<'a> {
    namespace: Namespace,
    name: Cow<'a, str>,
    /// Sorted by prefix and name, as scraper stores them.
    attributes: Vec<Attribute<'a>>,
}

enum Data<'a> {
    Root,
    /// A template's contents, kept as the template's first child like scraper does.
    Contents,
    Element(Element<'a>),
    Text(Cow<'a, str>),
    Comment(Cow<'a, str>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct NodeId(usize);

struct Node<'a> {
    parent: Option<NodeId>,
    previous_sibling: Option<NodeId>,
    next_sibling: Option<NodeId>,
    first_child: Option<NodeId>,
    last_child: Option<NodeId>,
    data: Data<'a>,
}

/// A parsed HTML tree whose nodes keep creation order, like scraper's arena.
pub struct Document<'a> {
    nodes: Vec<Node<'a>>,
}

const ROOT: NodeId = NodeId(0);

impl<'a> Document<'a> {
    pub fn parse(html: &'a str) -> Self {
        tree_builder::TreeBuilder::document(html).run()
    }

    /// Parses `html` as the contents of a `<body>` element.
    pub fn parse_fragment(html: &'a str) -> Self {
        tree_builder::TreeBuilder::fragment(html).run()
    }

    pub fn root_element(&self) -> ElementRef<'_> {
        self.node(ROOT)
            .children()
            .find_map(NodeRef::as_element)
            .expect("parsed documents have a root element")
    }

    /// Elements attached to a parent, in creation order, the order scraper selects them in.
    pub fn elements(&self) -> impl Iterator<Item = ElementRef<'_>> {
        (0..self.nodes.len())
            .map(|index| self.node(NodeId(index)))
            .filter(|node| node.parent().is_some())
            .filter_map(NodeRef::as_element)
    }

    fn node(&self, id: NodeId) -> NodeRef<'_> {
        NodeRef { document: self, id }
    }

    fn new() -> Self {
        let mut document = Self { nodes: Vec::new() };
        document.create(Data::Root);
        document
    }

    fn create(&mut self, data: Data<'a>) -> NodeId {
        self.nodes.push(Node {
            parent: None,
            previous_sibling: None,
            next_sibling: None,
            first_child: None,
            last_child: None,
            data,
        });
        NodeId(self.nodes.len() - 1)
    }

    fn create_element(
        &mut self,
        namespace: Namespace,
        name: Cow<'a, str>,
        mut attributes: Vec<Attribute<'a>>,
    ) -> NodeId {
        attributes.sort_unstable_by(|left, right| {
            (left.prefix, &left.name).cmp(&(right.prefix, &right.name))
        });
        let template = namespace == Namespace::Html && name == "template";
        let element = self.create(Data::Element(Element {
            namespace,
            name,
            attributes,
        }));
        if template {
            let contents = self.create(Data::Contents);
            self.append(element, contents);
        }
        element
    }

    fn element(&self, id: NodeId) -> &Element<'a> {
        match &self.nodes[id.0].data {
            Data::Element(element) => element,
            _ => unreachable!("tree builder handles always point to elements"),
        }
    }

    fn detach(&mut self, id: NodeId) {
        let Some(parent) = self.nodes[id.0].parent.take() else {
            return;
        };
        let previous = self.nodes[id.0].previous_sibling.take();
        let next = self.nodes[id.0].next_sibling.take();
        match previous {
            Some(previous) => self.nodes[previous.0].next_sibling = next,
            None => self.nodes[parent.0].first_child = next,
        }
        match next {
            Some(next) => self.nodes[next.0].previous_sibling = previous,
            None => self.nodes[parent.0].last_child = previous,
        }
    }

    fn append(&mut self, parent: NodeId, child: NodeId) {
        let last = self.nodes[parent.0].last_child;
        if last == Some(child) {
            return;
        }
        self.detach(child);
        self.nodes[child.0].parent = Some(parent);
        self.nodes[child.0].previous_sibling = last;
        match last {
            Some(last) => self.nodes[last.0].next_sibling = Some(child),
            None => self.nodes[parent.0].first_child = Some(child),
        }
        self.nodes[parent.0].last_child = Some(child);
    }

    fn insert_before(&mut self, sibling: NodeId, child: NodeId) {
        self.detach(child);
        let parent = self.nodes[sibling.0].parent;
        let previous = self.nodes[sibling.0].previous_sibling;
        self.nodes[child.0].parent = parent;
        self.nodes[child.0].previous_sibling = previous;
        self.nodes[child.0].next_sibling = Some(sibling);
        self.nodes[sibling.0].previous_sibling = Some(child);
        match previous {
            Some(previous) => self.nodes[previous.0].next_sibling = Some(child),
            None => {
                let parent = parent.expect("siblings have a parent");
                self.nodes[parent.0].first_child = Some(child);
            }
        }
    }

    fn append_text(&mut self, parent: NodeId, text: Cow<'a, str>) {
        if let Some(last) = self.nodes[parent.0].last_child
            && let Data::Text(existing) = &mut self.nodes[last.0].data
        {
            existing.to_mut().push_str(&text);
            return;
        }
        let node = self.create(Data::Text(text));
        self.append(parent, node);
    }

    fn insert_text_before(&mut self, sibling: NodeId, text: Cow<'a, str>) {
        if let Some(previous) = self.nodes[sibling.0].previous_sibling
            && let Data::Text(existing) = &mut self.nodes[previous.0].data
        {
            existing.to_mut().push_str(&text);
            return;
        }
        let node = self.create(Data::Text(text));
        self.insert_before(sibling, node);
    }

    fn reparent_children(&mut self, from: NodeId, to: NodeId) {
        let Some(first) = self.nodes[from.0].first_child.take() else {
            return;
        };
        let last = self.nodes[from.0].last_child.take();
        let mut child = Some(first);
        while let Some(id) = child {
            self.nodes[id.0].parent = Some(to);
            child = self.nodes[id.0].next_sibling;
        }
        match self.nodes[to.0].last_child {
            Some(previous) => {
                self.nodes[previous.0].next_sibling = Some(first);
                self.nodes[first.0].previous_sibling = Some(previous);
            }
            None => self.nodes[to.0].first_child = Some(first),
        }
        self.nodes[to.0].last_child = last;
    }

    fn add_attributes_if_missing(&mut self, id: NodeId, attributes: Vec<Attribute<'a>>) {
        let Data::Element(element) = &mut self.nodes[id.0].data else {
            unreachable!("only elements receive attributes");
        };
        for attribute in attributes {
            let key = (attribute.prefix, &attribute.name);
            if let Err(index) = element
                .attributes
                .binary_search_by(|existing| (existing.prefix, &existing.name).cmp(&key))
            {
                element.attributes.insert(index, attribute);
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct NodeRef<'d> {
    document: &'d Document<'d>,
    id: NodeId,
}

impl<'d> NodeRef<'d> {
    fn node(self) -> &'d Node<'d> {
        &self.document.nodes[self.id.0]
    }

    fn link(self, id: Option<NodeId>) -> Option<Self> {
        id.map(|id| self.document.node(id))
    }

    pub fn parent(self) -> Option<Self> {
        self.link(self.node().parent)
    }

    pub fn children(self) -> impl Iterator<Item = Self> {
        iter::successors(self.link(self.node().first_child), |child| {
            child.link(child.node().next_sibling)
        })
    }

    fn children_reversed(self) -> impl Iterator<Item = Self> {
        iter::successors(self.link(self.node().last_child), |child| {
            child.link(child.node().previous_sibling)
        })
    }

    /// The node and everything below it, in document order.
    pub fn descendants(self) -> impl Iterator<Item = Self> {
        iter::successors(Some(self), move |&node| {
            if let Some(child) = node.link(node.node().first_child) {
                return Some(child);
            }
            let mut current = node;
            while current.id != self.id {
                if let Some(next) = current.link(current.node().next_sibling) {
                    return Some(next);
                }
                current = current.parent()?;
            }
            None
        })
    }

    pub fn ancestors(self) -> impl Iterator<Item = Self> {
        iter::successors(self.parent(), |node| node.parent())
    }

    pub fn as_text(self) -> Option<&'d str> {
        match &self.node().data {
            Data::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_element(self) -> Option<ElementRef<'d>> {
        match &self.node().data {
            Data::Element(element) => Some(ElementRef {
                node: self,
                element,
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ElementRef<'d> {
    node: NodeRef<'d>,
    element: &'d Element<'d>,
}

impl PartialEq for ElementRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.element, other.element)
    }
}

impl<'d> ElementRef<'d> {
    pub fn node(self) -> NodeRef<'d> {
        self.node
    }

    pub fn name(self) -> &'d str {
        &self.element.name
    }

    pub fn is_html(self) -> bool {
        self.element.namespace == Namespace::Html
    }

    pub fn attr(self, name: &str) -> Option<&'d str> {
        self.element
            .attributes
            .iter()
            .find(|attribute| attribute.prefix == Prefix::None && attribute.name == name)
            .map(|attribute| &*attribute.value)
    }

    pub fn attrs(self) -> impl Iterator<Item = (&'d str, &'d str)> {
        self.element
            .attributes
            .iter()
            .map(|attribute| (&*attribute.name, &*attribute.value))
    }

    /// Text nodes below the element, in document order.
    pub fn text(self) -> impl Iterator<Item = &'d str> {
        self.node.descendants().filter_map(NodeRef::as_text)
    }

    /// The element's text without `skip` elements, with `block` elements set
    /// apart by spaces. Template contents are left out.
    pub fn layout_text(self, skip: impl Fn(&str) -> bool, block: impl Fn(&str) -> bool) -> String {
        let mut text = String::new();
        // `None` closes a block element; walking without recursion handles any nesting depth.
        let mut pending = vec![Some(self.node)];
        while let Some(node) = pending.pop() {
            let Some(node) = node else {
                text.push(' ');
                continue;
            };
            if let Some(value) = node.as_text() {
                text.push_str(value);
            } else if let Some(element) = node.as_element()
                && !skip(element.name())
            {
                if block(element.name()) {
                    text.push(' ');
                    pending.push(None);
                }
                pending.extend(node.children_reversed().map(Some));
            }
        }
        text
    }

    pub fn html(self) -> String {
        serialize::html(self.node, true)
    }

    pub fn inner_html(self) -> String {
        serialize::html(self.node, false)
    }
}
