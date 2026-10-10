use super::{Attribute, Prefix, doctype::Doctype, entities::ENTITIES, names::LocalName};
use std::borrow::Cow;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RawKind {
    Rcdata,
    Rawtext,
    ScriptData,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    Raw(RawKind),
    Plaintext,
    Cdata,
}

#[derive(Clone)]
pub(super) struct Tag<'a> {
    pub(super) end: bool,
    pub(super) name: Cow<'a, str>,
    pub(super) local: LocalName,
    pub(super) self_closing: bool,
    pub(super) attributes: Vec<Attribute<'a>>,
}

pub(super) enum Token<'a> {
    Tag(Tag<'a>),
    /// `after_error` marks text starting with a character reference that misses
    /// its semicolon: html5ever reports that error before emitting the text.
    Characters {
        text: Cow<'a, str>,
        after_error: bool,
    },
    NullCharacter,
    Comment(Cow<'a, str>),
    Doctype(Doctype),
    Eof,
}

enum Reference {
    Named(&'static str),
    Numeric(char),
}

/// Text that borrows the input for as long as it equals a slice of it.
struct Text<'a> {
    text: Cow<'a, str>,
    end: usize,
    after_error: bool,
}

impl Default for Text<'_> {
    fn default() -> Self {
        Self {
            text: Cow::Borrowed(""),
            end: 0,
            after_error: false,
        }
    }
}

impl<'a> Text<'a> {
    fn source(&mut self, input: &'a str, start: usize, end: usize) {
        if start == end {
            return;
        }
        match &mut self.text {
            Cow::Borrowed("") => self.text = Cow::Borrowed(&input[start..end]),
            Cow::Borrowed(text) if self.end == start => {
                self.text = Cow::Borrowed(&input[start - text.len()..end]);
            }
            text => text.to_mut().push_str(&input[start..end]),
        }
        self.end = end;
    }

    fn push(&mut self, text: &str) {
        self.text.to_mut().push_str(text);
    }

    fn reference(&mut self, (reference, missing_semicolon): (Reference, bool)) {
        self.after_error |= missing_semicolon && self.text.is_empty();
        match reference {
            Reference::Named(text) => self.push(text),
            Reference::Numeric(character) => self.text.to_mut().push(character),
        }
    }

    fn token(self) -> Token<'a> {
        if self.text.is_empty() {
            Token::Eof
        } else {
            Token::Characters {
                text: self.text,
                after_error: self.after_error,
            }
        }
    }
}

fn space(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | b'\x0C' | b' ' | b'\r')
}

/// Preprocesses text that is copied as is: CR and CRLF become LF, NUL becomes U+FFFD.
fn clean(text: &str) -> Cow<'_, str> {
    if !text.bytes().any(|byte| matches!(byte, b'\r' | b'\0')) {
        return Cow::Borrowed(text);
    }
    let mut clean = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                characters.next_if_eq(&'\n');
                clean.push('\n');
            }
            '\0' => clean.push('\u{fffd}'),
            character => clean.push(character),
        }
    }
    Cow::Owned(clean)
}

/// Tag, attribute and doctype names: ASCII letters lowercased, NUL replaced.
fn name(name: &str) -> String {
    name.to_ascii_lowercase().replace('\0', "\u{fffd}")
}

/// What numeric references to the C1 controls stand for, from windows-1252.
const C1: [char; 32] = [
    '\u{20ac}', '\u{81}', '\u{201a}', '\u{192}', '\u{201e}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{2c6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8d}', '\u{17d}', '\u{8f}',
    '\u{90}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{2dc}', '\u{2122}', '\u{161}', '\u{203a}', '\u{153}', '\u{9d}', '\u{17e}', '\u{178}',
];

fn numeric(value: u32) -> char {
    match value {
        0 | 0xD800..=0xDFFF | 0x110000.. => '\u{fffd}',
        0x80..=0x9F => C1[value as usize - 0x80],
        _ => char::from_u32(value).expect("surrogates and out-of-range values are replaced"),
    }
}

fn lookup(name: &str) -> Option<&'static str> {
    ENTITIES
        .binary_search_by(|(entity, _)| (*entity).cmp(name))
        .ok()
        .map(|index| ENTITIES[index].1)
}

pub(super) struct Tokenizer<'a> {
    input: &'a str,
    position: usize,
    state: State,
    last_start_tag: Cow<'a, str>,
}

impl<'a> Tokenizer<'a> {
    pub(super) fn new(input: &'a str) -> Self {
        Self {
            input: input.strip_prefix('\u{feff}').unwrap_or(input),
            position: 0,
            state: State::Data,
            last_start_tag: Cow::Borrowed(""),
        }
    }

    pub(super) fn raw(&mut self, kind: RawKind) {
        self.state = State::Raw(kind);
    }

    pub(super) fn plaintext(&mut self) {
        self.state = State::Plaintext;
    }

    /// `foreign` tells whether the adjusted current node is outside the HTML
    /// namespace, where `<![CDATA[` opens a CDATA section.
    pub(super) fn next(&mut self, foreign: bool) -> Token<'a> {
        match self.state {
            State::Data => self.data(foreign),
            State::Raw(RawKind::ScriptData) => self.script(),
            State::Raw(kind) => self.raw_text(kind == RawKind::Rcdata),
            State::Plaintext => {
                let text = clean(&self.input[self.position..]);
                self.position = self.input.len();
                Text {
                    text,
                    ..Text::default()
                }
                .token()
            }
            State::Cdata => self.cdata(foreign),
        }
    }

    fn byte(&self) -> Option<u8> {
        self.input.as_bytes().get(self.position).copied()
    }

    fn skip_while(&mut self, condition: impl Fn(u8) -> bool) {
        while self.byte().is_some_and(&condition) {
            self.position += 1;
        }
    }

    /// The end of the run starting at the current position that `stop` does not end.
    fn run(&self, stop: impl Fn(u8) -> bool) -> usize {
        let bytes = &self.input.as_bytes()[self.position..];
        self.position
            + bytes
                .iter()
                .position(|&byte| stop(byte))
                .unwrap_or(bytes.len())
    }

    /// Reads a name from the current position through `from` and on up to
    /// whitespace or `stop`.
    fn read_name(&mut self, from: usize, stop: impl Fn(u8) -> bool) -> Cow<'a, str> {
        let start = self.position;
        self.position = from;
        self.skip_while(|byte| !space(byte) && !stop(byte));
        let text = &self.input[start..self.position];
        if text
            .bytes()
            .any(|byte| byte.is_ascii_uppercase() || byte == b'\0')
        {
            Cow::Owned(name(text))
        } else {
            Cow::Borrowed(text)
        }
    }

    fn newline(&mut self, text: &mut Text<'a>) {
        text.push("\n");
        self.position += 1;
        if self.byte() == Some(b'\n') {
            self.position += 1;
        }
    }

    fn data(&mut self, foreign: bool) -> Token<'a> {
        let bytes = self.input.as_bytes();
        let mut text = Text::default();
        loop {
            let start = self.position;
            let end = self.run(|byte| matches!(byte, b'<' | b'&' | b'\r' | b'\0'));
            text.source(self.input, start, end);
            self.position = end;
            match bytes.get(end) {
                None => break,
                Some(b'\r') => self.newline(&mut text),
                Some(b'\0') if text.text.is_empty() => {
                    self.position += 1;
                    return Token::NullCharacter;
                }
                Some(b'\0') => break,
                Some(b'&') => {
                    self.position += 1;
                    match self.reference(false) {
                        Some(reference) => text.reference(reference),
                        None => text.source(self.input, end, end + 1),
                    }
                }
                Some(_) => match bytes.get(end + 1) {
                    Some(b'/') if bytes.get(end + 2) == Some(&b'>') => self.position += 3,
                    Some(b'/') if end + 2 == bytes.len() => {
                        text.source(self.input, end, end + 2);
                        self.position += 2;
                    }
                    Some(&next)
                        if next.is_ascii_alphabetic() || matches!(next, b'!' | b'/' | b'?') =>
                    {
                        if !text.text.is_empty() {
                            break;
                        }
                        if let Some(token) = self.markup(foreign) {
                            return token;
                        }
                    }
                    _ => {
                        text.source(self.input, end, end + 1);
                        self.position += 1;
                    }
                },
            }
        }
        text.token()
    }

    /// Consumes the markup starting at `<`; tags cut off by the end of input yield nothing.
    fn markup(&mut self, foreign: bool) -> Option<Token<'a>> {
        let start = self.position;
        let rest = &self.input.as_bytes()[start + 1..];
        match rest[0] {
            b'!' if rest[1..].starts_with(b"--") => Some(self.comment(start + 4)),
            b'!' if rest
                .get(1..8)
                .is_some_and(|word| word.eq_ignore_ascii_case(b"doctype")) =>
            {
                self.position = start + 9;
                Some(Token::Doctype(self.doctype()))
            }
            b'!' if foreign && rest[1..].starts_with(b"[CDATA[") => {
                self.position = start + 9;
                self.state = State::Cdata;
                Some(self.cdata(foreign))
            }
            b'!' => Some(self.bogus_comment(start + 2)),
            b'?' => Some(self.bogus_comment(start + 1)),
            b'/' if !rest[1].is_ascii_alphabetic() => Some(self.bogus_comment(start + 2)),
            b'/' => {
                self.position = start + 2;
                self.tag(true).map(Token::Tag)
            }
            _ => {
                self.position = start + 1;
                self.tag(false).map(Token::Tag)
            }
        }
    }

    fn bogus_comment(&mut self, start: usize) -> Token<'a> {
        self.position = start;
        let end = self.run(|byte| byte == b'>');
        self.position = (end + 1).min(self.input.len());
        Token::Comment(clean(&self.input[start..end]))
    }

    /// The comment states after `<!--`: the text runs up to the closing `-->` or
    /// `--!>`, keeping the dashes that do not close it.
    fn comment(&mut self, start: usize) -> Token<'a> {
        #[derive(Clone, Copy)]
        enum Comment {
            Start,
            StartDash,
            Text,
            EndDash,
            End,
            EndBang,
        }
        let bytes = self.input.as_bytes();
        let mut state = Comment::Start;
        let mut position = start;
        let mut end = start;
        while let Some(&byte) = bytes.get(position) {
            position += 1;
            state = match (state, byte) {
                (Comment::Start | Comment::StartDash | Comment::End | Comment::EndBang, b'>') => {
                    break;
                }
                (Comment::Start, b'-') => Comment::StartDash,
                (Comment::StartDash | Comment::EndDash, b'-') => Comment::End,
                (Comment::Text, b'-') => Comment::EndDash,
                (Comment::End, b'!') => Comment::EndBang,
                (Comment::End, b'-') => {
                    end = position - 2;
                    Comment::End
                }
                (Comment::EndBang, b'-') => {
                    end = position - 1;
                    Comment::EndDash
                }
                _ => {
                    end = position;
                    Comment::Text
                }
            };
        }
        self.position = position;
        Token::Comment(clean(&self.input[start..end]))
    }

    /// CDATA text up to `]]>`; NUL characters still come as their own tokens.
    fn cdata(&mut self, foreign: bool) -> Token<'a> {
        let rest = &self.input[self.position..];
        let close = rest.find("]]>");
        let end = close.unwrap_or(rest.len());
        match rest[..end].find('\0') {
            Some(0) => {
                self.position += 1;
                Token::NullCharacter
            }
            Some(nul) => {
                self.position += nul;
                Text {
                    text: clean(&rest[..nul]),
                    ..Text::default()
                }
                .token()
            }
            None => {
                self.position += close.map_or(end, |close| close + 3);
                self.state = State::Data;
                match (Text {
                    text: clean(&rest[..end]),
                    ..Text::default()
                })
                .token()
                {
                    Token::Eof if close.is_some() => self.data(foreign),
                    token => token,
                }
            }
        }
    }

    /// The doctype states after `<!doctype`.
    fn doctype(&mut self) -> Doctype {
        #[derive(Clone, Copy)]
        enum Stage {
            BeforeName,
            Name(usize),
            AfterName,
            BeforeIdentifier(bool),
            Identifier {
                system: bool,
                quote: u8,
                start: usize,
            },
            AfterPublic,
            AfterSystem,
            Bogus,
        }
        let mut doctype = Doctype::default();
        let mut stage = Stage::BeforeName;
        loop {
            let Some(byte) = self.byte() else {
                match stage {
                    Stage::Bogus => {}
                    Stage::Name(start) => {
                        doctype.name = Some(name(&self.input[start..]));
                        doctype.force_quirks = true;
                    }
                    Stage::Identifier { system, start, .. } => {
                        doctype.set_identifier(system, clean(&self.input[start..]).into_owned());
                        doctype.force_quirks = true;
                    }
                    _ => doctype.force_quirks = true,
                }
                return doctype;
            };
            stage = match stage {
                Stage::BeforeName
                | Stage::AfterName
                | Stage::BeforeIdentifier(_)
                | Stage::AfterPublic
                | Stage::AfterSystem
                    if space(byte) =>
                {
                    self.position += 1;
                    stage
                }
                Stage::BeforeName if byte == b'>' => {
                    self.position += 1;
                    doctype.force_quirks = true;
                    return doctype;
                }
                Stage::BeforeName => {
                    self.position += 1;
                    Stage::Name(self.position - 1)
                }
                Stage::Name(start) if space(byte) || byte == b'>' => {
                    doctype.name = Some(name(&self.input[start..self.position]));
                    Stage::AfterName
                }
                Stage::Name(_) | Stage::Identifier { .. } | Stage::Bogus
                    if !matches!(stage, Stage::Identifier { quote, .. } if byte == quote)
                        && byte != b'>' =>
                {
                    self.position += 1;
                    stage
                }
                Stage::AfterName | Stage::AfterPublic | Stage::AfterSystem | Stage::Bogus
                    if byte == b'>' =>
                {
                    self.position += 1;
                    return doctype;
                }
                Stage::AfterName => {
                    let keyword = self.input.as_bytes().get(self.position..self.position + 6);
                    let public = keyword.is_some_and(|word| word.eq_ignore_ascii_case(b"public"));
                    let system = keyword.is_some_and(|word| word.eq_ignore_ascii_case(b"system"));
                    if public || system {
                        self.position += 6;
                        Stage::BeforeIdentifier(system)
                    } else {
                        doctype.force_quirks = true;
                        Stage::Bogus
                    }
                }
                Stage::BeforeIdentifier(system) if matches!(byte, b'"' | b'\'') => {
                    self.position += 1;
                    Stage::Identifier {
                        system,
                        quote: byte,
                        start: self.position,
                    }
                }
                Stage::AfterPublic if matches!(byte, b'"' | b'\'') => {
                    self.position += 1;
                    Stage::Identifier {
                        system: true,
                        quote: byte,
                        start: self.position,
                    }
                }
                Stage::BeforeIdentifier(_) if byte == b'>' => {
                    self.position += 1;
                    doctype.force_quirks = true;
                    return doctype;
                }
                Stage::Identifier { system, start, .. } => {
                    doctype.set_identifier(
                        system,
                        clean(&self.input[start..self.position]).into_owned(),
                    );
                    self.position += 1;
                    if byte == b'>' {
                        doctype.force_quirks = true;
                        return doctype;
                    }
                    if system {
                        Stage::AfterSystem
                    } else {
                        Stage::AfterPublic
                    }
                }
                Stage::BeforeIdentifier(_) | Stage::AfterPublic => {
                    doctype.force_quirks = true;
                    Stage::Bogus
                }
                Stage::AfterSystem => Stage::Bogus,
                Stage::Name(_) | Stage::Bogus => unreachable!("handled above"),
            };
        }
    }

    /// The tag states from the first character of the name.
    fn tag(&mut self, end: bool) -> Option<Tag<'a>> {
        let name = self.read_name(self.position, |byte| matches!(byte, b'/' | b'>'));
        let mut tag = Tag {
            end,
            local: LocalName::new(&name),
            name,
            self_closing: false,
            attributes: Vec::new(),
        };
        loop {
            self.skip_while(space);
            match self.byte()? {
                b'>' => break,
                b'/' => {
                    self.position += 1;
                    if self.byte()? == b'>' {
                        tag.self_closing = true;
                        break;
                    }
                }
                _ => {
                    let attribute = self.attribute()?;
                    if !tag
                        .attributes
                        .iter()
                        .any(|existing| existing.name == attribute.name)
                    {
                        tag.attributes.push(attribute);
                    }
                }
            }
        }
        self.position += 1;
        if !end {
            self.last_start_tag = tag.name.clone();
        }
        Some(tag)
    }

    /// Reads an attribute starting at its first character, which may be any
    /// character but whitespace, `/` and `>`.
    fn attribute(&mut self) -> Option<Attribute<'a>> {
        let mut attribute = Attribute {
            prefix: Prefix::None,
            name: self.read_name(self.position + 1, |byte| matches!(byte, b'/' | b'=' | b'>')),
            value: Cow::Borrowed(""),
        };
        self.skip_while(space);
        if self.byte()? != b'=' {
            return Some(attribute);
        }
        self.position += 1;
        self.skip_while(space);
        let mut value = Text::default();
        match self.byte() {
            Some(b'>') => return Some(attribute),
            Some(quote @ (b'"' | b'\'')) => {
                self.position += 1;
                self.attribute_value(&mut value, |byte| byte == quote)?;
                self.position += 1;
            }
            _ => self.attribute_value(&mut value, |byte| space(byte) || byte == b'>')?,
        }
        attribute.value = value.text;
        Some(attribute)
    }

    fn attribute_value(&mut self, value: &mut Text<'a>, stop: impl Fn(u8) -> bool) -> Option<()> {
        loop {
            let start = self.position;
            let end = self.run(|byte| stop(byte) || matches!(byte, b'&' | b'\r' | b'\0'));
            value.source(self.input, start, end);
            self.position = end;
            match self.byte()? {
                byte if stop(byte) => return Some(()),
                b'&' => {
                    self.position += 1;
                    match self.reference(true) {
                        Some(reference) => value.reference(reference),
                        None => value.source(self.input, end, end + 1),
                    }
                }
                b'\r' => self.newline(value),
                _ => {
                    value.push("\u{fffd}");
                    self.position += 1;
                }
            }
        }
    }

    /// Decodes the character reference after an `&`. Returns `None` when the `&`
    /// is literal text, and otherwise whether the reference misses its semicolon.
    fn reference(&mut self, in_attribute: bool) -> Option<(Reference, bool)> {
        let bytes = self.input.as_bytes();
        let start = self.position;
        if self.byte() == Some(b'#') {
            let (radix, digits) = match bytes.get(start + 1) {
                Some(b'x' | b'X') => (16, start + 2),
                _ => (10, start + 1),
            };
            let mut end = digits;
            let mut value = 0u32;
            while let Some(digit) = bytes
                .get(end)
                .and_then(|&byte| char::from(byte).to_digit(radix))
            {
                value = value.saturating_mul(radix).saturating_add(digit);
                end += 1;
            }
            if end == digits {
                return None;
            }
            let semicolon = bytes.get(end) == Some(&b';');
            self.position = end + usize::from(semicolon);
            return Some((Reference::Numeric(numeric(value)), !semicolon));
        }
        let end = self.run(|byte| !byte.is_ascii_alphanumeric());
        if end > start
            && bytes.get(end) == Some(&b';')
            && let Some(value) = lookup(&self.input[start..=end])
        {
            self.position = end + 1;
            return Some((Reference::Named(value), false));
        }
        // Only the legacy entities, two to six characters long, may omit the semicolon.
        for length in (2..=(end - start).min(6)).rev() {
            if let Some(value) = lookup(&self.input[start..start + length]) {
                let next = bytes.get(start + length);
                if in_attribute
                    && next.is_some_and(|&byte| byte == b'=' || byte.is_ascii_alphanumeric())
                {
                    return None;
                }
                self.position = start + length;
                return Some((Reference::Named(value), true));
            }
        }
        None
    }

    /// RCDATA and RAWTEXT, which only an end tag for their element closes.
    fn raw_text(&mut self, references: bool) -> Token<'a> {
        let bytes = self.input.as_bytes();
        let mut text = Text::default();
        loop {
            let start = self.position;
            let end = self
                .run(|byte| matches!(byte, b'<' | b'\r' | b'\0') || (references && byte == b'&'));
            text.source(self.input, start, end);
            self.position = end;
            match bytes.get(end) {
                None => break,
                Some(b'\r') => self.newline(&mut text),
                Some(b'\0') => {
                    text.push("\u{fffd}");
                    self.position += 1;
                }
                Some(b'&') => {
                    self.position += 1;
                    match self.reference(false) {
                        Some(reference) => text.reference(reference),
                        None => text.source(self.input, end, end + 1),
                    }
                }
                Some(_) if self.end_tag_at(end) => {
                    self.state = State::Data;
                    break;
                }
                Some(_) => {
                    text.source(self.input, end, end + 1);
                    self.position += 1;
                }
            }
        }
        match text.token() {
            Token::Eof if self.state == State::Data => self.data(false),
            token => token,
        }
    }

    /// Whether an end tag for the element that switched to raw text starts at `at`.
    fn end_tag_at(&self, at: usize) -> bool {
        let bytes = self.input.as_bytes();
        let name = self.last_start_tag.as_bytes();
        let end = at + 2 + name.len();
        bytes.get(at + 1) == Some(&b'/')
            && bytes
                .get(at + 2..end)
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
            && bytes
                .get(end)
                .is_some_and(|&byte| space(byte) || matches!(byte, b'/' | b'>'))
    }

    /// Script data, whose escaped and double-escaped states decide which
    /// `</script>` closes the element.
    fn script(&mut self) -> Token<'a> {
        #[derive(Clone, Copy)]
        enum Script {
            Data,
            Escaped,
            EscapedDash,
            EscapedDashDash,
            Double,
            DoubleDash,
            DoubleDashDash,
        }
        let bytes = self.input.as_bytes();
        let letters = |from: usize| {
            from + bytes[from..]
                .iter()
                .take_while(|byte| byte.is_ascii_alphabetic())
                .count()
        };
        let delimiter = |at: usize| {
            bytes
                .get(at)
                .is_some_and(|&byte| space(byte) || matches!(byte, b'/' | b'>'))
        };
        let start = self.position;
        let mut state = Script::Data;
        let mut position = start;
        let end = loop {
            let Some(&byte) = bytes.get(position) else {
                break bytes.len();
            };
            position += 1;
            state = match (state, byte) {
                (Script::Data, b'<') => {
                    if self.end_tag_at(position - 1) {
                        break position - 1;
                    }
                    if bytes[position..].starts_with(b"!--") {
                        position += 3;
                        Script::EscapedDashDash
                    } else {
                        Script::Data
                    }
                }
                (Script::Escaped | Script::EscapedDash | Script::EscapedDashDash, b'<') => {
                    if self.end_tag_at(position - 1) {
                        break position - 1;
                    }
                    let name_end = letters(position);
                    if name_end > position && delimiter(name_end) {
                        let script = bytes[position..name_end].eq_ignore_ascii_case(b"script");
                        position = name_end + 1;
                        if script {
                            Script::Double
                        } else {
                            Script::Escaped
                        }
                    } else {
                        position = name_end;
                        Script::Escaped
                    }
                }
                (Script::Double | Script::DoubleDash | Script::DoubleDashDash, b'<') => {
                    if bytes.get(position) == Some(&b'/') {
                        let name_end = letters(position + 1);
                        if delimiter(name_end) {
                            let script =
                                bytes[position + 1..name_end].eq_ignore_ascii_case(b"script");
                            position = name_end + 1;
                            if script {
                                Script::Escaped
                            } else {
                                Script::Double
                            }
                        } else {
                            position = name_end;
                            Script::Double
                        }
                    } else {
                        Script::Double
                    }
                }
                (Script::Escaped, b'-') => Script::EscapedDash,
                (Script::EscapedDash | Script::EscapedDashDash, b'-') => Script::EscapedDashDash,
                (Script::EscapedDashDash | Script::DoubleDashDash, b'>') => Script::Data,
                (Script::Escaped | Script::EscapedDash | Script::EscapedDashDash, _) => {
                    Script::Escaped
                }
                (Script::Double, b'-') => Script::DoubleDash,
                (Script::DoubleDash | Script::DoubleDashDash, b'-') => Script::DoubleDashDash,
                (Script::Double | Script::DoubleDash | Script::DoubleDashDash, _) => Script::Double,
                (Script::Data, _) => Script::Data,
            };
        };
        self.position = end;
        let token = Text {
            text: clean(&self.input[start..end]),
            ..Text::default()
        }
        .token();
        if end == bytes.len() {
            return token;
        }
        self.state = State::Data;
        match token {
            Token::Eof => self.data(false),
            token => token,
        }
    }
}
