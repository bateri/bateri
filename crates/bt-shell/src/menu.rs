//! Ana menü: uygulama menüsü (About, Settings…, Hide, Quit), Shell (New
//! Window, New Tab, Close Tab, Close Window), Edit (Cut, Copy, Paste, Paste
//! Escaped Text, Select All, Clear to Start, Clear Scrollback, Find ▸
//! Find…/Find Next/Find Previous/Use Selection for Find), View (Theme ▸,
//! Bigger, Smaller, Actual Size, Scroll to Top, Scroll to Bottom, Page Up,
//! Page Down) ve Window (Minimize,
//! Zoom, sekme geçişi, Select Tab ▸, Move Tab to New Window, Merge All
//! Windows, Bring All to Front). Settings… (⌘,) ayar penceresini açıyor
//! (`settings_window`; 029'a kadar dosyayı editörde açıyordu, o iş artık
//! pencerenin "Open settings.toml" düğmesinde); öğe ve kısayol aynı.
//!
//! **Hiçbir öğenin hedefi yok.** Eylem responder zincirinden geçip onu
//! tanımlayan ilk nesneye varıyor: `cut:`/`copy:`/`paste:`/`pasteEscaped:`/
//! `selectAll:` first responder `BateriView`'a (Cut'ın etkinliği onun
//! `validateMenuItem:`'ında — yalnız dock seçimi varken ve düzenleme kapısı
//! açıkken; Paste Escaped Text'inki panoda metin varken); punto
//! eylemleri, `closeTab:`, `closeWindow:`, `selectTab:`, Find ▸'nin dört
//! eylemi, temizlemenin iki eylemi ve dört kaydırma (alternatif ekranda
//! gri, 034 Karar 2) key pencerenin
//! delegate'ine (`window::TerminalWindow` — pencereye ait);
//! `performMiniaturize:`, `performZoom:` ve sekme eylemleri
//! (`selectNextTab:`, `moveTabToNewWindow:`…) `NSWindow`'un kendisine;
//! `openSettings:`, tema eylemleri ve `newWindow:`/`newTab:` app delegate'e
//! (ayar kaydının `settingsDidChange:`'i ile aynı yol — bütün
//! pencerelere yayılıyorlar ya da pencere yokken de çalışmalılar);
//! `terminate:`, `hide:`, `arrangeInFront:` ve
//! `orderFrontStandardAboutPanel:` `NSApp`'in kendisine. Menü bu
//! yüzden kimseye referans tutmuyor; eylemi karşılayan yoksa AppKit öğeyi devre
//! dışı gösteriyor.
//!
//! Tek istisna Theme ▸'nin **delegate**'i (app delegate): alt menü sabit
//! değil, açılırken `themes/`'ten doluyor ([`fill_themes`]). Delegate zayıf
//! referans; app delegate süreç boyunca yaşıyor.
//!
//! Kısayollar da buradan: AppKit Command'lı tuşu `keyDown:`'dan önce ana
//! menüye veriyor (`performKeyEquivalent:`), yakalanmayanı `view` yutuyor.
//! **Control'lü tuş da** menüye önce soruluyor (026 phase-3, ölçüldü): ⌃⇥ ve
//! ⌃⇧⇥ gizli Window öğeleri olarak sekme geçiriyor ve `keyDown:`'ın Cmd
//! izin listesi el değmeden kalıyor; Ctrl-I zsh'e hâlâ sekme olarak gidiyor.
//! **Fonksiyon tuşu da** (034): ⌘Home/⌘End/⌘PgUp/⌘PgDn View öğeleri,
//! kısayol karakteri AppKit'in fonksiyon tuşu kod noktası
//! (`NSHomeFunctionKey` U+F729 …). Tuş kodlaması değil menü kısayolu — Home/
//! End'in `keyDown:`'da yutulması ve `bt_core::Arrow`'un değişmezi el
//! değmiyor.
//!
//! **Sekme öğelerini AppKit eklemiyor** (ölçüldü): tabbing açıkken View'a
//! Show Tab Bar / Show All Tabs, Window'a pencere yerleşimi öğeleri geliyor
//! ama Show Previous/Next Tab, Move Tab to New Window ve Merge All Windows
//! gelmiyor — o yüzden burada. AppKit Shell'e Close All'u (⌥⌘W, ⌘W'nin
//! alternatifi) kendisi ekliyor.
//!
//! Dizgiler İngilizce (`CLAUDE.md` → Dil). Uygulama menüsünün menü çubuğundaki
//! başlığı buradan değil süreç adından geliyor; öğelerin adındaki "bateri"
//! elle yazılı. "Edit" adlı menüye AppKit kendi öğelerini (dikte, emoji),
//! "View" adlı menüye tam ekran öğesini ekliyor; `setWindowsMenu` ile
//! kaydedilen Window menüsüne de pencere listesini ve yerleşim öğelerini.

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
    // Edit ▸ Find (033 Karar 10): macOS'un alt menüsü ve kısayolları.
    // Seçiciler **kendi adlarımız** — `performFindPanelAction:` alan
    // odaktayken AppKit'in alan düzenleyicisine yutulurdu; karşılayan
    // `TerminalWindow` (pencerenin delegesi, responder zincirinde alanın da
    // üstünde). `keyDown:`'ın Cmd izin listesi değişmiyor: menü tuşu önce
    // yakalıyor (⌘A emsali).
    let find_menu = submenu(
        mtm,
        "Find",
        &[
            item(mtm, "Find…", sel!(findInScrollback:), "f"),
            item(mtm, "Find Next", sel!(findNextMatch:), "g"),
            with_modifiers(
                item(mtm, "Find Previous", sel!(findPreviousMatch:), "g"),
                command | NSEventModifierFlags::Shift,
            ),
            item(
                mtm,
                "Use Selection for Find",
                sel!(useSelectionForFind:),
                "e",
            ),
        ],
    );
    let edit_menu = submenu(
        mtm,
        "Edit",
        &[
            item(mtm, "Cut", sel!(cut:), "x"),
            item(mtm, "Copy", sel!(copy:), "c"),
            item(mtm, "Paste", sel!(paste:), "v"),
            with_modifiers(
                item(mtm, "Paste Escaped Text", sel!(pasteEscaped:), "v"),
                command | NSEventModifierFlags::Control,
            ),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Select All", sel!(selectAll:), "a"),
            NSMenuItem::separatorItem(mtm),
            // Terminal.app'in yeri ve kısayolları (034 Karar 4); seçiciler
            // kendi adlarımız, karşılayan `TerminalWindow`.
            item(mtm, "Clear to Start", sel!(clearToStart:), "k"),
            with_modifiers(
                item(mtm, "Clear Scrollback", sel!(clearScrollback:), "k"),
                command | NSEventModifierFlags::Option,
            ),
            NSMenuItem::separatorItem(mtm),
            find_menu,
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
            NSMenuItem::separatorItem(mtm),
            // AppKit'in fonksiyon tuşu kod noktaları (`NSHomeFunctionKey`,
            // `NSEndFunctionKey`, `NSPageUpFunctionKey`,
            // `NSPageDownFunctionKey`): menü onları "⌘↖" gibi gösteriyor.
            item(mtm, "Scroll to Top", sel!(scrollToTop:), "\u{F729}"),
            item(mtm, "Scroll to Bottom", sel!(scrollToBottom:), "\u{F72B}"),
            item(mtm, "Page Up", sel!(scrollPageUp:), "\u{F72C}"),
            item(mtm, "Page Down", sel!(scrollPageDown:), "\u{F72D}"),
        ],
    );
    let shell_menu = submenu(
        mtm,
        "Shell",
        &[
            item(mtm, "New Window", sel!(newWindow:), "n"),
            item(mtm, "New Tab", sel!(newTab:), "t"),
            NSMenuItem::separatorItem(mtm),
            // `performClose:` değil (028 phase-2, ölçüldü): kırmızı düğmenin
            // iptal edilen grup kapanışından sonra AppKit `performClose:`'u
            // grubun tamamına yayıyor, yani ⌘W bir sekme yerine pencereyi
            // sorardı.
            item(mtm, "Close Tab", sel!(closeTab:), "w"),
            with_modifiers(
                item(mtm, "Close Window", sel!(closeWindow:), "w"),
                command | NSEventModifierFlags::Shift,
            ),
        ],
    );
    let mut select_tab: Vec<_> = (1..=8)
        .map(|n| {
            tagged(
                item(mtm, &format!("Tab {n}"), sel!(selectTab:), &n.to_string()),
                n,
            )
        })
        .collect();
    select_tab.push(tagged(item(mtm, "Last Tab", sel!(selectTab:), "9"), 9));
    let window_menu = submenu(
        mtm,
        "Window",
        &[
            item(mtm, "Minimize", sel!(performMiniaturize:), "m"),
            item(mtm, "Zoom", sel!(performZoom:), ""),
            NSMenuItem::separatorItem(mtm),
            // `{`/`}` Shift'li karakterler: menü ⇧⌘[ / ⇧⌘] ile eşleşiyor
            // (Bigger'ın `+`'sıyla aynı deyim).
            item(mtm, "Show Previous Tab", sel!(selectPreviousTab:), "{"),
            item(mtm, "Show Next Tab", sel!(selectNextTab:), "}"),
            hidden_shortcut(with_modifiers(
                item(mtm, "Show Previous Tab", sel!(selectPreviousTab:), "\t"),
                NSEventModifierFlags::Control | NSEventModifierFlags::Shift,
            )),
            hidden_shortcut(with_modifiers(
                item(mtm, "Show Next Tab", sel!(selectNextTab:), "\t"),
                NSEventModifierFlags::Control,
            )),
            submenu(mtm, "Select Tab", &select_tab),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Move Tab to New Window", sel!(moveTabToNewWindow:), ""),
            item(mtm, "Merge All Windows", sel!(mergeAllWindows:), ""),
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Bring All to Front", sel!(arrangeInFront:), ""),
        ],
    );
    let bar = NSMenu::new(mtm);
    bar.addItem(&app_menu);
    bar.addItem(&shell_menu);
    bar.addItem(&edit_menu);
    bar.addItem(&view_menu);
    bar.addItem(&window_menu);
    let app = NSApplication::sharedApplication(mtm);
    app.setMainMenu(Some(&bar));
    app.setWindowsMenu(window_menu.submenu().as_deref());
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
    // (`BateriView`, `TerminalWindow`, `NSWindow`, `AppDelegate`, `NSApplication`) onu tek
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

/// Görünmeyen ama kısayolu **çalışan** öğe: ikinci bir kısayolu, başlığı iki
/// kez göstermeden menüye bağlıyor (⌃⇥ / ⌃⇧⇥ — Safari'nin deyimi). Gizli
/// öğenin kısayolu varsayılan olarak yok sayılıyor; bayrak onu geri açıyor.
fn hidden_shortcut(item: Retained<NSMenuItem>) -> Retained<NSMenuItem> {
    item.setHidden(true);
    item.setAllowsKeyEquivalentWhenHidden(true);
    item
}

/// Öğenin `tag`'ini `tag` yapar — Select Tab ▸'nin sırası
/// (`window::tab_index`).
fn tagged(item: Retained<NSMenuItem>, tag: u8) -> Retained<NSMenuItem> {
    item.setTag(isize::from(tag));
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
