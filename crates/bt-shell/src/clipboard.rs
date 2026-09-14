//! Pano köprüsü: `bt-core`'un bildiği metni AppKit panosuna taşır.
//!
//! Panoya dokunan yalnız `bt-shell` (AppKit); `bt-core` bayt görür, pano
//! görmez (`CLAUDE.md` → katman düzeni). Kopyanın metni phase-1'in tek metin
//! yolundan gelir (`Session::selection_text`); ikinci bir metin yolu, ikinci
//! bir sarma hatası demek olurdu.
//!
//! İki yön de `NSPasteboard` üstünden; pano her çağrıya **parametre** olarak
//! gelir. Üretimde ikisi de genel panodur (`copy` kullanıcının Cmd-C'si,
//! `read` Cmd-V'si), sınamada ise yön başına ayrı bir benzersiz pano verilir:
//! başsız ortamda genel panoya dokunulamaz ve iki sınama birbirinin içeriğini
//! görmemeli. Panoyu parametre yapmak, canlılık çapasını da sınanabilir
//! kılıyor (`board()`).
//!
//! `copy`/`read` adları AppKit'in de değil `NSPasteboard`'un da değil, bu
//! köprünün kendi sözlüğüdür: `copy` yazma yönü, `read` okuma yönüdür.

use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;

/// Seçili metni panoya yazar. Yazılacak metin yoksa pano **el değmeden**
/// kalır ve `false` döner.
///
/// Boş yazma genel panoyu temizler ve kullanıcının başka uygulamadan
/// kopyaladığını silerdi. Kapı bu yüzden iki hâli birden eler:
///
/// - `None` → seçim yok.
/// - `Some("")` → seçim **var** ama boş metin veriyor: boşluklardan oluşan
///   bir satırın üstünde sürükleme. O satırda alacritty'nin `line_length()`'i
///   sıfır, yani `selection_to_string()` `Some("")` döner. Bu hâl kapıdan
///   geçseydi prompt altındaki boş bir satırı seçip Cmd-C demek kullanıcının
///   panosunu boşaltırdı — `None` kapısının engellediği kaybın ta kendisi.
///   (Sürüklemesiz tık buraya düşmez: iki ucu eşit seçim boştur ve `None`
///   verir.)
///
/// **Yalnız boşluk** (`Some("   ")`) elenmez: satırın içindeki boşlukları
/// seçip kopyalamak meşru ve o metin boş değil.
pub(crate) fn copy(board: &NSPasteboard, text: Option<String>) -> bool {
    let Some(text) = text.filter(|t| !t.is_empty()) else {
        return false;
    };
    // `clearContents` + `setString`: `setString` tek başına da yazar ama o
    // zaman eski tipler (RTF, TIFF) panoda kalır ve yapıştıran taraf metin
    // yerine onları alabilir.
    board.clearContents();
    let string = NSString::from_str(&text);
    // SAFETY: `unsafe` blok `NSPasteboardTypeString` **statik** erişimi için;
    // `setString:forType:`'ın kendisi `unsafe` değil (`objc2` onu güvenli
    // sarmalıyor). Statik gerçek bir `NSPasteboardType` kaydı ve `None`'a
    // çözümlenmiyor.
    //
    // Bağlam `MainThreadMarker` değil: `NSPasteboard` `objc2`'de `AnyThread`,
    // yani tip ana thread'i zorlamıyor. Üretimde çağıran `keyDown:`'dır (ana
    // thread) ve sınama onu işçi thread'lerden çağırıyor — ölçülen davranış
    // ikisinde de aynı.
    unsafe { board.setString_forType(&string, NSPasteboardTypeString) }
}

/// Panodaki metni okur. Metin yoksa (`None`) yapıştırma sessizdir.
pub(crate) fn read(board: &NSPasteboard) -> Option<String> {
    // SAFETY: yukarıdakiyle aynı statik erişimi.
    unsafe { board.stringForType(NSPasteboardTypeString) }.map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canlı bir pano; başsız ortamda (CI, `cargo test` ssh üstünde) `None`.
    ///
    /// Benzersiz pano başsızda da **doğar**, ama yazma tutmaz. Çapa bu yüzden
    /// varlığa değil **yazıp okuyabilmeye** bağlı: tutan bir yazma gerçek bir
    /// pano sunucusu demek. Bunu `copy`/`read` üzerinden kurmak, canlılık
    /// ölçüsünü de köprünün kendi gövdesinden almak demek — ayrı bir yazma
    /// yolu kurup iğneyi iki yerde tutmaktan iyidir.
    fn board() -> Option<objc2::rc::Retained<NSPasteboard>> {
        // `pasteboardWithUniqueName` her çağrıda taze bir pano verir —
        // sınamalar birbirinin içeriğini görmez.
        let board = NSPasteboard::pasteboardWithUniqueName();
        const SENTINEL: &str = "bateri pano çapası";
        let alive =
            copy(&board, Some(SENTINEL.to_owned())) && read(&board).as_deref() == Some(SENTINEL);
        alive.then_some(board)
    }

    /// Başsız dala ortak gerekçe. Her iki sınama da aynı yolu tutar: pano
    /// yoksa **sessizce atlanır**. Boş bir panoya iddia kurmak kapının sahte
    /// yeşil verdiği yol olurdu — taze pano zaten boştur, `read` köprü
    /// çalışmasa da `None` döner.
    const HEADLESS: &str = "başsız ortamda pano yok, sınama atlandı";

    #[test]
    fn copy_writes_selection_text_to_clipboard() {
        let Some(board) = board() else {
            eprintln!("{HEADLESS}");
            return;
        };
        assert!(copy(&board, Some("hello".to_owned())));
        assert_eq!(read(&board).as_deref(), Some("hello"));
    }

    #[test]
    fn copy_without_selection_leaves_board_untouched() {
        let Some(board) = board() else {
            eprintln!("{HEADLESS}");
            return;
        };
        assert!(copy(&board, Some("önce".to_owned())));
        // Seçimsiz kopya `false` döner ve panodaki duranı silmez.
        assert!(!copy(&board, None));
        assert_eq!(read(&board).as_deref(), Some("önce"));
    }

    #[test]
    fn copy_of_empty_text_leaves_board_untouched() {
        // `Some("")` = seçim var ama metin boş: boş bir satırın üstünde
        // sürükleme. Kapı bunu da eler, yoksa Cmd-C kullanıcının panosunu
        // boşaltırdı. Pano gerektirmeyen kısım her ortamda ölçülür
        // (`Some("")` hiç yazmaz), gerisi canlı pano ister.
        let fresh = NSPasteboard::pasteboardWithUniqueName();
        assert!(!copy(&fresh, Some(String::new())));

        let Some(board) = board() else {
            eprintln!("{HEADLESS}");
            return;
        };
        assert!(copy(&board, Some("önce".to_owned())));
        assert!(!copy(&board, Some(String::new())));
        assert_eq!(read(&board).as_deref(), Some("önce"));
    }
}
