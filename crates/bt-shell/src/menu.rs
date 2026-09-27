//! Ana menü: uygulama menüsü (About, Settings…, Hide, Quit), Shell (New
//! Window, New Tab, New Local Tab, Mark Host as ▸, Cancel Upload, Split
//! Right, Split Down, Close Tab/Close, Close Window), Edit (Cut, Copy, Paste, Paste
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
//! eylemleri, Find ▸'nin dört eylemi, temizlemenin iki eylemi, dört
//! kaydırma (alternatif ekranda gri, 034 Karar 2) ve `cancelUpload:`
//! odaktaki pane'e (`pane::TerminalPane`, `BateriView`'ın üst view'ı — 039
//! Karar 2); `closeTab:`, `closeWindow:`, `selectTab:`, `splitRight:` ve
//! `splitDown:` key pencerenin delegate'ine (`window::TerminalWindow` —
//! sekmeye ait);
//! `performMiniaturize:`, `performZoom:` ve sekme eylemleri
//! (`selectNextTab:`, `moveTabToNewWindow:`…) `NSWindow`'un kendisine;
//! `openSettings:`, tema eylemleri, `markHost:` ve
//! `newWindow:`/`newTab:`/`newLocalTab:` app delegate'e
//! (ayar kaydının `settingsDidChange:`'i ile aynı yol — bütün
//! pencerelere yayılıyorlar ya da pencere yokken de çalışmalılar);
//! `terminate:`, `hide:`, `arrangeInFront:` ve
//! `orderFrontStandardAboutPanel:` `NSApp`'in kendisine. Menü bu
//! yüzden kimseye referans tutmuyor; eylemi karşılayan yoksa AppKit öğeyi devre
//! dışı gösteriyor.
//!
//! İki istisna delegate'ler. Theme ▸'ninki app delegate: alt menü sabit
//! değil, açılırken `themes/`'ten doluyor ([`fill_themes`]). Shell'inki
//! [`ShellMenuDelegate`]: Mark “{host}” as ▸'nin başlığı, etkinliği ve onay
//! işareti etkin sekmeye göre ([`mark_menu`], 037 Karar 5). Delegate zayıf
//! referans; ikisini de app delegate süreç boyunca yaşatıyor.
//!
//! Shell'in delegate'i **ayrı bir nesne ve yalnız `menuWillOpen:`**:
//! `menuNeedsUpdate:` ya da `menuHasKeyEquivalent:…` tanımlayan delegate
//! AppKit'in kısayol aramasına giriyor — app delegate'inki "kısayol yok"
//! diyor ve Shell'e bağlansaydı ⌘N/⌘T/⌘W ölürdü. Alt menünün tutucusu
//! doğrulamadan geçmiyor (ölçüldü: hedefi alt menünün kendisi,
//! `submenuAction:`, ve `update` onun `setEnabled`'ına dokunmuyor), yani
//! gri ve başlık açılışta elle kuruluyor.
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

use bt_core::{HostMark, SYSTEM_THEME, bare_host};
use objc2::rc::Retained;
use objc2::runtime::{ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags, NSMenu,
    NSMenuDelegate, NSMenuItem,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};

/// Shell ▸ Mark … as ▸'nin tutucusunun `tag`'i: [`ShellMenuDelegate`] onu
/// Shell menüsünde bununla buluyor.
const MARK_HOLDER_TAG: isize = 37;

/// Mark … as ▸'nin öğeleri, sırasıyla; öğenin `tag`'i buradaki indeks ve
/// eylem (`markHost:`) işareti oradan okuyor ([`mark_of_tag`]). Doğrudan renk
/// yok: menü onu hiç yazmıyor (037 Karar 2).
const MARKS: [(&str, HostMark); 4] = [
    ("Production", HostMark::Production),
    ("Staging", HostMark::Staging),
    ("Development", HostMark::Development),
    ("None", HostMark::None),
];

/// Mark … as ▸'nin öğesinin `tag`'inden işaret; bilinmeyen `tag` `None`.
pub(crate) fn mark_of_tag(tag: isize) -> Option<HostMark> {
    usize::try_from(tag)
        .ok()
        .and_then(|index| MARKS.get(index))
        .map(|(_, mark)| *mark)
}

/// Mark … as ▸'nin o anki hâli ([`mark_menu`]).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MarkMenu {
    pub(crate) title: String,
    pub(crate) enabled: bool,
    /// Onaylı öğenin [`MARKS`]'taki indeksi; doğrudan renkte ve yerelde yok.
    pub(crate) checked: Option<usize>,
}

/// Menünün modeli, saf (037 Karar 5): uzak sekmede başlık host'un
/// `user@`'siz kısmını taşıyor ve onay **geçerli çözümde** (glob'dan gelse
/// de); yerelde (`None`) "Mark Host as" ve gri.
pub(crate) fn mark_menu(remote: Option<(&str, HostMark)>) -> MarkMenu {
    match remote {
        Some((host, mark)) => MarkMenu {
            title: format!("Mark \u{201c}{}\u{201d} as", bare_host(host)),
            enabled: true,
            checked: MARKS.iter().position(|(_, candidate)| *candidate == mark),
        },
        None => MarkMenu {
            title: "Mark Host as".to_owned(),
            enabled: false,
            checked: None,
        },
    }
}

define_class!(
    // SAFETY: NSObject alt sınıflama şartı taşımaz; Drop uygulanmıyor.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriShellMenuDelegate"]
    pub(crate) struct ShellMenuDelegate;

    unsafe impl NSObjectProtocol for ShellMenuDelegate {}

    unsafe impl NSMenuDelegate for ShellMenuDelegate {
        /// Shell açılıyor: Mark … as ▸ etkin sekmenin uzak hâlinden
        /// kuruluyor. Yalnız açılışta — kısayol araması buraya uğramıyor.
        #[unsafe(method(menuWillOpen:))]
        fn menu_will_open(&self, menu: &NSMenu) {
            let Some(holder) = menu.itemWithTag(MARK_HOLDER_TAG) else {
                return;
            };
            let remote = crate::app::delegate(self.mtm()).and_then(|app| app.key_remote_mark());
            let model = mark_menu(remote.as_ref().map(|(host, mark)| (host.as_str(), *mark)));
            holder.setTitle(&NSString::from_str(&model.title));
            holder.setEnabled(model.enabled);
            if let Some(submenu) = holder.submenu() {
                for (index, item) in submenu.itemArray().iter().enumerate() {
                    item.setState(if model.checked == Some(index) {
                        NSControlStateValueOn
                    } else {
                        NSControlStateValueOff
                    });
                }
            }
        }
    }
);

impl ShellMenuDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `NSObject`'in `init`'i; alt sınıfın ivar'ı yok.
        unsafe { msg_send![super(this), init] }
    }
}

/// Menü çubuğunu kurar. `applicationDidFinishLaunching:`'in başında, pencere
/// öne alınmadan: menü uygulama etkinleşmeden yerinde olsun.
///
/// Süreli koşuda da kuruluyor: öğelerin hiçbiri kendiliğinden koşmuyor ve
/// kullanıcıya dokunan öğeler (Settings…, Theme ▸) kararı kendi eyleminde ve
/// dolumunda veriyor (`app::Inputs`).
///
/// `themes`: Theme ▸'nin delegate'i — `menuNeedsUpdate:` ile doldurur.
/// Dönen Shell menüsünün delegate'i; delegate zayıf referans, çağıran onu
/// süreç boyunca tutuyor.
pub(crate) fn install(
    mtm: MainThreadMarker,
    themes: &ProtocolObject<dyn NSMenuDelegate>,
) -> Retained<ShellMenuDelegate> {
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
    // `TerminalPane` (alanın atası, responder zincirinde alanın da üstünde). `keyDown:`'ın Cmd izin listesi değişmiyor: menü tuşu önce
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
            // kendi adlarımız, karşılayan `TerminalPane`.
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
            // Uzak sekmede ⌘T aynı host'a gidiyor; bu her zaman yerel (037
            // Karar 6).
            with_modifiers(
                item(mtm, "New Local Tab", sel!(newLocalTab:), "t"),
                command | NSEventModifierFlags::Option,
            ),
            NSMenuItem::separatorItem(mtm),
            // Başlığı ve grisi açılışta ([`ShellMenuDelegate`]); öğeler
            // `markHost:`'la app delegate'e (etkin sekmenin host'u).
            mark_holder(mtm),
            // Uzak dizine yüklemenin bütün kuyruğu (037 Karar 7); yalnız
            // kuyruk varken etkin (`TerminalPane`'in `validateMenuItem:`'ı).
            item(mtm, "Cancel Upload", sel!(cancelUpload:), "."),
            NSMenuItem::separatorItem(mtm),
            // Bölmeler (039 Karar 8, Ghostty/iTerm2 emsali): karşılayan
            // `TerminalWindow` (odaktaki pane'i böler); en küçük pane
            // sınırında gri (`validateMenuItem:`).
            item(mtm, "Split Right", sel!(splitRight:), "d"),
            with_modifiers(
                item(mtm, "Split Down", sel!(splitDown:), "d"),
                command | NSEventModifierFlags::Shift,
            ),
            NSMenuItem::separatorItem(mtm),
            // `performClose:` değil (028 phase-2, ölçüldü): kırmızı düğmenin
            // iptal edilen grup kapanışından sonra AppKit `performClose:`'u
            // grubun tamamına yayıyor, yani ⌘W bir sekme yerine pencereyi
            // sorardı. Başlığı çok pane'de "Close" (odaktaki pane'i
            // kapatıyor; `TerminalWindow`'un `validateMenuItem:`'ı, 039
            // Karar 8).
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
    let shell_delegate = ShellMenuDelegate::new(mtm);
    if let Some(menu) = shell_menu.submenu() {
        menu.setDelegate(Some(ProtocolObject::from_ref(&*shell_delegate)));
    }
    shell_delegate
}

/// Shell ▸ Mark … as ▸: dört öğe ([`MARKS`]), `tag`'leri sıraları. Başta
/// yerel sekmenin hâli; açılışta [`ShellMenuDelegate`] kuruyor.
fn mark_holder(mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    let items: Vec<_> = (0u8..)
        .zip(MARKS)
        .map(|(tag, (title, _))| tagged(item(mtm, title, sel!(markHost:), ""), tag))
        .collect();
    let model = mark_menu(None);
    let holder = submenu(mtm, &model.title, &items);
    holder.setTag(MARK_HOLDER_TAG);
    holder.setEnabled(model.enabled);
    holder
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
    // (`BateriView`, `TerminalPane`, `TerminalWindow`, `NSWindow`, `AppDelegate`,
    // `NSApplication`) onu tek
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
/// (`window::tab_index`) ve Mark … as ▸'nin işareti ([`mark_of_tag`]).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mark_menu_follows_the_active_tab() {
        // Yerel sekme: genel ad ve gri.
        assert_eq!(
            mark_menu(None),
            MarkMenu {
                title: "Mark Host as".to_owned(),
                enabled: false,
                checked: None,
            }
        );
        // Uzak sekme: başlıkta `user@`'siz host, onay geçerli çözümde.
        assert_eq!(
            mark_menu(Some(("deploy@prod-web", HostMark::Staging))),
            MarkMenu {
                title: "Mark \u{201c}prod-web\u{201d} as".to_owned(),
                enabled: true,
                checked: Some(1),
            }
        );
        // İşaretsiz host "None"da onaylı; doğrudan renk hiçbir öğede değil.
        assert_eq!(mark_menu(Some(("vm", HostMark::None))).checked, Some(3));
        assert_eq!(
            mark_menu(Some(("vm", HostMark::Rgb(0xc678dd)))).checked,
            None
        );
    }

    #[test]
    fn every_mark_item_reads_back_its_mark() {
        for (index, (_, mark)) in MARKS.iter().enumerate() {
            let tag = isize::try_from(index).expect("küçük indeks");
            assert_eq!(mark_of_tag(tag), Some(*mark));
            assert_eq!(mark_menu(Some(("h", *mark))).checked, Some(index));
        }
        assert_eq!(mark_of_tag(-1), None);
        assert_eq!(mark_of_tag(4), None);
    }
}
