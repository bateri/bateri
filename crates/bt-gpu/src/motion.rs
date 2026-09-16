//! İmlecin hücreler arasındaki kayması — **saf**, ObjC'siz, kilitsiz.
//!
//! Emsali `Gate` ve `FailureStreak`: politikanın kendisi platformdan bağımsız
//! olduğu için ayrı bir tipte yaşıyor ve gerçek bir pencere olmadan sınanıyor.
//! `link.rs`'e gömülü kalsaydı yalnız ekranda denenebilirdi.
//!
//! **Durum hücre birimindedir**, pikselde değil (008 Karar 5): font, zoom ya
//! da ekran ölçeği değişince konumun piksel karşılığı oynar ama hücre
//! koordinatı aynı kalır, yani ölçü değişimi kendiliğinden doğru yere düşer.
//!
//! **Her animasyon bir durma koşulu taşır** (`CLAUDE.md`) ve buradaki iki
//! katlı: konum+hız eşiği **ya da** süre tavanı. İkincisi kemer — birincisini
//! hiç sağlamayan bir parametre seti (aşırı düşük sönümleme, sonsuz salınım)
//! link'i sonsuza uyanık tutardı.

/// Yay sertliği, rad/s. **Seçilmiş bir sayı, ölçülmüş değil.**
///
/// Kritik sönümlemede (ζ = 1) bir hücrelik kayma bu değerde ~230 ms'de
/// yerleşiyor (bağlayan eşik [`VEL_EPSILON`], konum değil). the reference'in
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

/// İmlecin kaymasının durumu.
///
/// `Option`'ın içi "uçuşta bir imleç var" demek; `None` hem ilk kare hem de
/// görünmez imleç. İkisini ayıran bir bayrak **bilerek yok**: ikisinde de
/// yapılacak şey aynı (sıradaki hedefe anında otur) ve iki bayrak
/// ayrışabilen iki gerçek olurdu.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Motion {
    state: Option<State>,
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
    /// Hücre/saniye.
    vel: [f32; 2],
    target: [f32; 2],
    /// Hedef kurulalı beri geçen süre; süre tavanının operandı.
    elapsed: f32,
}

impl Motion {
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
    pub(crate) fn sync(&mut self, col: u16, row: u16, visible: bool, offset: i32, geometry: bool) {
        let scrolled = self.offset != Some(offset);
        self.offset = Some(offset);
        if !visible {
            self.state = None;
            return;
        }
        let target = [f32::from(col), f32::from(row)];
        match &mut self.state {
            Some(state) if !scrolled && !geometry => {
                if state.target != target {
                    state.target = target;
                    state.elapsed = 0.0;
                }
            }
            // Snap: hem uçuştaki durum hem de yoklukta aynı yere iniyor.
            state => {
                *state = Some(State {
                    pos: target,
                    vel: [0.0; 2],
                    target,
                    elapsed: 0.0,
                });
            }
        }
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
        let Some(state) = &mut self.state else {
            return;
        };
        let dt = dt.clamp(0.0, DT_MAX);
        state.elapsed += dt;
        for axis in 0..2 {
            let before = state.pos[axis] - state.target[axis];
            let (after, vel) = critically_damped(before, state.vel[axis], dt);
            // **Taşma kırpması** — kapalı formun tek başına vermediği garanti.
            //
            // ζ = 1 "durgun hâlden taşma yok" demek; [`Motion::sync`] ise
            // uçuşta hızı **bilerek** koruyor (momentum) ve o hız hedefin
            // ötesine taşıyabiliyor: `(d + c·t)e^{-ωt}` ifadesi
            // `|v| > OMEGA × kalan mesafe` olduğunda sıfırı geçiyor. Ölçülen
            // en kötü hâl, uzun bir sıçramanın ortasında yapılan küçük bir
            // hedef düzeltmesinde **0,87 hücre** — neredeyse tam bir hücre,
            // yani gözle görülür bir geri tepme. 008 Karar 6 bunu adıyla
            // yasaklıyor ("kritik sönümlemeye yakın, **taşma yok**), o yüzden
            // hedefi geçen eksen hedefte durduruluyor.
            //
            // Sert bir duruş değil: kırpmanın ateşlediği an imleç zaten tam
            // hedefin üstünde, yani görünen şey "vardı ve durdu".
            if before != 0.0 && (before < 0.0) != (after < 0.0) {
                state.pos[axis] = state.target[axis];
                state.vel[axis] = 0.0;
            } else {
                state.pos[axis] = state.target[axis] + after;
                state.vel[axis] = vel;
            }
        }
        if state.settled() {
            state.pos = state.target;
            state.vel = [0.0; 2];
        }
    }

    /// Uçuştaki kaymayı **hedefinde bitirir** — animasyonun ilerleyemeyeceği
    /// anlar için.
    ///
    /// Bugünkü tek çağıranı örtülen pencere: link duruyor, yani `advance` bir
    /// daha koşmuyor ve durum sonsuza kadar "yerleşmemiş" kalırdı. Bedeli iki
    /// katlı olurdu — süreli koşu deadline'da `MotionUnsettled` deyip
    /// **kod doğruyken** kırmızı düşer ve tanı "bir durma koşulu bozuk" diye
    /// yanlış yeri gösterirdi; örtülme kalkınca da imleç, kullanıcının hiç
    /// görmediği bir noktadan kayarak gelirdi.
    ///
    /// Snap politikasının zaten söylediği şey (008 Karar 5): görünürlük
    /// dönüşü animasyonsuz. Burada yalnız aynı kural bir kare erken
    /// uygulanıyor.
    pub(crate) fn finish(&mut self) {
        if let Some(state) = &mut self.state {
            state.pos = state.target;
            state.vel = [0.0; 2];
            state.elapsed = 0.0;
        }
    }

    /// Bu karede imlecin çizileceği yer, **hücre birimi**. Durum yoksa hedef
    /// de yok: çağıran görünmez imleci zaten çizmiyor.
    pub(crate) fn position(&self) -> Option<[f32; 2]> {
        self.state.map(|state| state.pos)
    }

    /// Animasyon durdu mu — link'in "uyuyabilir miyim" sorusu.
    ///
    /// Durum yokken `true`: çizilecek bir imleç yoksa bekleyecek bir şey de
    /// yok. Süreli koşunun kapısı (`Verdict::MotionUnsettled`) da bunu
    /// okuyor.
    pub(crate) fn settled(&self) -> bool {
        self.state.is_none_or(|state| state.settled())
    }
}

impl State {
    fn settled(&self) -> bool {
        // Süre tavanı **veya** eşik; ikisi de tek başına yeterli.
        self.elapsed >= TIME_CEILING
            || (0..2).all(|axis| {
                (self.pos[axis] - self.target[axis]).abs() <= POS_EPSILON
                    && self.vel[axis].abs() <= VEL_EPSILON
            })
    }
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

    fn moving() -> Motion {
        let mut motion = Motion::default();
        // İlk `sync` snap: durum yok.
        motion.sync(0, 0, true, 0, false);
        assert!(motion.settled(), "ilk kare animasyon başlattı");
        motion.sync(10, 4, true, 0, false);
        motion
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
            motion.sync(0, 0, true, 0, false);
            motion.sync(col, row, true, 0, false);
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
            target: [200.0, 0.0],
            elapsed: TIME_CEILING,
        };
        assert!(far.settled(), "süre tavanı dolmuş koşuyu durdurmadı");
        assert!(
            !State {
                elapsed: TIME_CEILING - TICK,
                ..far
            }
            .settled(),
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

        motion.sync(20, 4, true, 0, false);
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
        motion.sync(0, 0, true, 0, false);
        motion.sync(10, 0, true, 0, false);
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
            target: [target, 0.0],
            elapsed: 0.0,
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
            motion.sync(0, 0, true, 0, false);
            motion.sync(distance, 0, true, 0, false);
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
        motion.sync(10, 4, true, 0, false);
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

        motion.sync(10, 7, true, 3, false);
        assert!(motion.settled(), "kaydırma animasyon başlattı");
        assert_eq!(motion.position(), Some([10.0, 7.0]));

        // Aynı satır değişimi ofset **sabitken** animasyonlu.
        motion.sync(10, 4, true, 3, false);
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
        motion.sync(10, 4, true, 0, false);
        assert!(motion.settled(), "görünürlük dönüşü animasyon başlattı");
    }

    #[test]
    fn geometry_and_visibility_snap() {
        // Geometri: pencere/font/zoom oynadı, ızgara kaydı.
        let mut motion = moving();
        motion.sync(3, 1, true, 0, true);
        assert!(motion.settled(), "geometri animasyon başlattı");
        assert_eq!(motion.position(), Some([3.0, 1.0]));

        // Görünmezlik durumu boşaltır; geri açılan imleç yeni yerinde doğar.
        // TUI'ler tam bunu yapıyor: çizerken imleci gizleyip taşıyorlar.
        motion.sync(3, 1, false, 0, false);
        assert!(motion.settled());
        assert_eq!(motion.position(), None, "görünmez imleç konum verdi");
        motion.sync(40, 20, true, 0, false);
        assert!(motion.settled(), "görünürlük dönüşü animasyon başlattı");
        assert_eq!(motion.position(), Some([40.0, 20.0]));
    }

    #[test]
    fn a_hidden_cursor_keeps_the_scroll_history() {
        // Ofset `state`'ten ayrı yaşamalı: TUI imleci gizler, pencereyi
        // kaydırır, sonra geri açar. Ofset boşalsaydı geri açılan imleç
        // "kaydırma olmadı" der ve kaydırmayı animasyon sanırdı — burada
        // zaten snap olduğu için belirti yok, ama ters yönde (gizliyken
        // kaydırma **olmadığında**) yanlış snap üretirdi.
        let mut motion = Motion::default();
        motion.sync(0, 0, false, 5, false);
        motion.sync(0, 0, true, 5, false);
        assert_eq!(motion.offset, Some(5));
    }
}
