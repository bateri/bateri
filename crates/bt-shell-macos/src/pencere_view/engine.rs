//! What the web engine opens beside the page's policy, and the switches that close it.
//!
//! The policy of a page (`serve`: the `Content-Security-Policy` of every answer) governs what the
//! page loads and sends. WebKit also reaches the network in ways no policy names (specification
//! §12.2): a peer connection a script opens (WebRTC), whose ICE agent sends STUN over UDP and
//! connects TURN over TCP to the addresses the page chooses, loopback and private ones included;
//! the connection the engine opens ahead of a request for a `<link rel=preconnect>`; and the
//! lookups of names it makes ahead (DNS prefetching), with the prefetch of `<link rel=prefetch>`
//! beside it. They are closed for every page, whatever the person allowed: allowing an origin
//! allows requests to it and none of these. Measured on a real `WKWebView` before this module
//! closed them (`tests/pencere.rs`, `make test-view`): a page with no `access` made WebKit send
//! three STUN datagrams and one TURN connection to loopback and open one preconnect.
//!
//! They are closed in the engine, by its own preferences, and not by a user script that deletes
//! `RTCPeerConnection` from the page: another frame of the page gives the interface back. The
//! preferences are WebKit's private ones (`-[WKPreferences _setPeerConnectionEnabled:]`,
//! `_setDNSPrefetchingEnabled:` and the feature list `+[WKPreferences _features]` that
//! `_setEnabled:forFeature:` writes), so each is looked for at run time and **a channel that cannot
//! be closed means no web view**: the specification says a host that cannot close one MUST NOT open
//! one, and the caller then closes the program's view.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send, sel};
use objc2_foundation::{NSArray, NSString};

/// A channel the engine opens beside the policy, with the ways WebKit has of closing it.
struct Channel {
    /// What the specification calls it, for the one line that says why a view did not open.
    name: &'static str,
    /// The dedicated setter of `WKPreferences`, if it has one, called with `NO`.
    setter: Option<fn(&AnyObject) -> bool>,
    /// The key of its feature in `+[WKPreferences _features]`, which is set to off.
    feature: Option<&'static str>,
    /// Whether the channel is only closed when one of the two ways above was found. A channel the
    /// engine does not have at all (a prefetch that is no feature of this WebKit) is none to close.
    required: bool,
}

/// `-[WKPreferences _setPeerConnectionEnabled:]` with `NO`, if the preferences answer to it.
fn set_peer_connection(preferences: &AnyObject) -> bool {
    // SAFETY: a message the object was asked about, with the BOOL the property is of.
    unsafe {
        if !responds(preferences, sel!(_setPeerConnectionEnabled:)) {
            return false;
        }
        let _: () = msg_send![preferences, _setPeerConnectionEnabled: Bool::NO];
    }
    true
}

/// `-[WKPreferences _setDNSPrefetchingEnabled:]` with `NO`, if the preferences answer to it.
fn set_dns_prefetching(preferences: &AnyObject) -> bool {
    // SAFETY: as above.
    unsafe {
        if !responds(preferences, sel!(_setDNSPrefetchingEnabled:)) {
            return false;
        }
        let _: () = msg_send![preferences, _setDNSPrefetchingEnabled: Bool::NO];
    }
    true
}

const CHANNELS: [Channel; 4] = [
    Channel {
        name: "peer connections (WebRTC)",
        setter: Some(set_peer_connection),
        feature: Some("PeerConnectionEnabled"),
        required: true,
    },
    Channel {
        name: "DNS prefetching",
        setter: Some(set_dns_prefetching),
        feature: Some("LinkDNSPrefetchEnabled"),
        required: true,
    },
    Channel {
        name: "preconnect",
        setter: None,
        feature: Some("LinkPreconnect"),
        required: true,
    },
    Channel {
        name: "prefetch",
        setter: None,
        feature: Some("LinkPrefetchEnabled"),
        required: false,
    },
];

/// Whether `object` answers to `selector`.
///
/// # Safety
///
/// `object` is a live Objective-C object.
unsafe fn responds(object: &AnyObject, selector: objc2::runtime::Sel) -> bool {
    let answers: Bool = unsafe { msg_send![object, respondsToSelector: selector] };
    answers.as_bool()
}

/// Writes `on` to the engine feature `key` of `preferences`; `false` if WebKit has no such feature
/// (or no list of them).
fn set_feature(preferences: &AnyObject, key: &str, on: bool) -> bool {
    let class = class!(WKPreferences);
    // SAFETY: `+_features` answers an array of `_WKFeature` objects, each of which has a `key`;
    // each message is asked about first, and the one that writes takes the object the list gave.
    unsafe {
        let listed: Bool = msg_send![class, respondsToSelector: objc2::sel!(_features)];
        let writes = responds(preferences, sel!(_setEnabled:forFeature:));
        if !listed.as_bool() || !writes {
            return false;
        }
        let features: Option<Retained<NSArray<AnyObject>>> = msg_send![class, _features];
        let Some(features) = features else {
            return false;
        };
        for feature in features.iter() {
            if !responds(&feature, sel!(key)) {
                continue;
            }
            let name: Option<Retained<NSString>> = msg_send![&*feature, key];
            if name.is_some_and(|name| name.to_string() == key) {
                let _: () =
                    msg_send![preferences, _setEnabled: Bool::new(on), forFeature: &*feature];
                return true;
            }
        }
    }
    false
}

/// Closes every channel of [`CHANNELS`] in `preferences` (a `WKPreferences`), each by every way
/// WebKit has of it. The name of the first it could not close is the error.
pub(super) fn close(preferences: &AnyObject) -> Result<(), &'static str> {
    for channel in &CHANNELS {
        let by_setter = channel.setter.is_some_and(|set| set(preferences));
        let by_feature = channel
            .feature
            .is_some_and(|key| set_feature(preferences, key, false));
        if channel.required && !(by_setter || by_feature) {
            return Err(channel.name);
        }
    }
    Ok(())
}
