//! Dock'ta yazılan ve silinen glyph'lerin efektleri — **saf**, ObjC'siz,
//! kilitsiz (030).
//!
//! Hangi glyph'in geldiğini ya da gittiğini `bt-core` söylüyor
//! ([`bt_core::DockEdit`]); burada yalnız **zaman** var: uçuştaki girdilerin
//! listesi, geçen süre ve durma koşulu. Çizim `Frame`'in ve `glyph_fx`
//! pipeline'ının işi. Kararların gerekçesi
//! `.tasks/030-dock-yazim-animasyonlari/discussion.md` → Karar 3, 4 ve 7.
//!
//! **`Motion`'ın içinde değil yanında** ([`crate::blink`] emsali): `Motion`
//! `Copy` ve `Cell` içinde yaşıyor, bir liste onu ya `Copy`'den çıkarır ya da
//! her `get`/`set`'te kopyalatırdı.
//!
//! **Durma koşulu boş liste.** Süresi dolan girdi düşüyor ve liste boşsa
//! yapacak iş kalmıyor ([`GlyphFx::is_empty`] link'in uyku testinin bir
//! terimi). Kare talebi hareketin: `Waker::wake`'e dokunulmuyor, hasar
//! dikilmiyor — efekt içeriği değil içeriğin nasıl çizildiğini değiştiriyor.

use bt_core::{Cell, DockEdit, EDIT_MAX, Erase, Keypress};

use crate::motion::Motion;

/// Gelişin süresi, saniye — **seçilmiş, ölçülmüş değil**; referansın temel
/// süresi (the reference, 240 ms).
///
/// İlk seçim 120 ms'ydi ve kullanıcı gerçek pencerede "animasyonlar hiç belli
/// olmuyor" dedi: görünür kısım üç-dört kareye sığıyordu ve gelişin ilk anı
/// henüz ayrılmamış caret'in içinde geçiyordu. Hızlı yazımda iki-üç geliş
/// üst üste uçuşta ve bu bir titreme değil: her biri kendi hücresinde,
/// sütun kuralı ([`GlyphFx::apply`]) yalnız sağdakini bitiriyor.
pub(crate) const KEYPRESS_DURATION: f32 = 0.24;

/// Hayaletin süresi, saniye — **seçilmiş, ölçülmüş değil**.
///
/// Gelişten bir tık uzun: gidiş gözün takip ettiği bir hareket (neyin
/// silindiğini okumak), geliş ise yazılanın zaten bilinen bir onayı. İlk
/// seçim 160 ms'ydi ve gelişinkiyle aynı sebeple uzadı; Backspace'ten sonra
/// caret tam hayaletin sütununa geliyor, yani hayalet görünür olmak için
/// caret'in üstünde çizilmek zorunda (`Renderer::encode_fx`).
pub(crate) const ERASE_DURATION: f32 = 0.30;

/// Uçuştaki girdilerin tavanı — **tasarım sabiti**.
///
/// Basılı Backspace kare başına en çok bir hayalet, yazım kare başına iki-üç
/// geliş doğuruyor ve her biri [`ERASE_DURATION`] kadar yaşıyor: tavan olağan
/// yazımın hiçbir zaman değmediği bir sayı. Dolunca **en eski** girdi bitiyor
/// — gelişse statik glyph'i geri geliyor, hayaletse kalkıyor; yanlışın yönü
/// güvenli, efekt kısalır ama hiçbir glyph kaybolmaz.
pub(crate) const FX_MAX: usize = 32;

/// Bir efekt adının shader'daki kimliği (`shaders/glyph_fx.metal` → `FX_*`).
///
/// Adların sözlüğü `bt-core`'un ([`Keypress`], [`Erase`] — ayar modeli,
/// `CursorMotion` emsali); kimlik çizimin bilgisi ve burada. Kapsamlı
/// `match`: `bt-core`'a yeni bir ad girdiği an burası derlenmez, yani
/// kimliksiz bir ad popup'a sızamaz.
pub(crate) trait Effect: Copy + PartialEq + 'static {
    /// Gelişler `1..16`'da, hayaletler `16..32`'de: shader girdinin türünü
    /// kimlikten okuyor ve ikinci bir bit taşımıyor. `Off` çizilmiyor,
    /// kimliği yok.
    fn id(self) -> Option<u32>;

    /// Çizen efektlerin hepsi (`Off` hariç), `NAMES` sırasıyla — hermetik
    /// değişmezlerin döngüsü (`plan.md` → R5): ayar modeline giren her ad
    /// girdiği an sınamaların altında.
    #[cfg(test)]
    fn effects() -> Vec<Self>;
}

impl Effect for Keypress {
    fn id(self) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::Fade => Some(1),
            Self::Rise => Some(2),
            Self::Pop => Some(3),
            Self::Extrude => Some(4),
            Self::Heat => Some(5),
            Self::Echo => Some(6),
            Self::Drop => Some(7),
            Self::Ink => Some(8),
            Self::Squeeze => Some(9),
        }
    }

    #[cfg(test)]
    fn effects() -> Vec<Self> {
        drawn(Self::NAMES)
    }
}

impl Effect for Erase {
    fn id(self) -> Option<u32> {
        match self {
            Self::Off => None,
            // `recede` 16'da kaldı (phase-2'nin kimliği); öteki yedi Karar 6'nın
            // sırasıyla arkasına dizildi — kimlik yalnız shader'la sözleşme,
            // sıranın ürün anlamı yok.
            Self::Recede => Some(16),
            Self::Iris => Some(17),
            Self::Undertow => Some(18),
            Self::Echo => Some(19),
            Self::Bleed => Some(20),
            Self::Unravel => Some(21),
            Self::Sublime => Some(22),
            Self::Shatter => Some(23),
        }
    }

    #[cfg(test)]
    fn effects() -> Vec<Self> {
        drawn(Self::NAMES)
    }
}

#[cfg(test)]
fn drawn<T: Effect>(names: &[(&str, T)]) -> Vec<T> {
    names
        .iter()
        .map(|&(_, effect)| effect)
        .filter(|effect| effect.id().is_some())
        .collect()
}

/// Girdinin türü: gelen glyph ya da gidenin hayaleti.
///
/// Çizim sırası buna bağlı (`Renderer::encode_dock`): hayaletler dock'un
/// glyph'lerinden **önce** — satır ortasında silinen harfin yerine kayan harf
/// onun üstünde durur —, gelişler **sonra**.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Arrival,
    Ghost,
}

/// Çizime giden bir girdi: hücresi, türü, efekti ve ilerlemesi.
///
/// `Frame`'in girdisi, [`GlyphFx`]'in iç durumu değil: sınamalar bir
/// efektin istedikleri ilerlemesini ([`Fx::t`]) doğrudan kurabiliyor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fx {
    /// Sınırın çözülmüş hücresi — sütunu **bu karenin** ekran sütunu.
    pub(crate) cell: Cell,
    pub(crate) kind: Kind,
    /// Shader'ın efekt kimliği.
    pub(crate) effect: u32,
    /// İlerleme, `0..=1`; eğri shader'da.
    pub(crate) t: f32,
    /// Girdinin tohumu — parçalı efektlerin (`shatter`, `unravel`) rastgelesi.
    /// Girdi doğarken bir kez veriliyor ve ömrü boyunca sabit, yani hareket
    /// karesinde parçalar titremiyor. Sütundan değil sıradan türüyor: pencere
    /// kayınca sütun değişiyor, parçaların deseni değişmemeli.
    pub(crate) seed: f32,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    fx: Fx,
    elapsed: f32,
    duration: f32,
}

/// Uçuştaki efektler.
///
/// Liste sabit tavanlı ([`FX_MAX`]) ve kapasitesi korunuyor: ilk efektten
/// sonra kare başına ayırma yok.
#[derive(Debug, Default)]
pub(crate) struct GlyphFx {
    entries: Vec<Entry>,
    /// Kullanıcının seçtiği geliş efekti — ham, indirgenmemiş. İndirgeme
    /// ([`Motion::glyph_fx`]) her düzenlemede yeniden soruluyor, yani
    /// Hareketi Azalt'ın değişimi bir sonraki tuşta geçerli.
    keypress: Keypress,
    erase: Erase,
    /// Tohumun kaynağı; sarması zararsız.
    serial: u32,
}

impl GlyphFx {
    /// Bu karenin düzenlemesini işler.
    ///
    /// Sıra: önce pencerenin kayması (uçuştakiler metinle birlikte kayıyor ve
    /// metnin sütunlarından — `window` — taşan düşüyor), sonra **sütun
    /// kuralı** — yeni düzenlemenin sütununa eşit ya da sağındaki gelişler
    /// biter (`discussion.md` → Karar 3: caret'i uçuştaki bir glyph'in soluna
    /// taşıyıp yazan kullanıcıda o glyph erken oturur), en son yeni girdiler.
    ///
    /// `window` giriş satırının metin sütunları, `[ilk, son)`.
    pub(crate) fn apply(&mut self, edit: DockEdit, motion: Motion, window: (u16, u16)) {
        let (keypress, erase) = motion.glyph_fx(self.keypress, self.erase);
        let (col, cells, shift, kind, effect, duration) = match edit {
            DockEdit::Reset => {
                self.finish();
                return;
            }
            DockEdit::Shift { by } => {
                self.shift(by, window);
                return;
            }
            DockEdit::Arrive { col, cells, shift } => (
                col,
                cells,
                shift,
                Kind::Arrival,
                keypress.id(),
                KEYPRESS_DURATION,
            ),
            DockEdit::Erase { col, ghosts, shift } => {
                (col, ghosts, shift, Kind::Ghost, erase.id(), ERASE_DURATION)
            }
        };
        self.shift(shift, window);
        // **Uçuştaki bir gelişi silmek hayalet doğurmuyor**: hayalet `t = 0`'da
        // tam opak başlıyor, yani yarı belirmiş bir harf silinince bir kare
        // tam renge sıçrayıp öyle giderdi — hızlı bir yazım hatası
        // düzeltmesinin her seferinde. Harf olduğu yerden kayboluyor.
        let mut unborn = [false; EDIT_MAX];
        if kind == Kind::Ghost {
            for (slot, cell) in unborn.iter_mut().zip(cells.as_slice()) {
                *slot = self.entries.iter().any(|entry| {
                    entry.fx.kind == Kind::Arrival
                        && entry.fx.cell.col == cell.col
                        && entry.fx.cell.ch == cell.ch
                });
            }
        }
        self.entries
            .retain(|entry| entry.fx.kind == Kind::Ghost || entry.fx.cell.col < col);
        let Some(effect) = effect else {
            return;
        };
        for (&cell, _) in cells
            .as_slice()
            .iter()
            .zip(unborn)
            .filter(|&(_, unborn)| !unborn)
        {
            if self.entries.len() >= FX_MAX {
                // En eski girdi bitiyor: liste ekleme sırasında.
                self.entries.remove(0);
            }
            self.serial = self.serial.wrapping_add(1);
            self.entries.push(Entry {
                fx: Fx {
                    cell,
                    kind,
                    effect,
                    t: 0.0,
                    // audit: tohum yalnız desen; küçük tutuluyor ki `f32`'de
                    // tam temsil edilsin.
                    seed: (self.serial % 1024) as f32,
                },
                elapsed: 0.0,
                duration,
            });
        }
    }

    /// Kullanıcının seçtiği iki efekti kurar — ham adlar, indirgeme her
    /// düzenlemede ([`Motion::glyph_fx`]).
    ///
    /// **Değişince uçuştakiler biter** (`Motion::set_style`'ın emsali): eski
    /// efektle başlamış bir girdiyi yeni efektin eğrisinde sürdürmenin
    /// anlamı yok, `off`'a geçen kullanıcının gördüğü ise tam da "anında"
    /// olmalı. Aynı seçim no-op. Dönüş bir şeyin bitirilip bitirilmediği:
    /// bitirilen girdinin son hâli ekrana ancak bir kare çizilirse düşer ve
    /// o karenin talebi çağıranın (`DisplayLink::set_glyph_fx`).
    pub(crate) fn set_effects(&mut self, keypress: Keypress, erase: Erase) -> bool {
        if (self.keypress, self.erase) == (keypress, erase) {
            return false;
        }
        self.keypress = keypress;
        self.erase = erase;
        let finished = !self.is_empty();
        self.finish();
        finished
    }

    /// Uçuştakileri pencerenin kayması kadar kaydırır; metnin sütunlarından
    /// taşan girdi düşer (hayalet işaretin ya da sağ payın üstünde asılı
    /// kalmasın, gelişin ise statik glyph'i zaten o pencerede yok).
    fn shift(&mut self, by: i32, (first, end): (u16, u16)) {
        if by == 0 {
            return;
        }
        self.entries.retain_mut(|entry| {
            let cell = &mut entry.fx.cell;
            let width = if cell.wide { 2 } else { 1 };
            let col = i32::from(cell.col) + by;
            if col < i32::from(first) || col + width > i32::from(end) {
                return false;
            }
            // audit: `first ≤ col < end` ve ikisi de `u16`.
            cell.col = col as u16;
            true
        });
    }

    /// Geçen süreyi işler; süresi dolan girdi düşer.
    ///
    /// `dt` [`crate::motion::DT_MAX`]'ta kırpılıyor, öteki animatörlerle aynı
    /// gerekçe: uykudan uyanan link'in ilk aralığı sınırsız olabilir.
    pub(crate) fn advance(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, crate::motion::DT_MAX);
        self.entries.retain_mut(|entry| {
            entry.elapsed += dt;
            entry.elapsed < entry.duration
        });
    }

    /// Uçuştakilerin hepsini bitirir: gelişler statik glyph'lerine oturur,
    /// hayaletler kalkar.
    ///
    /// Çağıranı `Motion::finish`'inkilerle aynı (örtülen pencere, `snap`'e ya
    /// da Hareketi Azalt'a geçen ayar, çizim hatası) ve gerekçesi de: arka
    /// sekmede donan bir efekt geri gelince görülmemiş bir fazdan devam
    /// ederdi, kalıcı bir hatada ise hareket karesi dönmeye devam ederdi.
    pub(crate) fn finish(&mut self) {
        self.entries.clear();
    }

    /// Uçuşta hiçbir şey yok mu — link'in uyku testinin terimi.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Yalnız istenen girdileri tutar — `Frame::suppress_dock`'un kolu:
    /// statik glyph'i bulunamayan geliş biter.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&Fx) -> bool) {
        self.entries.retain(|entry| keep(&entry.fx));
    }

    /// Bu karenin girdileri, ilerlemeleriyle.
    pub(crate) fn iter(&self) -> impl Iterator<Item = Fx> + '_ {
        self.entries.iter().map(|entry| Fx {
            t: (entry.elapsed / entry.duration).min(1.0),
            ..entry.fx
        })
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{CursorMotion, DOCK_TEXT_COL, DockEdit, EDIT_MAX, EditCells};

    use super::*;

    const WINDOW: (u16, u16) = (DOCK_TEXT_COL, 80);

    fn cell(col: u16, ch: char) -> Cell {
        Cell {
            col,
            ch: Some(ch),
            ..Cell::default()
        }
    }

    fn cells(list: &[Cell]) -> EditCells {
        assert!(list.len() <= EDIT_MAX);
        list.iter().copied().collect()
    }

    fn arrive(col: u16, ch: char) -> DockEdit {
        DockEdit::Arrive {
            col,
            cells: cells(&[cell(col, ch)]),
            shift: 0,
        }
    }

    fn erase(col: u16, ch: char) -> DockEdit {
        DockEdit::Erase {
            col,
            ghosts: cells(&[cell(col, ch)]),
            shift: 0,
        }
    }

    fn cols(fx: &GlyphFx) -> Vec<(u16, Kind)> {
        fx.iter().map(|fx| (fx.cell.col, fx.kind)).collect()
    }

    #[test]
    fn a_new_choice_finishes_what_is_in_flight() {
        let mut fx = GlyphFx::default();
        fx.apply(arrive(4, 'a'), Motion::default(), WINDOW);
        // Aynı seçimi yeniden yazan kayıt no-op: efekt sürüyor, kare de
        // istenmiyor.
        assert!(!fx.set_effects(Keypress::Fade, Erase::Recede));
        assert_eq!(cols(&fx), [(4, Kind::Arrival)]);
        // Değişim uçuştakini bitiriyor ve bunu söylüyor (link kare ister).
        assert!(fx.set_effects(Keypress::Off, Erase::Recede));
        assert!(fx.is_empty());
        // Yeni seçim bir sonraki düzenlemede geçerli.
        fx.apply(arrive(5, 'b'), Motion::default(), WINDOW);
        fx.apply(erase(7, 'c'), Motion::default(), WINDOW);
        assert_eq!(cols(&fx), [(7, Kind::Ghost)]);
        assert!(fx.set_effects(Keypress::Fade, Erase::Off));
        fx.apply(erase(7, 'c'), Motion::default(), WINDOW);
        assert!(fx.is_empty());
        // Boş listede değişim bir şey bitirmedi: kare gerekmiyor.
        assert!(!fx.set_effects(Keypress::Off, Erase::Off));
    }

    #[test]
    fn an_arrival_and_a_ghost_live_for_their_duration() {
        let mut fx = GlyphFx::default();
        fx.apply(arrive(4, 'a'), Motion::default(), WINDOW);
        fx.apply(erase(6, 'b'), Motion::default(), WINDOW);
        assert_eq!(cols(&fx), [(4, Kind::Arrival), (6, Kind::Ghost)]);
        assert!(fx.iter().all(|fx| fx.t == 0.0));
        // Adımlar `DT_MAX`'ın altında: tek bir büyük `dt` kırpılırdı.
        let step = KEYPRESS_DURATION / 4.0;
        assert!(step <= crate::motion::DT_MAX);
        // Yarı yolda ilerleme süreye göre.
        fx.advance(step);
        fx.advance(step);
        let t: Vec<f32> = fx.iter().map(|fx| fx.t).collect();
        assert!((t[0] - 0.5).abs() < 1e-6, "{t:?}");
        assert!(t[1] < t[0], "hayaletin süresi daha uzun: {t:?}");
        // Geliş biter, hayalet sürer; sonra o da biter ve liste boşalır.
        fx.advance(step);
        fx.advance(step);
        assert_eq!(cols(&fx), [(6, Kind::Ghost)]);
        for _ in 0..4 {
            fx.advance(ERASE_DURATION / 4.0);
        }
        assert!(fx.is_empty(), "süresi dolan girdi düşmedi");
    }

    #[test]
    fn a_reset_finishes_everything() {
        let mut fx = GlyphFx::default();
        fx.apply(arrive(4, 'a'), Motion::default(), WINDOW);
        fx.apply(erase(6, 'b'), Motion::default(), WINDOW);
        fx.apply(DockEdit::Reset, Motion::default(), WINDOW);
        assert!(fx.is_empty());
    }

    #[test]
    fn a_new_edit_settles_the_arrivals_at_and_right_of_its_column() {
        let mut fx = GlyphFx::default();
        fx.apply(arrive(4, 'a'), Motion::default(), WINDOW);
        fx.apply(arrive(5, 'b'), Motion::default(), WINDOW);
        fx.apply(erase(9, 'z'), Motion::default(), WINDOW);
        // Normal yazım: sonraki tuş sağa ekliyor, önceki gelişler sürüyor.
        fx.apply(arrive(6, 'c'), Motion::default(), WINDOW);
        assert_eq!(
            cols(&fx),
            [
                (4, Kind::Arrival),
                (5, Kind::Arrival),
                (9, Kind::Ghost),
                (6, Kind::Arrival)
            ]
        );
        // Caret sola taşındı ve 5'te yazıldı: 5 ve sağındaki gelişler oturur,
        // hayalet yerinde kalır.
        fx.apply(arrive(5, 'x'), Motion::default(), WINDOW);
        assert_eq!(
            cols(&fx),
            [(4, Kind::Arrival), (9, Kind::Ghost), (5, Kind::Arrival)]
        );
    }

    #[test]
    fn a_shift_moves_the_flight_with_the_text_and_drops_what_leaves() {
        let mut fx = GlyphFx::default();
        let window = (DOCK_TEXT_COL, 10);
        fx.apply(arrive(3, 'a'), Motion::default(), window);
        fx.apply(erase(9, 'b'), Motion::default(), window);
        // Taşan satırın sonunda yazım: metin bir sütun sola kaydı ve yeni
        // harf caret'in solunda.
        fx.apply(
            DockEdit::Arrive {
                col: 8,
                cells: cells(&[cell(8, 'c')]),
                shift: -1,
            },
            Motion::default(),
            window,
        );
        assert_eq!(
            cols(&fx),
            [(2, Kind::Arrival), (8, Kind::Ghost), (8, Kind::Arrival)]
        );
        // Caret gezindi, pencere iki sütun sağa: 8'deki iki girdi pencerenin
        // dışına düşüyor.
        fx.apply(DockEdit::Shift { by: 2 }, Motion::default(), window);
        assert_eq!(cols(&fx), [(4, Kind::Arrival)]);
        // Sola taşan da düşüyor: işaretin üstünde asılı kalmıyor.
        fx.apply(DockEdit::Shift { by: -3 }, Motion::default(), window);
        assert!(fx.is_empty(), "{:?}", cols(&fx));
    }

    #[test]
    fn a_full_list_finishes_the_oldest() {
        let mut fx = GlyphFx::default();
        for n in 0..FX_MAX + 3 {
            // Hayaletler sütun kuralına girmiyor: hepsi yaşıyor.
            fx.apply(erase(DOCK_TEXT_COL, 'a'), Motion::default(), WINDOW);
            fx.advance(0.0001 * n as f32);
        }
        assert_eq!(fx.iter().count(), FX_MAX);
        // En eskiler gitti: kalanların hiçbiri ilk üç girdinin tohumunu
        // taşımıyor.
        assert!(fx.iter().all(|fx| fx.seed > 3.0), "en eski düşmedi");
    }

    #[test]
    fn a_seed_is_fixed_for_the_life_of_an_entry() {
        // Parçalı efektlerin deseni tohumdan: iki karede aynı girdi aynı
        // tohumu taşımazsa `shatter`'ın parçaları her hareket karesinde
        // yeniden dağılır. Kayma ve ilerleme tohuma dokunmuyor; yan yana iki
        // girdinin tohumu ise ayrı (aynı desenle kırılmasınlar).
        let mut fx = GlyphFx::default();
        fx.apply(erase(6, 'a'), Motion::default(), WINDOW);
        fx.apply(erase(5, 'b'), Motion::default(), WINDOW);
        let seeds = |fx: &GlyphFx| fx.iter().map(|fx| fx.seed).collect::<Vec<f32>>();
        let born = seeds(&fx);
        assert_ne!(born[0], born[1], "iki girdi aynı tohumla doğdu");
        fx.advance(ERASE_DURATION / 3.0);
        assert_eq!(seeds(&fx), born, "ilerleme tohumu değiştirdi");
        fx.apply(DockEdit::Shift { by: 1 }, Motion::default(), WINDOW);
        assert_eq!(seeds(&fx), born, "kayma tohumu değiştirdi");
    }

    #[test]
    fn erasing_an_arrival_in_flight_leaves_no_ghost() {
        // Yarı belirmiş harfin hayaleti `t = 0`'da tam opak doğup bir kare
        // sıçrardı: silinen geliş olduğu yerden kayboluyor. Aynı sütundaki
        // **başka** bir harfin silinmesi (geliş çoktan bitmiş) hayalet alıyor.
        let mut fx = GlyphFx::default();
        fx.apply(arrive(4, 'a'), Motion::default(), WINDOW);
        fx.apply(erase(4, 'a'), Motion::default(), WINDOW);
        assert!(fx.is_empty(), "{:?}", cols(&fx));
        fx.apply(erase(4, 'b'), Motion::default(), WINDOW);
        assert_eq!(cols(&fx), [(4, Kind::Ghost)]);
    }

    #[test]
    fn finish_empties_the_flight() {
        let mut fx = GlyphFx::default();
        fx.apply(arrive(4, 'a'), Motion::default(), WINDOW);
        fx.finish();
        assert!(fx.is_empty());
    }

    #[test]
    fn snap_and_reduce_motion_reduce_the_effects() {
        // `discussion.md` → Karar 7'nin tablosu: `snap` ikisini de kapatır,
        // Hareketi Azalt gelişi `fade`'e indirir ve hayaleti kapatır.
        let motion = |style: CursorMotion, reduce: bool| {
            let mut motion = Motion::default();
            motion.set_style(style);
            motion.set_reduce(reduce);
            motion
        };
        for reduce in [false, true] {
            let snap = motion(CursorMotion::Snap, reduce);
            let mut fx = GlyphFx::default();
            fx.apply(arrive(4, 'a'), snap, WINDOW);
            fx.apply(erase(6, 'b'), snap, WINDOW);
            assert!(fx.is_empty(), "snap animasyon doğurdu (reduce={reduce})");
        }
        for style in [CursorMotion::Ease, CursorMotion::Spring] {
            let reduced = motion(style, true);
            let mut fx = GlyphFx::default();
            fx.apply(arrive(4, 'a'), reduced, WINDOW);
            fx.apply(erase(6, 'b'), reduced, WINDOW);
            let kinds: Vec<(Kind, u32)> = fx.iter().map(|fx| (fx.kind, fx.effect)).collect();
            assert_eq!(
                kinds,
                [(Kind::Arrival, Keypress::Fade.id().expect("fade"))],
                "Hareketi Azalt: geliş belirir, hayalet yok ({style:?})"
            );
            let plain = motion(style, false);
            assert_eq!(
                plain.glyph_fx(Keypress::Fade, Erase::Recede),
                (Keypress::Fade, Erase::Recede)
            );
        }
        // Erişilebilirlik ayarı animasyon **eklemez**: kapalı geliş kapalı kalır.
        assert_eq!(
            motion(CursorMotion::Spring, true).glyph_fx(Keypress::Off, Erase::Recede),
            (Keypress::Off, Erase::Off)
        );
    }
}
