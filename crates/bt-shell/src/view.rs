//! Pencerenin içeriği: `CAMetalLayer`'ı taşıyan ve klavyeyi PTY'ye akıtan view.
//!
//! Çizim burada **yok** — layer'ın içeriğini `bt-gpu` doldurur. Bu sınıfın tek
//! işi first responder olmak ve tuş vuruşunu [`crate::keys::encode_key`]'e
//! verip çıkan baytları oturuma yazmak.

use std::cell::OnceCell;
use std::sync::Arc;

use bt_core::Session;
use bt_gpu::CellMetrics;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSPasteboard, NSView};
use objc2_foundation::{NSObjectProtocol, NSRect};

use crate::clipboard;
use crate::keys::encode_key;

/// Fare noktası → grid hücresi. **Saf ve AppKit'siz**, bu yüzden sınanabilir.
///
/// `view_px` view koordinatında (nokta), `cell_px` fiziksel piksel, `scale`
/// backing ölçeği: ölçü `bt-gpu`'dan fiziksel geldiği için fare de önce
/// fiziksel piksele çıkar, sonra bölünür.
///
/// Kenar davranışı asimetrik ve bilerek: sol/üst dışarısı negatif ara değerden
/// **0'a kırpılır** (sürükleme grid'in o yanına yapışır), sağ/alt dışarısı
/// `None` ile **yutulur** (olmayan hücreyi seçmek `Some("")` üretip phase-2'nin
/// kopyasını boşaltırdı). Kırpma `to_range`'ın kırpmasıyla aynı yönde, yani
/// iki katman aynı kenarda aynı kararı veriyor.
///
/// Taban yuvarlama (`as u16` kesmesi): farenin hücrenin neresinde olduğu değil
/// **hangi** hücrede olduğu soruluyor ve `split_into_grid` ile aynı aritmetik.
/// Negatif ara değer `as u16`'da doygun (0'a iner) — sarmaz, çünkü kaynak
/// `f64` ve `f64 as u16` negatifi 0'a doyurur.
pub(crate) fn point_to_cell(
    view_px: (f64, f64),
    cell_px: (u16, u16),
    scale: f64,
    cols: u16,
    rows: u16,
) -> Option<(u16, u16)> {
    let (cell_w, cell_h) = (f64::from(cell_px.0), f64::from(cell_px.1));
    // View `isFlipped`, yani y grid yönünde (üstten) geliyor: tersine çevirme
    // yok. `rows` yalnız dışarılık kapısında kullanılıyor — yüksekliği view
    // değil grid söylüyor ki pencere kenar boşluğundaki tıklama yutulsun.
    let col = (view_px.0 * scale / cell_w) as u16;
    let row = (view_px.1 * scale / cell_h) as u16;
    // Sağ/alt: `cols`/`rows` `u16`'nın tamamını tutabilir, o yüzden `col < cols`
    // kapısı taşan bir değeri kaçırmaz — kapıdan geçen her değer grid'dedir.
    (col < cols && row < rows).then_some((col, row))
}

pub(crate) struct ViewIvars {
    /// View, oturumdan **önce** doğmak zorunda: grid ölçüsü contentView'ın
    /// bounds'undan türüyor ve `Session::spawn` o ölçüyü istiyor. Bir tuş
    /// vuruşu arada geçemez ama sebebi pencerenin henüz key olmaması değil
    /// (`makeKeyAndOrderFront` daha önce koşuyor): boşluk
    /// `applicationDidFinishLaunching`'in içinde, **run loop dönmeden**
    /// kapanıyor, yani araya hiçbir olay düşemiyor.
    session: OnceCell<Arc<Session>>,
    /// Sürüklemenin çapası: basışın hücresi. `bt-core` aralığı tutar ama
    /// çapayı hatırlamaz — `mouseDragged:` bunu okur.
    anchor: std::cell::Cell<Option<(u16, u16)>>,
    /// Fare çevirisinin canlı girdileri: ölçü `bt-gpu`'dan, grid `bt-core`'un
    /// bildiği sayı. `OnceCell` değil `Cell<Option<…>>`, çünkü pencere boyu
    /// değişince tazeleniyor (`set_metrics`). Ayrı bir kopya gibi görünüyor
    /// ama değil: `start_session`'a ve `DisplayLink::resize`'a giden değerlerin
    /// aynısı, aynı çağrı yerinde yazılıyor.
    metrics: std::cell::Cell<Option<(CellMetrics, (u16, u16))>>,
}

define_class!(
    // SAFETY: NSView alt sınıflama için tasarlanmıştır; BateriView `Drop`
    // uygulamaz ve `initWithFrame:` dışında bir kurucu sunmaz.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriView"]
    #[ivars = ViewIvars]
    pub(crate) struct BateriView;

    unsafe impl NSObjectProtocol for BateriView {}

    impl BateriView {
        /// Tuş vuruşlarının buraya gelmesinin şartı. `NSView`'un varsayılanı
        /// `false`; `makeFirstResponder` bu olmadan sessizce reddedilir.
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        /// View'ın y ekseni üstten: fare noktası grid yönünde gelir.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            // Fare y'si grid yönünde (üstten) gelsin: çeviride tersine çevirme
            // yok, `bounds.height` kesiği yok — pencere boyu değişince
            // kayan bir sabit değil tipin sözü.
            true
        }

        /// Fare basıldı: seçimin çapası burada atılır ve sürükleme başlar.
        ///
        /// Yalnız sol tuş (button 0): sağ/orta tık bir seçim başlatmaz —
        /// bağlam tıklaması beklenmedik bir vurgu üretirdi. Tek tıkla
        /// odaklanma değişmez — view zaten first responder; tıklama bir seçim
        /// başlatır (R1). `super`'e geçilmiyor: varsayılan `NSView` davranışı
        /// seçimi bilmez ve olayı yutardı.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if event.buttonNumber() != 0 {
                return;
            }
            let Some((session, anchor)) = self.cell_under(event) else {
                return;
            };
            // İmleç çapa hücresinden sürüklenir: ters yöne ilk hareket seçimi
            // boşaltmamalı, fare ucundan büyümeli.
            session.set_selection(anchor, anchor);
        }

        /// Sürükleme: çapa fare basışının hücresi, aktif uç farenin şimdiki
        /// yeri. Çapa `Session`'dan okunmuyor — `bt-core` yalnız aralığı tutar,
        /// çapayı hatırlamaz. İki olay da aynı `set_selection`'ı çağırıyor;
        /// `mouseDown:` iki ucu da çapaya veriyor, burası aktif ucu fareye.
        /// Aynı hücrede kalan olaylar `set_selection`'ın eşitlik kapısında
        /// eleniyor — kare istenmez.
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let Some((session, anchor, cell)) = self.drag_cells(event) else {
                return;
            };
            session.set_selection(anchor, cell);
        }

        /// Tuş bırakıldı: çapa düşer. Çapa grid hücresi cinsinden saklanıyor;
        /// bırakma ile sonraki basış arasında kaydırma olursa bayat çapayla
        /// sürükleme hiç başlamıyor — `drag_cells` çapasız olayı yutuyor.
        /// Kaydırma **sürerken** (basılı) çapa kayması phase-3'ün işi:
        /// `set_selection` aralığı grid mutlağında tutuyor ve alacritty
        /// döndürmesi onu içerikle taşıyor, ama view'daki çapa viewport
        /// cinsinden kalıyor.
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            self.ivars().anchor.set(None);
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            // `characters` modifier'lar uygulanmış hâli verir (Option-basılı
            // "ø", Ctrl-C → U+0003); ham tuş kodu `charactersIgnoringModifiers`
            // olurdu ve klavye düzenini bizim yeniden uygulamamızı isterdi.
            let Some(chars) = event.characters() else {
                return;
            };
            let flags = event.modifierFlags();
            let Some(session) = self.ivars().session.get() else {
                return;
            };
            // Command basılıyken tuş bir kısayoldur, girdi değil. Yalnız
            // Cmd-C/V ele alınır (Karar 2 (a)); geri kalan **yutulmaya devam
            // eder** — ana menü henüz yok (00X), menüsüz bir uygulamada
            // `performKeyEquivalent:` hiçbir şeyi yakalamıyor ve bu dal
            // olmadan Cmd-V shell'e "v" yazardı. Yutma bu yüzden
            // `command_shortcut`'ın `None`'undan bağımsız.
            if flags.contains(NSEventModifierFlags::Command) {
                if let Some(shortcut) = Self::command_shortcut(&chars.to_string(), flags) {
                    Self::run_shortcut(session, shortcut);
                }
                return;
            }
            let ctrl = flags.contains(NSEventModifierFlags::Control);
            // `super`'e geçmiyoruz: `NSResponder::keyDown:` tanımadığı tuşta
            // beep çalar ve terminalde her ok tuşu bip sesi olurdu.
            if let Some(bytes) = encode_key(&chars.to_string(), ctrl) {
                session.write(&bytes);
            }
        }
    }
);

impl BateriView {
    pub(crate) fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ViewIvars {
            session: OnceCell::new(),
            anchor: std::cell::Cell::new(None),
            metrics: std::cell::Cell::new(None),
        });
        // SAFETY: `initWithFrame:` NSView'un tasarlanmış kurucusu ve ivar'lar
        // set edildi.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// Oturumu bağlar; bu andan sonra tuşlar PTY'ye gider.
    pub(crate) fn attach(&self, session: Arc<Session>) {
        // İkinci çağrı sessizce düşseydi tuşlar eski oturuma giderdi ve
        // pencere yazmıyor gibi görünürdü — tek satır iz bile bırakmadan.
        assert!(
            self.ivars().session.set(session).is_ok(),
            "oturum ikinci kez bağlandı"
        );
    }

    /// Fare çevirisinin girdilerini tazeler: `start_session` ve `resize`
    /// yolundan, oturuma ve link'e giden grid'in aynısıyla. Üçü aynı çağrı
    /// yerinde yazılıyor; biri değişip öteki eski kalamıyor.
    pub(crate) fn set_metrics(&self, grid: crate::app::Grid) {
        self.ivars()
            .metrics
            .set(Some((grid.cell, (grid.cols, grid.rows))));
    }

    /// Oturum + olayın altındaki hücre. Üçü (`session`, ölçü, grid) birlikte
    /// yoksa `None`: yarım bilgiyle seçim başlatılamaz.
    fn session_cell(&self, event: &NSEvent) -> Option<(Arc<Session>, (u16, u16))> {
        let session = Arc::clone(self.ivars().session.get()?);
        let cell = self.event_cell(event)?;
        Some((session, cell))
    }

    /// Olayın altındaki hücre + bağlı oturum; çapayı da kurar.
    fn cell_under(&self, event: &NSEvent) -> Option<(Arc<Session>, (u16, u16))> {
        let (session, cell) = self.session_cell(event)?;
        // Çapa burada saklanıyor: sürükleme çapa + aktif uç ister, `bt-core`
        // yalnız aralığı tutar.
        self.ivars().anchor.set(Some(cell));
        Some((session, cell))
    }

    /// Sürüklemenin iki ucu: çapa basıştan, aktif uç bu olaydan. Basışsız
    /// sürükleme (çapa yok) yutulur — `mouseDown:`'sız `mouseDragged:` olmaz
    /// ama AppKit'in sözüne güvenilmez, tipe güvenilir.
    ///
    /// Üçlü `#[allow]`'suz geçiyor, çünkü clippy'nin saydığı şey parantez:
    /// dördüncü bir eleman eklenecekse o gün ayrı bir struct doğar.
    #[allow(clippy::type_complexity)]
    fn drag_cells(&self, event: &NSEvent) -> Option<(Arc<Session>, (u16, u16), (u16, u16))> {
        // `cell_under` çağrılamaz: çapayı ezerdi.
        let anchor = self.ivars().anchor.get()?;
        let (session, cell) = self.session_cell(event)?;
        Some((session, anchor, cell))
    }

    /// Olay noktasını hücreye indirir; dışarısı `None` (yutulur).
    fn event_cell(&self, event: &NSEvent) -> Option<(u16, u16)> {
        let (metrics, (cols, rows)) = self.ivars().metrics.get()?;
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let scale = self.window()?.backingScaleFactor();
        point_to_cell((point.x, point.y), metrics.cell_px(), scale, cols, rows)
    }

    /// Command'lı tuşun düştüğü kısayol — **saf karar**, panoya ve oturuma
    /// dokunmaz, bu yüzden sınanabilir.
    ///
    /// `flags`'in Command'ı **içerdiği varsayılır** (çağıran onu zaten
    /// süzüyor); burada sorulan yalnız "hangi kısayol" ve "kısayol mu".
    ///
    /// Kapı: Shift/Option/Control/Fn'li Command bir terminal kısayolu
    /// değildir — Cmd-Shift-C yutulur. `Function` da kapıda, çünkü Fn ile
    /// gelen Command kombinasyonu da tanınmıyor.
    ///
    /// Karşılaştırma ASCII-duyarsız, çünkü `characters` **CapsLock**'ta
    /// "C"/"V" verir (`AlphaShift` yukarıdaki kapıda yok). Shift'in harfe
    /// gömülmesi ("C") bu satıra hiç ulaşmaz — Shift kapıda eleniyor.
    fn command_shortcut(chars: &str, flags: NSEventModifierFlags) -> Option<Shortcut> {
        // Bayraklar `bitflags` ilişkili sabitleri, enum varyantı değil: tam
        // nitelenirler, `use` ile içe aktarılmazlar.
        let extra = NSEventModifierFlags::Shift
            | NSEventModifierFlags::Option
            | NSEventModifierFlags::Control
            | NSEventModifierFlags::Function;
        if flags.intersects(extra) {
            return None;
        }
        // `eq_ignore_ascii_case`, `to_lowercase` değil: ikincisi vuruş başına
        // ikinci bir `String` kurardı, karşılaştırmanın buna ihtiyacı yok.
        if chars.eq_ignore_ascii_case("c") {
            Some(Shortcut::Copy)
        } else if chars.eq_ignore_ascii_case("v") {
            Some(Shortcut::Paste)
        } else {
            None
        }
    }

    /// Kısayolu uygular. Pano **burada** alınıyor: `generalPasteboard()`
    /// AppKit'e bir mesajdır (ilk kullanımda pboard bağlantısı kurar) ve her
    /// Command'lı tuşta ödenmesi gerekmez — yutulan kısayollar (Cmd-W, Cmd-Q)
    /// artık panoya hiç dokunmuyor.
    fn run_shortcut(session: &Session, shortcut: Shortcut) {
        let board = NSPasteboard::generalPasteboard();
        match shortcut {
            // Kopya `selection_text()`'ten okur — phase-1'in tek metin yolu.
            // Seçim yoksa ya da boşsa pano el değmeden kalır (`clipboard`).
            Shortcut::Copy => {
                clipboard::copy(&board, session.selection_text());
            }
            // Yapıştırma `paste()` yolundan girer: 2004 setse bracketed
            // sarılır, değilse ham yazılır. Ham bayt `session.write`'a değmez.
            Shortcut::Paste => {
                if let Some(text) = clipboard::read(&board) {
                    session.paste(text.into_bytes());
                }
            }
        }
    }
}

/// [`BateriView::command_shortcut`]'ın iki cevabı. Menü günü (00X) bu tip ve
/// onu okuyan `keyDown:` dalı birlikte **silinir**: kalıcı çözüm menü
/// seçicileridir, bu geçici köprüdür.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shortcut {
    Copy,
    Paste,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dört testin ortak sahnesi: 100×33 grid, 9×18 hücre, @2x.
    /// View 450×297 nokta eder.
    fn scene(view_px: (f64, f64)) -> Option<(u16, u16)> {
        point_to_cell(view_px, (9, 18), 2.0, 100, 33)
    }

    #[test]
    fn view_origin_maps_to_top_left_cell() {
        // View `isFlipped`: sol üst köşe (0,0) hücresi. Y alttan gelseydi
        // satır 32'ye inerdi.
        assert_eq!(scene((0.0, 0.0)), Some((0, 0)));
    }

    #[test]
    fn cell_middle_stays_in_same_cell() {
        // Hücrenin ortası aynı hücreyi verir — kenar değil taban yuvarlama.
        // Hücre view'da 4.5×9 nokta eder; (2,1) hücresinin ortası x = 2.5,
        // y = 1.5 hücre.
        assert_eq!(scene((2.5 * 4.5, 1.5 * 9.0)), Some((2, 1)));
    }

    #[test]
    fn outside_points_are_swallowed() {
        // Sağ ve alt kenar dışı yutulur: olmayan hücreyi seçmek `Some("")`
        // üretip phase-2'nin kopyasını boşaltırdı.
        assert_eq!(scene((900.0, 100.0)), None);
        assert_eq!(scene((100.0, 600.0)), None);
        // Pencere grid'den büyük olabilir (kenar boşluğu): view 500×400 ama
        // grid 450×297 — taşan tıklama yutulur.
        assert_eq!(scene((470.0, 100.0)), None);
        assert_eq!(scene((100.0, 350.0)), None);
    }

    /// Command'lı bir tuşun bayrakları: Command tek başına.
    fn cmd() -> NSEventModifierFlags {
        NSEventModifierFlags::Command
    }

    #[test]
    fn command_shortcut_routes_c_and_v() {
        assert_eq!(
            BateriView::command_shortcut("c", cmd()),
            Some(Shortcut::Copy)
        );
        assert_eq!(
            BateriView::command_shortcut("v", cmd()),
            Some(Shortcut::Paste)
        );
        // CapsLock `characters`'ı "C"/"V" yapar — tek harf dışı fark bu.
        assert_eq!(
            BateriView::command_shortcut("C", cmd()),
            Some(Shortcut::Copy)
        );
        assert_eq!(
            BateriView::command_shortcut("V", cmd()),
            Some(Shortcut::Paste)
        );
    }

    #[test]
    fn command_shortcut_swallows_unknown_and_composed_keys() {
        // Tanınmayan harf kısayol değil: `keyDown:` yine yutar (menü yok),
        // ama panoya dokunmaz.
        assert_eq!(BateriView::command_shortcut("w", cmd()), None);
        assert_eq!(BateriView::command_shortcut("q", cmd()), None);
        // Çok harfli `characters` (ölü tuş bileşimi) tek harfe indirgenmez.
        assert_eq!(BateriView::command_shortcut("cv", cmd()), None);

        // Shift/Option/Control/Fn'li Command kısayol değil: Cmd-Shift-C
        // yutulur. Kapı bu dört bayrakta ve yalnız burada.
        for extra in [
            NSEventModifierFlags::Shift,
            NSEventModifierFlags::Option,
            NSEventModifierFlags::Control,
            NSEventModifierFlags::Function,
        ] {
            assert_eq!(
                BateriView::command_shortcut("c", cmd() | extra),
                None,
                "{extra:?}"
            );
            assert_eq!(
                BateriView::command_shortcut("v", cmd() | extra),
                None,
                "{extra:?}"
            );
        }
        // CapsLock kapıda **değil**: Cmd-CapsLock-C hâlâ kopyalar, çünkü
        // `AlphaShift` karakteri büyütmekten başka bir şey yapmıyor.
        assert_eq!(
            BateriView::command_shortcut("C", cmd() | NSEventModifierFlags::CapsLock),
            Some(Shortcut::Copy)
        );
    }

    #[test]
    fn scale_changes_the_cell() {
        // Aynı view noktası iki ölçekte iki ayrı hücre: ölçü fiziksel
        // pikselden geliyor ve ölçek çarpanı atlanırsa retina makinede seçim
        // yarı kayar.
        let at1x = point_to_cell((90.0, 150.0), (9, 18), 1.0, 100, 33);
        let at2x = point_to_cell((90.0, 150.0), (9, 18), 2.0, 100, 33);
        assert_eq!((at1x, at2x), (Some((10, 8)), Some((20, 16))));
    }
}
