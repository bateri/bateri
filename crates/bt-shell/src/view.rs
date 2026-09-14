//! Pencerenin içeriği: `CAMetalLayer`'ı taşıyan ve klavyeyi PTY'ye akıtan view.
//!
//! Çizim burada **yok** — layer'ın içeriğini `bt-gpu` doldurur. Bu sınıfın işi
//! first responder olmak, tuş vuruşunu [`crate::keys::encode_key`]'e verip
//! çıkan baytı ya da oku oturuma yazmak ve fareyi (basış, sürükleme, bırakış ve
//! tekerlek) hücreye çevirip oturuma iletmek. Terminal kararları (seçim
//! aralığı, sayfanın boyu, tekerleğin kipe göre yolu, okun baytı) `bt-core`'da;
//! burada AppKit'e bakan taraf yaşar — piksel → hücre aritmetiği, tekerleğin
//! satır artığı, sürüklemenin sürüp sürmediği.

use std::cell::OnceCell;
use std::sync::Arc;

use bt_core::{CellHalf, SelectionPoint, Session, Wheel};
use bt_gpu::CellMetrics;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventPhase, NSPasteboard, NSView};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect};

use crate::clipboard;
use crate::keys::{KeyInput, encode_key, page_scroll};

/// Fare noktası → seçim ucu. **Saf ve AppKit'siz**, bu yüzden sınanabilir.
///
/// `view_px` view koordinatında (nokta), `cell_px` fiziksel piksel, `scale`
/// backing ölçeği: ölçü `bt-gpu`'dan fiziksel geldiği için fare de önce
/// fiziksel piksele çıkar, sonra bölünür.
///
/// **Adı "hücre" kaldı, dönen şey hücre + yarısı**: yarı hücrenin içindeki
/// yerin ikinci yarısı, ayrı bir soru değil — `col` ile aynı bölmeden çıkar.
/// Çağıranı (`window_point_cell`: fare olayı ve kaydırmada fare konumu) zaten
/// "farenin altındaki hücre" diyor; ikinci bir ad (`point_to_selection_point`) yalnız churn
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

/// Tekerlek deltası → tam satır ve **taşınan artık**. Saf, sınanabilir.
///
/// `unit` bir satırın delta cinsinden boyu: trackpad'de (`hasPreciseScrollingDeltas`)
/// delta nokta cinsinden gelir ve birim hücre boyudur (nokta); klasik
/// tekerlekte delta zaten satırdır ve birim 1. İşaret korunur — AppKit'in
/// `scrollingDeltaY`'si "doğal kaydırma" tercihi uygulanmış hâldedir ve artısı
/// belgenin başına doğrudur, yani `Session::scroll_wheel`'in "artı geriye"
/// yönüyle aynı.
///
/// **Artık neden taşınıyor:** trackpad hücre boyundan küçük deltalar yağdırır;
/// her olay tek başına sıfıra kesilseydi yavaş bir kaydırma hiç satır
/// üretmezdi. Kesme sıfıra doğru (`trunc`), artık işaretini korur: yön dönünce
/// önce birikmiş artık erir.
///
/// Sonlu olmayan toplam (sıfır birim, NaN delta) `(0, 0.0)` verir — NaN artığa
/// girseydi sonraki her toplam NaN olur ve tekerlek sessizce ölürdü. Dev delta
/// `as i32` ile doyar; geçmişin boyuna kırpma `bt-core`'da.
pub(crate) fn wheel_lines(delta: f64, unit: f64, carry: f64) -> (i32, f64) {
    let total = carry + delta / unit;
    if !total.is_finite() {
        return (0, 0.0);
    }
    let whole = total.trunc();
    (whole as i32, total - whole)
}

pub(crate) struct ViewIvars {
    /// View, oturumdan **önce** doğmak zorunda: grid ölçüsü contentView'ın
    /// bounds'undan türüyor ve `Session::spawn` o ölçüyü istiyor. Bir tuş
    /// vuruşu arada geçemez ama sebebi pencerenin henüz key olmaması değil
    /// (`makeKeyAndOrderFront` daha önce koşuyor): boşluk
    /// `applicationDidFinishLaunching`'in içinde, **run loop dönmeden**
    /// kapanıyor, yani araya hiçbir olay düşemiyor.
    session: OnceCell<Arc<Session>>,
    /// Sol tuş basılı ve seçim bu basışla başladı mı.
    ///
    /// Çapanın **kendisi** burada değil: basışın hücresi ve yarısı
    /// `Session::set_selection`'la `bt-core`'a gidiyor ve orada grid mutlağında
    /// kalıyor. Çapa pencere hücresi olarak burada tutulduğu sürece basılı
    /// sürüklemenin ortasındaki kaydırma onu bayatlatıyordu — aynı satır
    /// numarası kaydırmadan sonra başka bir içeriği gösterir (phase-1'in
    /// devri, 006 phase-3'te kapandı). Geriye kalan soru yalnız "sürükleme
    /// sürüyor mu": basışsız bir `mouseDragged:` eski seçimin ucunu
    /// taşımasın.
    dragging: std::cell::Cell<bool>,
    /// Tekerleğin satıra dönmemiş artığı ([`wheel_lines`]). Üç yerde sıfırlanır,
    /// üçünde de kalan artık bir sonraki kaydırmaya ait değil: yeni jestin
    /// başında (önceki jestin kırıntısı yeni jesti erken ya da geç tetiklemesin),
    /// tekerlek yoksayılınca (`Wheel::Ignored`: bir kipin artığı sonraki kipe
    /// taşınmasın) ve geçmişin ucuna dayanınca (uca doğru biriken momentum
    /// ters yöndeki ilk satırı geciktirmesin). Tekerlek uygulamaya gidince
    /// (`Wheel::Sent`) **korunur**: trackpad'le yavaş kaydırmada her olayın
    /// küsuratı düşseydi `less` sarsak kayardı.
    scroll_carry: std::cell::Cell<f64>,
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
        /// Çapa **yarısıyla** `bt-core`'a gider: basış hücrenin hangi
        /// yarısındaysa sınır oradan geçer, sürükleme boyunca da orada kalır.
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
            let Some((session, anchor)) = self.session_cell(event) else {
                return;
            };
            self.ivars().dragging.set(true);
            // İmleç çapa hücresinden sürüklenir: ters yöne ilk hareket seçimi
            // boşaltmamalı, fare ucundan büyümeli. İki uç **aynı** olduğu
            // sürece seçim boştur — yani sürüklemesiz tık hiçbir şey seçmez ve
            // Cmd-C panoya dokunmaz (`selection_text()` `None`).
            session.set_selection(anchor, anchor);
        }

        /// Sürükleme: aktif uç farenin şimdiki yeri, çapa `bt-core`'da
        /// (`Session::update_selection` yalnız bitişi taşır). Çizilen aralığı
        /// değiştirmeyen olaylar (aynı yarıda kalmak, hücre sınırını geçmek)
        /// oturumun aralık kapısında eleniyor — kare istenmez.
        ///
        /// Basışsız sürükleme yutulur: `mouseDown:`'sız `mouseDragged:` olmaz
        /// ama AppKit'in sözüne güvenilmez — olsaydı önceki seçimin ucunu
        /// taşırdı.
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            if !self.ivars().dragging.get() {
                return;
            }
            if let Some((session, cell)) = self.session_cell(event) {
                session.update_selection(cell);
            }
        }

        /// Tuş bırakıldı: sürükleme biter, seçim ekranda kalır (Cmd-C onu
        /// kopyalar).
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            self.ivars().dragging.set(false);
        }

        /// Tekerlek ve trackpad: uygulama fare raporu istediyse — ekran fark
        /// etmez — tekerlek raporu olarak uygulamaya gider; istemediyse
        /// alternate screen'de ok olarak gider, birincil ekranda görünen
        /// pencereyi geçmişe kaydırır. Kaydırma çubuğu **yok** — AppKit kroniği
        /// (thumb, orantı, sürükleme), eşik için gerekli değil.
        ///
        /// Kipe göre karar `bt-core`'da (`Session::scroll_wheel`); burası
        /// satırı, işaretçinin hücresini ve Shift'i verir. Yatay delta
        /// yoksayılıyor — yatay kaydırılacak bir şey yok (yatay tekerlek
        /// raporu, 66/67, kapsam dışı). macOS klasik farede Shift+tekerleği
        /// yatay deltaya çeviriyor, yani Shift'in kolu bu yolda çoğunlukla
        /// trackpad'den gelir.
        ///
        /// Basılı sürüklemenin ortasında kaydırma olursa seçimin ucu farenin
        /// **yeni** altındaki hücreye taşınır ([`BateriView::follow_pointer`]).
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let Some(session) = self.ivars().session.get() else {
                return;
            };
            let Some((metrics, _)) = self.ivars().metrics.get() else {
                return;
            };
            let Some(window) = self.window() else {
                return;
            };
            // Trackpad nokta cinsinden: birim hücre boyu, fiziksel pikselden
            // noktaya indirilmiş (ölçü `bt-gpu`'dan fiziksel geliyor). Klasik
            // tekerlek zaten satır verir.
            let unit = if event.hasPreciseScrollingDeltas() {
                f64::from(metrics.cell_px().1) / window.backingScaleFactor()
            } else {
                1.0
            };
            let carry = &self.ivars().scroll_carry;
            if event.phase().contains(NSEventPhase::Began) {
                carry.set(0.0);
            }
            let (lines, rest) = wheel_lines(event.scrollingDeltaY(), unit, carry.get());
            carry.set(rest);
            if lines == 0 {
                return;
            }
            // İşaretçinin hücresi fare kipinde rapora giriyor; yarısı girmiyor
            // (`bt-core` okumuyor). Kenar dışı nokta yapışır, `None` yalnız
            // sıfır boyutlu grid'de.
            let Some(pointer) = self.event_cell(event) else {
                return;
            };
            let shift = event.modifierFlags().contains(NSEventModifierFlags::Shift);
            match session.scroll_wheel(lines, pointer, shift) {
                Wheel::Scrolled(0) | Wheel::Ignored => carry.set(0.0),
                Wheel::Scrolled(_) => self.follow_pointer(session),
                // Pencere kaymadı, uygulama kendi ekranını çiziyor: seçim ucu
                // taşınmaz, artık korunur (`ViewIvars::scroll_carry`).
                Wheel::Sent => {}
            }
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
            let chars = chars.to_string();
            // Shift+PgUp/PgDn terminalin kaydırmasıdır, uygulamanın tuşu değil —
            // ama yalnız oturum kabul ederse. Alternate screen'de kaydırma
            // reddedilir (`None`) ve tuş aşağıdaki yoldan uygulamaya düz PgUp
            // olarak gider: less/vim'de Shift+PgUp da sayfa çevirir, yutulmaz.
            // Sayfanın kaç satır olduğu `bt-core`'un kararı (`scroll_page`).
            let shift = flags.contains(NSEventModifierFlags::Shift);
            if let Some(pages) = page_scroll(&chars, shift)
                && let Some(moved) = session.scroll_page(pages)
            {
                if moved != 0 {
                    self.follow_pointer(session);
                }
                return;
            }
            let ctrl = flags.contains(NSEventModifierFlags::Control);
            // `super`'e geçmiyoruz: `NSResponder::keyDown:` tanımadığı tuşta
            // beep çalar ve terminalde her ok tuşu bip sesi olurdu.
            match encode_key(&chars, ctrl) {
                Some(KeyInput::Bytes(bytes)) => session.write(&bytes),
                // Okun baytı DECCKM'e bağlı, kip `bt-core`'da.
                Some(KeyInput::Arrow(arrow)) => session.write_arrow(arrow),
                None => {}
            }
        }
    }
);

impl BateriView {
    pub(crate) fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ViewIvars {
            session: OnceCell::new(),
            dragging: std::cell::Cell::new(false),
            scroll_carry: std::cell::Cell::new(0.0),
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

    /// Olay noktasını seçim ucuna indirir. `None` yalnız ölçü ya da pencere
    /// henüz yokken ve grid sıfır boyutluyken — kenar dışı nokta yapışır.
    fn event_cell(&self, event: &NSEvent) -> Option<SelectionPoint> {
        self.window_point_cell(event.locationInWindow())
    }

    /// Pencere koordinatındaki noktayı seçim ucuna indirir — [`Self::event_cell`]'in
    /// olaysız hâli: tuşla kaydırmada farenin yerini taşıyan bir fare olayı yok.
    fn window_point_cell(&self, in_window: NSPoint) -> Option<SelectionPoint> {
        let (metrics, (cols, rows)) = self.ivars().metrics.get()?;
        let point = self.convertPoint_fromView(in_window, None);
        let scale = self.window()?.backingScaleFactor();
        point_to_cell((point.x, point.y), metrics.cell_px(), scale, cols, rows)
    }

    /// Pencere kaydı; basılı bir sürükleme varsa seçimin ucunu farenin **yeni**
    /// altındaki hücreye taşır — fare kıpırdamadı ama altındaki içerik değişti.
    /// Tuşu basılı tutup geçmişe inmek (tekerlek ya da Shift+PgUp) seçimi oraya
    /// uzatır; çapa `bt-core`'da grid mutlağında, kaymaz. İki tetikleyici **tek**
    /// yoldan geçiyor ki aynı jest iki ayrı davranış göstermesin.
    ///
    /// Fare konumu olaydan değil pencereden okunuyor
    /// (`mouseLocationOutsideOfEventStream`): tuş olayının konumu yok.
    ///
    /// `dragging` tek başına yetmez: `mouseUp:` bu view'a hiç varmazsa
    /// (sürükleme ortasında bir modal, sistem jesti) bayrak bayat `true` kalır
    /// ve tuşsuz her kaydırma eski seçimi sessizce uzatırdı — sonraki Cmd-C onu
    /// kopyalar. Tuşun **gerçekten** basılı olduğu sistemden soruluyor; değilse
    /// bayat bayrak burada iner.
    fn follow_pointer(&self, session: &Session) {
        if !self.ivars().dragging.get() {
            return;
        }
        if NSEvent::pressedMouseButtons() & 1 == 0 {
            self.ivars().dragging.set(false);
            return;
        }
        let Some(window) = self.window() else {
            return;
        };
        if let Some(cell) = self.window_point_cell(window.mouseLocationOutsideOfEventStream()) {
            session.update_selection(cell);
        }
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
    fn wheel_whole_lines_pass_through() {
        // Trackpad: birim hücre boyu (nokta). Tam bir hücre = bir satır, işaret
        // korunur — artı geriye, `Session::scroll_wheel` ile aynı yön.
        assert_eq!(wheel_lines(9.0, 9.0, 0.0), (1, 0.0));
        assert_eq!(wheel_lines(-27.0, 9.0, 0.0), (-3, 0.0));
        // Klasik tekerlek: `scrollingDeltaY` zaten satır, birim 1.
        assert_eq!(wheel_lines(2.0, 1.0, 0.0), (2, 0.0));
    }

    #[test]
    fn wheel_sub_line_deltas_accumulate() {
        // Trackpad hücre boyundan küçük deltalar yağdırır. Artık taşınmasaydı
        // yavaş bir kaydırma **hiç** satır üretmezdi: her olay tek başına
        // sıfıra kesilir.
        let (lines, carry) = wheel_lines(4.0, 9.0, 0.0);
        assert_eq!(lines, 0);
        let (lines, carry) = wheel_lines(4.0, 9.0, carry);
        assert_eq!(lines, 0);
        let (lines, carry) = wheel_lines(4.0, 9.0, carry);
        assert_eq!(lines, 1);
        assert!((carry - 3.0 / 9.0).abs() < 1e-9, "{carry}");
        // Yön dönünce artık önce eriyor: geriye birikmiş üçte bir, ileriye
        // üçte iki hücre → toplam üçte bir ileri, satır yok.
        let (lines, carry) = wheel_lines(-6.0, 9.0, carry);
        assert_eq!(lines, 0);
        assert!((carry + 3.0 / 9.0).abs() < 1e-9, "{carry}");
    }

    #[test]
    fn wheel_degenerate_inputs_do_not_poison_the_carry() {
        // Sıfır birim (ölçüsüz hücre) sonsuz, 0/0 NaN üretir; NaN artığa
        // girerse sonraki her toplam NaN olur ve tekerlek sessizce ölürdü.
        assert_eq!(wheel_lines(9.0, 0.0, 0.0), (0, 0.0));
        assert_eq!(wheel_lines(0.0, 0.0, 0.0), (0, 0.0));
        assert_eq!(wheel_lines(f64::NAN, 9.0, 0.5), (0, 0.0));
        // Dev delta doyar; kırpma `bt-core`'da (geçmişin boyuna).
        assert_eq!(wheel_lines(1e300, 1.0, 0.0).0, i32::MAX);
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
