//! View ▸ Bigger / Smaller / Actual Size (Cmd +/−/0): ayardaki puntonun
//! üstüne tutulan **geçici** fark.
//!
//! Dosyaya yazılmıyor ve uygulama kapanınca gidiyor: ekranı bir an büyütmek
//! bir ayar değişikliği değil. İki punto kaynağı yarışmasın diye dosyadaki
//! `size` değişince fark sıfırlanıyor ([`Zoom::after_reload`]) — editörde
//! `size` yazan kullanıcı yazdığını görür. Saf; tutan ve renderer'a veren
//! `app`.

use bt_core::FontOptions;

/// Bir basışın puntosu. Seçilmiş bir sabit, ölçülmüş bir sayı değil.
const STEP: f64 = 1.0;

/// Basışların aralığı, punto. Uçları atlasın `punto × ölçek` kırpmasından
/// (`bt-atlas`, 4–144 piksel; aralığın sahibi orası): 72 punto 2× ekranda
/// tavan, 4 punto 1× ekranda taban. Aralığın içinde **her basış görünür**;
/// dışında kalan basış kırpmaya çarpıp hiçbir şey değiştirmezdi ve tuşu
/// basılı tutan kullanıcı geri dönmek için onları tek tek geri basardı.
///
/// Ayardaki `size` bu aralığa bağlı değil (kırpma orada da sessiz): aralığın
/// dışındaki bir puntodan içeri doğru basış çalışır, dışarı doğru olan
/// çalışmaz.
const MIN_SIZE: f64 = 4.0;
const MAX_SIZE: f64 = 72.0;

/// Ayardaki puntonun üstüne kaç adım çıkıldı (eksi: indi). Adım sayısı,
/// punto değil: tekrarlanan toplama ondalık birikim bırakmasın.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Zoom {
    steps: i32,
}

impl Zoom {
    /// Renderer'a gidecek font: ayarınki, puntosu farkla.
    pub(crate) fn apply(self, font: &FontOptions) -> FontOptions {
        FontOptions {
            family: font.family.clone(),
            size: self.size(font),
        }
    }

    /// Bigger: bir adım büyük, tavanı geçmiyorsa.
    pub(crate) fn bigger(self, font: &FontOptions) -> Zoom {
        let next = Zoom {
            steps: self.steps.saturating_add(1),
        };
        if next.size(font) <= MAX_SIZE {
            next
        } else {
            self
        }
    }

    /// Smaller: bir adım küçük, tabanın altına inmiyorsa. Taban sıfırın
    /// üstünde, yani [`FontOptions::size`]'ın "sıfırdan büyük" kuralı da
    /// buradan korunuyor.
    pub(crate) fn smaller(self, font: &FontOptions) -> Zoom {
        let next = Zoom {
            steps: self.steps.saturating_sub(1),
        };
        if next.size(font) >= MIN_SIZE {
            next
        } else {
            self
        }
    }

    /// Ayar dosyası yeniden okundu: `size` değiştiyse fark sıfırlanır, yoksa
    /// kalır — ailesini değiştiren kullanıcı büyüttüğü puntoyu kaybetmez.
    pub(crate) fn after_reload(self, old: &FontOptions, new: &FontOptions) -> Zoom {
        if old.size == new.size {
            self
        } else {
            Zoom::default()
        }
    }

    fn size(self, font: &FontOptions) -> f64 {
        font.size + f64::from(self.steps) * STEP
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(size: f64) -> FontOptions {
        FontOptions {
            family: Some("Menlo".to_owned()),
            size,
        }
    }

    #[test]
    fn steps_move_the_size_and_actual_size_returns() {
        let base = font(13.0);
        let zoom = Zoom::default().bigger(&base).bigger(&base);
        assert_eq!(zoom.apply(&base), font(15.0));
        assert_eq!(zoom.smaller(&base).apply(&base), font(14.0));
        // Aile ayarınki kalır; Actual Size farkı sıfırlar.
        assert_eq!(Zoom::default().apply(&base), base);
        assert_eq!(Zoom::default(), Zoom { steps: 0 });
    }

    #[test]
    fn size_change_in_the_file_resets_the_difference() {
        let zoom = Zoom::default().bigger(&font(13.0)).bigger(&font(13.0));
        // Editörde `size` yazıldı: yazılan görünür, fark gider.
        assert_eq!(zoom.after_reload(&font(13.0), &font(16.0)), Zoom::default());
        // Punto aynı kaldı (aile ya da başka bir anahtar değişti): fark kalır.
        let other = FontOptions {
            family: None,
            size: 13.0,
        };
        assert_eq!(zoom.after_reload(&font(13.0), &other), zoom);
        assert_eq!(zoom.after_reload(&font(13.0), &font(13.0)), zoom);
    }

    #[test]
    fn steps_stop_at_the_ends() {
        // Tavan: 72'ye kadar çıkar, bir fazlası basışı yok sayar.
        let base = font(70.0);
        let top = Zoom::default().bigger(&base).bigger(&base);
        assert_eq!(top.apply(&base).size, 72.0);
        assert_eq!(top.bigger(&base), top);
        // Taban: 4'e kadar iner.
        let base = font(5.0);
        let bottom = Zoom::default().smaller(&base);
        assert_eq!(bottom.apply(&base).size, 4.0);
        assert_eq!(bottom.smaller(&base), bottom);
        // Uçtaki basış **hiç** sayılmaz: geri dönüş hemen görünür.
        assert_eq!(
            top.bigger(&font(70.0))
                .smaller(&font(70.0))
                .apply(&font(70.0))
                .size,
            71.0
        );
        // Aralığın dışındaki ayardan içeri doğru basış çalışır; dışarı doğru
        // olan çalışmaz.
        assert_eq!(
            Zoom::default()
                .smaller(&font(100.0))
                .apply(&font(100.0))
                .size,
            99.0
        );
        assert_eq!(Zoom::default().bigger(&font(100.0)), Zoom::default());
        assert_eq!(
            Zoom::default().bigger(&font(2.0)).apply(&font(2.0)).size,
            3.0
        );
        assert_eq!(Zoom::default().smaller(&font(2.0)), Zoom::default());
    }
}
