//! Geçmişte aramanın paneli (⌘F, 033): pencerenin sağ üstünde, terminalin
//! **üstünde yüzen** bir AppKit yüzeyi — `NSSearchField`, `Aa` ve `.*`
//! anahtarları, sayım etiketi, iki ok ve kapatma.
//!
//! **Neden AppKit** (`.tasks/033-gecmiste-arama/discussion.md` → Karar 1):
//! metin girişinin bütün doğruluğu (ölü tuş, IME, pano, geri alma, VoiceOver)
//! alanla bedava geliyor; Metal'de çizilen bir alan onların her birini
//! yeniden yazardı. Panel içeriği **itmiyor**, üstüne biniyor — PTY boyutu
//! ⌘F'de değişmiyor (032'nin "PTY sabit" kuralı).
//!
//! **Bu dosyada karar yok, görünüş var.** Sorgunun derlenmesi, geçerli
//! eşleşme ve pencerenin eşleşmeye gidişi `bt-core`'da
//! (`Session::set_search`, `search_next`, `search_reveal`); olayların
//! sahibi pencere (`window::TerminalWindow` alanın delegesi ve düğmelerin
//! hedefi — ikisi de zayıf referans, paneli pencere tutuyor). Burada kalan:
//! görünümlerin kurulması, temaya boyanması, yeri ve açılış/kapanış
//! animasyonu, anahtarların ve etiketin okunup yazılması.
//!
//! Durum **sekme başına** (Karar 6): panel kapanınca gizleniyor, sorgu ve iki
//! anahtar alanda kalıyor ve yeniden ⌘F'de seçili geliyor. Ayar dosyasına
//! yazılmıyor.

use std::cell::Cell;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use bt_core::{SearchQuery, SearchReport, SearchStatus, Theme, escape_search};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{MainThreadMarker, Message, sel};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSAutoresizingMaskOptions, NSBezelStyle,
    NSBox, NSBoxType, NSButton, NSButtonType, NSColor, NSControlSize, NSControlStateValueOff,
    NSControlStateValueOn, NSFont, NSFontWeightRegular, NSImage, NSSearchField,
    NSSearchFieldDelegate, NSShadow, NSStackView, NSTextField, NSTitlePosition,
    NSUserInterfaceLayoutOrientation, NSView, NSWindowOrderingMode,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString, ns_string};
use objc2_quartz_core::{
    CAMediaTimingFunction, kCAMediaTimingFunctionEaseIn, kCAMediaTimingFunctionEaseOut,
};

/// Panelin pencerenin iç kenarlarına uzaklığı, nokta. Tasarım sabiti:
/// başlık çubuğunun altında nefes alacak, sağ kenara yapışmayacak kadar.
const INSET: f64 = 10.0;

/// Yüzeyin içindeki pay (yatay, dikey), nokta — alanın yuvarlak köşesi
/// yüzeyin köşesiyle eş merkezli dursun diye dikey pay yatayınkinden dar.
const PADDING: (f64, f64) = (7.0, 6.0);

/// Yüzeyin köşe yarıçapı: alanın (≈ 6 pt) ve payın (≈ 6 pt) toplamına yakın,
/// iç ve dış köşe eş merkezli okunsun.
const RADIUS: f64 = 11.0;

/// Arama alanının genişliği, nokta. Uzun bir yolu ya da deseni sığdıracak,
/// terminalin sağ üst köşesini gereğinden fazla örtmeyecek kadar.
const FIELD_WIDTH: f64 = 190.0;

/// Sayım etiketinin **sabit** genişliği: "Invalid pattern" ve "999 of 9999…"
/// sığıyor, yani sayım ilerlerken panel eni zıplamıyor. Daha uzun bir sayım
/// (beş haneli) kuyruğundan kırpılıyor.
const COUNT_WIDTH: f64 = 96.0;

/// Anahtarların ve simgeli düğmelerin sabit genişliği, nokta: çerçevenin
/// varsayılan iç payı bir-iki harflik başlığı gereğinden geniş gösteriyordu.
const BUTTON_WIDTH: f64 = 24.0;

/// Açılış ve kapanışın süresi, saniye. 030'un gözle bulunan 240 ms
/// tabanından başladı ve gerçek pencerede kısaldı (phase-4 Uygulama
/// Notları): panel küçük ve hareketi kısa, 240 ms'de yazmaya başlayan göz
/// alanı hâlâ yerine oturuyor görüyordu.
const APPEAR_SECS: f64 = 0.18;

/// Belirirken panelin yukarıdan indiği mesafe, nokta — "yerine oturuyor"
/// hissi için yeter, bir hareket olarak okunmayacak kadar kısa.
const SLIDE: f64 = 6.0;

/// Panelin görünümleri ve animasyonun nesli.
pub(crate) struct SearchBar {
    /// Panelin kapsayıcısı (pencerenin içerik view'ı) — yerleşimin ölçüsü.
    parent: Retained<NSView>,
    surface: Retained<NSBox>,
    field: Retained<NSSearchField>,
    case: Retained<NSButton>,
    regex: Retained<NSButton>,
    count: Retained<NSTextField>,
    /// Panel **açık** mı (kapanış animasyonu sürerken de `false`).
    shown: Cell<bool>,
    /// Her açılış ve kapanışta artar: kapanış animasyonunun tamamlanma bloğu
    /// paneli yalnız kendi nesli hâlâ geçerliyse gizliyor — araya giren bir
    /// ⌘F onu gizlememeli.
    generation: Rc<Cell<u64>>,
    /// Oturuma en son verilen sorgu — aynı sorgu ikinci kez `set_search`
    /// doğurmasın (alanın eylemi ve anahtarlar aynı sorguyu tekrar
    /// gönderebiliyor, ve her `set_search` bir kare).
    applied: std::cell::RefCell<Option<SearchQuery>>,
}

impl SearchBar {
    /// Paneli kurar ve `parent`'a, `below`'un **üstüne** ekler; gizli doğar.
    ///
    /// `target` düğmelerin ve alanın eylem hedefi, `delegate` alanın delegesi
    /// — ikisi de pencere ve ikisi de zayıf tutuluyor.
    pub(crate) fn new(
        mtm: MainThreadMarker,
        parent: &NSView,
        below: &NSView,
        target: &AnyObject,
        delegate: &ProtocolObject<dyn NSSearchFieldDelegate>,
    ) -> Self {
        let field = NSSearchField::new(mtm);
        field.setPlaceholderString(Some(ns_string!("Find")));
        // Her değişim eylemi hemen gönderiyor: vurgu yazdıkça (Karar 7) ve
        // alanın ⊗ düğmesi de aynı yoldan (metni silmek `controlTextDidChange:`
        // doğurmuyor).
        field.setSendsSearchStringImmediately(true);
        field.setSendsWholeSearchString(false);
        // SAFETY: hedef zayıf ve pencere paneli yaşattığı sürece yaşıyor;
        // seçici pencerede tek `Option<&AnyObject>` argümanlı bir eylem.
        unsafe {
            field.setTarget(Some(target));
            field.setAction(Some(sel!(searchFieldChanged:)));
            field.setDelegate(Some(delegate));
        }
        width(&field, FIELD_WIDTH);

        let case = toggle(mtm, "Aa", "Match Case", target);
        let regex = toggle(mtm, ".*", "Use Regular Expression", target);

        let count = NSTextField::labelWithString(ns_string!(""), mtm);
        count.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            NSFont::smallSystemFontSize(),
            // SAFETY: AppKit'in dışa açtığı sabit, süreç boyunca yaşıyor.
            unsafe { NSFontWeightRegular },
        )));
        count.setTextColor(Some(&NSColor::secondaryLabelColor()));
        width(&count, COUNT_WIDTH);

        // ⏎ = yukarı, daha eski (Karar 3): yukarı ok ⌘G'nin, aşağı ok ⇧⌘G'nin
        // eylemi — menüyle aynı seçiciler.
        let older = symbol(
            mtm,
            "chevron.up",
            "Find Next (Older)",
            target,
            sel!(findNextMatch:),
        );
        let newer = symbol(
            mtm,
            "chevron.down",
            "Find Previous (Newer)",
            target,
            sel!(findPreviousMatch:),
        );
        let close = symbol(mtm, "xmark", "Close", target, sel!(closeSearch:));

        let views: [&NSView; 7] = [&field, &case, &regex, &count, &older, &newer, &close];
        let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(&views), mtm);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        stack.setSpacing(2.0);
        // Alan ile anahtarlar bir grup, sayım ve oklar ikinci, kapatma
        // üçüncü: gruplar arası boşluk içtekinden geniş.
        stack.setCustomSpacing_afterView(6.0, &field);
        stack.setCustomSpacing_afterView(8.0, &regex);
        stack.setCustomSpacing_afterView(4.0, &count);
        stack.setCustomSpacing_afterView(6.0, &newer);

        let surface = NSBox::new(mtm);
        surface.setBoxType(NSBoxType::Custom);
        surface.setTitlePosition(NSTitlePosition::NoTitle);
        surface.setCornerRadius(RADIUS);
        surface.setBorderWidth(1.0);
        surface.setContentViewMargins(NSSize::new(PADDING.0, PADDING.1));
        surface.setContentView(Some(&stack));
        let shadow = NSShadow::new();
        shadow.setShadowBlurRadius(14.0);
        shadow.setShadowOffset(NSSize::new(0.0, -3.0));
        shadow.setShadowColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
            0.0, 0.0, 0.0, 0.28,
        )));
        surface.setShadow(Some(&shadow));
        let fit = stack.fittingSize();
        surface.setFrameSize(NSSize::new(
            fit.width + 2.0 * PADDING.0 + 2.0,
            fit.height + 2.0 * PADDING.1 + 2.0,
        ));
        // Kapsayıcı çevrilmemiş (y yukarı): sağ üst köşeye yapışmak = sol ve
        // alt kenar esnek.
        surface.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        surface.setHidden(true);
        parent.addSubview_positioned_relativeTo(&surface, NSWindowOrderingMode::Above, Some(below));

        SearchBar {
            parent: parent.retain(),
            surface,
            field,
            case,
            regex,
            count,
            shown: Cell::new(false),
            generation: Rc::new(Cell::new(0)),
            applied: std::cell::RefCell::new(None),
        }
    }

    /// Panel açık mı.
    pub(crate) fn is_shown(&self) -> bool {
        self.shown.get()
    }

    /// Panelin kapsayıcısı — [`SearchBar::resting_frame`]'in koordinatı.
    pub(crate) fn parent(&self) -> &NSView {
        &self.parent
    }

    /// Arama alanı — first responder yapılacak görünüm.
    pub(crate) fn field(&self) -> &NSSearchField {
        &self.field
    }

    /// Panelin **durduğu** çerçeve, kapsayıcının koordinatında: sağ üst
    /// köşe, iç payla. Animasyon sürerken görünümün kendi çerçevesi yolda;
    /// örtülen hücreler varılacak yerden sorulmalı.
    pub(crate) fn resting_frame(&self) -> NSRect {
        let bounds = self.parent.bounds();
        let size = self.surface.frame().size;
        let origin = NSPoint::new(
            (bounds.size.width - INSET - size.width).max(0.0),
            bounds.size.height - INSET - size.height,
        );
        NSRect::new(origin, size)
    }

    /// Paneli açar: sağ üste yerleştirir ve (Hareketi Azalt değilse) kısa
    /// bir belirme + inişle gösterir. Zaten açıksa no-op.
    pub(crate) fn show(&self, animate: bool) {
        if self.shown.replace(true) {
            return;
        }
        self.generation.set(self.generation.get().wrapping_add(1));
        let place = self.resting_frame().origin;
        let surface = self.surface.clone();
        surface.setHidden(false);
        if !animate {
            surface.setAlphaValue(1.0);
            surface.setFrameOrigin(place);
            return;
        }
        surface.setAlphaValue(0.0);
        surface.setFrameOrigin(NSPoint::new(place.x, place.y + SLIDE));
        let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            // SAFETY: AppKit bloğa canlı bir bağlam veriyor, blok süresince.
            let context = unsafe { context.as_ref() };
            context.setDuration(APPEAR_SECS);
            // SAFETY: QuartzCore'un dışa açtığı sabit ad, süreç boyunca yaşıyor.
            let curve = unsafe { kCAMediaTimingFunctionEaseOut };
            context.setTimingFunction(Some(&CAMediaTimingFunction::functionWithName(curve)));
            let animator = surface.animator();
            animator.setAlphaValue(1.0);
            animator.setFrameOrigin(place);
        });
        NSAnimationContext::runAnimationGroup(&changes);
    }

    /// Paneli kapatır: kısa bir sönme + yükselmeyle (Hareketi Azalt'ta
    /// anında) gizler. Kapalıysa no-op.
    pub(crate) fn hide(&self, animate: bool) {
        if !self.shown.replace(false) {
            return;
        }
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let surface = self.surface.clone();
        if !animate {
            surface.setHidden(true);
            return;
        }
        let origin = surface.frame().origin;
        let moving = surface.clone();
        let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            // SAFETY: AppKit bloğa canlı bir bağlam veriyor, blok süresince.
            let context = unsafe { context.as_ref() };
            context.setDuration(APPEAR_SECS);
            // SAFETY: QuartzCore'un dışa açtığı sabit ad, süreç boyunca yaşıyor.
            let curve = unsafe { kCAMediaTimingFunctionEaseIn };
            context.setTimingFunction(Some(&CAMediaTimingFunction::functionWithName(curve)));
            let animator = moving.animator();
            animator.setAlphaValue(0.0);
            animator.setFrameOrigin(NSPoint::new(origin.x, origin.y + SLIDE));
        });
        let current = Rc::clone(&self.generation);
        let done = RcBlock::new(move || {
            // Araya bir açılış girdiyse panel onundur.
            if current.get() == generation {
                surface.setHidden(true);
                surface.setFrameOrigin(origin);
            }
        });
        NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&done));
    }

    /// Alanın ve iki anahtarın hâli — oturuma gidecek sorgu.
    pub(crate) fn query(&self) -> SearchQuery {
        SearchQuery {
            text: self.field.stringValue().to_string(),
            regex: self.regex.state() == NSControlStateValueOn,
            case_sensitive: self.case.state() == NSControlStateValueOn,
        }
    }

    /// Alanın metnini yazar (⌘E, find panosu).
    pub(crate) fn set_text(&self, text: &str) {
        self.field.setStringValue(&NSString::from_str(text));
    }

    /// Regex anahtarı açık mı — ⌘E'nin kaçırma kararı.
    pub(crate) fn regex(&self) -> bool {
        self.regex.state() == NSControlStateValueOn
    }

    /// `query` oturuma en son verilen sorgudan farklıysa onu kaydeder ve
    /// `true` döner — çağıran ancak o zaman `set_search` çağırır.
    pub(crate) fn take_change(&self, query: &SearchQuery) -> bool {
        let mut applied = self.applied.borrow_mut();
        if applied.as_ref() == Some(query) {
            return false;
        }
        *applied = Some(query.clone());
        true
    }

    /// Oturuma verilen sorguyu unutur: panel kapanınca arama da kapanıyor ve
    /// yeniden açılış aynı sorguyu bir daha uygulamalı.
    pub(crate) fn forget_applied(&self) {
        self.applied.borrow_mut().take();
    }

    /// Sayım etiketini yazar (Karar 3).
    pub(crate) fn set_count(&self, status: SearchStatus, report: SearchReport) {
        self.count
            .setStringValue(&NSString::from_str(&count_label(status, report)));
    }

    /// Yüzeyi temaya boyar: zemin temanın zemininden bir adım öne çıkmış,
    /// kenar ön planın ince bir izi. Alanın kendisi sistem kontrolü ve
    /// görünümü pencereden (`apply_chrome`'un Aqua/DarkAqua'sı).
    pub(crate) fn paint(&self, theme: &Theme, dark: bool) {
        let (fill, border) = surface_colors(theme.background, theme.foreground, dark);
        self.surface.setFillColor(&srgb(fill, 1.0));
        self.surface.setBorderColor(&srgb(border.0, border.1));
    }
}

/// Etiketin metni (Karar 3). Boş sorguda boş; geçersiz desende "Invalid
/// pattern"; eşleşme yoksa "No matches"; varsa bütün defterin sayımı —
/// "3 of 17", geçerli eşleşmenin sırası henüz bilinmiyorsa "17 matches".
/// Sayım sürerken (dizin parça parça ilerliyor ya da defter değişti) sonda
/// "…": sayı o ana kadar sayılanlar.
pub(crate) fn count_label(status: SearchStatus, report: SearchReport) -> String {
    let more = if report.complete { "" } else { "…" };
    match status {
        SearchStatus::Empty => String::new(),
        SearchStatus::Invalid => "Invalid pattern".to_owned(),
        SearchStatus::Ready if !report.found => "No matches".to_owned(),
        SearchStatus::Ready => match (report.ordinal, report.total) {
            (_, 0) if !report.complete => "…".to_owned(),
            (Some(ordinal), total) if ordinal <= total => {
                format!("{ordinal} of {total}{more}")
            }
            (_, 1) => format!("1 match{more}"),
            (_, total) => format!("{total} matches{more}"),
        },
    }
}

/// ⌘E'nin sorgusu (Karar 6): seçimin **ilk satırı** — arama sert satır
/// sonunu aşmıyor, yani sonraki satırlar hiçbir şeyle eşleşemezdi — ve regex
/// kipindeyse kaçırılmış hâli, ki seçilen metin kendisini düz eşleştirsin.
/// Boş seçimde `None`.
pub(crate) fn selection_query(selection: &str, regex: bool) -> Option<String> {
    let line = selection.lines().next().unwrap_or_default();
    if line.is_empty() {
        return None;
    }
    Some(if regex {
        escape_search(line)
    } else {
        line.to_owned()
    })
}

/// Yüzeyin iki rengi, sRGB: zemin ve (kenar, alfa).
///
/// Zemin temanın zemininin ön plana doğru küçük bir karışımı — koyu temada
/// bir adım açık, açık temada bir adım koyu; ayrı bir tema rolü değil
/// (`Theme`'in "çizilmeyen rol eklenmiyor" kuralı). Oranlar tasarım sabiti,
/// gerçek pencerede iki gömülü temada gözle indi.
fn surface_colors(background: u32, foreground: u32, dark: bool) -> (u32, (u32, f64)) {
    let (lift, edge) = if dark { (0.11, 0.16) } else { (0.045, 0.14) };
    (mix(background, foreground, lift), (foreground, edge))
}

/// `a`'dan `b`'ye `t` kadar, sRGB baytlarında.
fn mix(a: u32, b: u32, t: f64) -> u32 {
    let channel = |shift: u32| {
        let (x, y) = (
            f64::from((a >> shift) & 0xff),
            f64::from((b >> shift) & 0xff),
        );
        ((x + (y - x) * t).round() as u32).min(0xff) << shift
    };
    channel(16) | channel(8) | channel(0)
}

fn srgb(color: u32, alpha: f64) -> Retained<NSColor> {
    let byte = |shift: u32| f64::from((color >> shift) & 0xff) / 255.0;
    NSColor::colorWithSRGBRed_green_blue_alpha(byte(16), byte(8), byte(0), alpha)
}

fn width(view: &NSView, points: f64) {
    view.widthAnchor()
        .constraintEqualToConstant(points)
        .setActive(true);
}

/// Açık/kapalı iki konumlu küçük bir anahtar (`Aa`, `.*`): kapalıyken yalnız
/// metin, açıkken ve üstüne gelince çerçeveli — Safari'nin ve Xcode'un bul
/// çubuğundaki seçenek düğmeleri gibi.
fn toggle(mtm: MainThreadMarker, title: &str, tip: &str, target: &AnyObject) -> Retained<NSButton> {
    // SAFETY: hedef zayıf ve pencere paneli yaşattığı sürece yaşıyor; seçici
    // pencerede tek `Option<&AnyObject>` argümanlı bir eylem.
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(sel!(searchOptionsChanged:)),
            mtm,
        )
    };
    button.setButtonType(NSButtonType::PushOnPushOff);
    width(&button, BUTTON_WIDTH);
    button.setBezelStyle(NSBezelStyle::AccessoryBar);
    button.setShowsBorderOnlyWhileMouseInside(true);
    button.setControlSize(NSControlSize::Small);
    button.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(
        NSFont::smallSystemFontSize(),
        // SAFETY: AppKit'in dışa açtığı sabit, süreç boyunca yaşıyor.
        unsafe { NSFontWeightRegular },
    )));
    button.setState(NSControlStateValueOff);
    button.setToolTip(Some(&NSString::from_str(tip)));
    button
}

/// SF Symbol'lü çerçevesiz düğme (oklar, kapatma): üstüne gelince çerçeve.
fn symbol(
    mtm: MainThreadMarker,
    name: &str,
    tip: &str,
    target: &AnyObject,
    action: Sel,
) -> Retained<NSButton> {
    let tip = NSString::from_str(tip);
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(name),
        Some(&tip),
    )
    .unwrap_or_default();
    // SAFETY: hedef zayıf ve pencere paneli yaşattığı sürece yaşıyor; seçici
    // pencerede tek `Option<&AnyObject>` argümanlı bir eylem.
    let button =
        unsafe { NSButton::buttonWithImage_target_action(&image, Some(target), Some(action), mtm) };
    button.setBezelStyle(NSBezelStyle::AccessoryBar);
    button.setShowsBorderOnlyWhileMouseInside(true);
    button.setControlSize(NSControlSize::Small);
    button.setToolTip(Some(&tip));
    width(&button, BUTTON_WIDTH);
    button
}

#[cfg(test)]
mod tests {
    use super::{count_label, mix, selection_query, surface_colors};
    use bt_core::{SearchReport, SearchStatus};

    #[test]
    fn the_label_says_what_the_query_found() {
        let report = |found, total, ordinal, complete| SearchReport {
            found,
            total,
            ordinal,
            complete,
        };
        let ready = |r| count_label(SearchStatus::Ready, r);
        assert_eq!(
            count_label(SearchStatus::Empty, report(false, 0, None, true)),
            ""
        );
        assert_eq!(
            count_label(SearchStatus::Invalid, report(false, 0, None, true)),
            "Invalid pattern"
        );
        assert_eq!(ready(report(false, 0, None, true)), "No matches");
        assert_eq!(ready(report(true, 17, Some(3), true)), "3 of 17");
        assert_eq!(ready(report(true, 17, Some(3), false)), "3 of 17…");
        assert_eq!(ready(report(true, 17, None, true)), "17 matches");
        assert_eq!(ready(report(true, 1, None, true)), "1 match");
        assert_eq!(ready(report(true, 40, None, false)), "40 matches…");
        assert_eq!(ready(report(true, 0, None, false)), "…");
    }

    #[test]
    fn use_selection_escapes_in_regex_mode_and_keeps_the_first_line() {
        assert_eq!(selection_query("a.b(c)", false).as_deref(), Some("a.b(c)"));
        assert_eq!(
            selection_query("a.b(c)", true).as_deref(),
            Some("a\\.b\\(c\\)"),
            "regex kipinde seçilen metin kendisini düz eşleştirmeli"
        );
        assert_eq!(
            selection_query("first\nsecond", false).as_deref(),
            Some("first")
        );
        assert_eq!(selection_query("", true), None);
        assert_eq!(selection_query("\nx", false), None);
    }

    #[test]
    fn the_surface_steps_toward_the_foreground() {
        assert_eq!(mix(0x000000, 0xffffff, 0.0), 0x000000);
        assert_eq!(mix(0x000000, 0xffffff, 1.0), 0xffffff);
        let (dark, _) = surface_colors(0x000000, 0xe6e6e6, true);
        assert!(dark > 0x000000 && dark < 0x303030, "{dark:06x}");
        let (light, _) = surface_colors(0xffffff, 0x1a1a1a, false);
        assert!(light < 0xffffff && light > 0xe0e0e0, "{light:06x}");
    }
}
