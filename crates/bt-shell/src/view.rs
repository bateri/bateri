//! Pencerenin içeriği: `CAMetalLayer`'ı taşıyan ve klavyeyi PTY'ye akıtan view.
//!
//! Çizim burada **yok** — layer'ın içeriğini `bt-gpu` doldurur. Bu sınıfın işi
//! first responder olmak, tuş vuruşunu doğru kola vermek, fareyi (basış,
//! sürükleme, bırakış ve tekerlek) hücreye çevirip oturuma iletmek, Finder'dan
//! bırakılan dosyanın yolunu giriş satırına düşürmek ve Edit
//! menüsünün Copy/Paste eylemlerini karşılamak. Terminal kararları (seçim
//! aralığı, sayfanın boyu, tekerleğin kipe göre yolu, okun baytı) `bt-core`'da;
//! burada AppKit'e bakan taraf yaşar — piksel → hücre aritmetiği, tekerleğin
//! satır artığı, sürüklemenin sürüp sürmediği.
//!
//! **Klavyenin metin yolu artık AppKit'in yığınından geçiyor** ve `keyDown:`
//! tek kapı değil bir **arbitraj**: Cmd'li olay kapalı bir izin listesinin
//! tek tuşu (⌘⌫) dışında yutulur, Shift+PgUp/PgDn
//! terminalin kaydırmasıdır, Control'lü olay doğrudan
//! [`crate::keys::encode_key`]'e gider ve **kalanı** `interpretKeyEvents:` ile
//! metin yığınına verilir. Yığın ölü tuş durumunu kendi tutar ve bileşim
//! tamamlanınca metni `insertText:` ile geri verir — düzen verisini biz
//! okumuyoruz. Yığın olayı almadıysa ([`ViewIvars::consumed`]) olay yine
//! `encode_key`'e düşer: fonksiyon tuşları, Enter/Tab/Esc/Backspace ve
//! tanınmayan her şey oradan geçer.
//!
//! **View aynı zamanda bir sürükleme hedefi** (`NSDraggingDestination`):
//! Finder'dan bırakılan dosyanın yolu kaçırılıp
//! ([`crate::quote::shell_quote`]) `Session::paste`'ten giriş satırına düşer.
//! Kayıt `NSPasteboardTypeFileURL` ile ve **yalnız** onunla — düz metin
//! damlası kaçış kuralını tipe koşullu yapardı ve Finder tek damlada iki tip
//! koyduğu için kolların sırası da bir karara dönerdi (018 Karar 4).

use std::cell::{Cell, OnceCell, RefCell};
use std::sync::Arc;

use bt_core::{CellHalf, Click, MouseButton, MouseModifiers, SelectionPoint, Session, Wheel};
use bt_gpu::{CellMetrics, Origin};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject, Sel};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSApplication, NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSEvent,
    NSEventModifierFlags, NSEventPhase, NSPasteboard, NSPasteboardTypeFileURL, NSTextInputClient,
    NSView,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSNotFound, NSObjectProtocol, NSPoint,
    NSRange, NSRangePointer, NSRect, NSString, NSUInteger, NSURL,
};

use crate::clipboard;
use crate::keys::{BACKSPACE, KeyInput, KeyPress, encode_key, only_char, page_scroll};
use crate::quote::shell_quote;

/// Izgaranın dışına düşen noktaya ne olacağı — [`point_to_cell`]'in tek
/// karar ekseni.
///
/// Kural tek cümle: **jest başlatan olay reddedilir, süren jestin devamı
/// kırpılır.** Basış ve düğmesiz hareket bir yer *söylüyor*, yani pencerenin
/// başlık çubuğundan, sol payından ya da dock bandından gelen bir koordinat
/// uygulamaya **yanlış** bir hücre bildirirdi; sürüklemenin ve bırakmanın
/// koordinatı ise zaten başlamış bir jestin devamı ve orada kenara yapışmak
/// hem xterm'in davranışı hem R6'nın şartı (düşen bırakma uygulamada takılı
/// kalmış bir düğme bırakır).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutOfGrid {
    /// En yakın hücreye yapıştır. `fill_rows` doldurma bandının boyu:
    /// orijinin üstü **doluysa** yine `None`, çünkü orada çizili metin var.
    Clamp { fill_rows: u16 },
    /// Nokta `[0, cols) × [0, rows)` dışındaysa `None`.
    Reject,
}

/// Fare noktası → seçim ucu. **Saf ve AppKit'siz**, bu yüzden sınanabilir.
///
/// `view_px` view koordinatında (nokta), `metrics` ve `origin_px` fiziksel
/// piksel, `scale` backing ölçeği: ölçü `bt-gpu`'dan fiziksel geldiği için
/// fare de önce fiziksel piksele çıkar, **sol payı ve dikey orijini düşer**,
/// sonra bölünür. Pay `cols` hesabıyla (`split_into_grid`) ve çizim
/// orijiniyle (`Frame::pos_at`) aynı `CellMetrics`'ten geliyor; üçü
/// ayrışsaydı belirti "fare bir sütun kayıyor" olurdu (010 Karar 3).
///
/// `origin_px` aynı cümlenin dikey yarısı ve kaynağı da tek
/// ([`bt_gpu::Origin`]): **çizilen** karenin orijini, kare yolunun yazdığı
/// değer. İkinci bir hesap olsaydı belirti "fare bir satır kayıyor" olurdu ve
/// kayma animasyonu boyunca (phase-2) her karede başka bir satır kayardı.
/// Parametre, alan değil: fonksiyon saf kalıyor ve orijini konu etmeyen
/// sınamalar `0.0` geçiyor.
///
/// `fill_rows` aynı gövdeden geliyor ([`bt_gpu::Origin`]) ve aynı sebeple:
/// "orijinin üstünde ne var" sorusunun iki yarısı — kaç piksel ve orası boş
/// mu — aynı karenin geometrisi. İkinci bir senkronizasyon kurulmadı; kare
/// yolu yazıyor, fare yolu okuyor, ikisi de ana thread.
///
/// Payın **içine** düşen tıklama ilk sütuna kırpılır, yani seçim payda
/// başlamaz: çıkarmadan sonra x negatif kalır ve aşağıdaki iki dil kuralı onu
/// 0. hücrenin sol yarısına yapıştırır — grid'in solundaki noktayla aynı yol,
/// ayrı bir kırpma dalı yok.
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
/// son harf seçimde kalmalı.
///
/// `None`'ın **iki** sebebi var: sıfır sütunlu/satırlı grid (simge
/// durumundaki pencere — yapışacak hücre yok) ve `fill_rows > 0` iken
/// orijinin **üstüne** düşen nokta. İkincisi bu fonksiyondaki tek **ret**:
/// doldurma bandı çizilince (017) orası boş değil, geçmişin satırları orada
/// duruyor ve o satırlar sınırın satır numaralarıyla temsil edilemiyor. Ret
/// kırpmanın yerine geçmiyor, **yanına** geçiyor — `fill_rows == 0` iken
/// yukarı taşan nokta bugünkü gibi 0. satıra yapışır ve yapışmalı: orası
/// gerçekten boş, üstelik `u16` taşmasının asıl koruması o kırpmada
/// (`the_origin_shifts_the_grid_down_and_the_blank_area_clamps`).
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
    metrics: CellMetrics,
    origin_px: f64,
    outside: OutOfGrid,
    scale: f64,
    cols: u16,
    rows: u16,
) -> Option<SelectionPoint> {
    if cols == 0 || rows == 0 {
        return None;
    }
    let (cell_px_w, cell_px_h) = metrics.cell_px();
    let (cell_w, cell_h) = (f64::from(cell_px_w), f64::from(cell_px_h));
    // View `isFlipped`, yani y grid yönünde (üstten) geliyor: tersine çevirme
    // yok. Grid'in boyunu view değil `cols`/`rows` söylüyor — pencere kenar
    // boşluğundaki nokta son hücreye yapışsın.
    let x = view_px.0 * scale - f64::from(metrics.gutter_px());
    // Dikey orijin de payla aynı şekilde düşülüyor ve **`f64`'te**: tabana
    // yapışmada boş alan **üstte** ve oraya yapılan tıklamada fark negatife
    // iner. `u16`'da yapılsaydı taşar ve pencerenin üst yarısına yapılan
    // tıklama son satırı seçerdi; `f64`'te negatif kalıyor ve `as u16` onu
    // sıfıra **doyuruyor** — payın yatayda kullandığı yolun aynısı, ayrı bir
    // kırpma dalı yok.
    let y = view_px.1 * scale - origin_px;
    match outside {
        // **Orijinin üstü doluysa ret, kırpma değil.** Kırpma yalnız orası
        // *boşken* doğru: doldurma bandı çizilince kullanıcı orada metin
        // görüyor ve 0. satıra yapışan bir çapa vurguyu gözün gördüğü yerden
        // başka bir yere koyardı. Doldurulan satırlar sınırın satır
        // numaralarıyla temsil edilemiyor (hepsi geçmişte, yani negatif) —
        // "yanlış seçilir" ile "seçilemez" arasında ikincisi dürüst olan.
        OutOfGrid::Clamp { fill_rows } if fill_rows > 0 && y < 0.0 => return None,
        OutOfGrid::Clamp { .. } => {}
        OutOfGrid::Reject
            if x < 0.0
                || y < 0.0
                || x >= cell_w * f64::from(cols)
                || y >= cell_h * f64::from(rows) =>
        {
            return None;
        }
        OutOfGrid::Reject => {}
    }
    let row = ((y / cell_h) as u16).min(rows - 1);
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

/// Fare olayının değiştiricileri. Shift rapora girmez, arbitrajı yapar —
/// gerekçesi [`MouseModifiers`]'ın doc'unda.
fn modifiers(event: &NSEvent) -> MouseModifiers {
    let flags = event.modifierFlags();
    MouseModifiers {
        shift: flags.contains(NSEventModifierFlags::Shift),
        // macOS'un Option'ı xterm'in Meta'sı — klavyedeki Meta kodlamasıyla
        // (`Option+←` → `\eb`) aynı tuş.
        meta: flags.contains(NSEventModifierFlags::Option),
        control: flags.contains(NSEventModifierFlags::Control),
    }
}

/// Çentiği taze hücreye taşır ve hücrenin **değiştiğini** söyler
/// ([`ViewIvars::motion_cell`]). Basış ve bırakma da buradan geçiyor,
/// cevabını atarak: onların işi çentiği tazelemek.
///
/// Serbest fonksiyon, metod değil: `define_class!` gövdeleri sınanamıyor ve
/// kısmanın kuralı ("ilk görüşte `true`, tekrarda `false`") bir bekçi hak
/// ediyor.
fn moved_to_new_cell(notch: &Cell<Option<(u16, u16)>>, cell: SelectionPoint) -> bool {
    let now = (cell.col, cell.row);
    notch.replace(Some(now)) != Some(now)
}

/// Düğmenin [`ViewIvars::sent_buttons`] içindeki biti. Raporun düğme
/// kodundan ([`bt_core`] içinde) **ayrı**: bu bir maske, o bir bayt değeri.
fn button_bit(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 1,
        MouseButton::Middle => 2,
        MouseButton::Right => 4,
    }
}

/// Tuş vuruşu terminale gider mi — **saf karar**, sınanıyor: Command'lı tuş,
/// **tek istisna dışında**, gitmez.
///
/// Menünün kısayolları (Cmd-C, Cmd-V, Cmd-Q, Cmd-,, Cmd +/−/0) bu soruya hiç
/// varmıyor: AppKit Command'lı tuşu `keyDown:`'dan **önce** `performKeyEquivalent:` ile
/// ana menüye veriyor (`menu`). Buraya varan Command'lı tuşun menüde
/// karşılığı yok (Cmd-T) ya da öğesi o an devre dışı; terminale düşseydi
/// kabuğa düz harf yazardı. Değiştiricinin geri kalanı sorulmuyor: Cmd-Shift-T
/// de bir kısayol denemesi, girdi değil.
///
/// **İstisna tek ve liste kapalı:** ⌘⌫ ([`BACKSPACE`]) geçer, baytı
/// [`encode_key`]'de (`\x15` = `^U`, zsh'te `kill-whole-line`). Liste kapalı
/// kalmak zorunda — açık bir kural bir gün Cmd-T'yi de geçirir ve kabuğa `t`
/// yazar (018 Karar 3).
///
/// İstisna **yalnız karakteri** soruyor, yanındaki değiştiricileri değil:
/// CapsLock açıkken de ⌘⌫ satırı silmeli ve Shift ya da Control ⌫'e ikinci
/// bir anlam vermiyor. `page_scroll`'un "Shift dışındaki değiştiriciler
/// sorulmuyor" kuralının aynısı; ters karar, bayrağı tesadüfen açık olan bir
/// kullanıcıda tuşu sessizce yutardı.
///
/// `chars` yoksa (saf modifier tuşu) Command'lı olay yutulur: izin listesinin
/// ölçütü bir karakter ve ortada karakter yok.
fn reaches_terminal(flags: NSEventModifierFlags, chars: Option<&str>) -> bool {
    if !flags.contains(NSEventModifierFlags::Command) {
        return true;
    }
    // Tek karakterlik eşleşme, `page_scroll` emsali ve **aynı sahipten**
    // ([`only_char`]): ⌫ ile **başlayan** çok karakterli bir `characters`
    // izin listesine girmez.
    only_char(chars.unwrap_or_default()) == Some(BACKSPACE)
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
    dragging: Cell<bool>,
    /// Basışı **uygulamaya raporlanmış** düğmeler — düğme başına bir bit
    /// (sol 1, orta 2, sağ 4).
    ///
    /// Rota basışta kilitleniyor (R6): Shift her olayda okunsaydı
    /// sürüklemenin ortasında Shift'i bırakmak seçim jestini rapor jestine
    /// çevirirdi. Bırakma bu yüzden kipi değil **bu biti** soruyor.
    ///
    /// `dragging`'in yanında ve onun içinde değil: ikisi ayrı sorulara cevap
    /// veriyor ("bu basış seçim başlattı" / "bu basış raporlandı") ve tek bir
    /// jest alanına katlanamazlar — sol tuşla seçim sürerken sağ tuşa basmak
    /// ikisini **aynı anda** doğuruyor. Bitmask, çünkü üç düğme birden basılı
    /// tutulabilir; tek bir "son rota" alanı sol bırakmayı sağın rotasıyla
    /// raporlardı.
    sent_buttons: Cell<u8>,
    /// Hareket raporunun son gittiği hücre — kısmanın çentiği.
    ///
    /// Rapor **hücre başına** en çok bir kez gitmeli: kısma olmadan
    /// işaretçinin her pikseli bir rapor üretir ve boşta duran bir uygulamayı
    /// sürekli çizdirirdi. Karşılaştırma `bt-core` çağrısından **önce**
    /// koşuyor, yani aynı hücrede kalan hareket `Term` kilidine hiç
    /// uğramıyor — "her pencerede dinle" kararının bedelini düşüren şey bu.
    ///
    /// Ölçü **görünür pencere** hücresi, grid satırı değil: uygulamanın
    /// kendi kaydırması işaretçi dururken rapor üretmemeli (xterm de ekran
    /// konumunda kısıyor). `half` girmiyor — rapor hücre çözünürlüğünde.
    ///
    /// Basış ve bırakma da çentiği **tazeliyor** (sıfırlamıyor) — ama yalnız
    /// raporlandıklarında ([`BateriView::report_button`]): ölçüt "işaretçi
    /// burada görüldü" değil **"burası uygulamaya bildirildi"**. `Select` ve
    /// `Ignored` kollarında hiçbir şey gitmedi ve damgalamak o hücredeki ilk
    /// hover raporunu sessizce yutardı. Tazelemenin kendisi şart: çentik
    /// bayat kalsaydı basışın hücresi ikinci kez, bu kez hareket olarak
    /// raporlanırdı.
    motion_cell: Cell<Option<(u16, u16)>>,
    /// **Metin yığını bu olayı aldı mı** — `keyDown:`'ın yeniden giriş
    /// bayrağı. `interpretKeyEvents:` çağrılmadan önce `false`'a çekilir;
    /// `insertText:` **ve** `setMarkedText:` onu `true` yapar, `keyDown:`
    /// dönüşte okur ve `false` ise olayı [`crate::keys::encode_key`]'e düşürür.
    ///
    /// Değişmez **"yığın olayı aldı"**, "metin geldi" değil — adı bu yüzden
    /// `consumed`. Ölü tuşun ilk vuruşunda (`Option+ü`) `characters` boş
    /// olduğu için fallback bugün tesadüfen zararsız; bayrağı yalnız
    /// `insertText:` set etseydi değişmez o tesadüfe yazılır ve boş olmayan
    /// bir bileşim başlangıcı tuşu iki kez gönderirdi.
    ///
    /// `Cell`, ivar: `interpretKeyEvents:` bizi **yeniden çağırıyor**, yani
    /// değer `keyDown:`'ın yığın çerçevesinde taşınamaz. Tek thread (ana
    /// thread) olduğu için paylaşılan durum değil — emsal yanındaki
    /// [`ViewIvars::dragging`].
    ///
    /// **Göremediği bir hâl var ve ölçülmedi:** bekleyen bir bileşimi
    /// yalnız `unmarkText` ile iptal eden tuş (ölü tuştan sonra Backspace ya
    /// da Esc) bayrağı kurmuyor, yani olay `encode_key`'e düşüyor ve PTY'ye
    /// `0x7f` gidiyor — kullanıcının **gerçekten** yazdığı bir harf silinir.
    /// Karşı hâl de ölçülmedi: `unmarkText`'i tüketme saymak, bileşimden
    /// sonraki ilk oku da yutardı (yığın onu `unmarkText` + `moveLeft:`
    /// olarak veriyor). İki yön de bir tuş turuyla ayrışıyor ve savunma o
    /// ölçümden **sonra** kurulur — bugün yazılacak kol, hangisinin gerçek
    /// olduğunu bilmeden yanlış yarıyı seçebilir.
    consumed: Cell<bool>,
    /// Bileşimin (marked text) **asgari** durumu: yığının henüz
    /// tamamlanmamış girdisi. Çizim **yok** — `bt-gpu`'nun altı çizili
    /// preedit yüzeyi bu sette doğmuyor; burada yalnız
    /// `hasMarkedText`/`markedRange`/`selectedRange`'in cevap verebileceği
    /// **durum** var.
    ///
    /// Boş dizge "bileşim yok" demek: `unmarkText` ve `insertText:` onu
    /// boşaltır. Stub bırakmak (her şeye "bileşim yok" demek) **ölçülmemiş**
    /// bir iddiaydı; alacritty ve ghostty ikisi de bir marked-text alanı
    /// tutuyor.
    marked_text: RefCell<String>,
    /// Tekerleğin satıra dönmemiş artığı ([`wheel_lines`]). Üç yerde sıfırlanır,
    /// üçünde de kalan artık bir sonraki kaydırmaya ait değil: yeni jestin
    /// başında (önceki jestin kırıntısı yeni jesti erken ya da geç tetiklemesin),
    /// tekerlek yoksayılınca (`Wheel::Ignored`: bir kipin artığı sonraki kipe
    /// taşınmasın) ve geçmişin ucuna dayanınca (uca doğru biriken momentum
    /// ters yöndeki ilk satırı geciktirmesin). Tekerlek uygulamaya gidince
    /// (`Wheel::Sent`) **korunur**: trackpad'le yavaş kaydırmada her olayın
    /// küsuratı düşseydi `less` sarsak kayardı.
    scroll_carry: Cell<f64>,
    /// Fare çevirisinin canlı girdileri: ölçü `bt-gpu`'dan, grid `bt-core`'un
    /// bildiği sayı. `OnceCell` değil `Cell<Option<…>>`, çünkü pencere boyu
    /// değişince tazeleniyor (`set_metrics`). Ayrı bir kopya gibi görünüyor
    /// ama değil: `start_session`'a ve `DisplayLink::resize`'a giden değerlerin
    /// aynısı, aynı çağrı yerinde yazılıyor.
    ///
    /// **Dikeyde `origin` ile aynı kareden gelmiyor** ve bu bilinen bir
    /// geçiş: bu üçlü pencere olayında (`set_metrics`), öteleme ise sıradaki
    /// **kare** yolunda tazeleniyor. Aradaki tek karede `rows` yeni, öteleme
    /// eski olur — ama ekranda duran kare de eski, yani `origin`'in eskiliği
    /// doğru olanı; ayrışan tek şey alt kenara yapılan tıklamanın kırpılma
    /// sınırı. Geometri ötelemeyi zaten snap'lediği için pencere bir karede
    /// kapanıyor. Dejenere boyut bu geçişi hiç doğurmuyor: oturum onu
    /// reddediyor (`Session::resize`) ve `point_to_cell` sıfır satır/sütunda
    /// `None` dönüyor, yani iki taraf da aynı yerde susuyor.
    metrics: Cell<Option<(CellMetrics, (u16, u16))>>,
    /// Çizilen karenin dikey orijini — kare yolunun yazdığı gövdenin okuma
    /// ucu ([`bt_gpu::Origin`]).
    ///
    /// `metrics`'in yanında ama onun **içinde değil**: o üçlü pencere
    /// olaylarında tazeleniyor (`set_metrics`), orijin ise kare başına
    /// değişiyor. İçine konsaydı fare, tabana yapışmayı bir sonraki yeniden
    /// boyutlandırmaya kadar görmezdi.
    ///
    /// `OnceCell`: link oturumla birlikte bir kez doğuyor ve gövdesi ondan
    /// sonra hiç değişmiyor — değişen şey gövdenin **içeriği** ve onu kare
    /// yolu yazıyor. Yokken (link kurulmadan önceki tek pencere) orijin
    /// sıfırdır ve çizim de tavana yapışıktır, yani ikisi tutarlı.
    origin: OnceCell<Origin>,
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

        /// Etkin görünüm değişti (sistemin açık/koyu ayarı): kararı app
        /// delegate'e **hedefsiz eylemle** iletir.
        ///
        /// View temayı bilmez ve delegate'e referans tutmaz — tek referansı
        /// oturum (`ViewIvars`). Hedefsiz eylem responder zincirinden geçip
        /// `appearanceDidChange:`'i tanımlayan app delegate'e varır; pencere
        /// key olmasa da (kullanıcı Sistem Ayarları'nda) zincirin sonu
        /// `NSApp` ve onun delegate'i.
        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn view_did_change_effective_appearance(&self) {
            // SAFETY: `NSView`'un kendi uygulaması argümansız ve dönüşsüz;
            // bir geçersiz kılma noktası, ama zinciri kırmamak için çağrılıyor.
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            let app = NSApplication::sharedApplication(self.mtm());
            // SAFETY: seçici geçerli; hedef `None` → responder zinciri. Alıcısı
            // `AppDelegate::appearance_did_change`, tek `Option<&AnyObject>`
            // argüman alıyor ve gönderene bakmıyor. Alıcı yoksa `false` döner
            // ve görünüm değişimi sessizce yok sayılır — doğru sonuç.
            let _ = unsafe {
                app.sendAction_to_from(sel!(appearanceDidChange:), None, Some(self.as_ref()))
            };
        }

        /// Edit ▸ Copy (Cmd-C): seçili metni genel panoya yazar. Seçim yoksa
        /// ya da boşsa pano el değmeden kalır (`clipboard::copy`).
        ///
        /// Menü öğesinin hedefi yok: eylem responder zincirinden first
        /// responder'a, yani buraya varıyor (`menu`). Metin `selection_text()`'ten
        /// — seçimin tek metin yolu.
        #[unsafe(method(copy:))]
        fn copy_selection(&self, _sender: Option<&AnyObject>) {
            if let Some(session) = self.ivars().session.get() {
                clipboard::copy(&NSPasteboard::generalPasteboard(), session.selection_text());
            }
        }

        /// Edit ▸ Paste (Cmd-V): panodaki metni oturuma yapıştırır.
        ///
        /// `paste()` yolundan girer: 2004 setse bracketed sarılır, değilse ham
        /// yazılır. Ham bayt `session.write`'a değmez. Panoda metin yoksa
        /// sessiz.
        #[unsafe(method(paste:))]
        fn paste_clipboard(&self, _sender: Option<&AnyObject>) {
            let Some(session) = self.ivars().session.get() else {
                return;
            };
            if let Some(text) = clipboard::read(&NSPasteboard::generalPasteboard()) {
                session.paste(text.into_bytes());
            }
        }

        /// View'ın y ekseni üstten: fare noktası grid yönünde gelir.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            // Fare y'si grid yönünde (üstten) gelsin: çeviride tersine çevirme
            // yok, `bounds.height` kesiği yok — pencere boyu değişince
            // kayan bir sabit değil tipin sözü.
            true
        }

        /// Sol tuş basıldı: jest uygulamanın mı terminalin mi, kararı
        /// `bt-core` veriyor ([`BateriView::button_event`]).
        ///
        /// `buttonNumber()` kapısı duruyor: AppKit bu selector'ı sol tuşa
        /// ayırıyor ve buraya düşen başka bir düğme **yanlış** düğmeyle
        /// raporlanırdı — sağ ve ortanın kendi selector'ı var. `super`'e
        /// geçilmiyor: varsayılan `NSView` davranışı seçimi bilmez ve olayı
        /// yutardı.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if event.buttonNumber() != 0 {
                return;
            }
            self.button_event(event, MouseButton::Left, true);
        }

        /// Sol tuş basılı sürükleme — **iki jestin tek selector'ı**. Basış
        /// raporlandıysa ([`ViewIvars::sent_buttons`]) hareket de rapor
        /// olarak gidiyor; yoksa aktif uç farenin şimdiki yerine taşınıyor,
        /// çapa `bt-core`'da (`Session::update_selection` yalnız bitişi
        /// taşır). Çizilen aralığı değiştirmeyen olaylar (aynı yarıda
        /// kalmak, hücre sınırını geçmek) oturumun aralık kapısında eleniyor
        /// — kare istenmez.
        ///
        /// Basışsız sürükleme yutulur: `mouseDown:`'sız `mouseDragged:` olmaz
        /// ama AppKit'in sözüne güvenilmez — olsaydı önceki seçimin ucunu
        /// taşırdı.
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.drag_event(event, MouseButton::Left);
        }

        #[unsafe(method(rightMouseDragged:))]
        fn right_mouse_dragged(&self, event: &NSEvent) {
            self.drag_event(event, MouseButton::Right);
        }

        #[unsafe(method(otherMouseDragged:))]
        fn other_mouse_dragged(&self, event: &NSEvent) {
            if event.buttonNumber() != 2 {
                return;
            }
            self.drag_event(event, MouseButton::Middle);
        }

        /// Düğmesiz hareket. Pencere `setAcceptsMouseMovedEvents:` ile
        /// açıldığı için **her pencerede** geliyor, kip açık olmasa da:
        /// olayın bedeli bir koordinat aritmetiği ve hücre değişmediyse
        /// `bt-core` hiç çağrılmıyor ([`BateriView::motion_event`]). Kipe
        /// göre açmak kipi `bt-shell`'e yayınlamayı, yani yeni bir paylaşılan
        /// durumu isterdi (`.tasks/020-fare-raporlama/discussion.md` →
        /// Karar 4); belirti görülürse o kola dönülür.
        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            self.motion_event(event, None);
        }

        /// Sol tuş bırakıldı: basış raporlandıysa bırakma da raporlanır
        /// (R6), yoksa sürükleme biter ve seçim ekranda kalır (Cmd-C onu
        /// kopyalar).
        ///
        /// `buttonNumber()` kapısı burada **yok** ve asimetri bilerek: kapı
        /// olsaydı beklenmedik bir düğme numarası `dragging`'i bayat `true`
        /// bırakır, sonraki her kaydırma eski seçimi sessizce uzatırdı
        /// ([`BateriView::follow_pointer`]'ın kapattığı hâlin aynısı).
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            self.button_event(event, MouseButton::Left, false);
        }

        /// Sağ tuş — bugün yalnız rapor yolu var: fare kipi kapalıyken sağ
        /// tık hiçbir şey yapmıyor (bağlam menüsü yok, seçim de başlatmıyor:
        /// beklenmedik bir vurgu üretirdi).
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            self.button_event(event, MouseButton::Right, true);
        }

        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, event: &NSEvent) {
            self.button_event(event, MouseButton::Right, false);
        }

        /// Orta tuş ve **ötesi**: AppKit dördüncü düğmeden sonrasını da bu
        /// selector'a yolluyor, X10'un iki biti ise yalnız üç düğme taşıyor
        /// ve `3` bırakmaya ayrılmış. Numara 2 değilse olay düşüyor — orta
        /// tuş diye raporlamak uygulamaya **yanlış** bir düğme söylerdi.
        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) {
            if event.buttonNumber() != 2 {
                return;
            }
            self.button_event(event, MouseButton::Middle, true);
        }

        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, event: &NSEvent) {
            if event.buttonNumber() != 2 {
                return;
            }
            self.button_event(event, MouseButton::Middle, false);
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
            //
            // **Doldurma reddi burada geçerli değil** ve sıfır bilerek
            // geçiliyor: buradaki nokta bir seçim ucu değil rapora giden
            // koordinat ve reddedilseydi bu `else` kaydırmanın **tamamını**
            // düşürürdü — band ekrandayken işaretçiyi oraya götüren kullanıcı
            // hiç kaydıramazdı. Bandın üstündeki nokta rapora bugünkü gibi 0.
            // satır olarak giriyor: uygulama doldurmayı zaten bilmiyor, o bir
            // terminal çizimi.
            let Some(pointer) =
                self.window_point_cell(event.locationInWindow(), OutOfGrid::Clamp { fill_rows: 0 })
            else {
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

        /// Tuş vuruşunun **arbitrajı** — dört kol, ve sırası sözleşme.
        ///
        /// İlk üç kol AppKit'in metin yığınına (`interpretKeyEvents:`)
        /// **girmez** ve girmemeleri ayrı ayrı gerekçeli:
        ///
        /// 1. **Cmd'li olay** yutulur (`reaches_terminal`); **tek istisna**
        ///    kapalı izin listesinde (⌘⌫ → `\x15`) ve o da yığına
        ///    **girmiyor**, doğrudan [`encode_key`]'e gidiyor. Yığına
        ///    girseydi ⌘⌫ orada `deleteToBeginningOfLine:` olur, ⌘T de
        ///    `insertText:`'e varıp kabuğa `t` yazardı; izin listesi o
        ///    tuşları hiç görmezdi.
        /// 2. **Shift+PgUp/PgDn** terminalin kaydırmasıdır
        ///    ([`page_scroll`]). Kol Control'ünkinden **önce**, çünkü
        ///    `page_scroll` Shift dışındaki değiştiricileri sormuyor —
        ///    Ctrl'lü Shift+PgUp da bugün kaydırıyor ve sıra ters olsaydı
        ///    o tuş `\e[5~`'e düşerdi.
        /// 3. **Control'lü olay** doğrudan [`encode_key`]'e gider. AppKit'in
        ///    kolunu seçmesine bırakılamaz: numpad Enter'ın `characters`'ı
        ///    U+0003 (Ctrl-C'nin baytı) ve Ctrl-Y'ninki U+0019'u Shift+Tab ile
        ///    paylaşıyor — yığın yanlış kolu seçerse her komut kesilir.
        ///    Yan kazanç: Ctrl+Shift+Tab ve Ctrl+numpad Enter borçları bugünkü
        ///    hâllerinde kalıyor. **Bedeli adıyla:** bekleyen bir bileşim bu
        ///    koldan yıkılmıyor — Option+ü'den sonra `^C`, ardından `a`
        ///    yazmak `ã` üretebilir, çünkü yığın hâlâ ölü tuşu bekliyor.
        ///    Ölçülmedi ve savunma kurulmadı: kolu yığına sokmak numpad
        ///    Enter'ın U+0003'ünü AppKit'in seçimine bırakırdı, yani takas
        ///    "her komut kesilebilir"e karşı "seyrek bir aksan".
        /// 4. **Kalanı** yığına verilir; yığın olayı almadıysa
        ///    ([`ViewIvars::consumed`]) yine `encode_key`'e düşer.
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let flags = event.modifierFlags();
            let Some(session) = self.ivars().session.get() else {
                return;
            };
            // `characters` modifier'lar uygulanmış hâli verir (Option-basılı
            // "ø", Ctrl-C → U+0003); ham tuş kodu `charactersIgnoringModifiers`
            // olurdu ve klavye düzenini bizim yeniden uygulamamızı isterdi.
            // Yokluğu (saf modifier tuşu) aşağıdaki iki kolu da susturuyor ama
            // **yığını susturmuyor**: bileşimin ilk vuruşunda `characters` boş
            // gelir ve ölü tuş tam oradan başlar.
            //
            // Cmd kolundan **önce** okunuyor: izin listesinin ölçütü artık
            // tuşun kimliği, bayrakları değil.
            let chars = event.characters().map(|c| c.to_string());
            // Command basılıyken tuş bir kısayoldur, girdi değil. Menü onu
            // `performKeyEquivalent:` ile önce yakalıyor (Cmd-C/V/Q/,, Cmd +/−/0);
            // yakalamadığı buraya varır ve **yutulur** (`reaches_terminal`) —
            // izin listesindeki tek tuş (⌘⌫) dışında.
            if !reaches_terminal(flags, chars.as_deref()) {
                return;
            }
            let command = flags.contains(NSEventModifierFlags::Command);
            // Shift+PgUp/PgDn terminalin kaydırmasıdır, uygulamanın tuşu değil —
            // ama yalnız oturum kabul ederse. Alternate screen'de kaydırma
            // reddedilir (`None`) ve tuş aşağıdaki yoldan uygulamaya düz PgUp
            // olarak gider: less/vim'de Shift+PgUp da sayfa çevirir, yutulmaz.
            // Sayfanın kaç satır olduğu `bt-core`'un kararı (`scroll_page`).
            let shift = flags.contains(NSEventModifierFlags::Shift);
            if let Some(chars) = chars.as_deref()
                && let Some(pages) = page_scroll(chars, shift)
                && let Some(moved) = session.scroll_page(pages)
            {
                if moved != 0 {
                    self.follow_pointer(session);
                }
                return;
            }
            let ctrl = flags.contains(NSEventModifierFlags::Control);
            // `!command` R4.2'nin **uygulandığı** yer: izin listesinden geçen
            // ⌘⌫ de yığına girmiyor. Girseydi yığın onu
            // `deleteToBeginningOfLine:`e çevirir, `doCommandBySelector:`
            // sessizce yutar ve aşağıdaki kol `\x15`'i hiç göremezdi.
            if !ctrl && !command {
                // Metin yığını: ölü tuş durumunu o tutuyor ve bileşim
                // tamamlanınca metni `insertText:` ile geri veriyor. Bayrak
                // çağrıdan **önce** iniyor; yığın bizi yeniden çağırdığı için
                // cevabı ivar taşıyor, `keyDown:`'ın yığın çerçevesi değil.
                self.ivars().consumed.set(false);
                // Tek olaylık dizi: yığın onu senkron tüketiyor ve
                // aşağıdaki okuma dönüşten sonra geçerli.
                self.interpretKeyEvents(&NSArray::from_slice(&[event]));
                if self.ivars().consumed.get() {
                    return;
                }
            }
            // Yığının almadığı (ya da hiç uğramadığı) olay: fonksiyon tuşları,
            // Enter/Tab/Esc/Backspace, Control'lü harfler, Option'lı
            // gezinme/silme (yığın onları `doCommandBySelector:`'a veriyor ve
            // o metot sessiz no-op) ve izin listesinden geçen ⌘⌫.
            //
            // `super`'e geçmiyoruz: `NSResponder::keyDown:` tanımadığı tuşta
            // beep çalar ve terminalde her ok tuşu bip sesi olurdu.
            let Some(chars) = chars else {
                return;
            };
            let key = KeyPress {
                chars: &chars,
                ctrl,
                option: flags.contains(NSEventModifierFlags::Option),
                command,
            };
            match encode_key(key) {
                Some(KeyInput::Bytes(bytes)) => session.write(&bytes),
                // Okun baytı DECCKM'e bağlı, kip `bt-core`'da.
                Some(KeyInput::Arrow(arrow)) => session.write_arrow(arrow),
                None => {}
            }
        }
    }

    /// AppKit'in metin yığınının bu view'a bakan yüzü. Protokolün **11
    /// zorunlu** metodu da burada: `objc2-app-kit` hiçbirini `#[optional]`
    /// işaretlemiyor ve eksik kalanı `define_class!`'ın debug assertion'ında
    /// panikliyor — kısmi uyum bir seçenek değil.
    ///
    /// Üçü bileşim durumunu **yazıyor** (`insertText:`, `setMarkedText:`,
    /// `unmarkText`), üçü onu **okuyor** (`selectedRange`, `markedRange`,
    /// `hasMarkedText`); `doCommandBySelector:` bilerek boş ve kalan dördü
    /// sabit cevap veriyor — her biri kendi "neden"iyle.
    unsafe impl NSTextInputClient for BateriView {
        /// Bileşim tamamlandı (ya da düz bir harf geldi): metin PTY'ye gider.
        ///
        /// Argüman `&AnyObject` — yığın `NSString` **ya da**
        /// `NSAttributedString` gönderebiliyor. **Tek** çözme kuralı:
        /// `NSString`'e downcast, olmazsa `NSAttributedString::string()`;
        /// ikisi de değilse olay **tüketilmiş sayılmıyor** ve `keyDown:`
        /// onu `encode_key`'e düşürüyor — tanımadığımız bir tipi sessizce
        /// yutmak tuşu büsbütün kaybettirirdi.
        ///
        /// `replacement_range` yoksayılıyor: yığının düzenleyebileceği bir
        /// belgemiz yok, yazılan şey doğrudan PTY'ye akıyor ve satırın
        /// sahibi kabuk. **Bilinen sonucu var**: aksan popover'ı bir harf
        /// seçtirdiğinde çağrı `insertText:"é" replacementRange:{n-1,1}`
        /// oluyor, yani "son harfi bununla değiştir"; biz aralığı
        /// atladığımız için kabuğa `eé` gider. Popover kapalı
        /// (`app::disable_press_and_hold`) ve bu yüzden yol bugün ölü; o
        /// bastırma tutmazsa belirtinin **sessiz yarısı** budur (gürültülü
        /// yarısı basılı tuşun yinelememesi).
        #[unsafe(method(insertText:replacementRange:))]
        fn insert_text(&self, string: &AnyObject, _replacement_range: NSRange) {
            // Bileşim **çözme kuralından önce** siliniyor: tanımadığımız bir
            // tip gelse bile yığın o bileşimi bitirmiş oluyor ve durum
            // orada kalsaydı `hasMarkedText` sonsuza kadar `true` derdi.
            self.ivars().marked_text.borrow_mut().clear();
            let Some(text) = resolve_text(string) else {
                return;
            };
            // Bayrak **oturumdan önce**: değişmez "yığın bu olayı aldı", "bayt
            // yazıldı" değil. Oturum henüz bağlanmadıysa tuş kaybolur ama
            // `encode_key` onu ikinci kez göndermez.
            self.ivars().consumed.set(true);
            if let Some(session) = self.ivars().session.get() {
                session.write(text.as_bytes());
            }
        }

        /// Yığının tanıdığı bir düzenleme komutu (Enter → `insertNewline:`,
        /// Tab → `insertTab:`, Esc → `cancelOperation:`, `^A` →
        /// `moveToBeginningOfParagraph:`…): **sessiz no-op**.
        ///
        /// Metot gövdesiz kalamaz, boş da olsa: yoksa `NSResponder`'ın
        /// varsayılanı koşar ve tanımadığı seçicide **bip çalar** —
        /// `keyDown:`'da `super`'e geçmeme gerekçesinin aynısı, yeni kapıdan.
        /// Bayrak set edilmiyor, yani olay `encode_key`'e düşüyor ve baytı
        /// bugünkü yerden geliyor.
        #[unsafe(method(doCommandBySelector:))]
        fn do_command_by_selector(&self, _selector: Sel) {}

        /// Bileşim sürüyor (ölü tuş basıldı, henüz tamamlanmadı): durum
        /// güncellenir. **Çizim yok** — altı çizili preedit yüzeyi bu sette
        /// doğmuyor.
        ///
        /// Bayrağı bu metot da set ediyor ([`ViewIvars::consumed`]): değişmez
        /// "yığın olayı aldı", "metin geldi" değil.
        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        fn set_marked_text(
            &self,
            string: &AnyObject,
            _selected_range: NSRange,
            _replacement_range: NSRange,
        ) {
            let Some(text) = resolve_text(string) else {
                return;
            };
            self.ivars().consumed.set(true);
            *self.ivars().marked_text.borrow_mut() = text;
        }

        /// Bileşim iptal edildi ya da tamamlandı. Bayrak **set edilmiyor**:
        /// yığın bunu `keyDown:` dışından da (odak kaybı, fare) çağırıyor ve
        /// o çağrı bir tuş olayını tüketmiş sayılmaz.
        ///
        /// **Sözleşmeden bilinçli sapma:** Apple "işaretli metni normal
        /// yazılmış gibi kabul et" diyor, biz **atıyoruz**. Sebebi bizde
        /// geri alınacak bir belge olmaması: `insertText:` baytı doğrudan
        /// PTY'ye akıtıyor ve kabuk onu satırına almış oluyor, yani
        /// "kabul etmek" bekleyen aksanı kullanıcının hiç istemediği bir yere
        /// yazmak demek. Bedeli adıyla duruyor — bileşim ortasında pencereye
        /// tıklamak bekleyen `~`'yi sessizce düşürür; alacritty ve ghostty de
        /// aynı yerde aynı şeyi yapıyor.
        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            self.ivars().marked_text.borrow_mut().clear();
        }

        /// Seçim aralığı. Modelimiz tek cümle: **belge = bileşim metni,
        /// imleç sonunda**. Terminalin ızgarasındaki seçim (fareyle yapılan)
        /// bu soruya girmiyor — o `bt-core`'un seçimi ve yığının
        /// düzenleyebileceği bir metin değil.
        #[unsafe(method(selectedRange))]
        fn selected_range(&self) -> NSRange {
            NSRange::new(self.marked_utf16_len(), 0)
        }

        /// İşaretli aralık; bileşim yoksa `NSNotFound` — "işaretli bir şey
        /// yok"un sözleşmedeki karşılığı, sıfır uzunluklu bir aralık değil.
        #[unsafe(method(markedRange))]
        fn marked_range(&self) -> NSRange {
            match self.marked_utf16_len() {
                0 => EMPTY_RANGE,
                len => NSRange::new(0, len),
            }
        }

        #[unsafe(method(hasMarkedText))]
        fn has_marked_text(&self) -> bool {
            !self.ivars().marked_text.borrow().is_empty()
        }

        /// Yığının geri okuyabileceği bir belge **yok**: yazılan her şey
        /// PTY'ye akıyor ve ızgaranın içeriği `bt-core`'un, metin
        /// yığınının değil. `None` = "bu aralıkta metnim yok".
        ///
        /// `actual_range` yazılmıyor: hiçbir aralık döndürmediğimiz için
        /// doldurulacak bir gerçek aralık da yok (Apple'ın sözleşmesi).
        #[unsafe(method_id(attributedSubstringForProposedRange:actualRange:))]
        fn attributed_substring(
            &self,
            _range: NSRange,
            _actual_range: NSRangePointer,
        ) -> Option<Retained<NSAttributedString>> {
            None
        }

        /// İşaretli metnin taşıyabileceği öznitelikler: **hiçbiri**. Boş
        /// dizi "altını çizme, renklendirme, ruby — hiçbirini uygulayamam"
        /// demek ve preedit'i çizmediğimiz için doğrusu bu.
        #[unsafe(method_id(validAttributesForMarkedText))]
        fn valid_attributes_for_marked_text(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            NSArray::new()
        }

        /// Bileşim yüzeyinin (aksan popover'ı, aday penceresi) ekranda
        /// konumlanacağı dikdörtgen — **ekran koordinatında**.
        ///
        /// Cevap view'ın kendi dikdörtgeni, hücre hassasiyetinde değil ve
        /// bu **bilinçli**: kapsam içinde tüketicisi yok (ölü tuş önizlemesi
        /// popover değil marked text, aday penceresi de CJK'nın, yani tam
        /// IME borcunun). İmleç hücresini crate sınırı ötesinden taşımak
        /// (`bt_gpu::Origin` emsali) o iş geldiğinde ilk adım olur.
        /// Yaklaşımın yönü yine de doğru: içerik pencerenin **tabanına**
        /// yaslanıyor, yani imleç view dikdörtgeninin sol alt köşesinin
        /// yakınında ve yüzey oradan açılıyor.
        ///
        /// Sıfır dikdörtgen dönmemenin sebebi duruyor: yüzey o zaman ekranın
        /// köşesinde belirirdi. Penceresi olmayan view'da (henüz takılmamış)
        /// çevirecek bir uzay yok, cevap sıfır.
        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        fn first_rect_for_character_range(
            &self,
            range: NSRange,
            actual_range: NSRangePointer,
        ) -> NSRect {
            // Sorulan aralığın tamamını karşıladığımızı söylüyoruz: tek bir
            // dikdörtgen dönüyoruz ve o dikdörtgen aralığın tamamına ait.
            // SAFETY: işaretçi ya null ya da çağıranın yığınındaki geçerli
            // bir `NSRange`; AppKit'in sözleşmesi bu.
            unsafe {
                if let Some(actual) = actual_range.as_mut() {
                    *actual = range;
                }
            }
            let Some(window) = self.window() else {
                return NSRect::ZERO;
            };
            window.convertRectToScreen(self.convertRect_toView(self.bounds(), None))
        }

        /// Ekrandaki bir noktanın hangi karaktere denk geldiği: **cevabımız
        /// yok**. Yığın bunu sürükleyerek metin seçmek için soruyor ve
        /// ızgaranın seçimi bizim kendi yolumuz (`mouseDragged:`), yığının
        /// değil. `NSNotFound` sözleşmedeki "bu noktada karakterim yok".
        #[unsafe(method(characterIndexForPoint:))]
        fn character_index_for_point(&self, _point: NSPoint) -> NSUInteger {
            NOT_FOUND
        }
    }

    /// Finder'dan gelen damlanın bu view'a bakan yüzü. Protokolün **bütün**
    /// metotları `#[optional]` — `NSTextInputClient`'ın tam tersi — yani iki
    /// tanesi yetiyor: damlanın kabul edildiğini söyleyen ve onu yazan.
    ///
    /// `prepareForDragOperation:` bilerek yok: uygulanmayan metotta AppKit
    /// "evet" varsayıp doğrudan `performDragOperation:`e geçiyor, yani
    /// yazılacak gövde sabit bir `true` olurdu.
    unsafe impl NSDraggingDestination for BateriView {
        /// İşaretçi damlayla pencereye girdi: cevap **kopya**.
        ///
        /// Koşulsuz, çünkü eleme kayıtta yapıldı
        /// ([`BateriView::new`]'daki `registerForDraggedTypes`): bu metot
        /// ancak panoda bir dosya URL'si varsa çağrılıyor ve ikinci bir
        /// eleme aynı soruyu iki kez sormak olurdu.
        ///
        /// **Kopya**, taşıma değil: Finder'daki dosya yerinde kalmalı, biz
        /// yalnız yolunu yazıyoruz. `draggingUpdated:` de uygulanmıyor —
        /// AppKit onu uygulamayan hedefte buradaki cevabı sürdürüyor, yani
        /// ikinci metot aynı sabiti tekrarlardı.
        ///
        /// **Oturum sorulmuyor ve asimetri bilerek duruyor:** aşağıdaki
        /// `performDragOperation:` oturum bağlı değilken `false` dönüyor, yani
        /// imleç "+" gösterip damla "poof" ile geri dönebilir. Burada da
        /// sormak iki cevabı eşitlerdi ama ölçüt yanlış olurdu — bu metot
        /// sürüklemenin **başında** koşuyor ve oturum o an yoksa damla
        /// bırakılana kadar doğmuş olabilir. Pencerede oturumun yokluğu zaten
        /// erişilemez ([`ViewIvars::session`]: view ile oturum arasına run
        /// loop dönmediği için hiçbir olay düşemiyor), yani asimetrinin
        /// görülebileceği bir kare yok; adı yine de burada dursun.
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(
            &self,
            _sender: &ProtocolObject<dyn NSDraggingInfo>,
        ) -> NSDragOperation {
            NSDragOperation::Copy
        }

        /// Damla bırakıldı: yollar kaçırılıp giriş satırına yazılır.
        ///
        /// Çıkış [`Session::paste`] — `session.write` **değil**: bracketed
        /// paste sarması ve dock istisnası oradan bedavaya geliyor
        /// (018 Karar 4). Dock satırın sahibiyken tek dosyalık damla dock'a
        /// "yazılmış gibi" giriyor (`Session::can_be_typed`; ters bölü bir
        /// kontrol karakteri değil, ham daldan sorunsuz geçiyor) ve bu
        /// **doğru** davranış: kullanıcı damlayı yazdığı satırın devamı
        /// olarak görüyor.
        ///
        /// `false`'ın iki sebebi var ve ikisi de "yazacak bir şey yok":
        /// oturum henüz bağlanmamış, ya da damlada okunabilen yol çıkmamış.
        /// AppKit bunu damlanın reddi olarak gösteriyor — sessizce `true`
        /// demek kullanıcıya hiçbir şey olmamışken olmuş gibi gösterirdi.
        ///
        /// Gövdede erken `return` **yok** ve olamaz: `define_class!` cevabı
        /// ObjC'nin `BOOL`'una çeviriyor ve çeviri yalnız **kuyruk
        /// ifadesine** uygulanıyor, yani bir `return false` dış imzayla
        /// çelişip derlemeyi kırardı.
        #[unsafe(method(performDragOperation:))]
        fn perform_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let line = shell_quote(&dropped_paths(&sender.draggingPasteboard()));
            match self.ivars().session.get() {
                Some(session) if !line.is_empty() => {
                    session.paste(line.into_bytes());
                    true
                }
                _ => false,
            }
        }
    }
);

/// Panodaki dosya URL'lerinin dosya sistemi yolları.
///
/// Okuma API'si **seçili**: `readObjectsForClasses:options:` + `NSURL`
/// sınıfı. `pasteboardItems()` aynı işi görürdü ama `NSPasteboardItem`
/// feature'ını isterdi ve bize kalan iş yine öğeyi URL'ye çözmek olurdu.
///
/// Yol `NSURL.path`'ten alınıyor: **yüzde çözme ikinci kez yazılmıyor.**
/// `bt-core`'un kendi çözücüsü OSC 7 için var ve orada kalıyor (katman
/// düzeni); burada Foundation'ın kendi cevabı okunuyor.
///
/// Çözülemeyen öğe **sessizce düşüyor**: damlanın bir parçasını anlamamak
/// tamamını düşürmek için sebep değil. Eleme üç kademeli ve ortadaki şart —
/// `NSURL`'e çözülemeyen, **`isFileURL` demeyeni** ve yol vermeyen.
///
/// Ortadaki kademe set kapısında eklendi (018): `NSURL` sınıfı `http://`'yi
/// de okur ve `NSURL.path` ona `/foo` cevabını verir, yani web adresi
/// damlatan kullanıcı giriş satırında kökten bir yol bulurdu. Karar 4 "yalnız
/// dosya URL'si" diyor ve `plan.md` metin/URL damlasını kapsam dışında
/// tutuyor; kayıt doğruydu, kod eksikti.
fn dropped_paths(board: &NSPasteboard) -> Vec<String> {
    let classes: Retained<NSArray<AnyClass>> = NSArray::from_slice(&[NSURL::class()]);
    // SAFETY: imzanın iki koşulu da sağlanıyor — sınıf dizisi gerçek bir
    // sınıf (`NSURL`) taşıyor ve seçenek sözlüğü verilmiyor (`None`).
    let Some(objects) = (unsafe { board.readObjectsForClasses_options(&classes, None) }) else {
        return Vec::new();
    };
    objects
        .iter()
        .filter_map(|object| {
            object
                .downcast_ref::<NSURL>()
                .filter(|url| url.isFileURL())
                .and_then(NSURL::path)
                .map(|path| path.to_string())
        })
        .collect()
}

/// `NSNotFound`'un `NSRange` alanlarındaki tipi. Sabit `NSInteger` olarak
/// geliyor, aralıkların iki alanı ise `NSUInteger`; dönüşüm tek yerde dursun.
const NOT_FOUND: NSUInteger = NSNotFound as NSUInteger;

/// "İşaretli bir şey yok" — `markedRange`'in bileşimsiz cevabı.
const EMPTY_RANGE: NSRange = NSRange::new(NOT_FOUND, 0);

/// Yığının verdiği metin nesnesini dizgeye indirger — **tek** çözme kuralı
/// ([`NSTextInputClient::insertText_replacementRange`] ve
/// `setMarkedText:` aynı soruyu soruyor).
///
/// Argümanın tipi belgede "doğru tipte olmalı" diye geçiyor ve pratikte iki
/// tip geliyor: düz `NSString` (çoğu yol) ve `NSAttributedString` (işaretli
/// metin, aday penceresi). `None` = ikisi de değil; çağıran o olayı
/// tüketilmiş saymıyor ve `encode_key`'e düşürüyor.
fn resolve_text(string: &AnyObject) -> Option<String> {
    if let Some(text) = string.downcast_ref::<NSString>() {
        return Some(text.to_string());
    }
    string
        .downcast_ref::<NSAttributedString>()
        .map(|text| text.string().to_string())
}

impl BateriView {
    pub(crate) fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ViewIvars {
            session: OnceCell::new(),
            dragging: Cell::new(false),
            sent_buttons: Cell::new(0),
            motion_cell: Cell::new(None),
            consumed: Cell::new(false),
            marked_text: RefCell::new(String::new()),
            scroll_carry: Cell::new(0.0),
            metrics: Cell::new(None),
            origin: OnceCell::new(),
        });
        // SAFETY: `initWithFrame:` NSView'un tasarlanmış kurucusu ve ivar'lar
        // set edildi.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        // Sürükleme hedefi olmanın tek şartı: view hangi tipleri kabul
        // ettiğini **önceden** söylemeli, yoksa `draggingEntered:` hiç
        // çağrılmaz. Liste tek tipli — düz metin damlası kapsam dışı ve
        // kaçış kuralı bu yüzden tipe koşullu değil (018 Karar 4).
        //
        // SAFETY: `unsafe` blok yalnız `NSPasteboardTypeFileURL` **statik**
        // erişimi için (`clipboard` emsali); gerçek bir pasteboard tipi
        // kaydı ve `None`'a çözümlenmiyor.
        this.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]));
        this
    }

    /// Bileşim metninin **UTF-16 kod birimi** sayısı — `NSRange`'in birimi o.
    ///
    /// Bayt değil: `ü` bir kod birimi ama iki bayt, ve ölü tuş bileşimi tam
    /// olarak o harflerde yaşıyor. `String::len()` yazılsaydı yığın bileşimin
    /// boyunu olduğundan uzun görürdü.
    fn marked_utf16_len(&self) -> usize {
        self.ivars().marked_text.borrow().encode_utf16().count()
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

    /// Fare çevirisinin dikey orijinini bağlar; link doğduktan hemen sonra,
    /// bir kez.
    ///
    /// `set_metrics`'ten ayrı çağrı, çünkü kaynağı ayrı: o üçlü pencere
    /// geometrisinden, bu link'ten geliyor ve link `set_metrics`'ten sonra
    /// kuruluyor (`app::start_session`). İkinci çağrı sessizce düşseydi fare
    /// eski gövdeyi, yani sonsuza kadar sıfır bir orijin okurdu.
    pub(crate) fn attach_origin(&self, origin: Origin) {
        assert!(
            self.ivars().origin.set(origin).is_ok(),
            "orijin ikinci kez bağlandı"
        );
    }

    /// Fare düğmesinin **altı** selector'ının ortak gövdesi: basış ya da
    /// bırakma, üç düğme.
    ///
    /// Kararı `bt-core` veriyor ([`Session::mouse_button`]) — kip burada
    /// tutulmuyor ve sorulmuyor. Burası yalnız AppKit çevirisi: hücre,
    /// değiştiriciler ve cevabın üç kolu.
    ///
    /// **Basış ile bırakma farklı hücre kapısından geçiyor** ve bu bir
    /// tutarsızlık değil, [`OutOfGrid`]'in tek kuralının iki yüzü. Basış bir
    /// jest *başlatıyor*: ızgaranın dışına düşen nokta reddediliyor, yani
    /// başlık çubuğu, sol pay, dock bandı ve doldurma bandı üstündeki basış
    /// ne rapor ne seçim üretiyor (R8 bunun özel hâli). Bırakma başlamış bir
    /// jesti *bitiriyor*: nokta kırpılıyor, çünkü düşürülen bırakma
    /// uygulamada **takılı kalmış bir düğme** bırakırdı (R6).
    ///
    /// Bırakmada `Clamp`'in `fill_rows`'u sıfır geçiyor: bandın üstü de bir
    /// hücre vermeli, kırpmayı `bt-core` yapıyor.
    fn button_event(&self, event: &NSEvent, button: MouseButton, pressed: bool) {
        let Some(session) = self.ivars().session.get() else {
            return;
        };
        let bit = button_bit(button);
        let sent = &self.ivars().sent_buttons;
        if !pressed {
            if sent.get() & bit == 0 {
                // Rapor edilmemiş basışın bırakması: seçim jestinin sonu.
                if button == MouseButton::Left {
                    self.ivars().dragging.set(false);
                }
                return;
            }
            sent.set(sent.get() & !bit);
            let clamp = OutOfGrid::Clamp { fill_rows: 0 };
            if let Some(cell) = self.window_point_cell(event.locationInWindow(), clamp) {
                self.report_button(session, button, false, cell, event);
            }
            return;
        }
        // Yeni basış yeni jest: kayıp bir `mouseUp:`'ın (sürüklemenin
        // ortasında bir modal, bir sistem jesti) bıraktığı **bayat** bit
        // burada iniyor — [`BateriView::follow_pointer`]'ın `dragging` için
        // yaptığının basıştaki eşi. İnmeseydi kip bu arada kapandığında
        // basış `Select` olur, bırakma bayat biti bulup rapor yolunu seçer ve
        // `dragging`'i hiç düşürmezdi: sonraki her kaydırma eski seçimi
        // sessizce uzatırdı.
        sent.set(sent.get() & !bit);
        let Some(cell) = self.window_point_cell(event.locationInWindow(), OutOfGrid::Reject) else {
            return;
        };
        match self.report_button(session, button, true, cell, event) {
            // Jest uygulamanın: `dragging` **kurulmuyor**, yoksa
            // `mouseDragged:` var olan eski seçimin ucunu büyütürdü.
            Click::Sent => sent.set(sent.get() | bit),
            // Jest terminalin — ama seçimi yalnız sol tuş başlatır: sağ ya da
            // orta tık beklenmedik bir vurgu üretirdi.
            Click::Select if button == MouseButton::Left => {
                self.ivars().dragging.set(true);
                // İmleç çapa hücresinden sürüklenir: ters yöne ilk hareket
                // seçimi boşaltmamalı, fare ucundan büyümeli. İki uç **aynı**
                // olduğu sürece seçim boştur — yani sürüklemesiz tık hiçbir
                // şey seçmez ve Cmd-C panoya dokunmaz (`selection_text()`
                // `None`). Çapa **yarısıyla** gidiyor: basış hücrenin hangi
                // yarısındaysa sınır oradan geçer ve sürükleme boyunca orada
                // kalır.
                session.set_selection(cell, cell);
            }
            Click::Select | Click::Ignored => {}
        }
    }

    /// Basılı sürüklemenin ortak gövdesi: jest uygulamanınsa hareket raporu,
    /// terminalinse seçimin ucu.
    ///
    /// Rota basışta kilitlendi (R6) ve burada yeniden sorulmuyor: aynı jestin
    /// ortasında Shift'i bırakmak ya da uygulamanın kipi kapatması yolu
    /// değiştirmemeli. Kilidin iki yarısı da okunuyor — `sent_buttons`
    /// raporlanan basışı, `dragging` seçim başlatan basışı biliyor ve ikisi
    /// aynı anda kurulu olabilir (sol seçim sürerken sağ tuşa basmak).
    fn drag_event(&self, event: &NSEvent, button: MouseButton) {
        if self.ivars().sent_buttons.get() & button_bit(button) != 0 {
            self.motion_event(event, Some(button));
            return;
        }
        // Rapor edilmemiş sürükleme yalnız sol tuşun seçimi; sağ/orta tuşun
        // terminalde bir jesti yok.
        if button != MouseButton::Left || !self.ivars().dragging.get() {
            return;
        }
        if let Some((session, cell)) = self.session_cell(event) {
            session.update_selection(cell);
        }
    }

    /// Düğme raporunu gönderir ve **raporlandıysa** kısmanın çentiğini o
    /// hücreye damgalar: aynı hücrede gelecek ilk hareket ikinci bir rapor
    /// üretmesin. Ölçüt cevabın kendisi, çünkü `Select` ve `Ignored`
    /// kollarında uygulamaya hiçbir şey gitmedi ve damgalamak oradaki ilk
    /// hover raporunu sessizce yutardı.
    fn report_button(
        &self,
        session: &Session,
        button: MouseButton,
        pressed: bool,
        cell: SelectionPoint,
        event: &NSEvent,
    ) -> Click {
        let answer = session.mouse_button(button, pressed, cell, modifiers(event));
        if answer == Click::Sent {
            moved_to_new_cell(&self.ivars().motion_cell, cell);
        }
        answer
    }

    /// Kayıp bir `mouseUp:`'ın uygulamada basılı bıraktığı düğmeleri serbest
    /// bırakır — [`BateriView::follow_pointer`]'ın `dragging` için yaptığının
    /// uygulama tarafındaki eşi.
    ///
    /// **Kanıt selector'ın kendisi:** AppKit `mouseMoved:`'ı yalnız hiçbir
    /// düğme basılı değilken gönderiyor (basılıyken `*MouseDragged:` gelir),
    /// yani orada kurulu bir bit tek bir şey demek — bırakma olayı bu view'a
    /// hiç varmadı (sürüklemenin ortasında bir modal, Mission Control, bir
    /// sistem jesti). Biti sessizce düşürmek yetmez: uygulama düğmeyi
    /// **hâlâ basılı** sanar ve her hareket raporunda kendi seçimini
    /// büyütür, yani bırakmanın kendisi gönderilmek zorunda.
    ///
    /// Basıştaki bayat-bit temizliği bunun yerine geçmiyor: o, terminalin
    /// kendi defterini düzeltiyor ve ancak kullanıcı **aynı düğmeye yeniden
    /// bastığında** koşuyor.
    fn flush_lost_releases(&self, session: &Session, event: &NSEvent) {
        let sent = self.ivars().sent_buttons.replace(0);
        if sent == 0 {
            return;
        }
        // Jestin devamı, başlangıcı değil: koordinat kırpılıyor (R6).
        let clamp = OutOfGrid::Clamp { fill_rows: 0 };
        let Some(cell) = self.window_point_cell(event.locationInWindow(), clamp) else {
            return;
        };
        for button in [MouseButton::Left, MouseButton::Middle, MouseButton::Right] {
            if sent & button_bit(button) != 0 {
                self.report_button(session, button, false, cell, event);
            }
        }
    }

    /// Hareket raporunun tek yolu: düğmesiz (`mouseMoved:`) ve basılı
    /// (`*MouseDragged:`).
    ///
    /// **Kısma `bt-core` çağrısından önce** ([`ViewIvars::motion_cell`]):
    /// hücre değişmediyse `Term` kilidi hiç alınmıyor. Çentik rapor
    /// gitmese de yazılıyor — kip kapalıyken de hücre değişimi başına tek
    /// bir sonuçsuz çağrı kalsın, piksel başına değil.
    ///
    /// Hücrenin kapısı düğmeye bağlı ([`OutOfGrid`]): basılı sürükleme
    /// başlamış bir jestin devamı ve kırpılıyor, düğmesiz hareket ise bir
    /// yer *söylüyor* ve ızgaranın dışında reddediliyor — başlık çubuğunda
    /// gezinen işaretçi uygulamaya 0. satırı bildirmemeli.
    fn motion_event(&self, event: &NSEvent, button: Option<MouseButton>) {
        let Some(session) = self.ivars().session.get() else {
            return;
        };
        let outside = if button.is_some() {
            OutOfGrid::Clamp { fill_rows: 0 }
        } else {
            self.flush_lost_releases(session, event);
            OutOfGrid::Reject
        };
        let Some(cell) = self.window_point_cell(event.locationInWindow(), outside) else {
            return;
        };
        if !moved_to_new_cell(&self.ivars().motion_cell, cell) {
            return;
        }
        session.mouse_motion(button, cell, modifiers(event));
    }

    /// Oturum + olayın altındaki uç (hücre ve yarısı). Üçü (`session`, ölçü,
    /// grid) birlikte yoksa `None`: yarım bilgiyle seçimin ucu taşınamaz.
    /// Doldurma bandının üstüne düşen nokta da `None`
    /// ([`point_to_cell`]).
    ///
    /// Bugün tek tüketicisi sürükleme; düğme olayları oturumu ve hücreyi
    /// ayrı ayrı istiyor ([`BateriView::button_event`]), çünkü bırakma
    /// hücreyi başka bir kapıdan (`fill_rows = 0`) alıyor.
    fn session_cell(&self, event: &NSEvent) -> Option<(Arc<Session>, SelectionPoint)> {
        let session = Arc::clone(self.ivars().session.get()?);
        let cell = self.event_cell(event)?;
        Some((session, cell))
    }

    /// Olay noktasını seçim ucuna indirir. `None` ölçü ya da pencere henüz
    /// yokken, grid sıfır boyutluyken ve doldurma bandının üstünde — kenar
    /// dışı nokta yapışır.
    fn event_cell(&self, event: &NSEvent) -> Option<SelectionPoint> {
        let fill_rows = self.fill_rows();
        self.window_point_cell(event.locationInWindow(), OutOfGrid::Clamp { fill_rows })
    }

    /// Pencere koordinatındaki noktayı seçim ucuna indirir — [`Self::event_cell`]'in
    /// olaysız hâli: tuşla kaydırmada farenin yerini taşıyan bir fare olayı yok.
    ///
    /// `outside` **argüman**, alan değil: aynı nokta çağıranına göre bir
    /// seçim ucu ya da rapora giden koordinat oluyor ve ızgaranın dışına
    /// düşünce ikisi ayrı şey istiyor ([`OutOfGrid`]).
    fn window_point_cell(&self, in_window: NSPoint, outside: OutOfGrid) -> Option<SelectionPoint> {
        let (metrics, (cols, rows)) = self.ivars().metrics.get()?;
        let point = self.convertPoint_fromView(in_window, None);
        let scale = self.window()?.backingScaleFactor();
        // Orijin **çizilen** karenin değeri: link yoksa (ilk pencere) sıfır ve
        // çizim de tavana yapışık, yani ikisi tutarlı.
        let origin_px = self.ivars().origin.get().map_or(0.0, Origin::px);
        point_to_cell(
            (point.x, point.y),
            metrics,
            f64::from(origin_px),
            outside,
            scale,
            cols,
            rows,
        )
    }

    /// Çizilen karenin doldurma bandının boyu — orijinle **aynı gövdeden**
    /// ([`bt_gpu::Origin`]), yani ikisi aynı kareye ait. Link yoksa sıfır:
    /// band da çizim de yok.
    fn fill_rows(&self) -> u16 {
        self.ivars().origin.get().map_or(0, Origin::fill_rows)
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
        // `None` gelirse uç **taşınmıyor**: fare doldurma bandının üstüne
        // çıktıysa seçim son geçerli hücresinde kalır, 0. satıra fırlamaz.
        let fill_rows = self.fill_rows();
        if let Some(cell) = self.window_point_cell(
            window.mouseLocationOutsideOfEventStream(),
            OutOfGrid::Clamp { fill_rows },
        ) {
            session.update_selection(cell);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::ns_string;

    /// Sahnelerin ızgara ölçüsü; pay **argüman**, çünkü sorulan iki ayrı şey
    /// var: hücre aritmetiği (pay sıfır) ve payın kendisi.
    fn grid(gutter: u16) -> CellMetrics {
        CellMetrics::new(9, 18, 9, gutter, 1).expect("sıfır olmayan hücre")
    }

    /// Testlerin ortak sahnesi: 100×33 grid, 9×18 hücre, @2x.
    /// View 450×297 nokta eder.
    ///
    /// **Sol pay bu sahnede sıfır** ve bu bilinçli: aşağıdaki sınamaların
    /// sorduğu şey hücre ile yarısının aritmetiği, ve beklenen x değerlerini
    /// pay kadar kaydırmak o gerekçeleri okunmaz hâle getirirdi. Payın kendi
    /// sınaması `the_gutter_shifts_the_grid_origin`.
    fn scene_point(view_px: (f64, f64)) -> Option<SelectionPoint> {
        point_to_cell(
            view_px,
            grid(0),
            0.0,
            OutOfGrid::Clamp { fill_rows: 0 },
            2.0,
            100,
            33,
        )
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
    fn reject_keeps_the_report_inside_the_grid() {
        // Sahne 100×33 hücre, 9×18 piksel @2x → view 450×297 nokta.
        // `Clamp` kenar dışını yapıştırıyor (seçimin kuralı), `Reject`
        // reddediyor (rapor **başlatan** olayın kuralı): başlık çubuğundan,
        // sol paydan ya da dock bandından gelen bir koordinat uygulamaya
        // ızgaranın kenar hücresini bildirirdi ve işaretçi orada değil.
        let reject =
            |view_px| point_to_cell(view_px, grid(0), 0.0, OutOfGrid::Reject, 2.0, 100, 33);
        // İçeride: iki kapı da aynı hücreyi veriyor.
        assert_eq!(reject((5.0, 9.0)), scene_point((5.0, 9.0)));
        // Son hücrenin içi hâlâ geçerli (449.5 nokta < 450).
        assert!(reject((449.0, 296.0)).is_some());
        // Üstte (başlık çubuğu tarafı) ve solda (pay) ret; `Clamp` yapıştırır.
        assert_eq!(reject((5.0, -1.0)), None);
        assert_eq!(reject((-1.0, 9.0)), None);
        assert_eq!(scene((5.0, -1.0)), Some((1, 0)));
        assert_eq!(scene((-1.0, 9.0)), Some((0, 1)));
        // Altta (dock bandı) ve sağda ret; `Clamp` son satıra/sütuna yapıştırır.
        assert_eq!(reject((5.0, 297.0)), None);
        assert_eq!(reject((450.0, 9.0)), None);
        assert_eq!(scene((5.0, 297.0)), Some((1, 32)));
        assert_eq!(scene((450.0, 9.0)), Some((99, 1)));
    }

    #[test]
    fn reject_measures_from_the_origin_like_clamp_does() {
        // Öteleme ızgarayı aşağı itiyor: üstte kalan boşluk ızgaranın
        // **dışı**, yani rapor başlatan olay orada da reddediliyor. Kapı
        // ötelemeyi `Clamp` ile aynı yerden okuyor (`origin_px`), yoksa
        // tabana yaslı pencerede bütün üst yarı geçerli sayılırdı.
        let origin_px = 100.0;
        let at =
            |view_px, outside| point_to_cell(view_px, grid(0), origin_px, outside, 2.0, 100, 33);
        // 49 nokta × 2 = 98 piksel < 100: orijinin üstü.
        assert_eq!(at((5.0, 49.0), OutOfGrid::Reject), None);
        // Doldurma yokken `Clamp` orayı 0. satıra yapıştırmayı sürdürüyor.
        assert_eq!(
            at((5.0, 49.0), OutOfGrid::Clamp { fill_rows: 0 }).map(|p| p.row),
            Some(0)
        );
        // Orijinin hemen altı geçerli.
        assert_eq!(at((51.0, 51.0), OutOfGrid::Reject).map(|p| p.row), Some(0));
    }

    #[test]
    fn motion_is_throttled_to_one_report_per_cell() {
        // Kısmanın tek kuralı: ilk görüşte `true`, aynı hücrenin tekrarında
        // `false`. Bu olmadan işaretçinin her pikseli bir rapor üretir ve
        // boşta duran bir uygulamayı sürekli çizdirirdi.
        let notch = Cell::new(None);
        let cell = |col, row, half| SelectionPoint { col, row, half };
        assert!(moved_to_new_cell(&notch, cell(3, 7, CellHalf::Left)));
        assert!(!moved_to_new_cell(&notch, cell(3, 7, CellHalf::Left)));
        // **Yarı okunmuyor**: rapor hücre çözünürlüğünde ve hücrenin öteki
        // yarısına geçmek yeni bir rapor doğurmamalı.
        assert!(!moved_to_new_cell(&notch, cell(3, 7, CellHalf::Right)));
        // Sütun ya da satır değişince rapor yeniden gidiyor.
        assert!(moved_to_new_cell(&notch, cell(4, 7, CellHalf::Right)));
        assert!(moved_to_new_cell(&notch, cell(4, 8, CellHalf::Right)));
        // Geri dönüş de bir değişim.
        assert!(moved_to_new_cell(&notch, cell(4, 7, CellHalf::Right)));
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
    fn the_gutter_shifts_the_grid_origin() {
        // Sahne: 9×18 hücre, @2x, **8 fiziksel piksel** pay. View'da pay
        // 4 nokta, hücre 4.5 nokta eder.
        let at = |x: f64| {
            point_to_cell(
                (x, 9.0),
                grid(8),
                0.0,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                100,
                33,
            )
        };
        let cell = |point: Option<SelectionPoint>| point.map(|p| (p.col, p.half));

        // Payın **içi** ilk sütuna kırpılır ve sol yarıda kalır: seçim payda
        // başlamaz. Ayrı bir kırpma dalı yok — çıkarmadan sonra x negatif
        // ve `as u16` onu sıfıra doyuruyor, `%` de negatif artığı sol yarıya
        // yazıyor (grid'in solundaki noktayla aynı yol).
        assert_eq!(cell(at(0.0)), Some((0, CellHalf::Left)), "payın sol ucu");
        assert_eq!(cell(at(2.0)), Some((0, CellHalf::Left)), "payın ortası");

        // Payın solundaki nokta da aynı yere yapışır: grid'in solundan
        // başlayan sürükleme ilk harfi seçime katmalı.
        assert_eq!(cell(at(-1.0)), Some((0, CellHalf::Left)), "payın solu");

        // Payın bittiği yer 0. sütunun **başı**: metnin ilk karakterine
        // tıklamak ilk sütunu verir.
        assert_eq!(cell(at(4.0)), Some((0, CellHalf::Left)), "payın bitişi");
        assert_eq!(cell(at(8.5)), Some((1, CellHalf::Left)), "bir hücre sonra");

        // **Kaymayı gören iki nokta.** Pay hücre genişliğinden dar olduğu
        // için çoğu x paylı da paysız da aynı sütuna düşüyor ve yalnız yarısı
        // değişiyor; sütunun gerçekten oynadığı yerler bunlar. Paysız sahne
        // aynı soruyu sorup farklı cevap veriyor — sınamayı ayıran şey bu,
        // yoksa pay hiç uygulanmasa da geçerdi.
        assert_eq!(cell(at(5.0)), Some((0, CellHalf::Left)), "paylı");
        assert_eq!(
            point_to_cell(
                (5.0, 9.0),
                grid(0),
                0.0,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                100,
                33
            )
            .map(|p| (p.col, p.half)),
            Some((1, CellHalf::Left)),
            "paysız aynı nokta bir sonraki sütun"
        );

        // Sağ kenar: pay sütunları sağa ittiği için grid'in sağ ucu da pay
        // kadar geç bitiyor. Paysız sahnede aynı nokta grid'i **taşar** ve
        // son sütunun sağ yarısına kırpılır; paylı sahnede hâlâ 99. sütunun
        // içinde. Payın `cols` hesabıyla aynı kaynaktan geldiğinin kanıtı da
        // bu: ikisi ayrışsaydı son sütun ya erken biterdi ya taşardı.
        assert_eq!(cell(at(451.5)), Some((99, CellHalf::Left)), "paylı sağ uç");
        assert_eq!(
            point_to_cell(
                (451.5, 9.0),
                grid(0),
                0.0,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                100,
                33
            )
            .map(|p| (p.col, p.half)),
            Some((99, CellHalf::Right)),
            "paysız aynı nokta grid'i taşar"
        );
    }

    /// Orijinin üstündeki sahnenin ölçüsü: 9×18 hücre, @2x, **180 fiziksel
    /// piksel** orijin — yani on satırlık bir alan, ardından içerik. View'da
    /// orijin 90 nokta eder, hücre 9 nokta.
    ///
    /// İki sınama aynı sahneyi iki `fill` ile soruyor: sıfırda alan **boş**
    /// ve tıklama kırpılır, sıfırdan büyükte alanda **geçmiş** var ve tıklama
    /// reddedilir.
    const ORIGIN_PX: f64 = 180.0;

    #[test]
    fn the_origin_shifts_the_grid_down_and_the_blank_area_clamps() {
        // Payın dikey ikizi ve **`u16` tuzağının asıl yeri**: tabana
        // yapışmada boş alan üstte, yani pencerenin üst yarısına yapılan
        // tıklamada fark negatife iniyor. `u16`'da yapılsaydı taşar ve o
        // tıklama son satırı seçerdi — sürüklemenin başı ekranın dibine
        // fırlardı. `f64`'te negatif kalıyor ve `as u16` sıfıra doyuruyor.
        let at = |y: f64| {
            point_to_cell(
                (0.0, y),
                grid(0),
                ORIGIN_PX,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                100,
                33,
            )
        };
        let row = |point: Option<SelectionPoint>| point.map(|p| p.row);

        // Boş alanın tamamı 0. satıra yapışır: üst kenar, ortası ve orijinin
        // bittiği yerin bir öncesi. Ayrı bir kırpma dalı yok — ve kırpma
        // **kaldırılamaz**: doldurma yokken orası gerçekten boş ve yukarıdan
        // başlayan sürükleme ilk satırı seçime katmalı.
        assert_eq!(row(at(0.0)), Some(0), "üst kenar");
        assert_eq!(row(at(45.0)), Some(0), "boş alanın ortası");
        assert_eq!(row(at(89.0)), Some(0), "içeriğin bir öncesi");

        // Orijinin bittiği yer 0. satırın **başı**: içeriğin ilk satırına
        // tıklamak ilk satırı verir, bir sonraki hücre bir sonraki satırı.
        assert_eq!(row(at(90.0)), Some(0), "içeriğin başı");
        assert_eq!(row(at(99.0)), Some(1), "bir satır sonra");

        // **Kaymayı gören nokta:** orijinsiz sahne aynı soruyu sorup farklı
        // cevap veriyor. Bu satır olmasa orijin hiç uygulanmasa da sınama
        // geçerdi — payın kendi sınamasındaki ayrımın aynısı.
        assert_eq!(
            row(point_to_cell(
                (0.0, 99.0),
                grid(0),
                0.0,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                100,
                33
            )),
            Some(11),
            "orijinsiz aynı nokta on bir satır aşağıda"
        );

        // Alt taşma hâlâ son satıra kırpılıyor: orijin alt kenarın kuralını
        // değiştirmiyor, yalnız başlangıcı iteliyor.
        assert_eq!(row(at(600.0)), Some(32), "alt taşma");

        // **Kaymanın ortası da meşru bir orijin** (R2.7): fare çizilen değeri
        // okuyor ve o değer kayma boyunca satır sınırında durmuyor. Burada
        // yarım hücre (9 fiziksel piksel) eklenmiş: içeriğin ilk satırı artık
        // yarım hücre aşağıda ve eski sınır bir satır yukarıya düşüyor.
        // Fonksiyonun tam satır varsayımı yok — olsaydı belirti "kayarken
        // tıklama bir satır şaşıyor" olurdu.
        let mid = |y: f64| {
            point_to_cell(
                (0.0, y),
                grid(0),
                ORIGIN_PX + 9.0,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                100,
                33,
            )
        };
        assert_eq!(
            row(mid(99.0)),
            Some(0),
            "kayma ortasında içeriğin ilk satırı"
        );
        assert_eq!(
            row(mid(103.5)),
            Some(1),
            "yarım hücre sonra bir satır aşağı"
        );
    }

    #[test]
    fn a_click_over_the_filled_area_is_rejected_instead_of_clamped() {
        // Doldurma gelince orijinin üstü **boş değil**: kullanıcı orada metin
        // görüyor. Kırpma sürseydi çapa gözün gördüğü satıra değil içeriğin
        // tepesine düşer, vurgu bambaşka bir yerde belirirdi — seçim
        // sözleşmesinin ("gözün gördüğü ile panonun verdiği ayrışmıyor")
        // adıyla yasakladığı şey. Doldurulan satırlar **seçilemez** olduğu
        // için (satır numaraları negatife açılmadan temsil edilemezler) tek
        // doğru cevap reddetmek.
        let at = |y: f64| {
            point_to_cell(
                (0.0, y),
                grid(0),
                ORIGIN_PX,
                OutOfGrid::Clamp { fill_rows: 10 },
                2.0,
                100,
                33,
            )
        };
        let row = |point: Option<SelectionPoint>| point.map(|p| p.row);

        // Boş alanın kırpıldığı **üç noktanın aynısı**, bu kez `None`: iki
        // sınamayı ayıran tek girdi `fill`.
        assert_eq!(at(0.0), None, "üst kenar");
        assert_eq!(at(45.0), None, "bandın ortası");
        assert_eq!(at(89.0), None, "içeriğin bir öncesi");

        // Sürükleme tam burada duruyor: iki çağrı yeri de (`mouseDragged:` ve
        // `follow_pointer`) `if let Some` ile giriyor, yani `None` gelen
        // olayda seçimin ucu **son geçerli hücresinde** kalıyor.

        // İçeriğin kendisi el değmeden geçiyor — ret yalnız orijinin üstüne.
        assert_eq!(row(at(90.0)), Some(0), "içeriğin başı");
        assert_eq!(row(at(99.0)), Some(1), "bir satır sonra");
        assert_eq!(row(at(600.0)), Some(32), "alt taşma hâlâ son satır");

        // **Band boşluğun tamamını kaplamasa da** ret orijinin üstünün
        // tamamına: `fill = min(gap, taze satır)` ve üstte hâlâ boşluk
        // kalabilir. İki bölgeyi ayırmak farenin `fill`'i bir de piksele
        // çevirmesini isterdi; reddin yönü güvenli, kırpmanınki değil.
        let thin = |y: f64| {
            point_to_cell(
                (0.0, y),
                grid(0),
                ORIGIN_PX,
                OutOfGrid::Clamp { fill_rows: 1 },
                2.0,
                100,
                33,
            )
        };
        assert_eq!(thin(0.0), None, "bandın üstünde kalan boşluk");
        assert_eq!(thin(89.0), None, "bandın içi");
    }

    #[test]
    fn empty_grid_has_no_cell() {
        // Simge durumundaki pencere sıfır sütun/satır verebilir: yapışacak bir
        // son hücre yok.
        assert_eq!(
            point_to_cell(
                (1.0, 1.0),
                grid(0),
                0.0,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                0,
                33
            ),
            None
        );
        assert_eq!(
            point_to_cell(
                (1.0, 1.0),
                grid(0),
                0.0,
                OutOfGrid::Clamp { fill_rows: 0 },
                2.0,
                100,
                0
            ),
            None
        );
    }

    #[test]
    fn command_keys_never_reach_the_terminal() {
        let extras = [
            NSEventModifierFlags::empty(),
            NSEventModifierFlags::Shift,
            NSEventModifierFlags::Option,
            NSEventModifierFlags::Control,
            NSEventModifierFlags::Function,
            NSEventModifierFlags::CapsLock,
        ];
        // Menüde karşılığı olmayan Command'lı harf (Cmd-T) kabuğa "t" yazmaz;
        // yanındaki değiştirici ne olursa olsun. Liste **kapalı**: ölçüt
        // "Command'lı mı" değil "izin listesinde mi" oldu ve açık bir kural
        // bir gün bu tuşu da geçirirdi.
        for extra in extras {
            assert!(
                !reaches_terminal(NSEventModifierFlags::Command | extra, Some("t")),
                "Command + {extra:?}"
            );
        }
        // Saf modifier tuşu: `characters` yok, ortada kimliği sorulacak bir
        // tuş da yok — yutulur.
        assert!(!reaches_terminal(NSEventModifierFlags::Command, None));
        // **Tek istisna**: ⌘⌫ (`\x15`, baytı `encode_key`'de). Yanındaki
        // değiştirici sorulmuyor — CapsLock açıkken de satırı silmeli.
        for extra in extras {
            assert!(
                reaches_terminal(NSEventModifierFlags::Command | extra, Some("\u{7f}")),
                "Command + Delete + {extra:?}"
            );
        }
        // Tek karakterlik eşleşme: ⌫ ile başlayan çok karakterli bir
        // `characters` listeye girmez.
        assert!(!reaches_terminal(
            NSEventModifierFlags::Command,
            Some("\u{7f}x")
        ));
        // Command'sız tuş terminalin: Control'lü harf bir bayt, Option'lı
        // gezinme tuşu bir Meta dizisi, Option'lı harf bir karakter.
        for flags in extras {
            assert!(reaches_terminal(flags, Some("t")), "{flags:?}");
        }
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
        let at1x = point_to_cell(
            (90.0, 150.0),
            grid(0),
            0.0,
            OutOfGrid::Clamp { fill_rows: 0 },
            1.0,
            100,
            33,
        );
        let at2x = point_to_cell(
            (90.0, 150.0),
            grid(0),
            0.0,
            OutOfGrid::Clamp { fill_rows: 0 },
            2.0,
            100,
            33,
        );
        assert_eq!(
            (at1x.map(|p| (p.col, p.row)), at2x.map(|p| (p.col, p.row))),
            (Some((10, 8)), Some((20, 16)))
        );
    }

    /// Damlanın elemesi: **yalnız dosya URL'si** yol veriyor.
    ///
    /// Bekçi set kapısından **sonra** eklendi, çünkü kapının bulduğu
    /// doc↔kod çelişkisinin düzeltmesi (`isFileURL` kademesi) kapıyı
    /// görmemişti. Çivilediği şey `NSURL`'ün cömertliği: sınıf `http://`'yi
    /// de okuyor ve `NSURL.path` ona `/foo` cevabını veriyor, yani kademe
    /// olmadan tarayıcıdan sürüklenen bir bağlantı giriş satırına kökten bir
    /// yol yazardı (`discussion.md` → Karar 4: yalnız dosya URL'si).
    ///
    /// Pano **benzersiz ve yerel**: `generalPasteboard` kullanılsaydı sınama
    /// kullanıcının kopyaladığı şeyi silerdi.
    #[test]
    fn only_file_urls_become_dropped_paths() {
        let board = NSPasteboard::pasteboardWithUniqueName();
        let file = NSURL::fileURLWithPath(ns_string!("/tmp/bir dosya.txt"));
        let web = NSURL::URLWithString(ns_string!("http://example.com/foo")).expect("geçerli URL");
        board.clearContents();
        let written = board.writeObjects(&NSArray::from_retained_slice(&[
            ProtocolObject::from_retained(file),
            ProtocolObject::from_retained(web),
        ]));
        assert!(written, "pano iki URL'yi de aldı");

        // Web adresi düşüyor, dosya yolu **yüzde çözülmüş** geliyor: yüzde
        // çözmeyi ikinci kez yazmama kararının (Foundation'ın kendi cevabı)
        // gözlemlenebilir yarısı.
        assert_eq!(
            dropped_paths(&board),
            vec!["/tmp/bir dosya.txt".to_string()]
        );

        // Pano benzersiz ve süreç-yerel: sınama süreci bitince gidiyor,
        // elle bırakma (`releaseGlobally`) bu bağlamada yok.
        board.clearContents();
    }
}
