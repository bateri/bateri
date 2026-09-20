# Phase 1 — Metin girişi AppKit'in yığınından geçer

## Özet

`keyDown:` tek giriş kapısı olmaktan çıkıp dört kollu bir arbitraja dönüşür;
`BateriView` `NSTextInputClient` olur ve ölü tuş bileşimi tamamlanır.

_Requirements: R1, R2, R6 (bu fazın dokunduğu doc'lar)_

## Değişiklikler

- **`crates/bt-shell/src/view.rs`** — `keyDown:` dört kola ayrılır ve
  **kolların sırası sözleşmedir**: (a) Cmd'li olay `interpretKeyEvents:`'ten
  **önce** yutulur — bu fazda davranış değişmiyor, ama kol burada kuruluyor ve
  faz 2 onu açıyor; (b) Control'lü olay, yine yığından **önce**,
  `encode_key`'e gider; (c) Shift+PgUp/PgDn bugünkü `page_scroll` kolunda
  kalır (`scroll_page` `None` dönerse bugünkü yol: `encode_key`); (d) kalanı
  `interpretKeyEvents:`e. İlk iki kol yığından sonraya konursa davranış faz
  1'de **aynı görünür** ama ⌘⌫ yığına `deleteToBeginningOfLine:` olarak girer
  ve faz 2 izin listesini yanlış yere kurar; gerekçe `plan.md` R4.2 ve R1.2b.
  `ViewIvars`'a iki alan: `consumed: Cell<bool>` (emsal: yanındaki
  `dragging`) ve bileşim durumu için asgari bir `RefCell<String>`.
  `NSTextInputClient`'ın **11 zorunlu** metodu yazılır — eksiği derlenmez.
  Gövdeli olanlar `insertText:replacementRange:` ve
  `doCommandBySelector:`; dördü bileşim durumundan
  (`setMarkedText:…`, `unmarkText`, `hasMarkedText`, `markedRange`); kalanı
  sabit cevap, **her biri kendi "neden" yorumuyla**.
  `interpretKeyEvents:` `NSResponder`'da ve feature zaten açık;
  `NSArray::from_slice(&[…])` emsali `app.rs`'te.
- **`crates/bt-shell/src/app.rs`** — açılışta `ApplePressAndHoldEnabled =
  false` uygulamanın **kendi** `registerDefaults`'ına yazılır. Kullanıcının
  plist'ine yazılmaz; gerekçe rc dosyasına dokunmama kuralının aynısı.
- **`crates/bt-shell/Cargo.toml`** — `objc2-app-kit`'e `NSTextInputClient`
  feature'ı, dosyanın kendi konvansiyonuna göre **gerekçe yorumuyla**. Aynı
  yorum `NSUserDefaults`'ın feature **istemediğini** (objc2-foundation'ın
  varsayılan setinde) söyler. `Cargo.lock` değişmez.
- **`crates/bt-shell/src/keys.rs`** — imza değişmez (girdi kaydı faz 2'de).
  Yalnız doc: `encode_key`'in artık **hangi olayları gördüğü** yazılır
  (basılabilir metin ona ulaşmıyor) ve `plain_text_passes_as_utf8` ile
  `keys_without_sequences_are_swallowed`'ın ne ölçtüğü güncellenir — ikisinin
  gerçek üreticisi artık yığında.
- **`CLAUDE.md`** — `bt-shell` satırı ve giriş özeti: klavyenin metin yolu
  AppKit'in yığınından geçiyor.

## Kabul

**Kazanç (elle, gerçek pencere):**
- `Option+ü` sonra `Boşluk` → `~`
- `Option+,` sonra `Boşluk` → `` ` `` (bugün hiçbir yolla yazılamıyor)
- `Option+ü` sonra `n` → `ñ`
- `ü` **basılı tutunca** aksan popover'ı **yok**, tuş yineleniyor (R1.6)

**Sıfır regresyon (elle, R1.7 listesi).** Ölçüt "doğruluğu artık AppKit'in
hangi kolu seçtiğine bağlı olan tuşlar":

| tuş | beklenen |
|---|---|
| Enter · Tab · Esc · Backspace | bugünkü baytlar |
| **numpad Enter / Fn-Return** | `\r` — `0x03` **değil**, yoksa her komut kesilir |
| **Ctrl+Y** | `0x19` (yank), Shift+Tab'ın `\e[Z`'si değil |
| **Ctrl+Shift+harf** | `\x03` gibi kontrol baytı, harf değil |
| **düz çok baytlı harf** (`ğ`, `İ`) | UTF-8 bayt bayt |
| oklar · Shift+Tab · fn+Backspace · Shift+PgUp | bugünkü diziler |
| `^A ^C ^D ^E ^K ^U ^W` | bugünkü kontrol baytları |

**Ölçülecek belirsizlik** — hepsinin öznesi aynı: *bekleyen bir bileşim
varken tek bir `keyDown:` yığından kaç geri çağrı üretiyor.* `/code-review`
listeyi üçe çıkardı; üçü de gerçek pencerede, ölü tuş bekletilerek basılır ve
sonuç `## Uygulama Notları`'na yazılır.

| jest | görülecek | "evet" çıkarsa |
|---|---|---|
| `Option+ü` sonra **Enter/Esc** | `~` gelip Enter yeniyor mu (`consumed` erken `true`) | R1.2'nin değişmezine not |
| `Option+ü` sonra **Backspace** | kabuktaki **gerçek** bir harf siliniyor mu (yığın yalnız `unmarkText` çağırıyorsa `0x7f` PTY'ye gider) | `consumed`'a üçüncü setter |
| `Option+ü` sonra **`^C`**, ardından `a` | `a` mı geliyor `ã` mı (Control kolu bileşimi yıkmıyor) | Control kolunda `unmarkText` |

Son iki satırın **karşı hâli de ölçülmemiş**: `unmarkText`'i tüketme saymak
bileşimden sonraki ilk oku yutardı (`unmarkText` + `moveLeft:`). Savunma bu
yüzden ölçümden sonra kurulur — bugün yazılacak kol yanlış yarıyı seçebilir.

**Kapı:** `make hepsi` yeşil. `make duman` **kullanıcı koşar** — klavyeye kör
ama **sınıf kaydına kör değil**: hiçbir sınama `BateriView` üretmiyor, yani
`define_class!`'ın protokol assertion'ı ilk kez orada patlar.

## Yayın Etkisi

- **shader** — yok.
- **terminfo / `TERM`** — yok.
- **ayar şeması** — yok (yeni anahtar yok, `docs/AYARLAR.md` değişmiyor).
- **tema / materyal** — yok.
- **shell entegrasyonu** — yok.
- **app bundle** — yok. `ApplePressAndHoldEnabled` **kodda**, `Info.plist`'te
  değil; kullanıcının plist'ine dokunulmuyor.
- **yeni bağımlılık** — yok. `NSTextInputClient` var olan `objc2-app-kit`'in
  bayrağı, `Cargo.lock` değişmiyor.
- **ölçüm bekliyor:** bellek içi `registerDefaults`'ın press-and-hold
  popover'ını bastırdığı (iTerm2'nin kanıtı *kalıcı* domain değeri, bizimki
  registration domain). Elle turda görülürse kapanır.
- `CLAUDE.md` güncellenir (yukarıda).

## Checklist

- [x] `keyDown:` dört kollu arbitraja ayrıldı; Cmd ve Control kolları yığına
      girmiyor
- [x] `NSTextInputClient`'ın 11 zorunlu metodu yazıldı; `insertText:` ve
      `setMarkedText:`'in `&AnyObject` argümanı tek kuralla çözülüyor
      (`NSString` → `NSAttributedString::string()` → tüketilmedi)
- [x] `doCommandBySelector:` sessiz no-op (bip yok)
- [x] `consumed` bayrağını `insertText:` **ve** `setMarkedText:` set ediyor
- [x] `ApplePressAndHoldEnabled` registration domain'e yazıldı
- [x] `Cargo.toml`'a feature + gerekçe yorumu; `Cargo.lock` değişmedi
- [~] Test: kazanç listesi (üç ölü tuş bileşimi) elle geçti — **kullanıcı
      koşacak**: gerçek pencere ve tuş vuruşu gerekiyor, ajan kabuğunda
      klavye sentezi yok
- [~] Test: R1.7 regresyon tablosunun tamamı elle geçti — **kullanıcı
      koşacak**, aynı gerekçe
- [~] Test: bekleyen ölü tuş + Enter ölçüldü — **kullanıcı koşacak**;
      üç satırlık ölçüm tablosu `## Kabul`'de, mekanizma
      `## Uygulama Notları`'nda
- [x] `keys.rs` ve `CLAUDE.md` doc'ları güncellendi
- [x] Doğrulama geçti (`make hepsi` → exit 0); `make duman` kullanıcıda
      (gerçek pencere ister; R1.1'in protokol assertion'ı ilk kez orada
      patlar)
- [x] Riskli phase: `/code-review` koştu (5 bulgu: 1 düzeltildi, 2 gerekçe
      yazıldı, 2 **waive** — tablo `## Uygulama Notları`'nda)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

- **Kol sırası (b) ↔ (c) takas edildi: Cmd → Shift+PgUp/PgDn → Control →
  yığın.** Planın savunduğu şey "Cmd ve Control **yığından önce**" ve o
  duruyor; ama Control kolu `page_scroll`'un önüne konsaydı Ctrl+Shift+PgUp
  bugünkü kaydırmasını kaybedip `\e[5~`'e düşerdi — `page_scroll`'un doc'u
  "Shift dışındaki değiştiriciler sorulmuyor … Control ya da Option'lı
  Shift+PgUp da kaydırır" diyor ve R1.7 sıfır regresyon istiyor. Gerekçe
  `keyDown:`'ın doc'una yazıldı.
- **`Retained` dönen iki metot `#[unsafe(method_id(...))]` ile yazıldı**
  (`attributedSubstringForProposedRange:` ve `validAttributesForMarkedText`).
  objc2 0.6'da `define_class!` `method(...)` + `method_family` birleşimini
  desteklemiyor ("not yet supported") ve `method(...)` ile `Retained` dönüş
  `Encode` istiyor; `method_id` otoreleaze sözleşmesini seçiciden türetiyor.
- **`characters` artık `keyDown:`'ın ilk kapısı değil.** Eskiden yokluğu
  (saf modifier tuşu) erken `return` ediyordu; bileşimin ilk vuruşunda
  `characters` **boş** geldiği için o `return` ölü tuşu yığına hiç
  ulaştırmazdı. Şimdi `Option<String>` olarak taşınıyor: `page_scroll` ve
  `encode_key` kolları onu `None`'da atlıyor, yığın kolu atlamıyor.
- **`selectedRange`/`markedRange` UTF-16 kod birimi sayıyor** (`String::len`
  değil): `ü` bir kod birimi ama iki bayt ve ölü tuş bileşimi tam o harflerde
  yaşıyor.
- **Ölçülecek belirsizlik — kullanıcının turuna not.** Tablo `## Kabul`'de.
  Üçünün de öznesi tek: bekleyen bir bileşim varken bir `keyDown:` yığından
  kaç geri çağrı üretiyor. Savunma **kurulmadı**; her birinin karşı hâli de
  ölçülmemiş ve bugün yazılacak kol yanlış yarıyı seçebilir.

### `/code-review` bulguları (yüksek efor, 5 bulgu)

| # | bulgu | karar |
|---|---|---|
| 1 | `consumed` yalnız `unmarkText`'le iptal edilen bileşimi göremiyor; ölü tuştan sonra Backspace `0x7f` sızdırabilir | **waive** — düzeltmenin karşı riski de ölçülmemiş (bileşimden sonraki ilk ok yutulur). Ölçüm tablosuna satır, `consumed`'ın doc'una gerekçe |
| 2 | `unmarkText` bileşimi Apple'ın sözleşmesinin tersine **atıyor** | gerekçe yazıldı (PTY'ye akmış baytı geri alacak belge yok; alacritty/ghostty aynı) |
| 3 | Control kolu bekleyen bileşimi yıkmıyor (`Option+ü` → `^C` → `a` = `ã`) | **waive** — kolu yığına sokmanın bedeli numpad Enter; takas `keyDown:`'ın doc'unda, ölçüm tablosunda satır |
| 4 | Press-and-hold bastırması tutmazsa popover'ın harfi `replacementRange` atlandığı için `eé` olur | bileşik belirti `disable_press_and_hold` ve `insertText:`'in doc'una yazıldı |
| 5 | `insert_text`'te çözülemeyen tip `marked_text`'i asılı bırakıyor | **düzeltildi** — `clear()` çözme kuralının önüne alındı |

**İki waive orkestratörde kabul edildi** (2026-09-20). Ortak gerekçe: ikisinin
de **karşı hâli** ölçülmemiş — tüketme saymak bileşimden sonraki ilk oku
yutar, yani düzeltme yeni bir sessiz kayıp açabilir. Üstelik preedit
**çizilmiyor**: `Option+ü`'den sonra kullanıcı hiçbir şey görmüyor, yani
bugünkü belirti (Backspace gerçek bir harfi siler) bugünkü deneyimin ta
kendisi, reviewer'ın önerdiği hâlde ise Backspace sebepsiz hiçbir şey
yapmıyor gibi görünürdü — yanlışın yönü bu kolda daha güvenli. Kalem elle tuş
turunun ölçüm tablosunda ve `teslim.md`'de; **kapatma kararı kullanıcının
ölçümünden sonra**, bu sette değil.
