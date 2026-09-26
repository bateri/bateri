//! Tuş vuruşu → PTY baytları ya da ok, Shift+PgUp/PgDn'in kaydırma kararı ve
//! dock seçiminin tuşları ([`dock_key`]).
//! **Saf ve AppKit'siz**, bu yüzden sınanabilir.
//!
//! Okun baytı burada **yazılmaz**, yalnız hangi ok olduğu (`bt_core::Arrow`,
//! nedeni orada).

use std::borrow::Cow;

use bt_core::{Arrow, DockKey};

/// AppKit'in fonksiyon tuşu aralığı: U+F700–U+F8FF. Adlandırılmış sabitler
/// (`NSUpArrowFunctionKey` … `NSModeSwitchFunctionKey`) bunun ilk dilimini
/// kullanıyor. Oklar [`KeyInput::Arrow`]'a, PgUp/PgDn ve ileri silme kendi
/// dizilerine çevriliyor; aralığın **tanınmayan geri kalanı** bilerek
/// yutuluyor çünkü bilmediğimiz bir tuş kodunu UTF-8'e çevirip shell'e
/// göndermek her zaman daha kötü. Private Use Area bundan geniştir (U+E000'den
/// başlar) ve **kapsam dışı**: powerline glyph'i gibi gerçek bir karakter düz
/// metin dalından geçer.
const FUNCTION_KEYS: std::ops::RangeInclusive<char> = '\u{f700}'..='\u{f8ff}';

/// `NSPageUpFunctionKey` ve `NSPageDownFunctionKey`. İki yerde okunuyor —
/// düz hâlin dizisi ([`encode_key`]) ve Shift'li hâlin kaydırması
/// ([`page_scroll`]) — ve iki yerde ayrı yazılan bir sayı birinde kayardı.
const PAGE_UP: char = '\u{f72c}';
const PAGE_DOWN: char = '\u{f72d}';

/// `NSDeleteCharacter` — geri sekmenin (⌫) `characters`'ı. İleri silme (⌦)
/// **bu değil**, o `NSDeleteFunctionKey` (U+F728) ve fonksiyon tuşu
/// aralığında.
///
/// `PAGE_UP` emsali, iki yerde okunuyor: Cmd'nin kapalı izin listesi
/// (`view::reaches_terminal` — "bu tuş terminale gider mi") ve o listenin
/// baytı ([`encode_key`] — "hangi bayt"). Karar ile kodlamanın ayrı yerlerde
/// olması `page_scroll` ile aynı bölünme; literali iki yerde yazmak ise
/// birinde kaymasına açık kapı bırakırdı.
pub(crate) const BACKSPACE: char = '\u{7f}';

/// `NSLeftArrowFunctionKey` ve `NSRightArrowFunctionKey`.
///
/// `BACKSPACE` ile `PAGE_UP` emsali ve aynı gerekçe: ikisi de **iki yerde**
/// okunuyor — Cmd'nin kapalı izin listesi (`view::reaches_terminal`) ve o
/// listenin baytı ([`encode_key`]) — ve iki yerde ayrı yazılan bir literal
/// birinde kayardı. Yukarı/aşağı ok literal kalıyor: onları tek yer okuyor.
pub(crate) const ARROW_LEFT: char = '\u{f702}';
pub(crate) const ARROW_RIGHT: char = '\u{f703}';

/// `NSDeleteFunctionKey` — ileri silme (⌦, fn-⌫). İki yerde okunuyor:
/// baytı ([`encode_key`]) ve dock seçiminin tuşu ([`dock_key`]).
const FORWARD_DELETE: char = '\u{f728}';

/// Dizge **tam olarak tek karakter** mi — öyleyse o karakter, değilse `None`.
///
/// Ölçütün **tek sahibi** burası ve üç tüketicisi var: [`encode_key`]'in
/// `single` disiplini, [`page_scroll`]'un kaydırma kararı ve
/// `view::reaches_terminal`'ın ⌘ izin listesi. Üçü de aynı soruyu soruyor —
/// "bu bir tuşun karakteri mi, yoksa bir bileşimin çok karakterli çıktısı
/// mı" — ve üçü de ayrı yazılmıştı; `BACKSPACE` ile `PAGE_UP`'ın tek yerde
/// durma gerekçesinin aynısı, bir yerde kayan ölçüt ötekileri sessizce
/// ayrıştırırdı.
///
/// `chars().next()` **yetmiyor**: çok karakterli bir `characters` (ölü tuş
/// bileşiminin çıktısı, marked text) ilk karakterinden okunursa kalanı izsiz
/// düşer.
pub(crate) fn only_char(chars: &str) -> Option<char> {
    let mut it = chars.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// [`encode_key`]'in cevabı: baytı belli bir tuş ya da ok.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum KeyInput {
    /// Baytı kipten bağımsız: harf, Enter, PgUp…
    Bytes(Cow<'static, [u8]>),
    /// `Session::write_arrow`'a gider.
    Arrow(Arrow),
}

/// [`encode_key`]'in girdisi: `NSEvent`'in bu fonksiyonu ilgilendiren yarısı.
///
/// Dört ayrı parametre değil **tek kayıt**, çünkü kollar bayrakları artık
/// karakterle **birlikte** soruyor — ⌘⌫ ile ⌥⌫ aynı `characters`'tan
/// (`BACKSPACE`) yalnız bayrakla ayrılıyor — ve her yeni değiştirici çağrı
/// yerlerini tek tek gezdirirdi.
///
/// **Shift yok** ve eksik değil: hiçbir kol onu sormuyor. Shift'in tek
/// anlamlı olduğu yer kaydırma kararı ([`page_scroll`]) ve orayı Shift'siz
/// düşünmek mümkün değil; burada kullanılmayan bir bayrak kaydın sözleşmesini
/// ("bu bayraklar okunuyor") gevşetirdi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KeyPress<'a> {
    /// `NSEvent.characters` — değiştiriciler **uygulanmış** hâl (Option-basılı
    /// `ø`, Ctrl-C → U+0003).
    pub(crate) chars: &'a str,
    /// Control basılı.
    pub(crate) ctrl: bool,
    /// Option (⌥) basılı. Yalnız gezinme/silme sınıfını açıyor; basılabilir
    /// harfe dokunmuyor (aşağıda, R3.2).
    pub(crate) option: bool,
    /// Command (⌘) basılı. Buraya **yalnız** izin listesinden geçen tuş
    /// geliyor (`view::reaches_terminal`), yani bayrağın tek işi listenin üç
    /// tuşunu (⌘⌫, ⌘←, ⌘→) Option'lı hâllerinden ayırmak.
    pub(crate) command: bool,
}

/// Tuş vuruşu ([`KeyPress`]) → PTY baytları ya da ok.
///
/// **Artık her tuş buraya gelmiyor.** Metin yolu AppKit'in yığınından geçiyor
/// (`view::BateriView`'ın `NSTextInputClient` uyumu) ve basılabilir harfin
/// olağan üreticisi bu fonksiyon değil `insertText:`. Buraya kalan **beş**
/// küme var:
///
/// 1. **Control'lü olay** — `keyDown:` onu yığına hiç vermiyor (numpad
///    Enter'ın U+0003'ü ve Ctrl-Y'nin U+0019'u paylaşımlı, kolu AppKit'e
///    bırakmak her komutu kesebilirdi).
/// 2. **Yığının `doCommandBySelector:`'a verdiği tuş** — Enter, Tab, Escape,
///    Backspace, oklar, Shift+Tab, PgUp/PgDn, fn+Backspace **ve Option'lı
///    gezinme/silme** (`moveWordLeft:`, `deleteWordBackward:`). O metot sessiz
///    bir no-op ve olay bayraksız döndüğü için baytı buradan geliyor.
/// 3. **Oturumun reddettiği Shift+PgUp/PgDn** — alternate screen'de kaydırma
///    yok, tuş uygulamaya düz dizi olarak gidiyor.
/// 4. **Yığının tanımadığımız bir tiple verdiği metin** — `insertText:`'in
///    downcast'i tutmazsa olay tüketilmiş sayılmıyor ve buraya düşüyor.
/// 5. **Cmd'nin kapalı izin listesinden geçen tuş** — ⌘⌫, ⌘← ve ⌘→; Cmd'li
///    olay yığına **hiç girmiyor**, kararı `view::reaches_terminal` veriyor.
///
/// Düz metin dalı bu yüzden **kalkmadı**: 2. ve 4. kümenin baytı oradan
/// çıkıyor ve Ctrl'lü harf (1.) de aynı dala düşebiliyor.
///
/// `None` → tuş yutulur; çağıran hiçbir şey yazmaz.
///
/// **Kapsam dışı:** IME; **Option'ın topluca Meta olması** — Meta kodlaması
/// yalnız gezinme/silme sınıfına, basılabilir harf değişmiyor (`Option+7`
/// Türkçe Q'da `{` yazmaya devam ediyor, R3.2); kitty klavye protokolü;
/// **değiştiricili oklar** (`\e[1;5A`) — Option+ok'un `\eb`'si onların yerine
/// geçmiyor, o bir Meta dizisi, xterm'in değiştirici kodlaması değil;
/// **Control'lü geri sekme** ve **Option/Control'lü ileri silme** (⌦, U+F728);
/// Home/End (dizileri henüz yazılmadı — borç; aşağıda yutuluyor). **Ölü
/// tuşlar kapsam dışı kalmaya devam ediyor ve artık bir borç değil**:
/// bileşimi AppKit'in yığını tamamlıyor, buraya hiç uğramıyor.
pub(crate) fn encode_key(key: KeyPress<'_>) -> Option<KeyInput> {
    let c = key.chars.chars().next()?;
    // `characters` tek karakter mi — tek bir `c`'den çıkan kolların ortak
    // koruması (aşağıda geri sekme, oklar, PgUp/PgDn, ileri silme ve
    // Control): çok karakterli girdi (ölü tuş bileşimi, marked text) ilk
    // karakterinden okunmaz. Ölçütün sahibi [`only_char`] — `page_scroll` ve
    // `view::reaches_terminal` de aynı soruyu oradan soruyor.
    let single = only_char(key.chars).is_some();
    let bytes: Cow<'static, [u8]> = match (c, key.ctrl) {
        // Cmd'nin kapalı izin listesinin ilk tuşu: ⌘⌫ → `\x15` (`^U`,
        // zsh'te `kill-whole-line`). macOS'un katı anlamı "satır **başına
        // kadar** sil" ama zsh'te `backward-kill-line` varsayılanda hiç bağlı
        // değil (ölçüldü) — beklenti satırın gitmesi, `^U` tam onu yapıyor
        // (018 Karar 3). Kol Option'ınkinden **önce**: ⌘⌥⌫ satırı siler,
        // kelimeyi değil — izin listesi adı konmuş bir istisna, Option'ın
        // sınıfı bir kural.
        (BACKSPACE, _) if key.command && single => Cow::Borrowed(b"\x15"),
        // İzin listesinin diğer iki tuşu: ⌘← → `\x01` (`^A`,
        // `beginning-of-line`), ⌘→ → `\x05` (`^E`, `end-of-line`). ⌘⌫'in
        // kararının aynısı — macOS'un satır başı/sonu jesti, zsh'in o işi
        // **gerçekten** yapan baytıyla.
        //
        // 018 Karar 3 bu iki tuşu reddetmişti ve gerekçesi iki parçaydı:
        // "istenmedi" ile "Home/End dizileri zsh'te karşılıksız". İlki
        // düştü (kullanıcı istedi, 2026-09-21), ikincisi **hiç bu tuşların
        // gerekçesi değildi**: ölçüm `^[[H`/`^[[F`/`^[OH`/`^[OF` için sıfır
        // bağlama gösteriyor ama `^A`/`^E` emacs keymap'inde (zsh'in
        // varsayılanı) `beginning-of-line`/`end-of-line`'a bağlı — yani
        // reddin ölçümü Home/End'in şekline aitti, bu baytlara değil.
        // Ghostty, VS Code ve Warp da aynı iki baytı gönderiyor.
        //
        // **Bilinen bedel, ⌘⌫'inkiyle aynı sınıfta:** `viins` keymap'inde
        // `^A`/`^E` `self-insert`, yani vi kipinde satıra bir kontrol
        // karakteri düşer (ölçüldü). Option'ın `\eb`/`\ef`'i de orada
        // `undefined-key` ve o takas kabul edilmişti; vi kipinin satır
        // başı/sonu tuşu `0`/`$`.
        (ARROW_LEFT, _) if key.command && single => Cow::Borrowed(b"\x01"),
        (ARROW_RIGHT, _) if key.command && single => Cow::Borrowed(b"\x05"),
        // Option'ın **gezinme/silme** sınıfı → Meta dizileri. Ayar sorulmuyor,
        // çünkü bu tuşlar hiçbir klavye düzeninde basılabilir karakter
        // üretmiyor: çatışma Option'lı **harfte** ve orası dokunulmadan
        // kalıyor (018 Karar 2). Diziler **küçük harf**: büyük harfli hâl
        // zsh'te başka widget'lara bağlı (`\eA` = `accept-and-hold`; ölçüldü).
        //
        // Buraya 2. kümeden geliyorlar: yığın Option'lı oku/⌫'i
        // `doCommandBySelector:`'a (`moveWordLeft:`, `deleteWordBackward:`)
        // veriyor, o metot sessiz no-op ve olay tüketilmemiş dönüyor. O
        // metoda gövde yazmak bu kolu sessizce öldürürdü.
        //
        // Yanındaki değiştiriciler sorulmuyor (`page_scroll` emsali):
        // Ctrl'lü ya da Shift'li Option+ok da kelime gezer, ⌥'in bu sınıfta
        // ikinci bir anlamı yok.
        (BACKSPACE, _) if key.option && single => Cow::Borrowed(b"\x1b\x7f"),
        (ARROW_LEFT, _) if key.option && single => Cow::Borrowed(b"\x1bb"),
        (ARROW_RIGHT, _) if key.option && single => Cow::Borrowed(b"\x1bf"),
        // Sayısal tuş takımının Enter'ı ve Fn-Return `NSEnterCharacter` =
        // U+0003 verir — Ctrl-C'nin baytıyla aynı. Ctrl basılı DEĞİLSE bu bir
        // satır sonudur; bu kol olmasaydı düz metin dalından 0x03 olarak geçer
        // ve numpad Enter her komutu çalıştırmak yerine keserdi.
        ('\u{3}', false) => Cow::Borrowed(b"\r"),
        // Shift+Tab: `characters` `NSBackTabCharacter` = U+0019 verir.
        // `xterm-256color`'ın `kcbt`'si `\e[Z` — zsh'ın tamamlama menüsü ve
        // readline geri sekmeyi bu diziden okuyor, ham 0x19'dan değil. U+0019
        // Ctrl-Y'nin de baytı (yank); ayıran yine Control bayrağı, yukarıdaki
        // U+0003 kolunun aynısı. Ctrl'lü hâl düz metin dalından 0x19 gider.
        ('\u{19}', false) if single => Cow::Borrowed(b"\x1b[Z"),
        ('\u{f700}', _) if single => return Some(KeyInput::Arrow(Arrow::Up)),
        ('\u{f701}', _) if single => return Some(KeyInput::Arrow(Arrow::Down)),
        (ARROW_LEFT, _) if single => return Some(KeyInput::Arrow(Arrow::Left)),
        (ARROW_RIGHT, _) if single => return Some(KeyInput::Arrow(Arrow::Right)),
        // `xterm-256color`'ın `kpp`/`knp`'si: less ve vim sayfa gezmeyi bu
        // iki diziden okuyor. Shift'li hâl buraya **gelmez** — o terminalin
        // kaydırması ([`page_scroll`]), `view` önce onu soruyor. Alternate
        // screen'de kaydırma reddedilince Shift'li tuş da buraya düşer ve
        // uygulama düz PgUp alır: Shift'in `;2` kodlaması, oklardaki
        // değiştiriciler gibi, kapsam dışı.
        //
        // `single` (yukarıda): `page_scroll` ile aynı ölçüt; çok karakterli
        // girdi aşağıdaki fonksiyon tuşu kolunda bütünüyle yutulur.
        (PAGE_UP, _) if single => Cow::Borrowed(b"\x1b[5~"),
        (PAGE_DOWN, _) if single => Cow::Borrowed(b"\x1b[6~"),
        // `kdch1`: imlecin sağındaki harfi siler. Fonksiyon tuşu aralığında
        // olduğu için aşağıdaki yutma kolundan **önce** yazılı; değiştiricili
        // hâli (`\e[3;5~`) oklar gibi kapsam dışı ve düz diziyi alır.
        // `NSDeleteFunctionKey`: fn+Backspace, tam klavyede Delete (⌦).
        (FORWARD_DELETE, _) if single => Cow::Borrowed(b"\x1b[3~"),
        // AppKit Control'ü `characters`'a çoğu tuşta kendi uygular (Ctrl-C →
        // U+0003) ve o hâl aşağıdaki düz metin dalından geçer. Ama hepsinde
        // uygulamaz — Ctrl-Shift-C'de harf harf kalır. İki yol da aynı baytı
        // versin diye dönüşüm burada tekrarlanıyor.
        // `single`: bu kol yalnız `c`'den bayt üretiyor, yani çok karakterli
        // bir `characters` gelseydi ilk karakterden sonrasını izsiz
        // düşürürdü. Öyle bir girdi düz metin dalına gitsin, orada tamamı
        // geçiyor.
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
        _ => Cow::Owned(key.chars.as_bytes().to_vec()),
    };
    Some(KeyInput::Bytes(bytes))
}

/// Shift+PgUp/PgDn → kaydırılacak sayfa sayısı (±1); `None` → kaydırma tuşu
/// değil. Artı geriye, `Session::scroll_page`'in işaretiyle aynı.
///
/// **Saf karar**, sayfa boyunu bilmez: sayfanın kaç satır olduğu
/// `bt-core`'un kararı (`Session::scroll_page`). Shift dışındaki
/// değiştiriciler sorulmuyor — Control ya da Option'lı Shift+PgUp da
/// kaydırır; kaydırma tuşunun başka bir anlamı yok.
///
/// Eşleşme **tüm dizgiyle** ([`only_char`]): çok karakterli bir `characters`
/// (bileşim) ilk karakterinden kaydırma diye okunmaz — [`encode_key`]'in
/// `single` disiplininin aynısı, artık aynı yerden.
pub(crate) fn page_scroll(chars: &str, shift: bool) -> Option<i32> {
    if !shift {
        return None;
    }
    match only_char(chars)? {
        PAGE_UP => Some(1),
        PAGE_DOWN => Some(-1),
        _ => None,
    }
}

/// Dock seçimi varken terminalin karşılayabileceği tuş (031 Karar 8) —
/// hangisinin ne yapacağı `bt-core`'da (`Session::dock_key`), burası yalnız
/// `NSEvent`'in sözlüğü.
///
/// **Değiştiricisiz** ⌫, ⌦, ←, →, Shift'li iki ok, ⏎ ve ⇧⏎. Option, Control ya da
/// Command taşıyan tuş hiç dock tuşu değil: ⌥⌫ `backward-kill-word`, ⌘⌫
/// `kill-whole-line` ve ikisi de bugünkü yolundan gidip seçimi kaldırıyor —
/// "başka her tuş" kolu. Shift'li ⌫ düz ⌫ sayılıyor: macOS'ta da aynı tuş.
pub(crate) fn dock_key(key: KeyPress<'_>, shift: bool) -> Option<DockKey> {
    if key.ctrl || key.option || key.command {
        return None;
    }
    match (only_char(key.chars)?, shift) {
        (BACKSPACE, _) => Some(DockKey::Backspace),
        (FORWARD_DELETE, _) => Some(DockKey::Delete),
        (ARROW_LEFT, false) => Some(DockKey::Left),
        (ARROW_RIGHT, false) => Some(DockKey::Right),
        (ARROW_LEFT, true) => Some(DockKey::ShiftLeft),
        (ARROW_RIGHT, true) => Some(DockKey::ShiftRight),
        // ⇧⏎: dock'ta satırı çalıştırmadan yeni satır (iTerm'in ve Claude
        // Code'un alışkanlığı). Kapı kapalıysa `bt-core` tüketmiyor ve tuş
        // bugünkü gibi Enter olarak gidiyor.
        ('\r', true) => Some(DockKey::NewLine),
        // Düz ⏎: yalnız yeniden bağlanma teklifi varken tüketiliyor (037
        // Karar 8); yoksa `bt-core` ilk soruda `false` diyor ve Enter bugünkü
        // yolundan gidiyor.
        // Numpad Enter'ın `characters`'ı U+0003 (Control'süz; `encode_key`
        // onu `\r`'ye çeviriyor) — aynı tuşun ikinci yüzü.
        ('\r' | '\u{3}', false) => Some(DockKey::Enter),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dock_keys_are_the_plain_eight() {
        assert_eq!(dock_key(plain("\u{7f}"), false), Some(DockKey::Backspace));
        assert_eq!(dock_key(plain("\u{7f}"), true), Some(DockKey::Backspace));
        assert_eq!(dock_key(plain("\u{f728}"), false), Some(DockKey::Delete));
        assert_eq!(dock_key(plain("\u{f702}"), false), Some(DockKey::Left));
        assert_eq!(dock_key(plain("\u{f703}"), false), Some(DockKey::Right));
        assert_eq!(dock_key(plain("\u{f702}"), true), Some(DockKey::ShiftLeft));
        assert_eq!(dock_key(plain("\u{f703}"), true), Some(DockKey::ShiftRight));
        assert_eq!(dock_key(plain("\r"), true), Some(DockKey::NewLine));
        assert_eq!(dock_key(plain("\r"), false), Some(DockKey::Enter));
        assert_eq!(dock_key(plain("\u{3}"), false), Some(DockKey::Enter));
        // Ctrl-C (aynı `characters`, Control'lü) dock tuşu değil.
        assert_eq!(dock_key(ctrl("\u{3}"), false), None);
        // ⌥⏎ ve ⌘⏎ dock tuşu değil.
        for flag in [
            KeyPress {
                option: true,
                ..plain("\r")
            },
            KeyPress {
                command: true,
                ..plain("\r")
            },
        ] {
            assert_eq!(dock_key(flag, false), None);
        }
        // ⌥⇧⏎ ve ⌃⇧⏎ dock tuşu değil.
        assert_eq!(
            dock_key(
                KeyPress {
                    option: true,
                    ..plain("\r")
                },
                true
            ),
            None
        );
        // "Başka her tuş": değiştiricili olanlar ve metin.
        for key in [
            KeyPress {
                option: true,
                ..plain("\u{7f}")
            },
            KeyPress {
                command: true,
                ..plain("\u{7f}")
            },
            KeyPress {
                ctrl: true,
                ..plain("\u{f702}")
            },
            plain("a"),
            plain("\u{f700}"),
            plain("\u{7f}\u{7f}"),
        ] {
            assert_eq!(dock_key(key, false), None, "{:?}", key.chars);
        }
    }

    /// Değiştiricisiz vuruş. Tek bayrağı açan kollar `..plain(chars)` ile
    /// yazılıyor, yani sınama satırı hangi bayrağın konu olduğunu adıyla
    /// söylüyor.
    fn plain(chars: &str) -> KeyPress<'_> {
        KeyPress {
            chars,
            ctrl: false,
            option: false,
            command: false,
        }
    }

    fn ctrl(chars: &str) -> KeyPress<'_> {
        KeyPress {
            ctrl: true,
            ..plain(chars)
        }
    }

    fn option(chars: &str) -> KeyPress<'_> {
        KeyPress {
            option: true,
            ..plain(chars)
        }
    }

    fn command(chars: &str) -> KeyPress<'_> {
        KeyPress {
            command: true,
            ..plain(chars)
        }
    }

    fn encode(key: KeyPress<'_>) -> Vec<u8> {
        match encode_key(key) {
            Some(KeyInput::Bytes(bytes)) => bytes.into_owned(),
            other => panic!("{key:?} bayt vermedi: {other:?}"),
        }
    }

    #[test]
    fn only_char_is_the_single_owner_of_the_one_character_test() {
        // Ölçütün üç tüketicisi var (`encode_key`'in `single`'ı,
        // `page_scroll`, `view::reaches_terminal`) ve üçü de buraya bağlı;
        // sözleşme burada çivileniyor.
        assert_eq!(only_char("a"), Some('a'));
        // Çok baytlı **tek** karakter de tek karakter: ölçüt bayt değil
        // karakter sayısı.
        assert_eq!(only_char("ğ"), Some('ğ'));
        assert_eq!(only_char("\u{7f}"), Some('\u{7f}'));
        // İki karakter tek değil — bileşimin çıktısı ilk karakterinden
        // okunmasın diye ölçüt tam da bu.
        assert_eq!(only_char("ab"), None);
        assert_eq!(only_char("\u{f72c}x"), None);
        // Boş dizge (saf modifier tuşu) de tek karakter değil.
        assert_eq!(only_char(""), None);
    }

    #[test]
    fn return_and_delete_are_single_bytes() {
        assert_eq!(encode(plain("\r")), b"\r");
        assert_eq!(encode(plain("\u{7f}")), b"\x7f");
        assert_eq!(encode(plain("\t")), b"\t");
        assert_eq!(encode(plain("\u{1b}")), b"\x1b");
    }

    #[test]
    fn arrows_are_keys_not_bytes() {
        // Okun baytı DECCKM'e bağlı (`\e[A` ya da `\eOA`) ve kip `bt-core`'da:
        // burada koşulsuz `\e[A` yazılıyordu, oysa `xterm-256color`'ın
        // terminfo'sunu okuyan less açılışta DECCKM'i açıp `\eOA` bekliyor.
        assert_eq!(
            encode_key(plain("\u{f700}")),
            Some(KeyInput::Arrow(Arrow::Up))
        );
        assert_eq!(
            encode_key(plain("\u{f701}")),
            Some(KeyInput::Arrow(Arrow::Down))
        );
        assert_eq!(
            encode_key(plain("\u{f702}")),
            Some(KeyInput::Arrow(Arrow::Left))
        );
        assert_eq!(
            encode_key(plain("\u{f703}")),
            Some(KeyInput::Arrow(Arrow::Right))
        );
        // Control'lü ok da düz ok: değiştiricili oklar (`\e[1;5A`) kapsam dışı.
        assert_eq!(
            encode_key(ctrl("\u{f700}")),
            Some(KeyInput::Arrow(Arrow::Up))
        );
        // Tek karakterlik eşleşme, PgUp'la aynı ölçüt: okla başlayan bir
        // bileşim ilk karakterinden ok diye okunmaz.
        assert_eq!(encode_key(plain("\u{f700}x")), None);
    }

    #[test]
    fn ctrl_letter_yields_control_char_both_ways() {
        // AppKit Control'ü çoğu tuşta kendi uygular: `characters` doğrudan
        // U+0003 gelir. İki yol da aynı baytı vermeli, yoksa Ctrl-C'nin
        // çalışması AppKit'in o gün hangi yolu seçtiğine bağlı olur.
        assert_eq!(encode(ctrl("\u{3}")), b"\x03");
        assert_eq!(encode(ctrl("c")), b"\x03");
        assert_eq!(encode(ctrl("C")), b"\x03", "Shift ile de aynı");
    }

    #[test]
    fn numpad_enter_sends_newline_not_interrupt() {
        // U+0003 iki ayrı tuşun `characters`'ı: Ctrl-C ve numpad Enter.
        // Ayıran tek şey Control bayrağı; karıştırılırsa numpad Enter her
        // komutu çalıştırmak yerine keser.
        assert_eq!(encode(plain("\u{3}")), b"\r");
        assert_eq!(encode(ctrl("\u{3}")), b"\x03");
    }

    #[test]
    fn plain_text_passes_as_utf8() {
        // Düz harfin **olağan** üreticisi artık burası değil, AppKit
        // yığınının `insertText:`'i (018). Bu sınama yine de bekçi: düz metin
        // dalı 2. ve 4. kümenin (bkz. [`encode_key`]) tek yolu ve o iki küme
        // de çok baytlı harf taşıyabiliyor — `insertText:`'in downcast'i
        // tutmadığında `ğ` buradan geçer.
        assert_eq!(encode(plain("a")), b"a");
        // Türkçe karakter çok baytlı: bayt bayt geçmeli, `as u8` ile kırpılmamalı.
        assert_eq!(encode(plain("ğ")), "ğ".as_bytes());
        assert_eq!(encode(plain("İ")), "İ".as_bytes());
    }

    #[test]
    fn keys_without_sequences_are_swallowed() {
        // Saf modifier tuşu: `characters` boş. Yığın yolunda bu olay
        // `keyDown:`'ın fallback'ine `None` olarak varıyor ve oraya hiç
        // gelmiyor; sınama yine de yutmanın sözleşmesini çiviliyor.
        assert!(encode_key(plain("")).is_none());
        // F1 ve Home private use alanında. UTF-8'e çevirip PTY'ye yazmak
        // shell'e çöp göndermek olurdu; dizileri 00X'te. **Yığın bunları
        // yutmuyor**: ikisi de `doCommandBySelector:`'a düşüyor, no-op'tan
        // geçiyor ve yutma kararı hâlâ burada.
        assert!(encode_key(plain("\u{f704}")).is_none(), "F1");
        assert!(encode_key(plain("\u{f729}")).is_none(), "Home");
    }

    #[test]
    fn page_keys_emit_xterm_sequences() {
        // PgUp/PgDn artık fonksiyon tuşu aralığında yutulmuyor: less ve vim
        // sayfa sayfa gezmek için bu iki diziyi bekliyor. Diziler
        // `xterm-256color`'ın `kpp`/`knp`'si — `TERM` oynamıyor.
        assert_eq!(encode(plain("\u{f72c}")), b"\x1b[5~");
        assert_eq!(encode(plain("\u{f72d}")), b"\x1b[6~");
        // Tek karakterlik eşleşme, `page_scroll` ile aynı ölçüt: PgUp ile
        // başlayan çok karakterli bir `characters` dizi üretip kalanını
        // izsiz düşürmez — tanınmayan fonksiyon tuşu gibi bütünüyle yutulur.
        assert!(encode_key(plain("\u{f72c}x")).is_none());
    }

    #[test]
    fn back_tab_and_forward_delete_emit_xterm_sequences() {
        // `xterm-256color`'ın `kcbt`'si ve `kdch1`'i (`infocmp`). Shift+Tab'ın
        // `characters`'ı `NSBackTabCharacter` (U+0019): düz metin dalından
        // ham `0x19` gidiyordu, zsh'ın menüsü onu geri gitme diye okumuyor.
        // fn+Backspace `NSDeleteFunctionKey` (U+F728): fonksiyon tuşu kolunda
        // yutuluyordu.
        assert_eq!(encode(plain("\u{19}")), b"\x1b[Z");
        assert_eq!(encode(plain("\u{f728}")), b"\x1b[3~");
        // U+0019 Ctrl-Y'nin de `characters`'ı (`'y' & 0x1f`) — readline'ın
        // yank'i. Ayıran yalnız Control bayrağı, numpad Enter/Ctrl-C ikilisi
        // gibi; karışırsa Ctrl-Y geri sekme olur.
        assert_eq!(encode(ctrl("\u{19}")), b"\x19");
        assert_eq!(encode(ctrl("y")), b"\x19");
        // Tek karakterlik eşleşme, PgUp'la aynı ölçüt.
        assert!(encode_key(plain("\u{f728}x")).is_none());
        // İleri silme (⌦) Option'lı da düz `kdch1`: Meta sınıfı geri sekmeye
        // ve oklara, ileri silme **kapsam dışı** (`encode_key`'in doc'u).
        assert_eq!(encode(option("\u{f728}")), b"\x1b[3~");
    }

    #[test]
    fn option_navigation_sends_meta_sequences() {
        // Option'ın gezinme/silme sınıfı hiçbir düzende basılabilir karakter
        // üretmiyor, yani ayar sorulmadan Meta kodlanıyor (018 Karar 2).
        // Diziler varsayılan zsh'te ölçüldü: `\eb` `backward-word`, `\ef`
        // `forward-word`, `\e\x7f` `backward-kill-word`. Harf **küçük** —
        // büyük harfli hâl başka widget'lara bağlı (`\eA` =
        // `accept-and-hold`), yani `\eB` kelime gezmezdi.
        assert_eq!(encode(option("\u{f702}")), b"\x1bb", "Option+←");
        assert_eq!(encode(option("\u{f703}")), b"\x1bf", "Option+→");
        assert_eq!(encode(option("\u{7f}")), b"\x1b\x7f", "Option+Delete");
        // Option'sız hâl dokunulmadan duruyor: ok yine ok (baytı DECCKM'e
        // bağlı), ⌫ yine tek bayt.
        assert_eq!(
            encode_key(plain("\u{f702}")),
            Some(KeyInput::Arrow(Arrow::Left))
        );
        assert_eq!(encode(plain("\u{7f}")), b"\x7f");
        // Yukarı/aşağı ok Option'lı da ok: kelime gezme yatay bir jest,
        // dikeyde karşılığı yok.
        assert_eq!(
            encode_key(option("\u{f700}")),
            Some(KeyInput::Arrow(Arrow::Up))
        );
        // Tek karakterlik eşleşme, PgUp'la aynı ölçüt.
        assert!(encode_key(option("\u{f702}x")).is_none());
    }

    #[test]
    fn option_printable_characters_are_untouched() {
        // R3.2: Türkçe Q'da `{` = Option+7, `∫` = Option+b. Option topluca
        // Meta olsaydı kabuğun metakarakterleri yazılamaz hâle gelirdi (018
        // Karar 2) — Meta yalnız gezinme/silme sınıfına.
        //
        // Bu harflerin **olağan** üreticisi artık `insertText:`; buraya
        // yalnız yığının çözemediği tip düşüyor (4. küme) ve o zaman da harf
        // harf geçmeli.
        assert_eq!(encode(option("{")), b"{");
        assert_eq!(encode(option("∫")), "∫".as_bytes());
    }

    #[test]
    fn command_backspace_kills_the_whole_line() {
        // İzin listesinin ilk tuşu. `\x15` = `^U`, zsh'te
        // `kill-whole-line`: macOS'un "satır başına kadar sil"i zsh'te
        // varsayılanda bağlı değil (ölçüldü) ve kullanıcının ölçütü satırın
        // gitmesi (018 Karar 3).
        assert_eq!(encode(command("\u{7f}")), b"\x15");
        // Cmd, Option'ın **önünde**: ⌘⌥⌫ satırı siler, kelimeyi değil.
        assert_eq!(
            encode(KeyPress {
                option: true,
                ..command("\u{7f}")
            }),
            b"\x15"
        );
        // Cmd'siz ⌫ yine tek bayt — bayrak karakteri değil kararı taşıyor.
        assert_eq!(encode(plain("\u{7f}")), b"\x7f");
        // Buraya **yalnız** izin listesinden geçen tuş geliyor
        // (`view::reaches_terminal`); Cmd'li başka bir karakter gelseydi
        // bayrak onu değiştirmezdi.
        assert_eq!(encode(command("t")), b"t");
    }

    #[test]
    fn command_arrows_jump_to_the_line_edges() {
        // İzin listesinin diğer iki tuşu: `\x01` = `^A` (`beginning-of-line`),
        // `\x05` = `^E` (`end-of-line`). Şekil `Arrow` **değil**: bu iki bayt
        // DECCKM'den bağımsız, tıpkı ⌘⌫'in `\x15`'i gibi — ok baytının kipe
        // bağlı olması aşağıdaki Cmd'siz kolun konusu.
        let (left, right) = (ARROW_LEFT.to_string(), ARROW_RIGHT.to_string());
        assert_eq!(encode(command(&left)), b"\x01");
        assert_eq!(encode(command(&right)), b"\x05");
        // Cmd, Option'ın **önünde** (⌘⌥⌫ emsali): ⌘⌥← satır başına gider,
        // kelime başına değil.
        assert_eq!(
            encode(KeyPress {
                option: true,
                ..command(&left)
            }),
            b"\x01"
        );
        // Cmd'siz ok yine **ok**, bayt değil: kipi `bt-core` biliyor.
        assert_eq!(encode_key(plain(&left)), Some(KeyInput::Arrow(Arrow::Left)));
        assert_eq!(
            encode_key(plain(&right)),
            Some(KeyInput::Arrow(Arrow::Right))
        );
        // Tek karakterlik eşleşme, PgUp ve ⌘⌫ ile aynı ölçüt: bileşimin ilk
        // karakteri satır başı diye okunmaz.
        assert!(encode_key(command(&format!("{ARROW_LEFT}x"))).is_none());
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
