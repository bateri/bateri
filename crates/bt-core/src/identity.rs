//! Terminalin kimliği: kabuğa hangi terminalde koştuğunu söyleyen ortam
//! (`TERM_PROGRAM`, `TERM_PROGRAM_VERSION`) ve sekmenin dışarıdan açılabilen
//! adı (`TERM_SESSION_ID`, `BATERI_TAB_URL=bateri://tab/<id>`; 038).
//!
//! **Neden burada:** dördü `TERM`'ün ailesi ve onunla aynı "ezilemez"
//! katmanda yazılıyor (`Session::spawn`), yani sahibi `TERM`'ün sahibi. Aynı
//! sebeple `bateri://` şeması tek crate'te: şemanın öbür yolu, prompt'un iç
//! çıpası `bateri://block/<n>`, `session.rs`'teki `block_id`'de. Buradaki yol
//! (`tab/`) dış ad — uygulama onu URL olarak alıp yalnız o sekmeyi öne
//! getiriyor. UUID'yi **üreten** taraf `bt-shell` (`NSUUID`; bu crate
//! platformsuz ve rastgelelik kaynağı taşımıyor), biçimin sahibi burası.

/// `TERM_PROGRAM`'ın değeri.
pub const TERM_PROGRAM: &str = "bateri";

/// `TERM_PROGRAM_VERSION`'ın değeri: workspace sürümü. Bütün crate'ler
/// `version.workspace = true`; `bt-shell`'deki bir sınama eşitliği bekliyor
/// (`.tasks/038-terminal-kimligi/discussion.md` → Karar 3).
pub const TERM_PROGRAM_VERSION: &str = env!("CARGO_PKG_VERSION");

/// URL'nin şema + host öneki; yazımı tek yer ([`TabId::url`]).
const TAB_URL_PREFIX: &str = "bateri://tab/";

/// Bir sekmenin kalıcı kimliği: kanonik UUID metni (8-4-4-4-12 onaltılık),
/// büyük harfe normalize.
///
/// Tipli, çünkü aynı metin iki yöne gidiyor — kabuğun ortamına ve dışarıdan
/// gelen URL'nin eşleşmesine — ve iki taraf aynı biçimi görmeli: küçük harfli
/// bir URL ile büyük harfli bir kimlik ancak normalize edilince eşleşir.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TabId(String);

impl TabId {
    /// Kanonik UUID metnini kabul eder (harf büyüklüğü serbest); başka her
    /// biçim `None`.
    pub fn parse(text: &str) -> Option<TabId> {
        let bytes = text.as_bytes();
        if bytes.len() != 36 {
            return None;
        }
        let canonical = bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
        canonical.then(|| TabId(text.to_ascii_uppercase()))
    }

    /// Kimliğin metni (`TERM_SESSION_ID`'nin değeri).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `bateri://tab/<id>` — URL'yi yazan tek yer.
    pub fn url(&self) -> String {
        format!("{TAB_URL_PREFIX}{}", self.0)
    }

    /// `bateri://tab/<id>`'yi çözen tek yer. Şema ve host büyük-küçük harf
    /// duyarsız, UUID [`TabId::parse`]'tan; sorgu, parça, fazladan yol
    /// bileşeni, sondaki `/` ve `bateri://block/…` → `None`.
    pub fn from_url(url: &str) -> Option<TabId> {
        let prefix = url.get(..TAB_URL_PREFIX.len())?;
        if !prefix.eq_ignore_ascii_case(TAB_URL_PREFIX) {
            return None;
        }
        // `parse` uzunluğu ve karakter kümesini tam sınıyor: `?`, `#` ve `/`
        // onaltılık değil, yani sorgu/parça/fazla yol burada düşüyor.
        TabId::parse(&url[TAB_URL_PREFIX.len()..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0";

    #[test]
    fn url_round_trips() {
        let id = TabId::parse(ID).expect("kanonik UUID kabul edilmeli");
        assert_eq!(id.url(), format!("bateri://tab/{ID}"));
        assert_eq!(TabId::from_url(&id.url()), Some(id));
    }

    #[test]
    fn case_is_normalized() {
        let lower = ID.to_ascii_lowercase();
        let id = TabId::parse(&lower).expect("küçük harfli UUID kabul edilmeli");
        assert_eq!(id.as_str(), ID);
        assert_eq!(
            TabId::from_url(&format!("BATERI://TAB/{lower}")),
            Some(id.clone())
        );
        assert_eq!(TabId::from_url(&format!("bateri://tab/{lower}")), Some(id));
    }

    #[test]
    fn foreign_forms_are_rejected() {
        let rejected = [
            "bateri://block/3".to_owned(),
            "bateri://tab/".to_owned(),
            "bateri://tab".to_owned(),
            format!("bateri://tab/{ID}/"),
            format!("bateri://tab/{ID}?x"),
            format!("bateri://tab/{ID}#x"),
            format!("bateri://tab/{ID}/extra"),
            "bateri://tab/zzzzzzzz-zzzz-zzzz-zzzz-zzzzzzzzzzzz".to_owned(),
            format!("https://tab/{ID}"),
            format!("bateri://tab/{}", ID.replace('-', "")),
            // Çok baytlı karakter önekin sınırını bölmemeli (panik yok).
            "bateri://tağ/".to_owned(),
            "ğ".to_owned(),
            String::new(),
        ];
        for url in rejected {
            assert_eq!(TabId::from_url(&url), None, "{url} reddedilmeli");
        }
        assert_eq!(TabId::parse(&format!("{ID}0")), None);
        assert_eq!(TabId::parse(&ID.replace('-', "_")), None);
    }
}
