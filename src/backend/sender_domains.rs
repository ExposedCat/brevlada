/// Domains that host mailboxes for the general public. A message from one of
/// these says nothing about its sender beyond "they have an email address", so
/// the domain's icon must never stand in for a person — otherwise every
/// correspondent with a Gmail address would wear the Gmail logo.
///
/// Brand domains are deliberately absent: mail from `alza.cz` really is from
/// Alza, so its icon is a good avatar.
pub const CONSUMER: &[&str] = &[
    // Global providers
    "gmail.com",
    "googlemail.com",
    "outlook.com",
    "outlook.co.uk",
    "hotmail.com",
    "hotmail.co.uk",
    "hotmail.fr",
    "hotmail.it",
    "hotmail.de",
    "hotmail.es",
    "live.com",
    "live.co.uk",
    "live.nl",
    "msn.com",
    "yahoo.com",
    "yahoo.co.uk",
    "yahoo.co.jp",
    "yahoo.fr",
    "yahoo.de",
    "yahoo.it",
    "yahoo.es",
    "yahoo.ca",
    "yahoo.com.br",
    "ymail.com",
    "rocketmail.com",
    "aol.com",
    "icloud.com",
    "me.com",
    "mac.com",
    "proton.me",
    "protonmail.com",
    "protonmail.ch",
    "pm.me",
    "tutanota.com",
    "tutanota.de",
    "tuta.com",
    "tuta.io",
    "fastmail.com",
    "fastmail.fm",
    "hey.com",
    "zoho.com",
    "zohomail.com",
    "mail.com",
    "email.com",
    "gmx.com",
    "gmx.de",
    "gmx.net",
    "gmx.at",
    "gmx.ch",
    "web.de",
    "yandex.ru",
    "yandex.com",
    "mail.ru",
    "bk.ru",
    "inbox.ru",
    "list.ru",
    "internet.ru",
    "qq.com",
    "foxmail.com",
    "163.com",
    "126.com",
    "sina.com",
    "sina.cn",
    "naver.com",
    "hanmail.net",
    "daum.net",
    "rediffmail.com",
    "hushmail.com",
    "mailfence.com",
    "posteo.de",
    "disroot.org",
    "riseup.net",
    "runbox.com",
    "migadu.com",
    "purelymail.com",
    "mailbox.org",
    // Czech and Slovak
    "seznam.cz",
    "email.cz",
    "centrum.cz",
    "centrum.sk",
    "post.cz",
    "volny.cz",
    "atlas.cz",
    "azet.sk",
    "zoznam.sk",
    // Rest of Europe
    "wp.pl",
    "o2.pl",
    "onet.pl",
    "interia.pl",
    "op.pl",
    "freemail.hu",
    "citromail.hu",
    "abv.bg",
    "mail.bg",
    "libero.it",
    "virgilio.it",
    "alice.it",
    "tiscali.it",
    "orange.fr",
    "wanadoo.fr",
    "free.fr",
    "laposte.net",
    "sfr.fr",
    "neuf.fr",
    "t-online.de",
    "freenet.de",
    "arcor.de",
    "bluewin.ch",
    "telenet.be",
    "skynet.be",
    "xs4all.nl",
    "ziggo.nl",
    "kpnmail.nl",
    "home.nl",
    "online.no",
    "telia.com",
    "bredband.net",
    "spray.se",
    "eircom.net",
    "btinternet.com",
    "sky.com",
    "virginmedia.com",
    "talktalk.net",
    "ntlworld.com",
    "blueyonder.co.uk",
    "mail.ee",
    "inbox.lv",
    "ukr.net",
    "i.ua",
    // North America
    "comcast.net",
    "verizon.net",
    "att.net",
    "sbcglobal.net",
    "bellsouth.net",
    "cox.net",
    "charter.net",
    "earthlink.net",
    "juno.com",
    "netzero.net",
    "optonline.net",
    "roadrunner.com",
    "rr.com",
    "shaw.ca",
    "rogers.com",
    "sympatico.ca",
    "telus.net",
    "videotron.ca",
    // Oceania and Asia-Pacific
    "bigpond.com",
    "bigpond.net.au",
    "optusnet.com.au",
    "iinet.net.au",
    "xtra.co.nz",
];

/// Bulk-mail domains that belong to a brand hosted elsewhere. Google's icon
/// service only knows the brand's main domain, so `redditmail.com` alone
/// answers with a placeholder. Only mappings that actually rescue a miss
/// belong here: `e.linkedin.com` needs no entry because dropping the
/// sub-domain already reaches `linkedin.com`.
pub const BRANDS: &[(&str, &str)] = &[
    ("redditmail.com", "reddit.com"),
    ("twitchmail.com", "twitch.tv"),
    ("meetupmail.com", "meetup.com"),
];

/// The domain whose icon represents `host`.
pub fn brand_of(host: &str) -> String {
    let host = host.trim().trim_end_matches('.').to_lowercase();
    BRANDS
        .iter()
        .find(|(from, _)| *from == host)
        .map(|(_, to)| (*to).to_owned())
        .unwrap_or(host)
}

/// Whether `host` is one of those mailbox providers.
///
/// Matching is exact on purpose. Public mailboxes always sit at the bare
/// domain, so a subdomain such as `offers.proton.me` or `mail.zed.dev` is a
/// service sending on its own behalf and should keep its brand icon.
pub fn is_consumer(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_lowercase();
    CONSUMER.contains(&host.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_bulk_mail_domains_to_the_brand_they_send_for() {
        assert_eq!(brand_of("redditmail.com"), "reddit.com");
        assert_eq!(brand_of("REDDITMAIL.COM."), "reddit.com");
        assert_eq!(brand_of("github.com"), "github.com");
        assert_eq!(brand_of(""), "");
    }

    #[test]
    fn suppresses_icons_for_mailbox_providers_but_not_for_brands() {
        assert!(is_consumer("gmail.com"));
        assert!(is_consumer("GMAIL.COM"));
        assert!(is_consumer("seznam.cz"));
        assert!(is_consumer("proton.me"));
        // A trailing dot is a fully qualified name for the same domain.
        assert!(is_consumer("gmail.com."));
        // Brands keep their icons.
        assert!(!is_consumer("github.com"));
        assert!(!is_consumer("alza.cz"));
        // Nobody holds a mailbox at these, so they are the brand itself
        // sending mail and the brand icon is the right avatar.
        assert!(!is_consumer("offers.proton.me"));
        assert!(!is_consumer("mail.zed.dev"));
        // A lookalike must not inherit the icon of the provider it imitates.
        assert!(!is_consumer("notgmail.com"));
        assert!(!is_consumer("gmail.com.evil.example"));
        assert!(!is_consumer(""));
    }
}
