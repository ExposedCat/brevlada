//! The insertion modes and the rules for foreign content.
use super::{
    Formatting, Mode, SplitStatus, Step, Token, TreeBuilder, any_not_whitespace, button_scope,
    cursory_implied_end, default_scope, foreign, heading, html, list_item_scope,
    mathml_text_integration_point, special, svg_html_integration_point, table_body_context,
    table_row_context, table_scope, td_th, thorough_implied_end,
};
use crate::models::dom::{Namespace, ROOT, tokenizer::RawKind, tokenizer::Tag};
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
            Token::Tag(tag) if !tag.end && tag.name == "html" => {
                self.create_root(tag.attributes);
                self.mode = Mode::BeforeHead;
                Step::Done
            }
            Token::Tag(tag)
                if tag.end && !matches!(&*tag.name, "head" | "body" | "html" | "br") =>
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
            Token::Tag(tag) if !tag.end && tag.name == "html" => self.in_body(Token::Tag(tag)),
            Token::Tag(tag) if !tag.end && tag.name == "head" => {
                self.head = Some(self.insert_element_for(tag));
                self.mode = Mode::InHead;
                Step::Done
            }
            Token::Tag(tag)
                if tag.end && !matches!(&*tag.name, "head" | "body" | "html" | "br") =>
            {
                Step::Done
            }
            token => {
                self.head = Some(self.insert_phantom("head"));
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
        match (tag.end, &*tag.name) {
            (false, "html") => self.in_body(Token::Tag(tag)),
            (false, "base" | "basefont" | "bgsound" | "link" | "meta") => {
                self.insert_and_pop_element_for(tag);
                Step::Done
            }
            (false, "title") => self.parse_raw_data(tag, RawKind::Rcdata),
            (false, "noframes" | "style" | "noscript") => {
                self.parse_raw_data(tag, RawKind::Rawtext)
            }
            (false, "script") => self.parse_raw_data(tag, RawKind::ScriptData),
            (true, "head") => {
                self.pop();
                self.mode = Mode::AfterHead;
                Step::Done
            }
            (false, "template") => {
                self.formatting.push(Formatting::Marker);
                self.frameset_ok = false;
                self.mode = Mode::InTemplate;
                self.template_modes.push(Mode::InTemplate);
                self.insert_element_for(tag);
                Step::Done
            }
            (true, "template") => {
                if self.in_html_named("template") {
                    self.generate_implied_end_tags(thorough_implied_end);
                    self.pop_until_named("template");
                    self.clear_active_formatting_to_marker();
                    self.template_modes.pop();
                    self.mode = self.reset_insertion_mode();
                }
                Step::Done
            }
            (true, "body" | "html" | "br") | (false, _) if tag.name != "head" => {
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
                self.insert_phantom("body");
                return Step::Reprocess(Mode::InBody, token);
            }
        };
        match (tag.end, &*tag.name) {
            (false, "html") => self.in_body(Token::Tag(tag)),
            (false, "body") => {
                self.insert_element_for(tag);
                self.frameset_ok = false;
                self.mode = Mode::InBody;
                Step::Done
            }
            (false, "frameset") => {
                self.insert_element_for(tag);
                self.mode = Mode::InFrameset;
                Step::Done
            }
            (
                false,
                "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" | "script" | "style"
                | "template" | "title",
            ) => {
                let head = self.head.expect("the head element exists after the head");
                self.open.push(head);
                let step = self.in_head(Token::Tag(tag));
                self.remove_from_stack(head);
                step
            }
            (true, "template") => self.in_head(Token::Tag(tag)),
            (true, "body" | "html" | "br") | (false, _) if tag.name != "head" => {
                self.insert_phantom("body");
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
        match &*tag.name {
            "html" => {
                if !self.in_html_named("template") {
                    self.document
                        .add_attributes_if_missing(self.open[0], tag.attributes);
                }
            }
            "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" | "script" | "style"
            | "template" | "title" => return self.in_head(Token::Tag(tag)),
            "body" => {
                if let Some(body) = self.body_element()
                    && !self.in_html_named("template")
                {
                    self.frameset_ok = false;
                    self.document
                        .add_attributes_if_missing(body, tag.attributes);
                }
            }
            "frameset" => {
                if self.frameset_ok
                    && let Some(body) = self.body_element()
                {
                    self.document.detach(body);
                    self.open.truncate(1);
                    self.insert_element_for(tag);
                    self.mode = Mode::InFrameset;
                }
            }
            "address" | "article" | "aside" | "blockquote" | "center" | "details" | "dialog"
            | "dir" | "div" | "dl" | "fieldset" | "figcaption" | "figure" | "footer" | "header"
            | "hgroup" | "main" | "menu" | "nav" | "ol" | "p" | "search" | "section"
            | "summary" | "ul" => {
                self.close_p_element_in_button_scope();
                self.insert_element_for(tag);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.close_p_element_in_button_scope();
                if self.current_in(heading) {
                    self.pop();
                }
                self.insert_element_for(tag);
            }
            "pre" | "listing" => {
                self.close_p_element_in_button_scope();
                self.insert_element_for(tag);
                self.ignore_lf = true;
                self.frameset_ok = false;
            }
            "form" => {
                if self.form.is_none() || self.in_html_named("template") {
                    self.close_p_element_in_button_scope();
                    let form = self.insert_element_for(tag);
                    if !self.in_html_named("template") {
                        self.form = Some(form);
                    }
                }
            }
            "li" | "dd" | "dt" => {
                let list = tag.name == "li";
                self.frameset_ok = false;
                let mut close = None;
                for &node in self.open.iter().rev() {
                    let (namespace, name) = self.name(node);
                    let closes = if list {
                        html(namespace, name, &["li"])
                    } else {
                        html(namespace, name, &["dd", "dt"])
                    };
                    if closes {
                        close = Some(match name {
                            "li" => "li",
                            "dd" => "dd",
                            _ => "dt",
                        });
                        break;
                    }
                    if special(namespace, name) && !html(namespace, name, &["address", "div", "p"])
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
            "plaintext" => {
                self.close_p_element_in_button_scope();
                self.insert_element_for(tag);
                self.tokenizer.plaintext();
            }
            "button" => {
                if self.in_scope_named(default_scope, "button") {
                    self.generate_implied_end_tags(cursory_implied_end);
                    self.pop_until_named("button");
                }
                self.reconstruct_active_formatting_elements();
                self.insert_element_for(tag);
                self.frameset_ok = false;
            }
            "a" => {
                self.handle_misnested_a_tags();
                self.reconstruct_active_formatting_elements();
                self.create_formatting_element_for(tag);
            }
            "b" | "big" | "code" | "em" | "font" | "i" | "s" | "small" | "strike" | "strong"
            | "tt" | "u" => {
                self.reconstruct_active_formatting_elements();
                self.create_formatting_element_for(tag);
            }
            "nobr" => {
                self.reconstruct_active_formatting_elements();
                if self.in_scope_named(default_scope, "nobr") {
                    self.adoption_agency("nobr");
                    self.reconstruct_active_formatting_elements();
                }
                self.create_formatting_element_for(tag);
            }
            "applet" | "marquee" | "object" => {
                self.reconstruct_active_formatting_elements();
                self.insert_element_for(tag);
                self.formatting.push(Formatting::Marker);
                self.frameset_ok = false;
            }
            "table" => {
                if !self.quirks {
                    self.close_p_element_in_button_scope();
                }
                self.insert_element_for(tag);
                self.frameset_ok = false;
                self.mode = Mode::InTable;
            }
            "area" | "br" | "embed" | "img" | "keygen" | "wbr" => {
                self.reconstruct_active_formatting_elements();
                self.insert_and_pop_element_for(tag);
                self.frameset_ok = false;
            }
            "input" => {
                if self.in_scope_named(default_scope, "select") {
                    self.pop_until_named("select");
                }
                let hidden = Self::is_type_hidden(&tag);
                self.reconstruct_active_formatting_elements();
                self.insert_and_pop_element_for(tag);
                if !hidden {
                    self.frameset_ok = false;
                }
            }
            "param" | "source" | "track" => {
                self.insert_and_pop_element_for(tag);
            }
            "hr" => {
                self.close_p_element_in_button_scope();
                if self.in_scope_named(default_scope, "select") {
                    self.generate_implied_end_tags(cursory_implied_end);
                }
                self.insert_and_pop_element_for(tag);
                self.frameset_ok = false;
            }
            "image" => {
                return self.in_body(Token::Tag(Tag {
                    name: Cow::Borrowed("img"),
                    ..tag
                }));
            }
            "textarea" => {
                self.ignore_lf = true;
                self.frameset_ok = false;
                return self.parse_raw_data(tag, RawKind::Rcdata);
            }
            "xmp" => {
                self.close_p_element_in_button_scope();
                self.reconstruct_active_formatting_elements();
                self.frameset_ok = false;
                return self.parse_raw_data(tag, RawKind::Rawtext);
            }
            "iframe" => {
                self.frameset_ok = false;
                return self.parse_raw_data(tag, RawKind::Rawtext);
            }
            "noembed" | "noscript" => return self.parse_raw_data(tag, RawKind::Rawtext),
            "select" => {
                if self.in_scope_named(default_scope, "select") {
                    self.pop_until_named("select");
                } else {
                    self.reconstruct_active_formatting_elements();
                    self.insert_element_for(tag);
                    self.frameset_ok = false;
                }
            }
            "option" | "optgroup" => {
                if self.in_scope_named(default_scope, "select") {
                    if tag.name == "option" {
                        self.generate_implied_end_except("optgroup");
                    } else {
                        self.generate_implied_end_tags(cursory_implied_end);
                    }
                } else if self.current_named("option") {
                    self.pop();
                }
                self.reconstruct_active_formatting_elements();
                self.insert_element_for(tag);
            }
            "rb" | "rtc" => {
                if self.in_scope_named(default_scope, "ruby") {
                    self.generate_implied_end_tags(cursory_implied_end);
                }
                self.insert_element_for(tag);
            }
            "rp" | "rt" => {
                if self.in_scope_named(default_scope, "ruby") {
                    self.generate_implied_end_except("rtc");
                }
                self.insert_element_for(tag);
            }
            "math" => {
                self.reconstruct_active_formatting_elements();
                return self.enter_foreign(tag, Namespace::MathMl);
            }
            "svg" => {
                self.reconstruct_active_formatting_elements();
                return self.enter_foreign(tag, Namespace::Svg);
            }
            "caption" | "col" | "colgroup" | "frame" | "head" | "tbody" | "td" | "tfoot" | "th"
            | "thead" | "tr" => {}
            _ => {
                self.reconstruct_active_formatting_elements();
                self.insert_element_for(tag);
            }
        }
        Step::Done
    }

    fn in_body_end_tag(&mut self, tag: Tag<'a>) -> Step<'a> {
        match &*tag.name {
            "template" => return self.in_head(Token::Tag(tag)),
            "body" => {
                if self.in_scope_named(default_scope, "body") {
                    self.mode = Mode::AfterBody;
                }
            }
            "html" => {
                if self.in_scope_named(default_scope, "body") {
                    return Step::Reprocess(Mode::AfterBody, Token::Tag(tag));
                }
            }
            "address" | "article" | "aside" | "blockquote" | "button" | "center" | "details"
            | "dialog" | "dir" | "div" | "dl" | "fieldset" | "figcaption" | "figure" | "footer"
            | "header" | "hgroup" | "listing" | "main" | "menu" | "nav" | "ol" | "pre"
            | "search" | "section" | "select" | "summary" | "ul" => {
                if self.in_scope_named(default_scope, &tag.name) {
                    self.generate_implied_end_tags(cursory_implied_end);
                    self.pop_until_named(&tag.name);
                }
            }
            "form" if self.in_html_named("template") => {
                if self.in_scope_named(default_scope, "form") {
                    self.generate_implied_end_tags(cursory_implied_end);
                    self.pop_until_named("form");
                }
            }
            "form" => {
                if let Some(form) = self.form.take()
                    && self.in_scope(default_scope, |node| node == form)
                {
                    self.generate_implied_end_tags(cursory_implied_end);
                    self.remove_from_stack(form);
                }
            }
            "p" => {
                if !self.in_scope_named(button_scope, "p") {
                    self.insert_phantom("p");
                }
                self.close_p_element();
            }
            "li" => {
                if self.in_scope_named(list_item_scope, "li") {
                    self.generate_implied_end_except("li");
                    self.pop_until_named("li");
                }
            }
            "dd" | "dt" => {
                if self.in_scope_named(default_scope, &tag.name) {
                    self.generate_implied_end_except(&tag.name);
                    self.pop_until_named(&tag.name);
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if self.in_scope(default_scope, |node| self.in_set(node, heading)) {
                    self.generate_implied_end_tags(cursory_implied_end);
                    self.pop_until(heading);
                }
            }
            "a" | "b" | "big" | "code" | "em" | "font" | "i" | "nobr" | "s" | "small"
            | "strike" | "strong" | "tt" | "u" => self.adoption_agency(&tag.name),
            "applet" | "marquee" | "object" => {
                if self.in_scope_named(default_scope, &tag.name) {
                    self.generate_implied_end_tags(cursory_implied_end);
                    self.pop_until_named(&tag.name);
                    self.clear_active_formatting_to_marker();
                }
            }
            "br" => {
                return self.in_body(Token::Tag(Tag {
                    end: false,
                    attributes: Vec::new(),
                    ..tag
                }));
            }
            _ => self.process_end_tag_in_body(&tag.name),
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
        match (tag.end, &*tag.name) {
            (false, "caption") => {
                self.pop_until_current(table_scope);
                self.formatting.push(Formatting::Marker);
                self.insert_element_for(tag);
                self.mode = Mode::InCaption;
            }
            (false, "colgroup") => {
                self.pop_until_current(table_scope);
                self.insert_element_for(tag);
                self.mode = Mode::InColumnGroup;
            }
            (false, "col") => {
                self.pop_until_current(table_scope);
                self.insert_phantom("colgroup");
                return Step::Reprocess(Mode::InColumnGroup, Token::Tag(tag));
            }
            (false, "tbody" | "tfoot" | "thead") => {
                self.pop_until_current(table_scope);
                self.insert_element_for(tag);
                self.mode = Mode::InTableBody;
            }
            (false, "td" | "th" | "tr") => {
                self.pop_until_current(table_scope);
                self.insert_phantom("tbody");
                return Step::Reprocess(Mode::InTableBody, Token::Tag(tag));
            }
            (false, "table") => {
                if self.in_scope_named(table_scope, "table") {
                    self.pop_until_named("table");
                    return Step::Reprocess(self.reset_insertion_mode(), Token::Tag(tag));
                }
            }
            (true, "table") => {
                if self.in_scope_named(table_scope, "table") {
                    self.pop_until_named("table");
                    self.mode = self.reset_insertion_mode();
                }
            }
            (
                true,
                "body" | "caption" | "col" | "colgroup" | "html" | "tbody" | "td" | "tfoot" | "th"
                | "thead" | "tr",
            ) => {}
            (false, "style" | "script" | "template") | (true, "template") => {
                return self.in_head(Token::Tag(tag));
            }
            (false, "input") if Self::is_type_hidden(&tag) => {
                self.insert_and_pop_element_for(tag);
            }
            (false, "form") => {
                if !self.in_html_named("template") && self.form.is_none() {
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
        match (tag.end, &*tag.name) {
            (
                false,
                "caption" | "col" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr",
            )
            | (true, "table" | "caption") => {
                if !self.in_scope_named(table_scope, "caption") {
                    return Step::Done;
                }
                self.generate_implied_end_tags(cursory_implied_end);
                self.pop_until_named("caption");
                self.clear_active_formatting_to_marker();
                if tag.end && tag.name == "caption" {
                    self.mode = Mode::InTable;
                    Step::Done
                } else {
                    Step::Reprocess(Mode::InTable, Token::Tag(tag))
                }
            }
            (
                true,
                "body" | "col" | "colgroup" | "html" | "tbody" | "td" | "tfoot" | "th" | "thead"
                | "tr",
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
            Token::Tag(tag) => match (tag.end, &*tag.name) {
                (false, "html") => return self.in_body(Token::Tag(tag)),
                (false, "col") => {
                    self.insert_and_pop_element_for(tag);
                    return Step::Done;
                }
                (true, "colgroup") => {
                    if self.current_named("colgroup") {
                        self.pop();
                        self.mode = Mode::InTable;
                    }
                    return Step::Done;
                }
                (true, "col") => return Step::Done,
                (_, "template") => return self.in_head(Token::Tag(tag)),
                _ => Token::Tag(tag),
            },
            token => token,
        };
        if self.current_named("colgroup") {
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
        match (tag.end, &*tag.name) {
            (false, "tr") => {
                self.pop_until_current(table_body_context);
                self.insert_element_for(tag);
                self.mode = Mode::InRow;
                Step::Done
            }
            (false, "th" | "td") => {
                self.pop_until_current(table_body_context);
                self.insert_phantom("tr");
                Step::Reprocess(Mode::InRow, Token::Tag(tag))
            }
            (true, "tbody" | "tfoot" | "thead") => {
                if self.in_scope_named(table_scope, &tag.name) {
                    self.pop_until_current(table_body_context);
                    self.pop();
                    self.mode = Mode::InTable;
                }
                Step::Done
            }
            (false, "caption" | "col" | "colgroup" | "tbody" | "tfoot" | "thead")
            | (true, "table") => {
                // html5ever leaves `thead` out of this check.
                let table =
                    |namespace, name: &str| html(namespace, name, &["table", "tbody", "tfoot"]);
                if self.in_scope(table_scope, |node| self.in_set(node, table)) {
                    self.pop_until_current(table_body_context);
                    self.pop();
                    Step::Reprocess(Mode::InTable, Token::Tag(tag))
                } else {
                    Step::Done
                }
            }
            (true, "body" | "caption" | "col" | "colgroup" | "html" | "td" | "th" | "tr") => {
                Step::Done
            }
            _ => self.in_table(Token::Tag(tag)),
        }
    }

    fn in_row(&mut self, token: Token<'a>) -> Step<'a> {
        let Token::Tag(tag) = token else {
            return self.in_table(token);
        };
        match (tag.end, &*tag.name) {
            (false, "th" | "td") => {
                self.pop_until_current(table_row_context);
                self.insert_element_for(tag);
                self.mode = Mode::InCell;
                self.formatting.push(Formatting::Marker);
                Step::Done
            }
            (true, "tr") => {
                if self.in_scope_named(table_scope, "tr") {
                    self.pop_until_current(table_row_context);
                    self.pop();
                    self.mode = Mode::InTableBody;
                }
                Step::Done
            }
            (false, "caption" | "col" | "colgroup" | "tbody" | "tfoot" | "thead" | "tr")
            | (true, "table") => self.close_row(tag),
            (true, "tbody" | "tfoot" | "thead") => {
                if self.in_scope_named(table_scope, &tag.name) {
                    self.close_row(tag)
                } else {
                    Step::Done
                }
            }
            (true, "body" | "caption" | "col" | "colgroup" | "html" | "td" | "th") => Step::Done,
            _ => self.in_table(Token::Tag(tag)),
        }
    }

    fn close_row(&mut self, tag: Tag<'a>) -> Step<'a> {
        if !self.in_scope_named(table_scope, "tr") {
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
        match (tag.end, &*tag.name) {
            (true, "td" | "th") => {
                if self.in_scope_named(table_scope, &tag.name) {
                    self.generate_implied_end_tags(cursory_implied_end);
                    self.pop_until_named(&tag.name);
                    self.clear_active_formatting_to_marker();
                    self.mode = Mode::InRow;
                }
                Step::Done
            }
            (
                false,
                "caption" | "col" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr",
            ) => {
                if self.in_scope(table_scope, |node| self.in_set(node, td_th)) {
                    self.close_the_cell();
                    Step::Reprocess(Mode::InRow, Token::Tag(tag))
                } else {
                    Step::Done
                }
            }
            (true, "body" | "caption" | "col" | "colgroup" | "html") => Step::Done,
            (true, "table" | "tbody" | "tfoot" | "thead" | "tr") => {
                if self.in_scope_named(table_scope, &tag.name) {
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
            Token::Eof if !self.in_html_named("template") => return Step::Done,
            Token::Eof => {
                self.pop_until_named("template");
                self.clear_active_formatting_to_marker();
                self.template_modes.pop();
                self.mode = self.reset_insertion_mode();
                return Step::Reprocess(self.mode, Token::Eof);
            }
            Token::NullCharacter => return Step::Done,
            Token::Tag(tag) => tag,
        };
        let mode = match (tag.end, &*tag.name) {
            (
                false,
                "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" | "script" | "style"
                | "template" | "title",
            )
            | (true, "template") => return self.in_head(Token::Tag(tag)),
            (true, _) => return Step::Done,
            (false, "caption" | "colgroup" | "tbody" | "tfoot" | "thead") => Mode::InTable,
            (false, "col") => Mode::InColumnGroup,
            (false, "tr") => Mode::InTableBody,
            (false, "td" | "th") => Mode::InRow,
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
            Token::Tag(tag) if !tag.end && tag.name == "html" => self.in_body(Token::Tag(tag)),
            Token::Tag(tag) if tag.end && tag.name == "html" => {
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
        match (tag.end, &*tag.name) {
            (false, "html") => return self.in_body(Token::Tag(tag)),
            (false, "frameset") => {
                self.insert_element_for(tag);
            }
            (true, "frameset") if self.open.len() != 1 => {
                self.pop();
                if self.context.is_none() && !self.current_named("frameset") {
                    self.mode = Mode::AfterFrameset;
                }
            }
            (false, "frame") => {
                self.insert_and_pop_element_for(tag);
            }
            (false, "noframes") => return self.in_head(Token::Tag(tag)),
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
        match (tag.end, &*tag.name) {
            (false, "html") => self.in_body(Token::Tag(tag)),
            (true, "html") => {
                self.mode = Mode::AfterAfterFrameset;
                Step::Done
            }
            (false, "noframes") => self.in_head(Token::Tag(tag)),
            _ => Step::Done,
        }
    }

    fn after_after_body(&mut self, token: Token<'a>) -> Step<'a> {
        match token {
            Token::Characters(SplitStatus::NotSplit, text) => Step::SplitWhitespace(text),
            Token::Characters(SplitStatus::Whitespace, _) => self.in_body(token),
            Token::Comment(text) => self.append_comment_to(ROOT, text),
            Token::Eof => Step::Done,
            Token::Tag(tag) if !tag.end && tag.name == "html" => self.in_body(Token::Tag(tag)),
            token => Step::Reprocess(Mode::InBody, token),
        }
    }

    fn after_after_frameset(&mut self, token: Token<'a>) -> Step<'a> {
        match token {
            Token::Characters(SplitStatus::NotSplit, text) => Step::SplitWhitespace(text),
            Token::Characters(SplitStatus::Whitespace, _) => self.in_body(token),
            Token::Comment(text) => self.append_comment_to(ROOT, text),
            Token::Tag(tag) if !tag.end && tag.name == "html" => self.in_body(Token::Tag(tag)),
            Token::Tag(tag) if !tag.end && tag.name == "noframes" => self.in_head(Token::Tag(tag)),
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
            matches!(&*tag.name, "br" | "p")
        } else {
            matches!(
                &*tag.name,
                "b" | "big"
                    | "blockquote"
                    | "body"
                    | "br"
                    | "center"
                    | "code"
                    | "dd"
                    | "div"
                    | "dl"
                    | "dt"
                    | "em"
                    | "embed"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "head"
                    | "hr"
                    | "i"
                    | "img"
                    | "li"
                    | "listing"
                    | "menu"
                    | "meta"
                    | "nobr"
                    | "ol"
                    | "p"
                    | "pre"
                    | "ruby"
                    | "s"
                    | "small"
                    | "span"
                    | "strong"
                    | "strike"
                    | "sub"
                    | "sup"
                    | "table"
                    | "tt"
                    | "u"
                    | "ul"
                    | "var"
            ) || (tag.name == "font"
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
            let (namespace, name) = self.name(self.open[index]);
            if !first && namespace == Namespace::Html {
                return self.step(self.mode, Token::Tag(tag));
            }
            if name.eq_ignore_ascii_case(&tag.name) {
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
        self.insert_element(!tag.self_closing, namespace, tag.name, tag.attributes);
        Step::Done
    }

    fn enter_foreign(&mut self, mut tag: Tag<'a>, namespace: Namespace) -> Step<'a> {
        match namespace {
            Namespace::MathMl => foreign::adjust_mathml_attributes(&mut tag),
            Namespace::Svg => foreign::adjust_svg_attributes(&mut tag),
            Namespace::Html => unreachable!("only svg and math enter foreign content"),
        }
        foreign::adjust_foreign_attributes(&mut tag);
        self.insert_element(!tag.self_closing, namespace, tag.name, tag.attributes);
        Step::Done
    }
}
