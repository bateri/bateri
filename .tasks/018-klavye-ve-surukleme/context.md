# Klavye ve dosya sürükleme — Bağlam

## Mevcut Durum

Klavyenin tamamı **tek dosyada** ve iki fonksiyonda: `bt-shell/view.rs`'in
`keyDown:` kancası ile `bt-shell/keys.rs`'in saf `encode_key`'i.

`keyDown:` (`view.rs:412`) üç şey yapıyor:

1. `NSEvent.characters`'ı okuyor — **başka hiçbir kaynağa bakmıyor**.
   `charactersIgnoringModifiers` ve `keyCode` hiç okunmuyor.
2. Command'lı tuşu yutuyor (`reaches_terminal`, `view.rs:160`). Menü onu
   `performKeyEquivalent:` ile önce yakalıyor; yakalamadığı buraya varıp
   **kayboluyor**, çünkü terminale düşseydi kabuğa düz harf yazardı.
3. Shift+PgUp/PgDn'i kaydırmaya çeviriyor (`page_scroll`), kalanı
   `encode_key`'e veriyor.

`encode_key` (`keys.rs:41`) `characters`'ın ilk karakterine bakıp bayt
üretiyor; tanımadığı fonksiyon tuşunu (U+F700–U+F8FF) bilerek yutuyor ve
doc'unda kapsam dışını **adıyla** sayıyor: "IME, ölü tuşlar, Option-as-Meta,
kitty klavye protokolü, değiştiricili oklar (`\e[1;5A`), Control'lü/Option'lı
geri sekme ve ileri silme, Home/End".

Çıkışlar `Session::write(&[u8])` ve `Session::write_arrow(Arrow)` — okun baytı
DECCKM'e bağlı olduğu için kip kararı `bt-core`'da.

**Sürükleme yok.** View hiçbir sürükleme tipi kaydetmiyor: bütün depoda
`registerForDraggedTypes`, `draggingEntered:` ve `performDragOperation:`
**hiç geçmiyor**. `NSView`'un varsayılanı sürükleme hedefi olmamak, yani
Finder'dan bırakılan dosya sessizce düşüyor — hata mesajı da yok.

Yapıştırma yolu ise **hazır**: `Session::paste(Vec<u8>)` bracketed paste
sarmasını (`\e[200~` … `\e[201~`) ve dock istisnasını
(`Session::can_be_typed`: dock satırın sahibiyken, ZLE ekleme keymap'indeyken,
tek satırlık ve kontrol karakteri taşımayan yük sarmadan akıtılıyor) tek yerde
tutuyor.

## Motivasyon

Dört kalem, hepsi **günlük kullanımda hissedilen** eksik. Tek set olmalarının
sebebi kapsam değil **dosya**: dördü de `view.rs` + `keys.rs`'te buluşuyor ve
üçü `keyDown:`'ın aynı yönlendirmesine dokunuyor.

1. **macOS metin kısayolları yok** (kullanıcı isteği 2026-09-19: "bu kısayollar
   yok diye pratiklik çok azalıyor"). Option+oklar kelime atlamıyor,
   Option+Delete kelime silmiyor, Cmd+Delete satırı silmiyor. Option'ın
   terminal karşılığı **var** (Meta: `\eb`, `\ef`, `\e\x7f` — zsh onları zaten
   biliyor, biz göndermiyoruz), Cmd'nin **yok**: hiçbir kaçış dizisi Cmd'yi
   kodlamıyor ve `reaches_terminal` Command'lı her tuşu yapısal olarak kesiyor.
   Home/End aynı tesisattan geliyor; değiştiricili ok dizileri (`\e[1;5A`)
   ise ayrı bir kalem — ölçüm onların varsayılan zsh'te karşılıksız olduğunu
   gösterdi (`discussion.md` → Karar 2).
2. **Ölü tuşlar çalışmıyor** (kullanıcı isteği 2026-09-20). Türkçe Q (PC
   uyumlu) düzeninde `~` ve `` ` `` yazılamıyor; ölçümü aşağıda.
3. **Option'ın iki sınıfı ayrışmamış.** (2) ile (1) ilk bakışta aynı tuşu
   istiyor gibi duruyor — Option ya Meta olur ya karakter üretir. Ayrışıyorlar:
   Option+ok hiçbir düzende basılabilir karakter üretmiyor, Option+harf
   üretiyor. Referansın çaresi (sol/sağ Option, üç kip —
   `docs/ARASTIRMA.md`:178 ve :127) yalnız ikinci sınıf için gerekli ve bu
   sette **kapsam dışı** (`discussion.md` → Muhakeme).
4. **Finder'dan dosya sürükleme yok** (kullanıcı isteği 2026-09-20).
   Referansta var (`docs/ARASTIRMA.md`:176). Dosyanın yolunu elle yazmak ya da
   Finder'dan kopyalayıp yapıştırmak gerekiyor.

## Kanıt

### (a) Düzen taraması — Türkçe Q (PC uyumlu), 2026-09-20

`UCKeyTranslate` ile `com.apple.keylayout.Turkish-QWERTY-PC`'nin kendi düzen
verisinden okundu (ekranda tuşa basmadan; kaynak bir kerelik Swift betiği,
depoya girmedi). Kullanıcının seçili düzeni bu.

**Ölü tuşlar ve bileşimleri** (ilk vuruş hiçbir karakter vermiyor, `durum`
`UCKeyTranslate`'in ölü tuş durumu):

```
Option+ü  (kod 30): ilk vuruş=""  durum=2 →  boşluk:"~"  a:"ã"  n:"ñ"  o:"õ"
Option+ğ  (kod 33): ilk vuruş=""  durum=3 →  boşluk:"¨"  a:"ä"  u:"ü"  o:"ö"
Option+ş  (kod 41): ilk vuruş=""  durum=4 →  boşluk:"´"  a:"á"  u:"ú"  o:"ó"
Option+,  (kod 42): ilk vuruş=""  durum=5 →  boşluk:"`"  a:"à"  u:"ù"  o:"ò"
Shift+3   (kod 20): ilk vuruş=""  durum=1 →  boşluk:"^"  a:"â"  u:"û"  o:"ô"
```

**Kabuk metakarakterlerine ölü tuşsuz ulaşan yollar** (keycode 0–127 × {düz,
Shift, Option, Shift+Option} tarandı):

```
"~": Option+kod45 (N)
"`": *** YOK — yalnız ölü tuşla ***
"^": Option+kod4 (H)
"|": Option+kod24, Option+kod50, Option+kod94
"\": Option+kod27        "{": Option+kod26        "}": Option+kod29
"[": Option+kod28        "]": Option+kod25        "$": Option+kod21
"@": Option+kod12        "#": Option+kod20
"'": Shift+kod19         "\"": düz+kod10
```

İki sonuç, ikisi de tasarımı bağlıyor:

- `` ` `` bu düzende **hiçbir düz kombinasyonla yazılamıyor**. Tek yolu ölü
  tuş, yani bugün bateri'de backtick **büsbütün erişilemez**. `~`'in bir
  kaçış kapısı var (Option+N), backtick'in yok.
- Kabuğun **bütün** metakarakterleri Option kombinasyonu. Option'ı topluca
  Meta yapmak bu düzende kabuğu yazılamaz hâle getirir.

### (b) Kullanıcı ölçümü — gerçek pencere, 2026-09-20

| tuş | sonuç | ne söylüyor |
|---|---|---|
| Option+N | `~` çıkıyor | düz metin dalı sağlam; `encode_key`'in varsayılan kolu çok baytlı karakteri doğru geçiriyor |
| Option+`,` sonra Boşluk | **boşluk** çıkıyor | AppKit ölü tuş durumunu bir sonraki olayın `characters`'ına **taşımıyor** |

İkinci satır setin çekirdeği: eksik olan yalnız görsel geri bildirim **değil**,
bileşimin kendisi kırık. `interpretKeyEvents:` olmadan ölü tuş bir hiçe
dönüşüyor — ilk vuruş boş `characters` veriyor
(`keys_without_sequences_are_swallowed` sınaması bunu zaten çiviliyor), ikinci
vuruş da bileşmemiş hâlini veriyor.

Kullanıcı ayrıca hangi tuşu denediğini söyledi: `ü`. Yani PC alışkanlığı
(Windows Türkçe Q'da AltGr+`ü` **anında** `~` verir) doğrudan ölü tuşa
çarpıyor ve ekranda hiçbir iz bırakmıyor.

### (c) Kod durumu — grep'lendi, 2026-09-20

- `NSTextInputClient`, `interpretKeyEvents:`, `insertText:`, `setMarkedText:`,
  `inputContext` → **depoda hiç yok**.
- `registerForDraggedTypes`, `draggingEntered:`, `performDragOperation:`,
  `NSPasteboardTypeFileURL` → **depoda hiç yok**.
- `keys.rs:39` doc'u ölü tuşları ve IME'yi adıyla kapsam dışı ilan ediyor, yani
  bu bir kaçak değil **kayıtlı borç**.

## Mevcut Mimari

```
NSEvent (AppKit)
   │  characters  ← tek kaynak; keyCode ve charactersIgnoringModifiers okunmuyor
   ▼
view.rs keyDown:
   ├─ reaches_terminal(flags)  → Command'lı tuş YUTULUR
   ├─ page_scroll(chars, shift) → Session::scroll_page   (terminalin kendi tuşu)
   └─ keys.rs encode_key(chars, ctrl)
        ├─ KeyInput::Bytes  → Session::write(&bytes)
        ├─ KeyInput::Arrow  → Session::write_arrow(arrow)   (bayt DECCKM'e bağlı)
        └─ None             → yutulur (fonksiyon tuşu, boş characters)

Finder'dan sürükleme
   └─ (hiçbir kanca yok — damla sessizce düşer)

Pano yapıştırma  (hazır tesisat, sürüklemenin de çıkışı olacak)
   └─ Session::paste(bytes)
        ├─ can_be_typed(bytes) ? → ham akıt (dock satırın sahibi, tek satır)
        └─ değilse              → \e[200~ … \e[201~ (bracketed paste)
```

Değişecek sınır **`keyDown:`'ın tekliği**: bugün hem metni hem kontrol
dizisini o üretiyor. `insertText:` gelince metin oradan girecek, yani
`keyDown:` "fonksiyon tuşu + kontrol dizisi"ne inecek — yoksa basılabilir
harf iki yoldan birden gider.
