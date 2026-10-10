mod transfer;

use charset::Charset;
use mailparse::{
    MailHeader, MailHeaderMap, MailParseError, ParsedContentDisposition, ParsedContentType,
};
use memchr::{memchr, memmem};
use std::borrow::Cow;

pub struct Part<'a> {
    pub headers: Vec<MailHeader<'a>>,
    pub ctype: ParsedContentType,
    content: &'a [u8],
    pub subparts: Vec<Part<'a>>,
}

pub fn parse(bytes: &[u8]) -> Result<Part<'_>, MailParseError> {
    parse_part(bytes, false)
}

fn parse_part(bytes: &[u8], in_digest: bool) -> Result<Part<'_>, MailParseError> {
    let (headers, body) = mailparse::parse_headers(bytes)?;
    let ctype = headers
        .get_first_value("Content-Type")
        .map(|value| mailparse::parse_content_type(&value))
        .unwrap_or_else(|| ParsedContentType {
            mimetype: if in_digest {
                "message/rfc822"
            } else {
                "text/plain"
            }
            .into(),
            ..Default::default()
        });
    let mut part = Part {
        headers,
        ctype,
        content: &bytes[body..],
        subparts: Vec::new(),
    };
    if let Some(boundary) = part.ctype.params.get("boundary")
        && part.ctype.mimetype.starts_with("multipart/")
        && bytes.len() > body
    {
        let boundary = format!("--{boundary}");
        let finder = memmem::Finder::new(&boundary);
        let in_digest = part.ctype.mimetype == "multipart/digest";
        if let Some(first) = line_start(bytes, body, &finder) {
            part.content = &bytes[body..trim_newline(bytes, body, first)];
            let mut after = first + boundary.len();
            while let Some(start) = memchr(b'\n', &bytes[after..]).map(|at| after + at + 1) {
                let next = line_start(bytes, start, &finder);
                let end = next.map_or(bytes.len(), |next| trim_newline(bytes, start, next));
                part.subparts
                    .push(parse_part(&bytes[start..end], in_digest)?);
                after = next.map_or(bytes.len(), |next| next + boundary.len());
                if after + 2 > bytes.len() || bytes[after..].starts_with(b"--") {
                    break;
                }
            }
        }
    }
    Ok(part)
}

fn line_start(bytes: &[u8], start: usize, finder: &memmem::Finder) -> Option<usize> {
    let mut from = start;
    loop {
        let found = from + finder.find(&bytes[from..])?;
        if found == start || bytes[found - 1] == b'\n' {
            return Some(found);
        }
        from = found + 1;
    }
}

fn trim_newline(bytes: &[u8], start: usize, end: usize) -> usize {
    match &bytes[start..end] {
        [.., b'\r', b'\n'] => end - 2,
        [.., b'\n'] => end - 1,
        _ => end,
    }
}

impl Part<'_> {
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers.get_first_value(name)
    }

    pub fn disposition(&self) -> ParsedContentDisposition {
        self.header("Content-Disposition")
            .map(|value| mailparse::parse_content_disposition(&value))
            .unwrap_or_default()
    }

    fn transfer_encoding(&self) -> Option<String> {
        self.header("Content-Transfer-Encoding")
            .map(|value| value.to_lowercase())
    }

    pub fn body_raw(&self) -> Result<Cow<'_, [u8]>, MailParseError> {
        Ok(match self.transfer_encoding().as_deref() {
            Some("base64") => Cow::Owned(transfer::base64(self.content)?),
            Some("quoted-printable") => Cow::Owned(transfer::quoted_printable(self.content)),
            _ => Cow::Borrowed(self.content),
        })
    }

    pub fn body_len(&self) -> Result<usize, MailParseError> {
        if self.transfer_encoding().as_deref() == Some("base64")
            && let Some(len) = transfer::base64_len(self.content)
        {
            return Ok(len);
        }
        Ok(self.body_raw()?.len())
    }

    pub fn body(&self) -> Result<String, MailParseError> {
        let bytes = self.body_raw()?;
        let text = match Charset::for_label(self.ctype.charset.as_bytes()) {
            Some(charset) => charset.decode(&bytes).0,
            None => charset::decode_ascii(&bytes),
        };
        Ok(text.into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use mailparse::ParsedMail;
    use std::collections::BTreeMap;

    #[derive(Debug, PartialEq)]
    struct Shape {
        headers: Vec<(Vec<u8>, String)>,
        mimetype: String,
        charset: String,
        params: BTreeMap<String, String>,
        disposition: String,
        raw: Result<Vec<u8>, String>,
        len: Result<usize, String>,
        body: Result<String, String>,
        subparts: Vec<Shape>,
    }

    impl From<&Part<'_>> for Shape {
        fn from(part: &Part<'_>) -> Self {
            Self {
                headers: headers(&part.headers),
                mimetype: part.ctype.mimetype.clone(),
                charset: part.ctype.charset.clone(),
                params: part.ctype.params.clone(),
                disposition: format!("{:?}", part.disposition()),
                raw: part
                    .body_raw()
                    .map(Cow::into_owned)
                    .map_err(|error| error.to_string()),
                len: part.body_len().map_err(|error| error.to_string()),
                body: part.body().map_err(|error| error.to_string()),
                subparts: part.subparts.iter().map(Self::from).collect(),
            }
        }
    }

    impl From<&ParsedMail<'_>> for Shape {
        fn from(mail: &ParsedMail<'_>) -> Self {
            Self {
                headers: headers(&mail.headers),
                mimetype: mail.ctype.mimetype.clone(),
                charset: mail.ctype.charset.clone(),
                params: mail.ctype.params.clone(),
                disposition: format!("{:?}", mail.get_content_disposition()),
                raw: mail.get_body_raw().map_err(|error| error.to_string()),
                len: mail
                    .get_body_raw()
                    .map(|bytes| bytes.len())
                    .map_err(|error| error.to_string()),
                body: mail.get_body().map_err(|error| error.to_string()),
                subparts: mail.subparts.iter().map(Self::from).collect(),
            }
        }
    }

    fn headers(headers: &[MailHeader<'_>]) -> Vec<(Vec<u8>, String)> {
        headers
            .iter()
            .map(|header| (header.get_key_raw().to_vec(), header.get_value()))
            .collect()
    }

    fn shape(raw: &[u8]) -> Result<Shape, String> {
        let shape = parse(raw)
            .as_ref()
            .map(Shape::from)
            .map_err(|error| error.to_string());
        let expected = mailparse::parse_mail(raw)
            .as_ref()
            .map(Shape::from)
            .map_err(|error| error.to_string());
        assert_eq!(shape, expected, "{}", String::from_utf8_lossy(raw));
        shape
    }

    fn body(encoding: &str, charset: &str, body: &[u8]) -> Shape {
        let mut raw = format!(
            "Content-Type: text/plain; charset={charset}\r\nContent-Transfer-Encoding: {encoding}\r\n\r\n"
        )
        .into_bytes();
        raw.extend_from_slice(body);
        shape(&raw).unwrap()
    }

    #[test]
    fn decodes_transfer_encodings_with_mailparse_errors() {
        for (encoding, encoded, decoded) in [
            (
                "Quoted-Printable",
                &b"soft=\r\nbreak =C3=A9 =ZZ\r\n"[..],
                Ok(&b"softbreak \xc3\xa9 =ZZ\r\n"[..]),
            ),
            ("BASE64", b"aGk=aGk=\r\n", Ok(b"hihi")),
            ("base64", b" aGVs\tbG8=\x0c\r\n", Ok(b"hello")),
            ("base64", b"aGl=", Ok(b"hi")),
            (
                "base64",
                b"aGVsbG8\r\n",
                Err("Base64 decode error: invalid length at 4"),
            ),
            (
                "base64",
                b"aG!s\r\n",
                Err("Base64 decode error: invalid symbol at 2"),
            ),
            (
                "base64",
                b"a===\r\n",
                Err("Base64 decode error: invalid padding length at 1"),
            ),
            ("7bit", b"=41", Ok(b"=41")),
            ("x-unknown", b"=41", Ok(b"=41")),
        ] {
            let shape = body(encoding, "utf-8", encoded);
            assert_eq!(
                shape.raw.as_deref().map_err(String::as_str),
                decoded
                    .map(<[u8]>::to_vec)
                    .as_deref()
                    .map_err(|error| *error)
            );
        }
    }

    #[test]
    fn decodes_generated_bodies_like_mailparse() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move |limit: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as usize % limit
        };
        let alphabet = b"AQgw+/09=_\r\n \t\x0c\x00\xe9";
        for _ in 0..3000 {
            let random: Vec<u8> = (0..next(32))
                .map(|_| alphabet[next(alphabet.len())])
                .collect();
            let bytes: Vec<u8> = (0..next(40)).map(|_| next(256) as u8).collect();
            let mut canonical = Vec::new();
            for symbol in base64::engine::general_purpose::STANDARD
                .encode(bytes)
                .bytes()
            {
                if next(8) == 0 {
                    canonical.extend_from_slice([&b"\r\n"[..], b"\n", b"\r", b" "][next(4)]);
                }
                canonical.push(symbol);
            }
            let mut damaged = canonical.clone();
            damaged.truncate(next(damaged.len() + 1));
            damaged.extend((0..next(3)).map(|_| alphabet[next(alphabet.len())]));
            for encoding in ["base64", "quoted-printable"] {
                for input in [&random, &canonical, &damaged] {
                    body(encoding, "utf-8", input);
                }
            }
        }
    }

    #[test]
    fn decodes_legacy_charsets() {
        for (encoding, charset, encoded, decoded) in [
            ("8bit", "koi8-r", &b"\xf0\xd2\xc9\xd7\xc5\xd4"[..], "Привет"),
            (
                "8bit",
                "windows-1251",
                b"\xcf\xf0\xe8\xe2\xe5\xf2",
                "Привет",
            ),
            (
                "quoted-printable",
                "\"Windows-1251\"",
                b"=CF=F0=E8=E2=E5=F2",
                "Привет",
            ),
            (
                "7bit",
                "iso-2022-jp",
                b"\x1b$B$3$s$K$A$O\x1b(B",
                "こんにちは",
            ),
            ("8bit", "iso-8859-2", b"\xa3\xf3d\xbc", "Łódź"),
            ("8bit", "gbk", b"\xc4\xe3\xba\xc3", "你好"),
            ("8bit", "ms950", b"\xa7\x41\xa6\x6e", "你好"),
            ("8bit", "euc-kr", b"\xbe\xc8\xb3\xe7", "안녕"),
            ("8bit", "shift_jis", b"\x82\xb1\x82\xf1", "こん"),
            ("7bit", "utf-7", b"+AGEAYgBj-", "abc"),
            ("binary", "utf-8", b"\xff\xfeh\x00i\x00", "hi"),
            ("8bit", "utf-8", b"\xef\xbb\xbfbom", "bom"),
            ("8bit", "x-unknown", b"caf\xe9", "caf\u{fffd}"),
            ("8bit", "csiso2022kr", b"text", "\u{fffd}"),
            ("base64", "utf-8", b"w6k=", "é"),
        ] {
            assert_eq!(body(encoding, charset, encoded).body.unwrap(), decoded);
        }
    }

    #[test]
    fn splits_multipart_trees_like_mailparse() {
        let nested = shape(b"Content-Type: multipart/mixed; boundary=\"outer\"\r\n\r\npreamble\r\n--outer\r\nContent-Type: multipart/digest; boundary=inner\r\n\r\n--inner\r\n\r\nSubject: digested\r\n\r\n--inner--\r\n--outer \r\nContent-Type: text/plain\r\n\r\nnot --outer at line start\r\n--outer\r\nContent-Type: text/html\r\n\r\nunterminated").unwrap();
        assert_eq!(nested.raw.unwrap(), b"preamble");
        assert_eq!(nested.subparts.len(), 3);
        assert_eq!(nested.subparts[0].subparts[0].mimetype, "message/rfc822");
        assert_eq!(
            nested.subparts[1].body.as_deref().unwrap(),
            "not --outer at line start"
        );
        for raw in [
            &b"Content-Type: multipart/alternative; boundary=bound\n\n--bound\nContent-Type: text/plain\n\nplain\n--bound\n\nno headers\n--bound--\nepilogue"[..],
            b"Content-Type: multipart/mixed; boundary=a\r\n\r\n--ab\r\n\r\nsuffix\r\n--a\r\n\r\nreal\r\n--a--",
            b"Content-Type: multipart/mixed; boundary*0=a; boundary*1=b\r\n\r\n--ab\r\n\r\njoined\r\n--ab--",
            b"Content-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\n--x\r\n\r\n--x--",
            b"Content-Type: multipart/mixed; boundary=x\r\n\r\nno boundary at all",
            b"Content-Type: multipart/mixed; boundary=x\r\n\r\n--x",
            b"Content-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\n",
            b"Content-Type: multipart/mixed\r\n\r\n--x\r\n\r\nmissing parameter",
            b"Content-Type: text/plain; boundary=x\r\n\r\n--x\r\n\r\nnot multipart",
            b"Content-Type: multipart/mixed; boundary=x",
            b"",
        ] {
            shape(raw).unwrap();
        }
    }

    #[test]
    fn reports_header_errors_like_mailparse() {
        for raw in [
            &b" Overhang: x\r\n\r\n"[..],
            b"A: b\r\n\rbody",
            b"Content-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\n Overhang: x\r\n\r\n--x--",
        ] {
            assert!(shape(raw).is_err());
        }
        let folded = shape(b"NoColon\r\nKey:  value \r\n folded\r\n\r\n").unwrap();
        assert_eq!(folded.headers[1].1, "value  folded");
    }
}
