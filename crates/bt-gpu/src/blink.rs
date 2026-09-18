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
const HALF_PERIOD: f64 = 0.5;

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
            self.next_flip = enabled.then_some(now + HALF_PERIOD);
        } else if enabled && self.next_flip.is_none() {
            // Hareketsizlikten dönüş: sayaç yukarıda tazelendi, tik yeniden
            // kuruluyor.
            self.next_flip = Some(now + HALF_PERIOD);
        }
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
        self.next_flip = Some(now + HALF_PERIOD);
        true
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
