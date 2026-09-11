use super::http;
use crate::{models::Account, theme};
use anyhow::{Result, bail};
use gtk::{
    gdk_pixbuf::{self, prelude::PixbufLoaderExt},
    glib,
};
use std::time::Duration;

/// How long a downloaded avatar stays usable before it is looked up again.
pub const TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// Senders without any avatar are remembered too, so a miss is not retried on
/// every launch, but for a shorter time because people do add photos later.
pub const MISS_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The OpenID profile of the signed-in user. This is the one Google endpoint a
/// GNOME Online Accounts credential can always reach, and it carries the
/// account owner's own photo.
const USERINFO: &str = "https://www.googleapis.com/oauth2/v3/userinfo";
/// Photos of people in the account's contacts. Online Accounts' OAuth client
/// does not have the People API enabled, so in practice this is refused and
/// disabled for the session; it is still attempted once per account so that
/// contact photos appear for credentials that do allow it.
const PEOPLE: &str = "https://people.googleapis.com/v1";
const READ_MASK: &str = "emailAddresses,photos";
/// Identifies how the cached images were produced. Changing a source or the way
/// an image is prepared makes every cached entry stale, including the senders
/// recorded as having no avatar at all, so this string is bumped alongside.
pub const SOURCES: &str = "google,gravatar,icons-fitted-padded15";
/// Google's favicon service, which needs no credential. It is the last resort
/// for senders that are a brand rather than a person.
const FAVICONS: &str = "https://www.google.com/s2/favicons";
/// A second icon service. Neither is reliably better: Google has the larger
/// icon for reddit.com and rohlik.cz, this one for github.com and vodafone.cz,
/// so both are asked and the higher resolution wins.
const ICON_HORSE: &str = "https://icon.horse/icon";
/// Smallest icon worth showing. Below this an icon is too coarse to draw at
/// avatar size, and initials look better than a handful of enlarged pixels.
const MIN_ICON: u32 = 32;
/// Detail beyond which there is no point asking another service or domain.
const GOOD_ICON: u32 = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Google,
    Gravatar,
    Favicon,
    None,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Google => "google",
            Source::Gravatar => "gravatar",
            Source::Favicon => "favicon",
            Source::None => "none",
        }
    }
}

/// A Google account that may be able to answer avatar lookups.
pub struct Credential {
    pub account: Account,
    /// Whether the contacts endpoints are still worth asking. Cleared once an
    /// account refuses them so later senders skip straight to Gravatar.
    pub search: bool,
}

pub struct Fetched {
    pub source: Source,
    pub image: Option<Vec<u8>>,
    /// Accounts whose Google credential was rejected. Their lookups are dropped
    /// for the rest of the session instead of being retried per sender.
    pub unauthorized: Vec<String>,
}

/// The cache key for a sender, shared with the sender list so both sides agree
/// on what counts as the same address.
pub fn normalize(address: &str) -> String {
    crate::models::senders::address(address)
}

/// Google is only asked about senders when an account authenticates against a
/// Google IMAP host with OAuth2, which is the credential the People API needs.
pub fn is_google(account: &Account) -> bool {
    let host = account.host.trim().to_lowercase();
    account.oauth2
        && ["gmail.com", "googlemail.com", "google.com"]
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

pub fn gravatar_url(email: &str) -> String {
    let hash = glib::compute_checksum_for_string(glib::ChecksumType::Sha256, normalize(email))
        .unwrap_or_default();
    format!(
        "https://gravatar.com/avatar/{hash}?s={}&d=404",
        theme::AVATAR_PIXELS
    )
}

/// The domain part of an address, or empty when there is none.
pub fn domain(email: &str) -> String {
    normalize(email)
        .rsplit_once('@')
        .map(|(_, domain)| domain.to_owned())
        .unwrap_or_default()
}

/// Domains to ask for a brand icon, most specific first, or empty when the
/// sender's domain is a mailbox provider and so says nothing about who wrote.
///
/// Google only resolves registrable domains, so `mail.zed.dev` has to fall back
/// to `zed.dev`. The sub-domain is still tried first because a sender like
/// `news.example.com` may have branding of its own. Only the address's own host
/// is checked against the provider list: `someone@proton.me` is a person, while
/// `no-reply@offers.proton.me` is Proton itself and has earned its logo.
pub fn favicon_domains(email: &str) -> Vec<String> {
    let host = domain(email);
    if host.is_empty() || !host.contains('.') || super::sender_domains::is_consumer(&host) {
        return Vec::new();
    }
    let labels: Vec<&str> = host.split('.').collect();
    let mut candidates = Vec::new();
    for take in [labels.len(), 3, 2] {
        if take > labels.len() {
            continue;
        }
        let candidate = labels[labels.len() - take..].join(".");
        if candidate.contains('.') && !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

pub fn favicon_urls(domain: &str) -> [String; 2] {
    let brand = super::sender_domains::brand_of(domain);
    [
        format!(
            "{FAVICONS}?domain={}&sz={}",
            http::encode(&brand),
            theme::ICON_PIXELS
        ),
        format!("{ICON_HORSE}/{}", http::encode(&brand)),
    ]
}

/// Resolves one sender: the Google account photo, then Gravatar, then the
/// sender domain's brand icon. `None` means the caller should fall back to
/// initials.
pub fn fetch(credentials: &[Credential], email: &str) -> Result<Fetched> {
    let email = normalize(email);
    let mut unauthorized = Vec::new();
    let mut transient = false;
    for credential in credentials {
        let own = normalize(&credential.account.email) == email;
        if !own && !credential.search {
            continue;
        }
        let Ok(token) = super::accounts::token(&credential.account) else {
            transient = true;
            continue;
        };
        let outcome = if own {
            own_photo(&token)
        } else {
            contact_photo(&token, &email)
        };
        match outcome {
            Outcome::Found(url) => match download(&url) {
                Ok(Some(image)) => {
                    return Ok(Fetched {
                        source: Source::Google,
                        image: Some(image),
                        unauthorized,
                    });
                }
                Ok(None) => {}
                Err(_) => transient = true,
            },
            Outcome::Missing => {}
            // Only contact lookups are given up on. The account's own photo
            // comes from a different endpoint that keeps working.
            Outcome::Denied if !own => unauthorized.push(credential.account.email.clone()),
            Outcome::Denied => {}
            Outcome::Failed => transient = true,
        }
    }
    match download(&gravatar_url(&email)) {
        Ok(Some(image)) => {
            return Ok(Fetched {
                source: Source::Gravatar,
                image: Some(image),
                unauthorized,
            });
        }
        Ok(None) => {}
        Err(_) => transient = true,
    }
    match brand_icon(&favicon_domains(&email)) {
        Ok(Some(image)) => {
            return Ok(Fetched {
                source: Source::Favicon,
                image: Some(plate(&image).unwrap_or(image)),
                unauthorized,
            });
        }
        Ok(None) => {}
        Err(_) => transient = true,
    }
    if transient {
        bail!("Could not reach the avatar services");
    }
    Ok(Fetched {
        source: Source::None,
        image: None,
        unauthorized,
    })
}

enum Outcome {
    Found(String),
    Missing,
    Denied,
    Failed,
}

fn own_photo(token: &str) -> Outcome {
    let Ok(response) = http::get(USERINFO, Some(token)) else {
        return Outcome::Failed;
    };
    match response.status {
        401 | 403 => return Outcome::Denied,
        200 => {}
        _ => return Outcome::Failed,
    }
    match picture_url(&response.text()) {
        Some(url) => Outcome::Found(url),
        None => Outcome::Missing,
    }
}

fn contact_photo(token: &str, email: &str) -> Outcome {
    let query = http::encode(email);
    let mut outcome = Outcome::Missing;
    for collection in ["people:searchContacts", "otherContacts:search"] {
        let endpoint =
            format!("{PEOPLE}/{collection}?readMask={READ_MASK}&pageSize=10&query={query}");
        let Ok(response) = http::get(&endpoint, Some(token)) else {
            outcome = Outcome::Failed;
            continue;
        };
        // 401 means the credential was refused, 403 that this account may not
        // use the People API. Neither improves by asking again.
        if matches!(response.status, 401 | 403) {
            return Outcome::Denied;
        }
        if response.status != 200 {
            outcome = Outcome::Failed;
            continue;
        }
        if let Some(url) = photo_url(&response.text(), email) {
            return Outcome::Found(url);
        }
    }
    outcome
}

/// Reads the photo out of an OpenID userinfo document.
fn picture_url(body: &str) -> Option<String> {
    let response: serde_json::Value = serde_json::from_str(body).ok()?;
    let picture = response.get("picture")?.as_str()?;
    Some(sized(picture, theme::AVATAR_PIXELS))
}

/// Picks a real photo for `email` out of a People API response. Google returns
/// a generated silhouette flagged `default` for contacts without a photo; those
/// are skipped so the sender's initials are shown instead.
fn photo_url(body: &str, email: &str) -> Option<String> {
    let response: serde_json::Value = serde_json::from_str(body).ok()?;
    let people = response.get("results")?.as_array()?;
    for person in people.iter().filter_map(|result| result.get("person")) {
        let matches = person
            .get("emailAddresses")
            .and_then(|addresses| addresses.as_array())
            .is_some_and(|addresses| {
                addresses
                    .iter()
                    .filter_map(|address| address.get("value").and_then(|v| v.as_str()))
                    .any(|address| normalize(address) == email)
            });
        if !matches {
            continue;
        }
        let photo = person
            .get("photos")
            .and_then(|photos| photos.as_array())?
            .iter()
            .find(|photo| photo.get("default").and_then(|d| d.as_bool()) != Some(true))
            .and_then(|photo| photo.get("url").and_then(|url| url.as_str()))?;
        return Some(sized(photo, theme::AVATAR_PIXELS));
    }
    None
}

/// Rewrites a Google photo URL to request a square image of the size the UI
/// draws, instead of the small default the API hands out.
fn sized(url: &str, pixels: u32) -> String {
    if url.contains('?') {
        return url.to_owned();
    }
    let base = match url.rsplit_once('=') {
        Some((base, options))
            if options.starts_with('s')
                && options[1..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit()) =>
        {
            base
        }
        _ => url,
    };
    format!("{base}=s{pixels}-c")
}

/// The sharpest icon any service holds for any of a sender's domains.
///
/// Every combination is scored rather than taking the first answer, because a
/// service will happily return something useless: a placeholder tile, or a tiny
/// favicon enlarged to whatever size was asked for. Searching stops early once
/// an icon is good enough to draw at avatar size.
fn brand_icon(domains: &[String]) -> Result<Option<Vec<u8>>> {
    let mut best: Option<(u32, Vec<u8>)> = None;
    let mut transient = false;
    for domain in domains {
        for url in favicon_urls(domain) {
            match download(&url) {
                Ok(Some(image)) => {
                    let pixels = detail(&image);
                    if pixels >= MIN_ICON && best.as_ref().is_none_or(|(seen, _)| pixels > *seen) {
                        best = Some((pixels, image));
                    }
                }
                Ok(None) => {}
                Err(_) => transient = true,
            }
        }
        if best
            .as_ref()
            .is_some_and(|(pixels, _)| *pixels >= GOOD_ICON)
        {
            break;
        }
    }
    match best {
        Some((_, image)) => Ok(Some(image)),
        None if transient => bail!("Could not reach the icon services"),
        None => Ok(None),
    }
}

/// How much real detail an icon holds, along its shorter side. Zero for
/// anything that should be treated as no icon at all.
fn detail(image: &[u8]) -> u32 {
    let Some(icon) = decode(image) else {
        return 0;
    };
    if is_generated(&icon) {
        return 0;
    }
    effective_pixels(&icon)
}

fn decode(image: &[u8]) -> Option<gdk_pixbuf::Pixbuf> {
    let loader = gdk_pixbuf::PixbufLoader::new();
    loader.write(image).ok()?;
    loader.close().ok()?;
    loader.pixbuf()
}

/// The resolution an icon really has, which is not what it claims.
///
/// Asked for a large icon, Google enlarges whatever it holds by repeating
/// pixels, so a site with only a 16 pixel favicon still answers with a 256
/// pixel image built from 16 pixel blocks. The size of those blocks is the
/// stride at which neighbouring rows and columns actually differ, and dividing
/// it out recovers the detail that is genuinely present.
fn effective_pixels(icon: &gdk_pixbuf::Pixbuf) -> u32 {
    let (width, height) = (icon.width(), icon.height());
    if width <= 0 || height <= 0 {
        return 0;
    }
    let bytes = icon.read_pixel_bytes();
    let stride = icon.rowstride() as usize;
    let channels = icon.n_channels() as usize;
    let pixel = |x: i32, y: i32| {
        let at = y as usize * stride + x as usize * channels;
        bytes.get(at..at + channels)
    };
    let mut columns = 0;
    for x in 1..width {
        if (0..height).any(|y| pixel(x, y) != pixel(x - 1, y)) {
            columns = gcd(columns, x);
        }
    }
    let mut rows = 0;
    for y in 1..height {
        if (0..width).any(|x| pixel(x, y) != pixel(x, y - 1)) {
            rows = gcd(rows, y);
        }
    }
    // Enlarging by repeating pixels always lays down blocks that tile the image
    // exactly. A stride that does not divide the side is just a logo whose
    // edges happen to line up, and that image keeps its full resolution.
    let block = |stride: i32, side: i32| match stride {
        0 => side,
        stride if side % stride == 0 => stride,
        _ => 1,
    };
    let across = width / block(columns, width);
    let down = height / block(rows, height);
    across.min(down).max(0) as u32
}

fn gcd(a: i32, b: i32) -> i32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Whether this is the tile a service draws for a domain it does not know: the
/// domain's first letter in grey on a fixed light grey field. It arrives as a
/// genuine full-size image, so only the palette gives it away, and it must be
/// rejected or it outranks the real icon a smaller service found.
fn is_generated(icon: &gdk_pixbuf::Pixbuf) -> bool {
    const FIELD: &[u8] = &[226, 226, 226];
    let bytes = icon.read_pixel_bytes();
    if bytes.get(..3) != Some(FIELD) {
        return false;
    }
    let stride = icon.rowstride() as usize;
    let channels = icon.n_channels() as usize;
    // The tile is drawn wholly in shades of grey. A real logo starting on the
    // very same grey is possible, but one with no colour anywhere is not.
    (0..icon.height()).step_by(4).all(|y| {
        (0..icon.width()).step_by(4).all(|x| {
            let at = y as usize * stride + x as usize * channels;
            bytes
                .get(at..at + 3)
                .is_none_or(|p| p[0] == p[1] && p[1] == p[2])
        })
    })
}

/// Lays a brand icon on an opaque square so it reads as an avatar.
///
/// Without this a logo with a transparent background would show whatever is
/// behind the window. The icon keeps its original detail, and the plate grows
/// when needed to fit its marks inside the circular avatar with a little room
/// between the corners and the rim.
fn plate(image: &[u8]) -> Option<Vec<u8>> {
    let icon = decode(image)?;
    let (width, height) = (icon.width(), icon.height());
    if width <= 0 || height <= 0 {
        return None;
    }
    let colour = background(&icon);
    // Give marks that extend beyond the circle 15% extra room so their corners
    // sit clear of the rim. Already fitting icons keep their original size.
    let original_side = width.max(height);
    let fitted_side = 2 * reach(&icon, colour);
    let side = if fitted_side > original_side {
        (f64::from(fitted_side) * 1.15).ceil() as i32
    } else {
        original_side
    };
    let plate = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, false, 8, side, side)?;
    plate.fill(colour);
    let (x, y) = ((side - width) / 2, (side - height) / 2);
    icon.composite(
        &plate,
        x,
        y,
        width,
        height,
        x as f64,
        y as f64,
        1.0,
        1.0,
        gdk_pixbuf::InterpType::Nearest,
        255,
    );
    plate.save_to_bufferv("png", &[]).ok()
}

/// How far the icon's own marks reach from its centre, in pixels.
///
/// The avatar crops to a circle, so anything further from the centre than half
/// the plate is cut away. That is fine for the parts of an icon that match the
/// plate, which are indistinguishable from it, and fine for a round logo, whose
/// corners are empty. It is not fine for a square logo such as the Windows
/// flag: its corners carry the logo itself. Measuring the reach tells the two
/// apart exactly, rather than guessing from the shape.
fn reach(icon: &gdk_pixbuf::Pixbuf, plate: u32) -> i32 {
    /// How far a pixel must sit from the plate colour to count as a mark, per
    /// channel. Antialiasing against the plate lands well inside this.
    const TOLERANCE: i16 = 24;
    /// Below this a pixel is too faint to be worth protecting from the crop.
    const VISIBLE: u8 = 24;

    let bytes = icon.read_pixel_bytes();
    let channels = icon.n_channels() as usize;
    let stride = icon.rowstride() as usize;
    let (width, height) = (icon.width(), icon.height());
    let [red, green, blue, _] = plate.to_be_bytes();
    let (centre_x, centre_y) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
    let mut furthest = 0.0f64;
    for y in 0..height {
        for x in 0..width {
            let at = y as usize * stride + x as usize * channels;
            let Some(pixel) = bytes.get(at..at + channels) else {
                continue;
            };
            if channels == 4 && pixel[3] < VISIBLE {
                continue;
            }
            let differs = [(pixel[0], red), (pixel[1], green), (pixel[2], blue)]
                .iter()
                .any(|(found, plate)| (i16::from(*found) - i16::from(*plate)).abs() > TOLERANCE);
            if !differs {
                continue;
            }
            let (dx, dy) = (f64::from(x) + 0.5 - centre_x, f64::from(y) + 0.5 - centre_y);
            furthest = furthest.max(dx.hypot(dy));
        }
    }
    furthest.ceil() as i32
}

/// The colour to lay the icon on.
///
/// What matters is the colour where the avatar's circular crop falls. There the
/// icon is usually either one solid shape or nothing at all. When it is a solid
/// shape of a single colour, the plate takes that colour so the shape's
/// antialiased rim blends into it; laying such a logo on white instead leaves a
/// pale bezel about a pixel wide all the way round. When the crop runs through
/// several colours, as it does across the four quadrants of the Windows logo,
/// no single colour matches and white is the neutral choice — picking one of
/// them would flood the logo's transparent gaps with, say, orange.
fn background(icon: &gdk_pixbuf::Pixbuf) -> u32 {
    const WHITE: u32 = 0xFFFF_FFFF;
    const SAMPLES: i32 = 96;
    /// Where the avatar crops, as a fraction of the icon's shorter side.
    const RADIUS: f64 = 0.47;
    /// Sampled colours this far from their mean, per channel, still count as
    /// one colour. Generous enough to accept a logo with a gradient.
    const UNIFORM: f64 = 48.0;

    let bytes = icon.read_pixel_bytes();
    let channels = icon.n_channels() as usize;
    let stride = icon.rowstride() as usize;
    let (width, height) = (icon.width(), icon.height());
    let pixel = |x: i32, y: i32| {
        let at = y as usize * stride + x as usize * channels;
        bytes.get(at..at + channels)
    };
    let opaque = |sample: &[u8]| channels < 4 || sample[3] == u8::MAX;

    let side = width.min(height) as f64;
    let mut ring = Vec::with_capacity(SAMPLES as usize);
    let mut sampled = 0;
    for step in 0..SAMPLES {
        let angle = f64::from(step) * std::f64::consts::TAU / f64::from(SAMPLES);
        let x = (f64::from(width) / 2.0 + angle.cos() * side * RADIUS).round() as i32;
        let y = (f64::from(height) / 2.0 + angle.sin() * side * RADIUS).round() as i32;
        let Some(sample) = pixel(x.clamp(0, width - 1), y.clamp(0, height - 1)) else {
            continue;
        };
        sampled += 1;
        if opaque(sample) {
            ring.push([sample[0], sample[1], sample[2]]);
        }
    }
    // A rim that is mostly empty belongs to a logo carrying its own padding,
    // and there is no edge colour to match.
    if sampled > 0 && ring.len() * 10 >= sampled as usize * 9 {
        let mean = |channel: usize| {
            ring.iter().map(|c| f64::from(c[channel])).sum::<f64>() / ring.len() as f64
        };
        let mean = [mean(0), mean(1), mean(2)];
        let deviation = ring
            .iter()
            .map(|colour| {
                (0..3)
                    .map(|c| (f64::from(colour[c]) - mean[c]).abs())
                    .sum::<f64>()
                    / 3.0
            })
            .sum::<f64>()
            / ring.len() as f64;
        if deviation <= UNIFORM {
            return u32::from_be_bytes([mean[0] as u8, mean[1] as u8, mean[2] as u8, 0xFF]);
        }
    }
    // Any transparency at all means the icon expects something behind it.
    let transparent = channels == 4
        && (0..height).step_by(2).any(|y| {
            (0..width)
                .step_by(2)
                .any(|x| !pixel(x, y).is_none_or(opaque))
        });
    if transparent {
        return WHITE;
    }
    match pixel(0, 0) {
        Some(corner) if corner.len() >= 3 => {
            u32::from_be_bytes([corner[0], corner[1], corner[2], 0xFF])
        }
        _ => WHITE,
    }
}

/// Downloads an avatar. `Ok(None)` is a definitive "no avatar", while `Err` is
/// a transient failure that must not be cached as a miss.
fn download(url: &str) -> Result<Option<Vec<u8>>> {
    let response = http::get(url, None)?;
    if matches!(response.status, 404 | 403) {
        return Ok(None);
    }
    if response.status != 200 {
        bail!("Avatar request failed with status {}", response.status);
    }
    // Error pages and redirects to sign-in forms arrive with a 200; only keep
    // bytes that actually decode as an image.
    Ok(is_image(&response.body).then_some(response.body))
}

fn is_image(data: &[u8]) -> bool {
    data.starts_with(b"\x89PNG\r\n\x1a\n")
        || data.starts_with(&[0xFF, 0xD8, 0xFF])
        || data.starts_with(b"GIF87a")
        || data.starts_with(b"GIF89a")
        || (data.len() > 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP")
}

/// Seconds since the Unix epoch, the resolution avatar timestamps are kept at.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

/// Whether a cached entry may still be used without asking the network again.
pub fn is_fresh(found: bool, fetched_at: i64, now: i64) -> bool {
    let age = now - fetched_at;
    let ttl = if found { TTL } else { MISS_TTL };
    (0..ttl.as_secs() as i64).contains(&age)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(host: &str, oauth2: bool) -> Account {
        Account {
            path: String::new(),
            email: "me@gmail.com".into(),
            name: String::new(),
            host: host.into(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2,
        }
    }

    #[test]
    fn hashes_gravatar_addresses_the_way_gravatar_documents() {
        let url = gravatar_url("  MyEmailAddress@example.com ");
        assert!(url.starts_with(
            "https://gravatar.com/avatar/\
             84059b07d4be67b806386c0aad8070a23f18836bbaae342275dc0a83414c32ee?"
        ));
        // Missing avatars must 404 rather than return a generated image.
        assert!(url.ends_with("&d=404"));
        assert_eq!(gravatar_url("<MyEmailAddress@example.com>"), url);
    }

    #[test]
    fn only_google_oauth_accounts_offer_a_people_credential() {
        assert!(is_google(&account("imap.gmail.com", true)));
        assert!(is_google(&account("IMAP.GOOGLEMAIL.COM", true)));
        assert!(is_google(&account("imap.google.com", true)));
        assert!(!is_google(&account("imap.gmail.com", false)));
        assert!(!is_google(&account("imap.notgmail.com", true)));
        assert!(!is_google(&account("imap.fastmail.com", true)));
    }

    #[test]
    fn takes_the_matching_contact_photo_and_ignores_generated_ones() {
        let body = r#"{"results":[
            {"person":{"emailAddresses":[{"value":"other@example.com"}],
             "photos":[{"url":"https://lh3.googleusercontent.com/other=s100"}]}},
            {"person":{"emailAddresses":[{"value":"Ada@Example.com"}],
             "photos":[{"url":"https://lh3.googleusercontent.com/blank=s100","default":true},
                       {"url":"https://lh3.googleusercontent.com/ada=s100"}]}}]}"#;
        assert_eq!(
            photo_url(body, "ada@example.com").as_deref(),
            Some("https://lh3.googleusercontent.com/ada=s128-c")
        );
        assert_eq!(photo_url(body, "nobody@example.com"), None);
        let only_default = r#"{"results":[{"person":{"emailAddresses":[{"value":"ada@example.com"}],
            "photos":[{"url":"https://lh3.googleusercontent.com/blank","default":true}]}}]}"#;
        assert_eq!(photo_url(only_default, "ada@example.com"), None);
        assert_eq!(photo_url("{\"results\":[]}", "ada@example.com"), None);
        assert_eq!(photo_url("not json", "ada@example.com"), None);
    }

    #[test]
    fn reads_the_accounts_own_photo_from_its_openid_profile() {
        let body = r#"{"sub":"1","email":"me@gmail.com",
            "picture":"https://lh3.googleusercontent.com/a/ACg8oc=s96-c"}"#;
        assert_eq!(
            picture_url(body).as_deref(),
            Some("https://lh3.googleusercontent.com/a/ACg8oc=s128-c")
        );
        assert_eq!(picture_url(r#"{"sub":"1"}"#), None);
        assert_eq!(picture_url("not json"), None);
    }

    #[test]
    fn requests_display_sized_images_without_corrupting_urls() {
        assert_eq!(
            sized("https://host/photo=s100", 128),
            "https://host/photo=s128-c"
        );
        assert_eq!(
            sized("https://host/photo=s100-c-k", 128),
            "https://host/photo=s128-c"
        );
        assert_eq!(
            sized("https://host/photo", 128),
            "https://host/photo=s128-c"
        );
        assert_eq!(sized("https://host/a=b", 128), "https://host/a=b=s128-c");
        assert_eq!(sized("https://host/p?sz=50", 128), "https://host/p?sz=50");
    }

    #[test]
    fn asks_for_brand_icons_only_where_the_domain_identifies_the_sender() {
        assert_eq!(
            favicon_urls("github.com"),
            [
                "https://www.google.com/s2/favicons?domain=github.com&sz=256",
                "https://icon.horse/icon/github.com"
            ]
        );
        // Bulk-mail domains ask under the brand they send for.
        assert_eq!(
            favicon_urls("redditmail.com")[0],
            "https://www.google.com/s2/favicons?domain=reddit.com&sz=256"
        );
        assert_eq!(favicon_domains("noreply@github.com"), ["github.com"]);
        assert_eq!(
            favicon_domains("Sluzebnicek <sluzebnicek@ALZA.CZ>"),
            ["alza.cz"]
        );
        // Google only resolves registrable domains, so the parent is the
        // fallback, but the sub-domain gets its chance at its own branding.
        assert_eq!(
            favicon_domains("zed@mail.zed.dev"),
            ["mail.zed.dev", "zed.dev"]
        );
        assert_eq!(
            favicon_domains("a@deep.sub.example.co.uk"),
            ["deep.sub.example.co.uk", "example.co.uk", "co.uk"]
        );
        // A person with a mailbox at a provider must not wear its logo.
        assert!(favicon_domains("someone@gmail.com").is_empty());
        assert!(favicon_domains("someone@seznam.cz").is_empty());
        // The provider's own marketing domain is the brand speaking.
        assert_eq!(
            favicon_domains("no-reply@offers.proton.me"),
            ["offers.proton.me", "proton.me"]
        );
        assert!(favicon_domains("not-an-address").is_empty());
        assert!(favicon_domains("root@localhost").is_empty());
        assert!(favicon_domains("").is_empty());
    }

    /// Builds a PNG of `side` square, transparent when `alpha` is set and a
    /// solid colour otherwise, to stand in for a downloaded brand icon.
    fn icon(side: i32, alpha: bool, colour: u32) -> Vec<u8> {
        let pixbuf =
            gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, alpha, 8, side, side).unwrap();
        pixbuf.fill(colour);
        pixbuf.save_to_bufferv("png", &[]).unwrap()
    }

    /// Repeats each pixel `scale` times in both directions, the way an icon
    /// service enlarges a small favicon to the size that was asked for.
    fn enlarged(side: i32, scale: i32) -> Vec<u8> {
        let small =
            gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, false, 8, side, side).unwrap();
        for y in 0..side {
            for x in 0..side {
                small.put_pixel(x as u32, y as u32, (x * 40) as u8, (y * 40) as u8, 90, 255);
            }
        }
        small
            .scale_simple(side * scale, side * scale, gdk_pixbuf::InterpType::Nearest)
            .unwrap()
            .save_to_bufferv("png", &[])
            .unwrap()
    }

    #[test]
    fn scores_an_enlarged_favicon_by_the_detail_it_actually_has() {
        // A 16 pixel favicon blown up to 256 is still worth 16 pixels, and so
        // is rejected however large the service claims it to be.
        let stretched = enlarged(16, 16);
        let decoded = decode(&stretched).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 256));
        assert_eq!(detail(&stretched), 16);
        assert!(detail(&stretched) < MIN_ICON);
        // An icon with detail in every pixel is worth its full size.
        assert_eq!(detail(&enlarged(64, 1)), 64);
        assert_eq!(detail(&enlarged(48, 2)), 48);
        // A blank image has no detail at all to show.
        assert_eq!(detail(&icon(64, false, 0x2233_44FF)), 1);
        assert_eq!(detail(b"<!DOCTYPE html>"), 0);
    }

    #[test]
    fn rejects_the_grey_letter_tile_services_invent_for_unknown_domains() {
        // Grey on the exact field colour the tile uses, with no colour anywhere.
        let tile = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, false, 8, 64, 64).unwrap();
        tile.fill(0xE2E2_E2FF);
        for x in 20..40 {
            for y in 20..40 {
                tile.put_pixel(x, y, 120, 120, 120, 255);
            }
        }
        let tile = tile.save_to_bufferv("png", &[]).unwrap();
        assert_eq!(detail(&tile), 0, "a generated tile counts as no icon");
        // A real logo that merely starts on the same grey keeps its score, and
        // its hard edges do not read as the blocks of an enlarged icon.
        let logo = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, false, 8, 64, 64).unwrap();
        logo.fill(0xE2E2_E2FF);
        for x in 20..40 {
            for y in 20..40 {
                logo.put_pixel(x, y, 230, 0, 0, 255);
            }
        }
        assert!(detail(&logo.save_to_bufferv("png", &[]).unwrap()) >= MIN_ICON);
    }

    /// Paints `shape` over a transparent canvas, in the style of a real logo.
    fn drawn(side: i32, shape: impl Fn(i32, i32) -> Option<(u8, u8, u8)>) -> Vec<u8> {
        let pixbuf =
            gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, side, side).unwrap();
        pixbuf.fill(0x0000_0000);
        for y in 0..side {
            for x in 0..side {
                if let Some((r, g, b)) = shape(x, y) {
                    pixbuf.put_pixel(x as u32, y as u32, r, g, b, 255);
                }
            }
        }
        pixbuf.save_to_bufferv("png", &[]).unwrap()
    }

    #[test]
    fn plates_a_solid_logo_in_its_own_colour_so_no_bezel_shows() {
        // A disc that fills the icon, as most brand marks do. Laid on white its
        // antialiased rim would blend pale and ring the avatar.
        let middle = 64.0;
        let disc = drawn(128, |x, y| {
            let (dx, dy) = (f64::from(x) - middle, f64::from(y) - middle);
            (dx * dx + dy * dy <= middle * middle).then_some((230, 2, 2))
        });
        assert_eq!(background(&decode(&disc).unwrap()), 0xE602_02FF);

        // Quadrants of different colours, like the Windows logo, with gaps
        // between them. No single colour matches the rim, and choosing one
        // would flood those gaps with it.
        let quadrants = drawn(128, |x, y| {
            let gap = (x - 64).abs() < 6 || (y - 64).abs() < 6;
            (!gap).then_some(match (x < 64, y < 64) {
                (true, true) => (242, 80, 34),
                (false, true) => (127, 186, 0),
                (true, false) => (0, 164, 239),
                (false, false) => (255, 185, 0),
            })
        });
        assert_eq!(background(&decode(&quadrants).unwrap()), 0xFFFF_FFFF);

        // A small mark floating in its own padding has no rim to match.
        let padded = drawn(128, |x, y| {
            ((48..80).contains(&x) && (48..80).contains(&y)).then_some((10, 10, 10))
        });
        assert_eq!(background(&decode(&padded).unwrap()), 0xFFFF_FFFF);
    }

    #[test]
    fn pads_only_when_the_circle_would_cut_into_the_logo() {
        let middle = 64.0;
        let width = |bytes: &[u8]| decode(bytes).unwrap().width();

        // A round logo filling the icon loses nothing to the crop, so it stays
        // edge to edge and keeps every pixel it has.
        let disc = drawn(128, |x, y| {
            let (dx, dy) = (f64::from(x) - middle, f64::from(y) - middle);
            (dx * dx + dy * dy <= middle * middle).then_some((230, 2, 2))
        });
        assert_eq!(width(&plate(&disc).unwrap()), 128);

        // Quadrants of different colours reach the corners, where the circle
        // would cut the logo itself, so the plate grows until they clear it.
        let quadrants = drawn(128, |x, y| {
            let gap = (x - 64).abs() < 6 || (y - 64).abs() < 6;
            (!gap).then_some(match (x < 64, y < 64) {
                (true, true) => (242, 80, 34),
                (false, true) => (127, 186, 0),
                (true, false) => (0, 164, 239),
                (false, false) => (255, 185, 0),
            })
        });
        // Leave visible room between the corners and the circle.
        let grown = width(&plate(&quadrants).unwrap());
        assert!(
            f64::from(grown) >= 128.0 * std::f64::consts::SQRT_2 * 1.14,
            "plate grew to {grown}, too small to clear the corners"
        );

        // A mark on its own opaque background: the corners are background, are
        // indistinguishable from the plate, and are not worth protecting.
        let framed = drawn(128, |x, y| {
            Some(if (40..88).contains(&x) && (40..88).contains(&y) {
                (20, 20, 20)
            } else {
                (255, 255, 255)
            })
        });
        assert_eq!(width(&plate(&framed).unwrap()), 128);

        // A single-colour square needs no room either: the plate takes its
        // colour, so the cropped corners are invisible.
        let solid = drawn(128, |_, _| Some((17, 71, 109)));
        assert_eq!(width(&plate(&solid).unwrap()), 128);
    }

    #[test]
    fn plates_brand_icons_opaquely_and_edge_to_edge() {
        let loaded = |bytes: &[u8]| decode(bytes).unwrap();

        // A transparent logo is laid on white so it cannot show the window
        // behind it, and the white reaches the edge rather than ringing the
        // logo, because the avatar crops it to a circle anyway.
        let plated = plate(&icon(32, true, 0x0000_0000)).unwrap();
        let flat = loaded(&plated);
        assert_eq!((flat.width(), flat.height()), (32, 32));
        assert!(!flat.has_alpha());
        assert_eq!(
            &flat.read_pixel_bytes()[..3],
            &[0xFF, 0xFF, 0xFF],
            "transparent icons sit on white"
        );

        // An opaque icon covers the plate, so only a non-square one reveals it
        // and then it matches the icon's own corner.
        let flat = loaded(&plate(&icon(64, false, 0x1020_30FF)).unwrap());
        assert_eq!(&flat.read_pixel_bytes()[..3], &[0x10, 0x20, 0x30]);
        assert_eq!(flat.width(), 64);

        // An icon that carries an unused alpha channel counts as opaque: it is
        // its own background and must not be given a white one.
        let opaque = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, 48, 48).unwrap();
        opaque.fill(0xF250_22FF);
        let flat = loaded(&plate(&opaque.save_to_bufferv("png", &[]).unwrap()).unwrap());
        assert_eq!(&flat.read_pixel_bytes()[..3], &[0xF2, 0x50, 0x22]);

        // A logo is never scaled to fit the plate, so it keeps its own detail.
        assert_eq!(loaded(&plate(&icon(256, true, 0)).unwrap()).width(), 256);
        assert_eq!(plate(b"<!DOCTYPE html>"), None);
    }

    #[test]
    fn accepts_image_payloads_and_rejects_error_pages() {
        assert!(is_image(b"\x89PNG\r\n\x1a\n\x00"));
        assert!(is_image(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(is_image(b"GIF89a....."));
        assert!(is_image(b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(!is_image(b"<!DOCTYPE html><html>"));
        assert!(!is_image(b""));
    }

    #[test]
    #[ignore = "Requires network access to gravatar.com"]
    fn downloads_real_gravatars_and_reports_missing_ones_as_a_definitive_miss() {
        // `d=identicon` always has an image, so this exercises the whole
        // transport: TLS, redirects, status handling and image sniffing.
        let url = gravatar_url("brevlada@example.com").replace("&d=404", "&d=identicon");
        let image = download(&url).unwrap().expect("identicon fallback");
        assert!(is_image(&image));
        // The same address with the real `d=404` has no avatar, and a miss must
        // be reported as such rather than as an unreachable service.
        assert!(
            download(&gravatar_url("brevlada@example.com"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn keeps_avatars_longer_than_misses_and_distrusts_future_timestamps() {
        let now = 1_000_000_000;
        assert!(is_fresh(true, now - TTL.as_secs() as i64 + 1, now));
        assert!(!is_fresh(true, now - TTL.as_secs() as i64, now));
        assert!(is_fresh(false, now - MISS_TTL.as_secs() as i64 + 1, now));
        assert!(!is_fresh(false, now - MISS_TTL.as_secs() as i64, now));
        // A cache written by a clock that has since been corrected.
        assert!(!is_fresh(true, now + 60, now));
    }
}
