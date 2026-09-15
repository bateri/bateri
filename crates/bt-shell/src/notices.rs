//! Pencere alt başlığında görünen tanılar: kaynak başına bir yuva.
//!
//! Modal bir uyarı yok: ayar dosyası canlı düzenleniyor ve her kayıtta açılan
//! bir pencere kullanıcıyı durdururdu (`discussion.md` → Karar 8). Alt başlık
//! araç çubuksuz pencerede başlıkla **aynı satırda** çiziliyor ("bateri –
//! …"), yani metin kısa kalmalı.
//!
//! **Yuva yalnız kendi kaynağı düzelince boşalır:** alakasız bir kaynağın
//! başarılı okuması başka kaynağın tanısını silmemeli. Alt başlığın tek
//! yazanı `app::AppDelegate::post_notices`; burası yalnız metni kuruyor.

use std::collections::BTreeMap;

/// Tanının geldiği yer. Sıra alt başlıkta hangi yuvanın önce görüneceği.
///
/// Font ve yazma yuvaları kendi phase'leriyle gelir — kullanılmayan varyant
/// ölü kod olurdu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Source {
    /// `settings.toml`: okunamadı, ayrıştırılamadı ya da bir anahtar kabul
    /// edilmedi.
    Settings,
    /// Seçilen tema: bulunamadı, dosyası okunamadı ya da ayrıştırılamadı, ya
    /// da bir rengi kabul edilmedi.
    Theme,
}

/// Dolu yuvalar; boş yuva haritada durmuyor.
#[derive(Debug, Default)]
pub(crate) struct Notices {
    slots: BTreeMap<Source, Vec<String>>,
}

impl Notices {
    /// Kaynağın yuvasını **tamamen** yeniden yazar; boş liste yuvayı boşaltır.
    /// Başka kaynağın yuvasına dokunmaz.
    pub(crate) fn replace(&mut self, source: Source, messages: Vec<String>) {
        if messages.is_empty() {
            self.slots.remove(&source);
        } else {
            self.slots.insert(source, messages);
        }
    }

    /// Alt başlığın metni: boşsa `""`, değilse ilk tanı ve kalanların sayısı.
    ///
    /// Hepsi yan yana yazılmıyor: tek satırda kesilirdi. Tamamı stderr'de.
    pub(crate) fn subtitle(&self) -> String {
        let mut messages = self.slots.values().flatten();
        let Some(first) = messages.next() else {
            return String::new();
        };
        match messages.count() {
            0 => first.clone(),
            rest => format!("{first} (+{rest} more)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_notices_clear_the_subtitle() {
        let mut notices = Notices::default();
        assert_eq!(notices.subtitle(), "");
        notices.replace(Source::Settings, vec!["bozuk".to_owned()]);
        assert_eq!(notices.subtitle(), "bozuk");
        // Kaynak düzelince yuva boşalır, alt başlık da.
        notices.replace(Source::Settings, Vec::new());
        assert_eq!(notices.subtitle(), "");
    }

    #[test]
    fn several_notices_show_the_first_and_the_rest_count() {
        let mut notices = Notices::default();
        notices.replace(
            Source::Settings,
            vec!["ilk".to_owned(), "ikinci".to_owned(), "üçüncü".to_owned()],
        );
        assert_eq!(notices.subtitle(), "ilk (+2 more)");
        // Yeniden yazmak eklemek değil: eski üçlü gider.
        notices.replace(Source::Settings, vec!["tek".to_owned()]);
        assert_eq!(notices.subtitle(), "tek");
    }

    #[test]
    fn a_source_clears_only_its_own_slot() {
        let mut notices = Notices::default();
        notices.replace(Source::Theme, vec!["tema".to_owned()]);
        notices.replace(Source::Settings, vec!["ayar".to_owned()]);
        // Ayar yuvası önce görünür; tema yuvası sayıda.
        assert_eq!(notices.subtitle(), "ayar (+1 more)");
        // Ayar dosyası düzeldi: temanın tanısı yerinde kalır.
        notices.replace(Source::Settings, Vec::new());
        assert_eq!(notices.subtitle(), "tema");
    }
}
