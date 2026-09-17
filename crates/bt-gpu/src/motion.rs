//! İmlecin ve içeriğin kayması — **saf**, ObjC'siz, kilitsiz.
//!
//! Emsali `Gate` ve `FailureStreak`: politikanın kendisi platformdan bağımsız
//! olduğu için ayrı bir tipte yaşıyor ve gerçek bir pencere olmadan sınanıyor.
//! `link.rs`'e gömülü kalsaydı yalnız ekranda denenebilirdi.
//!
//! **İki animatör, tek tip** ([`Motion`]): imleç ([`State`], iki eksen) ve
//! ötelemenin kayması ([`Slide`], tek eksen). Ayrı bir tipe çıkarılmadılar,
//! çünkü link'in uyku kararı tek: `motion.settled()` (`link.rs`'in "hasar yok"
//! dalı). İkinci bir animatör o kapının **dışında** kalsaydı link kayma
//! ortasında uyur ve içerik donardı — animasyonun görünen belirtisi
//! "yarıda kaldı" olurdu.
//!
//! **İkisi aynı fiziği paylaşıyor, aynı kipi değil.** Sabitler, kübik
//! yavaşlama ve yayın kapalı formu ortak ([`ease_axis`], [`spring_axis`],
//! [`axis_settled`]); Hareketi Azalt'ta imleç **belirirken** öteleme
//! **snap**'liyor ([`Motion::origin_mode`]). Gerekçe indirgemenin kendisi: her
//! yeni satırda bütün ekranın belirmesi, kaldırmaya çalıştığı hareketten beter
//! olurdu.
//!
//! **İmlecin hedefi ekran satırıdır**, grid satırı değil ([`Motion::sync`]):
//! Enter'da grid satırı `r → r+1` olurken öteleme bir azalıyor, yani ekran
//! satırı hiç değişmiyor. İki hedef aynı uzayda olmasaydı imleç bir satır
//! düşüp geri binerdi.
//!
//! **Durum hücre birimindedir**, pikselde değil (008 Karar 5): font, zoom ya
//! da ekran ölçeği değişince konumun piksel karşılığı oynar ama hücre
//! koordinatı aynı kalır, yani ölçü değişimi kendiliğinden doğru yere düşer.
//!
//! **Her animasyon bir durma koşulu taşır** (`CLAUDE.md`) ve buradaki iki
//! katlı: konum+hız eşiği **ya da** süre tavanı. İkincisi kemer — birincisini
//! hiç sağlamayan bir parametre seti (aşırı düşük sönümleme, sonsuz salınım)
//! link'i sonsuza uyanık tutardı.
//!
//! **Üç stil, tek durum makinesi** ([`bt_core::CursorMotion`]): `Snap`
//! animasyonu hiç başlatmaz (yani hareket karesi de doğmaz), `Ease` sabit
//! süreli ve yapısal olarak taşmasız, `Spring` kritik sönümlü yay. Stil
//! ayardan geliyor ve **çözülmüş** olarak: `bt-shell` dosyayı okuyor, burası
//! yalnız fiziği biliyor. Süreler ve katsayılar **seçilmiş** sayılardır,
//! ölçülmüş değil; hepsi bu dosyanın başında, doc'larıyla.
//!
//! **Hareketi Azalt dördüncü bir kip** ([`Mode::Fade`]), dördüncü bir stil
//! değil: stil ile `reduce` bayrağı [`Motion::mode`]'da birleşiyor ve
//! indirgeme **tek yerde** yaşıyor. Açıkken kayma yok — imleç yeni hücresinde
//! [`FADE_DURATION`] boyunca **belirir**. Bayrak da çözülmüş geliyor: üç
//! değerli `reduce_motion` ile sistemin cevabını `bt-shell` birleştiriyor,
//! çünkü `bt-gpu` AppKit görmüyor.

use bt_core::CursorMotion;

/// Yay sertliği, rad/s. **Seçilmiş bir sayı, ölçülmüş değil.**
///
/// Kritik sönümlemede (ζ = 1) bir hücrelik kayma bu değerde ~230 ms'de
/// yerleşiyor (bağlayan eşik [`VEL_EPSILON`], konum değil). Metalterm'in
/// imleci gözle o mertebede; sayının kendisi bir ölçüme değil bu hedefe
/// dayanıyor.
///
/// **Yerleşme süresi mesafeyle büyüyor** ve bu, eşiklerin **mutlak** olmasının
/// doğrudan sonucu: `D` hücrelik bir sıçramada süre `ln(D)` ile artıyor —
/// 1 hücre ~230 ms, 200 hücre ~430 ms, 400 hücre ~460 ms. Oran değil fark
/// önemli: sabit bir "yerleşme süresi" yok ve [`TIME_CEILING`] payını buna
/// göre taşımak zorunda. (Sayılar kapalı formdan **hesaplandı**, ölçülmedi;
/// ölçüm gerçek pencere ister ve bu bir kapı değil.)
const OMEGA: f32 = 30.0;

/// Yerleşmiş sayılmak için konum eşiği, **hücre**. Yarım pikselin altında
/// kalması yetiyor: tipik hücre 8–20 piksel, yani `0.02` hücre ≤ 0,4 piksel.
const POS_EPSILON: f32 = 0.02;

/// Yerleşmiş sayılmak için hız eşiği, **hücre/saniye**. Konumla **birlikte**
/// sorulmak zorunda: hedefin tam üstünden geçerken konum farkı bir an sıfıra
/// yaklaşır ve tek başına konuma bakan bir eşik animasyonu ortasında
/// durdururdu.
const VEL_EPSILON: f32 = 0.2;

/// Süre tavanı, saniye — **kemer, ölçülmüş bir süre değil**.
///
/// Eşik yolunun tıkandığı her hâlde (parametre değişimi, `dt`'nin hiç
/// ilerlemediği patolojik bir ritim) animasyonu sonlu tutan şey bu.
/// Ateşlerse imleç hedefe atlar, yani belirti görünür bir sıçrama olur,
/// sonsuz bir kare akışı değil.
///
/// **Payın operandı en uzun meşru sıçrama, "tipik" kayma değil.** İlk sayı
/// `0.5` idi ve [`OMEGA`]'nın bir hücrelik ~230 ms'sinin iki katı diye
/// gerekçelendirilmişti; oysa eşikler mutlak olduğu için 400 hücrelik bir
/// sıçrama (geniş ekranda satır başına dönüş) ~460 ms'de yerleşiyor — yani
/// eski tavan meşru bir kaymayı **kesmeye %10 kalmıştı** ve belirtisi
/// kaymanın sonunda görünür bir snap olurdu. `0.7` o hesabın ~%50 üstünde.
/// Sayılar kapalı formdan hesaplandı ([`OMEGA`]'nın doc'u), ölçülmedi.
const TIME_CEILING: f32 = 0.7;

/// `dt`'nin üst sınırı, saniye.
///
/// Örtülme kalkınca (ya da sistem link'i kıstığında) iki damga arası
/// sınırsız olabilir. Kırpılmazsa süre tavanı **vahşi kareyi önlemek yerine
/// ondan sonra** ateşler: tek adımda `elapsed` tavanı aşar ve animasyon hiç
/// görünmeden biter. Değer iki kare (~120 Hz'de 16 ms, 60 Hz'de 33 ms) ile
/// bir göz kırpması arasında; yine seçilmiş.
const DT_MAX: f32 = 0.1;

/// `ease` stilinin kayma süresi, saniye — **seçilmiş bir sayı, ölçülmüş
/// değil.**
///
/// [`OMEGA`]'nın bir hücrelik ~230 ms'sinin biraz altında: `ease`'in ayırt
/// edici yanı süresinin **mesafeden bağımsız** olması, yani uzak sıçramada
/// yaydan hızlı, yakın sıçramada ona yakın. Değer [`TIME_CEILING`]'in altında
/// kalmak zorunda, yoksa kemer meşru bir `ease` kaymasını keserdi
/// (`ease_settles_well_inside_the_ceiling`).
const EASE_DURATION: f32 = 0.18;

// `ease`'in durma koşulu kendi saati, yani süre tavanının kemeri ona
// **uygulanmıyor**; iki sayının sırası bu yüzden bir yorum cümlesi değil bir
// şart. Tavanı `ease`'in altına düşüren bir gelecek değişiklik burada patlar,
// ekranda kesilen bir kaymada değil.
const _: () = assert!(EASE_DURATION < TIME_CEILING);

// Belirmenin "duraksama" ölçütü `dt`'nin kırpılmasına **yaslanıyor**
// (`Motion::sync`): link uyuduktan sonraki ilk kare `DT_MAX` kadar sayılıyor ve
// bunun bir duraksama sayılması için kırpmanın belirmeden uzun olması şart.
// Ters çevrilseydi uykudan uyanan imleç hiç belirmezdi ve belirti "bazen
// belirmiyor" gibi sinsi olurdu.
const _: () = assert!(FADE_DURATION < DT_MAX);

/// Hareketi Azalt açıkken belirmenin süresi, saniye — **seçilmiş bir sayı,
/// ölçülmüş değil.**
///
/// Apple'ın Reduce Motion rehberi kaymanın yerine **solma** koyuyor; burada
/// da indirgeme "animasyon yok" değil "yer değiştirmeyen kısa bir animasyon".
/// 90 ms göz kırpmanın altında: değişimi bildirecek kadar var, hareket diye
/// okunmayacak kadar kısa.
///
/// **[`DT_MAX`]'ten (100 ms) küçük ve bu bilerek bırakıldı.** Tek bir vahşi
/// kare (örtülme sonrası, sistem link'i kısınca) belirmeyi tek adımda
/// bitirebilir, yani görünmeden geçer. Örtülmenin kendi yolu zaten bunu
/// istiyor ([`Motion::finish`]); geri kalan hâlde bedel bir kez görünmeyen bir
/// belirme, kazanç ise `dt` kırpmasının tek sayıda kalması. `DT_MAX`'i
/// düşüren biri buradaki ilişkiyi değil, kendi gerekçesini tartmalı.
const FADE_DURATION: f32 = 0.09;

/// Etkin hareket kipi: kullanıcının stili ile Hareketi Azalt'ın **birleştiği
/// tek yer** ([`Motion::mode`]).
///
/// [`bt_core::CursorMotion`]'ın kopyası değil, üstüne bir kol: ayar üç değerli
/// kalıyor (kullanıcı "Hareketi Azalt açıkken hangi stil" diye bir şey
/// seçmiyor), karar burada çıkıyor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Snap,
    Ease,
    Spring,
    /// Hareketi Azalt: konum anında hedefte, değişen şey opaklık.
    Fade,
}

/// İmlecin kaymasının durumu.
///
/// `Option`'ın içi "uçuşta bir imleç var" demek; `None` hem ilk kare hem de
/// görünmez imleç. İkisini ayıran bir bayrak **bilerek yok**: ikisinde de
/// yapılacak şey aynı (sıradaki hedefe anında otur) ve iki bayrak
/// ayrışabilen iki gerçek olurdu.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Motion {
    /// Kullanıcının seçtiği stil ([`Motion::set_style`]).
    ///
    /// Varsayılanı **burada yok**: `Default` türetiliyor ve değeri
    /// [`bt_core::CursorMotion`]'ın `Default`'undan alıyor. Buraya bir
    /// `Spring` yazılsaydı varsayılanın ikinci bir sahibi doğardı ve ayar
    /// modelininkiyle sessizce ayrışabilirdi — hermetik süreli koşu tam da o
    /// değeri alıyor (`hareket > 0` kapısı ona yaslanıyor).
    style: CursorMotion,
    /// Hareketi Azalt açık mı — **çözülmüş** değer ([`Motion::set_reduce`]).
    ///
    /// `bt_core::ReduceMotion`'ın üç değeri burada yok: "sistemi izle"nin
    /// cevabını `NSWorkspace` veriyor ve o soruyu soran katman `bt-shell`.
    /// Üçlüyü buraya taşımak `bt-gpu`'yu ayar dosyasının değil sistemin
    /// erişilebilirlik ayarının müşterisi yapardı.
    ///
    /// `Default` `false`: hermetik süreli koşu sistem ayarını okumuyor
    /// (`bt-shell`'in `Inputs::Hermetic`'i) ve `make duman`'ın `hareket > 0`
    /// gerekliliği ölçen makinenin erişilebilirlik ayarına bağlanamaz.
    reduce: bool,
    state: Option<State>,
    /// Ötelemenin kayması; `None` → henüz hiç içerik karesi yok.
    ///
    /// İmlecinkiyle aynı `Option` sözleşmesi ve aynı sebeple: yokluk da ilk
    /// kare de "sıradaki hedefe anında otur" demek.
    origin: Option<Slide>,
    /// Son kareden görülen kaydırma ofseti; `None` → henüz hiç kare yok.
    ///
    /// Ayrı tutuluyor çünkü `state` boşalsa da (görünmez imleç) ofsetin
    /// geçmişi kaybolmamalı: TUI imleci gizleyip pencereyi kaydırır, sonra
    /// geri açar.
    offset: Option<i32>,
}

#[derive(Clone, Copy, Debug)]
struct State {
    /// Hücre biriminde `(sütun, satır)`; tam sayı olmak zorunda değil.
    pos: [f32; 2],
    /// Hücre/saniye. `ease`'de **hep sıfır**: o stilin konumu zamanın
    /// fonksiyonu, hızın entegrali değil.
    vel: [f32; 2],
    /// Kaymanın başladığı yer — yalnız `ease`'in operandı.
    ///
    /// Yay hızı taşıdığı için geçmişe ihtiyaç duymuyor; `ease` ise konumu
    /// `from → target` arasında `elapsed`'e göre **yeniden hesaplıyor**, yani
    /// çıkış noktasını unutamaz. Uçuşta hedef değişince (ya da stil
    /// değişince) burası bulunulan konuma çekiliyor: yoksa imleç eski
    /// başlangıçtan yeniden başlar, yani geri sıçrardı.
    from: [f32; 2],
    target: [f32; 2],
    /// Hedef kurulalı beri geçen süre; süre tavanının **ve** `ease`'in ilerleme
    /// operandı.
    elapsed: f32,
    /// Son **hedef değişiminden** beri geçen süre — yalnız [`Mode::Fade`]'in
    /// operandı ve `elapsed`'in kopyası değil: o, belirmenin kendi saati;
    /// bu, iki hareket **arasındaki** boşluk.
    ///
    /// Belirmenin "yeniden başlar mı" sorusu buna bakıyor (`Motion::sync`):
    /// duraksamadan sonraki hareket yeni bir belirmedir, akan çıktının her
    /// karede oynattığı imleç değil.
    since_move: f32,
}

/// Ötelemenin kaymasının durumu — [`State`]'in **tek eksenli** kardeşi.
///
/// Ayrı bir tip, `State`'in ikinci ekseni boş bırakılarak değil: kullanılmayan
/// bir eksen tipin söylediği yalan olurdu ve `since_move` (belirmenin saati)
/// buraya hiç girmiyor — öteleme belirmiyor ([`Motion::origin_mode`]).
/// Paylaşılan şey **fizik**: [`ease_axis`], [`spring_axis`] ve
/// [`axis_settled`] ikisinin de altında.
#[derive(Clone, Copy, Debug)]
struct Slide {
    /// Satır cinsinden; tam sayı olmak zorunda değil. Piksele çeviren ve
    /// **aygıt ızgarasına yuvarlayan** taraf `Frame::set_origin_rows`.
    pos: f32,
    /// Satır/saniye. `ease`'de hep sıfır ([`State::vel`] ile aynı gerekçe).
    vel: f32,
    /// Kaymanın başladığı yer — `ease`'in operandı ve iki kipin "gidecek yol
    /// yok" koşulu.
    from: f32,
    target: f32,
    elapsed: f32,
}

impl Motion {
    /// Stil ile Hareketi Azalt'ın tek karara indiği yer.
    ///
    /// **`Snap` bayrağın üstünde** ve bu bir ürün kararı: `cursor_motion =
    /// "snap"` diyen kullanıcı hareketi zaten kapatmış, Hareketi Azalt ona bir
    /// belirme *eklememeli*. Erişilebilirlik ayarı animasyonu kısar, var
    /// olmayanı doğurmaz (`docs/AYARLAR.md` → `[motion]`).
    fn mode(self) -> Mode {
        match (self.style, self.reduce) {
            (CursorMotion::Snap, _) => Mode::Snap,
            (_, true) => Mode::Fade,
            (CursorMotion::Ease, false) => Mode::Ease,
            (CursorMotion::Spring, false) => Mode::Spring,
        }
    }

    /// Ötelemenin kipi: [`Motion::mode`] ile aynı, **belirme hariç**.
    ///
    /// Hareketi Azalt'ta öteleme kaymaz ama **belirmez de**, snap'ler (R2.3).
    /// [`Mode::Fade`] "konum anında hedefte, değişen şey opaklık" demek ve
    /// opaklık burada kimsenin alanı değil: kayan şey bütün ızgara ve onu her
    /// yeni satırda belirtmek, indirgemenin kaldırmaya çalıştığı hareketten
    /// beter olurdu.
    ///
    /// "İndirgemenin tek yeri `bt-gpu::motion`" kuralı (`CLAUDE.md`) yerinde
    /// kalıyor: **yer** aynı, **kip** iki.
    fn origin_mode(self) -> Mode {
        match self.mode() {
            Mode::Fade => Mode::Snap,
            mode => mode,
        }
    }

    /// İçerik karesinin ucu: yeni imleç geldi.
    ///
    /// Dört snap hâlinin **tamamı** burada ve tek ifadede (008 Karar 5, R3.4):
    ///
    /// - **ilk kare** — `state` yok,
    /// - **görünmezken açılan imleç** — görünmezlik `state`'i boşalttı,
    /// - **geçmişte kaydırma** — ofset oynadı,
    /// - **geometri** (pencere, font, zoom) — çağıran `geometry` diyor.
    ///
    /// Ortak gerekçe: bunların hiçbirinde imleç hareket etmedi, **altındaki
    /// ızgara** hareket etti. Animasyon uydurmak imleci olmadığı bir yerden
    /// geliyormuş gibi gösterirdi.
    ///
    /// **Aynı hedefe yeniden hedeflemek no-op'tur** ve bu şart: imleci
    /// oynatmayan içerik kareleri (renk değişimi, alt satıra yazı) saniyede
    /// onlarca gelebiliyor ve her biri süre tavanını sıfırlasaydı tavan hiç
    /// dolmazdı — kemerin kendisi kopardı.
    ///
    /// **`CursorMotion::Snap` beşinci bir snap hâli değil, hepsinin üstü:**
    /// o stilde her `sync` anında oturuyor, yani animasyon hiç başlamıyor ve
    /// `settled()` hiç `false` olmuyor — hareket karesi de doğmuyor.
    ///
    /// **Belirme kipinde konum da anında oturuyor**, yalnız opaklık
    /// animasyonlu: `pos` burada hedefe çekilmezse imleç ilk içerik karesinde
    /// **eski** hücresinde alfa sıfırla çizilir ve yerine ancak belirme
    /// bitince atlardı — hiçbir sayaç görmeyen bir kusur, çünkü hareket
    /// karesi de kare de doğru sayıda.
    ///
    /// **`row` grid satırı, hedef ekran satırı.** İkisini `origin_rows`
    /// ayırıyor ve toplama burada yapılıyor, çağıranda değil: imlecin
    /// ötelemeden **muaf** olması bu setin R2.1'i ve iki yerde yazılsaydı
    /// ayrışabilirdi. Toplam her zaman ızgaranın içinde —
    /// `content_rows > cursor_row` olduğu için `row + (rows - content_rows)`
    /// en çok `rows - 1` — ama `saturating_add` dejenere bir kareyi de
    /// sarmadan geçiriyor.
    ///
    /// **Öteleme `visible`'dan önce ve koşulsuz kuruluyor.** İmleci gizleyen
    /// bir program (vim değil, `tput civis` ile çalışan bir betik) çıktı
    /// akıtmaya devam ediyor ve ötelemenin donması içeriği yanlış yerde
    /// bırakırdı — imlecin görünürlüğü ızgaranın nerede durduğuna karar
    /// veremez.
    pub(crate) fn sync(
        &mut self,
        col: u16,
        row: u16,
        origin_rows: u16,
        visible: bool,
        offset: i32,
        geometry: bool,
    ) {
        let scrolled = self.offset != Some(offset);
        self.offset = Some(offset);
        // Ötelemenin snap tetiği imlecinkini **kapsıyor**: tekerlek (R2.6) ve
        // geometri ikisini de snap'liyor, ötelemenin bir de kendi yön kapısı
        // var ([`Motion::sync_origin`]). `docs/AYARLAR.md`'nin "Izgaranın başka
        // sebeple yer değiştirmesi kaymaz" maddesi bu iki tetik.
        self.sync_origin(f32::from(origin_rows), scrolled || geometry);
        if !visible {
            self.state = None;
            return;
        }
        // Guard'dan **önce** okunuyor: `self.state`'in ödüncü altında ikinci
        // bir alanı okumak match guard'ında kabul edilmiyor.
        let mode = self.mode();
        let animated = mode != Mode::Snap;
        let target = [f32::from(col), f32::from(row.saturating_add(origin_rows))];
        match &mut self.state {
            Some(state) if animated && !scrolled && !geometry => {
                if state.target != target {
                    // **Belirme duraksamadan sonra yeniden başlar, her
                    // karede değil** (`/code-review` bulgusu). Kayan iki
                    // stilde saati koşulsuz sıfırlamak doğru: yeni hedef yeni
                    // bir yol demek. Belirmede ise saat yolu değil
                    // **opaklığı** sürüyor ve koşulsuz sıfırlama Hareketi
                    // Azalt'ı tersine çeviriyordu: akan çıktıda her içerik
                    // karesi hedefi oynattığı için alfa sıfıra çakılıyor ve
                    // imleç **hiç görünmüyordu**. "Yalnız yerleşmişken
                    // sıfırla" da çare değil — o hâlde imleç 90 ms'de bir
                    // yeniden belirir, yani ~11 Hz'de **yanıp söner**; titreme
                    // kaldırmaya çalıştığımız hareketten beterdir, üstelik
                    // erişilebilirlik ayarının içinde.
                    //
                    // Ayıran ölçüt hareketler arasındaki boşluk: `FADE_DURATION`
                    // kadar duraksamadan sonraki hareket **ayrı** bir
                    // harekettir ve belirmeyi hak eder; daha sık gelen hedef
                    // değişimi tek bir akışın parçasıdır ve opaklığı
                    // tazelemez. Eşik olarak belirmenin kendi süresi
                    // kullanılıyor — ikinci bir sabit, ikinci bir gerekçe
                    // isterdi.
                    let resumed = state.since_move >= FADE_DURATION;
                    state.since_move = 0.0;
                    state.from = state.pos;
                    state.target = target;
                    if mode != Mode::Fade || resumed {
                        state.elapsed = 0.0;
                    }
                    if mode == Mode::Fade {
                        state.pos = target;
                    }
                }
            }
            // Snap: hem uçuştaki durum hem de yoklukta aynı yere iniyor.
            state => {
                *state = Some(State {
                    pos: target,
                    vel: [0.0; 2],
                    from: target,
                    target,
                    elapsed: 0.0,
                    since_move: 0.0,
                });
            }
        }
    }

    /// Ötelemenin hedefi: [`Motion::sync`]'in tek eksenli yarısı.
    ///
    /// Snap hâlleri imlecinkilerle **aynı sınıf** ve aynı gerekçe: ilk kare,
    /// tekerlek ve geometri. Üçünde de içerik kendi büyümesiyle yükselmedi —
    /// ızgara başka bir sebeple yer değiştirdi ve animasyon uydurmak onu
    /// gelmediği bir yerden geliyormuş gibi gösterirdi.
    ///
    /// **Dördüncüsü ötelemenin kendine ait: yön.** Yalnız **düşen** hedef
    /// kayar, yükselen snap'ler. Öteleme `rows - content_rows`, yani hedefin
    /// düşmesi içeriğin **büyümesi** (grid yukarı akar), yükselmesi
    /// **daralması** (grid aşağı iner). Yukarı akış içeriğin *gelmesi* gibi
    /// okunuyor ve hoşa gidiyor; aşağı iniş *düşmesi* gibi okunuyor ve
    /// tuhaf — kabuğun vim'den çıkarken aşağı süzülmesi, dolu bir ekranda
    /// `clear`'ın prompt'u tepeden dibe indirmesi. Kural bu yüzden mesafeye
    /// değil **işarete** bakıyor: eşik ölçülmemiş bir sayı olurdu, yön
    /// bedava (gözle kontrol, 011 kapı sonrası).
    ///
    /// Bunun bir bedeli var ve adı konmuş: art arda satır yazıp silen bir
    /// program (spinner) büyürken kayıp daralırken zıplar. Simetrik bir
    /// salınım yerine testere; gözle kontrolde kabul edildi, çünkü tek
    /// alternatifi o ölçülmemiş eşikti.
    ///
    /// **Aynı hedefe yeniden hedeflemek no-op** ([`Motion::sync`] ile aynı
    /// şart): içeriği büyütmeyen kareler (renk değişimi, satır içi yazı)
    /// saniyede onlarca geliyor ve her biri `elapsed`'i sıfırlasaydı süre
    /// tavanı hiç dolmazdı.
    ///
    /// **Girdisi monoton değil** (`bt_core::Cursor::content_rows`): imleci
    /// yukarı taşıyıp alt satırı `\e[K` ile silen bir program hedefi daraltıp
    /// genişletebilir. Durma koşulu bunu kaldırıyor, çünkü hedefe değil
    /// **hedefe olan mesafeye** bakıyor: her yeni hedef kaymayı bulunduğu
    /// yerden yeniden başlatıyor ve her biri tek başına sonlu
    /// ([`Slide::settled`]). Salınımın kendisi sonsuz sürerse link uyanık
    /// kalır — ama o kareleri isteyen şey animasyon değil, salınımı üreten
    /// **çıktının hasarı** olur.
    fn sync_origin(&mut self, target: f32, snap: bool) {
        let mode = self.origin_mode();
        let animated = mode != Mode::Snap;
        match &mut self.origin {
            // `<=`, `<` değil: **eşit** hedef hiçbir yöne gitmiyor ve içerideki
            // no-op'a düşmeli. `<` yazılsaydı hedefi değişmeyen her kare snap
            // koluna girer, uçuştaki kaymayı her karede yeniden kurar ve
            // animasyonu büsbütün öldürürdü.
            Some(slide) if animated && !snap && target <= slide.target => {
                if slide.target != target {
                    slide.from = slide.pos;
                    slide.target = target;
                    slide.elapsed = 0.0;
                }
            }
            slide => {
                *slide = Some(Slide {
                    pos: target,
                    vel: 0.0,
                    from: target,
                    target,
                    elapsed: 0.0,
                });
            }
        }
    }

    /// Kullanıcı stili değiştirdi (ayar dosyası kaydedildi).
    ///
    /// Dönüş: **uçuştaki bir kayma bu çağrıda bitirildi mi**. Çağıran bunu
    /// bilmek zorunda, çünkü link'in "hasar yok" dalı yerleşmiş bir
    /// animasyonda hiç çizmeden uyuyor — `Snap`'e geçen kullanıcının imleci
    /// aksi hâlde ara hücrede asılı kalır ve ancak alakasız bir içerik karesi
    /// onu yerine koyardı (`Renderer::set_font`'un "değişti mi" dönüşüyle aynı
    /// örüntü; aynı stili yeniden yazan kayıt no-op).
    ///
    /// **Işınlama yok.** `Snap` uçuştaki kaymayı **hedefinde** bitiriyor
    /// ([`Motion::finish`]); öteki iki stil kaymayı bulunduğu yerden
    /// devralıyor — `from` bulunulan konuma, `elapsed` sıfıra çekiliyor.
    /// Devralmasaydı `ease` eski çıkış noktasından yeniden başlar, yani imleç
    /// geri sıçrardı; yay ise hızı koruduğu için zaten sorunsuz, ama iki stil
    /// için iki ayrı kural yazmanın kazandırdığı bir şey yok.
    ///
    /// **"Uçuşta mı" sorusu eski stille sorulmak zorunda** ve bu bir sıra
    /// inceliği değil, ışınlamanın kendisi: durma koşulu stile göre
    /// değişiyor ([`State::settled`]), yani stil önce yazılırsa 180 ms'den
    /// uzun uçmuş bir yay `ease`'in saatine göre "yerleşmiş" görünür, devir
    /// atlanır ve sıradaki `advance` `t = 1` ile imleci hedefe atar — hem de
    /// **çizmeden**, çünkü link o kareyi yerleşmiş sayıp uyuyor. `snap`'te
    /// giderilen kusurun bu yoldan geri gelmiş hâli
    /// (`a_long_spring_flight_does_not_teleport_when_the_style_changes`).
    /// **Belirme kipinde `ease` ↔ `spring` hiçbir şeydir.** Kayma zaten yok,
    /// devralınacak bir konum da yok; devralma kolunu yine de koşturmak
    /// `from`'u bulunulan konuma çekerdi ve `from == target` belirmeyi
    /// **yerleşmiş** gösterirdi — link o kareyi hiç çizmeden uyur, imleç yarı
    /// saydam asılı kalırdı (`style_change_during_a_fade_keeps_fading`).
    pub(crate) fn set_style(&mut self, style: CursorMotion) -> bool {
        if self.style == style {
            return false;
        }
        let in_flight = !self.settled();
        let was_fading = self.mode() == Mode::Fade;
        self.style = style;
        if !in_flight {
            return false;
        }
        if style == CursorMotion::Snap {
            self.finish();
            return true;
        }
        if was_fading {
            return false;
        }
        if let Some(state) = &mut self.state {
            state.from = state.pos;
            state.elapsed = 0.0;
        }
        // Öteleme de devralıyor ve aynı sebeple: `ease` çıkış noktasını
        // hatırlıyor, tazelenmezse ızgara eski başlangıcına geri sıçrardı.
        if let Some(slide) = &mut self.origin {
            slide.from = slide.pos;
            slide.elapsed = 0.0;
        }
        false
    }

    /// Hareketi Azalt açıldı ya da kapandı (sistem ayarı ya da
    /// `[motion] reduce_motion`).
    ///
    /// Dönüşü [`Motion::set_style`] ile aynı sözleşme: **bu çağrı yerleşmemiş
    /// bir durumu yerleşmiş hâle getirdi mi**. Çağıran bunu bilmek zorunda,
    /// çünkü link'in "hasar yok" dalı yerleşmiş bir animasyonda hiç çizmeden
    /// uyuyor.
    ///
    /// **İki yön de uçuştakini bitiriyor** ve bu, stil değişiminden daha sert
    /// bir kural olmak zorunda — devralmanın iki yönde de anlamı yok:
    ///
    /// - **Açılırken** devralınacak şey bir kayma ve kip artık kaymıyor.
    /// - **Kapanırken** devralınacak şey bir belirme ve `ease` onu konum
    ///   animasyonu sanardı: `from` bir önceki hücrede duruyor, yani imleç
    ///   geldiği hücreye geri dönüp yeniden kayardı
    ///   (`reduce_off_mid_fade_does_not_slide_backwards`). `spring` de
    ///   hedefinde ve hızsız olduğu için anında yerleşir, yani yarı saydam
    ///   imleci ekranda bırakırdı.
    pub(crate) fn set_reduce(&mut self, reduce: bool) -> bool {
        if self.reduce == reduce {
            return false;
        }
        let in_flight = !self.settled();
        self.reduce = reduce;
        if !in_flight {
            return false;
        }
        self.finish();
        true
    }

    /// Fiziği `dt` saniye ilerletir. `dt` **burada** kırpılıyor, çağıranda
    /// değil: kırpma bu modülün durma koşulunun parçası ve çağıranın onu
    /// hatırlamasına bırakılamaz.
    ///
    /// Yerleşmeye karar verildiği anda konum **tam hedefe** oturtuluyor.
    /// Eşiğe bırakılsaydı imleç `POS_EPSILON` kadar hücre dışında dinlenir ve
    /// bloğun dikdörtgeni altındaki glyph'le sub-piksel ayrışırdı — hiçbir
    /// sayaç görmez, göz görür.
    pub(crate) fn advance(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, DT_MAX);
        self.advance_origin(dt);
        let mode = self.mode();
        let Some(state) = &mut self.state else {
            return;
        };
        state.elapsed += dt;
        state.since_move += dt;
        match mode {
            // İkisinde de `sync` zaten hedefe oturttu; ilerletilecek konum
            // yok. `Fade`'de ilerleyen şey `elapsed`'in kendisi, çünkü
            // opaklık onun fonksiyonu ([`Motion::alpha`]).
            Mode::Snap | Mode::Fade => {}
            Mode::Ease => state.ease(),
            Mode::Spring => state.spring(dt),
        }
        if state.settled(mode) {
            state.pos = state.target;
            state.vel = [0.0; 2];
            // `from` da hedefe çekiliyor, yani yerleşme kolu [`Motion::finish`]
            // ile **aynı** durumu bırakıyor. Bırakmasaydı (`/code-review`
            // bulgusu) yayın eşiğiyle erkenden oturan bir kayma `ease` ve
            // `fade`'in "gidecek yol yok" koşulunu (`from == target`)
            // sağlamaz, yani kip değişince yerleşmemiş görünürdü — üstelik
            // link o kareyi zaten yerleşmiş sayıp **uyumuş** olur ve
            // `advance` bir daha koşmazdı.
            state.from = state.target;
        }
    }

    /// [`Motion::advance`]'ın öteleme yarısı; `dt` çağıranda **kırpılmış**
    /// geliyor (tek kırpma, tek kural).
    fn advance_origin(&mut self, dt: f32) {
        let mode = self.origin_mode();
        let Some(slide) = &mut self.origin else {
            return;
        };
        slide.elapsed += dt;
        match mode {
            Mode::Ease => slide.ease(),
            Mode::Spring => slide.spring(dt),
            // `Snap`'te [`Motion::sync_origin`] zaten hedefe oturttu.
            // `Fade` buraya **hiç gelmiyor**: [`Motion::origin_mode`] onu
            // `Snap`'e çeviriyor ve öteleme belirmiyor (R2.3).
            Mode::Snap | Mode::Fade => {}
        }
        if slide.settled(mode) {
            // İmleçtekiyle **aynı üçlü** ve aynı gerekçe ([`Motion::advance`]):
            // konum tam hedefe, hız sıfıra, `from` da hedefe — yoksa erkenden
            // oturan bir kayma `ease`'in "gidecek yol yok" koşulunu
            // sağlamaz ve kip değişince yerleşmemiş görünürdü.
            slide.pos = slide.target;
            slide.vel = 0.0;
            slide.from = slide.target;
        }
    }

    /// Uçuştaki kaymayı **hedefinde bitirir** — animasyonun ilerleyemeyeceği
    /// anlar için.
    ///
    /// Bugünkü çağıranları örtülen pencere ve `snap`'e geçen ayar. Örtülmede
    /// link duruyor, yani `advance` bir daha koşmuyor ve durum sonsuza kadar
    /// "yerleşmemiş" kalırdı. Bedeli iki katlı olurdu — süreli koşu
    /// deadline'da `MotionUnsettled` deyip **kod doğruyken** kırmızı düşer ve
    /// tanı "bir durma koşulu bozuk" diye yanlış yeri gösterirdi; örtülme
    /// kalkınca da imleç, kullanıcının hiç görmediği bir noktadan kayarak
    /// gelirdi.
    ///
    /// Snap politikasının zaten söylediği şey (008 Karar 5): görünürlük
    /// dönüşü animasyonsuz. Burada yalnız aynı kural bir kare erken
    /// uygulanıyor.
    pub(crate) fn finish(&mut self) {
        if let Some(state) = &mut self.state {
            state.pos = state.target;
            state.from = state.target;
            state.vel = [0.0; 2];
            state.elapsed = 0.0;
        }
        // **İkisi birlikte bitiyor.** Yarısı bırakılsaydı link "yerleşmedi"
        // deyip uyanık kalır ve örtülen pencerede [`Motion::finish`]'in
        // kapatmak istediği delik açık kalırdı.
        if let Some(slide) = &mut self.origin {
            slide.pos = slide.target;
            slide.from = slide.target;
            slide.vel = 0.0;
            slide.elapsed = 0.0;
        }
    }

    /// Bu karede imlecin çizileceği yer, **ekran hücresi** — grid hücresi
    /// değil ([`Motion::sync`]). Durum yoksa hedef de yok: çağıran görünmez
    /// imleci zaten çizmiyor.
    pub(crate) fn position(&self) -> Option<[f32; 2]> {
        self.state.map(|state| state.pos)
    }

    /// İmlecin bu karedeki opaklığı; belirme dışında **her zaman `1.0`**.
    ///
    /// Blok da altındaki metnin rengi de bununla çarpılıyor
    /// (`Frame::push_cursor`): ikisi ayrılsaydı harf, henüz görünmeyen bir
    /// bloğun rengine boyanırdı — zeminin üstünde zemin renginde bir harf,
    /// yani okunmayan bir hücre.
    ///
    /// Yerleşmiş belirme `1.0` veriyor, `elapsed / FADE_DURATION` değil:
    /// gidecek yolu olmayan hâller ([`Motion::finish`], snap halleri) `elapsed`
    /// sıfırken yerleşik ve oran onları görünmez kılardı.
    pub(crate) fn alpha(&self) -> f32 {
        if self.mode() != Mode::Fade {
            return 1.0;
        }
        self.state.map_or(1.0, |state| {
            if state.settled(Mode::Fade) {
                1.0
            } else {
                (state.elapsed / FADE_DURATION).clamp(0.0, 1.0)
            }
        })
    }

    /// Bu karede içeriğin duracağı öteleme, **satır**. Durum yoksa `0.0`:
    /// tavana yapışık yerleşim, yani `Frame::clear`'ın bıraktığı değer.
    pub(crate) fn origin(&self) -> f32 {
        self.origin.map_or(0.0, |slide| slide.pos)
    }

    /// **Her iki** animasyon da durdu mu — link'in "uyuyabilir miyim" sorusu.
    ///
    /// Öteleme bu kapının **içinde** olmak zorunda (R2.5): dışında kalsaydı
    /// link "hasar yok" dalında kayma ortasında uyur ve içerik yarı yolda
    /// donardı. Süreli koşunun kapısı (`Verdict::MotionUnsettled`) da bunu
    /// okuyor.
    pub(crate) fn settled(&self) -> bool {
        self.cursor_settled() && self.origin_settled()
    }

    /// Yalnız imlecin animasyonu durdu mu — `hareket=` jetonunun tanığı.
    ///
    /// Durum yokken `true`: çizilecek bir imleç yoksa bekleyecek bir şey de
    /// yok.
    pub(crate) fn cursor_settled(&self) -> bool {
        let mode = self.mode();
        self.state.is_none_or(|state| state.settled(mode))
    }

    /// Yalnız ötelemenin kayması durdu mu — `kayma=` jetonunun tanığı.
    ///
    /// İkisi ayrı sorulabiliyor, çünkü jeton ikisini ayrı sayıyor: kırmızı bir
    /// koşuyu okuyan taraf hangi animatörün yerleşmediğini satırdan görmeli.
    pub(crate) fn origin_settled(&self) -> bool {
        let mode = self.origin_mode();
        self.origin.is_none_or(|slide| slide.settled(mode))
    }
}

impl State {
    /// `ease`: konum **zamanın fonksiyonu** — `from`'dan `target`'a kübik
    /// yavaşlama (`1 − (1−t)³`).
    ///
    /// Taşmanın yapısal olarak imkânsız olduğu yer burası: ifade `t ∈ [0,1]`
    /// için monoton ve `1`'i geçmiyor, yani yayın gerektirdiği taşma kırpması
    /// bu stilde hiç gerekmiyor. Hız da entegre edilmiyor (`vel` sıfır kalır);
    /// ödediği bedel `from`'u hatırlamak.
    fn ease(&mut self) {
        let t = (self.elapsed / EASE_DURATION).clamp(0.0, 1.0);
        for axis in 0..2 {
            self.pos[axis] = ease_axis(self.from[axis], self.target[axis], t);
        }
    }

    /// `spring`: kritik sönümlü yayın bir adımı.
    fn spring(&mut self, dt: f32) {
        for axis in 0..2 {
            let (pos, vel) = spring_axis(self.pos[axis], self.vel[axis], self.target[axis], dt);
            self.pos[axis] = pos;
            self.vel[axis] = vel;
        }
    }

    /// Durma koşulu; stile göre **iki ayrı soru**.
    ///
    /// `ease`'inki saattir ve bu onun tanımı: kayma [`EASE_DURATION`] sürer,
    /// mesafe ne olursa olsun. Eşiğe bağlansaydı süre sessizce mesafeye
    /// bağlanırdı (kübik yavaşlamada kalan mesafe eşiğin altına kısa
    /// sıçramada erken, uzun sıçramada geç iniyor), yani stilin adı yalan
    /// söylerdi.
    ///
    /// Belirmeninki de bir saat ve `ease` ile **aynı biçim**: süre dolmuşsa
    /// ya da gidecek yol yoksa yerleşmiş. İkisi tek kolda çünkü soru da tek —
    /// yalnız sabit değişiyor.
    ///
    /// Yayınki iki katlı: konum+hız eşiği **veya** süre tavanı. `snap`
    /// eşikten geçiyor — `sync` onu zaten hedefe oturttuğu için ilk soruda
    /// `true`.
    fn settled(&self, mode: Mode) -> bool {
        if let Mode::Ease | Mode::Fade = mode {
            let duration = if let Mode::Fade = mode {
                FADE_DURATION
            } else {
                EASE_DURATION
            };
            // "Saat doldu" **ya da** gidecek yol yok. İkinci koşul şart:
            // anında oturan imleç (snap hâlleri, [`Motion::finish`]) `from`'u
            // da hedefe koyuyor ve tek başına saate bakan bir kural onu
            // [`EASE_DURATION`] boyunca "yerleşmemiş" sayardı — link hiçbir
            // şeyi değiştirmeyen kareler çizerdi, üstelik her `sync`'te
            // yeniden.
            return self.elapsed >= duration || self.from == self.target;
        }
        // Süre tavanı **veya** eşik; ikisi de tek başına yeterli.
        self.elapsed >= TIME_CEILING
            || (0..2).all(|axis| axis_settled(self.pos[axis], self.vel[axis], self.target[axis]))
    }
}

impl Slide {
    /// [`State::ease`]'in tek eksenli hâli.
    fn ease(&mut self) {
        let t = (self.elapsed / EASE_DURATION).clamp(0.0, 1.0);
        self.pos = ease_axis(self.from, self.target, t);
    }

    /// [`State::spring`]'in tek eksenli hâli.
    fn spring(&mut self, dt: f32) {
        let (pos, vel) = spring_axis(self.pos, self.vel, self.target, dt);
        self.pos = pos;
        self.vel = vel;
    }

    /// [`State::settled`] ile **aynı iki soru**, belirme kolu olmadan:
    /// [`Motion::origin_mode`] `Fade` üretmiyor.
    ///
    /// Girdisi monoton olmadığı için ([`Motion::sync_origin`]) durma koşulu
    /// hedefe değil **mesafeye** bakıyor: hedef her oynadığında kayma
    /// bulunduğu yerden yeniden başlıyor ve her biri tek başına sonlu.
    fn settled(&self, mode: Mode) -> bool {
        if let Mode::Ease = mode {
            return self.elapsed >= EASE_DURATION || self.from == self.target;
        }
        self.elapsed >= TIME_CEILING || axis_settled(self.pos, self.vel, self.target)
    }
}

/// Kübik yavaşlamanın tek ekseni: `from → target` arasında `t ∈ [0,1]`.
///
/// Taşmanın **yapısal** olarak imkânsız olduğu yer: `1 − (1−t)³` monoton ve
/// `1`'i geçmiyor, yani yayın gerektirdiği kırpma bu stilde hiç gerekmiyor.
fn ease_axis(from: f32, target: f32, t: f32) -> f32 {
    let eased = 1.0 - (1.0 - t).powi(3);
    from + (target - from) * eased
}

/// Kritik sönümlü yayın bir adımı, tek eksen — **taşma kırpması dahil**.
///
/// ζ = 1 "durgun hâlden taşma yok" demek; [`Motion::sync`] ise uçuşta hızı
/// **bilerek** koruyor (momentum) ve o hız hedefin ötesine taşıyabiliyor:
/// `(d + c·t)e^{-ωt}` ifadesi `|v| > OMEGA × kalan mesafe` olduğunda sıfırı
/// geçiyor. Ölçülen en kötü hâl, uzun bir sıçramanın ortasında yapılan küçük
/// bir hedef düzeltmesinde **0,87 hücre** — neredeyse tam bir hücre, yani
/// gözle görülür bir geri tepme. 008 Karar 6 bunu adıyla yasaklıyor ("kritik
/// sönümlemeye yakın, **taşma yok**"), o yüzden hedefi geçen eksen hedefte
/// durduruluyor.
///
/// Sert bir duruş değil: kırpmanın ateşlediği an imleç zaten tam hedefin
/// üstünde, yani görünen şey "vardı ve durdu".
///
/// **Tek yer, iki animatör:** öteleme de aynı kırpmayı ödüyor
/// ([`Slide::spring`]). İkinci bir kopya, hedefi aşan bir ızgaranın geri
/// tepmesini sessizce geri getirirdi.
fn spring_axis(pos: f32, vel: f32, target: f32, dt: f32) -> (f32, f32) {
    let before = pos - target;
    let (after, vel) = critically_damped(before, vel, dt);
    if before != 0.0 && (before < 0.0) != (after < 0.0) {
        (target, 0.0)
    } else {
        (target + after, vel)
    }
}

/// Eşik yolu, tek eksen: konum **ve** hız birlikte.
///
/// Birlikte sorulmak zorunda — hedefin tam üstünden geçerken konum farkı bir
/// an sıfıra yaklaşır ve tek başına konuma bakan bir eşik animasyonu ortasında
/// durdururdu ([`VEL_EPSILON`]).
fn axis_settled(pos: f32, vel: f32, target: f32) -> bool {
    (pos - target).abs() <= POS_EPSILON && vel.abs() <= VEL_EPSILON
}

/// Kritik sönümlü yayın **kapalı formu**: hedefe göre göreli konum `d` ve hız
/// `v`, `dt` saniye sonra ne olur.
///
/// Euler adımı değil, çünkü kararlılığı `dt`'ye bağlı olurdu: `OMEGA * dt > 2`
/// olan tek bir vahşi kare (örtülme sonrası) ıraksardı ve `DT_MAX` o zaman
/// bir kemer değil bir **şart** olurdu. Kapalı form her `dt` için doğru; `dt`
/// yine de kırpılıyor ama başka bir sebeple (süre tavanı, bkz. [`DT_MAX`]).
///
/// ζ = 1 seçildi: **taşma yok**. Altındaki her değer imleci hedefin ötesine
/// atıp geri getirirdi ve 008 bunu açıkça istemiyor ("kritik sönümlemeye
/// yakın, taşma yok").
fn critically_damped(d: f32, v: f32, dt: f32) -> (f32, f32) {
    let c = v + OMEGA * d;
    let decay = (-OMEGA * dt).exp();
    let pos = (d + c * dt) * decay;
    let vel = (c - OMEGA * (d + c * dt)) * decay;
    (pos, vel)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 120 Hz'lik bir kare; sınamaların ortak adımı.
    const TICK: f32 = 1.0 / 120.0;

    /// `(0,0)`'dan `(10,4)`'e uçuşta bir imleç, **verilen stille**.
    fn moving_with(style: CursorMotion) -> Motion {
        let mut motion = Motion::default();
        motion.set_style(style);
        // İlk `sync` snap: durum yok.
        motion.sync(0, 0, 0, true, 0, false);
        assert!(motion.settled(), "ilk kare animasyon başlattı");
        motion.sync(10, 4, 0, true, 0, false);
        motion
    }

    /// Yayın fiziğini sınayanların ortak kurulumu; varsayılan zaten `Spring`
    /// ama sınamanın neyi ölçtüğü çağrı yerinde yazılı olmalı.
    fn moving() -> Motion {
        moving_with(CursorMotion::Spring)
    }

    /// `settled` olana kadar ilerletir; kaç kare sürdüğünü döndürür.
    /// Tavan sonsuz döngüyü kesiyor — sınama asılmasın diye.
    fn run_to_rest(motion: &mut Motion, dt: f32) -> u32 {
        for frames in 1..=10_000 {
            motion.advance(dt);
            if motion.settled() {
                return frames;
            }
        }
        panic!("animasyon yerleşmedi");
    }

    #[test]
    fn every_start_settles_in_finite_steps() {
        // Durma koşulunun kendisi: hangi mesafeden başlarsa başlasın sonlu
        // adımda duruyor. Bu sınama olmadan "boşta sıfır kare" sözleşmesi
        // yalnız bir yorum cümlesi olurdu.
        for (col, row) in [(1, 0), (0, 1), (200, 60), (10, 4)] {
            let mut motion = Motion::default();
            motion.sync(0, 0, 0, true, 0, false);
            motion.sync(col, row, 0, true, 0, false);
            assert!(!motion.settled(), "hedef değişimi animasyon başlatmadı");
            let frames = run_to_rest(&mut motion, TICK);
            assert!(
                frames < u32::try_from((TIME_CEILING / TICK).ceil() as i64 + 2).unwrap(),
                "({col},{row}) süre tavanını aştı: {frames} kare"
            );
            assert_eq!(
                motion.position(),
                Some([f32::from(col), f32::from(row)]),
                "yerleşen imleç tam hücreye oturmadı"
            );
        }
    }

    #[test]
    fn the_time_ceiling_settles_a_run_the_epsilon_never_would() {
        // **Kemerin kendisi, eşikten bağımsız.** Normal bir kaymada eşik çok
        // daha erken geliyor, yani tavanı gerçek bir animasyonla sınamak
        // imkânsız: ölçtüğü şey hep eşik olurdu. Durum bu yüzden doğrudan
        // kuruluyor — hedeften 200 hücre uzakta ve **duruyor**, yani konum
        // eşiği de hız eşiği de asla sağlanmaz. Duran tek şey tavan olabilir.
        let far = State {
            pos: [0.0; 2],
            vel: [0.0; 2],
            from: [0.0; 2],
            target: [200.0, 0.0],
            elapsed: TIME_CEILING,
            since_move: TIME_CEILING,
        };
        assert!(
            far.settled(Mode::Spring),
            "süre tavanı dolmuş koşuyu durdurmadı"
        );
        assert!(
            !State {
                elapsed: TIME_CEILING - TICK,
                ..far
            }
            .settled(Mode::Spring),
            "tavan dolmadan durdu: kemer erken ateşliyor"
        );
    }

    #[test]
    fn retarget_in_flight_keeps_velocity() {
        // Uçuşta hedef değişince hız korunmalı: sıfırlansaydı yazarken her
        // tuş imleci durdurup yeniden hızlandırır, hareket tırtıklı olurdu.
        let mut motion = moving();
        for _ in 0..6 {
            motion.advance(TICK);
        }
        let before = motion.state.expect("uçuşta").vel;
        assert!(before[0] > 0.0, "hiç hızlanmadı: {before:?}");

        motion.sync(20, 4, 0, true, 0, false);
        let after = motion.state.expect("uçuşta").vel;
        assert_eq!(after, before, "retarget hızı sıfırladı");
        assert_eq!(
            motion.state.expect("uçuşta").elapsed,
            0.0,
            "yeni hedef süre tavanını sıfırlamadı"
        );
    }

    #[test]
    fn a_retarget_in_flight_does_not_overshoot() {
        // 008 Karar 6: "kritik sönümlemeye yakın, **taşma yok**". ζ = 1 bunu
        // yalnız durgun hâlden veriyor; `sync` hızı bilerek koruduğu için
        // uzun bir sıçramanın ortasındaki küçük bir düzeltme hedefin ötesine
        // taşırdı — kırpma olmadan ölçülen en kötü hâl 0,87 hücreydi.
        let mut motion = Motion::default();
        motion.sync(0, 0, 0, true, 0, false);
        motion.sync(10, 0, 0, true, 0, false);
        for _ in 0..6 {
            motion.advance(TICK);
        }
        let pos = motion.position().expect("uçuşta")[0];
        let vel = motion.state.expect("uçuşta").vel[0];
        // Taşmanın şartı: hız, kalan mesafenin OMEGA katından büyük.
        let target = pos + 0.5;
        assert!(
            vel > OMEGA * 0.5,
            "senaryo taşma üretmiyor: hız {vel}, eşik {}",
            OMEGA * 0.5
        );

        // Hedefi kesirli kuramıyoruz (`sync` hücre alıyor), o yüzden durumu
        // doğrudan kuruyoruz: sorulan şey `advance`'ın kırpması.
        motion.state = Some(State {
            pos: [pos, 0.0],
            vel: [vel, 0.0],
            from: [pos, 0.0],
            target: [target, 0.0],
            elapsed: 0.0,
            since_move: 0.0,
        });
        for _ in 0..60 {
            motion.advance(TICK);
            assert!(
                motion.position().expect("uçuşta")[0] <= target + POS_EPSILON,
                "imleç hedefi {target} aştı: {:?}",
                motion.position()
            );
        }
    }

    #[test]
    fn even_the_longest_jump_settles_before_the_ceiling() {
        // Tavan bir **kemer**: meşru bir kaymayı kesmemeli. Eşikler mutlak
        // olduğu için yerleşme süresi mesafeyle büyüyor ve geniş bir ekranda
        // satır başına dönüş 400 hücre olabiliyor. Kesseydi belirti kaymanın
        // sonunda görünür bir snap olurdu — ve hiçbir sayaç görmezdi.
        for distance in [200u16, 400] {
            let mut motion = Motion::default();
            motion.sync(0, 0, 0, true, 0, false);
            motion.sync(distance, 0, 0, true, 0, false);
            let frames = run_to_rest(&mut motion, TICK);
            let elapsed = motion.state.expect("yerleşti").elapsed;
            assert!(
                elapsed < TIME_CEILING,
                "{distance} hücrelik sıçramayı tavan kesti: {elapsed}s ({frames} kare)"
            );
        }
    }

    #[test]
    fn retarget_to_the_same_cell_does_not_reset_the_ceiling() {
        // İmleci oynatmayan içerik kareleri (renk değişimi, alt satıra yazı)
        // tavanı sıfırlasaydı kemer hiç dolmazdı.
        let mut motion = moving();
        for _ in 0..6 {
            motion.advance(TICK);
        }
        let elapsed = motion.state.expect("uçuşta").elapsed;
        motion.sync(10, 4, 0, true, 0, false);
        assert_eq!(motion.state.expect("uçuşta").elapsed, elapsed);
    }

    #[test]
    fn a_clipped_dt_does_not_teleport() {
        // Örtülme kalkınca gelen vahşi damga: kırpılmazsa tek adımda hedefe
        // ışınlar ve animasyon hiç görünmez. `DT_MAX` bir adımın en çok ne
        // kadar ilerleyebileceğini bağlıyor.
        let mut clipped = moving();
        clipped.advance(30.0);
        let mut stepped = moving();
        stepped.advance(DT_MAX);
        assert_eq!(
            clipped.position(),
            stepped.position(),
            "`dt` kırpılmadı: vahşi damga fazladan ilerletti"
        );
    }

    #[test]
    fn scrolling_snaps_instead_of_animating() {
        // Geçmişe kaydırmak imleci ekranda taşır ama imleç hareket etmedi;
        // `row`'dan ayırt edilemeyen tek şey bu ve ofset onu ayırıyor.
        let mut motion = moving();
        run_to_rest(&mut motion, TICK);

        motion.sync(10, 7, 0, true, 3, false);
        assert!(motion.settled(), "kaydırma animasyon başlattı");
        assert_eq!(motion.position(), Some([10.0, 7.0]));

        // Aynı satır değişimi ofset **sabitken** animasyonlu.
        motion.sync(10, 4, 0, true, 3, false);
        assert!(!motion.settled(), "imlecin kendi hareketi snap'ledi");
    }

    #[test]
    fn finishing_a_flight_settles_it_at_the_target() {
        // Örtülen pencerenin kolu: link duruyor, yani `advance` bir daha
        // koşmayacak. Bitirilmeseydi durum sonsuza kadar "yerleşmemiş"
        // kalır ve süreli koşu kod doğruyken `MotionUnsettled` derdi.
        let mut motion = moving();
        motion.advance(TICK);
        assert!(!motion.settled());

        motion.finish();
        assert!(motion.settled(), "bitirilen kayma yerleşmedi");
        assert_eq!(motion.position(), Some([10.0, 4.0]));

        // Örtülme kalkınca gelen içerik karesi aynı hedefi bildiriyor:
        // animasyon yeniden başlamamalı.
        motion.sync(10, 4, 0, true, 0, false);
        assert!(motion.settled(), "görünürlük dönüşü animasyon başlattı");
    }

    #[test]
    fn geometry_and_visibility_snap() {
        // Geometri: pencere/font/zoom oynadı, ızgara kaydı.
        let mut motion = moving();
        motion.sync(3, 1, 0, true, 0, true);
        assert!(motion.settled(), "geometri animasyon başlattı");
        assert_eq!(motion.position(), Some([3.0, 1.0]));

        // Görünmezlik durumu boşaltır; geri açılan imleç yeni yerinde doğar.
        // TUI'ler tam bunu yapıyor: çizerken imleci gizleyip taşıyorlar.
        motion.sync(3, 1, 0, false, 0, false);
        assert!(motion.settled());
        assert_eq!(motion.position(), None, "görünmez imleç konum verdi");
        motion.sync(40, 20, 0, true, 0, false);
        assert!(motion.settled(), "görünürlük dönüşü animasyon başlattı");
        assert_eq!(motion.position(), Some([40.0, 20.0]));
    }

    #[test]
    fn snap_never_starts_an_animation() {
        // Stilin tanımı: kayma yok. Sonucu yalnız görsel değil muhasebe de —
        // `settled()` hiç `false` olmadığı için link "hasar yok" dalında
        // uyuyor, yani `hareket=0`.
        let mut motion = moving_with(CursorMotion::Snap);
        assert!(motion.settled(), "snap animasyon başlattı");
        assert_eq!(motion.position(), Some([10.0, 4.0]));

        // Uçuşun her ihtimali: uzak sıçrama, tek hücre, aynı hücre.
        for (col, row) in [(400, 0), (11, 4), (11, 4), (0, 0)] {
            motion.sync(col, row, 0, true, 0, false);
            assert!(motion.settled(), "({col},{row}) snap'te animasyon başlattı");
            assert_eq!(motion.position(), Some([f32::from(col), f32::from(row)]));
        }
    }

    #[test]
    fn ease_takes_the_same_time_at_every_distance() {
        // `ease`'in yaydan ayrıldığı tek yer bu: süre mesafeden bağımsız.
        // Yay 1 hücrede ~230 ms, 400 hücrede ~460 ms harcıyor (`OMEGA`).
        for (col, row) in [(1, 0), (200, 60), (10, 4)] {
            let mut motion = moving_with(CursorMotion::Ease);
            motion.sync(0, 0, 0, true, 0, true);
            motion.sync(col, row, 0, true, 0, false);
            assert!(!motion.settled(), "hedef değişimi animasyon başlatmadı");
            let frames = run_to_rest(&mut motion, TICK);
            let expected = (EASE_DURATION / TICK).ceil() as u32;
            assert_eq!(
                frames, expected,
                "({col},{row}) sabit sürede yerleşmedi: {frames} kare"
            );
            assert_eq!(
                motion.position(),
                Some([f32::from(col), f32::from(row)]),
                "yerleşen imleç tam hücreye oturmadı"
            );
        }
    }

    #[test]
    fn ease_approaches_the_target_without_overshooting() {
        // Taşmanın **yapısal** olarak imkânsız olduğu stil: konum
        // `from → target` arasında monoton ilerliyor, hız entegre edilmiyor.
        // Yaydaki kırpmanın karşılığı burada bir sınama, bir dal değil.
        let mut motion = moving_with(CursorMotion::Ease);
        let mut last = 0.0;
        for _ in 0..40 {
            motion.advance(TICK);
            let pos = motion.position().expect("uçuşta")[0];
            assert!(pos >= last, "ease geri gitti: {last} → {pos}");
            assert!(pos <= 10.0, "ease hedefi aştı: {pos}");
            last = pos;
        }
    }

    #[test]
    fn ease_settles_well_inside_the_ceiling() {
        // `ease`'in durma koşulu kendi saati, yani süre tavanı ona
        // uygulanmıyor; sıra bozulursa kemer meşru bir kaymayı keserdi.
        // Sabitlerin yanındaki `const` assert bunu derlemeye, bu sınama da
        // gerçek bir koşuya bağlıyor.
        let mut motion = moving_with(CursorMotion::Ease);
        run_to_rest(&mut motion, TICK);
        let elapsed = motion.state.expect("yerleşti").elapsed;
        assert!(elapsed < TIME_CEILING, "tavan ease'i kesti: {elapsed}s");
    }

    #[test]
    fn switching_to_snap_finishes_the_flight_and_asks_for_a_frame() {
        // Kayma ortasında `snap`'e geçen kullanıcı: imleç ara hücrede asılı
        // kalamaz. Dönüş `true`, çünkü link'in "hasar yok" dalı yerleşmiş
        // animasyonda hiç çizmeden uyuyor — kareyi isteyen o dönüş.
        let mut motion = moving();
        motion.advance(TICK);
        assert!(!motion.settled());

        assert!(motion.set_style(CursorMotion::Snap), "kare istenmedi");
        assert!(motion.settled(), "snap'e geçiş kaymayı bitirmedi");
        assert_eq!(motion.position(), Some([10.0, 4.0]));

        // Yerleşmiş bir imleçte ve aynı stilde no-op: kare istemek boşa bir
        // uyandırma olurdu.
        assert!(
            !motion.set_style(CursorMotion::Snap),
            "aynı stil kare istedi"
        );
        assert!(
            !motion.set_style(CursorMotion::Spring),
            "yerleşmiş imleç kare istedi"
        );
    }

    #[test]
    fn switching_style_in_flight_does_not_teleport() {
        // Stil değişimi bir hedef değişimi değil: imleç bulunduğu yerden
        // devam etmeli. `ease` çıkış noktasını hatırladığı için asıl risk
        // orada — `from` tazelenmeseydi imleç eski başlangıca geri sıçrardı.
        for style in [CursorMotion::Ease, CursorMotion::Spring] {
            let mut motion = moving_with(match style {
                CursorMotion::Ease => CursorMotion::Spring,
                _ => CursorMotion::Ease,
            });
            for _ in 0..8 {
                motion.advance(TICK);
            }
            let before = motion.position().expect("uçuşta");
            assert!(before[0] > 0.0, "hiç ilerlemedi: {before:?}");

            assert!(!motion.set_style(style), "stil değişimi kare istedi");
            assert_eq!(motion.position(), Some(before), "stil değişimi ışınladı");
            // Ve kayma yeni stille sonlanıyor, takılmıyor.
            run_to_rest(&mut motion, TICK);
            assert_eq!(motion.position(), Some([10.0, 4.0]));
        }
    }

    #[test]
    fn a_long_spring_flight_does_not_teleport_when_the_style_changes() {
        // Stil değişiminin en sinsi hâli: yay `EASE_DURATION`'dan uzun
        // uçmuşken `ease`'e geçmek. Durma koşulu stile göre değiştiği için
        // "uçuşta mı" sorusu **eski** stille sorulmazsa kayma yerleşmiş
        // görünür, devir atlanır ve sıradaki kare imleci hedefe atar — üstelik
        // link o kareyi çizmeden uyuduğu için imleç ara hücrede kalır.
        let mut motion = moving();
        for _ in 0..25 {
            motion.advance(TICK);
        }
        let elapsed = motion.state.expect("uçuşta").elapsed;
        assert!(
            elapsed > EASE_DURATION,
            "senaryo kurulmadı: yay {elapsed}s uçtu, eşik {EASE_DURATION}s"
        );
        let before = motion.position().expect("uçuşta");
        assert!(!motion.settled(), "yay bu noktada yerleşmiş olmamalı");

        assert!(!motion.set_style(CursorMotion::Ease), "stil kare istedi");
        assert!(
            !motion.settled(),
            "stil değişimi kaymayı yerleşmiş gösterdi: ışınlama bir kare sonra"
        );
        motion.advance(TICK);
        let after = motion.position().expect("uçuşta");
        let remaining = 10.0 - before[0];
        assert!(
            after[0] - before[0] < remaining,
            "tek adımda hedefe atladı: {before:?} → {after:?}"
        );
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.position(), Some([10.0, 4.0]));
    }

    /// Hareketi Azalt açık, uçuşta bir belirme: `(0,0)` → `(10,4)`.
    fn fading() -> Motion {
        let mut motion = Motion::default();
        motion.set_reduce(true);
        motion.sync(0, 0, 0, true, 0, false);
        assert!(motion.settled(), "ilk kare belirme başlattı");
        motion.sync(10, 4, 0, true, 0, false);
        motion
    }

    #[test]
    fn reduce_motion_fades_in_place_instead_of_sliding() {
        // İndirgemenin tanımı: imleç **yeni hücresinde** belirir, yola
        // çıkmaz. Konumun `sync`'te oturması şart — bir kare sonra oturursa
        // imleç ilk içerik karesinde eski hücresinde alfa sıfırla çizilir ve
        // hiçbir sayaç bunu görmez.
        let mut motion = fading();
        assert_eq!(motion.position(), Some([10.0, 4.0]), "belirme kaydı");
        assert_eq!(motion.alpha(), 0.0, "belirme opak başladı");
        assert!(!motion.settled(), "belirme hiç başlamadı");

        // Opaklık monoton artıyor ve konum hiç oynamıyor.
        let mut last = 0.0;
        while !motion.settled() {
            motion.advance(TICK);
            let alpha = motion.alpha();
            assert!(alpha >= last, "belirme geri gitti: {last} → {alpha}");
            assert!(alpha <= 1.0, "opaklık 1'i aştı: {alpha}");
            assert_eq!(motion.position(), Some([10.0, 4.0]), "belirme kaydırdı");
            last = alpha;
        }
        assert_eq!(motion.alpha(), 1.0, "yerleşen belirme opak değil");

        // Süre **mesafeden bağımsız** ve `FADE_DURATION` kadar: `ease`'in
        // saatiyle aynı biçim, yalnız sabit ayrı. `run_to_rest` aradaki
        // duraksamayı da veriyor, yani bu hareket "ayrı bir hareket" sayılıp
        // yeniden beliriyor (`Motion::sync`'in duraksama ölçütü).
        for (col, row) in [(1u16, 0u16), (200, 60)] {
            let mut motion = fading();
            run_to_rest(&mut motion, TICK);
            motion.sync(col, row, 0, true, 0, false);
            let frames = run_to_rest(&mut motion, TICK);
            assert_eq!(
                frames,
                (FADE_DURATION / TICK).ceil() as u32,
                "({col},{row})"
            );
        }
    }

    #[test]
    fn reduce_motion_does_not_fade_what_did_not_move() {
        // Snap hâlleri belirmiyor: imleç hareket etmedi, **altındaki ızgara**
        // hareket etti (008 Karar 5). Tek başına saate bakan bir kural onları
        // `FADE_DURATION` boyunca yerleşmemiş sayar ve link hiçbir şeyi
        // değiştirmeyen kareler çizerdi — `ease`'in ikinci koşuluyla aynı
        // gerekçe, aynı kol.
        let mut motion = fading();
        run_to_rest(&mut motion, TICK);

        // Kaydırma, geometri ve görünürlük dönüşü: üçü de anında ve opak.
        motion.sync(10, 7, 0, true, 3, false);
        assert!(motion.settled(), "kaydırma belirme başlattı");
        assert_eq!(motion.alpha(), 1.0);
        motion.sync(3, 1, 0, true, 3, true);
        assert!(motion.settled(), "geometri belirme başlattı");
        assert_eq!(motion.alpha(), 1.0);
        motion.sync(3, 1, 0, false, 3, false);
        motion.sync(40, 20, 0, true, 3, false);
        assert!(motion.settled(), "görünürlük dönüşü belirme başlattı");
        assert_eq!(motion.alpha(), 1.0);

        // Aynı hücreye yeniden hedefleme de belirme değil.
        motion.sync(40, 20, 0, true, 3, false);
        assert!(motion.settled(), "yerinde duran imleç belirdi");
    }

    #[test]
    fn snap_outranks_reduce_motion() {
        // Ürün kararı: hareketi zaten kapatmış kullanıcıya erişilebilirlik
        // ayarı bir animasyon **eklemez**. Kip de bunu söylüyor: `Snap`
        // bayrağın üstünde.
        let mut motion = Motion::default();
        motion.set_style(CursorMotion::Snap);
        motion.set_reduce(true);
        motion.sync(0, 0, 0, true, 0, false);
        motion.sync(10, 4, 0, true, 0, false);
        assert!(motion.settled(), "snap + reduce animasyon başlattı");
        assert_eq!(motion.position(), Some([10.0, 4.0]));
        assert_eq!(motion.alpha(), 1.0, "snap imleci yarı saydam çizildi");
    }

    #[test]
    fn turning_reduce_motion_on_finishes_the_flight() {
        // Uçuşta bir kayma varken ayarın açılması: kip artık kaymıyor, yani
        // devralınacak bir şey yok. Dönüş `true`, çünkü link'in "hasar yok"
        // dalı yerleşmiş animasyonda hiç çizmeden uyuyor.
        let mut motion = moving();
        motion.advance(TICK);
        assert!(!motion.settled());

        assert!(motion.set_reduce(true), "kare istenmedi");
        assert!(motion.settled(), "açılış kaymayı bitirmedi");
        assert_eq!(motion.position(), Some([10.0, 4.0]));
        assert_eq!(motion.alpha(), 1.0, "bitirilen kayma yarı saydam kaldı");

        // Aynı değeri yeniden yazmak ve yerleşmiş imleç no-op.
        assert!(!motion.set_reduce(true), "aynı değer kare istedi");
        assert!(!motion.set_reduce(false), "yerleşmiş imleç kare istedi");
    }

    #[test]
    fn a_cursor_that_keeps_moving_still_becomes_visible_while_fading() {
        // `/code-review` bulgusu ve indirgemenin **tersine döndüğü** yer:
        // her hedef değişimi saati sıfırlasaydı hızla oynayan bir imleç
        // 90 ms'yi hiç dolduramaz, yani Hareketi Azalt imleci yanıp söner
        // (yazarken) ya da tamamen kaybederdi (akan çıktıda).
        //
        // Senaryo akan çıktı: her karede bir hücre ilerleyen imleç.
        let mut motion = fading();
        let mut col = 10;
        for _ in 0..30 {
            motion.advance(TICK);
            col += 1;
            motion.sync(col, 4, 0, true, 0, false);
        }
        assert_eq!(
            motion.alpha(),
            1.0,
            "akan çıktıda imleç saydam kaldı: opaklık {}",
            motion.alpha()
        );
        // Konum her zaman hedefte: belirme kaymıyor.
        assert_eq!(motion.position(), Some([f32::from(col), 4.0]));
        // Ve yerleşiyor — link bu imleç için sonsuza uyanık kalmıyor.
        assert!(motion.settled(), "belirme yerleşmedi");

        // Akışın içindeki bir sonraki hareket de belirmeyi tazelemiyor:
        // 90 ms'de bir yeniden belirmek ~11 Hz'de bir titreme demekti.
        motion.advance(TICK);
        motion.sync(col + 1, 4, 0, true, 0, false);
        assert_eq!(
            motion.alpha(),
            1.0,
            "akıştaki hareket yeniden belirdi (titreme)"
        );
        assert!(motion.settled(), "akıştaki hareket link'i uyandırdı");

        // **Duraksamadan sonraki** hareket ayrı bir harekettir ve beliriyor:
        // indirgemenin sözü burada duruyor. Uykudan uyanan link'in ilk karesi
        // de buraya düşüyor — `dt` `DT_MAX`'e kırpılıyor ve kırpma belirmeden
        // uzun (dosya başındaki `const _`).
        motion.advance(DT_MAX);
        motion.sync(col + 2, 4, 0, true, 0, false);
        assert_eq!(
            motion.alpha(),
            0.0,
            "duraksamadan sonraki hareket belirmedi"
        );
        assert!(!motion.settled(), "belirme hiç başlamadı");
    }

    #[test]
    fn reduce_off_mid_fade_does_not_slide_backwards() {
        // Kapanışın devralma kolu olsaydı en sinsi kusur burada olurdu:
        // `from` bir önceki hücrede duruyor ve `ease` konumu ondan yeniden
        // hesaplıyor — imleç geldiği hücreye dönüp yeniden kayardı. `spring`
        // ise hedefinde ve hızsız olduğu için anında yerleşir, yani yarı
        // saydam imleci ekranda bırakırdı.
        for style in [CursorMotion::Ease, CursorMotion::Spring] {
            let mut motion = fading();
            motion.set_style(style);
            for _ in 0..4 {
                motion.advance(TICK);
            }
            let alpha = motion.alpha();
            assert!(alpha > 0.0 && alpha < 1.0, "senaryo kurulmadı: {alpha}");

            assert!(motion.set_reduce(false), "{style:?}: kare istenmedi");
            assert!(motion.settled(), "{style:?}: belirme bitmedi");
            assert_eq!(motion.position(), Some([10.0, 4.0]), "{style:?}: ışınlandı");
            assert_eq!(motion.alpha(), 1.0, "{style:?}: yarı saydam kaldı");
        }
    }

    #[test]
    fn style_change_during_a_fade_keeps_fading() {
        // Belirme kipinde `ease` ↔ `spring` hiçbir şey: devralma kolu
        // koşsaydı `from` bulunulan konuma çekilir, `from == target` olur ve
        // belirme **yerleşmiş** görünürdü — link o kareyi çizmeden uyar,
        // imleç yarı saydam asılı kalırdı.
        for style in [CursorMotion::Ease, CursorMotion::Snap] {
            let mut motion = fading();
            for _ in 0..3 {
                motion.advance(TICK);
            }
            let before = motion.alpha();
            assert!(before > 0.0 && before < 1.0, "senaryo kurulmadı: {before}");

            let asked = motion.set_style(style);
            if style == CursorMotion::Snap {
                // `snap` belirmeyi de bitirir ve kareyi ister: stil "animasyon
                // yok" diyor ve yarım kalmış bir opaklık da animasyondur.
                assert!(asked, "snap'e geçiş kare istemedi");
                assert!(motion.settled());
                assert_eq!(motion.alpha(), 1.0);
            } else {
                assert!(!asked, "{style:?}: belirme kare istedi");
                assert!(!motion.settled(), "{style:?}: belirme yerleşmiş göründü");
                assert_eq!(motion.alpha(), before, "{style:?}: opaklık sıçradı");
                run_to_rest(&mut motion, TICK);
                assert_eq!(motion.alpha(), 1.0);
            }
        }
    }

    #[test]
    fn a_flight_that_settles_early_stays_settled_in_every_mode() {
        // `/code-review` bulgusu. `advance`'ın yerleşme kolu `pos` ve `vel`'i
        // hedefe çekiyordu ama `from`'u **bıraktığı yerde** — oysa `ease` ile
        // `fade`'in durma koşulu tam olarak `from == target`'a ("gidecek yol
        // yok") bakıyor. Yayın eşiğiyle erkenden oturan bir kayma bu yüzden
        // başka bir kipte yerleşmemiş görünürdü ve asıl bedel şu: link o
        // kareyi zaten "yerleşti" diye **uyumuş** oluyor, yani `advance` bir
        // daha koşmuyor. Sonuç, süreli koşuda var olmayan bir animasyon
        // yüzünden `MotionUnsettled`, belirmede de sebepsiz yarı saydam bir
        // imleç.
        //
        // Senaryo taşma kırpması: hedefi hızın hemen önüne koymak yayı **ilk
        // adımda** oturtuyor, yani `elapsed` iki sürenin de çok altında.
        let mut motion = moving();
        for _ in 0..6 {
            motion.advance(TICK);
        }
        let pos = motion.position().expect("uçuşta")[0];
        let vel = motion.state.expect("uçuşta").vel[0];
        motion.state = Some(State {
            pos: [pos, 0.0],
            vel: [vel, 0.0],
            from: [pos, 0.0],
            target: [pos + 0.5, 0.0],
            elapsed: 0.0,
            since_move: 0.0,
        });
        motion.advance(TICK);
        assert!(motion.settled(), "yay taşma kırpmasıyla oturmadı");
        let elapsed = motion.state.expect("yerleşti").elapsed;
        // İki süreden **küçüğü**: hangisinin küçük olduğu bu sınamanın
        // iddiası değil ve `min` onu sabitlerin sırasına bağlamadan yazıyor.
        assert!(
            elapsed < FADE_DURATION.min(EASE_DURATION),
            "senaryo kurulmadı: {elapsed}s iki sürenin altında değil"
        );

        // Yerleşmiş bir kayma hiçbir kipte "uçuşta" görünmemeli — ve bu iki
        // setter'in kare istememesinin de şartı: istemedikleri kareyi
        // çizecek kimse yok.
        let mut eased = motion;
        assert!(!eased.set_style(CursorMotion::Ease), "kare istendi");
        assert!(eased.settled(), "yerleşmiş kayma `ease`'de uçuşta göründü");

        let mut faded = motion;
        assert!(!faded.set_reduce(true), "kare istendi");
        assert!(faded.settled(), "yerleşmiş kayma belirmede uçuşta göründü");
        assert_eq!(faded.alpha(), 1.0, "yerleşmiş imleç yarı saydam çizildi");
    }

    /// Bir Enter'ın kare çifti: 30 satırlık ızgarada imleç 2. satırda ve
    /// içerik üç satır (öteleme 27), sonra imleç 3. satıra iniyor ve içerik
    /// dört satır oluyor (öteleme 26). İmlecin **ekran** satırı ikisinde de
    /// 29 — dipteki satır.
    fn after_enter() -> Motion {
        let mut motion = Motion::default();
        motion.sync(0, 2, 27, true, 0, false);
        assert!(motion.settled(), "ilk kare animasyon başlattı");
        motion.sync(0, 3, 26, true, 0, false);
        motion
    }

    #[test]
    fn the_cursor_does_not_move_while_the_origin_slides() {
        // **R2.1, setin can alıcı yeri.** Enter'da grid satırı `r → r+1`
        // olurken öteleme bir azalıyor; iki hedef aynı uzayda olmasaydı imleç
        // bir satır düşüp yay onu geri bindirirdi (phase-1'in bilinen ara
        // durumu). Ekran uzayında hedef **hiç** değişmiyor.
        let mut motion = after_enter();
        assert!(motion.cursor_settled(), "imleç Enter'da yola çıktı");
        assert_eq!(motion.position(), Some([0.0, 29.0]), "imleç dipte değil");
        assert!(!motion.origin_settled(), "öteleme kaymaya başlamadı");

        // Öteleme kayarken imleç ekranda **hiç** oynamıyor: iki animatör tek
        // `settled()` kapısında ama tek başlarına.
        while !motion.settled() {
            motion.advance(TICK);
            assert_eq!(motion.position(), Some([0.0, 29.0]), "imleç kaydı");
        }
        assert_eq!(motion.origin(), 26.0, "öteleme hedefine oturmadı");
    }

    #[test]
    fn the_origin_settles_and_then_lets_the_link_sleep() {
        // Checklist: "kayma yerleştikten sonra kare istenmiyor". Link'in uyku
        // kararı tek ifade (`link.rs`'in "hasar yok" dalı): `motion.settled()`.
        // Öteleme o kapının **içinde** (R2.5), yani kayma bitince kare de
        // bitiyor — boşta sıfır kare sözleşmesi ayakta.
        let mut motion = after_enter();
        assert!(!motion.settled(), "kayma link'i uyandırmadı");

        // Kayma **ilerliyor**: iki uç arasında bir yerde. Sınama bunu sormasa
        // hiç kaymayan bir kod da geçerdi.
        motion.advance(TICK);
        let mid = motion.origin();
        assert!(mid < 27.0 && mid > 26.0, "öteleme kaymadı: {mid}");

        let frames = run_to_rest(&mut motion, TICK);
        assert!(
            frames < u32::try_from((TIME_CEILING / TICK).ceil() as i64 + 2).unwrap(),
            "kayma süre tavanını aştı: {frames} kare"
        );
        assert_eq!(motion.origin(), 26.0);
        // Ve yerleşen kayma bir daha uyanmıyor: aynı hedefi bildiren içerik
        // kareleri (renk değişimi, imleç yanıp sönmesi) kaymayı yeniden
        // başlatmamalı.
        motion.sync(0, 3, 26, true, 0, false);
        assert!(motion.settled(), "aynı hedef kaymayı yeniden başlattı");
    }

    #[test]
    fn a_growing_origin_slides_and_a_shrinking_one_snaps() {
        // **Yön kuralı** (011 kapı sonrası, gözle kontrol): öteleme
        // `rows - content_rows`, yani hedefin **düşmesi** içeriğin büyümesi
        // (grid yukarı akar) ve **yükselmesi** daralması (grid aşağı iner).
        // Yukarı akış içeriğin gelmesi gibi okunuyor, aşağı iniş düşmesi gibi.
        let mut motion = after_enter();
        run_to_rest(&mut motion, TICK);

        // vim'e giriş: doluluk bir hamlede `rows`'a fırlıyor, öteleme 0'a
        // **düşüyor** — arayüz süzülerek geliyor ve bu isteniyor.
        motion.sync(0, 0, 0, true, 0, false);
        assert!(!motion.origin_settled(), "büyüyen içerik snap'lendi");
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.origin(), 0.0);

        // vim'den çıkış: doluluk daralıyor, öteleme **yükseliyor** — kabuk
        // aşağı süzülmüyor, anında yerine oturuyor.
        motion.sync(0, 3, 26, true, 0, false);
        assert!(motion.origin_settled(), "daralan içerik kaydı");
        assert_eq!(motion.origin(), 26.0);

        // Dolu ekranda `clear` aynı sınıf: öteleme tepeden dibe yükseliyor.
        motion.sync(0, 0, 0, true, 0, false);
        run_to_rest(&mut motion, TICK);
        motion.sync(0, 0, 29, true, 0, false);
        assert!(motion.origin_settled(), "clear kayarak indi");
        assert_eq!(motion.origin(), 29.0);
    }

    #[test]
    fn scrolling_and_geometry_snap_the_origin() {
        // R2.6 ve `docs/AYARLAR.md`'nin "Izgaranın başka sebeple yer
        // değiştirmesi kaymaz" maddesi: tekerlek parmağı takip eder (008
        // Karar 5), pencere/font/punto değişimi de ızgarayı animasyonsuz
        // taşır. İkisinde de içerik kendi büyümesiyle yükselmedi.
        let mut motion = after_enter();
        run_to_rest(&mut motion, TICK);

        motion.sync(0, 3, 20, true, 1, false);
        assert!(motion.settled(), "kaydırma kayma başlattı");
        assert_eq!(motion.origin(), 20.0);

        motion.sync(0, 3, 10, true, 1, true);
        assert!(motion.settled(), "geometri kayma başlattı");
        assert_eq!(motion.origin(), 10.0);

        // Ama ofset **sabitken** aynı değişim kayıyor: snap'i doğuran şey
        // hedefin kendisi değil, ızgaranın başka bir sebeple oynaması.
        motion.sync(0, 3, 9, true, 1, false);
        assert!(!motion.origin_settled(), "içerik büyümesi snap'ledi");
    }

    #[test]
    fn a_hidden_cursor_does_not_freeze_the_origin() {
        // `sync`'in `!visible` erken dönüşü ötelemeyi de atlasaydı, imleci
        // gizleyip çıktı akıtan bir betikte içerik yanlış yerde donardı.
        // İmlecin görünürlüğü ızgaranın nerede durduğuna karar veremez.
        let mut motion = after_enter();
        run_to_rest(&mut motion, TICK);

        motion.sync(0, 4, 25, false, 0, false);
        assert_eq!(motion.position(), None, "görünmez imleç konum verdi");
        assert!(
            !motion.origin_settled(),
            "görünmez imleç ötelemeyi dondurdu"
        );
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.origin(), 25.0);
    }

    #[test]
    fn a_shrinking_content_settles_too() {
        // **R2.4:** girdisi monoton değil — imleci yukarı taşıyıp alt satırı
        // `\e[K` ile silen bir program ötelemeyi büyütüp küçültebilir. Durma
        // koşulu hedefe değil mesafeye baktığı için her yeni hedef tek başına
        // sonlu; salınımın kendisi sürerse o kareleri isteyen şey animasyon
        // değil, salınımı üreten çıktının hasarı olur.
        let mut motion = after_enter();
        let mut previous = 26u16;
        for origin in [26u16, 27, 25, 27, 26] {
            motion.advance(TICK);
            motion.sync(0, 29 - origin, origin, true, 0, false);
            // İddia **yerleşmektir**, kaç kare koştuğu değil: yön kuralından
            // beri salınımın iki yarısı iki yoldan geçiyor — daralan yön
            // (hedef yükseliyor) hiç kare koşmadan oturuyor, büyüyen yön
            // kayarak. R2.4'ün istediği ikisinin de **sonlu** olması.
            if origin > previous {
                assert!(
                    motion.origin_settled(),
                    "daralan içerik ({previous} → {origin}) kaydı"
                );
            }
            run_to_rest(&mut motion, TICK);
            assert!(motion.origin_settled(), "öteleme {origin} yerleşmedi");
            assert_eq!(motion.origin(), f32::from(origin));
            previous = origin;
        }
    }

    #[test]
    fn reduce_motion_snaps_the_origin_instead_of_fading_it() {
        // **R2.3.** İmleç belirirken öteleme snap'liyor: her yeni satırda
        // bütün ekranın belirmesi, indirgemenin kaldırmaya çalıştığı
        // hareketten beter olurdu. "İndirgemenin tek yeri `bt-gpu::motion`"
        // kuralı yerinde — **yer** aynı, **kip** iki.
        let mut motion = Motion::default();
        motion.set_reduce(true);
        motion.sync(0, 2, 27, true, 0, false);
        // İmleç **sütun da** değiştiriyor: Enter'da ekran satırı hiç
        // oynamadığı için (R2.1) tek başına bir satır ilerlemesi belirme de
        // doğurmaz — sınamanın iki kipi ayırt edebilmesi için imlecin
        // gerçekten yer değiştirmesi gerek.
        motion.sync(5, 3, 26, true, 0, false);
        assert_eq!(motion.origin(), 26.0, "öteleme belirmeye kalktı");
        assert!(motion.origin_settled(), "öteleme kaydı");
        // İmleç ise belirmenin içinde: aynı karede iki ayrı kip.
        assert_eq!(motion.alpha(), 0.0, "imleç belirmedi");
        assert!(!motion.cursor_settled());

        // Uçuştaki bir kayma varken ayarın açılması onu **hedefinde**
        // bitiriyor: kip artık kaymıyor, devralınacak bir şey yok.
        let mut motion = after_enter();
        motion.advance(TICK);
        assert!(!motion.origin_settled());
        assert!(motion.set_reduce(true), "kare istenmedi");
        assert_eq!(motion.origin(), 26.0, "açılış kaymayı bitirmedi");
    }

    #[test]
    fn snap_style_never_slides_the_origin() {
        // **R2.2:** kayma `cursor_motion`'ı izliyor, yeni anahtar yok.
        // `"snap"`ın "hareketi tamamen kapatmanın yolu bu" sözü bu satırda
        // duruyor — `docs/AYARLAR.md` onu yazıyor.
        let mut motion = Motion::default();
        motion.set_style(CursorMotion::Snap);
        motion.sync(0, 2, 27, true, 0, false);
        motion.sync(0, 3, 26, true, 0, false);
        assert!(motion.settled(), "snap kayma başlattı");
        assert_eq!(motion.origin(), 26.0);

        // Kayma ortasında `"snap"`e geçmek de onu hedefinde bitiriyor ve kare
        // istiyor: link yerleşmiş animasyonda hiç çizmeden uyuyor.
        let mut motion = after_enter();
        motion.advance(TICK);
        assert!(motion.set_style(CursorMotion::Snap), "kare istenmedi");
        assert!(motion.settled());
        assert_eq!(motion.origin(), 26.0);
    }

    #[test]
    fn switching_style_mid_slide_does_not_teleport_the_origin() {
        // İmlecinkiyle aynı kural (`switching_style_in_flight_does_not_teleport`):
        // stil değişimi bir hedef değişimi değil, ızgara bulunduğu yerden
        // devam etmeli. `ease` çıkış noktasını hatırladığı için asıl risk
        // orada — `from` tazelenmeseydi içerik eski başlangıcına geri
        // sıçrardı.
        let mut motion = after_enter();
        for _ in 0..8 {
            motion.advance(TICK);
        }
        let before = motion.origin();
        assert!(
            before < 27.0 && before > 26.0,
            "senaryo kurulmadı: {before}"
        );

        motion.set_style(CursorMotion::Ease);
        assert_eq!(motion.origin(), before, "stil değişimi ışınladı");
        run_to_rest(&mut motion, TICK);
        assert_eq!(motion.origin(), 26.0);
    }

    #[test]
    fn covering_the_window_settles_the_slide_too() {
        // `Motion::finish`'in öteleme yarısı. Örtülmede link duruyor, yani
        // `advance` bir daha koşmuyor; yarısı bitirilseydi `settled()` sonsuza
        // kadar `false` kalır ve süreli koşu **kod doğruyken**
        // `MotionUnsettled` derdi.
        let mut motion = after_enter();
        motion.advance(TICK);
        assert!(!motion.settled());

        motion.finish();
        assert!(motion.settled(), "bitirilen kayma yerleşmedi");
        assert_eq!(motion.origin(), 26.0);
    }

    #[test]
    fn a_hidden_cursor_keeps_the_scroll_history() {
        // Ofset `state`'ten ayrı yaşamalı: TUI imleci gizler, pencereyi
        // kaydırır, sonra geri açar. Ofset boşalsaydı geri açılan imleç
        // "kaydırma olmadı" der ve kaydırmayı animasyon sanırdı — burada
        // zaten snap olduğu için belirti yok, ama ters yönde (gizliyken
        // kaydırma **olmadığında**) yanlış snap üretirdi.
        let mut motion = Motion::default();
        motion.sync(0, 0, 0, false, 5, false);
        motion.sync(0, 0, 0, true, 5, false);
        assert_eq!(motion.offset, Some(5));
    }
}
