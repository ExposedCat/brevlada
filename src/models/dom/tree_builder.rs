//! Tree construction, ported rule for rule from html5ever 0.39 as scraper drives it.
mod foreign;
mod rules;

use super::{
    Attribute, Document, Namespace, NodeId, ROOT,
    names::LocalName as N,
    tokenizer::{self, RawKind, Tag, Tokenizer},
};
use std::borrow::Cow;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    AfterHead,
    InBody,
    Text,
    InTable,
    InTableText,
    InCaption,
    InColumnGroup,
    InTableBody,
    InRow,
    InCell,
    InTemplate,
    AfterBody,
    InFrameset,
    AfterFrameset,
    AfterAfterBody,
    AfterAfterFrameset,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SplitStatus {
    NotSplit,
    Whitespace,
    NotWhitespace,
}

enum Token<'a> {
    Tag(Tag<'a>),
    Comment(Cow<'a, str>),
    Characters(SplitStatus, Cow<'a, str>),
    NullCharacter,
    Eof,
}

enum Step<'a> {
    Done,
    SplitWhitespace(Cow<'a, str>),
    Reprocess(Mode, Token<'a>),
}

enum Formatting<'a> {
    Marker,
    Element(NodeId, Tag<'a>),
}

enum InsertionPoint {
    LastChild(NodeId),
    FosterParent { table: NodeId, previous: NodeId },
}

enum Child<'a> {
    Node(NodeId),
    Text(Cow<'a, str>),
}

enum Bookmark {
    Replace(NodeId),
    InsertAfter(NodeId),
}

pub(super) struct TreeBuilder<'a> {
    document: Document<'a>,
    tokenizer: Tokenizer<'a>,
    mode: Mode,
    original_mode: Option<Mode>,
    template_modes: Vec<Mode>,
    pending_table_text: Vec<(SplitStatus, Cow<'a, str>)>,
    quirks: bool,
    open: Vec<NodeId>,
    formatting: Vec<Formatting<'a>>,
    head: Option<NodeId>,
    form: Option<NodeId>,
    frameset_ok: bool,
    ignore_lf: bool,
    foster_parenting: bool,
    context: Option<NodeId>,
}

fn any_not_whitespace(text: &str) -> bool {
    text.bytes().any(|byte| !byte.is_ascii_whitespace())
}

fn split(text: Cow<'_, str>, at: usize) -> (Cow<'_, str>, Cow<'_, str>) {
    match text {
        Cow::Borrowed(text) => (Cow::Borrowed(&text[..at]), Cow::Borrowed(&text[at..])),
        Cow::Owned(mut text) => {
            let rest = text.split_off(at);
            (Cow::Owned(text), Cow::Owned(rest))
        }
    }
}

fn html(namespace: Namespace, name: N, names: &[N]) -> bool {
    namespace == Namespace::Html && names.contains(&name)
}

fn mathml_text_integration_point(namespace: Namespace, name: N) -> bool {
    namespace == Namespace::MathMl && matches!(name, N::Mi | N::Mo | N::Mn | N::Ms | N::Mtext)
}

fn svg_html_integration_point(namespace: Namespace, name: N) -> bool {
    namespace == Namespace::Svg && matches!(name, N::ForeignObject | N::Desc | N::Title)
}

fn default_scope(namespace: Namespace, name: N) -> bool {
    (namespace == Namespace::Html
        && matches!(
            name,
            N::Applet
                | N::Caption
                | N::Html
                | N::Table
                | N::Td
                | N::Th
                | N::Marquee
                | N::Object
                | N::Select
                | N::Template
        ))
        || mathml_text_integration_point(namespace, name)
        || svg_html_integration_point(namespace, name)
}

fn list_item_scope(namespace: Namespace, name: N) -> bool {
    default_scope(namespace, name) || html(namespace, name, &[N::Ol, N::Ul])
}

fn button_scope(namespace: Namespace, name: N) -> bool {
    default_scope(namespace, name) || html(namespace, name, &[N::Button])
}

fn table_scope(namespace: Namespace, name: N) -> bool {
    html(namespace, name, &[N::Html, N::Table, N::Template])
}

fn table_body_context(namespace: Namespace, name: N) -> bool {
    html(
        namespace,
        name,
        &[N::Tbody, N::Tfoot, N::Thead, N::Template, N::Html],
    )
}

fn table_row_context(namespace: Namespace, name: N) -> bool {
    html(namespace, name, &[N::Tr, N::Template, N::Html])
}

fn td_th(namespace: Namespace, name: N) -> bool {
    html(namespace, name, &[N::Td, N::Th])
}

fn heading(namespace: Namespace, name: N) -> bool {
    html(namespace, name, &[N::H1, N::H2, N::H3, N::H4, N::H5, N::H6])
}

fn cursory_implied_end(namespace: Namespace, name: N) -> bool {
    namespace == Namespace::Html
        && matches!(
            name,
            N::Dd | N::Dt | N::Li | N::Option | N::Optgroup | N::P | N::Rb | N::Rp | N::Rt | N::Rtc
        )
}

fn thorough_implied_end(namespace: Namespace, name: N) -> bool {
    cursory_implied_end(namespace, name)
        || html(
            namespace,
            name,
            &[
                N::Caption,
                N::Colgroup,
                N::Tbody,
                N::Td,
                N::Tfoot,
                N::Th,
                N::Thead,
                N::Tr,
            ],
        )
}

fn special(namespace: Namespace, name: N) -> bool {
    namespace == Namespace::Html
        && matches!(
            name,
            N::Address
                | N::Applet
                | N::Area
                | N::Article
                | N::Aside
                | N::Base
                | N::Basefont
                | N::Bgsound
                | N::Blockquote
                | N::Body
                | N::Br
                | N::Button
                | N::Caption
                | N::Center
                | N::Col
                | N::Colgroup
                | N::Dd
                | N::Details
                | N::Dir
                | N::Div
                | N::Dl
                | N::Dt
                | N::Embed
                | N::Fieldset
                | N::Figcaption
                | N::Figure
                | N::Footer
                | N::Form
                | N::Frame
                | N::Frameset
                | N::H1
                | N::H2
                | N::H3
                | N::H4
                | N::H5
                | N::H6
                | N::Head
                | N::Header
                | N::Hgroup
                | N::Hr
                | N::Html
                | N::Iframe
                | N::Img
                | N::Input
                | N::Isindex
                | N::Li
                | N::Link
                | N::Listing
                | N::Main
                | N::Marquee
                | N::Menu
                | N::Meta
                | N::Nav
                | N::Noembed
                | N::Noframes
                | N::Noscript
                | N::Object
                | N::Ol
                | N::P
                | N::Param
                | N::Plaintext
                | N::Pre
                | N::Script
                | N::Section
                | N::Select
                | N::Source
                | N::Style
                | N::Summary
                | N::Table
                | N::Tbody
                | N::Td
                | N::Template
                | N::Textarea
                | N::Tfoot
                | N::Th
                | N::Thead
                | N::Title
                | N::Tr
                | N::Track
                | N::Ul
                | N::Wbr
                | N::Xmp
        )
}

/// Formatting elements match when their names and attribute sets are equal.
fn equivalent(left: &Tag<'_>, right: &Tag<'_>) -> bool {
    left.name == right.name
        && left.attributes.len() == right.attributes.len()
        && left
            .attributes
            .iter()
            .all(|attribute| right.attributes.contains(attribute))
}

impl<'a> TreeBuilder<'a> {
    fn new(html: &'a str) -> Self {
        Self {
            document: Document::new(html),
            tokenizer: Tokenizer::new(html),
            mode: Mode::Initial,
            original_mode: None,
            template_modes: Vec::new(),
            pending_table_text: Vec::new(),
            quirks: false,
            open: Vec::new(),
            formatting: Vec::new(),
            head: None,
            form: None,
            frameset_ok: true,
            ignore_lf: false,
            foster_parenting: false,
            context: None,
        }
    }

    pub(super) fn document(html: &'a str) -> Self {
        Self::new(html)
    }

    /// The fragment parsing algorithm with a `<body>` context element.
    pub(super) fn fragment(html: &'a str) -> Self {
        let mut builder = Self::new(html);
        let context = builder.document.create_element(
            Namespace::Html,
            N::Body,
            Cow::Borrowed("body"),
            Vec::new(),
        );
        builder.context = Some(context);
        builder.create_root(Vec::new());
        builder.mode = builder.reset_insertion_mode();
        builder
    }

    pub(super) fn run(mut self) -> Document<'a> {
        loop {
            let foreign = !self.open.is_empty()
                && self.namespace(self.adjusted_current_node()) != Namespace::Html;
            let token = self.tokenizer.next(foreign);
            let eof = matches!(token, tokenizer::Token::Eof);
            self.process_token(token);
            if eof {
                return self.document;
            }
        }
    }

    fn process_token(&mut self, token: tokenizer::Token<'a>) {
        let ignore_lf = std::mem::take(&mut self.ignore_lf);
        let token = match token {
            tokenizer::Token::Doctype(doctype) => {
                if self.mode == Mode::Initial {
                    self.quirks = doctype.quirks();
                    self.mode = Mode::BeforeHtml;
                }
                return;
            }
            tokenizer::Token::Tag(tag) => Token::Tag(tag),
            tokenizer::Token::Comment(text) => Token::Comment(text),
            tokenizer::Token::NullCharacter => Token::NullCharacter,
            tokenizer::Token::Eof => Token::Eof,
            tokenizer::Token::Characters { text, after_error } => {
                let text = if ignore_lf && !after_error && text.starts_with('\n') {
                    split(text, 1).1
                } else {
                    text
                };
                if text.is_empty() {
                    return;
                }
                Token::Characters(SplitStatus::NotSplit, text)
            }
        };
        self.process(token);
    }

    fn process(&mut self, mut token: Token<'a>) {
        let mut rest = None;
        loop {
            let step = if self.is_foreign(&token) {
                self.step_foreign(token)
            } else {
                self.step(self.mode, token)
            };
            match step {
                Step::Done => match rest.take() {
                    Some(next) => token = next,
                    None => return,
                },
                Step::Reprocess(mode, next) => {
                    self.mode = mode;
                    token = next;
                }
                Step::SplitWhitespace(text) => {
                    let whitespace =
                        text.starts_with(|character: char| character.is_ascii_whitespace());
                    let end = text
                        .find(|character: char| character.is_ascii_whitespace() != whitespace)
                        .unwrap_or(text.len());
                    let (first, second) = split(text, end);
                    if !second.is_empty() {
                        rest = Some(Token::Characters(SplitStatus::NotSplit, second));
                    }
                    let split = if whitespace {
                        SplitStatus::Whitespace
                    } else {
                        SplitStatus::NotWhitespace
                    };
                    token = Token::Characters(split, first);
                }
            }
        }
    }

    fn is_foreign(&self, token: &Token<'a>) -> bool {
        if matches!(token, Token::Eof) || self.open.is_empty() {
            return false;
        }
        let (namespace, name) = self.name(self.adjusted_current_node());
        let start = |token: &Token<'a>| matches!(token, Token::Tag(tag) if !tag.end);
        match namespace {
            Namespace::Html => false,
            _ if mathml_text_integration_point(namespace, name) => match token {
                Token::Characters(..) | Token::NullCharacter => false,
                Token::Tag(tag) => tag.end || matches!(tag.local, N::Mglyph | N::Malignmark),
                _ => true,
            },
            _ if svg_html_integration_point(namespace, name) => {
                !matches!(token, Token::Characters(..) | Token::NullCharacter) && !start(token)
            }
            Namespace::MathMl if name == N::AnnotationXml => {
                !matches!(token, Token::Tag(tag) if !tag.end && tag.local == N::Svg)
            }
            _ => true,
        }
    }

    fn name(&self, node: NodeId) -> (Namespace, N) {
        let element = self.document.element(node);
        (element.namespace, element.local)
    }

    fn namespace(&self, node: NodeId) -> Namespace {
        self.document.element(node).namespace
    }

    fn html_named(&self, node: NodeId, name: N) -> bool {
        self.name(node) == (Namespace::Html, name)
    }

    fn in_set(&self, node: NodeId, set: fn(Namespace, N) -> bool) -> bool {
        let (namespace, name) = self.name(node);
        set(namespace, name)
    }

    fn current_node(&self) -> NodeId {
        *self.open.last().expect("no current element")
    }

    fn adjusted_current_node(&self) -> NodeId {
        match self.context {
            Some(context) if self.open.len() == 1 => context,
            _ => self.current_node(),
        }
    }

    fn current_named(&self, name: N) -> bool {
        self.html_named(self.current_node(), name)
    }

    fn current_in(&self, set: fn(Namespace, N) -> bool) -> bool {
        self.in_set(self.current_node(), set)
    }

    fn in_html_named(&self, name: N) -> bool {
        self.open.iter().any(|&node| self.html_named(node, name))
    }

    fn in_scope(&self, scope: fn(Namespace, N) -> bool, matches: impl Fn(NodeId) -> bool) -> bool {
        for &node in self.open.iter().rev() {
            if matches(node) {
                return true;
            }
            if self.in_set(node, scope) {
                return false;
            }
        }
        false
    }

    fn in_scope_named(&self, scope: fn(Namespace, N) -> bool, name: N) -> bool {
        self.in_scope(scope, |node| self.html_named(node, name))
    }

    fn body_element(&self) -> Option<NodeId> {
        self.open
            .get(1)
            .copied()
            .filter(|&node| self.html_named(node, N::Body))
    }

    fn template_contents(&self, template: NodeId) -> NodeId {
        self.document[template]
            .first_child
            .expect("templates keep their contents as the first child")
    }

    fn appropriate_place_for_insertion(&self, override_target: Option<NodeId>) -> InsertionPoint {
        let target = override_target.unwrap_or_else(|| self.current_node());
        let foster = |namespace, name: N| {
            html(
                namespace,
                name,
                &[N::Table, N::Tbody, N::Tfoot, N::Thead, N::Tr],
            )
        };
        if !(self.foster_parenting && self.in_set(target, foster)) {
            if self.html_named(target, N::Template) {
                return InsertionPoint::LastChild(self.template_contents(target));
            }
            return InsertionPoint::LastChild(target);
        }
        for (index, &element) in self.open.iter().enumerate().rev() {
            if self.html_named(element, N::Template) {
                return InsertionPoint::LastChild(self.template_contents(element));
            }
            if self.html_named(element, N::Table) {
                return InsertionPoint::FosterParent {
                    table: element,
                    previous: self.open[index - 1],
                };
            }
        }
        InsertionPoint::LastChild(self.open[0])
    }

    fn insert_at(&mut self, point: InsertionPoint, child: Child<'a>) {
        let (parent, before) = match point {
            InsertionPoint::LastChild(parent) => (parent, None),
            InsertionPoint::FosterParent { table, previous } => {
                if self.document[table].parent.is_some() {
                    (table, Some(table))
                } else {
                    (previous, None)
                }
            }
        };
        match (child, before) {
            (Child::Node(node), None) => self.document.append(parent, node),
            (Child::Text(text), None) => self.document.append_text(parent, text),
            (Child::Node(node), Some(sibling)) => self.document.insert_before(sibling, node),
            (Child::Text(text), Some(sibling)) => self.document.insert_text_before(sibling, text),
        }
    }

    fn insert_appropriately(&mut self, child: Child<'a>, override_target: Option<NodeId>) {
        let point = self.appropriate_place_for_insertion(override_target);
        self.insert_at(point, child);
    }

    fn create_root(&mut self, attributes: Vec<Attribute<'a>>) {
        let root = self.document.create_element(
            Namespace::Html,
            N::Html,
            Cow::Borrowed("html"),
            attributes,
        );
        self.open.push(root);
        self.document.append(ROOT, root);
    }

    fn insert_element(
        &mut self,
        push: bool,
        namespace: Namespace,
        local: N,
        name: Cow<'a, str>,
        attributes: Vec<Attribute<'a>>,
    ) -> NodeId {
        let element = self
            .document
            .create_element(namespace, local, name, attributes);
        self.insert_appropriately(Child::Node(element), None);
        if push {
            self.open.push(element);
        }
        element
    }

    fn insert_element_for(&mut self, tag: Tag<'a>) -> NodeId {
        self.insert_element(true, Namespace::Html, tag.local, tag.name, tag.attributes)
    }

    fn insert_and_pop_element_for(&mut self, tag: Tag<'a>) -> NodeId {
        self.insert_element(false, Namespace::Html, tag.local, tag.name, tag.attributes)
    }

    fn insert_phantom(&mut self, local: N) -> NodeId {
        self.insert_element(
            true,
            Namespace::Html,
            local,
            Cow::Borrowed(local.text()),
            Vec::new(),
        )
    }

    fn append_text(&mut self, text: Cow<'a, str>) -> Step<'a> {
        self.insert_appropriately(Child::Text(text), None);
        Step::Done
    }

    fn append_comment(&mut self, text: Cow<'a, str>) -> Step<'a> {
        let comment = self.document.create(super::Data::Comment(text));
        self.insert_appropriately(Child::Node(comment), None);
        Step::Done
    }

    fn append_comment_to(&mut self, parent: NodeId, text: Cow<'a, str>) -> Step<'a> {
        let comment = self.document.create(super::Data::Comment(text));
        self.document.append(parent, comment);
        Step::Done
    }

    fn parse_raw_data(&mut self, tag: Tag<'a>, kind: RawKind) -> Step<'a> {
        self.insert_element_for(tag);
        self.original_mode = Some(self.mode);
        self.mode = Mode::Text;
        self.tokenizer.raw(kind);
        Step::Done
    }

    fn pop(&mut self) {
        self.open.pop().expect("no current element");
    }

    fn remove_from_stack(&mut self, element: NodeId) {
        if let Some(position) = self.open.iter().rposition(|&node| node == element) {
            self.open.remove(position);
        }
    }

    fn generate_implied_end_tags(&mut self, set: impl Fn(Namespace, N) -> bool) {
        while let Some(&node) = self.open.last() {
            let (namespace, name) = self.name(node);
            if !set(namespace, name) {
                return;
            }
            self.pop();
        }
    }

    fn generate_implied_end_except(&mut self, except: N) {
        self.generate_implied_end_tags(|namespace, name| {
            !(namespace == Namespace::Html && name == except)
                && cursory_implied_end(namespace, name)
        });
    }

    /// Pops elements until the current one is in `set`.
    fn pop_until_current(&mut self, set: fn(Namespace, N) -> bool) {
        while !self.current_in(set) {
            self.pop();
        }
    }

    /// Pops elements until one in `set` has been popped.
    fn pop_until(&mut self, set: impl Fn(Namespace, N) -> bool) {
        while let Some(node) = self.open.pop() {
            let (namespace, name) = self.name(node);
            if set(namespace, name) {
                return;
            }
        }
    }

    fn pop_until_named(&mut self, name: N) {
        self.pop_until(|namespace, element| namespace == Namespace::Html && element == name);
    }

    fn close_p_element(&mut self) {
        self.generate_implied_end_tags(|namespace, name| {
            name != N::P && cursory_implied_end(namespace, name)
        });
        self.pop_until_named(N::P);
    }

    fn close_p_element_in_button_scope(&mut self) {
        if self.in_scope_named(button_scope, N::P) {
            self.close_p_element();
        }
    }

    fn close_the_cell(&mut self) {
        self.generate_implied_end_tags(cursory_implied_end);
        self.pop_until(td_th);
        self.clear_active_formatting_to_marker();
    }

    fn reset_insertion_mode(&self) -> Mode {
        for (index, &node) in self.open.iter().enumerate().rev() {
            let last = index == 0;
            let node = match self.context {
                Some(context) if last => context,
                _ => node,
            };
            let (namespace, name) = self.name(node);
            if namespace != Namespace::Html {
                continue;
            }
            match name {
                N::Td | N::Th if !last => return Mode::InCell,
                N::Tr => return Mode::InRow,
                N::Tbody | N::Thead | N::Tfoot => return Mode::InTableBody,
                N::Caption => return Mode::InCaption,
                N::Colgroup => return Mode::InColumnGroup,
                N::Table => return Mode::InTable,
                N::Template => {
                    return *self
                        .template_modes
                        .last()
                        .expect("open templates have a template mode");
                }
                N::Head if !last => return Mode::InHead,
                N::Body => return Mode::InBody,
                N::Frameset => return Mode::InFrameset,
                N::Html if self.head.is_none() => return Mode::BeforeHead,
                N::Html => return Mode::AfterHead,
                _ => {}
            }
        }
        Mode::InBody
    }

    fn foster_parent_in_body(&mut self, token: Token<'a>) -> Step<'a> {
        self.foster_parenting = true;
        let step = self.step(Mode::InBody, token);
        self.foster_parenting = false;
        step
    }

    fn process_chars_in_table(&mut self, token: Token<'a>) -> Step<'a> {
        if self.current_in(|namespace, name| {
            html(
                namespace,
                name,
                &[N::Table, N::Tbody, N::Tfoot, N::Thead, N::Tr],
            )
        }) {
            self.original_mode = Some(self.mode);
            Step::Reprocess(Mode::InTableText, token)
        } else {
            self.foster_parent_in_body(token)
        }
    }

    fn is_type_hidden(tag: &Tag<'_>) -> bool {
        tag.attributes
            .iter()
            .find(|attribute| attribute.name == "type")
            .is_some_and(|attribute| attribute.value.eq_ignore_ascii_case("hidden"))
    }

    fn position_in_formatting(&self, element: NodeId) -> Option<usize> {
        self.formatting
            .iter()
            .position(|entry| matches!(entry, Formatting::Element(node, _) if *node == element))
    }

    /// Formatting entries after the last marker, latest first.
    fn formatting_to_marker(&self) -> impl Iterator<Item = (usize, NodeId, &Tag<'a>)> {
        self.formatting
            .iter()
            .enumerate()
            .rev()
            .map_while(|(index, entry)| match entry {
                Formatting::Marker => None,
                Formatting::Element(node, tag) => Some((index, *node, tag)),
            })
    }

    fn clear_active_formatting_to_marker(&mut self) {
        while let Some(entry) = self.formatting.pop() {
            if matches!(entry, Formatting::Marker) {
                return;
            }
        }
    }

    fn is_marker_or_open(&self, entry: &Formatting<'a>) -> bool {
        match entry {
            Formatting::Marker => true,
            Formatting::Element(node, _) => self.open.contains(node),
        }
    }

    fn reconstruct_active_formatting_elements(&mut self) {
        match self.formatting.last() {
            Some(last) if !self.is_marker_or_open(last) => {}
            _ => return,
        }
        let mut index = self.formatting.len() - 1;
        while index > 0 {
            index -= 1;
            if self.is_marker_or_open(&self.formatting[index]) {
                index += 1;
                break;
            }
        }
        loop {
            let Formatting::Element(_, tag) = &self.formatting[index] else {
                unreachable!("reconstruction starts after the last marker");
            };
            let tag = tag.clone();
            let element = self.insert_element(
                true,
                Namespace::Html,
                tag.local,
                tag.name.clone(),
                tag.attributes.clone(),
            );
            self.formatting[index] = Formatting::Element(element, tag);
            if index == self.formatting.len() - 1 {
                return;
            }
            index += 1;
        }
    }

    fn create_formatting_element_for(&mut self, tag: Tag<'a>) {
        let mut first_match = None;
        let mut matches = 0;
        for (index, _, old) in self.formatting_to_marker() {
            if equivalent(&tag, old) {
                first_match = Some(index);
                matches += 1;
            }
        }
        if matches >= 3 {
            self.formatting
                .remove(first_match.expect("matches have an index"));
        }
        let element = self.insert_element(
            true,
            Namespace::Html,
            tag.local,
            tag.name.clone(),
            tag.attributes.clone(),
        );
        self.formatting.push(Formatting::Element(element, tag));
    }

    fn process_end_tag_in_body(&mut self, tag: &Tag<'_>) {
        let mut matched = None;
        for (index, &node) in self.open.iter().enumerate().rev() {
            if self.html_named(node, tag.local)
                && (tag.local != N::Other || self.document.element(node).name == tag.name)
            {
                matched = Some(index);
                break;
            }
            if self.in_set(node, special) {
                return;
            }
        }
        let Some(index) = matched else {
            return;
        };
        self.generate_implied_end_except(tag.local);
        self.open.truncate(index);
    }

    fn handle_misnested_a_tags(&mut self, tag: &Tag<'_>) {
        let Some(node) = self
            .formatting_to_marker()
            .find(|&(_, node, _)| self.html_named(node, N::A))
            .map(|(_, node, _)| node)
        else {
            return;
        };
        self.adoption_agency(tag);
        if let Some(index) = self.position_in_formatting(node) {
            self.formatting.remove(index);
        }
        self.remove_from_stack(node);
    }

    /// Runs for the name of `subject`, which is always a formatting element's.
    fn adoption_agency(&mut self, subject: &Tag<'_>) {
        if self.current_named(subject.local)
            && self.position_in_formatting(self.current_node()).is_none()
        {
            self.pop();
            return;
        }
        for _ in 0..8 {
            let Some((formatting_index, element, tag)) = self
                .formatting_to_marker()
                .find(|(_, _, tag)| tag.local == subject.local)
                .map(|(index, node, tag)| (index, node, tag.clone()))
            else {
                return self.process_end_tag_in_body(subject);
            };
            let Some(stack_index) = self.open.iter().rposition(|&node| node == element) else {
                self.formatting.remove(formatting_index);
                return;
            };
            if !self.in_scope(default_scope, |node| node == element) {
                return;
            }
            let Some((furthest_index, furthest_block)) = self.open[stack_index..]
                .iter()
                .position(|&node| self.in_set(node, special))
                .map(|offset| (stack_index + offset, self.open[stack_index + offset]))
            else {
                self.open.truncate(stack_index);
                self.formatting.remove(formatting_index);
                return;
            };
            let common_ancestor = self.open[stack_index - 1];
            let mut bookmark = Bookmark::Replace(element);
            let mut node_index = furthest_index;
            let mut last_node = furthest_block;
            let mut inner = 0;
            loop {
                inner += 1;
                node_index -= 1;
                let node = self.open[node_index];
                if node == element {
                    break;
                }
                if inner > 3 {
                    if let Some(position) = self.position_in_formatting(node) {
                        self.formatting.remove(position);
                    }
                    self.open.remove(node_index);
                    continue;
                }
                let Some(node_formatting_index) = self.position_in_formatting(node) else {
                    self.open.remove(node_index);
                    continue;
                };
                let Formatting::Element(_, node_tag) = &self.formatting[node_formatting_index]
                else {
                    unreachable!("formatting positions point to elements");
                };
                let node_tag = node_tag.clone();
                let replacement = self.document.create_element(
                    Namespace::Html,
                    node_tag.local,
                    node_tag.name.clone(),
                    node_tag.attributes.clone(),
                );
                self.open[node_index] = replacement;
                self.formatting[node_formatting_index] = Formatting::Element(replacement, node_tag);
                if last_node == furthest_block {
                    bookmark = Bookmark::InsertAfter(replacement);
                }
                self.document.detach(last_node);
                self.document.append(replacement, last_node);
                last_node = replacement;
            }
            self.document.detach(last_node);
            self.insert_appropriately(Child::Node(last_node), Some(common_ancestor));
            let new_element = self.document.create_element(
                Namespace::Html,
                tag.local,
                tag.name.clone(),
                tag.attributes.clone(),
            );
            let entry = Formatting::Element(new_element, tag);
            self.document.reparent_children(furthest_block, new_element);
            self.document.append(furthest_block, new_element);
            match bookmark {
                Bookmark::Replace(replaced) => {
                    let index = self
                        .position_in_formatting(replaced)
                        .expect("bookmark is in the formatting list");
                    self.formatting[index] = entry;
                }
                Bookmark::InsertAfter(previous) => {
                    let index = self
                        .position_in_formatting(previous)
                        .expect("bookmark is in the formatting list");
                    self.formatting.insert(index + 1, entry);
                    let old = self
                        .position_in_formatting(element)
                        .expect("formatting element is in the formatting list");
                    self.formatting.remove(old);
                }
            }
            self.remove_from_stack(element);
            let furthest = self
                .open
                .iter()
                .position(|&node| node == furthest_block)
                .expect("furthest block stays open");
            self.open.insert(furthest + 1, new_element);
        }
    }
}
