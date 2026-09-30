//! Güncelleme: Sparkle 2'nin standart güncelleyicisi
//! (`SPUStandardUpdaterController`) ve onun "Check for Updates…" öğesi.
//!
//! **Framework link'lenmiyor, çalışma zamanında yükleniyor.** Sparkle
//! paketin `Contents/Frameworks/`'ünde yalnız `make kur`'un kurduğu pakette
//! var; `cargo run`, `make duman` ve sınamalar onu görmüyor. Link'lenseydi o
//! yolların hepsi dyld'de düşerdi, ya da her biri framework'ü indirmek
//! zorunda kalırdı. Yükleme `NSBundle`'dan, sınıf adıyla
//! (`AnyClass::get`, eksikte `None` — `class!` panikler): framework yoksa
//! güncelleyici hiç doğmuyor ve menü öğesi hiç eklenmiyor.
//!
//! Davranışın tamamı Sparkle'ın ve `Info.plist`'in (`SUFeedURL`,
//! `SUPublicEDKey`, `SUEnableAutomaticChecks`): günde bir arka plan
//! kontrolü, yeni sürümde soru, sessiz kurulum yok. Burada ayar yok —
//! Sparkle'ın kendi tercihleri `NSUserDefaults`'ta ve belgesi onların üstüne
//! ikinci bir katman kurmamayı istiyor.
//!
//! Süreli koşuda (`BT_RUN_SECONDS`) hiç başlatılmıyor: hermetik koşu ağa
//! çıkmaz ve bir güncelleme sorusu pencereyi örtmemeli. Karar çağıranda
//! (`app`).

use objc2::msg_send;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_foundation::{NSBundle, NSString};

/// Framework'ün paketteki adı; yolu `Contents/Frameworks/` altında.
const FRAMEWORK: &str = "Sparkle.framework";

/// Paketteki Sparkle'ı yükler ve güncelleyiciyi **başlatarak** kurar
/// (`initWithStartingUpdater:YES`). Framework yoksa, yüklenemiyorsa ya da
/// sınıf bulunamıyorsa `None` — güncellemesiz bir bateri hâlâ tam bir
/// terminal, yani hiçbir kol ölümcül değil.
///
/// Dönen nesne süreç boyunca tutulmalı: menü öğesinin hedefi o ve
/// `NSMenuItem` hedefini zayıf tutuyor.
pub(crate) fn start() -> Option<Retained<AnyObject>> {
    let frameworks = NSBundle::mainBundle().privateFrameworksPath()?;
    let path = NSString::from_str(&format!("{frameworks}/{FRAMEWORK}"));
    let sparkle = NSBundle::bundleWithPath(&path)?;
    // SAFETY: `load` framework'ün kodunu sürece bağlar; paketteki kopya
    // bizim kimliğimizle imzalı (hardened runtime'ın kütüphane doğrulaması
    // başka bir kimliği reddederdi) ve yan etkisi yalnız sınıf kaydı.
    if !unsafe { sparkle.load() } {
        eprintln!("bateri: {FRAMEWORK} yüklenemedi; güncelleme kapalı");
        return None;
    }
    let class = AnyClass::get(c"SPUStandardUpdaterController")?;
    let none: Option<&AnyObject> = None;
    // SAFETY: Sparkle 2'nin belgelenmiş başlatıcısı:
    // `-initWithStartingUpdater:(BOOL) updaterDelegate:(id) userDriverDelegate:(id)`,
    // iki delegate de nullable. Ana thread'deyiz (`applicationDidFinishLaunching:`).
    unsafe {
        let allocated: Allocated<AnyObject> = msg_send![class, alloc];
        msg_send![
            allocated,
            initWithStartingUpdater: Bool::YES,
            updaterDelegate: none,
            userDriverDelegate: none,
        ]
    }
}
