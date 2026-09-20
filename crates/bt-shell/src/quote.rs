//! Sürüklenen dosya yolları → kabuğa yazılabilir tek satır. **Saf ve
//! AppKit'siz**, bu yüzden sınanabilir.
//!
//! `keys.rs`'in içinde değil **kardeşi**: o modülün başlığı "tuş vuruşu →
//! PTY baytları" diyor ve buradaki soru bir tuş değil bir damla. Tek
//! tüketicisi `view::BateriView`'ın `performDragOperation:`'ı; çıkışı oradan
//! `Session::paste`'e gidiyor, yani bracketed paste sarması ve dock istisnası
//! bu modülün konusu değil.

/// Yolları ters bölüyle kaçırır ve **tek boşlukla** birleştirir — Terminal.app
/// paritesi (`/Users/…/İki\ Kelime/a.txt`).
///
/// **Kaçacak küme bir kara liste değil, bir beyaz listenin tümleyeni:** geçen
/// şey ASCII harf/rakam ile `/ . _ -`, ve **ASCII olmayan her karakter**;
/// kalan her ASCII kaçar. Ters karar (kabuğun metakarakterlerini tek tek
/// saymak) `~`, `=`, `#`, `!`, `%`'in hangi kabukta kelimenin neresinde özel
/// olduğunu tartışmaya açardı ve listeden düşen tek karakter sessiz bir
/// hataya dönerdi. Fazladan kaçırmanın bedeli yok (`\+` kabukta `+`), yani
/// yanlışın yönü güvenli.
///
/// ASCII olmayan karakter **dokunulmadan** geçiyor: `İki Kelime`'nin boşluğu
/// kaçar, `İ` kaçmaz — kabuk onu zaten düz harf sayıyor ve önüne ters bölü
/// koymak yalnız çirkin olurdu.
///
/// **Satır sonunun bilinen sınırı adıyla duruyor:** `\` + satır sonu zsh'te de
/// bash'te de *satır devamıdır*, yani adında satır sonu taşıyan bir dosya
/// (patolojik ama mümkün) iki parçası birleşmiş hâlde yazılır. Kaçmamak daha
/// kötü olurdu — ham satır sonu tamponda bir komut sınırı olur ve kullanıcının
/// yazmadığı bir satır koşardı. `$'\n'` doğru olurdu ama tek kuralı ikiye
/// bölerdi (018 Karar 4: tek tip, tek kural).
///
/// Boş liste boş dizge verir: damlada okunabilen yol yoksa yazılacak bir şey
/// de yok.
pub(crate) fn shell_quote(paths: &[String]) -> String {
    let mut line = String::new();
    for path in paths {
        if !line.is_empty() {
            line.push(' ');
        }
        for c in path.chars() {
            if needs_escape(c) {
                line.push('\\');
            }
            line.push(c);
        }
    }
    line
}

/// Beyaz listenin kendisi: ASCII harf/rakam ve dört noktalama geçer, ASCII
/// olmayan her şey geçer, kalan ASCII kaçar.
///
/// Dördün gerekçesi yolun kendi sözlüğü: `/` ayraç, `.` uzantı ve `..`, `_`
/// ile `-` dosya adlarının olağan noktalaması. Beşincisi eklenirken sorulacak
/// soru "kabukta özel mi" değil, "kaçarsa bozulur mu" — kaçmak zararsız, yani
/// liste kısa kalmalı.
fn needs_escape(c: char) -> bool {
    c.is_ascii() && !(c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(path: &str) -> String {
        shell_quote(&[path.to_owned()])
    }

    #[test]
    fn plain_paths_pass_through_untouched() {
        // Kaçacak karakteri olmayan yol olduğu gibi gidiyor: beyaz liste
        // harf/rakam ve yolun kendi noktalaması.
        assert_eq!(quote("/Users/kalaomer/a.txt"), "/Users/kalaomer/a.txt");
        assert_eq!(quote("/tmp/bir_iki-uc.tar.gz"), "/tmp/bir_iki-uc.tar.gz");
    }

    #[test]
    fn spaces_are_escaped_so_the_shell_sees_one_argument() {
        // Setin manşet hâli (018 Karar 4, Terminal.app paritesi): adında
        // boşluk olan dosya tek argüman olmalı, yoksa kabuk iki yol görür.
        assert_eq!(
            quote("/Users/a/İki Kelime/a.txt"),
            "/Users/a/İki\\ Kelime/a.txt"
        );
    }

    #[test]
    fn non_ascii_characters_are_not_escaped() {
        // Türkçe harfler kabukta düz harf: önlerine ters bölü koymak yalnız
        // çirkin olurdu. Kaçan tek şey aradaki boşluk (yukarıda).
        assert_eq!(quote("/tmp/ğüşİÖÇ.txt"), "/tmp/ğüşİÖÇ.txt");
        assert_eq!(quote("/tmp/日本語"), "/tmp/日本語");
    }

    #[test]
    fn shell_metacharacters_are_escaped() {
        // Kara liste tartışmasının kapandığı yer: kabuğun metakarakterleri
        // tek tek sayılmıyor, beyaz listenin dışında kaldıkları için kaçıyorlar.
        assert_eq!(quote("/tmp/$HOME"), "/tmp/\\$HOME");
        assert_eq!(quote("/tmp/`x`"), "/tmp/\\`x\\`");
        assert_eq!(quote("/tmp/a;b"), "/tmp/a\\;b");
        assert_eq!(quote("/tmp/a&b"), "/tmp/a\\&b");
        assert_eq!(quote("/tmp/a|b"), "/tmp/a\\|b");
        assert_eq!(quote("/tmp/a'b"), "/tmp/a\\'b");
        assert_eq!(quote("/tmp/a\"b"), "/tmp/a\\\"b");
        assert_eq!(quote("/tmp/a\\b"), "/tmp/a\\\\b");
        assert_eq!(quote("/tmp/a*b?c[d]"), "/tmp/a\\*b\\?c\\[d\\]");
        // Beyaz liste olmasaydı tartışmaya açılacak olanlar: `~` yalnız
        // kelimenin başında, `=` yalnız zsh'te, `#` yalnız kelime başında,
        // `!` yalnız etkileşimli kabukta özel. Hepsi kaçıyor ve kaçmaları
        // zararsız.
        assert_eq!(quote("/tmp/~=#!%"), "/tmp/\\~\\=\\#\\!\\%");
    }

    #[test]
    fn tab_and_newline_are_escaped_with_a_backslash() {
        // Sekme kelime ayracı, satır sonu komut sınırı: ikisi de kaçmak
        // zorunda. Satır sonunun **bilinen sınırı** sözleşme olarak çivili —
        // `\` + satır sonu kabukta satır devamı, yani adın iki parçası
        // birleşir. Kaçmamanın bedeli daha ağır (kullanıcının yazmadığı bir
        // satır koşardı) ve `$'\n'` tek kuralı ikiye bölerdi.
        assert_eq!(quote("/tmp/a\tb"), "/tmp/a\\\tb");
        assert_eq!(quote("/tmp/a\nb"), "/tmp/a\\\nb");
    }

    #[test]
    fn several_paths_are_joined_by_a_single_space() {
        // Çok dosyalı damla: her yol ayrı kaçıyor, aralarında **tek** boşluk
        // ve sonda boşluk yok — kullanıcının yazdığı satıra fazladan bir
        // karakter eklemiyoruz.
        assert_eq!(
            shell_quote(&["/tmp/a b".to_owned(), "/tmp/c".to_owned()]),
            "/tmp/a\\ b /tmp/c"
        );
        // Boş damla boş dizge: okunabilen yol yoksa yazılacak şey de yok.
        assert_eq!(shell_quote(&[]), "");
    }
}
