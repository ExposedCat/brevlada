//! Differential tests against the scraper-based code that read email HTML
//! before: everything the mail views take from HTML must stay identical on
//! generated tag soup and, when `BREVLADA_CORPUS` names a directory of `.eml`
//! files, on every HTML body in it.
use super::Html;
use crate::{
    backend::parser,
    components,
    models::{Message, schema, search},
};

/// The tag soup tokens of the Bun port's fuzz corpus.
const BUN_TOKENS: &str = "<p>|</p>|<div>|</div>|<b>|</b>|<br>|<table>|<tr>|<td>|</td>|</table>|<li>|<ul>|</ul>|<head>|</head>|<body>|</body>|<html>|</html>|<title>|</title>|<style>|</style>|<script>|</script>|<template>|</template>|<noscript>|</noscript>|<textarea>|</textarea>|<!-- c -->|&amp;|&nbsp;|&copy|&notit;|&#x41;|text|word| |\n|<img src='https://x'>|<a href='https://t'>|</a>|<span>|</span>|<h1>|</h1>|<pre>|</pre>|<meta itemprop=x content=y>|<div itemscope itemtype='https://schema.org/Thing'>|<span itemprop='name'>|<select>|<option>|<svg>|</svg>|<font>|</font>|<i>|</i>|<form>|</form>|<button>|<xmp>|<iframe>|</iframe>|<caption>|<col>|<tbody>|<th>|<dd>|<dt>|<dl>|<h2>|<blockquote>|</blockquote>|<hr>|<address>|<image src='https://img'>|<link itemprop=u href=https://l>|<time itemprop=t datetime=2026>|</br>|<meta charset=utf-8>";

const BUN_HANDMADE: [&str; 17] = [
    "<html><head><title>T</title><body><p>Body</p>",
    "<title>Title</title><p>Body</p>",
    "<p>Body</p></body></html><html><head><title>Second</title></head><body>B2",
    "<head><style>x</style></head>\n<body class=a><table><tr><td>Top</td></tr></table></div><p>Bottom</p></body>",
    "<p>Hello &amp; &#x1F600; &nbsp; &zwnj; &copy2024 <b>w</b></p>",
    "<ul><li>a<li>b</ul><dl><dt>x<dd>y</dl>",
    "<table><tr><td>a<td>b<tr><td>c</table>text after",
    "<div itemscope itemtype='https://schema.org/ParcelDelivery'><div itemprop=carrier itemscope itemtype='https://schema.org/Organization'><meta itemprop=name content=Courier></div><span itemprop=name>  Outer  </span></div>",
    "<script type='application/ld+json'>{\"@type\":\"Event\"}</script><p>after</p>",
    "<b>Tracking number:</b>XX12345678<i>Carrier:</i><i>DHL</i>",
    "<p>one<p>two<div>three</div>four",
    "<a href='https://t'>Track <b>your</b> order</a>",
    "<img srcset='a.png 1x, https://x 2x'><source srcset='//cdn'><video poster='cid:x'></video>",
    "<noscript><img src=https://x></noscript>",
    "<textarea>&lt;b&gt;</textarea><title>&amp;</title>",
    "plain text only",
    "<!doctype html><html lang=en><head><meta charset=utf-8><title>News</title><style>td{}</style></head><body><center><table width=600><tr><td>Hi&nbsp;there</td></tr></table></center></body></html>",
];

/// Classic hard cases for tree construction and tokenization.
const TRICKY: &[&str] = &[
    "<img src='&#104;ttps://x'>",
    "<img src='a&#44;https://x'>",
    "<img srcset='&nbsp;//x 1x'>",
    "<img src='x?a=1&amp;b=2'><img src=&quot;https://x>",
    "<img src=x src=https://y>",
    "<IMG SRC=HTTPS://x><img srcset=' https://x 1x'>",
    "<select><img src=https://x></select>",
    "<select><source srcset=//x><video poster=cid:a></select>",
    "<select><template><img src=https://x></template></select>",
    "<select><textarea><img src=https://x>",
    "<table><select><img src=https://x></select></table>",
    "<svg><image src=https://x></svg>",
    "<svg><foreignObject><image src=https://x>",
    "<math><mi><image srcset=//x>",
    "<svg><source srcset=https://x><video poster=//x>",
    "<textarea><img src=https://x></textarea><title><img src=https://x>",
    "<noscript><img src=https://x></noscript><xmp><img src=//x></xmp>",
    "<iframe><img src=https://x></iframe><noembed><img src=//x><noframes><img src=//x>",
    "<!--<img src=https://x>--><style><img src=https://x></style>",
    "<script><img src=https://x></script><plaintext><img src=https://x>",
    "<template><img src=https://x></template>",
    "<table><img src=https://x><tr><td>",
    "<frameset><img src=https://x>",
    "<script type='APPLICATION/LD+JSON'>{\"@type\":\"Thing\"}</script>",
    "<script type='application/ld&#43;json'>{\"a\":1}</script>",
    "<svg><script type='application/ld+json'>{\"a\":\"&amp;\"}</script></svg>",
    "<svg><script type='APPLICATION/LD+JSON'>{\"a\":1}</script></svg>",
    "<svg><script type='application/ld+json'>{\"a\":<b>1</b>}</script></svg>",
    "<script type=application/ld+json>{\"a\":\"\0\"}</script>",
    "<script type=application/ld+json>{\"a\":\"\r\n\"}</script><script type=application/ld+json>\r{\"b\":1}\r</script>",
    "<script type=application/ld+json></script><script>{\"c\":1}</script>",
    "<script type=application/ld+json>{\"a\":\"</scr\"}</script>",
    "<script type=application/ld+json><!--<script>{\"a\":1}</script>--></script>",
    "<select><script type=application/ld+json>{\"a\":1}</script></select>",
    "<textarea><script type=application/ld+json>{\"a\":1}</script></textarea>",
    "<template><script type=application/ld+json>{\"a\":2}</script></template>",
    "<script type=application/ld+json>{\"a\":1}",
    "<math><annotation-xml encoding=text/html><script type=application/ld+json>{\"a\":\"&amp;\"}</script>",
    "<math><mtext><script type=application/ld+json>{\"a\":\"&amp;\"}</script><img src=//x>",
    "<svg><foreignObject><script type=application/ld+json>{\"a\":\"&amp;\"}</script><svg><script type=application/ld+json>{}</script>",
    "<math><annotation-xml encoding=text/html><image src=https://x></annotation-xml></math>",
    "<math><annotation-xml encoding=TEXT/HTML><p><image src=https://x>",
    "<p ITEMSCOPE itemtype=https://schema.org/Thing><span itemprop=name>x</span></p>",
    "<a><p>X<a>Y</a>Z</p></a>",
    "<b><p>1</b>2</p>",
    "<a><b><i><u><s><div>x</a>y",
    "<a><b><i><u><s><em><strong><div>x</a>y</div>",
    "<p><b><b><b><b></p>x",
    "<p><b class=x><b class=x><b class=x><b class=x></p>x",
    "<p><b><b class=y><b><b><b></p>x",
    "<table><a>x<tr><td>y</a>z</table>",
    "<table>x<tr>y<td>z</table>",
    "<table><b>x</b></table>",
    "<div><table>text<tr>",
    "<table> <tr> </tr> x </table>",
    "<pre>&#10x",
    "<pre>&#10;x",
    "<pre>&#x0ax",
    "<pre>&#10",
    "<textarea>&#10x</textarea>",
    "<listing>&NewLine;x",
    "<pre>\r\nx",
    "<pre>\0\nx",
    "<script><!--<script></script>x</script>y",
    "<script><!--x-->y</script>z",
    "<script><!-- <script> --></script>z",
    "<script><!--<script>--></script>z</script>",
    "<script><!--<script></scripx></script>z",
    "<script><!--<script>-</script>--</script>",
    "<template><thead><caption>",
    "<template><tbody><caption>x",
    "<template><tfoot></tfoot><col>",
    "<template><col><img src=x></template>",
    "<template><td><tr>",
    "<frameset><frame></frameset><noframes>x</noframes>",
    "<html><frameset>",
    "<p>x<frameset>",
    "<frameset><p>x<img src=https://x>",
    "<head></head><frameset><frameset></frameset></frameset></html><!--x-->",
    "<body><frameset>",
    "<svg><foreignObject><![CDATA[a\0b]]></foreignObject></svg>",
    "<math><mi><![CDATA[x]]></mi></math>",
    "<svg><![CDATA[a\0b]]></svg>",
    "<select><option>a<option>b<optgroup>c</select>",
    "<select><input>",
    "<select><hr><option>",
    "<p><select><p>",
    "<table><caption><table>",
    "<table><colgroup><col></colgroup>x",
    "<table><td>",
    "<!DOCTYPE html><p><table>",
    "<p><table>",
    "<b>1<p>2</b>3</p>",
    "<i><b><p>x</i>y</b>",
    "<nobr>a<nobr>b<nobr>c",
    "<a href=1><a href=2>x",
    "<button><button>",
    "<form><form>",
    "<table><form><input type=hidden><input>",
    "<ruby>a<rb>b<rt>c<rp>d<rtc>e</ruby>",
    "<dl><dt>a<dd>b<dt>c</dl>",
    "<li>a<div><li>b",
    "<h1><h2>x</h1>",
    "<image><isindex>",
    "<svg><desc><p>x</p></desc><title><b>t</b></title></svg>",
    "<math><annotation-xml encoding=text/html><div>x</div></annotation-xml></math>",
    "<math><annotation-xml><svg><p>x",
    "<svg><font color=red>x</font></svg>",
    "<svg></p></svg>",
    "<svg><br></svg>",
    "<math><mglyph/><malignmark/></math>",
    "<svg><clippath><lineargradient/></clippath><foreignobject><svg></svg></foreignobject></svg>",
    "<svg xmlns:xlink=x xlink:href=y XML:LANG=z xmlns=w>",
    "\0",
    "<p>\0</p>",
    "<table>\0x</table>",
    "<!---->",
    "<!--a--!>b",
    "<!--a--!->b-->",
    "</>x",
    "</ x>y",
    "<?x>",
    "<style>a</style><head><style>b</style></head><body><style>c</style><template><style>d</style></template>",
    "<html><head><template><style>x</style></template></head><body>",
    "<noscript><style>x</style></noscript>",
    "<body bgcolor=x><body text=y>",
    "<html a=1><html b=2 a=3>",
    "<template><body class=x><html class=y>",
];

/// Doctypes for each quirks mode outcome, and malformed ones.
const DOCTYPES: &[&str] = &[
    "",
    "<!DOCTYPE html>",
    "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\">",
    "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \"http://www.w3.org/TR/html4/loose.dtd\">",
    "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">",
    "<!doctype html public \"-//W3C//DTD HTML 4.0 Transitional//EN\">",
    "<!DOCTYPE html PUBLIC \"HTML\">",
    "<!DOCTYPE html SYSTEM \"http://www.IBM.com/data/dtd/v11/ibmxhtml1-transitional.dtd\">",
    "<!DOCTYPE html SYSTEM 'about:legacy-compat'>",
    "<!DOCTYPE html PUBLIC>",
    "<!DOCTYPE html PUBLIC \"-//W3C//DTD HTML 4.01//EN\" \"x\" junk>",
    "<!DOCTYPE html PUBLIC 'x'\"y\">",
    "<!DOCTYPE html PUBLIC \"unterminated>",
    "<!DOCTYPEhtml>",
    "<!DOCTYPE HtMl>",
    "<!DOCTYPE>",
    "<!doctype x>",
    "<!doctype html bogus>",
    "\u{feff}<!DOCTYPE html>",
];

/// Token families reaching every insertion mode, foreign content, raw text,
/// character references, comments and attributes; a case mixes a few of them.
const FAMILIES: &[&[&str]] = &[
    &[
        "<html lang=en>",
        "<html dir=rtl>",
        "</html>",
        "<head>",
        "</head>",
        "<body class=a style='color:red'>",
        "<body id=b lang=fr dir=ltr>",
        "</body>",
        "<frameset>",
        "</frameset>",
        "<frame>",
        "<noframes>",
        "</noframes>",
        "<meta charset=utf-8>",
        "<link rel=x>",
        "<base href=x>",
        "<title>t</title>",
        "<style>s</style>",
        "<script>1</script>",
        "<template>",
        "</template>",
        "<!-- c -->",
        " ",
        "\n",
        "x",
    ],
    &[
        "<table>",
        "</table>",
        "<caption>",
        "</caption>",
        "<colgroup>",
        "</colgroup>",
        "<col>",
        "<tbody>",
        "</tbody>",
        "<thead>",
        "</thead>",
        "<tfoot>",
        "</tfoot>",
        "<tr>",
        "</tr>",
        "<td>",
        "</td>",
        "<th>",
        "</th>",
        "<input type=hidden>",
        "<input type=text>",
        "<form>",
        "</form>",
        "<style>",
        "<script>",
        "<p>",
        "<b>",
        "x",
        " ",
        "\n",
    ],
    &[
        "<a href=x>",
        "</a>",
        "<b>",
        "</b>",
        "<i>",
        "</i>",
        "<u class=x>",
        "</u>",
        "<em>",
        "<strong>",
        "</strong>",
        "<font color=red>",
        "<font size=2>",
        "</font>",
        "<nobr>",
        "</nobr>",
        "<s>",
        "</s>",
        "<small>",
        "<big>",
        "<code>",
        "<tt>",
        "<strike>",
        "<div>",
        "</div>",
        "<p>",
        "</p>",
        "<address>",
        "<blockquote>",
        "<td>",
        "<marquee>",
        "</marquee>",
        "<object>",
        "</object>",
        "<applet>",
        "x",
    ],
    &[
        "<ul>",
        "</ul>",
        "<ol>",
        "<li>",
        "</li>",
        "<dl>",
        "<dt>",
        "<dd>",
        "</dd>",
        "</dt>",
        "<p>",
        "</p>",
        "<h1>",
        "</h2>",
        "<h3>",
        "<pre>",
        "<pre>\n",
        "<listing>\r\n",
        "</pre>",
        "<center>",
        "<section>",
        "<main>",
        "<footer>",
        "<details>",
        "<summary>",
        "<menu>",
        "<button>",
        "</button>",
        "<hr>",
        "<br>",
        "</br>",
        "<search>",
        "<dialog>",
        "<fieldset>",
        "<ruby>",
        "<rb>",
        "<rt>",
        "<rp>",
        "<rtc>",
        "\n",
        "x",
    ],
    &[
        "<select>",
        "</select>",
        "<option>",
        "</option>",
        "<optgroup>",
        "</optgroup>",
        "<input>",
        "<keygen>",
        "<hr>",
        "<textarea>",
        "<p>",
        "<div>",
        "<b>",
        "x",
    ],
    &[
        "<title>",
        "</title>",
        "<textarea>",
        "<textarea>\n",
        "</textarea>",
        "<style>",
        "</style>",
        "<script>",
        "</script>",
        "</SCRIPT >",
        "</scriptx>",
        "<!--",
        "-->",
        "<!--<script>",
        "<script>",
        "<script><!--",
        "<script><!--<script>",
        "--!>",
        "-",
        "<",
        "</",
        "<xmp>",
        "</xmp>",
        "<iframe>",
        "</iframe>",
        "<noembed>",
        "</noembed>",
        "<noscript>",
        "</noscript>",
        "<plaintext>",
        "&amp;",
        "&lt;",
        "x",
        "\0",
    ],
    &[
        "<svg>",
        "</svg>",
        "<svg viewbox='0 0 1 1' xlink:href=x xmlns:xlink=y xml:lang=en xmlns=z>",
        "<foreignobject>",
        "</foreignObject>",
        "<desc>",
        "</desc>",
        "<title>",
        "<path d=x/>",
        "<path>",
        "</path>",
        "<lineargradient gradientunits=u>",
        "<clippath>",
        "<math definitionurl=d>",
        "</math>",
        "<mi>",
        "</mi>",
        "<mo>",
        "<mtext>",
        "<annotation-xml encoding=text/html>",
        "</annotation-xml>",
        "<mglyph>",
        "<malignmark>",
        "<![CDATA[x]]>",
        "<![CDATA[a\0b]]>",
        "<![CDATA[",
        "]]>",
        "<b>",
        "<p>",
        "</p>",
        "<div>",
        "<font color=x>",
        "<font>",
        "<br>",
        "</br>",
        "<img>",
        "<table>",
        "<style>",
        "<script>",
        "x",
        "\0",
    ],
    &[
        "<template>",
        "</template>",
        "<template shadowrootmode=open>",
        "<td>",
        "<tr>",
        "<col>",
        "<caption>",
        "<tbody>",
        "<thead>",
        "<tfoot>",
        "<th>",
        "<colgroup>",
        "<div>",
        "<b>",
        "<p>",
        "<frameset>",
        "<body>",
        "<html class=t>",
        "x",
    ],
    &[
        "&amp;",
        "&AMP",
        "&nbsp;",
        "&nbsp",
        "&notin;",
        "&notit;",
        "&ampx",
        "&copy=",
        "&#10",
        "&#10;",
        "&#x0a;",
        "&#X41",
        "&#0;",
        "&#128;",
        "&#x9D;",
        "&#13;",
        "&#xD800;",
        "&#1114112;",
        "&#99999999999;",
        "&#;",
        "&#x;",
        "&bogus;",
        "&",
        "&#",
        "&NewLine;",
        "<pre>",
        "<textarea>",
        "<listing>",
        "<pre>&#10",
        "<textarea>&#x0A",
        "<b><b><b><b>",
        "\r",
        "\r\n",
        "\n",
        "é",
        "😀",
        "\u{a0}",
        "\0",
        ">",
        "\"",
        "'",
        "=",
        "1 < 2",
        "<3",
        "</>",
    ],
    &[
        "<p a=1 a=2 B=3>",
        "<div title=\"a&amp;b\" data-x='&copy=1&copy;' data-y=&ampz>",
        "<span id=x class=\"a b\" style=\"\">",
        "<p\0x=1>",
        "<a href=\"&#10\">",
        "<div /x>",
        "<br/>",
        "<p/ >",
        "<img src=\"x\"alt=y>",
        "<p =x>",
        "<P CLASS=UP Style='A'>",
        "<a href=x\r\ny=z>",
        "<div title=\"\u{a0}<>\">",
        "<img srcset='a 1x, http://b 2x'>",
        "<image src=//y>",
        "<source srcset=cid:a>",
        "<video poster=https://p>",
        "<img src=x>",
        "</div x=y>",
        "<div\tid=t>",
    ],
    &[
        "<div itemscope itemtype='https://schema.org/EventReservation'>",
        "<div itemscope itemtype='https://schema.org/Event'>",
        "<div itemscope itemtype=Ticket>",
        "<span itemprop=name>",
        "<meta itemprop=startDate content=2026>",
        "<link itemprop=url href=https://x>",
        "<div itemprop=reservationFor itemscope itemtype=Thing>",
        "<script type=application/ld+json>{\"@type\":\"Event\"}</script>",
        "<script type='APPLICATION/LD+JSON'>[1]</script>",
        "<svg><script type=application/ld+json>2</script></svg>",
        "<a href=https://track>Track your order</a>",
        "</div>",
        "</span>",
        "x",
    ],
    &[
        "<!-- x -->",
        "<!-->",
        "<!--->",
        "<!-- a -- b --->",
        "<!-- x --!>",
        "<!---x-->",
        "<!-- x --!-->",
        "<!x>",
        "<?php x ?>",
        "</ x>",
        "</3>",
        "<!",
        "<!-",
        "<!--",
        "--",
        "->",
        "<p>",
        "x",
    ],
];

const COMMON: &[&str] = &[
    "x", "word", " ", "\n", "<p>", "<div>", "</div>", "<b>", "</b>", "<span>",
];

/// The fuzz generator of the Bun port, in floating point as JavaScript computes it.
struct Random(f64);

impl Random {
    fn below(&mut self, bound: usize) -> usize {
        self.0 = (self.0 * 1103515245.0 + 12345.0) % 2147483648.0;
        (self.0 / 2147483648.0 * bound as f64).floor() as usize
    }

    fn soup(&mut self, tokens: &[&str], longest: usize) -> String {
        let length = 1 + self.below(longest);
        (0..length)
            .map(|_| tokens[self.below(tokens.len())])
            .collect()
    }

    /// A doctype, then tokens mostly from two families.
    fn mixed_soup(&mut self) -> String {
        let doctype = DOCTYPES[self.below(DOCTYPES.len())];
        let families = [
            FAMILIES[self.below(FAMILIES.len())],
            FAMILIES[self.below(FAMILIES.len())],
        ];
        let length = 1 + self.below(40);
        let tokens = (0..length).map(|_| match self.below(10) {
            0..3 => COMMON[self.below(COMMON.len())],
            choice => {
                let family = families[choice % 2];
                family[self.below(family.len())]
            }
        });
        let mut html = doctype.to_owned();
        html.extend(tokens);
        html
    }
}

fn bun_cases() -> Vec<String> {
    let tokens: Vec<_> = BUN_TOKENS.split('|').collect();
    let mut random = Random(42.0);
    let long: String = (0..60)
        .map(|index| format!("<p>Paragraph {index} with &amp; some   words</p><div>\n \t</div>"))
        .collect();
    BUN_HANDMADE
        .iter()
        .map(|case| case.to_string())
        .chain((0..4000).map(|_| random.soup(&tokens, 30)))
        .chain([
            long.clone(),
            format!("<style>x</style>{long}"),
            format!("<p>{}</p>", "a ".repeat(200)),
            format!("<b>{}</b>", "é".repeat(170)),
            format!("<b>{}</b>", "😀".repeat(170)),
        ])
        .collect()
}

/// Everything the views read from HTML, through their public entry points.
fn assert_parity(html: &str) {
    let message = Message {
        body_html: html.into(),
        body_loaded: true,
        ..Default::default()
    };
    let rendered = components::html::document(&message, false, false);
    let body = rendered
        .split_once("data-hide-quotes='false'>")
        .and_then(|(_, rest)| rest.rsplit_once("</div><style>"))
        .unwrap()
        .0;
    assert_eq!(body, reference::normalized_html(html), "render of {html:?}");
    assert_eq!(
        components::html::has_remote_media(&message),
        reference::has_remote_media(html),
        "remote media of {html:?}"
    );
    let shared = Html::new(html);
    assert_eq!(
        components::preview::message(&message, &shared),
        reference::preview(html),
        "preview of {html:?}"
    );
    assert_eq!(
        search::html_text(html),
        reference::html_text(html),
        "search text of {html:?}"
    );
    let fragment = scraper::Html::parse_fragment(html);
    assert_eq!(
        schema::json_ld(&shared),
        reference::json_ld(&fragment),
        "JSON-LD of {html:?}"
    );
    // The item types tickets, calendar events and parcels look inside of.
    let tickets = |kind: &str| {
        matches!(
            kind.rsplit(['/', ':', '#']).next(),
            Some(
                "EventReservation"
                    | "BusReservation"
                    | "TrainReservation"
                    | "FlightReservation"
                    | "Ticket"
            )
        )
    };
    let events = |kind: &str| {
        matches!(
            kind.rsplit('/').next(),
            Some("EventReservation" | "InviteAction" | "RsvpAction")
        )
    };
    let parcels = |_: &str| false;
    for (consumer, nested) in [
        ("tickets", &tickets as &dyn Fn(&str) -> bool),
        ("calendar", &events),
        ("parcels", &parcels),
    ] {
        assert_eq!(
            schema::items(&shared, nested).collect::<Vec<_>>(),
            reference::items(&fragment, nested),
            "{consumer} schema items of {html:?}"
        );
    }
}

#[test]
fn bun_fuzz_cases_match_scraper() {
    for html in bun_cases() {
        assert_parity(&html);
    }
}

#[test]
fn tricky_cases_match_scraper() {
    for html in TRICKY {
        assert_parity(html);
    }
}

#[test]
fn generated_tag_soup_matches_scraper() {
    let mut random = Random(7.0);
    for _ in 0..5000 {
        assert_parity(&random.mixed_soup());
    }
}

#[test]
#[ignore = "Set BREVLADA_CORPUS to a directory of .eml files"]
fn corpus_matches_scraper() -> anyhow::Result<()> {
    let corpus = std::env::var("BREVLADA_CORPUS")?;
    let mut paths: Vec<_> = std::fs::read_dir(corpus)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    paths.sort();
    let mut compared = 0;
    for (index, path) in paths.iter().enumerate() {
        let message = parser::parse(index as u32 + 1, &std::fs::read(path)?, false, true)?;
        if !message.body_html.is_empty() {
            assert_parity(&message.body_html);
            compared += 1;
        }
    }
    println!("{compared} HTML bodies match");
    Ok(())
}

/// The scraper-based code this replaced, kept as the oracle.
mod reference {
    use scraper::{ElementRef, Html, Node, Selector};
    use serde_json::{Map, Value};

    pub fn has_remote_media(html: &str) -> bool {
        let document = Html::parse_fragment(html);
        let selector =
            Selector::parse("img[src], img[srcset], source[srcset], video[poster]").unwrap();
        document.select(&selector).any(|element| {
            ["src", "srcset", "poster"].into_iter().any(|attribute| {
                element.value().attr(attribute).is_some_and(|value| {
                    value.split(',').any(|candidate| {
                        let url = candidate.trim_start();
                        url.starts_with("https://")
                            || url.starts_with("http://")
                            || url.starts_with("//")
                            || url.starts_with("cid:")
                    })
                })
            })
        })
    }

    pub fn normalized_html(source: &str) -> String {
        let document = Html::parse_document(source);
        let styles = Selector::parse("head style").unwrap();
        let body = Selector::parse("body").unwrap();
        let mut result = document
            .select(&styles)
            .map(|style| style.html())
            .collect::<String>();
        if let Some(body) = document.select(&body).next() {
            result.push_str("<div");
            for (name, value) in body.value().attrs() {
                if matches!(name, "style" | "class" | "id" | "dir" | "lang") {
                    result.push_str(&format!(
                        " {name}=\"{}\"",
                        gtk::glib::markup_escape_text(value)
                    ));
                }
            }
            result.push('>');
            result.push_str(&body.inner_html());
            result.push_str("</div>");
        }
        result
    }

    pub fn preview(html: &str) -> String {
        let document = Html::parse_document(html);
        let mut text = String::new();
        append_text(document.root_element(), &mut text);
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut characters = text.chars();
        let mut preview: String = characters.by_ref().take(160).collect();
        if characters.next().is_some() {
            preview.truncate(preview.trim_end().len());
            preview.push('…');
        }
        preview
    }

    fn append_text(element: ElementRef<'_>, text: &mut String) {
        if matches!(
            element.value().name(),
            "head" | "script" | "style" | "template"
        ) {
            return;
        }
        let block = matches!(
            element.value().name(),
            "address"
                | "article"
                | "aside"
                | "blockquote"
                | "br"
                | "div"
                | "dl"
                | "dt"
                | "dd"
                | "footer"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "header"
                | "hr"
                | "li"
                | "main"
                | "ol"
                | "p"
                | "pre"
                | "section"
                | "table"
                | "td"
                | "th"
                | "tr"
                | "ul"
        );
        if block {
            text.push(' ');
        }
        for child in element.children() {
            match child.value() {
                Node::Text(value) => text.push_str(value),
                Node::Element(_) => append_text(ElementRef::wrap(child).unwrap(), text),
                _ => {}
            }
        }
        if block {
            text.push(' ');
        }
    }

    pub fn html_text(html: &str) -> String {
        let document = Html::parse_document(html);
        let mut text = String::new();
        let mut pending = vec![Some(*document.root_element())];
        while let Some(node) = pending.pop() {
            let Some(node) = node else {
                text.push(' ');
                continue;
            };
            if let Some(value) = node.value().as_text() {
                text.push_str(value);
            } else if let Some(element) = ElementRef::wrap(node) {
                let name = element.value().name();
                if matches!(name, "script" | "style" | "head") {
                    continue;
                }
                let block = matches!(
                    name,
                    "p" | "div"
                        | "br"
                        | "hr"
                        | "li"
                        | "tr"
                        | "td"
                        | "th"
                        | "section"
                        | "article"
                        | "blockquote"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "pre"
                );
                if block {
                    text.push(' ');
                    pending.push(None);
                }
                pending.extend(element.children().rev().map(Some));
            }
        }
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    pub fn json_ld(document: &Html) -> Vec<Value> {
        document
            .select(&Selector::parse("script[type='application/ld+json']").unwrap())
            .filter_map(|script| serde_json::from_str(&script.text().collect::<String>()).ok())
            .collect()
    }

    pub fn items(document: &Html, nested: impl Fn(&str) -> bool) -> Vec<Value> {
        let mut items = json_ld(document);
        for element in document.select(&Selector::parse("[itemscope][itemtype]").unwrap()) {
            if element
                .ancestors()
                .filter_map(ElementRef::wrap)
                .any(|parent| {
                    parent
                        .value()
                        .attr("itemtype")
                        .is_some_and(|kind| kind.split_whitespace().any(&nested))
                })
            {
                continue;
            }
            items.push(microdata(&element));
        }
        items
    }

    fn microdata(element: &ElementRef<'_>) -> Value {
        let mut map = Map::new();
        if let Some(kind) = element.value().attr("itemtype") {
            map.insert(
                "@type".into(),
                Value::String(kind.rsplit('/').next().unwrap_or(kind).into()),
            );
        }
        for child in element.descendants().skip(1).filter_map(ElementRef::wrap) {
            let Some(property) = child.value().attr("itemprop") else {
                continue;
            };
            let nearest_scope = child
                .ancestors()
                .filter_map(ElementRef::wrap)
                .find(|ancestor| ancestor.value().attr("itemscope").is_some());
            if nearest_scope.is_none_or(|scope| scope.id() != element.id()) {
                continue;
            }
            let value = if child.value().attr("itemscope").is_some() {
                microdata(&child)
            } else {
                let text = ["content", "href", "src", "datetime"]
                    .iter()
                    .find_map(|name| child.value().attr(name))
                    .map(str::to_owned)
                    .unwrap_or_else(|| child.text().collect::<String>().trim().to_owned());
                Value::String(text)
            };
            if let Some(previous) = map.get_mut(property) {
                if let Value::Array(items) = previous {
                    items.push(value);
                } else {
                    let old = std::mem::replace(previous, Value::Null);
                    *previous = Value::Array(vec![old, value]);
                }
            } else {
                map.insert(property.to_owned(), value);
            }
        }
        Value::Object(map)
    }
}
