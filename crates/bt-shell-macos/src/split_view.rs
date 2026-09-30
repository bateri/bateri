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
//! (`TerminalPane::observe_frame`); burada yalnız `setFrame` var — ayırıcı
//! sürüklenirken de, yani PTY sürükleme boyunca pencere boyutlandırmasının
//! yolundan boyutlanıyor.
//!
//! **Sürükleme tutamakları** (039 phase-4): çizilen çizgi bir piksel, isabet
//! alanı ise her yana [`HANDLE_PT`] geniş ve pane'lerin **üstünde** duran
//! saydam bir view ([`DividerHandle`]) — pane'ler opak ve çizginin dışındaki
//! her noktayı kaplıyor, yani alan arkadaki dolguda olamazdı. İmleci
//! `resizeLeftRight`/`resizeUpDown`. Tutamaklar yalnız ayırıcı **sayısı**
//! değişince yeniden kuruluyor (yeni pane onların üstüne eklendiği için o an
//! en üste geri alınmaları gerekiyor); sürükleme boyunca aynı view kalıyor,
//! çünkü AppKit `mouseDragged:`'ı basışı alan view'a veriyor.
//!
//! **Büyütme** (⇧⌘↩): büyütülmüş pane bütün alanı alıyor
//! ([`Tree::layout_zoomed`]), öteki pane'ler **gizli** ve çerçeveleri (yani
//! ızgaraları) olduğu gibi kalıyor; ayırıcı ve tutamak yok. Gizli pane'in
//! link'i örtülmüş pencereninki gibi uyuyor ([`SplitView::apply_visibility`]).

use std::cell::{Cell, RefCell};

use bt_core::Theme;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{NSBox, NSBoxType, NSColor, NSCursor, NSEvent, NSTitlePosition, NSView};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize};

use crate::pane::TerminalPane;
use crate::split::{self, Axis, Direction, Divider, Rect, Removal, Size, Tree};

/// Ayırıcının isabet alanının çizginin her yanına taşan payı, nokta. Ölçülmüş
/// değil, bir tasarım sabiti: bir piksellik çizgi fareyle tutulamıyor, altı
/// noktalık bant tutuluyor ve pane'in kenarındaki metinden çok az şey
/// yiyor (bandın içindeki tık pane'e değil ayırıcıya gidiyor).
const HANDLE_PT: f64 = 3.0;

pub(crate) struct HandleIvars {
    /// [`split::Layout::dividers`]'taki sırası — [`Tree::drag`]'in indeksi.
    index: Cell<usize>,
    /// Ayırdığı bölmenin ekseni: yan yana bölmenin tutamağı yatay
    /// sürükleniyor.
    axis: Cell<Axis>,
    /// Çizginin eksendeki konumu, kapsayıcının koordinatında (nokta).
    line: Cell<f64>,
    /// Basışta işaretçi ile çizgi arasındaki fark: çizgi işaretçinin altında
    /// sıçramasın, tutulduğu yerden kaysın.
    grab: Cell<f64>,
}

define_class!(
    // SAFETY: NSView alt sınıflama için tasarlanmıştır; DividerHandle `Drop`
    // uygulamaz ve `initWithFrame:` dışında bir kurucu sunmaz.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriDividerHandle"]
    #[ivars = HandleIvars]
    pub(crate) struct DividerHandle;

    unsafe impl NSObjectProtocol for DividerHandle {}

    impl DividerHandle {
        /// `resizeLeftRightCursor`/`resizeUpDownCursor` kullanımdan kalkmış
        /// ama yerini alan `columnResizeCursorInDirections:` macOS 15'te
        /// geliyor; taban macOS 14 (`CLAUDE.md` → Taban).
        #[unsafe(method(resetCursorRects))]
        #[allow(deprecated)]
        fn reset_cursor_rects(&self) {
            let cursor = match self.ivars().axis.get() {
                Axis::Horizontal => NSCursor::resizeLeftRightCursor(),
                Axis::Vertical => NSCursor::resizeUpDownCursor(),
            };
            self.addCursorRect_cursor(self.bounds(), &cursor);
        }

        /// Basış yutuluyor (responder zincirine, pencereye çıkmasın) ve
        /// tutulan yer kaydediliyor.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(along) = self.along(event) {
                self.ivars().grab.set(along - self.ivars().line.get());
            }
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let Some(along) = self.along(event) else {
                return;
            };
            if let Some(container) = self.container() {
                container.drag_divider(self.ivars().index.get(), along - self.ivars().grab.get());
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {}
    }
);

impl DividerHandle {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(HandleIvars {
            index: Cell::new(0),
            axis: Cell::new(Axis::Horizontal),
            line: Cell::new(0.0),
            grab: Cell::new(0.0),
        });
        // SAFETY: `initWithFrame:` NSView'un tasarlanmış kurucusu ve ivar'lar
        // set edildi.
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    fn container(&self) -> Option<Retained<SplitView>> {
        // SAFETY: üst view'ı okumak; ana thread'deyiz (`MainThreadOnly`).
        unsafe { self.superview() }?.downcast::<SplitView>().ok()
    }

    /// Olayın konumu kapsayıcının (üstten aşağı) koordinatında, tutamağın
    /// ekseni boyunca.
    fn along(&self, event: &NSEvent) -> Option<f64> {
        let container = self.container()?;
        let point = container.convertPoint_fromView(event.locationInWindow(), None);
        Some(match self.ivars().axis.get() {
            Axis::Horizontal => point.x,
            Axis::Vertical => point.y,
        })
    }

    /// Ayırıcıya oturur: sırası, ekseni, çizgisi ve çizgiden [`HANDLE_PT`]
    /// taşan çerçevesi (kapsayıcının sınırına kırpılmış).
    fn place(&self, index: usize, divider: Divider, bounds: NSSize) {
        let iv = self.ivars();
        iv.index.set(index);
        iv.axis.set(divider.axis);
        let rect = divider.rect;
        let frame = match divider.axis {
            Axis::Horizontal => {
                iv.line.set(rect.x);
                let x = (rect.x - HANDLE_PT).max(0.0);
                let right = (rect.x + rect.width + HANDLE_PT).min(bounds.width);
                NSRect::new(NSPoint::new(x, rect.y), NSSize::new(right - x, rect.height))
            }
            Axis::Vertical => {
                iv.line.set(rect.y);
                let y = (rect.y - HANDLE_PT).max(0.0);
                let bottom = (rect.y + rect.height + HANDLE_PT).min(bounds.height);
                NSRect::new(NSPoint::new(rect.x, y), NSSize::new(rect.width, bottom - y))
            }
        };
        self.setFrame(frame);
        if let Some(window) = self.window() {
            window.invalidateCursorRectsForView(self);
        }
    }
}

pub(crate) struct SplitIvars {
    /// Bölme ağacı; yaprakları [`SplitIvars::panes`]'in kimlikleri.
    tree: RefCell<Tree>,
    /// Sekmenin pane'leri. Kapsayıcı onları alt view olarak da tutuyor; bu
    /// liste tipli erişim için. **Hiç boşalmıyor**: son pane'i kaldırmak
    /// pencereyi kapatmak demek ([`Removal::Last`]).
    panes: RefCell<Vec<Retained<TerminalPane>>>,
    /// Ayırıcıların rengi: pane'lerin arkasındaki dolgu.
    backdrop: Retained<NSBox>,
    /// Büyütülmüş pane (⇧⌘↩); `None` → bölmeler görünüyor.
    zoomed: Cell<Option<u64>>,
    /// Ayırıcıların sürükleme tutamakları, [`split::Layout::dividers`]'ın
    /// sırasıyla.
    handles: RefCell<Vec<Retained<DividerHandle>>>,
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
            zoomed: Cell::new(None),
            handles: RefCell::new(Vec::new()),
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

    /// Büyütülmüş pane; `None` → bölmeler görünüyor.
    pub(crate) fn zoomed(&self) -> Option<u64> {
        self.ivars().zoomed.get()
    }

    /// Büyütmeyi kurar ya da bırakır ve pane'leri yeniden oturtur. Link'lerin
    /// görünürlüğü çağıranın ([`SplitView::apply_visibility`]): pencerenin
    /// örtülme durumunu o biliyor.
    pub(crate) fn set_zoomed(&self, zoomed: Option<u64>) {
        self.ivars().zoomed.set(zoomed);
        self.layout_panes();
    }

    /// Link'lerin görünürlüğü: pencere görünür **ve** pane gizli değil.
    /// Gizli pane (büyütmenin arkasında kalan) örtülmüş pencere gibi sıfır
    /// kare çiziyor; geri gelince bir kare istiyor (`DisplayLink::set_visible`).
    pub(crate) fn apply_visibility(&self, window_visible: bool) {
        for pane in self.ivars().panes.borrow().iter() {
            if let Some(link) = pane.link() {
                link.set_visible(window_visible && !pane.isHidden());
            }
        }
    }

    /// Ağacın sıradan (büyütmesiz) düzeni: gezinme onu soruyor.
    fn plain_layout(&self) -> split::Layout {
        self.ivars()
            .tree
            .borrow()
            .layout(self.bounds_rect(), self.scale())
    }

    /// `from`'un yöndeki komşusu (⌥⌘ + ok; [`split::Layout::neighbour`]).
    pub(crate) fn neighbour(&self, from: u64, direction: Direction) -> Option<u64> {
        self.plain_layout().neighbour(from, direction)
    }

    /// Sıradaki ya da önceki pane (⌘] / ⌘[; [`Tree::cycle`]).
    pub(crate) fn cycle(&self, from: u64, forward: bool) -> Option<u64> {
        self.ivars().tree.borrow().cycle(from, forward)
    }

    /// En küçük pane sınırı, yaprak başına (039 Karar 14): pane'in kendi
    /// hücresinden (`TerminalPane::min_size`). Ölçemeyen pane sınırsız.
    fn limits(&self) -> impl Fn(u64) -> Size + use<> {
        let panes = self.ivars().panes.borrow().clone();
        move |id| {
            panes
                .iter()
                .find(|pane| pane.id() == id)
                .and_then(|pane| pane.min_size())
                .map_or(Size::new(0.0, 0.0), |min| Size::new(min.width, min.height))
        }
    }

    /// ⌃⌘ + ok: `target`'ın o eksendeki en yakın ayırıcısını `step` nokta
    /// taşır ([`Tree::resize`]), sınırda kırparak. Oynadıysa `true`.
    pub(crate) fn resize(&self, target: u64, direction: Direction, step: f64) -> bool {
        let limits = self.limits();
        let moved = self.ivars().tree.borrow_mut().resize(
            target,
            direction,
            step,
            self.bounds_rect(),
            self.scale(),
            &limits,
        );
        if moved {
            self.layout_panes();
        }
        moved
    }

    /// ⌃⌘=: aynı eksendeki pane'ler eşit ([`Tree::equalize`]).
    pub(crate) fn equalize(&self) {
        self.ivars().tree.borrow_mut().equalize();
        self.layout_panes();
    }

    /// Tutamağın sürüklemesi: `index`'inci ayırıcı `position`'a
    /// ([`Tree::drag`]).
    fn drag_divider(&self, index: usize, position: f64) {
        let limits = self.limits();
        let moved = self.ivars().tree.borrow_mut().drag(
            index,
            position,
            self.bounds_rect(),
            self.scale(),
            &limits,
        );
        if moved {
            self.layout_panes();
        }
    }

    /// Tutamakları ayırıcılara oturtur; sayı değiştiyse hepsini yeniden
    /// kurup en üste ekler (modül başlığı).
    fn sync_handles(&self, dividers: &[Divider]) {
        let mut handles = self.ivars().handles.borrow_mut();
        if handles.len() != dividers.len() {
            for handle in handles.drain(..) {
                handle.removeFromSuperview();
            }
            for _ in dividers {
                let handle = DividerHandle::new(self.mtm());
                self.addSubview(&handle);
                handles.push(handle);
            }
        }
        let size = self.bounds().size;
        for (index, (handle, divider)) in handles.iter().zip(dividers).enumerate() {
            handle.place(index, *divider, size);
        }
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
    /// kapsayıcının sınırının ta kendisi, bölmeden önceki gibi. Büyütülmüşken
    /// yalnız büyütülen pane görünüyor, ötekiler gizli ve çerçeveleri yerinde.
    pub(crate) fn layout_panes(&self) {
        let panes = self.ivars().panes.borrow().clone();
        let zoomed = self.ivars().zoomed.get();
        // `resizeSubviewsWithOldSize:`'ı biz karşılıyoruz, yani AppKit'in
        // autoresizing'i bu view'ın çocuklarına uygulanmıyor: dolgu da elle.
        let backdrop = &self.ivars().backdrop;
        backdrop.setFrame(self.bounds());
        backdrop.setHidden(panes.len() <= 1 || zoomed.is_some());
        if let [only] = panes.as_slice() {
            only.setHidden(false);
            only.setFrame(self.bounds());
            self.sync_handles(&[]);
            return;
        }
        let layout =
            self.ivars()
                .tree
                .borrow()
                .layout_zoomed(self.bounds_rect(), self.scale(), zoomed);
        for pane in &panes {
            match layout.panes.iter().find(|(id, _)| *id == pane.id()) {
                Some((_, rect)) => {
                    pane.setHidden(false);
                    pane.setFrame(NSRect::new(
                        NSPoint::new(rect.x, rect.y),
                        NSSize::new(rect.width, rect.height),
                    ));
                }
                None => pane.setHidden(true),
            }
        }
        self.sync_handles(&layout.dividers);
    }
}
