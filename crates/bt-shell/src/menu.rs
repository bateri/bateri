//! Ana menü: uygulama menüsü (About, Settings…, Hide, Quit), Edit (Copy,
//! Paste) ve View (Theme ▸, Bigger, Smaller, Actual Size).
//!
//! **Hiçbir öğenin hedefi yok.** Eylem responder zincirinden geçip onu
//! tanımlayan ilk nesneye varıyor: `copy:`/`paste:` first responder
//! `BateriView`'a; `openSettings:`, tema ve punto eylemleri app delegate'e
//! (görünüm değişiminin `appearanceDidChange:`'i ile aynı yol); `terminate:`,
//! `hide:` ve `orderFrontStandardAboutPanel:` `NSApp`'in kendisine. Menü bu
//! yüzden kimseye referans tutmuyor; eylemi karşılayan yoksa AppKit öğeyi devre
//! dışı gösteriyor.
//!
//! Tek istisna Theme ▸'nin **delegate**'i (app delegate): alt menü sabit
//! değil, açılırken `themes/`'ten doluyor ([`fill_themes`]). Delegate zayıf
//! referans; app delegate süreç boyunca yaşıyor.
//!
//! Kısayollar da buradan: AppKit Command'lı tuşu `keyDown:`'dan önce ana
//! menüye veriyor (`performKeyEquivalent:`), yakalanmayanı `view` yutuyor.
//!
//! Dizgiler İngilizce (`CLAUDE.md` → Dil). Uygulama menüsünün menü çubuğundaki
//! başlığı buradan değil süreç adından geliyor; öğelerin adındaki "bateri"
//! elle yazılı. "Edit" adlı menüye AppKit kendi öğelerini (dikte, emoji),
//! "View" adlı menüye tam ekran öğesini ekliyor.

use bt_core::SYSTEM_THEME;
use objc2::rc::Retained;
use objc2::runtime::{ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOn, NSEventModifierFlags, NSMenu, NSMenuDelegate, NSMenuItem,
};
use objc2_foundation::NSString;

/// Menü çubuğunu kurar. `applicationDidFinishLaunching:`'in başında, pencere
/// öne alınmadan: menü uygulama etkinleşmeden yerinde olsun.
///
/// Süreli koşuda da kuruluyor: öğelerin hiçbiri kendiliğinden koşmuyor ve
/// kullanıcıya dokunan öğeler (Settings…, Theme ▸) kararı kendi eyleminde ve
/// dolumunda veriyor (`app::Inputs`).
///
/// `themes`: Theme ▸'nin delegate'i — `menuNeedsUpdate:` ile doldurur.
pub(crate) fn install(mtm: MainThreadMarker, themes: &ProtocolObject<dyn NSMenuDelegate>) {
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
    // Başta boş: öğeleri her açılışta `fill_themes` kuruyor.
    let theme_menu = submenu(mtm, "Theme", &[]);
    if let Some(menu) = theme_menu.submenu() {
        menu.setDelegate(Some(themes));
    }
    let view_menu = submenu(
        mtm,
        "View",
        &[
            theme_menu,
            NSMenuItem::separatorItem(mtm),
            // `+` Shift'li bir karakter: menü Cmd-Shift-= ile eşleşiyor ve
            // "⌘+" gösteriyor. `-` ASCII tire, eksi işareti (U+2212) değil —
            // klavye onu üretmiyor.
            item(mtm, "Bigger", sel!(makeFontBigger:), "+"),
            item(mtm, "Smaller", sel!(makeFontSmaller:), "-"),
            item(mtm, "Actual Size", sel!(resetFontSize:), "0"),
        ],
    );
    let bar = NSMenu::new(mtm);
    bar.addItem(&app_menu);
    bar.addItem(&edit_menu);
    bar.addItem(&view_menu);
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&bar));
}

/// Theme ▸'yi baştan kurar: "Match System", ayırıcı, gömülü temalar ve (varsa)
/// ayırıcıyla kullanıcı temaları. `selected` ayardaki `theme` değeri; eşleşen
/// öğe işaretli.
///
/// Öğenin başlığı temanın adı ve eylem onu okuyor (`selectTheme:`): başlık
/// dosya adından geliyor, çevrilmiyor. "Match System" ayrı eylem, çünkü
/// başlığı bir ad değil.
pub(crate) fn fill_themes(
    mtm: MainThreadMarker,
    menu: &NSMenu,
    selected: &str,
    embedded: &[&str],
    user: &[String],
) {
    menu.removeAllItems();
    let checked = |item: Retained<NSMenuItem>, on: bool| {
        if on {
            item.setState(NSControlStateValueOn);
        }
        item
    };
    menu.addItem(&checked(
        item(mtm, "Match System", sel!(matchSystemTheme:), ""),
        selected == SYSTEM_THEME,
    ));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    for name in embedded {
        menu.addItem(&checked(
            item(mtm, name, sel!(selectTheme:), ""),
            *name == selected,
        ));
    }
    if !user.is_empty() {
        menu.addItem(&NSMenuItem::separatorItem(mtm));
    }
    for name in user {
        menu.addItem(&checked(
            item(mtm, name, sel!(selectTheme:), ""),
            name == selected,
        ));
    }
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

/// `items`'ı taşıyan menü ve onu üst menüye bağlayan öğe. Öğenin başlığı da
/// `title`: menü çubuğu menünün başlığını, iç içe menü öğeninkini gösteriyor.
fn submenu(
    mtm: MainThreadMarker,
    title: &str,
    items: &[Retained<NSMenuItem>],
) -> Retained<NSMenuItem> {
    let title = NSString::from_str(title);
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &title);
    for item in items {
        menu.addItem(item);
    }
    let holder = NSMenuItem::new(mtm);
    holder.setTitle(&title);
    holder.setSubmenu(Some(&menu));
    holder
}
