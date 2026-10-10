//! A fixed-size summary of the byte pairs in some texts.

const WORDS: usize = 32;

pub(super) struct Pairs([u64; WORDS]);

impl Pairs {
    pub(super) fn of<'a>(texts: impl IntoIterator<Item = &'a str>) -> Self {
        let mut words = [0; WORDS];
        for text in texts {
            for pair in text.as_bytes().windows(2) {
                let bit = bucket(pair[0], pair[1]);
                words[bit / 64] |= 1 << (bit % 64);
            }
        }
        Self(words)
    }

    /// False when no summarized text can contain the text summarized as `query`.
    pub(super) fn covers(&self, query: &Self) -> bool {
        self.0
            .iter()
            .zip(&query.0)
            .all(|(text, query)| text & query == *query)
    }
}

fn bucket(first: u8, second: u8) -> usize {
    const BITS: u32 = (WORDS * 64).ilog2();
    (u32::from(u16::from_le_bytes([first, second])).wrapping_mul(0x9E37_79B9) >> (32 - BITS))
        as usize
}
