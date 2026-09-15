//! Ana menü: uygulama menüsü (About, Settings…, Hide, Quit) ve Edit (Copy,
//! Paste).
//!
//! **Hiçbir öğenin hedefi yok.** Eylem responder zincirinden geçip onu
//! tanımlayan ilk nesneye varıyor: `copy:`/`paste:` first responder
//! `BateriView`'a, `openSettings:` app delegate'e (görünüm değişiminin
//! `appearanceDidChange:`'i ile aynı yol), `terminate:`, `hide:` ve
//! `orderFrontStandardAboutPanel:` `NSApp`'in kendisine. Menü bu yüzden
//! kimseye referans tutmuyor; eylemi karşılayan yoksa AppKit öğeyi devre dışı
//! gösteriyor.
//!
//! Kısayollar da buradan: AppKit Command'lı tuşu `keyDown:`'dan önce ana
//! menüye veriyor (`performKeyEquivalent:`), yakalanmayanı `view` yutuyor.
//!
//! Dizgiler İngilizce (`CLAUDE.md` → Dil). Uygulama menüsünün menü çubuğundaki
//! başlığı buradan değil süreç adından geliyor; öğelerin adındaki "bateri"
//! elle yazılı. "Edit" adlı menüye AppKit kendi öğelerini (dikte, emoji)
//! ekliyor.

use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

/// Menü çubuğunu kurar. `applicationDidFinishLaunching:`'in başında, pencere
/// öne alınmadan: menü uygulama etkinleşmeden yerinde olsun.
///
/// Süreli koşuda da kuruluyor: öğelerin hiçbiri kendiliğinden koşmuyor ve
/// kullanıcıya dokunan tek öğe (Settings…) kararı kendi eyleminde veriyor
/// (`app::Inputs`).
pub(crate) fn install(mtm: MainThreadMarker) {
    let command = NSEventModifierFlags::Command;
    let app_menu = submenu(
        mtm,
        "bateri",
        &[
            item(mtm, "About bateri", sel!(orderFrontStandardAboutPanel:), ""),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Settings…", sel!(openSettings:), ","),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Hide bateri", sel!(hide:), "h"),
            with_modifiers(
                item(mtm, "Hide Others", sel!(hideOtherApplications:), "h"),
                command | NSEventModifierFlags::Option,
            ),
            item(mtm, "Show All", sel!(unhideAllApplications:), ""),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Quit bateri", sel!(terminate:), "q"),
        ],
    );
    let edit_menu = submenu(
        mtm,
        "Edit",
        &[
            item(mtm, "Copy", sel!(copy:), "c"),
            item(mtm, "Paste", sel!(paste:), "v"),
        ],
    );
    let bar = NSMenu::new(mtm);
    bar.addItem(&app_menu);
    bar.addItem(&edit_menu);
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&bar));
}

/// Hedefsiz bir öğe. `key` boşsa kısayol yok; değiştirici varsayılanı Command.
fn item(mtm: MainThreadMarker, title: &str, action: Sel, key: &str) -> Retained<NSMenuItem> {
    // SAFETY: `action` `sel!` ile kurulmuş geçerli bir seçici ve her alıcısı
    // (`BateriView`, `AppDelegate`, `NSApplication`) onu tek
    // `Option<&AnyObject>` argümanlı, dönüşsüz bir eylem olarak tanımlıyor.
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    }
}

/// Öğenin kısayol değiştiricilerini `modifiers` yapar.
fn with_modifiers(
    item: Retained<NSMenuItem>,
    modifiers: NSEventModifierFlags,
) -> Retained<NSMenuItem> {
    item.setKeyEquivalentModifierMask(modifiers);
    item
}

/// `items`'ı taşıyan menü ve onu menü çubuğuna bağlayan öğe.
fn submenu(
    mtm: MainThreadMarker,
    title: &str,
    items: &[Retained<NSMenuItem>],
) -> Retained<NSMenuItem> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    for item in items {
        menu.addItem(item);
    }
    let holder = NSMenuItem::new(mtm);
    holder.setSubmenu(Some(&menu));
    holder
}
