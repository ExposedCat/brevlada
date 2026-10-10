//! The insertion modes and the rules for foreign content.
use super::{
    Formatting, Mode, SplitStatus, Step, Token, TreeBuilder, any_not_whitespace, button_scope,
    cursory_implied_end, default_scope, foreign, heading, html, list_item_scope,
    mathml_text_integration_point, special, svg_html_integration_point, table_body_context,
    table_row_context, table_scope, td_th, thorough_implied_end,
};
use crate::models::dom::{
    Namespace, ROOT,
    names::LocalName as N,
    tokenizer::{RawKind, Tag},
};
use std::borrow::Cow;

impl<'a> TreeBuilder<'a> {
    pub(super) fn step(&mut self, mode: Mode, token: Token<'a>) -> Step<'a> {
        match mode {
            Mode::Initial => self.initial(token),
            Mode::BeforeHtml => self.before_html(token),
            Mode::BeforeHead => self.before_head(token),
            Mode::InHead => self.in_head(token),
            Mode::AfterHead => self.after_head(token),
            Mode::InBody => self.in_body(token),
            Mode::Text => self.text(token),
            Mode::InTable => self.in_table(token),
            Mode::InTableText => self.in_table_text(token),
            Mode::InCaption => self.in_caption(token),
            Mode::InColumnGroup => self.in_column_group(token),
            Mode::InTableBody => self.in_table_body(token),
            Mode::InRow => self.in_row(token),
            Mode::InCell => self.in_cell(token),
            Mode::InTemplate => self.in_template(token),
            Mode::AfterBody => self.after_body(token),
            Mode::InFrameset => self.in_frameset(token),
            Mode::AfterFrameset => self.after_frameset(token),
            Mode::AfterAfterBody => self.after_after_body(token),
            Mode::AfterAfterFrameset => self.after_after_frameset(token),
        }
    }

    fn initial(&mut self, token: Token<'a>) -> Step<'a> {
        match token {
            Token::Characters(SplitStatus::NotSplit, text) => Step::SplitWhitespace(text),
            Token::Characters(SplitStatus::Whitespace, _) => Step::Done,
            Token::Comment(text) => self.append_comment_to(ROOT, text),
            token => {
                self.quirks = true;
                Step::Reprocess(Mode::BeforeHtml, token)
            }
        }
    }

    fn before_html(&mut self, token: Token<'a>) -> Step<'a> {
        match token {
            Token::Comment(text) => self.append_comment_to(ROOT, text),
            Token::Characters(SplitStatus::NotSplit, text) => Step::SplitWhitespace(text),
            Token::Characters(SplitStatus::Whitespace, _) => Step::Done,
            Token::Tag(tag) if !tag.end && tag.local == N::Html => {
                self.create_root(tag.attributes);
                self.mode = Mode::BeforeHead;
                Step::Done
            }
            Token::Tag(tag)
                if tag.end && !matches!(tag.local, N::Head | N::Body | N::Html | N::Br) =>
            {
                Step::Done
            }
            token => {
                self.create_root(Vec::new());
                Step::Reprocess(Mode::BeforeHead, token)
            }
        }
    }

    fn before_head(&mut self, token: Token<'a>) -> Step<'a> {
        match token {
            Token::Characters(SplitStatus::NotSplit, text) => Step::SplitWhitespace(text),
            Token::Characters(SplitStatus::Whitespace, _) => Step::Done,
            Token::Comment(text) => self.append_comment(text),
            Token::Tag(tag) if !tag.end && tag.local == N::Html => self.in_body(Token::Tag(tag)),
            Token::Tag(tag) if !tag.end && tag.local == N::Head => {
                self.head = Some(self.insert_element_for(tag));
                self.mode = Mode::InHead;
                Step::Done
            }
            Token::Tag(tag)
                if tag.end && !matches!(tag.local, N::Head | N::Body | N::Html | N::Br) =>
            {
                Step::Done
            }
            token => {
                self.head = Some(self.insert_phantom(N::Head));
                Step::Reprocess(Mode::InHead, token)
            }
        }
    }

    fn in_head(&mut self, token: Token<'a>) -> Step<'a> {
        let tag = match token {
            Token::Characters(SplitStatus::NotSplit, text) => return Step::SplitWhitespace(text),
            Token::Characters(SplitStatus::Whitespace, text) => return self.append_text(text),
            Token::Comment(text) => return self.append_comment(text),
            Token::Tag(tag) => tag,
            token => {
                self.pop();
                return Step::Reprocess(Mode::AfterHead, token);
            }
        };
        match (tag.end, tag.local) {
            (false, N::Html) => self.in_body(Token::Tag(tag)),
            (false, N::Base | N::Basefont | N::Bgsound | N::Link | N::Meta) => {
                self.insert_and_pop_element_for(tag);
                Step::Done
            }
            (false, N::Title) => self.parse_raw_data(tag, RawKind::Rcdata),
            (false, N::Noframes | N::Style | N::Noscript) => {
                self.parse_raw_data(tag, RawKind::Rawtext)
            }
            (false, N::Script) => self.parse_raw_data(tag, RawKind::ScriptData),
            (true, N::Head) => {
                self.pop();
                self.mode = Mode::AfterHead;
                Step::Done
            }
            (false, N::Template) => {
                self.formatting.push(Formatting::Marker);
                self.frameset_ok = false;
                self.mode = Mode::InTemplate;
                self.template_modes.push(Mode::InTemplate);
                self.insert_element_for(tag);
                Step::Done
            }
            (true, N::Template) => {
                if self.in_html_named(N::Template) {
                    self.generate_implied_end_tags(thorough_implied_end);
                    self.pop_until_named(N::Template);
                    self.clear_active_formatting_to_marker();
                    self.template_modes.pop();
                    self.mode = self.reset_insertion_mode();
                }
                Step::Done
            }
            (true, N::Body | N::Html | N::Br) | (false, _) if tag.local != N::Head => {
                self.pop();
                Step::Reprocess(Mode::AfterHead, Token::Tag(tag))
            }
            _ => Step::Done,
        }
    }

    fn after_head(&mut self, token: Token<'a>) -> Step<'a> {
        let tag = match token {
            Token::Characters(SplitStatus::NotSplit, text) => return Step::SplitWhitespace(text),
            Token::Characters(SplitStatus::Whitespace, text) => return self.append_text(text),
            Token::Comment(text) => return self.append_comment(text),
            Token::Tag(tag) => tag,
            token => {
                self.insert_phantom(N::Body);
                return Step::Reprocess(Mode::InBody, token);
            }
        };
        match (tag.end, tag.local) {
            (false, N::Html) => self.in_body(Token::Tag(tag)),
            (false, N::Body) => {
                self.insert_element_for(tag);
                self.frameset_ok = false;
                self.mode = Mode::InBody;
                Step::Done
            }
            (false, N::Frameset) => {
                self.insert_element_for(tag);
                self.mode = Mode::InFrameset;
                Step::Done
            }
            (
                false,
                N::Base
                | N::Basefont
                | N::Bgsound
                | N::Link
                | N::Meta
                | N::Noframes
                | N::Script
                | N::Style
                | N::Template
                | N::Title,
            ) => {
                let head = self.head.expect("the head element exists after the head");
                self.open.push(head);
                let step = self.in_head(Token::Tag(tag));
                self.remove_from_stack(head);
                step
            }
            (true, N::Template) => self.in_head(Token::Tag(tag)),
            (true, N::Body | N::Html | N::Br) | (false, _) if tag.local != N::Head => {
                self.insert_phantom(N::Body);
                Step::Reprocess(Mode::InBody, Token::Tag(tag))
            }
            _ => Step::Done,
        }
    }

    fn in_body(&mut self, token: Token<'a>) -> Step<'a> {
        match token {
            Token::NullCharacter => Step::Done,
            Token::Characters(_, text) => {
                self.reconstruct_active_formatting_elements();
                if any_not_whitespace(&text) {
                    self.frameset_ok = false;
                }
                self.append_text(text)
            }
            Token::Comment(text) => self.append_comment(text),
            Token::Eof if !self.template_modes.is_empty() => self.in_template(Token::Eof),
            Token::Eof => Step::Done,
            Token::Tag(tag) if tag.end => self.in_body_end_tag(tag),
            Token::Tag(tag) => self.in_body_start_tag(tag),
        }
    }

    fn in_body_start_tag(&mut self, tag: Tag<'a>) -> Step<'a> {
        match tag.local {
            N::Html => {
                if !self.in_html_named(N::Template) {
                    self.document
                        .add_attributes_if_missing(self.open[0], tag.attributes);
                }
            }
            N::Base
            | N::Basefont
            | N::Bgsound
            | N::Link
            | N::Meta
            | N::Noframes
            | N::Script
            | N::Style
            | N::Template
            | N::Title => return self.in_head(Token::Tag(tag)),
            N::Body => {
                if let Some(body) = self.body_element()
                    && !self.in_html_named(N::Template)
                {
                    self.frameset_ok = false;
                    self.document
                        .add_attributes_if_missing(body, tag.attributes);
                }
            }
            N::Frameset => {
                if self.frameset_ok
                    && let Some(body) = self.body_element()
                {
                    self.document.detach(body);
                    self.open.truncate(1);
                    self.insert_element_for(tag);
                    self.mode = Mode::InFrameset;
                }
            }
            N::Address
            | N::Article
            | N::Aside
            | N::Blockquote
            | N::Center
            | N::Details
            | N::Dialog
            | N::Dir
            | N::Div
            | N::Dl
            | N::Fieldset
            | N::Figcaption
            | N::Figure
            | N::Footer
            | N::Header
            | N::Hgroup
            | N::Main
            | N::Menu
            | N::Nav
            | N::Ol
            | N::P
            | N::Search
            | N::Section
            | N::Summary
            | N::Ul => {
                self.close_p_element_in_button_scope();
                self.insert_element_for(tag);
            }
            N::H1 | N::H2 | N::H3 | N::H4 | N::H5 | N::H6 => {
                self.close_p_element_in_button_scope();
                if self.current_in(heading) {
                    self.pop();
                }
                self.insert_element_for(tag);
            }
            N::Pre | N::Listing => {
                self.close_p_element_in_button_scope();
                self.insert_element_for(tag);
                self.ignore_lf = true;
                self.frameset_ok = false;
            }
            N::Form => {
                if self.form.is_none() || self.in_html_named(N::Template) {
                    self.close_p_element_in_button_scope();
                    let form = self.insert_element_for(tag);
                    if !self.in_html_named(N::Template) {
                        self.form = Some(form);
                    }
                }
            }
            N::Li | N::Dd | N::Dt => {
                let list = tag.local == N::Li;
                self.frameset_ok = false;
                let mut close = None;
                for &node in self.open.iter().rev() {
                    let (namespace, name) = self.name(node);
                    let closes = if list {
                        html(namespace, name, &[N::Li])
                    } else {
                        html(namespace, name, &[N::Dd, N::Dt])
                    };
                    if closes {
                        close = Some(name);
                        break;
                    }
                    if special(namespace, name)
                        && !html(namespace, name, &[N::Address, N::Div, N::P])
                    {
                        break;
                    }
                }
                if let Some(name) = close {
                    self.generate_implied_end_except(name);
                    self.pop_until_named(name);
                }
                self.close_p_element_in_button_scope();
                self.insert_element_for(tag);
            }
            N::Plaintext => {
                self.close_p_element_in_button_scope();
                self.insert_element_for(tag);
                self.tokenizer.plaintext();
            }
            N::Button => {
                if self.in_scope_named(default_scope, N::Button) {
                    self.generate_implied_end_tags(cursory_implied_end);
                    self.pop_until_named(N::Button);
                }
                self.reconstruct_active_formatting_elements();
                self.insert_element_for(tag);
                self.frameset_ok = false;
            }
            N::A => {
                self.handle_misnested_a_tags(&tag);
                self.reconstruct_active_formatting_elements();
                self.create_formatting_element_for(tag);
            }
            N::B
            | N::Big
            | N::Code
            | N::Em
            | N::Font
            | N::I
            | N::S
            | N::Small
            | N::Strike
            | N::Strong
            | N::Tt
            | N::U => {
                self.reconstruct_active_formatting_elements();
                self.create_formatting_element_for(tag);
            }
            N::Nobr => {
                self.reconstruct_active_formatting_elements();
                if self.in_scope_named(default_scope, N::Nobr) {
                    self.adoption_agency(&tag);
                    self.reconstruct_active_formatting_elements();
                }
                self.create_formatting_element_for(tag);
            }
            N::Applet | N::Marquee | N::Object => {
                self.reconstruct_active_formatting_elements();
                self.insert_element_for(tag);
                self.formatting.push(Formatting::Marker);
                self.frameset_ok = false;
            }
            N::Table => {
                if !self.quirks {
                    self.close_p_element_in_button_scope();
                }
                self.insert_element_for(tag);
                self.frameset_ok = false;
                self.mode = Mode::InTable;
            }
            N::Area | N::Br | N::Embed | N::Img | N::Keygen | N::Wbr => {
                self.reconstruct_active_formatting_elements();
                self.insert_and_pop_element_for(tag);
                self.frameset_ok = false;
            }
            N::Input => {
                if self.in_scope_named(default_scope, N::Select) {
                    self.pop_until_named(N::Select);
                }
                let hidden = Self::is_type_hidden(&tag);
                self.reconstruct_active_formatting_elements();
                self.insert_and_pop_element_for(tag);
                if !hidden {
                    self.frameset_ok = false;
                }
            }
            N::Param | N::Source | N::Track => {
                self.insert_and_pop_element_for(tag);
            }
            N::Hr => {
                self.close_p_element_in_button_scope();
                if self.in_scope_named(default_scope, N::Select) {
                    self.generate_implied_end_tags(cursory_implied_end);
                }
                self.insert_and_pop_element_for(tag);
                self.frameset_ok = false;
            }
            N::Image => {
                return self.in_body(Token::Tag(Tag {
                    name: Cow::Borrowed("img"),
                    local: N::Img,
                    ..tag
                }));
            }
            N::Textarea => {
                self.ignore_lf = true;
                self.frameset_ok = false;
                return self.parse_raw_data(tag, RawKind::Rcdata);
            }
            N::Xmp => {
                self.close_p_element_in_button_scope();
                self.reconstruct_active_formatting_elements();
                self.frameset_ok = false;
                return self.parse_raw_data(tag, RawKind::Rawtext);
            }
            N::Iframe => {
                self.frameset_ok = false;
                return self.parse_raw_data(tag, RawKind::Rawtext);
            }
            N::Noembed | N::Noscript => return self.parse_raw_data(tag, RawKind::Rawtext),
            N::Select => {
                if self.in_scope_named(default_scope, N::Select) {
                    self.pop_until_named(N::Select);
                } else {
                    self.reconstruct_active_formatting_elements();
                    self.insert_element_for(tag);
                    self.frameset_ok = false;
                }
            }
            N::Option | N::Optgroup => {
                if self.in_scope_named(default_scope, N::Select) {
                    if tag.local == N::Option {
                        self.generate_implied_end_except(N::Optgroup);
                    } else {
                        self.generate_implied_end_tags(cursory_implied_end);
                    }
                } else if self.current_named(N::Option) {
                    self.pop();
                }
                self.reconstruct_active_formatting_elements();
                self.insert_element_for(tag);
            }
            N::Rb | N::Rtc => {
                if self.in_scope_named(default_scope, N::Ruby) {
                    self.generate_implied_end_tags(cursory_implied_end);
                }
                self.insert_element_for(tag);
            }
            N::Rp | N::Rt => {
                if self.in_scope_named(default_scope, N::Ruby) {
                    self.generate_implied_end_except(N::Rtc);
                }
                self.insert_element_for(tag);
            }
            N::Math => {
                self.reconstruct_active_formatting_elements();
                return self.enter_foreign(tag, Namespace::MathMl);
            }
            N::Svg => {
                self.reconstruct_active_formatting_elements();
                return self.enter_foreign(tag, Namespace::Svg);
            }
            N::Caption
            | N::Col
            | N::Colgroup
            | N::Frame
            | N::Head
            | N::Tbody
            | N::Td
            | N::Tfoot
            | N::Th
            | N::Thead
            | N::Tr => {}
            _ => {
                self.reconstruct_active_formatting_elements();
                self.insert_element_for(tag);
            }
        }
        Step::Done
    }

    fn in_body_end_tag(&mut self, tag: Tag<'a>) -> Step<'a> {
        match tag.local {
            N::Template => return self.in_head(Token::Tag(tag)),
            N::Body => {
                if self.in_scope_named(default_scope, N::Body) {
                    self.mode = Mode::AfterBody;
                }
            }
            N::Html => {
                if self.in_scope_named(default_scope, N::Body) {
                    return Step::Reprocess(Mode::AfterBody, Token::Tag(tag));
                }
            }
            N::Address
            | N::Article
            | N::Aside
            | N::Blockquote
            | N::Button
            | N::Center
            | N::Details
            | N::Dialog
            | N::Dir
            | N::Div
            | N::Dl
            | N::Fieldset
            | N::Figcaption
            | N::Figure
            | N::Footer
            | N::Header
            | N::Hgroup
            | N::Listing
            | N::Main
            | N::Menu
            | N::Nav
            | N::Ol
            | N::Pre
            | N::Search
            | N::Section
            | N::Select
            | N::Summary
            | N::Ul => {
                if self.in_scope_named(default_scope, tag.local) {
                    self.generate_implied_end_tags(cursory_implied_end);
                    self.pop_until_named(tag.local);
                }
            }
            N::Form if self.in_html_named(N::Template) => {
                if self.in_scope_named(default_scope, N::Form) {
                    self.generate_implied_end_tags(cursory_implied_end);
                    self.pop_until_named(N::Form);
                }
            }
            N::Form => {
                if let Some(form) = self.form.take()
                    && self.in_scope(default_scope, |node| node == form)
                {
                    self.generate_implied_end_tags(cursory_implied_end);
                    self.remove_from_stack(form);
                }
            }
            N::P => {
                if !self.in_scope_named(button_scope, N::P) {
                    self.insert_phantom(N::P);
                }
                self.close_p_element();
            }
            N::Li => {
                if self.in_scope_named(list_item_scope, N::Li) {
                    self.generate_implied_end_except(N::Li);
                    self.pop_until_named(N::Li);
                }
            }
            N::Dd | N::Dt => {
                if self.in_scope_named(default_scope, tag.local) {
                    self.generate_implied_end_except(tag.local);
                    self.pop_until_named(tag.local);
                }
            }
            N::H1 | N::H2 | N::H3 | N::H4 | N::H5 | N::H6 => {
                if self.in_scope(default_scope, |node| self.in_set(node, heading)) {
                    self.generate_implied_end_tags(cursory_implied_end);
                    self.pop_until(heading);
                }
            }
            N::A
            | N::B
            | N::Big
            | N::Code
            | N::Em
            | N::Font
            | N::I
            | N::Nobr
            | N::S
            | N::Small
            | N::Strike
            | N::Strong
            | N::Tt
            | N::U => self.adoption_agency(&tag),
            N::Applet | N::Marquee | N::Object => {
                if self.in_scope_named(default_scope, tag.local) {
                    self.generate_implied_end_tags(cursory_implied_end);
                    self.pop_until_named(tag.local);
                    self.clear_active_formatting_to_marker();
                }
            }
            N::Br => {
                return self.in_body(Token::Tag(Tag {
                    end: false,
                    attributes: Vec::new(),
                    ..tag
                }));
            }
            _ => self.process_end_tag_in_body(&tag),
        }
        Step::Done
    }

    fn text(&mut self, token: Token<'a>) -> Step<'a> {
        match token {
            Token::Characters(_, text) => self.append_text(text),
            Token::Eof => {
                self.pop();
                let mode = self
                    .original_mode
                    .take()
                    .expect("text mode remembers its origin");
                Step::Reprocess(mode, token)
            }
            Token::Tag(tag) if tag.end => {
                self.pop();
                self.mode = self
                    .original_mode
                    .take()
                    .expect("text mode remembers its origin");
                Step::Done
            }
            _ => unreachable!("raw text holds only characters and its end tag"),
        }
    }

    fn in_table(&mut self, token: Token<'a>) -> Step<'a> {
        let tag = match token {
            Token::NullCharacter | Token::Characters(..) => {
                return self.process_chars_in_table(token);
            }
            Token::Comment(text) => return self.append_comment(text),
            Token::Eof => return self.in_body(Token::Eof),
            Token::Tag(tag) => tag,
        };
        match (tag.end, tag.local) {
            (false, N::Caption) => {
                self.pop_until_current(table_scope);
                self.formatting.push(Formatting::Marker);
                self.insert_element_for(tag);
                self.mode = Mode::InCaption;
            }
            (false, N::Colgroup) => {
                self.pop_until_current(table_scope);
                self.insert_element_for(tag);
                self.mode = Mode::InColumnGroup;
            }
            (false, N::Col) => {
                self.pop_until_current(table_scope);
                self.insert_phantom(N::Colgroup);
                return Step::Reprocess(Mode::InColumnGroup, Token::Tag(tag));
            }
            (false, N::Tbody | N::Tfoot | N::Thead) => {
                self.pop_until_current(table_scope);
                self.insert_element_for(tag);
                self.mode = Mode::InTableBody;
            }
            (false, N::Td | N::Th | N::Tr) => {
                self.pop_until_current(table_scope);
                self.insert_phantom(N::Tbody);
                return Step::Reprocess(Mode::InTableBody, Token::Tag(tag));
            }
            (false, N::Table) => {
                if self.in_scope_named(table_scope, N::Table) {
                    self.pop_until_named(N::Table);
                    return Step::Reprocess(self.reset_insertion_mode(), Token::Tag(tag));
                }
            }
            (true, N::Table) => {
                if self.in_scope_named(table_scope, N::Table) {
                    self.pop_until_named(N::Table);
                    self.mode = self.reset_insertion_mode();
                }
            }
            (
                true,
                N::Body
                | N::Caption
                | N::Col
                | N::Colgroup
                | N::Html
                | N::Tbody
                | N::Td
                | N::Tfoot
                | N::Th
                | N::Thead
                | N::Tr,
            ) => {}
            (false, N::Style | N::Script | N::Template) | (true, N::Template) => {
                return self.in_head(Token::Tag(tag));
            }
            (false, N::Input) if Self::is_type_hidden(&tag) => {
                self.insert_and_pop_element_for(tag);
            }
            (false, N::Form) => {
                if !self.in_html_named(N::Template) && self.form.is_none() {
                    self.form = Some(self.insert_and_pop_element_for(tag));
                }
            }
            _ => return self.foster_parent_in_body(Token::Tag(tag)),
        }
        Step::Done
    }

    fn in_table_text(&mut self, token: Token<'a>) -> Step<'a> {
        match token {
            Token::NullCharacter => Step::Done,
            Token::Characters(split, text) => {
                self.pending_table_text.push((split, text));
                Step::Done
            }
            token => {
                let pending = std::mem::take(&mut self.pending_table_text);
                let not_space = pending.iter().any(|(split, text)| match split {
                    SplitStatus::Whitespace => false,
                    SplitStatus::NotWhitespace => true,
                    SplitStatus::NotSplit => any_not_whitespace(text),
                });
                for (split, text) in pending {
                    if not_space {
                        self.foster_parent_in_body(Token::Characters(split, text));
                    } else {
                        self.append_text(text);
                    }
                }
                let mode = self
                    .original_mode
                    .take()
                    .expect("table text remembers its origin");
                Step::Reprocess(mode, token)
            }
        }
    }

    fn in_caption(&mut self, token: Token<'a>) -> Step<'a> {
        let Token::Tag(tag) = token else {
            return self.in_body(token);
        };
        match (tag.end, tag.local) {
            (
                false,
                N::Caption
                | N::Col
                | N::Colgroup
                | N::Tbody
                | N::Td
                | N::Tfoot
                | N::Th
                | N::Thead
                | N::Tr,
            )
            | (true, N::Table | N::Caption) => {
                if !self.in_scope_named(table_scope, N::Caption) {
                    return Step::Done;
                }
                self.generate_implied_end_tags(cursory_implied_end);
                self.pop_until_named(N::Caption);
                self.clear_active_formatting_to_marker();
                if tag.end && tag.local == N::Caption {
                    self.mode = Mode::InTable;
                    Step::Done
                } else {
                    Step::Reprocess(Mode::InTable, Token::Tag(tag))
                }
            }
            (
                true,
                N::Body
                | N::Col
                | N::Colgroup
                | N::Html
                | N::Tbody
                | N::Td
                | N::Tfoot
                | N::Th
                | N::Thead
                | N::Tr,
            ) => Step::Done,
            _ => self.in_body(Token::Tag(tag)),
        }
    }

    fn in_column_group(&mut self, token: Token<'a>) -> Step<'a> {
        let token = match token {
            Token::Characters(SplitStatus::NotSplit, text) => return Step::SplitWhitespace(text),
            Token::Characters(SplitStatus::Whitespace, text) => return self.append_text(text),
            Token::Comment(text) => return self.append_comment(text),
            Token::Eof => return self.in_body(Token::Eof),
            Token::Tag(tag) => match (tag.end, tag.local) {
                (false, N::Html) => return self.in_body(Token::Tag(tag)),
                (false, N::Col) => {
                    self.insert_and_pop_element_for(tag);
                    return Step::Done;
                }
                (true, N::Colgroup) => {
                    if self.current_named(N::Colgroup) {
                        self.pop();
                        self.mode = Mode::InTable;
                    }
                    return Step::Done;
                }
                (true, N::Col) => return Step::Done,
                (_, N::Template) => return self.in_head(Token::Tag(tag)),
                _ => Token::Tag(tag),
            },
            token => token,
        };
        if self.current_named(N::Colgroup) {
            self.pop();
            Step::Reprocess(Mode::InTable, token)
        } else {
            Step::Done
        }
    }

    fn in_table_body(&mut self, token: Token<'a>) -> Step<'a> {
        let Token::Tag(tag) = token else {
            return self.in_table(token);
        };
        match (tag.end, tag.local) {
            (false, N::Tr) => {
                self.pop_until_current(table_body_context);
                self.insert_element_for(tag);
                self.mode = Mode::InRow;
                Step::Done
            }
            (false, N::Th | N::Td) => {
                self.pop_until_current(table_body_context);
                self.insert_phantom(N::Tr);
                Step::Reprocess(Mode::InRow, Token::Tag(tag))
            }
            (true, N::Tbody | N::Tfoot | N::Thead) => {
                if self.in_scope_named(table_scope, tag.local) {
                    self.pop_until_current(table_body_context);
                    self.pop();
                    self.mode = Mode::InTable;
                }
                Step::Done
            }
            (false, N::Caption | N::Col | N::Colgroup | N::Tbody | N::Tfoot | N::Thead)
            | (true, N::Table) => {
                // html5ever leaves `thead` out of this check.
                let table =
                    |namespace, name: N| html(namespace, name, &[N::Table, N::Tbody, N::Tfoot]);
                if self.in_scope(table_scope, |node| self.in_set(node, table)) {
                    self.pop_until_current(table_body_context);
                    self.pop();
                    Step::Reprocess(Mode::InTable, Token::Tag(tag))
                } else {
                    Step::Done
                }
            }
            (
                true,
                N::Body | N::Caption | N::Col | N::Colgroup | N::Html | N::Td | N::Th | N::Tr,
            ) => Step::Done,
            _ => self.in_table(Token::Tag(tag)),
        }
    }

    fn in_row(&mut self, token: Token<'a>) -> Step<'a> {
        let Token::Tag(tag) = token else {
            return self.in_table(token);
        };
        match (tag.end, tag.local) {
            (false, N::Th | N::Td) => {
                self.pop_until_current(table_row_context);
                self.insert_element_for(tag);
                self.mode = Mode::InCell;
                self.formatting.push(Formatting::Marker);
                Step::Done
            }
            (true, N::Tr) => {
                if self.in_scope_named(table_scope, N::Tr) {
                    self.pop_until_current(table_row_context);
                    self.pop();
                    self.mode = Mode::InTableBody;
                }
                Step::Done
            }
            (false, N::Caption | N::Col | N::Colgroup | N::Tbody | N::Tfoot | N::Thead | N::Tr)
            | (true, N::Table) => self.close_row(tag),
            (true, N::Tbody | N::Tfoot | N::Thead) => {
                if self.in_scope_named(table_scope, tag.local) {
                    self.close_row(tag)
                } else {
                    Step::Done
                }
            }
            (true, N::Body | N::Caption | N::Col | N::Colgroup | N::Html | N::Td | N::Th) => {
                Step::Done
            }
            _ => self.in_table(Token::Tag(tag)),
        }
    }

    fn close_row(&mut self, tag: Tag<'a>) -> Step<'a> {
        if !self.in_scope_named(table_scope, N::Tr) {
            return Step::Done;
        }
        self.pop_until_current(table_row_context);
        self.pop();
        Step::Reprocess(Mode::InTableBody, Token::Tag(tag))
    }

    fn in_cell(&mut self, token: Token<'a>) -> Step<'a> {
        let Token::Tag(tag) = token else {
            return self.in_body(token);
        };
        match (tag.end, tag.local) {
            (true, N::Td | N::Th) => {
                if self.in_scope_named(table_scope, tag.local) {
                    self.generate_implied_end_tags(cursory_implied_end);
                    self.pop_until_named(tag.local);
                    self.clear_active_formatting_to_marker();
                    self.mode = Mode::InRow;
                }
                Step::Done
            }
            (
                false,
                N::Caption
                | N::Col
                | N::Colgroup
                | N::Tbody
                | N::Td
                | N::Tfoot
                | N::Th
                | N::Thead
                | N::Tr,
            ) => {
                if self.in_scope(table_scope, |node| self.in_set(node, td_th)) {
                    self.close_the_cell();
                    Step::Reprocess(Mode::InRow, Token::Tag(tag))
                } else {
                    Step::Done
                }
            }
            (true, N::Body | N::Caption | N::Col | N::Colgroup | N::Html) => Step::Done,
            (true, N::Table | N::Tbody | N::Tfoot | N::Thead | N::Tr) => {
                if self.in_scope_named(table_scope, tag.local) {
                    self.close_the_cell();
                    Step::Reprocess(Mode::InRow, Token::Tag(tag))
                } else {
                    Step::Done
                }
            }
            _ => self.in_body(Token::Tag(tag)),
        }
    }

    fn in_template(&mut self, token: Token<'a>) -> Step<'a> {
        let tag = match token {
            Token::Characters(..) | Token::Comment(_) => return self.in_body(token),
            Token::Eof if !self.in_html_named(N::Template) => return Step::Done,
            Token::Eof => {
                self.pop_until_named(N::Template);
                self.clear_active_formatting_to_marker();
                self.template_modes.pop();
                self.mode = self.reset_insertion_mode();
                return Step::Reprocess(self.mode, Token::Eof);
            }
            Token::NullCharacter => return Step::Done,
            Token::Tag(tag) => tag,
        };
        let mode = match (tag.end, tag.local) {
            (
                false,
                N::Base
                | N::Basefont
                | N::Bgsound
                | N::Link
                | N::Meta
                | N::Noframes
                | N::Script
                | N::Style
                | N::Template
                | N::Title,
            )
            | (true, N::Template) => return self.in_head(Token::Tag(tag)),
            (true, _) => return Step::Done,
            (false, N::Caption | N::Colgroup | N::Tbody | N::Tfoot | N::Thead) => Mode::InTable,
            (false, N::Col) => Mode::InColumnGroup,
            (false, N::Tr) => Mode::InTableBody,
            (false, N::Td | N::Th) => Mode::InRow,
            (false, _) => Mode::InBody,
        };
        self.template_modes.pop();
        self.template_modes.push(mode);
        Step::Reprocess(mode, Token::Tag(tag))
    }

    fn after_body(&mut self, token: Token<'a>) -> Step<'a> {
        match token {
            Token::Characters(SplitStatus::NotSplit, text) => Step::SplitWhitespace(text),
            Token::Characters(SplitStatus::Whitespace, _) => self.in_body(token),
            Token::Comment(text) => self.append_comment_to(self.open[0], text),
            Token::Eof => Step::Done,
            Token::Tag(tag) if !tag.end && tag.local == N::Html => self.in_body(Token::Tag(tag)),
            Token::Tag(tag) if tag.end && tag.local == N::Html => {
                if self.context.is_none() {
                    self.mode = Mode::AfterAfterBody;
                }
                Step::Done
            }
            token => Step::Reprocess(Mode::InBody, token),
        }
    }

    fn in_frameset(&mut self, token: Token<'a>) -> Step<'a> {
        let tag = match token {
            Token::Characters(SplitStatus::NotSplit, text) => return Step::SplitWhitespace(text),
            Token::Characters(SplitStatus::Whitespace, text) => return self.append_text(text),
            Token::Comment(text) => return self.append_comment(text),
            Token::Tag(tag) => tag,
            _ => return Step::Done,
        };
        match (tag.end, tag.local) {
            (false, N::Html) => return self.in_body(Token::Tag(tag)),
            (false, N::Frameset) => {
                self.insert_element_for(tag);
            }
            (true, N::Frameset) if self.open.len() != 1 => {
                self.pop();
                if self.context.is_none() && !self.current_named(N::Frameset) {
                    self.mode = Mode::AfterFrameset;
                }
            }
            (false, N::Frame) => {
                self.insert_and_pop_element_for(tag);
            }
            (false, N::Noframes) => return self.in_head(Token::Tag(tag)),
            _ => {}
        }
        Step::Done
    }

    fn after_frameset(&mut self, token: Token<'a>) -> Step<'a> {
        let tag = match token {
            Token::Characters(SplitStatus::NotSplit, text) => return Step::SplitWhitespace(text),
            Token::Characters(SplitStatus::Whitespace, text) => return self.append_text(text),
            Token::Comment(text) => return self.append_comment(text),
            Token::Tag(tag) => tag,
            _ => return Step::Done,
        };
        match (tag.end, tag.local) {
            (false, N::Html) => self.in_body(Token::Tag(tag)),
            (true, N::Html) => {
                self.mode = Mode::AfterAfterFrameset;
                Step::Done
            }
            (false, N::Noframes) => self.in_head(Token::Tag(tag)),
            _ => Step::Done,
        }
    }

    fn after_after_body(&mut self, token: Token<'a>) -> Step<'a> {
        match token {
            Token::Characters(SplitStatus::NotSplit, text) => Step::SplitWhitespace(text),
            Token::Characters(SplitStatus::Whitespace, _) => self.in_body(token),
            Token::Comment(text) => self.append_comment_to(ROOT, text),
            Token::Eof => Step::Done,
            Token::Tag(tag) if !tag.end && tag.local == N::Html => self.in_body(Token::Tag(tag)),
            token => Step::Reprocess(Mode::InBody, token),
        }
    }

    fn after_after_frameset(&mut self, token: Token<'a>) -> Step<'a> {
        match token {
            Token::Characters(SplitStatus::NotSplit, text) => Step::SplitWhitespace(text),
            Token::Characters(SplitStatus::Whitespace, _) => self.in_body(token),
            Token::Comment(text) => self.append_comment_to(ROOT, text),
            Token::Tag(tag) if !tag.end && tag.local == N::Html => self.in_body(Token::Tag(tag)),
            Token::Tag(tag) if !tag.end && tag.local == N::Noframes => {
                self.in_head(Token::Tag(tag))
            }
            _ => Step::Done,
        }
    }

    pub(super) fn step_foreign(&mut self, token: Token<'a>) -> Step<'a> {
        let tag = match token {
            Token::NullCharacter => return self.append_text(Cow::Borrowed("\u{fffd}")),
            Token::Characters(_, text) => {
                if any_not_whitespace(&text) {
                    self.frameset_ok = false;
                }
                return self.append_text(text);
            }
            Token::Comment(text) => return self.append_comment(text),
            Token::Tag(tag) => tag,
            Token::Eof => unreachable!("end of input is never foreign content"),
        };
        let breaks_out = if tag.end {
            matches!(tag.local, N::Br | N::P)
        } else {
            matches!(
                tag.local,
                N::B | N::Big
                    | N::Blockquote
                    | N::Body
                    | N::Br
                    | N::Center
                    | N::Code
                    | N::Dd
                    | N::Div
                    | N::Dl
                    | N::Dt
                    | N::Em
                    | N::Embed
                    | N::H1
                    | N::H2
                    | N::H3
                    | N::H4
                    | N::H5
                    | N::H6
                    | N::Head
                    | N::Hr
                    | N::I
                    | N::Img
                    | N::Li
                    | N::Listing
                    | N::Menu
                    | N::Meta
                    | N::Nobr
                    | N::Ol
                    | N::P
                    | N::Pre
                    | N::Ruby
                    | N::S
                    | N::Small
                    | N::Span
                    | N::Strong
                    | N::Strike
                    | N::Sub
                    | N::Sup
                    | N::Table
                    | N::Tt
                    | N::U
                    | N::Ul
                    | N::Var
            ) || (tag.local == N::Font
                && tag
                    .attributes
                    .iter()
                    .any(|attribute| matches!(&*attribute.name, "color" | "face" | "size")))
        };
        if breaks_out {
            while !self.current_in(|namespace, name| {
                namespace == Namespace::Html
                    || mathml_text_integration_point(namespace, name)
                    || svg_html_integration_point(namespace, name)
            }) {
                self.pop();
            }
            return self.step(self.mode, Token::Tag(tag));
        }
        if !tag.end {
            return self.foreign_start_tag(tag);
        }
        let mut index = self.open.len() - 1;
        let mut first = true;
        while index > 0 {
            let element = self.document.element(self.open[index]);
            if !first && element.namespace == Namespace::Html {
                return self.step(self.mode, Token::Tag(tag));
            }
            if element.name.eq_ignore_ascii_case(&tag.name) {
                self.open.truncate(index);
                break;
            }
            first = false;
            index -= 1;
        }
        Step::Done
    }

    fn foreign_start_tag(&mut self, mut tag: Tag<'a>) -> Step<'a> {
        let namespace = self.namespace(self.adjusted_current_node());
        match namespace {
            Namespace::MathMl => foreign::adjust_mathml_attributes(&mut tag),
            Namespace::Svg => {
                foreign::adjust_svg_tag_name(&mut tag);
                foreign::adjust_svg_attributes(&mut tag);
            }
            Namespace::Html => {}
        }
        foreign::adjust_foreign_attributes(&mut tag);
        self.insert_element(
            !tag.self_closing,
            namespace,
            tag.local,
            tag.name,
            tag.attributes,
        );
        Step::Done
    }

    fn enter_foreign(&mut self, mut tag: Tag<'a>, namespace: Namespace) -> Step<'a> {
        match namespace {
            Namespace::MathMl => foreign::adjust_mathml_attributes(&mut tag),
            Namespace::Svg => foreign::adjust_svg_attributes(&mut tag),
            Namespace::Html => unreachable!("only svg and math enter foreign content"),
        }
        foreign::adjust_foreign_attributes(&mut tag);
        self.insert_element(
            !tag.self_closing,
            namespace,
            tag.local,
            tag.name,
            tag.attributes,
        );
        Step::Done
    }
}
