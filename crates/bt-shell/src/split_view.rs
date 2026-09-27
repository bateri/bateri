//! Bölmelerin kapsayıcısı: pencerenin `contentView`'ı olan düz bir `NSView`
//! (039 Karar 6). Sekmenin pane'lerini ve bölme ağacını ([`crate::split`])
//! tutuyor, ağacın çerçevelerini pane'lere uyguluyor ve ayırıcıları
//! gösteriyor. Kare yoluna girmiyor: çizdiği bir şey yok.
//!
//! **Ağaç burada, pencerede değil**: kapsayıcının kendi boyu pencereden
//! bağımsız değişiyor (sekme çubuğu içeriği kısaltıyor; 026 phase-4) ve o
//! bildirimi alan AppKit'in bu view'a çağırdığı `resizeSubviewsWithOldSize:`.
//! Ağaç pencerede dursaydı view her boy değişiminde pencereye geri uzanmak
//! zorunda kalırdı.
//!
//! **Ayırıcı bir boşluk**: pane'ler opak ve çerçeveleri arasında bir aygıt
//! pikseli açık kalıyor; oradan görünen şey pane'lerin arkasında duran ve
//! kapsayıcıyı dolduran tek bir `NSBox`'ın temanın `separator` tonundaki
//! dolgusu (039 Karar 7, R3.5). `drawRect:` yok, `CGColor` isteyen katman
//! yolu da yok (sekme noktasının emsali). Tek pane'de kutu gizli ve pane
//! kapsayıcıyı **oturtulmadan** dolduruyor — bölmeden önceki düzenin aynısı.
//!
//! Pane'in çerçevesi değişince geometrisini pane kendisi tazeliyor
//! (`TerminalPane::observe_frame`); burada yalnız `setFrame` var.

use std::cell::RefCell;

use bt_core::Theme;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{NSBox, NSBoxType, NSColor, NSTitlePosition, NSView};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize};

use crate::pane::TerminalPane;
use crate::split::{self, Axis, Rect, Removal, Tree};

pub(crate) struct SplitIvars {
    /// Bölme ağacı; yaprakları [`SplitIvars::panes`]'in kimlikleri.
    tree: RefCell<Tree>,
    /// Sekmenin pane'leri. Kapsayıcı onları alt view olarak da tutuyor; bu
    /// liste tipli erişim için. **Hiç boşalmıyor**: son pane'i kaldırmak
    /// pencereyi kapatmak demek ([`Removal::Last`]).
    panes: RefCell<Vec<Retained<TerminalPane>>>,
    /// Ayırıcıların rengi: pane'lerin arkasındaki dolgu.
    backdrop: Retained<NSBox>,
}

define_class!(
    // SAFETY: NSView alt sınıflama için tasarlanmıştır; SplitView `Drop`
    // uygulamaz ve `initWithFrame:` dışında bir kurucu sunmaz.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriSplitView"]
    #[ivars = SplitIvars]
    pub(crate) struct SplitView;

    unsafe impl NSObjectProtocol for SplitView {}

    impl SplitView {
        /// Üstten aşağı koordinat: ağacın "ikinci yaprak altta"sı işaret
        /// çevirmeden. Yalnız pane'lerin **çerçevelerini** etkiliyor; her
        /// pane'in kendi içi kendi koordinatında.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// Kapsayıcının boyu değişti (pencere, sekme çubuğu): pane'ler
        /// oranlarını koruyarak yeniden oturuyor (039 Karar 14).
        #[unsafe(method(resizeSubviewsWithOldSize:))]
        fn resize_subviews(&self, _old: NSSize) {
            self.layout_panes();
        }
    }
);

impl SplitView {
    /// Tek pane'li kapsayıcı; pane onu dolduruyor.
    pub(crate) fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        first: &TerminalPane,
    ) -> Retained<Self> {
        let backdrop = NSBox::new(mtm);
        backdrop.setBoxType(NSBoxType::Custom);
        backdrop.setTitlePosition(NSTitlePosition::NoTitle);
        backdrop.setBorderWidth(0.0);
        backdrop.setHidden(true);
        let this = Self::alloc(mtm).set_ivars(SplitIvars {
            tree: RefCell::new(Tree::Leaf(first.id())),
            panes: RefCell::new(vec![first.retain()]),
            backdrop: backdrop.clone(),
        });
        // SAFETY: `initWithFrame:` NSView'un tasarlanmış kurucusu ve ivar'lar
        // set edildi.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        // Önceki `contentView` (pane) layer-backed'di; Metal katmanının
        // bileşim biçimi değişmesin.
        this.setWantsLayer(true);
        this.addSubview(&backdrop);
        this.addSubview(first);
        this.layout_panes();
        this
    }

    /// Pane'ler, ağaç sırasıyla (soldan sağa, yukarıdan aşağı).
    pub(crate) fn panes(&self) -> Vec<Retained<TerminalPane>> {
        let panes = self.ivars().panes.borrow();
        self.ivars()
            .tree
            .borrow()
            .leaves()
            .into_iter()
            .filter_map(|id| panes.iter().find(|pane| pane.id() == id).cloned())
            .collect()
    }

    /// Kimliği `id` olan pane.
    pub(crate) fn pane(&self, id: u64) -> Option<Retained<TerminalPane>> {
        self.ivars()
            .panes
            .borrow()
            .iter()
            .find(|pane| pane.id() == id)
            .cloned()
    }

    /// Pencerenin ölçeği: sınırlar aygıt pikseline oturuyor. Kapsayıcı henüz
    /// bir pencereye takılı değilse (kurucunun ilk yerleşimi) `1` — pencere
    /// takılınca AppKit boyu yeniden bildiriyor ve yerleşim tekrarlanıyor,
    /// ölçek değişiminde de pencere yeniden yerleştiriyor
    /// (`TerminalWindow`'un `windowDidChangeBackingProperties:`'i).
    fn scale(&self) -> f64 {
        self.window()
            .map_or(1.0, |window| window.backingScaleFactor())
    }

    fn bounds_rect(&self) -> Rect {
        let size = self.bounds().size;
        Rect::new(0.0, 0.0, size.width, size.height)
    }

    /// `id`'nin çerçevesi, bölünse iki yarısıyla (nokta cinsinden, çerçeve
    /// hesabının aynı aritmetiği — [`split::split_halves`]). Pane yoksa
    /// `None`.
    pub(crate) fn halves(&self, id: u64, axis: Axis) -> Option<(NSSize, NSSize)> {
        let scale = self.scale();
        let layout = self.ivars().tree.borrow().layout(self.bounds_rect(), scale);
        let frame = layout.panes.iter().find(|(pane, _)| *pane == id)?.1;
        let (first, second) = split::split_halves(frame, axis, scale);
        Some((
            NSSize::new(first.width, first.height),
            NSSize::new(second.width, second.height),
        ))
    }

    /// `target`'ı `axis`'te böler ve `pane`'i ikinci yarıya koyar (sağa ya
    /// da alta). Hedef ağaçta yoksa `false` ve hiçbir şey değişmez.
    pub(crate) fn insert(&self, target: u64, axis: Axis, pane: &TerminalPane) -> bool {
        if !self
            .ivars()
            .tree
            .borrow_mut()
            .split(target, axis, pane.id())
        {
            return false;
        }
        self.ivars().panes.borrow_mut().push(pane.retain());
        self.addSubview(pane);
        self.layout_panes();
        true
    }

    /// `id`'yi ağaçtan kaldırır ([`Tree::remove`]); pane view'da ve listede
    /// kalıyor — çağıran önce odağı taşıyor, sonra [`SplitView::detach`]
    /// ediyor (first responder'ı taşıyan view'ı söken pencere responder'sız
    /// kalırdı).
    pub(crate) fn remove_leaf(&self, id: u64) -> Removal {
        self.ivars().tree.borrow_mut().remove(id)
    }

    /// Ağaçtan çıkmış pane'i view'dan ve listeden söker, kalanları yeniden
    /// oturtur. Pane'in son güçlü referansı çağıranda düşüyor.
    pub(crate) fn detach(&self, id: u64) -> Option<Retained<TerminalPane>> {
        let removed = {
            let mut panes = self.ivars().panes.borrow_mut();
            let index = panes.iter().position(|pane| pane.id() == id)?;
            panes.remove(index)
        };
        removed.removeFromSuperview();
        self.layout_panes();
        Some(removed)
    }

    /// Ayırıcının rengi temadan (039 Karar 7): `Theme::separator_srgb` —
    /// dock'un saç çizgileriyle aynı kademe. `NSColor` sRGB alıyor, lineer
    /// değer GPU'nun (`CLAUDE.md` → Renk uzayı).
    pub(crate) fn set_theme(&self, theme: &Theme) {
        let [r, g, b] = theme.separator_srgb().map(|byte| f64::from(byte) / 255.0);
        self.ivars()
            .backdrop
            .setFillColor(&NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0));
    }

    /// Ağacın çerçevelerini pane'lere uygular. Tek pane'de oturtma yok: pane
    /// kapsayıcının sınırının ta kendisi, bölmeden önceki gibi.
    pub(crate) fn layout_panes(&self) {
        let panes = self.ivars().panes.borrow().clone();
        // `resizeSubviewsWithOldSize:`'ı biz karşılıyoruz, yani AppKit'in
        // autoresizing'i bu view'ın çocuklarına uygulanmıyor: dolgu da elle.
        let backdrop = &self.ivars().backdrop;
        backdrop.setFrame(self.bounds());
        backdrop.setHidden(panes.len() <= 1);
        if let [only] = panes.as_slice() {
            only.setFrame(self.bounds());
            return;
        }
        let layout = self
            .ivars()
            .tree
            .borrow()
            .layout(self.bounds_rect(), self.scale());
        for (id, rect) in layout.panes {
            if let Some(pane) = panes.iter().find(|pane| pane.id() == id) {
                pane.setFrame(NSRect::new(
                    NSPoint::new(rect.x, rect.y),
                    NSSize::new(rect.width, rect.height),
                ));
            }
        }
    }
}
