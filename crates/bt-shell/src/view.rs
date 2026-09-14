//! Pencerenin içeriği: `CAMetalLayer`'ı taşıyan ve klavyeyi PTY'ye akıtan view.
//!
//! Çizim burada **yok** — layer'ın içeriğini `bt-gpu` doldurur. Bu sınıfın tek
//! işi first responder olmak ve tuş vuruşunu [`crate::keys::encode_key`]'e
//! verip çıkan baytları oturuma yazmak.

use std::cell::OnceCell;
use std::sync::Arc;

use bt_core::{CellHalf, SelectionPoint, Session};
use bt_gpu::CellMetrics;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSPasteboard, NSView};
use objc2_foundation::{NSObjectProtocol, NSRect};

use crate::clipboard;
use crate::keys::encode_key;

/// Fare noktası → seçim ucu. **Saf ve AppKit'siz**, bu yüzden sınanabilir.
///
/// `view_px` view koordinatında (nokta), `cell_px` fiziksel piksel, `scale`
/// backing ölçeği: ölçü `bt-gpu`'dan fiziksel geldiği için fare de önce
/// fiziksel piksele çıkar, sonra bölünür.
///
/// **Adı "hücre" kaldı, dönen şey hücre + yarısı**: yarı hücrenin içindeki
/// yerin ikinci yarısı, ayrı bir soru değil — `col` ile aynı bölmeden çıkar.
/// Çağrı yerlerinin hepsi (çapa, sürükleme, olay) zaten "farenin altındaki
/// hücre" diyor; ikinci bir ad (`point_to_selection_point`) yalnız churn
/// olurdu.
///
/// Kenar dışı her nokta **en yakın hücreye yapışır**: sürükleme grid'in hangi
/// yanından çıkarsa çıksın o kenara tutunur. Sağa taşan nokta son sütunun
/// **sağ** yarısıdır — satır sonuna sürükleyen fare grid'in sağındaki
/// kullanılmayan şeride (`split_into_grid` sütunu aşağı yuvarlıyor) geçince
/// son harf seçimde kalmalı. `None` yalnız sıfır sütunlu/satırlı grid içindir
/// (simge durumundaki pencere): yapışacak hücre yok.
///
/// Taban yuvarlama (`as u16` kesmesi): farenin **hangi** hücrede olduğu
/// soruluyor ve `split_into_grid` ile aynı aritmetik. Sol/üst yapışması ayrı
/// bir kırpma değil, dilin iki kuralı: `f64 as u16` negatifi 0'a **doyurur**
/// (sarmaz), ve `f64`'ün `%`'i bölünenin işaretini korur — negatif x'in artığı
/// negatiftir, yani her zaman yarı hücreden küçük ve **sol** yarı. Grid'in
/// solundan başlayan sürükleme bu yüzden 0. hücreyi seçime katar;
/// `rem_euclid`'e geçen bir "düzeltme" artığı pozitife çevirip onu dışarıda
/// bırakırdı (`dragging_left_of_the_grid_clamps_to_the_left_half` bekçisi).
pub(crate) fn point_to_cell(
    view_px: (f64, f64),
    cell_px: (u16, u16),
    scale: f64,
    cols: u16,
    rows: u16,
) -> Option<SelectionPoint> {
    if cols == 0 || rows == 0 {
        return None;
    }
    let (cell_w, cell_h) = (f64::from(cell_px.0), f64::from(cell_px.1));
    // View `isFlipped`, yani y grid yönünde (üstten) geliyor: tersine çevirme
    // yok. Grid'in boyunu view değil `cols`/`rows` söylüyor — pencere kenar
    // boşluğundaki nokta son hücreye yapışsın.
    let x = view_px.0 * scale;
    let row = ((view_px.1 * scale / cell_h) as u16).min(rows - 1);
    let col = (x / cell_w) as u16;
    let (col, half) = if col < cols {
        (col, cell_half(x, cell_w))
    } else {
        (cols - 1, CellHalf::Right)
    };
    Some(SelectionPoint { col, row, half })
}

/// Hücre içi x'in yarısı — seçim sınırını çizen tek girdi.
///
/// Yarı `col`'dan **türetilemez**: `col` tam sayıya kesiyor ve kesme artığı
/// atıyor, yani hücrenin neresinde olduğumuz bilgisi orada yok. Kaynak
/// bölmeden önceki **artıktır** (x, `cell_w`'ye göre). Negatif x'te artık da
/// negatiftir ve sol yarıya düşer — sol kenar kuralı [`point_to_cell`]'de.
///
/// **Orta nokta sağ yarıya yazıldı** (`>=`): iki yarı `[0, w/2)` ve
/// `[w/2, w)` diye tam bölüşür — hiçbir x yarısız kalmaz, hiçbiri iki yarıya
/// birden düşmez ve kural tek karşılaştırma olur. Tam ortaya basmak (fare
/// pikseli tam sınıra düşerse) hücreyi başlangıç ucunda **dışarıda**, bitiş
/// ucunda **içeride** bırakır — sağ yarının iki uçtaki anlamı bu
/// ([`CellHalf`]).
fn cell_half(x_px: f64, cell_w: f64) -> CellHalf {
    if x_px % cell_w >= cell_w / 2.0 {
        CellHalf::Right
    } else {
        CellHalf::Left
    }
}

pub(crate) struct ViewIvars {
    /// View, oturumdan **önce** doğmak zorunda: grid ölçüsü contentView'ın
    /// bounds'undan türüyor ve `Session::spawn` o ölçüyü istiyor. Bir tuş
    /// vuruşu arada geçemez ama sebebi pencerenin henüz key olmaması değil
    /// (`makeKeyAndOrderFront` daha önce koşuyor): boşluk
    /// `applicationDidFinishLaunching`'in içinde, **run loop dönmeden**
    /// kapanıyor, yani araya hiçbir olay düşemiyor.
    session: OnceCell<Arc<Session>>,
    /// Sürüklemenin çapası: basışın hücresi **ve yarısı**. Yarı da saklanıyor:
    /// sınırı o çiziyor, `bt-core` ise yalnız aralığı tutar — çapayı
    /// hatırlamaz. Yarı kaybolsaydı (çapa yalnız hücre olsaydı) sürükleme
    /// çapayı her olayda yeniden yorumlamak zorunda kalır, basış anındaki
    /// yarısını kaybederdi.
    anchor: std::cell::Cell<Option<SelectionPoint>>,
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
        /// Çapa **yarısıyla** saklanır: basış hücrenin hangi yarısındaysa
        /// sınır oradan geçer, sürükleme boyunca da orada kalır.
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
            // boşaltmamalı, fare ucundan büyümeli. İki uç **aynı** olduğu
            // sürece seçim boştur — yani sürüklemesiz tık hiçbir şey seçmez ve
            // Cmd-C panoya dokunmaz (`selection_text()` `None`).
            session.set_selection(anchor, anchor);
        }

        /// Sürükleme: çapa fare basışının hücresi **ve yarısı**, aktif uç
        /// farenin şimdiki yeri. Çapa `Session`'dan okunmuyor — `bt-core`
        /// yalnız aralığı tutar, çapayı hatırlamaz. İki olay da aynı
        /// `set_selection`'ı çağırıyor; `mouseDown:` iki ucu da çapaya
        /// veriyor, burası aktif ucu fareye. Çizilen aralığı değiştirmeyen
        /// olaylar (aynı yarıda kalmak, hücre sınırını geçmek)
        /// `set_selection`'ın aralık kapısında eleniyor — kare istenmez.
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let Some((session, anchor, cell)) = self.drag_cells(event) else {
                return;
            };
            session.set_selection(anchor, cell);
        }

        /// Tuş bırakıldı: çapa düşer. Çapa pencere hücresi (ve yarısı)
        /// cinsinden saklanıyor; bırakma ile sonraki basış arasında kaydırma
        /// olursa bayat çapayla sürükleme hiç başlamıyor — `drag_cells`
        /// çapasız olayı yutuyor.
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

    /// Oturum + olayın altındaki uç (hücre ve yarısı). Üçü (`session`, ölçü,
    /// grid) birlikte yoksa `None`: yarım bilgiyle seçim başlatılamaz.
    fn session_cell(&self, event: &NSEvent) -> Option<(Arc<Session>, SelectionPoint)> {
        let session = Arc::clone(self.ivars().session.get()?);
        let cell = self.event_cell(event)?;
        Some((session, cell))
    }

    /// Olayın altındaki uç + bağlı oturum; çapayı da kurar.
    fn cell_under(&self, event: &NSEvent) -> Option<(Arc<Session>, SelectionPoint)> {
        let (session, cell) = self.session_cell(event)?;
        // Çapa burada saklanıyor: sürükleme çapa + aktif uç ister, `bt-core`
        // yalnız aralığı tutar. Yarısı da çapayla gidiyor — basış anındaki
        // yarı, sürüklemenin bir ucunu sabitleyen şey.
        self.ivars().anchor.set(Some(cell));
        Some((session, cell))
    }

    /// Sürüklemenin iki ucu: çapa basıştan, aktif uç bu olaydan. Basışsız
    /// sürükleme (çapa yok) yutulur — `mouseDown:`'sız `mouseDragged:` olmaz
    /// ama AppKit'in sözüne güvenilmez, tipe güvenilir.
    ///
    /// Üçlü `#[allow]`'suz geçiyor: uçlar adlı tip (`SelectionPoint`), iç içe
    /// demet değil. Dördüncü bir eleman eklenecekse o gün ayrı bir struct doğar.
    fn drag_cells(
        &self,
        event: &NSEvent,
    ) -> Option<(Arc<Session>, SelectionPoint, SelectionPoint)> {
        // `cell_under` çağrılamaz: çapayı ezerdi.
        let anchor = self.ivars().anchor.get()?;
        let (session, cell) = self.session_cell(event)?;
        Some((session, anchor, cell))
    }

    /// Olay noktasını seçim ucuna indirir. `None` yalnız ölçü ya da pencere
    /// henüz yokken ve grid sıfır boyutluyken — kenar dışı nokta yapışır.
    fn event_cell(&self, event: &NSEvent) -> Option<SelectionPoint> {
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

    /// Testlerin ortak sahnesi: 100×33 grid, 9×18 hücre, @2x.
    /// View 450×297 nokta eder.
    fn scene_point(view_px: (f64, f64)) -> Option<SelectionPoint> {
        point_to_cell(view_px, (9, 18), 2.0, 100, 33)
    }

    /// Sahnenin hücresi ve yarısı ayrı okunuyor: hücre testleri hücreye, yarı
    /// testleri yarıya baksın.
    fn scene(view_px: (f64, f64)) -> Option<(u16, u16)> {
        scene_point(view_px).map(|point| (point.col, point.row))
    }

    fn scene_half(view_px: (f64, f64)) -> Option<CellHalf> {
        scene_point(view_px).map(|point| point.half)
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
        // Hücre ile yarı **aynı** çeviriden çıkıyor, ayrı sorulmuyor.
        assert_eq!(
            scene_point((11.0, 13.5)),
            Some(SelectionPoint {
                col: 2,
                row: 1,
                half: CellHalf::Left,
            })
        );
    }

    #[test]
    fn halves_split_the_cell_at_its_middle() {
        // (2,1) hücresi view'da x ∈ [9.0, 13.5), y ∈ [9.0, 18.0) nokta; yarısı
        // fiziksel x'te cell_w/2 = 4.5 piksel, yani view'da 2.25 nokta. Sol
        // yarı 9.0–11.25, sağ yarı 11.25–13.5.
        assert_eq!(scene_half((9.0, 9.0)), Some(CellHalf::Left));
        assert_eq!(scene_half((11.0, 9.0)), Some(CellHalf::Left));
        assert_eq!(scene_half((11.5, 9.0)), Some(CellHalf::Right));
        assert_eq!(scene_half((13.4, 9.0)), Some(CellHalf::Right));
        // Yarı hücreyi kaydırmıyor: dördü de (2,1) hücresinde.
        for x in [9.0, 11.0, 11.5, 13.4] {
            assert_eq!(scene((x, 9.0)), Some((2, 1)), "x = {x}");
        }
    }

    #[test]
    fn the_exact_middle_belongs_to_the_right_half() {
        // Orta nokta **yazılı** bir karar: yarılar `[0, w/2)` ve `[w/2, w)`
        // diye bölüşüyor, yani tam sınır sağ yarıya düşer (view'da
        // 9.0 + 2.25 = 11.25 nokta); bir tık solu hâlâ sol yarıdır. Sağ yarı
        // başlangıç ucunda hücreyi dışarıda, bitiş ucunda içeride bırakır.
        assert_eq!(scene_half((11.25, 9.0)), Some(CellHalf::Right));
        assert_eq!(scene_half((11.25 - 0.25, 9.0)), Some(CellHalf::Left));
    }

    #[test]
    fn dragging_left_of_the_grid_clamps_to_the_left_half() {
        // Grid'in solundaki x 0. hücrenin **sol** yarısına yapışır: `as u16`
        // doyuruyor, `%` bölünenin işaretini koruyor (negatif artık < w/2).
        // Artık pozitife çevrilseydi (`rem_euclid`) sağ yarıya düşer ve sol
        // kenardan başlayan sürükleme 0. hücreyi dışarıda bırakırdı —
        // kullanıcı satır başından seçmek isterken ilk harf eksik gelirdi.
        //
        // Nokta **seçilmiş**: view'da -1 nokta, @2x'te -2 piksel; `-2 % 9 = -2`
        // (sol), `(-2).rem_euclid(9) = 7` (sağ). İki kural her
        // `[-(k+½)w, -kw)` aralığında ayrışıyor, geri kalanında aynı yarıyı
        // veriyor — -3 nokta (-6 piksel, artık 3) ikisinde de sol yarıya düşer
        // ve bu sınamayı bekçi olmaktan çıkarırdı.
        assert_eq!(scene((-1.0, 9.0)), Some((0, 1)));
        assert_eq!(scene_half((-1.0, 9.0)), Some(CellHalf::Left));
    }

    #[test]
    fn points_past_the_grid_stick_to_its_edge() {
        // Sağ ve alt kenar dışı **yutulmaz**, son sütuna/satıra yapışır. Yarı
        // artık seçimi belirlediği için yutmak bir kayıp üretiyordu: pencere
        // genişliği hücrenin tam katı değilse grid'in sağında kullanılmayan bir
        // şerit kalıyor (`split_into_grid` sütunu aşağı yuvarlıyor) ve satır
        // sonuna doğru sürükleyen fare
        // oraya geçince olay düşer, seçim grid'deki son olayda kalırdı. O olay
        // son sütunun sol yarısındaysa son harf kopyadan eksik çıkardı.
        // Sağa taşan nokta son sütunun **sağ** yarısıdır: hücreyi katar.
        let last = |col, row| {
            Some(SelectionPoint {
                col,
                row,
                half: CellHalf::Right,
            })
        };
        assert_eq!(scene_point((900.0, 100.0)), last(99, 11));
        // Pencere grid'den büyük olabilir (kenar boşluğu): view 500×400 ama
        // grid 450×297.
        assert_eq!(scene_point((470.0, 100.0)), last(99, 11));
        // Alt taşma yalnız satırı kırpar; sütun ve yarı x'ten gelir.
        assert_eq!(scene((100.0, 600.0)), Some((22, 32)));
        assert_eq!(scene((100.0, 350.0)), Some((22, 32)));
    }

    #[test]
    fn empty_grid_has_no_cell() {
        // Simge durumundaki pencere sıfır sütun/satır verebilir: yapışacak bir
        // son hücre yok.
        assert_eq!(point_to_cell((1.0, 1.0), (9, 18), 2.0, 0, 33), None);
        assert_eq!(point_to_cell((1.0, 1.0), (9, 18), 2.0, 100, 0), None);
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
        assert_eq!(
            (at1x.map(|p| (p.col, p.row)), at2x.map(|p| (p.col, p.row))),
            (Some((10, 8)), Some((20, 16)))
        );
    }
}
