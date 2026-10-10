//! Tree construction, ported rule for rule from html5ever 0.39 as scraper drives it.
mod foreign;
mod rules;

use super::{
    Attribute, Document, Namespace, NodeId, ROOT,
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

fn html(namespace: Namespace, name: &str, names: &[&str]) -> bool {
    namespace == Namespace::Html && names.contains(&name)
}

fn mathml_text_integration_point(namespace: Namespace, name: &str) -> bool {
    namespace == Namespace::MathMl && matches!(name, "mi" | "mo" | "mn" | "ms" | "mtext")
}

fn svg_html_integration_point(namespace: Namespace, name: &str) -> bool {
    namespace == Namespace::Svg && matches!(name, "foreignObject" | "desc" | "title")
}

fn default_scope(namespace: Namespace, name: &str) -> bool {
    (namespace == Namespace::Html
        && matches!(
            name,
            "applet"
                | "caption"
                | "html"
                | "table"
                | "td"
                | "th"
                | "marquee"
                | "object"
                | "select"
                | "template"
        ))
        || mathml_text_integration_point(namespace, name)
        || svg_html_integration_point(namespace, name)
}

fn list_item_scope(namespace: Namespace, name: &str) -> bool {
    default_scope(namespace, name) || html(namespace, name, &["ol", "ul"])
}

fn button_scope(namespace: Namespace, name: &str) -> bool {
    default_scope(namespace, name) || html(namespace, name, &["button"])
}

fn table_scope(namespace: Namespace, name: &str) -> bool {
    html(namespace, name, &["html", "table", "template"])
}

fn table_body_context(namespace: Namespace, name: &str) -> bool {
    html(
        namespace,
        name,
        &["tbody", "tfoot", "thead", "template", "html"],
    )
}

fn table_row_context(namespace: Namespace, name: &str) -> bool {
    html(namespace, name, &["tr", "template", "html"])
}

fn td_th(namespace: Namespace, name: &str) -> bool {
    html(namespace, name, &["td", "th"])
}

fn heading(namespace: Namespace, name: &str) -> bool {
    html(namespace, name, &["h1", "h2", "h3", "h4", "h5", "h6"])
}

fn cursory_implied_end(namespace: Namespace, name: &str) -> bool {
    namespace == Namespace::Html
        && matches!(
            name,
            "dd" | "dt" | "li" | "option" | "optgroup" | "p" | "rb" | "rp" | "rt" | "rtc"
        )
}

fn thorough_implied_end(namespace: Namespace, name: &str) -> bool {
    cursory_implied_end(namespace, name)
        || html(
            namespace,
            name,
            &[
                "caption", "colgroup", "tbody", "td", "tfoot", "th", "thead", "tr",
            ],
        )
}

fn special(namespace: Namespace, name: &str) -> bool {
    namespace == Namespace::Html
        && matches!(
            name,
            "address"
                | "applet"
                | "area"
                | "article"
                | "aside"
                | "base"
                | "basefont"
                | "bgsound"
                | "blockquote"
                | "body"
                | "br"
                | "button"
                | "caption"
                | "center"
                | "col"
                | "colgroup"
                | "dd"
                | "details"
                | "dir"
                | "div"
                | "dl"
                | "dt"
                | "embed"
                | "fieldset"
                | "figcaption"
                | "figure"
                | "footer"
                | "form"
                | "frame"
                | "frameset"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "head"
                | "header"
                | "hgroup"
                | "hr"
                | "html"
                | "iframe"
                | "img"
                | "input"
                | "isindex"
                | "li"
                | "link"
                | "listing"
                | "main"
                | "marquee"
                | "menu"
                | "meta"
                | "nav"
                | "noembed"
                | "noframes"
                | "noscript"
                | "object"
                | "ol"
                | "p"
                | "param"
                | "plaintext"
                | "pre"
                | "script"
                | "section"
                | "select"
                | "source"
                | "style"
                | "summary"
                | "table"
                | "tbody"
                | "td"
                | "template"
                | "textarea"
                | "tfoot"
                | "th"
                | "thead"
                | "title"
                | "tr"
                | "track"
                | "ul"
                | "wbr"
                | "xmp"
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
            document: Document::new(),
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
        let context =
            builder
                .document
                .create_element(Namespace::Html, Cow::Borrowed("body"), Vec::new());
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
                Token::Tag(tag) => tag.end || matches!(&*tag.name, "mglyph" | "malignmark"),
                _ => true,
            },
            _ if svg_html_integration_point(namespace, name) => {
                !matches!(token, Token::Characters(..) | Token::NullCharacter) && !start(token)
            }
            Namespace::MathMl if name == "annotation-xml" => {
                !matches!(token, Token::Tag(tag) if !tag.end && tag.name == "svg")
            }
            _ => true,
        }
    }

    fn name(&self, node: NodeId) -> (Namespace, &str) {
        let element = self.document.element(node);
        (element.namespace, &element.name)
    }

    fn namespace(&self, node: NodeId) -> Namespace {
        self.document.element(node).namespace
    }

    fn html_named(&self, node: NodeId, name: &str) -> bool {
        self.name(node) == (Namespace::Html, name)
    }

    fn in_set(&self, node: NodeId, set: fn(Namespace, &str) -> bool) -> bool {
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

    fn current_named(&self, name: &str) -> bool {
        self.html_named(self.current_node(), name)
    }

    fn current_in(&self, set: fn(Namespace, &str) -> bool) -> bool {
        self.in_set(self.current_node(), set)
    }

    fn in_html_named(&self, name: &str) -> bool {
        self.open.iter().any(|&node| self.html_named(node, name))
    }

    fn in_scope(
        &self,
        scope: fn(Namespace, &str) -> bool,
        matches: impl Fn(NodeId) -> bool,
    ) -> bool {
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

    fn in_scope_named(&self, scope: fn(Namespace, &str) -> bool, name: &str) -> bool {
        self.in_scope(scope, |node| self.html_named(node, name))
    }

    fn body_element(&self) -> Option<NodeId> {
        self.open
            .get(1)
            .copied()
            .filter(|&node| self.html_named(node, "body"))
    }

    fn template_contents(&self, template: NodeId) -> NodeId {
        self.document.nodes[template.0]
            .first_child
            .expect("templates keep their contents as the first child")
    }

    fn appropriate_place_for_insertion(&self, override_target: Option<NodeId>) -> InsertionPoint {
        let target = override_target.unwrap_or_else(|| self.current_node());
        let foster = |namespace, name: &str| {
            html(namespace, name, &["table", "tbody", "tfoot", "thead", "tr"])
        };
        if !(self.foster_parenting && self.in_set(target, foster)) {
            if self.html_named(target, "template") {
                return InsertionPoint::LastChild(self.template_contents(target));
            }
            return InsertionPoint::LastChild(target);
        }
        for (index, &element) in self.open.iter().enumerate().rev() {
            if self.html_named(element, "template") {
                return InsertionPoint::LastChild(self.template_contents(element));
            }
            if self.html_named(element, "table") {
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
                if self.document.nodes[table.0].parent.is_some() {
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
        let root = self
            .document
            .create_element(Namespace::Html, Cow::Borrowed("html"), attributes);
        self.open.push(root);
        self.document.append(ROOT, root);
    }

    fn insert_element(
        &mut self,
        push: bool,
        namespace: Namespace,
        name: Cow<'a, str>,
        attributes: Vec<Attribute<'a>>,
    ) -> NodeId {
        let element = self.document.create_element(namespace, name, attributes);
        self.insert_appropriately(Child::Node(element), None);
        if push {
            self.open.push(element);
        }
        element
    }

    fn insert_element_for(&mut self, tag: Tag<'a>) -> NodeId {
        self.insert_element(true, Namespace::Html, tag.name, tag.attributes)
    }

    fn insert_and_pop_element_for(&mut self, tag: Tag<'a>) -> NodeId {
        self.insert_element(false, Namespace::Html, tag.name, tag.attributes)
    }

    fn insert_phantom(&mut self, name: &'static str) -> NodeId {
        self.insert_element(true, Namespace::Html, Cow::Borrowed(name), Vec::new())
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

    fn generate_implied_end_tags(&mut self, set: impl Fn(Namespace, &str) -> bool) {
        while let Some(&node) = self.open.last() {
            let (namespace, name) = self.name(node);
            if !set(namespace, name) {
                return;
            }
            self.pop();
        }
    }

    fn generate_implied_end_except(&mut self, except: &str) {
        self.generate_implied_end_tags(|namespace, name| {
            !(namespace == Namespace::Html && name == except)
                && cursory_implied_end(namespace, name)
        });
    }

    /// Pops elements until the current one is in `set`.
    fn pop_until_current(&mut self, set: fn(Namespace, &str) -> bool) {
        while !self.current_in(set) {
            self.pop();
        }
    }

    /// Pops elements until one in `set` has been popped.
    fn pop_until(&mut self, set: impl Fn(Namespace, &str) -> bool) {
        while let Some(node) = self.open.pop() {
            let (namespace, name) = self.name(node);
            if set(namespace, name) {
                return;
            }
        }
    }

    fn pop_until_named(&mut self, name: &str) {
        self.pop_until(|namespace, element| namespace == Namespace::Html && element == name);
    }

    fn close_p_element(&mut self) {
        self.generate_implied_end_tags(|namespace, name| {
            name != "p" && cursory_implied_end(namespace, name)
        });
        self.pop_until_named("p");
    }

    fn close_p_element_in_button_scope(&mut self) {
        if self.in_scope_named(button_scope, "p") {
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
                "td" | "th" if !last => return Mode::InCell,
                "tr" => return Mode::InRow,
                "tbody" | "thead" | "tfoot" => return Mode::InTableBody,
                "caption" => return Mode::InCaption,
                "colgroup" => return Mode::InColumnGroup,
                "table" => return Mode::InTable,
                "template" => {
                    return *self
                        .template_modes
                        .last()
                        .expect("open templates have a template mode");
                }
                "head" if !last => return Mode::InHead,
                "body" => return Mode::InBody,
                "frameset" => return Mode::InFrameset,
                "html" if self.head.is_none() => return Mode::BeforeHead,
                "html" => return Mode::AfterHead,
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
            html(namespace, name, &["table", "tbody", "tfoot", "thead", "tr"])
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
            tag.name.clone(),
            tag.attributes.clone(),
        );
        self.formatting.push(Formatting::Element(element, tag));
    }

    fn process_end_tag_in_body(&mut self, name: &str) {
        let mut matched = None;
        for (index, &node) in self.open.iter().enumerate().rev() {
            if self.html_named(node, name) {
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
        self.generate_implied_end_except(name);
        self.open.truncate(index);
    }

    fn handle_misnested_a_tags(&mut self) {
        let Some(node) = self
            .formatting_to_marker()
            .find(|&(_, node, _)| self.html_named(node, "a"))
            .map(|(_, node, _)| node)
        else {
            return;
        };
        self.adoption_agency("a");
        if let Some(index) = self.position_in_formatting(node) {
            self.formatting.remove(index);
        }
        self.remove_from_stack(node);
    }

    fn adoption_agency(&mut self, subject: &str) {
        if self.current_named(subject) && self.position_in_formatting(self.current_node()).is_none()
        {
            self.pop();
            return;
        }
        for _ in 0..8 {
            let Some((formatting_index, element, tag)) = self
                .formatting_to_marker()
                .find(|(_, _, tag)| tag.name == subject)
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
