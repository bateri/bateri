//! The system's language and region, read from `NSLocale` for the shell's
//! locale decision (`bt_shell_common::child::locale_env`).
//!
//! The Foundation read lives here because the shared crate sees no
//! Foundation; the decision stays there.

use bt_shell_common::child::primary_language;
use objc2_foundation::NSLocale;

/// The system's `(language, region)` pair, if both are known.
///
/// **The language comes from the first of `preferredLanguages`**, not from
/// `currentLocale().languageCode`: inside the bundle that one gives not the
/// **user's** language but the language chosen from the bundle's
/// localizations — `bateri.app` carries no `.lproj` and
/// `CFBundleDevelopmentRegion` is `en`, so `en` on every Dock launch. Probed
/// with the bundle (`-AppleLanguages (tr-TR) -AppleLocale tr_TR`): a Turkish
/// user got the fallback locale instead of `tr_TR.UTF-8`, a `fr-CA` user got
/// `en_CA.UTF-8` instead of `fr_CA.UTF-8`. Since `cargo run` (unbundled)
/// gives the right language, the bug did not show there. alacritty uses
/// `currentLocale`; not followed.
///
/// **The region comes from `currentLocale().regionCode`** (the bundle does
/// not affect it), not from `countryCode`: the SDK marks the latter as going
/// away in favour of `regionCode` (`#[deprecated]` in objc2, an error under
/// `-D warnings`). `regionCode` arrived in macOS 14; the baseline is 14
/// anyway. The difference is the `@rg=` subtag: if the user separately chose
/// a region format (`en_US@rg=gbzzzz`), it gives that region.
pub(crate) fn system_locale() -> Option<(String, String)> {
    let language = NSLocale::preferredLanguages().firstObject();
    let region = NSLocale::currentLocale().regionCode();
    language.zip(region).and_then(|(tag, region)| {
        let language = primary_language(&tag.to_string())?.to_owned();
        Some((language, region.to_string()))
    })
}
