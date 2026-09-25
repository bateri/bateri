//! Emoji dizilerinin kümelenmesi (035): bir kod noktası açık kümeyi uzatır
//! mı, ve küme kaç sütun tutar.
//!
//! **Tek yetkili.** Izgaranın sarmalayıcısı ([`crate::handler`]), dock'un
//! düzeni (`dock::layout_with`), bastırmanın ızgara yürüyüşü
//! (`dock::grid_span`) ve tazelik kapısının ayna yarısı (`last_ink`) yalnız
//! buradaki iki fonksiyonu soruyor. İki kural ayrıştığı gün dock bir sütun
//! kayar ya da tazelik kapısı kalıcı olarak "bayat" der ve belirti sessizdir
//! (024 Karar 1'in yürüyüşteki karşılığı).
//!
//! **Kural yalnız emoji kollarından**, UAX #29'un tamamı değil
//! (`.tasks/035-grapheme-dizileri/discussion.md` → Muhakeme, ilk madde):
//! genel bir "tablo diziyi yuttu" kolu Arapça `لا`'yı ve `⌚︎`'yi de tek
//! kümeye indiriyordu, yani wcwidth sayan kabukla ayrışan ve geniş hücreyi
//! daraltmayı isteyen iki yan etki doğuruyordu. Emoji dışı kümeler bugünkü
//! gibi kod noktası kod noktası kalıyor.
//!
//! **Genişlik yalnız büyür.** Izgara hücreyi 1'den 2'ye çıkarabiliyor ama
//! hiçbir ara adımda daraltmıyor ([`width`]): daraltma geniş hücreyi geri
//! almak, yani alacritty'nin özel yollarını yeniden yazmak olurdu. Dock aynı
//! sayıyı buradan okuduğu için ızgarayla bit bit aynı sütunu tutuyor.

use std::num::NonZeroU32;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Sıfır genişlikli birleştirici (U+200D). Arkasındaki emoji sunumlu kod
/// noktası kümeye katılıyor — UAX #29 GB11'in karşılığı.
const ZWJ: char = '\u{200D}';

/// Emoji sunumu seçicisi (VS16). [`emoji_capable`]'ın sorusu "bu kod noktası
/// VS16'yla iki sütunlu bir emojiye dönüşür mü".
const VS16: char = '\u{FE0F}';

/// Bölgesel gösterge (RI) — bayrak çiftinin iki yarısı.
fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

/// Fitzpatrick ten rengi değiştiricisi.
fn is_skin_tone(c: char) -> bool {
    ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}

/// `c` emoji sunumu alabiliyor mu: `c ++ VS16` iki sütun. ZWJ'nin arkasındaki
/// `❤` VS16'sız **metin** sunumlu ve ara dizgi (`…‍❤`) tabloda yok; ölçüt
/// kod noktasının kendi genişliği olsaydı on kod noktalı öpücük ikiye
/// bölünürdü (`discussion.md` → Karar 3, dördüncü kol).
fn emoji_capable(c: char) -> bool {
    let mut buf = [0u8; 8];
    let len = c.encode_utf8(&mut buf).len();
    let vs16 = VS16.encode_utf8(&mut buf[len..]).len();
    // Tampon iki kod noktası için yeter (4 + 3 bayt); `get` panik yasağı için.
    buf.get(..len + vs16)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .is_some_and(|pair| pair.width() == 2)
}

/// `c` **herhangi bir** kümeyi uzatabilir mi — [`extends`]'in kümeye
/// bakmayan ön elemesi. Izgaranın sarmalayıcısı bunu her kod noktasında
/// soruyor ve cevap `false`'sa baş hücreyi hiç okumuyor: akan düz metinde
/// kod noktası başına bir tablo sorusundan fazlası ödenmiyor.
pub(crate) fn may_extend(c: char) -> bool {
    // ASCII'de yalnız rakam, `#` ve `*` emoji sunumu alabiliyor (tuş
    // başlığı `1️⃣`); geri kalanı hiçbir kola girmiyor.
    if c.is_ascii() {
        return c.is_ascii_digit() || c == '#' || c == '*';
    }
    UnicodeWidthChar::width(c) == Some(0)
        || is_regional_indicator(c)
        || is_skin_tone(c)
        || emoji_capable(c)
}

/// Açık küme `open` (boş değil) `c` ile uzar mı. Dört kol, başka kol yok:
///
/// 1. **Sıfır genişlik** — alacritty'nin bugünkü `zerowidth` dalı (VS16,
///    ZWJ, ten rengi dışındaki birleştiriciler, etiket karakterleri).
/// 2. **Eşlenmemiş RI'nin arkasındaki RI** — bayrak çifti. Küme tek RI'den
///    oluşuyorsa; üçüncü RI yeni bir bayrağın başı.
/// 3. **Ten rengi, iki sütunlu kümenin arkasında** — `a🏽` iki küme kalıyor.
/// 4. **ZWJ'nin arkasında emoji sunumu alabilen kod noktası** —
///    [`emoji_capable`]. `a‍b`'de `b` ayrı küme.
///
/// Kontrol karakteri (genişliği `None`) hiçbir kola girmiyor: satır sonu
/// kümeyi hep kapatır. Başı sütunsuz küme (akışın başındaki ya da satır
/// sonundan sonraki birleştirici) de uzamıyor: ızgarada öyle bir baş hücre
/// yok — birleştirici önceki hücreye iniyor — ve `‍👍`'nin ZWJ'si iki
/// sütunlu bir kümenin başı olurdu.
pub(crate) fn extends(open: &str, c: char) -> bool {
    if open
        .chars()
        .next()
        .is_none_or(|head| crate::dock::column_width(head) == 0)
    {
        return false;
    }
    if UnicodeWidthChar::width(c) == Some(0) {
        return true;
    }
    if is_regional_indicator(c) {
        let mut chars = open.chars();
        return chars.next().is_some_and(is_regional_indicator) && chars.next().is_none();
    }
    if is_skin_tone(c) {
        return width(open) == 2;
    }
    open.ends_with(ZWJ) && emoji_capable(c)
}

/// Kümenin ızgarada tuttuğu sütun: taban karakterin genişliği, kümenin
/// herhangi bir ön eki iki sütuna çıktıysa 2; hiç daralmıyor.
///
/// **Her ön ek soruluyor, yalnız sonuç değil:** ızgara genişlemeyi her
/// uzamadan sonra soruyor ve `1` + VS16 + `U+20E3` genişlemeyi VS16'da
/// yapıyor — son dizgiye bakan bir kural o adımı kaçırırdı.
///
/// **Tek kod noktalı küme [`crate::dock::column_width`]'in tablosuyla**
/// (kontrol karakteri 1, sekme dahil): `UnicodeWidthStr` sekmeye `0` diyor ve
/// dock'un kümesiz aritmetiği bit bit korunmalı.
pub(crate) fn width(cluster: &str) -> usize {
    let mut chars = cluster.char_indices();
    let Some((_, head)) = chars.next() else {
        return 0;
    };
    let base = crate::dock::column_width(head);
    if base >= 2 {
        return 2;
    }
    let widened = chars.any(|(at, c)| {
        cluster
            .get(..at + c.len_utf8())
            .is_some_and(|prefix| prefix.width() >= 2)
    });
    if widened { 2 } else { base }
}

/// Bir akışı kümelere bölen yürüyüş: her küme için akıştaki karakter
/// aralığı (`start..end`, yarı açık), baş karakteri ve sütunu ([`width`]).
///
/// Bastırmanın ızgara yürüyüşü ve tazelik kapısı bunu kullanıyor; dock'un
/// düzeni aynı döngüyü etiketli akışta kendisi koşuyor
/// (`dock::layout_with`), ızgara kümeyi hücreden türetiyor — üçü de aynı iki
/// fonksiyonu ([`extends`], [`width`]) okuyor.
///
/// **Kümenin metni tembel**: yalnız sıradaki kod noktası [`may_extend`]'den
/// geçerse kuruluyor. Düz metinde (kare başına koşan yürüyüşler, tuş başına
/// koşan ayna çözücüsü) tek bir ayırma bile yok; emojili satırda tampon bir
/// kez büyüyor.
pub(crate) struct Walk {
    open: String,
}

/// [`Walk`]'un küme başına çıktısı.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cluster {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) head: char,
    pub(crate) width: usize,
}

impl Walk {
    pub(crate) fn new() -> Self {
        Self {
            open: String::new(),
        }
    }

    /// `chars`'ı kümelere böler ve her birini `each`'e verir.
    pub(crate) fn run(
        &mut self,
        chars: impl IntoIterator<Item = char>,
        mut each: impl FnMut(Cluster),
    ) {
        let mut chars = chars.into_iter().enumerate().peekable();
        while let Some((start, head)) = chars.next() {
            let mut end = start + 1;
            let mut columns = crate::dock::column_width(head);
            if chars.peek().is_some_and(|&(_, next)| may_extend(next)) {
                self.open.clear();
                self.open.push(head);
                while let Some((at, c)) = chars.next_if(|&(_, next)| extends(&self.open, next)) {
                    self.open.push(c);
                    end = at + 1;
                }
                columns = width(&self.open);
            }
            each(Cluster {
                start,
                end,
                head,
                width: columns,
            });
        }
    }
}

/// Karede bir kümenin kimliği — [`Clusters`]'taki sırası.
///
/// `NonZeroU32` (sıra + 1): `Option<ClusterId>` niche ile 4 bayt, yani
/// sınır [`crate::Cell`]'i kümesiz hücrede de aynı kalıbı taşıyor ve kümesiz
/// hücre bir dal fazlasını bile ödemiyor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClusterId(NonZeroU32);

/// Kare başına küme tablosu (035 Karar 4B): sınır hücresinin
/// [`crate::Cell::cluster`]'ı buradaki bir dizgiyi gösteriyor.
///
/// **Sahibi çizen taraf, dolduran [`crate::Session`]** — [`crate::SelectionRuns`]
/// emsali: tablo `frame()`'in ve `dock()`'un `&mut` argümanı, çağıran onu
/// listeleriyle birlikte tutup temizliyor. Hücreye dizgi koymak (4A) her
/// çizilen hücreyi ~40 bayt büyütürdü; oturum ömürlü bir interner (4C)
/// sonsuz büyür ve kilit isterdi.
///
/// Dizgiler tek tamponda, uçlarıyla: kare başına küme başına ayırma yok,
/// `clear` kapasiteyi koruyor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clusters {
    text: String,
    /// Her kümenin `text`'teki **bitişi**; başı bir öncekinin bitişi.
    ends: Vec<u32>,
}

impl Clusters {
    pub fn clear(&mut self) {
        self.text.clear();
        self.ends.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// Dizgiyi ekler ve kimliğini verir. Kimlik ya da bayt uzayı tükendiyse
    /// `None` — hücre o zaman kümesiz, yani **taban karakteriyle** çiziliyor
    /// (şekillenmeyen kümenin cevabıyla aynı, 035 R1.1).
    pub fn push(&mut self, cluster: &str) -> Option<ClusterId> {
        self.push_chars(cluster.chars())
    }

    /// [`Clusters::push`]'un kod noktası kod noktası hâli: ızgaranın hücresi
    /// kümeyi taban + `zerowidth` olarak, dock'un düzeni akışın bir aralığı
    /// olarak taşıyor — ikisi de ara bir `String` kurmadan doğrudan tampona.
    pub(crate) fn push_chars(
        &mut self,
        chars: impl IntoIterator<Item = char>,
    ) -> Option<ClusterId> {
        let start = self.text.len();
        self.text.extend(chars);
        let id = u32::try_from(self.ends.len() + 1)
            .ok()
            .and_then(NonZeroU32::new);
        match (u32::try_from(self.text.len()), id) {
            (Ok(end), Some(id)) => {
                self.ends.push(end);
                Some(ClusterId(id))
            }
            _ => {
                self.text.truncate(start);
                None
            }
        }
    }

    /// Kimliğin dizgisi; başka bir tablonun (ya da temizlenmiş bir karenin)
    /// kimliği `None` — çizim yolunda, yani panik değil taban karakter.
    pub fn get(&self, id: ClusterId) -> Option<&str> {
        let index = id.0.get() as usize - 1;
        let end = *self.ends.get(index)? as usize;
        let start = match index.checked_sub(1) {
            Some(before) => *self.ends.get(before)? as usize,
            None => 0,
        };
        self.text.get(start..end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cluster_table_returns_what_was_pushed() {
        let mut table = Clusters::default();
        let flag = table.push("🇹🇷").expect("kimlik");
        let family = table.push("👨\u{200D}👩\u{200D}👧").expect("kimlik");
        assert_eq!(table.get(flag), Some("🇹🇷"));
        assert_eq!(table.get(family), Some("👨\u{200D}👩\u{200D}👧"));
        assert_eq!(std::mem::size_of::<Option<ClusterId>>(), 4);
        table.clear();
        assert!(table.is_empty());
        assert_eq!(table.get(flag), None, "temizlenmiş tablonun kimliği");
    }

    /// Dizgiyi kümelere böler: her kümenin metni ve sütunu.
    fn split(text: &str) -> Vec<(String, usize)> {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        Walk::new().run(text.chars(), |cluster| {
            let run = chars
                .get(cluster.start..cluster.end)
                .unwrap_or_default()
                .iter()
                .collect();
            out.push((run, cluster.width));
        });
        out
    }

    fn one(text: &str) -> Vec<(String, usize)> {
        vec![(text.to_owned(), 2)]
    }

    /// Scratchpad ölçümünün (`discussion.md` → Karar 3) on yedi örneği:
    /// her biri tek küme ve iki sütun ya da ölçülen bölünme.
    #[test]
    fn the_measured_sequences_are_single_two_column_clusters() {
        for sequence in [
            "🇹🇷",
            "🇬🇧",
            "👨\u{200D}👩\u{200D}👧",
            "👍🏽",
            "❤\u{FE0F}",
            "☺\u{FE0F}",
            "🏳\u{FE0F}\u{200D}🌈",
            "1\u{FE0F}\u{20E3}",
            "🌡\u{FE0F}",
            // İskoç bayrağı: etiket dizisi.
            "🏴\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}",
            // On kod noktalı öpücük: ZWJ'nin arkasındaki `❤` metin sunumlu.
            "🧑🏻\u{200D}❤\u{FE0F}\u{200D}💋\u{200D}🧑🏼",
        ] {
            assert_eq!(split(sequence), one(sequence), "{sequence:?}");
        }
        assert_eq!(
            split("🇹🇷🇬"),
            vec![("🇹🇷".to_owned(), 2), ("🇬".to_owned(), 1)],
            "çift + eşlenmemiş RI"
        );
        assert_eq!(
            split("a🏽"),
            vec![("a".to_owned(), 1), ("🏽".to_owned(), 2)],
            "ten rengi dar kümeye katılmaz"
        );
        assert_eq!(
            split("👨\u{200D}a"),
            vec![("👨\u{200D}".to_owned(), 2), ("a".to_owned(), 1)],
            "ZWJ'nin arkasındaki harf yeni küme"
        );
        assert_eq!(
            split("a\u{200D}b"),
            vec![("a\u{200D}".to_owned(), 1), ("b".to_owned(), 1)]
        );
        assert_eq!(split("e\u{301}"), vec![("e\u{301}".to_owned(), 1)], "aksan");
    }

    /// Emoji dışı küme ve VS15 bugünkü gibi: `لا` iki küme, `⌚︎` geniş kalıyor
    /// (daralmıyor) — genel "tablo yuttu" kolunun iki yan etkisi.
    #[test]
    fn non_emoji_clusters_keep_todays_cells() {
        assert_eq!(split("لا"), vec![("ل".to_owned(), 1), ("ا".to_owned(), 1)]);
        assert_eq!(
            split("⌚\u{FE0E}"),
            vec![("⌚\u{FE0E}".to_owned(), 2)],
            "VS15 zerowidth'e iner, hücre daralmaz"
        );
    }

    #[test]
    fn regional_indicators_pair_up() {
        assert_eq!(split("🇹"), vec![("🇹".to_owned(), 1)], "tek RI");
        assert_eq!(split("🇹🇷"), one("🇹🇷"), "çift");
        assert_eq!(
            split("🇹🇷🇬🇧"),
            vec![("🇹🇷".to_owned(), 2), ("🇬🇧".to_owned(), 2)],
            "iki bayrak"
        );
        assert_eq!(
            split("🇹🇷🇬"),
            vec![("🇹🇷".to_owned(), 2), ("🇬".to_owned(), 1)],
            "üçlü"
        );
    }

    /// Ön eleme hiçbir uzamayı kaçırmıyor: `extends` evet diyorsa
    /// `may_extend` da diyor. Sarmalayıcı ön elemeden dönerse küme sessizce
    /// bölünürdü.
    #[test]
    fn the_prefilter_never_drops_an_extension() {
        let opens = ["a", "1", "👍", "🇹", "👨\u{200D}", "❤", "日", "a\u{200D}"];
        let probes = (0x20..0x3000)
            .chain(0x1F1E0..0x1F200)
            .chain(0x1F300..0x1FA00)
            .chain([0x200D, 0xFE0F, 0xFE0E, 0x20E3, 0xE0067, 0xE007F])
            .filter_map(char::from_u32);
        for c in probes {
            for open in opens {
                if extends(open, c) {
                    assert!(may_extend(c), "{open:?} + {c:?}");
                }
            }
        }
    }

    /// Tek kod noktalı küme dock'un tablosuyla: sekme ve kontrol karakteri
    /// 1, satır sonu kümeyi kapatıyor.
    #[test]
    fn single_code_points_keep_the_column_width_table() {
        assert_eq!(width("\t"), 1);
        assert_eq!(width("a"), 1);
        assert_eq!(width("日"), 2);
        assert_eq!(width(""), 0);
        assert!(!extends("a", '\n'));
        assert_eq!(
            split("\u{301}\u{301}a"),
            vec![
                ("\u{301}".to_owned(), 0),
                ("\u{301}".to_owned(), 0),
                ("a".to_owned(), 1)
            ],
            "başsız birleştirici sütunsuz ve kendi başına"
        );
        assert_eq!(
            split("\u{200D}👍"),
            vec![("\u{200D}".to_owned(), 0), ("👍".to_owned(), 2)]
        );
    }
}
