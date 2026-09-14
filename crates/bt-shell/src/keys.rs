//! Tuş vuruşu → PTY baytları, ve Shift+PgUp/PgDn'in kaydırma kararı. **Saf ve
//! AppKit'siz**, bu yüzden sınanabilir.

use std::borrow::Cow;

/// AppKit'in fonksiyon tuşu aralığı: U+F700–U+F8FF. Adlandırılmış sabitler
/// (`NSUpArrowFunctionKey` … `NSModeSwitchFunctionKey`) bunun ilk dilimini
/// kullanıyor. Oklar ve PgUp/PgDn aşağıda kendi dizilerine çevriliyor; aralığın
/// **tanınmayan geri kalanı** bilerek yutuluyor çünkü bilmediğimiz bir tuş
/// kodunu UTF-8'e çevirip shell'e göndermek her zaman daha kötü. Private Use
/// Area bundan geniştir (U+E000'den başlar) ve **kapsam dışı**: powerline
/// glyph'i gibi gerçek bir karakter düz metin dalından geçer.
const FUNCTION_KEYS: std::ops::RangeInclusive<char> = '\u{f700}'..='\u{f8ff}';

/// `NSPageUpFunctionKey` ve `NSPageDownFunctionKey`. İki yerde okunuyor —
/// düz hâlin dizisi ([`encode_key`]) ve Shift'li hâlin kaydırması
/// ([`page_scroll`]) — ve iki yerde ayrı yazılan bir sayı birinde kayardı.
const PAGE_UP: char = '\u{f72c}';
const PAGE_DOWN: char = '\u{f72d}';

/// `chars` = `NSEvent.characters`, `ctrl` = Control basılı.
///
/// `None` → tuş yutulur; çağıran hiçbir şey yazmaz. Kapsam dışı: IME, ölü
/// tuşlar, Option-as-Meta, kitty klavye protokolü.
pub(crate) fn encode_key(chars: &str, ctrl: bool) -> Option<Cow<'static, [u8]>> {
    let c = chars.chars().next()?;
    // `characters` tek karakter mi — tek bir `c`'den bayt üreten kolların
    // ortak koruması (aşağıda Control ve PgUp/PgDn).
    let single = chars.len() == c.len_utf8();
    Some(match (c, ctrl) {
        // Sayısal tuş takımının Enter'ı ve Fn-Return `NSEnterCharacter` =
        // U+0003 verir — Ctrl-C'nin baytıyla aynı. Ctrl basılı DEĞİLSE bu bir
        // satır sonudur; bu kol olmasaydı düz metin dalından 0x03 olarak geçer
        // ve numpad Enter her komutu çalıştırmak yerine keserdi.
        ('\u{3}', false) => Cow::Borrowed(b"\r"),
        ('\u{f700}', _) => Cow::Borrowed(b"\x1b[A"),
        ('\u{f701}', _) => Cow::Borrowed(b"\x1b[B"),
        ('\u{f702}', _) => Cow::Borrowed(b"\x1b[D"),
        ('\u{f703}', _) => Cow::Borrowed(b"\x1b[C"),
        // `xterm-256color`'ın `kpp`/`knp`'si: less ve vim sayfa gezmeyi bu
        // iki diziden okuyor. Shift'li hâl buraya **gelmez** — o terminalin
        // kaydırması ([`page_scroll`]), `view` önce onu soruyor. Alternate
        // screen'de kaydırma reddedilince Shift'li tuş da buraya düşer ve
        // uygulama düz PgUp alır: Shift'in `;2` kodlaması, oklardaki
        // değiştiriciler gibi, kapsam dışı.
        //
        // `single` (yukarıda): `page_scroll` ile aynı ölçüt; çok karakterli
        // girdi aşağıdaki fonksiyon tuşu kolunda bütünüyle yutulur. Okların
        // kolları bu korumayı taşımıyor — bilinçli bir fark değil, bu phase'in
        // kapsamı dışında kalan borç: AppKit ok tuşunda tek karakter veriyor.
        (PAGE_UP, _) if single => Cow::Borrowed(b"\x1b[5~"),
        (PAGE_DOWN, _) if single => Cow::Borrowed(b"\x1b[6~"),
        // AppKit Control'ü `characters`'a çoğu tuşta kendi uygular (Ctrl-C →
        // U+0003) ve o hâl aşağıdaki düz metin dalından geçer. Ama hepsinde
        // uygulamaz — Ctrl-Shift-C'de harf harf kalır. İki yol da aynı baytı
        // versin diye dönüşüm burada tekrarlanıyor.
        // `single`: bu kol yalnız `c`'den bayt üretiyor, yani çok karakterli
        // bir `characters` (ölü tuş bileşimi, marked text) gelseydi ilk
        // karakterden sonrasını izsiz düşürürdü. Öyle bir girdi düz metin
        // dalına gitsin, orada tamamı geçiyor.
        (c, true) if c.is_ascii_alphabetic() && single => {
            Cow::Owned(vec![(c.to_ascii_lowercase() as u8) & 0x1f])
        }
        // Dizisini bilmediğimiz fonksiyon tuşu (F1, Home, End…). Bunlar
        // gerçek bir karakter değil, AppKit'in private use kodları: UTF-8'e
        // çevirip PTY'ye yazmak shell'e çöp göndermek olurdu.
        (c, _) if FUNCTION_KEYS.contains(&c) => return None,
        // Enter, Tab, Escape ve Backspace de buradan geçiyor: AppKit onları
        // zaten doğru bayta çevirmiş (U+000D, U+0009, U+001B, U+007F) ve
        // ayrı kollar yazmak aynı baytı ikinci kez tarif etmek olurdu.
        // Sözleşmeyi `return_and_delete_are_single_bytes` çiviliyor.
        _ => Cow::Owned(chars.as_bytes().to_vec()),
    })
}

/// Shift+PgUp/PgDn → kaydırılacak sayfa sayısı (±1); `None` → kaydırma tuşu
/// değil. Artı geriye, `Session::scroll_page`'in işaretiyle aynı.
///
/// **Saf karar**, sayfa boyunu bilmez: sayfanın kaç satır olduğu
/// `bt-core`'un kararı (`Session::scroll_page`). Shift dışındaki
/// değiştiriciler sorulmuyor — Control ya da Option'lı Shift+PgUp da
/// kaydırır; kaydırma tuşunun başka bir anlamı yok.
///
/// Eşleşme **tüm dizgiyle**: çok karakterli bir `characters` (bileşim)
/// ilk karakterinden kaydırma diye okunmaz — [`encode_key`]'in `single`
/// disiplininin aynısı.
pub(crate) fn page_scroll(chars: &str, shift: bool) -> Option<i32> {
    if !shift {
        return None;
    }
    let mut it = chars.chars();
    match (it.next(), it.next()) {
        (Some(PAGE_UP), None) => Some(1),
        (Some(PAGE_DOWN), None) => Some(-1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(chars: &str, ctrl: bool) -> Vec<u8> {
        encode_key(chars, ctrl)
            .unwrap_or_else(|| panic!("{chars:?} (ctrl={ctrl}) yutuldu"))
            .into_owned()
    }

    #[test]
    fn return_and_delete_are_single_bytes() {
        assert_eq!(encode("\r", false), b"\r");
        assert_eq!(encode("\u{7f}", false), b"\x7f");
        assert_eq!(encode("\t", false), b"\t");
        assert_eq!(encode("\u{1b}", false), b"\x1b");
    }

    #[test]
    fn arrows_emit_csi_sequences() {
        assert_eq!(encode("\u{f700}", false), b"\x1b[A");
        assert_eq!(encode("\u{f701}", false), b"\x1b[B");
        assert_eq!(encode("\u{f702}", false), b"\x1b[D");
        assert_eq!(encode("\u{f703}", false), b"\x1b[C");
    }

    #[test]
    fn ctrl_letter_yields_control_char_both_ways() {
        // AppKit Control'ü çoğu tuşta kendi uygular: `characters` doğrudan
        // U+0003 gelir. İki yol da aynı baytı vermeli, yoksa Ctrl-C'nin
        // çalışması AppKit'in o gün hangi yolu seçtiğine bağlı olur.
        assert_eq!(encode("\u{3}", true), b"\x03");
        assert_eq!(encode("c", true), b"\x03");
        assert_eq!(encode("C", true), b"\x03", "Shift ile de aynı");
    }

    #[test]
    fn numpad_enter_sends_newline_not_interrupt() {
        // U+0003 iki ayrı tuşun `characters`'ı: Ctrl-C ve numpad Enter.
        // Ayıran tek şey Control bayrağı; karıştırılırsa numpad Enter her
        // komutu çalıştırmak yerine keser.
        assert_eq!(encode("\u{3}", false), b"\r");
        assert_eq!(encode("\u{3}", true), b"\x03");
    }

    #[test]
    fn plain_text_passes_as_utf8() {
        assert_eq!(encode("a", false), b"a");
        // Türkçe karakter çok baytlı: bayt bayt geçmeli, `as u8` ile kırpılmamalı.
        assert_eq!(encode("ğ", false), "ğ".as_bytes());
        assert_eq!(encode("İ", false), "İ".as_bytes());
    }

    #[test]
    fn keys_without_sequences_are_swallowed() {
        // Saf modifier tuşu: `characters` boş.
        assert!(encode_key("", false).is_none());
        // F1 ve Home private use alanında. UTF-8'e çevirip PTY'ye yazmak
        // shell'e çöp göndermek olurdu; dizileri 00X'te.
        assert!(encode_key("\u{f704}", false).is_none(), "F1");
        assert!(encode_key("\u{f729}", false).is_none(), "Home");
    }

    #[test]
    fn page_keys_emit_xterm_sequences() {
        // PgUp/PgDn artık fonksiyon tuşu aralığında yutulmuyor: less ve vim
        // sayfa sayfa gezmek için bu iki diziyi bekliyor. Diziler
        // `xterm-256color`'ın `kpp`/`knp`'si — `TERM` oynamıyor.
        assert_eq!(encode("\u{f72c}", false), b"\x1b[5~");
        assert_eq!(encode("\u{f72d}", false), b"\x1b[6~");
        // Tek karakterlik eşleşme, `page_scroll` ile aynı ölçüt: PgUp ile
        // başlayan çok karakterli bir `characters` dizi üretip kalanını
        // izsiz düşürmez — tanınmayan fonksiyon tuşu gibi bütünüyle yutulur.
        assert!(encode_key("\u{f72c}x", false).is_none());
    }

    #[test]
    fn shift_page_keys_scroll_the_view() {
        // Shift'li PgUp/PgDn terminalin kendi tuşu: bir sayfa kaydırma. Yön
        // `Session::scroll_page`'in işaretiyle aynı — artı geriye.
        assert_eq!(page_scroll("\u{f72c}", true), Some(1));
        assert_eq!(page_scroll("\u{f72d}", true), Some(-1));
        // Shift'siz hâl uygulamanındır (yukarıdaki diziler).
        assert_eq!(page_scroll("\u{f72c}", false), None);
        assert_eq!(page_scroll("\u{f72d}", false), None);
        // Başka tuş, Shift'li de olsa, kaydırma değil.
        assert_eq!(page_scroll("\u{f700}", true), None);
        assert_eq!(page_scroll("a", true), None);
        // Tek karakterlik eşleşme: bir bileşimin ilk karakteri PgUp diye
        // okunmaz.
        assert_eq!(page_scroll("\u{f72c}x", true), None);
    }
}
