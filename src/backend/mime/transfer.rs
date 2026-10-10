use mailparse::MailParseError;
use memchr::memchr;
use std::borrow::Cow;

pub fn base64(encoded: &[u8]) -> Result<Vec<u8>, MailParseError> {
    let symbols: Vec<u8> = encoded
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    Ok(data_encoding::BASE64_MIME_PERMISSIVE.decode(&symbols)?)
}

// Canonical base64 always decodes, so its length follows from the symbol count
// without decoding. Anything else returns None and has to be decoded. Counting
// in u8 lanes per 64-byte chunk keeps the scan vectorized.
pub fn base64_len(encoded: &[u8]) -> Option<usize> {
    let encoded = encoded.trim_ascii_end();
    let padding = encoded
        .iter()
        .rev()
        .take_while(|&&byte| byte == b'=')
        .count();
    let encoded = &encoded[..encoded.len() - padding];
    let count = |chunk: &[u8], matches: fn(u8) -> bool| {
        usize::from(
            chunk
                .iter()
                .map(|&byte| u8::from(matches(byte)))
                .sum::<u8>(),
        )
    };
    let mut symbols = 0;
    let mut line_breaks = 0;
    for chunk in encoded.chunks(64) {
        symbols += count(chunk, |byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/')
        });
        line_breaks += count(chunk, |byte| matches!(byte, b'\r' | b'\n'));
    }
    (symbols + line_breaks == encoded.len() && padding <= 2 && (symbols + padding) % 4 == 0)
        .then_some(symbols * 3 / 4)
}

// Decodes like quoted_printable's robust mode, but copies the runs between
// escapes in bulk instead of byte by byte.
pub fn quoted_printable(encoded: &[u8]) -> Vec<u8> {
    let printable = |byte: &u8| matches!(byte, b'\t' | b'\r' | b'\n' | b' '..=b'~');
    let text: Cow<[u8]> = if encoded.iter().fold(true, |all, byte| all & printable(byte)) {
        Cow::Borrowed(encoded)
    } else {
        Cow::Owned(encoded.iter().copied().filter(printable).collect())
    };
    let hex = |byte: u8| char::from(byte).to_digit(16);
    let mut decoded = Vec::with_capacity(text.len());
    let mut line_break = false;
    let mut equals = memchr(b'=', &text);
    let mut start = 0;
    while start < text.len() {
        let newline = memchr(b'\n', &text[start..]).map(|at| start + at);
        let mut end = newline.unwrap_or(text.len());
        while end > start && matches!(text[end - 1], b' ' | b'\t' | b'\r') {
            end -= 1;
        }
        if line_break {
            decoded.extend_from_slice(b"\r\n");
        }
        line_break = true;
        let mut index = start;
        loop {
            if equals.is_some_and(|at| at < index) {
                equals = memchr(b'=', &text[index..]).map(|at| index + at);
            }
            let Some(at) = equals.filter(|&at| at < end) else {
                decoded.extend_from_slice(&text[index..end]);
                break;
            };
            decoded.extend_from_slice(&text[index..at]);
            match text[at + 1..end] {
                [] => {
                    line_break = false;
                    break;
                }
                [_] => {
                    decoded.extend_from_slice(&text[at..end]);
                    break;
                }
                [high, low, ..] => match (hex(high), hex(low)) {
                    (Some(high), Some(low)) => decoded.push((high << 4 | low) as u8),
                    _ => decoded.extend_from_slice(&text[at..at + 3]),
                },
            }
            index = at + 3;
        }
        start = newline.map_or(text.len(), |newline| newline + 1);
    }
    if line_break && text.ends_with(b"\n") {
        decoded.extend_from_slice(b"\r\n");
    }
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_quoted_printable_lines_escapes_and_soft_breaks() {
        for (encoded, decoded) in [
            (
                &b"soft=\r\nbreak =C3=A9 =c3=a9 =ZZ =4\r\ntrailing   \r\n"[..],
                &b"softbreak \xc3\xa9 \xc3\xa9 =ZZ =4\r\ntrailing\r\n"[..],
            ),
            (
                b"\n\n\n\n\n\n\n\n\na=\n\nb\n",
                b"\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\na\r\nb\r\n",
            ),
            (b"caf\xc3\xa9\x00 =41\x7f\x1b\r\n", b"caf A\r\n"),
            (b"soft= \t\r\nbreak", b"softbreak"),
            (b"==41=\rx\r\n=4", b"==41=\rx\r\n=4"),
            (b"tab\t=09\t\r\n\r\nend=", b"tab\t\t\r\n\r\nend"),
            (b"=E2=82\r\n=AC", b"\xe2\x82\r\n\xac"),
            (b"=\n=\n", b""),
            (b"\r\n", b"\r\n"),
            (b"", b""),
        ] {
            assert_eq!(quoted_printable(encoded), decoded);
        }
    }

    #[test]
    fn measures_canonical_base64_without_decoding() {
        for encoded in [
            &b"aGVsbG8gd29ybGQ=\r\n"[..],
            b"aGVs\r\nbG8=",
            b"aGk=\r\n\r\n",
            b"YQ==",
            b"",
        ] {
            assert_eq!(base64_len(encoded), Some(base64(encoded).unwrap().len()));
        }
        for unusual in [
            &b"aGk=aGk="[..],
            b"aGVs bG8=",
            b"aGVsbG8",
            b"a===",
            b"aG!s",
            b"aGk=\r\n=",
        ] {
            assert_eq!(base64_len(unusual), None);
        }
    }
}
