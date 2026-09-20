# Klavye ve dosya sürükleme

## Hedef

Türkçe Q düzeninde `~` ve `` ` `` yazılabilsin, Option+ok ile Option+Delete
kelime gezsin/silsin, Cmd+Delete satırı silsin ve Finder'dan bırakılan
dosyanın yolu giriş satırına düşsün — **bugün çalışan hiçbir tuş bozulmadan**.

## Gereksinimler

- **R1 — Metin girişi AppKit'in yığınından geçer.**
  - **R1.1** — `BateriView` `NSTextInputClient`'ın **11 zorunlu** metodunu
    uygular. Kısmi uyum yok: `define_class!` debug assertion'larıyla panikler.
    `bt-shell/Cargo.toml`'a **iki** feature girer, her biri kendi gerekçe
    yorumuyla: `NSTextInputClient` ve (R5 için) `NSDragging`.
    `NSUserDefaults` **feature istemiyor** — `objc2-foundation`'ın varsayılan
    setinde; aynı yorum bloğu bunu da söyler (dosyanın "kırpma günü"
    konvansiyonu). `Cargo.lock` değişmiyor.
  - **R1.2** — `keyDown:` **dört kollu** arbitraj olur: (a) Cmd'li olay
    (R4), (b) Shift+PgUp/PgDn (bugünkü `page_scroll`), (c) **Control'lü
    olay** — yığına **girmez**, bugünkü `encode_key` kolunda kalır, (d)
    kalanı `interpretKeyEvents:`e. `consumed` bayrağı set edilmediyse
    `encode_key`'e düşer; bayrağı `insertText:` **ve** `setMarkedText:` set
    eder (değişmez: "yığın bu olayı aldı", "metin geldi" değil).
    **Kaydırma ile Control'ün sırası phase-1'de takas edildi** (2026-09-20):
    özgün sıra (b) Control / (c) kaydırma, **Ctrl+Shift+PgUp**'ı Control
    koluna düşürüp bugünkü kaydırmayı `\e[5~`'e çevirirdi ve R1.7 sıfır
    regresyon istiyor. Maddenin savunduğu şey sıra değil **yığından önce
    gelmek**: Cmd de Control de `interpretKeyEvents:`i hâlâ hiç görmüyor.
  - **R1.3** — `insertText:` ve `setMarkedText:`'in argümanı `&AnyObject`
    (`NSString` **ya da** `NSAttributedString`). **Tek** çözme kuralı:
    `NSString`'e downcast, olmazsa `NSAttributedString::string()`, ikisi de
    değilse olay **tüketilmiş sayılmaz** ve `encode_key`'e düşer.
  - **R1.4** — `doCommandBySelector:` sessiz no-op; bip yok, olay fallback'e
    düşer.
  - **R1.5** — `setMarkedText:`/`unmarkText` asgari bir bileşim durumu tutar;
    `hasMarkedText`/`markedRange`/`selectedRange` ona cevap verir. Çizim yok.
  - **R1.6** — `ApplePressAndHoldEnabled = false` uygulamanın kendi
    `registerDefaults`'ına yazılır; kullanıcının plist'ine dokunulmaz.
    **Ölçüm bekliyor:** bellek içi registration domain'in popover'ı gerçekten
    bastırdığı ölçülmedi (iTerm2'nin kanıtı *kalıcı* domain değeri).
  - **R1.7** — **Bugünkü tuş kümesinde sıfır regresyon.** Liste ölçütü
    "doğruluğu artık AppKit'in hangi kolu seçtiğine bağlı olan tuşlar":
    Enter, Tab, Esc, Backspace, oklar, Shift+Tab, fn+Backspace, Shift+PgUp,
    **numpad Enter / Fn-Return** (`characters` = U+0003, Ctrl'süz — yığın onu
    eklerse PTY'ye `0x03` gider ve her komut kesilir), **Ctrl+Shift+harf**
    (`keys.rs`: "Ctrl-Shift-C'de harf harf kalır"), **Ctrl+Y** (U+0019'u
    Shift+Tab ile paylaşıyor), **düz çok baytlı harf** (`ğ`, `İ` — UTF-8
    garantisi artık `insertText:`'in sınırında), ve `^A/^C/^D/^E/^K/^U/^W`.
    Dördü de (b) kolu sayesinde `encode_key`'de kalıyor; liste yine **elle**
    basılır, çünkü hiçbir sınama "hangi olay `encode_key`'e ulaştı"yı
    çivilemiyor.
- **R2 — Ölü tuş bileşimi tamamlanır.** `Option+ü` + `Boşluk` → `~`;
  `Option+,` + `Boşluk` → `` ` `` (bugün **hiçbir** yolla yazılamıyor);
  `Option+ü` + `n` → `ñ`. Setin manşet kazancı ve hermetik olarak kapanamaz.
- **R3 — Option'ın iki sınıfı.**
  - **R3.1** — Option+← → `\eb`, Option+→ → `\ef`, Option+Delete → `\e\x7f`.
    Ayar yok. Diziler **küçük harf**: `\eA`/`\eB` zsh'te başka widget'lar
    (`accept-and-hold`, `backward-word`), ölçüldü.
  - **R3.2** — Option+basılabilir harf **değişmez**: `Option+7` `{` yazar,
    `Option+b` `∫` verir.
- **R4 — Cmd'nin kapalı izin listesi tek tuş.**
  - **R4.1** — Cmd+Delete → `\x15` (zsh: `kill-whole-line`); Command'lı başka
    her tuş yine yutulur.
  - **R4.2** — Cmd'li olay `interpretKeyEvents:`e **hiç girmez** — girerse
    ⌘⌫ orada `deleteToBeginningOfLine:` olur, ⌘T de `insertText:`'e varıp
    kabuğa `t` yazar.
  - **R4.3** — `reaches_terminal` tuşun **kimliğini** öğrenir (imzası
    genişler) ve `command_keys_never_reach_the_terminal` sınaması tek
    istisnayla yeniden yazılır.
- **R5 — Finder'dan dosya sürükleme.**
  - **R5.1** — `NSPasteboardTypeFileURL` kaydedilir; `draggingEntered:` →
    `.copy`, `performDragOperation:` yolları yazar. Okuma API'si **seçili**:
    `readObjectsForClasses:options:` + `NSURL::class()` — ek feature
    istemiyor (`pasteboardItems()` `NSPasteboardItem` feature'ı isterdi). Yol
    `NSURL.path`'ten alınır, yüzde çözme **ikinci kez yazılmaz**.
  - **R5.2** — Saf `shell_quote` ters bölüyle kaçar (kabuğun metakarakterleri
    + boşluk/sekme/satır sonu), çok dosya boşlukla ayrılır; `bt-shell`'de
    kendi testleriyle durur (`keys.rs` emsali).
  - **R5.3** — Çıkış `Session::paste`; bracketed paste ve dock istisnası
    bedavaya gelir.
- **R6 — Belge kodla aynı commit'te düzelir.** `keys.rs`'in kapsam-dışı
  listesi **bölünüyor, eksilmiyor** (Option+Backspace kapsam içi; Ctrl+Backspace
  ve Option/Ctrl'lü ileri silme dışarıda; "Option-as-Meta" ibaresi yeniden
  yazılır — gezinme tuşları artık Meta, harf değil; "değiştiricili oklar
  `\e[1;5A`" hâlâ dışarıda); `view.rs`'in modül başlığı; `CLAUDE.md`'nin
  `bt-shell` satırı + giriş özeti; `docs/YOL-HARITASI.md`'nin "Klavye
  kalanları" maddesi tek satıra iner. `docs/AYARLAR.md` **değişmiyor** (yeni
  anahtar yok).

## Yaklaşım

1. **Yönlendirme değişir** (R1, R2 — faz 1). `keyDown:` dört kollu arbitraja
   dönüşür; 11 zorunlu metot yazılır. Gövdeli olanlar `insertText:` ve
   `doCommandBySelector:`; dördü asgari bileşim durumundan; kalanı sabit cevap,
   her biri kendi "neden" yorumuyla.
2. **Değiştirici kodlaması** (R3, R4 — faz 2). **İki dokunuş:** `keys.rs`'in
   Option satırları ve `view.rs`'in Cmd kolu. Girdi kaydının imzası
   (`encode_key`'e hangi değiştirici bayrakları girecek) **bu fazda**
   tanımlanır, yani kaydı tanımlayan faz onu tüketen faz olur.
3. **Sürükleme** (R5 — faz 3). İki `NSDraggingDestination` metodu
   (protokolün **bütün** metotları `#[optional]`), saf `shell_quote`, çıkış
   `Session::paste`.

## Kapsam Dışı

- **Home/End.** Hedef cümlesinde yok, kullanıcı istemedi ve ölçüm zsh'te
  `^[[H`/`^[[F`/`^[OH`/`^[OF` için **sıfır** bağlama gösterdi; tüketicisi
  less/vim ve terminfo `khome`/`kend`, yani tam ekran uygulama borcu. Bedeli
  setin **tek** `bt-core` `pub` sınır genişlemesi olurdu. Çıkarmak güvenli:
  Home `doCommandBySelector:`'a düşer, no-op'tan geçer, `encode_key` bugünkü
  gibi yutar — davranış bit bit aynı. Şekli `docs/YOL-HARITASI.md`'nin borç
  satırında bağlandı.
- **Ctrl+Shift+Tab ve Ctrl+numpad Enter.** İkisi de `keys.rs`'in doc'unda
  duran borç ve bu sette kapanmıyor. R1.2'nin Control kolu ikisini de bugünkü
  davranışta **tutuyor** — Ctrl'lü olay yığına hiç girmediği için U+0003 ve
  U+0019 paylaşımları AppKit'in koluna bırakılmıyor.
- **Tam IME** — CJK ve altı çizili preedit'in **çizilmesi**. `setMarkedText:`
  durum tutar, `bt-gpu`/`Frame` yüzeyi bu sette doğmaz.
- **`[keyboard]` bölümü, `left_option`/`right_option`, Option'ın topluca Meta
  olması** — `discussion.md` → Karar.
- **Cmd+←/→**, **metin/URL damlası**, **damlama sırasında görsel geri
  bildirim**, **Cmd'li damlanın `cd` olması**, **ikona damlatma**.
- **kitty klavye protokolü / CSI u**, **odak raporu (DEC 1004)**, **Secure
  Keyboard Entry**, **değiştiricili ok dizileri (`\e[1;5A`)**.
- **Emoji/karakter paleti bir kazanç sayılmaz**: `insertText:` girişini açar,
  glyph 020'ye kadar çizilmez.
- **`firstRectForCharacterRange:`in hassaslaştırılması** — kapsam içinde
  tüketicisi yok (ölü tuş önizlemesi marked text, aday penceresi CJK'nın);
  belgelenmiş yaklaşımla cevap verir.

## Göç

Göç yok: yeni ayar anahtarı yok, `TERM`/terminfo sabit, shell entegrasyon
dosyası yerinde, kullanıcının `settings.toml`'una ve plist'ine dokunulmuyor
(`ApplePressAndHoldEnabled` yalnız bellek içi `registerDefaults`'ta).

**Geri alma birimi faz 2'den sonra commit değil `set`:** girdi kaydının imzası
faz 2'de değişiyor ve faz 1'in arbitrajının üstüne oturuyor, yani faz 1'i tek
başına geri almak derlemeyi kırar. Aynı cümle faz 2'nin `## Yayın Etkisi`'nde
durur, `/implement` onu `teslim.md`'ye derler.

## Doğrulama

| ne | ne zaman | not |
|---|---|---|
| `make hepsi` | üç fazda + kapı düzeltmesinde | — |
| `make duman` | üç fazın **üçünde**, **kullanıcı koşar** | Klavyeye **kör** (tuş sentezi yok) ama **sınıf kaydına kör değil**: hiçbir sınama `BateriView` üretmiyor, yani R1.1'in protokol assertion'ı ilk kez gerçek pencerede patlar — faz 1'in tek uyum kapısı bu |
| elle tuş turu | her fazda, **en az üç oturum** | Faz 1: R1.7 listesi + R2 kazanç listesi + press-and-hold + "bir olay iki callback üretebilir mi" (bekleyen ölü tuş ardından Enter). Faz 2: Option+←/→/⌫. Faz 3: Finder damlası, çok dosya, boşluklu ad |
| `/code-review` | **faz 1 sonunda** ve set sonunda | Mekanik tetikleyici yok (`Cargo.lock` değişmiyor, `.metal` yok, paylaşılan durum yok) ama faz 1 kapının yazılı gerekçesinin tanımı: "hata sessizdir ve sonraki phase'ler onun üstüne kurulur". Faz 1 riskli phase kutusunu **tutar** |
| `/audit` | set sonu | Mercekler: bağımlılık kararının kaydı (feature yorumları), dil/belge kuralı, katman yönü |

Tetiklenmeyenler: `make kur`, `make shader`, `make terminfo`, `make test-yaris`
(`consumed: Cell<bool>` ana thread ivar'ı, paylaşılan durum yok).

## Akış

```
NSEvent (AppKit)
   ▼
keyDown:  — dört kol
   ├─ Cmd'li?     ──► izin listesi (⌘⌫ → \x15) ya da YUTULUR      [R4]
   │                   (metin yığınına HİÇ girmez)                 [R4.2]
   ├─ Shift+PgUp/PgDn? ──► Session::scroll_page                    (bugünkü kol)
   │                   (scroll_page None dönerse → encode_key, bugünkü yol;
   │                    Control'ün ÖNÜNDE, yoksa Ctrl+Shift+PgUp kaydırmayı
   │                    kaybeder — phase-1'de takas edildi)
   ├─ Control'lü? ──► encode_key                                   [R1.2c]
   │                   (U+0003 ve U+0019 paylaşımları AppKit'e
   │                    bırakılmaz; Ctrl'lü borçlar bugünkü hâlde)
   └─ kalanı:
        consumed.set(false)
        interpretKeyEvents(&[event])
          ├─ insertText:      → &AnyObject çözülür → write;  consumed [R1.3]
          ├─ setMarkedText:   → bileşim durumu;              consumed [R1.5]
          └─ doCommandBySelector: → sessiz no-op (bip yok)            [R1.4]
        consumed değilse ──► encode_key(girdi kaydı)
                               ├─ Option + ok/Delete → \eb \ef \e\x7f [R3.1]
                               ├─ Ctrl'lü harf, PgUp/PgDn, Shift+Tab, ⌦ (bugünkü)
                               └─ tanınmayan fonksiyon tuşu → yutulur

performDragOperation: ──► readObjectsForClasses:(NSURL) → NSURL.path
                          → shell_quote → Session::paste             [R5]
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| kapı | |
