//! Which records of a profile are a site's: what "clear this site's data" means to the sync.
//!
//! The helper deletes a site's data from the engine and says what it found ([`Event::SiteCleared`]),
//! but it can name only the Local Storage of the origins it has seen. The sync knows more: every
//! record it ever took from the home or gave to it. So the sync says the host's deletion for each
//! of those records too, and a Mac that has not had the site open (or an origin the helper never
//! saw) is signed out all the same when it next opens it. What counts as the host's is the
//! helper's rule (`helper/src/clear.rs` of the browser's repository: `cookie_goes_to`,
//! `host_of_origin`, `origin_of_folder`, with the same vectors in the tests below), written here
//! once more because the helper's crate is not a dependency of bateri's.

use browser_ipc::message::Record;

/// The host name of an origin (`https://a.test:8443`, with or without a closing slash), in lower
/// case.
pub fn host_of_origin(origin: &str) -> Option<String> {
    let rest = origin.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = if authority.starts_with('[') {
        let end = authority.find(']')?;
        authority.get(..=end)?
    } else {
        authority.split(':').next()?
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// The origin a folder of IndexedDB is named for (`http_localhost_3000`; port 0 is the scheme's
/// own), written as an address's origin.
pub fn origin_of_folder(name: &str) -> Option<String> {
    let (scheme, rest) = name.split_once('_')?;
    let (host, port) = rest.rsplit_once('_')?;
    if !matches!(scheme, "http" | "https") || host.is_empty() || port.parse::<u16>().is_err() {
        return None;
    }
    Some(if port == "0" {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}:{port}")
    })
}

/// Whether a cookie of `domain` is sent to `host`: a cookie of a name (without a leading dot)
/// belongs to that one host, a cookie of a domain (with it) to the domain and everything below.
pub fn cookie_goes_to(domain: &str, host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let bare = domain.trim_start_matches('.').to_ascii_lowercase();
    if bare.is_empty() {
        return false;
    }
    host == bare || (domain.starts_with('.') && host.ends_with(&format!(".{bare}")))
}

/// Whether `record` is something the site `host` keeps (a name, in lower case, not an address),
/// at any port.
pub fn is_the_hosts(record: &Record, host: &str) -> bool {
    match record {
        Record::Cookie { domain, .. } => cookie_goes_to(domain, host),
        Record::LocalStorage { origin, .. } => host_of_origin(origin).is_some_and(|h| h == host),
        Record::IndexedDb { origin, .. } => origin_of_folder(origin)
            .and_then(|origin| host_of_origin(&origin))
            .is_some_and(|h| h == host),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_of_an_origin_has_no_scheme_port_or_slash() {
        assert_eq!(
            host_of_origin("http://localhost:3000").as_deref(),
            Some("localhost")
        );
        assert_eq!(host_of_origin("https://a.test/").as_deref(), Some("a.test"));
        assert_eq!(
            host_of_origin("http://[::1]:8080/x").as_deref(),
            Some("[::1]")
        );
        assert_eq!(
            host_of_origin("https://u:p@a.test:1/").as_deref(),
            Some("a.test")
        );
        assert_eq!(
            host_of_origin("https://A.Test:8443").as_deref(),
            Some("a.test")
        );
        assert_eq!(host_of_origin("about:blank"), None);
    }

    #[test]
    fn a_folder_of_indexed_db_names_its_origin() {
        assert_eq!(
            origin_of_folder("http_localhost_3000").as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(
            origin_of_folder("https_example.com_0").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(origin_of_folder("file__0"), None);
        assert_eq!(origin_of_folder("http_a_b_x"), None);
    }

    #[test]
    fn a_cookie_of_a_domain_goes_to_the_domain_and_what_is_below_it() {
        assert!(cookie_goes_to("localhost", "localhost"));
        assert!(cookie_goes_to(".example.com", "app.example.com"));
        assert!(cookie_goes_to(".example.com", "example.com"));
        assert!(cookie_goes_to("app.example.com", "app.example.com"));
        // A cookie of a name belongs to that name alone; a parent's host-only one is not sent down.
        assert!(!cookie_goes_to("example.com", "app.example.com"));
        assert!(!cookie_goes_to(".app.example.com", "example.com"));
        assert!(!cookie_goes_to(".example.com", "badexample.com"));
        assert!(!cookie_goes_to("", "localhost"));
    }

    #[test]
    fn the_records_of_a_host_are_its_cookies_and_what_it_stores_at_any_port() {
        let cookie = |domain: &str| Record::Cookie {
            domain: domain.into(),
            path: "/".into(),
            name: "sid".into(),
            partition: None,
            value: None,
        };
        let item = |origin: &str| Record::LocalStorage {
            origin: origin.into(),
            key: "k".into(),
            value: None,
        };
        let folder = |origin: &str| Record::IndexedDb {
            origin: origin.into(),
            value: None,
        };
        assert!(is_the_hosts(&cookie(".site.test"), "site.test"));
        assert!(!is_the_hosts(&cookie("other.test"), "site.test"));
        assert!(is_the_hosts(&item("https://site.test"), "site.test"));
        assert!(is_the_hosts(&item("https://site.test:8443/"), "site.test"));
        assert!(!is_the_hosts(&item("https://sub.site.test"), "site.test"));
        assert!(is_the_hosts(&folder("https_site.test_8443"), "site.test"));
        assert!(is_the_hosts(&folder("http_site.test_0"), "site.test"));
        assert!(!is_the_hosts(&folder("https_other.test_0"), "site.test"));
        assert!(!is_the_hosts(&folder("not a folder"), "site.test"));
    }
}
