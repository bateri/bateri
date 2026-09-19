//! İmlecin yanıp sönmesi — **saf**, ObjC'siz, kilitsiz.
//!
//! Emsali [`crate::motion::Motion`] ve `Gate`: politikanın kendisi platformdan
//! bağımsız olduğu için ayrı bir tipte yaşıyor ve gerçek bir pencere olmadan
//! sınanıyor.
//!
//! **Blink bir hareket karesidir, içerik karesi değil.** Izgara değişmiyor,
//! yalnız caret'in alfası; `link.rs`'in modül başlığındaki üç şarttan
//! birincisini ("içerik gerçekten değişecek") geçemiyor. Ama ekran hızına da
//! bağlanamaz: 2 Hz'lik bir değişim için tazeleme hızında kare, setin bütün
//! gerekçesini çürütürdü. Kalan yol saatin ikinci tadı — hasar dikmeyen bir
//! uyandırma ([`crate::link::Waker::resume`]).
//!
//! **Faz mutlak son tarih tutuyor, `dt` biriktirmiyor** ve bu şart:
//! [`crate::motion::DT_MAX`] kırpması yüzünden 500 ms'lik bir uykuyu 100 ms
//! sayan bir birikim, imleci ~5 uyandırmada bir döndürür ve arada dört
//! **birebir aynı** kare çizdirirdi. Belirtisi sessiz olurdu: pencere uyanır,
//! çizer, hiçbir piksel değişmez.
//!
//! **Durma koşulu adlandırılmış** (`CLAUDE.md`): uygulama ya da kullanıcı
//! kapatır, imleç gizlenir, pencere örtülür (`Gate` zaten `setPaused`'a
//! düşüyor) ya da son içerik karesinden [`IDLE_STOP`] geçer. Durma **fazı
//! açığa bırakıyor**: sönük fazda durulsaydı imleç bir sonraki hasara kadar
//! kaybolurdu ve kullanıcı bunu "imleç kayboldu" diye okurdu.

/// Yarım periyot, saniye — **seçilmiş bir sayı, ölçülmüş değil**.
///
/// Tam devir 1 saniye, yani saniyede **iki** kare: biri yakan, biri söndüren.
/// Klasik terminal ritmi bu mertebede (xterm 600/300 ms, VS Code 500 ms) ve
/// sayının kendisi bir ölçüme değil o hedefe dayanıyor.
///
/// Periyodu kısaltmanın bedeli doğrusal: 250 ms'lik bir yarım periyot saniyede
/// dört kare eder. Uzatmanın bedeli yok ama imleç "yanıp sönüyor" gibi
/// okunmaz olur.
/// **Varsayılanın tek sahibi `bt-core`** (016 R2): periyot artık bir ayar
/// (`[terminal] cursor_blink_interval`) ve iki literal olsaydı dosyasız
/// kullanıcı ile süreli koşu iki ayrı ritme bağlanırdı.
const HALF_PERIOD: f64 = bt_core::CURSOR_BLINK_INTERVAL;

/// Klavye sessizliğinden sonra blink'in durma süresi, saniye — **seçilmiş,
/// ölçülmüş değil**; kaynağı kitty'nin `cursor_stop_blinking_after`
/// varsayılanı (15 sn).
///
/// Bu sabit blink'i bu deponun merkezî vaadiyle barıştıran şey: onsuz, açık
/// bir blink pencereyi **kalıcı olarak** boşta-değil yapardı. Onunla pencere
/// yazmayı bıraktıktan 15 saniye sonra gerçekten sıfır kareye dönüyor.
///
/// Tabanı **son içerik karesi**, son çizilen kare değil: blink kareleri de
/// çiziliyor ve onlara bakan bir sayaç hiç dolmazdı (`link.rs`'in
/// `last_frame_at`'i hareket kolunda da yazılıyor).
const IDLE_STOP: f64 = 15.0;

/// İmlecin yanıp sönmesinin durumu.
///
/// Zaman tabanı display link'in damgası ([`crate::link`]'in `now`'ı), saat
/// okuması değil: `sessiz=` ile animasyonun saati zaten oradan okunuyor ve
/// ikinci bir taban iki ayrı zaman yaratırdı.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Blink {
    /// Kullanıcı ve uygulama birlikte "sönsün" diyor mu
    /// (`bt_core::CursorBlink::resolve`'un cevabı).
    enabled: bool,
    /// Faz şu an **açık** mı. Kapalıyken de `true`: durma fazı açığa bırakıyor.
    lit: bool,
    /// Bir sonraki faz değişiminin **mutlak** zamanı; `None` → bekleyen tik yok.
    next_flip: Option<f64>,
    /// Son **içerik** karesinin damgası; [`IDLE_STOP`]'un tabanı.
    last_content_at: Option<f64>,
    /// Yarım periyot, saniye — ayardan geliyor
    /// (`[terminal] cursor_blink_interval`).
    ///
    /// Alan, `const` değil: kullanıcı kayıt anında değiştirebiliyor. Modülün
    /// saflığı bozulmuyor — `Blink` hâlâ `Copy` ve `Cell` içinde yaşıyor,
    /// emsal `Motion::set_style`.
    half_period: f64,
}

impl Default for Blink {
    /// **`lit` `true` başlıyor**, `derive`'ın `false`'u değil: alanın
    /// değişmezi "kapalıyken de açık" ve `derive` onu doğduğu anda çiğnerdi.
    /// Bugün [`Blink::alpha`] `enabled`'a kısa devre yaptığı için belirti
    /// görünmüyor; değişmezin lisans verdiği bir sadeleştirme (`alpha`'yı
    /// yalnız `lit`'e bağlamak) pencereyi **görünmez bir caret'le** doğururdu.
    fn default() -> Self {
        Self {
            enabled: false,
            lit: true,
            next_flip: None,
            last_content_at: None,
            half_period: HALF_PERIOD,
        }
    }
}

impl Blink {
    /// İçerik karesi: ayarı tazeler ve hareketsizlik sayacını sıfırlar.
    ///
    /// **Fazı sıfırlamıyor** ve bu bilerek: 013'ün canlı sayacı koşan komut
    /// boyunca saniyede bir içerik karesi üretiyor, yani faz her içerik
    /// karesinde açığa çekilseydi blink'in ritmi komut koşarken bozulurdu.
    /// Bedeli adlandırılmış: karanlık fazda basılan tuş imleci en çok yarım
    /// periyot bekletir.
    pub(crate) fn content_frame(&mut self, now: f64, enabled: bool) {
        self.last_content_at = Some(now);
        if self.enabled != enabled {
            self.enabled = enabled;
            // Kapanış fazı **açığa** bırakıyor (R9.1); açılış bir sonraki
            // yarım periyottan başlıyor.
            self.lit = true;
            self.next_flip = enabled.then_some(now + self.half_period);
        } else if enabled && self.next_flip.is_none() {
            // Hareketsizlikten dönüş: sayaç yukarıda tazelendi, tik yeniden
            // kuruluyor.
            self.next_flip = Some(now + self.half_period);
        }
    }

    /// Caret kıpırdadı: fazı **açığa** çekip sayacı baştan başlatır.
    ///
    /// **Yazarken imleç sönmez** ve bu kullanıcı bildirimiyle geldi
    /// (2026-09-19): tuşa basarken imlecin bir yandan sönüp yanması "yazma ile
    /// blink'in aynı anda olması" diye okundu ve haklı — her editör ve terminal
    /// yazarken caret'i sabit tutar, duraksayınca sönmeye döner.
    ///
    /// **Tetik caret'in hareketi**, tuş vuruşunun kendisi değil ve bu bilerek:
    /// tuş `bt-shell`'den `bt-gpu`'ya ayrı bir sinyal isterdi, oysa hareket
    /// zaten bu modülün elinde. Ayrım da doğru yerde duruyor — koşan bir
    /// komutun süre sayacı caret'i **kıpırdatmıyor**, yani `sleep 5` boyunca
    /// blink bozulmadan sürüyor; akan çıktı ise kıpırdatıyor ve orada caret'in
    /// sabit kalması zaten istenen.
    pub(crate) fn wake(&mut self, now: f64) {
        if !self.enabled {
            return;
        }
        self.lit = true;
        self.next_flip = Some(now + self.half_period);
    }

    /// Zamanı ilerletir; dönen değer **bu karede faz değişti mi**.
    ///
    /// Tek atımlık ve `link.rs`'in uyku testinde `motion.settled()`'ın erken
    /// dönüşünden **önce** tüketiliyor: sorulmasaydı hasar dikmeyen uyandırma
    /// kare üretmeyen bir uyan/uyu fırdöndüsü yaratırdı.
    ///
    /// Uzun uykudan dönüşte faz **tek adımda** doğru yere oturuyor: aradaki
    /// geçmiş tikler atlanıyor, biriktirilmiyor.
    pub(crate) fn advance(&mut self, now: f64) -> bool {
        if !self.enabled {
            return false;
        }
        // Hareketsizlik: durma koşulu. Faz açığa çekiliyor ve tik sönüyor;
        // `content_frame` ilk hasarda ikisini de geri kuruyor.
        if self.last_content_at.is_some_and(|at| now - at >= IDLE_STOP) {
            let was_dark = !self.lit;
            self.lit = true;
            self.next_flip = None;
            return was_dark;
        }
        let Some(due) = self.next_flip else {
            return false;
        };
        if now < due {
            return false;
        }
        self.lit = !self.lit;
        // **Mutlak**, `due + HALF_PERIOD` değil `now + HALF_PERIOD`: uzun bir
        // uykudan sonra geçmişte kalmış bir tabandan saymak, arka arkaya
        // birkaç tiki hemen ateşlerdi.
        self.next_flip = Some(now + self.half_period);
        true
    }

    /// Periyodu değiştirir ve bekleyen tiki **yeniden kurar**.
    ///
    /// Yeniden kurmak şart, çünkü [`Blink::next_flip`] **mutlak** bir son
    /// tarih: yalnız alanı yazmak, kaydedilen yeni ritmin bir flip **gecikmesi**
    /// demek olurdu — kullanıcı kaydeder, hiçbir şey olmaz, sonraki sönmede
    /// birden değişir. Aynı değerde hiçbir şey yapılmıyor, yoksa her ayar
    /// kaydı fazı sıfırlardı.
    pub(crate) fn set_half_period(&mut self, now: f64, half_period: f64) {
        if self.half_period == half_period {
            return;
        }
        self.half_period = half_period;
        if self.next_flip.is_some() {
            self.next_flip = Some(now + half_period);
        }
    }

    /// Caret'in bu karedeki opaklığı; blink kapalıyken **her zaman `1.0`**.
    pub(crate) fn alpha(self) -> f32 {
        if self.lit || !self.enabled { 1.0 } else { 0.0 }
    }

    /// Bir sonraki faz değişiminin mutlak zamanı — saatin blink yarısı.
    pub(crate) fn next_flip(self) -> Option<f64> {
        self.enabled.then_some(self.next_flip).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disabled_blink_never_flips() {
        let mut blink = Blink::default();
        blink.content_frame(0.0, false);
        assert!(!blink.advance(10.0), "kapalı blink faz değiştirdi");
        assert_eq!(blink.alpha(), 1.0);
        assert_eq!(blink.next_flip(), None, "kapalı blink saat kuruyor");
    }

    #[test]
    fn the_phase_is_an_absolute_deadline() {
        // **`dt` biriktiren bir uygulama burada düşerdi.** Uzun bir uykudan
        // dönüşte faz tek adımda dönüyor ve bir sonraki tik `now`'dan
        // sayılıyor — geçmiş bir tabandan değil, yoksa arka arkaya birkaç tik
        // hemen ateşlerdi.
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);
        assert_eq!(blink.next_flip(), Some(HALF_PERIOD));
        assert!(blink.advance(5.0), "uzun uykudan sonra faz dönmedi");
        assert_eq!(blink.alpha(), 0.0);
        assert_eq!(blink.next_flip(), Some(5.0 + HALF_PERIOD));
        // Aynı anda ikinci kez sorulunca dönmüyor: tik tek atımlık.
        assert!(!blink.advance(5.0), "faz aynı karede iki kez döndü");
    }

    #[test]
    fn a_lit_and_a_dark_phase_alternate() {
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);
        assert_eq!(blink.alpha(), 1.0);
        assert!(blink.advance(HALF_PERIOD));
        assert_eq!(blink.alpha(), 0.0);
        assert!(blink.advance(2.0 * HALF_PERIOD));
        assert_eq!(blink.alpha(), 1.0);
    }

    #[test]
    fn a_new_interval_rebuilds_the_pending_tick() {
        // **Yeniden kurmak şart**: `next_flip` mutlak bir son tarih, yani
        // yalnız alanı yazmak kaydedilen ritmi bir flip **geciktirirdi** —
        // kullanıcı kaydeder, hiçbir şey olmaz, sonraki sönmede birden
        // değişir.
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);
        assert_eq!(blink.next_flip(), Some(0.5), "varsayılan yarım periyot");

        // 2.0'da periyot kısalıyor: tik **o andan** itibaren yeniden kuruluyor.
        blink.set_half_period(2.0, 0.1);
        assert_eq!(blink.next_flip(), Some(2.1));
        assert!(blink.advance(2.1), "yeni ritimde dönmedi");
        assert_eq!(blink.next_flip(), Some(2.2), "yeni periyot sürmüyor");

        // Aynı değer **hiçbir şey yapmıyor**: her ayar kaydı fazı
        // sıfırlasaydı kaydeden kullanıcı imleci sürekli açığa çekerdi.
        let before = blink;
        blink.set_half_period(5.0, 0.1);
        assert_eq!(blink, before, "aynı periyot fazı kıpırdattı");
    }

    #[test]
    fn an_interval_change_while_stopped_arms_nothing() {
        // Bekleyen tik yokken (blink kapalı ya da hareketsizlikte durmuş)
        // periyot değişimi **tik doğurmamalı**: doğursaydı kapalı bir blink
        // saat kurar ve boşta sıfır kare sözleşmesi kırılırdı.
        let mut blink = Blink::default();
        blink.set_half_period(1.0, 0.2);
        assert_eq!(blink.next_flip(), None, "kapalı blink tik kurdu");
    }

    #[test]
    fn the_stop_condition_leaves_the_caret_lit() {
        // **R9.1.** Sönük fazda durulsaydı imleç bir sonraki hasara kadar
        // kaybolurdu; durma fazı açığa çekiyor ve son bir kare istiyor
        // (dönen `true`).
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);
        assert!(blink.advance(HALF_PERIOD), "faz sönmedi");
        assert_eq!(blink.alpha(), 0.0);
        assert!(blink.advance(IDLE_STOP), "durma koşulu son kareyi istemedi");
        assert_eq!(blink.alpha(), 1.0, "imleç sönük kaldı");
        assert_eq!(blink.next_flip(), None, "durduktan sonra saat kuruldu");
        // Bir daha kare istemiyor: durma tek atımlık.
        assert!(!blink.advance(IDLE_STOP + 10.0));
    }

    #[test]
    fn typing_keeps_the_caret_lit() {
        // Caret kıpırdayınca faz açığa dönüyor ve sayaç baştan başlıyor, yani
        // yazmaya devam eden kullanıcı imleci **hiç** sönük görmüyor.
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);
        assert!(blink.advance(HALF_PERIOD), "faz sönmedi");
        assert_eq!(blink.alpha(), 0.0);

        blink.wake(HALF_PERIOD);
        assert_eq!(blink.alpha(), 1.0, "yazarken imleç sönük kaldı");
        assert_eq!(blink.next_flip(), Some(2.0 * HALF_PERIOD));
        // Yarım periyot dolmadan tekrar yazmak sayacı yine öteliyor.
        blink.wake(1.5 * HALF_PERIOD);
        assert!(!blink.advance(2.0 * HALF_PERIOD), "sayaç ötelenmedi");
        assert_eq!(blink.alpha(), 1.0);
    }

    #[test]
    fn a_disabled_blink_ignores_the_caret_moving() {
        let mut blink = Blink::default();
        blink.content_frame(0.0, false);
        blink.wake(1.0);
        assert_eq!(blink.next_flip(), None, "kapalı blink saat kurdu");
    }

    #[test]
    fn damage_brings_the_blink_back() {
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);
        blink.advance(IDLE_STOP);
        assert_eq!(blink.next_flip(), None);
        blink.content_frame(IDLE_STOP, true);
        assert_eq!(
            blink.next_flip(),
            Some(IDLE_STOP + HALF_PERIOD),
            "hasar blink'i geri getirmedi"
        );
    }

    #[test]
    fn turning_it_off_mid_dark_phase_relights_the_caret() {
        let mut blink = Blink::default();
        blink.content_frame(0.0, true);
        blink.advance(HALF_PERIOD);
        assert_eq!(blink.alpha(), 0.0);
        blink.content_frame(0.6, false);
        assert_eq!(blink.alpha(), 1.0, "kapatılan blink imleci sönük bıraktı");
        assert_eq!(blink.next_flip(), None);
    }
}
