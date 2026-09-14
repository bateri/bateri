# Phase 3b — Tam ekran uygulamada tekerlek: fare tekerlek raporu ve ok tuşları

## Özet

Tekerlek, fare raporlaması isteyen uygulamaya (vim/nvim `mouse=a`, htop)
**fare tekerlek dizisi** olarak, istemeyen tam ekran uygulamaya (`less`,
`man`) **ok tuşu** olarak gider; klavye oklarının kodlaması DECCKM'e
(`\e[?1h`) uyar.

_Requirements: R3.3, R3.4, R3.5_

---

## Neden bu phase var

phase-3 R3.3'ü ("alternate screen'de tekerlek yoksayılır") uyguladı ve
`/code-review` bedelini buldu: `man ls` ve `less` tekerlekle
kaydırılamıyor. Kullanıcı 2026-09-14'te iki karar verdi (`discussion.md` →
Karar 4 eki):

1. "less man gibi yerlerde scroll edememek çok kötü bir olay" → tekerlek
   alternate screen'de ok tuşlarına çevrilir (xterm'in "alternate scroll"
   kipi, DECSET 1007; alacritty'de `TermMode::ALTERNATE_SCROLL` **varsayılan
   açık**). phase-3'ün `## Uygulama Notları` → "WAIVE önerisi — DECSET 1007"
   maddesinde sınanmış bir taslak var; oradan başla.
2. Fare raporlaması isteyen uygulamalar (macOS vim'i `.vimrc` yoksa
   `defaults.vim` ile `mouse=a` açıyor; nvim'de varsayılan açık; htop) ok
   tuşu değil fare olayı bekler → fare raporlamasının **yalnız tekerlek
   kolu** bu phase'e girer. Tıklama, sürükleme ve hareket raporlaması girmez:
   onlar bugünkü gibi seçim yapar.

Sıra: phase-4'ten sonra, phase-5'ten (boşta kare sınırının yeniden ölçümü)
**önce** — ölçüm kodun son hâline alınmalı (R5.1).

---

## 1. Tekerleğin karar tablosu — `bt-core`'da

`crates/bt-core/src/session.rs` → `scroll_by` ve çevresi. Kip `Term`'de
yaşıyor, karar orada verilir (R3.3'ün katman gerekçesi aynen geçerli).
Sıra alacritty'nin kendi davranışıyla aynı (`alacritty/src/input/mod.rs` →
`scroll_terminal`, `mouse_report`, `normal_mouse_report`, `sgr_mouse_report`;
depoda yok — orkestratör 2026-09-14'te `master`'dan okudu, aşağıdaki
ayrıntılar oradan):

| koşul | tekerlek ne yapar |
|---|---|
| herhangi bir fare raporlama kipi açık (`TermMode::MOUSE_MODE`: 1000/1002/1003), ekran fark etmez | işaretçinin altındaki hücre için satır başına bir **tekerlek raporu** (§3) |
| değilse: `ALT_SCREEN` **ve** `ALTERNATE_SCROLL` açık **ve** Shift basılı değil | satır başına bir **ok tuşu** (§2): geriye (artı) → yukarı, ileriye (eksi) → aşağı |
| değilse: `ALT_SCREEN` açık (`\e[?1007l` ya da Shift basılı) | yoksay |
| değilse (birincil ekran) | `scroll_display` — **davranış değişmez** |

Shift+PgUp/PgDn (`scroll_page`) bu tablodan geçmez: klavyedir, fare raporu
ya da ok üretmez; alternate screen'de `None` → "tuşu uygulamaya geçir"
sözleşmesi aynen kalır.

Baytlar `write_owned`'dan mı yoksa doğrudan PTY'ye mi gidecek, karar senin;
gerekçesini yaz: `write_owned` girdide pencereyi dibe döndürüyor (phase-3
sapması). Birincil ekranda fare kipi açıkken pencere geçmişe kaydırılmışsa
alacritty görünen hücreyi grid mutlağına çeviriyor ve işaretçi **geçmiş**
satırındaysa (mutlak satır `< 0`) raporu **göndermiyor** — aynısını yap.

**Kare:** rapor ya da ok göndermek kare **istemez** — uygulama ekranını
yeniden çizince okuyucu thread'in `Wakeup`'ı kareyi getirir. Boşta sıfır kare
korunur.

**Dönüş tipi:** bugün alternate screen `None` döndürüyor ve `view` `None`'da
`scroll_carry`'yi sıfırlıyor. Baytlar gönderildiğinde artık sıfırlanmamalı —
yoksa trackpad'le yavaş kaydırmada her olayın küsuratı düşer ve uygulama
sarsak kayar. Ayrımı tipte taşı (ör. `Option<i32>` yerine kollu bir
`pub enum`; **alacritty tipi pub API'ye çıkmaz**).

**İşaretçi hücresi:** tekerlek raporu hücre koordinatı ister. `bt-shell`
`scrollWheel:`'de olayın konumunu mevcut `point_to_cell` ile hücreye çevirir
(yeniden türetme; ızgara dışı kırpması zaten orada) ve `bt-core`'a geçirir.
`SelectionPoint`'in `half` alanı rapora girmez — ya yalnız `col`/`row`
geçir ya da neden tümünü geçirdiğini yaz.

## 2. Ok kodlaması DECCKM'e uyar — tek kaynak

`crates/bt-shell/src/keys.rs` bugün okları koşulsuz `\e[A`…`\e[D` diye
kodluyor. Ama `TERM=xterm-256color`'ın terminfo'su `smkx=\E[?1h\E=` ve
`kcuu1=\EOA` diyor: terminfo okuyan uygulama (less, ncurses) açılışta DECCKM'i
açar ve `\EOA` bekler. Tekerleğin gönderdiği ok da aynı kurala uymak zorunda,
yoksa özellik tam da hedeflediği uygulamada boşa düşebilir.

- Kodlama **tek yerde**: tekerlek ve klavye aynı yardımcıdan geçer; ok baytı
  iki dosyada iki kez yazılmaz.
- DECCKM sorusu `bt-core`'da cevaplanır; `bt-shell` kip tutmaz ve alacritty
  tipini görmez.
- `APP_CURSOR` açık → `\eOA`/`\eOB`/`\eOC`/`\eOD`; kapalı → `\e[A`…`\e[D`.
  alacritty'nin klavye bağları da böyle.
- **Tekerleğin okları:** alacritty burada DECCKM'e **bakmıyor**, her zaman
  `\eOA`/`\eOB` gönderiyor; xterm DECCKM'e göre gönderiyor. İkisi de çalışır
  (uygulamalar iki biçimi de tanıyor). Tek kaynak ilkesi DECCKM'e duyarlı
  yardımcıyı paylaşmayı öneriyor; hangisini seçtiğini gerekçesiyle yaz.
- `keys.rs` Home/End kodluyorsa aynı kural onlara da uygulanır
  (`khome=\EOH`, `kend=\EOF`); kodlamıyorsa ekleme, kapsam dışı.
- Değiştiricili oklar (`\e[1;2A` vb.) kapsam dışı.

Klavye yolunda kip sorgusu tuş başına bir `Term` kilidi demek; phase-3 girdi
başına zaten bir kilit alıyor (`write_owned`). İkinci bir kilit ekleme —
birleştirilebiliyorsa birleştir, gerekçesini yaz.

## 3. Tekerlek raporu — yalnız tekerlek kolu

Düğme kodu: geriye/yukarı **64**, ileriye/aşağı **65**; tekerleğin bırakma
olayı yok, yalnız basma gönderilir. Koordinatlar 1 tabanlı (`col + 1`,
`row + 1`). Kodlama kipe göre:

| kip | bayt dizisi |
|---|---|
| SGR (`TermMode::SGR_MOUSE`, 1006) | `\e[<{kod};{col+1};{row+1}M` |
| UTF-8 (`TermMode::UTF8_MOUSE`, 1005) | `\e[M` + `32+kod` + UTF-8(`32+col+1`) + UTF-8(`32+row+1`) |
| düz (X10/normal) | `\e[M` + `32+kod` + `32+col+1` + `32+row+1` (her biri tek bayt) |

Sınır alacritty'de: `col` ya da `row` düz kipte `>= 223`, UTF-8 kipinde
`>= 2015` ise rapor **gönderilmez**. UTF-8 kipinde `32+1+pos` değeri `>= 128`
(yani `pos >= 95`) olunca iki bayt: `0xC0 + v/64`, `0x80 + (v & 63)`; altında
tek bayt. SGR'ın sınırı yok. Sınırları sınamaya bağla.

Kapsam dışı (notlara yaz): değiştirici bitleri (alacritty ekliyor: Shift +4,
Alt +8, Ctrl +16 — bizde `0`),
tıklama/sürükleme/hareket raporları, SGR-pixel (1016), yatay tekerlek
(66/67). Tıklama raporlanmadığı için `mouse=a` açık vim'de fareyle tıklamak
imleci taşımaz, seçim yapar — bugünkü davranış.

Kodlama `bt-core`'da platformsuz ve saf fonksiyon olarak sınanabilir olsun.

**`git log` bu phase'in konusu değil:** git `LESS` tanımsızsa `FRX` veriyor
ve `-X` less'in alternate screen'e geçişini kapatıyor; `git log` birincil
ekranda koşuyor ve tekerlek terminal geçmişini kaydırıyor — Terminal.app ve
alacritty'de de aynı. Göz kontrolüne bu yüzden girmedi.

---

## Uygulama Notları

- **Başlangıç: phase-3'ün DECSET 1007 taslağı.** Taslak `scroll_by`'ın
  alternate screen kolunda `ALTERNATE_SCROLL && !MOUSE_MODE` ise satır başına
  ok `send` ediyordu. Karar tablosu onun genişlemiş hâli, gönderim de taslaktaki
  gibi `send` (aşağıda).
- **Yapı: karar ve kodlama saf, yeni `bt-core/src/input.rs`'te.**
  `wheel_route(TermMode, shift)`, `arrow(Arrow, TermMode)` ve
  `wheel_report(encoding, button, col, row)` PTY'siz sınanıyor. `session.rs`'te
  yalnız kilit, pencere→grid satırı inişi ve gönderim var. Ayrı modülün sebebi
  sınanabilirlik ve `session.rs`'in boyu. `TermMode` bu fonksiyonlarda
  `pub(crate)`, yani pub API'ye çıkmıyor. Yeni pub tipler `Arrow` ve `Wheel`,
  ikisi de bizim.
- **API (sapma: ad).** `Session::scroll_display(lines) -> Option<i32>` →
  `Session::scroll_wheel(lines, at, shift) -> Wheel`, varyantları
  `Scrolled(i32) | Sent | Ignored`.
  - İmza zaten değişiyordu (işaretçi + Shift). "display" artık doğru değil:
    tekerlek uygulamaya da gidiyor.
  - Eski `scroll_display`'in yerine başka bir pub yol bırakılmadı. Bıraksaydık
    tabloyu atlayan ikinci bir tekerlek yolu olurdu.
  - Birincil ekranın eski sınamaları yeni API'den geçiyor (`scroll` yardımcısı).
    "Birincil ekran değişmedi"nin bekçisi onlar; Shift'li kaydırma eklendi.
  - `scroll_page` `Option<i32>` olarak **kaldı**: kılavuz sözleşmeyi aynen
    bıraktı. Sayfa tuşu hiçbir şey göndermez, yani `Sent` ona uymaz.
  - Alternate screen'de Shift+PgUp hâlâ düz PgUp olarak uygulamaya düşüyor.
    phase-3 WAIVE notundaki "yeniden düşünülmeli" maddesinin cevabı bu:
    dokunulmadı.
  - İç yapı: `scroll_by` gitti. Yerine kilit altında koşan serbest
    `scroll_locked` (kip kapısı + kırpma) geldi; üç kaydırma yolu onu
    paylaşıyor. `write_owned` artık `send_input`'a gidiyor (kip sorusu + dibe
    dönüş tek kilitte).
- **İşaretçi.** `SelectionPoint` geçiyor, `half` okunmuyor.
  - Neden tamamı: alanları adlı, yani sütun ile satır yer değiştirirse derleme
    kırılır. View'ın `event_cell` → `point_to_cell` yolu onu zaten veriyor.
  - Rapor hücre çözünürlüğünde; SGR-pixel (1016) kapsam dışı.
  - `bt-core` kırpmıyor; ızgara dışı kırpma `point_to_cell`'de.
  - Pencere geçmişteyse hücre `viewport_point` ile grid satırına iniyor. Satır
    `< 0` ise rapor gitmiyor (`Ignored`), alacritty'nin kuralıyla aynı.
- **Gönderim: doğrudan kanala, `write_owned` üzerinden değil.** `write_owned`
  girdide pencereyi dibe döndürüyor.
  - Birincil ekranda fare kipi açık ve pencere Shift+PgUp ile geçmişteyken her
    rapor pencereyi dibe atardı. Raporlanan hücre de kullanıcının baktığı
    yerden kayardı.
  - Alternate screen'de dibe dönüş boş iş, üstelik ikinci bir `Term` kilidi.
  - alacritty'nin rapor ve ok yolu da dibe dönmüyor.
- **Tekerleğin ok biçimi: DECCKM'e duyarlı** (xterm gibi; alacritty her zaman
  SS3 gönderiyor).
  - Tekerlek, uygulamanın **aynı kipte klavye okundan** alacağı baytın aynısını
    alıyor.
  - less ve ncurses `smkx` ile DECCKM'i açıyor. Onlarda bayt alacritty'ninkiyle
    aynı (`\eOA`).
  - DECCKM'i açmayan uygulamada CSI gidiyor, klavyeyle tutarlı.
  - DECCKM sorusu depoda tek yerde: `input::arrow(arrow, mode)`. `/simplify`
    öncesinde `bool` alıyordu ve iki çağıran ayrı ayrı çözüyordu.
- **Klavye kilidi.** Ok tuşu tuş başına **tek** `Term` kilidi alıyor:
  `send_input` kip sorusunu ve dibe dönüşü aynı kilitte yapıyor. İkinci kilit
  eklenmedi.
- **Sapma: uygulamaya giden tekrar bir sayfaya kırpılıyor** (kılavuzda yok).
  - Neden: `wheel_lines` doyarak `i32::MAX` verebiliyor, kırpılmasa bu kadar
    tekrarlık bir tampon kurulurdu.
  - Sayfa (görünen satır sayısı) **ölçülmüş bir sınır değil**, `Term`'in
    verdiği tek ölçü. Göz kontrolü hızlı jestte kesilme görürse gevşetilir.
  - Bekçisi `alternate_screen_wheel_sends_arrows`: `i32::MAX` → 10 ok.
- **Sapma: uygulama yolunda sıfır satır, sığmayan koordinat ve geçmişteki
  işaretçi `Ignored` dönüyor.** Boş `Msg::Input` yazıcıyı kilitlerdi.
  `send_input` boş baytı kendisi düşürüyor; bu kural doc'ta değil kodda.
- **Sapma (küçük): okların kolları `single` korumasını aldı.** phase-3'ün
  "okların kolları bu korumayı taşımıyor — borç" notu kapandı. Okla başlayan
  çok karakterli bir `characters` artık ok diye okunmuyor, yutuluyor.
- **Home/End:** `keys.rs` kodlamıyordu, tuş yutuluyordu. Eklenmedi, kapsam dışı.
- **Kapsam dışı (kılavuz gereği):**
  - Değiştirici bitleri (alacritty'de Shift +4, Alt +8, Ctrl +16; bizde 0).
    Fare kipinde Shift'li tekerlek düz düğme kodunu gönderiyor.
  - Tıklama, sürükleme ve hareket raporları. `mouse=a` vim'de tıklama imleci
    taşımıyor, seçim yapıyor.
  - SGR-pixel (1016) ve yatay tekerlek (66/67).
  - Değiştiricili oklar (`\e[1;5A`).
- **Davranış değişikliği:** birincil ekranda fare kipini açan uygulamada
  tekerlek eskiden geçmişi kaydırıyordu, artık rapor gidiyor (tablonun 1.
  satırı, "ekran fark etmez"). O hâlde geçmişe Shift+PgUp ile iniliyor.
- **`[elle]` için not:** macOS klasik farede Shift+tekerleği yatay deltaya
  çeviriyor. Bu yolda `lines == 0` olur, yani Shift kolu çoğunlukla trackpad'den
  gelir.
- **Sınama altyapısı.** PTY'ye giden baytlar phase-2'nin `od` kalıbıyla
  okunuyor, iki farkla:
  - Çocuk önce `stty -echo -icanon` koşuyor. Ok ve rapor dizilerinde `\n` yok,
    kanonik kipte `od` onları görmezdi.
  - `expect_sent` `od`'nin 16 baytlık bloğunu noktayla dolduruyor ve iğne
    dolguyu da taşıyor. Sayıyı çivileyen bu: bir fazla ya da bir eksik dizi
    iğneyi hiç göstermez.
  - Sınırı (`/code-review`): aynı iğne bir oturumda iki kez sorulamaz, çünkü
    eski döküm ekranda kalıyor. Kural doc'ta; dört "hiçbir şey gitmedi"
    sorgusunun hepsi oturumunun ilk adımı.
- **Checklist → sınama:**
  - Oklar: `alternate_screen_wheel_sends_arrows`.
  - DECCKM: `arrows_follow_decckm_from_keyboard_and_wheel` +
    `input::arrow_follows_decckm`.
  - SGR ve fare kipinin önceliği: `mouse_mode_wheel_sends_sgr_reports`. Sahne
    alternate screen'de ve 1007 açık, Shift'li tekerlek de rapor gönderiyor.
  - Düz, UTF-8 ve sınır: `mouse_mode_wheel_sends_plain_and_utf8_reports` (224
    sütunluk grid'de 222/223 ve 94/95) + `input::*_report_*` (2014/2015 dahil).
  - Geçmişteki işaretçi: `wheel_report_skips_a_pointer_in_history`.
  - `\e[?1007l`, Shift ve sıfır satır:
    `wheel_is_ignored_without_alternate_scroll_or_with_shift`. Eski
    `alternate_screen_ignores_scroll`'un yerini aldı; `Ignored` ≠ `Scrolled(0)`
    ayrımını o soruyor.
  - Kare: `wheel_to_the_app_requests_no_frame`.
  - Karar tablosunun her satırı: `input::{mouse_mode_comes_first_on_either_screen,
    alternate_screen_turns_the_wheel_into_arrows,
    primary_screen_scrolls_whatever_else_is_set}`.
  - Dibe dönüş: `input_returns_the_view_to_the_bottom`, artık okla birlikte.
  - Test-first: saf sınamalar `todo!()` gövdeleriyle, PTY sınamaları eski
    davranışlı taslak API'yle kırmızı görüldü (7 kırmızı), sonra uygulandı.
- **Mutasyonlar (hepsi kırmızı düştü, geri alındı):**
  - Saf katman:
    - sınırda `>=` → `>`: düz ve UTF-8 rapor sınamaları kızardı.
    - `intersects` → `contains`: `MOUSE_MODE` sınamaları kızardı.
    - Shift'in yok sayılması: alternate screen sınaması kızardı.
  - Oturum katmanı:
    - gönderimde `request_frame`: `wheel_to_the_app_requests_no_frame`.
    - geçmiş kontrolünün silinmesi: `wheel_report_skips_a_pointer_in_history`.
    - tekerlek ya da klavye okunun DECCKM'e bakmaması:
      `arrows_follow_decckm_from_keyboard_and_wheel`.
    - `Ignore` → kaydırma:
      `wheel_is_ignored_without_alternate_scroll_or_with_shift`.
    - sayfa kırpmasının `u16::MAX`'e gevşemesi:
      `alternate_screen_wheel_sends_arrows`.
    - ok yönünün terslenmesi: iki sınama.
    - `send_input`'un dibe dönmemesi: `input_returns_the_view_to_the_bottom`.
- **`/simplify` (4 mercek; reuse ve sadeleştirme sonnet, verimlilik ve
  irtifa opus).**
  - **Uygulanan:**
    - `wake_if_moved`: "kaydıysa kare iste" üç yerde iki biçimde yazılıydı.
    - DECCKM sorusu `input::arrow(arrow, mode)`'a indi;
      `WheelRoute::Arrows`'un yükü kalktı.
    - `send_input` boş baytı kendisi düşürüyor; kural doc'tan koda geçti.
    - DECCKM gerekçesi dört yerde tekrarlıyordu; `Arrow`'da tek yere indi,
      diğerleri ona bağlanıyor.
  - **Atlanan:**
    - Tekerlek baytlarını kilitten sonra kurmak: birkaç baytlık ayırma. Taşımak,
      hiç koşmayan bir kol doğururdu.
    - `repeat` ve kapasite mikro ayırmaları: değmez.
    - View'da `event_cell` yerine `point_to_cell`'i doğrudan çağırmak: kılavuz
      mevcut yolu yeniden kullanmayı istiyor; kazanç birkaç objc mesajı.
    - Genel `Key` enum'u ve `write_key`: bugünkü kapsam istemiyor.
- **`/code-review` (high).** Ürün kodunda doğruluk bulgusu yok. Tek düşük bulgu
  sınama yardımcısındaydı: `expect_sent`'in "adımlar sıralanabilir" iddiası aynı
  iğnede yeşil geçer. Giderildi: sınır doc'a yazıldı, çağrı yerleri kurala
  uyuyor.
- **`/audit`.**
  - Temiz:
    - 1: `cargo tree` + kaynak grep, `bt-core`'da platform kütüphanesi yok.
    - 2: `Cargo.toml`/`Cargo.lock` el değmedi.
    - 3: yeni `unwrap`'lerin hepsi `#[cfg(test)]`'te.
    - 6: ölçüm sayısı ya da ölçülmemiş iddia yok.
    - 7, thread ve blokaj (opus): kare isteği ve gönderim kilit bırakıldıktan
      sonra; closure'lar `Session`'a geri girmiyor; `term → size` sırası
      korunuyor; olay başına kilit sayısı değişmedi.
    - 8, boşta sıfır kare (opus): `Sent` ve `Ignored` kare istemiyor; zamanlayıcı
      ya da animasyon yok; duman bu yollardan geçmiyor.
  - İlgisiz: 4 (ayar), 5 (shell), 9 (hücre/shader).
  - **10, belge borcu (opus): altı bulgu, beşi giderildi.**
    - `write_owned`'ın "tek gönderim noktası" iddiası artık `send_input`'u ve
      tekerlek istisnasını sayıyor.
    - `scrollWheel:` doc'u fare kipinin ekrandan bağımsız olduğunu söylüyor.
    - `Wheel::Ignored` sıfır satırı sayıyor.
    - `view` modül doc'u "bayt ya da ok" diyor.
    - `keys.rs` sarması düzeldi ve gereksiz takma ad gitti.
    - Lens 7'nin spekülatif notu doc'a girdi: `send_input`'un closure'u
      `Session`'a dokunmamalı.
  - 8'in iki notu bulgu değil:
    - Geçmişe kaydırılmış pencerede akan çıktının kare istemesi bilinen sınır
      (phase-3, `AdapterInner::dirty`).
    - Momentum uygulamanın ucunda da ok gönderiyor; kareyi uygulamanın çıktısı
      istiyor, jest bitince duruyor.
- **Doğrulama (kapı sonrası koşu):** `make hepsi` → 0; `make duman` → 0
  (`kare=1 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke istek=2
  kapanis=clean profil=debug ornek=off pipeline=ok`); `make test-yaris` → 0.
  `make test-yaris` zorunluydu, çünkü kilit yolu değişti (`scroll_by` →
  `scroll_locked`, `write_owned` → `send_input`).
- **WAIVE önerisi — `CLAUDE.md` `bt-core` satırı girdi kodlamasını saymıyor**
  (lens 10'un altıncı bulgusu). Çelişki değil, eksik: phase-3'ün `bt-shell`
  satırının fareyi saymamasıyla aynı sınıf. `CLAUDE.md`'yi implementer
  kendiliğinden değiştirmedi; eklenecekse "girdi kodlaması (DECCKM oku,
  tekerlek raporu)" yeter. Karar orkestratörün/kullanıcının.

## Yayın Etkisi

- **Tekerlek tam ekran uygulamada çalışıyor.**
  - Alternate screen'de ok olarak gidiyor. DECSET 1007 varsayılan açık;
    `\e[?1007l` ve Shift bunu kesiyor.
  - Fare kipinde (1000/1002/1003, ekran fark etmez) tekerlek raporu olarak
    gidiyor: SGR (1006), UTF-8 (1005) ya da düz kodlama.
  - Birincil ekranda, fare kipi kapalıyken davranış değişmedi.
- **Davranış değişikliği:**
  - Birincil ekranda fare kipini açan uygulamada tekerlek geçmişi
    kaydırmıyor, rapor gönderiyor.
  - Klavye okları DECCKM'e uyuyor: `\e[?1h` altında `\eOA`…, kapalıyken
    `\e[A`… (önceden koşulsuz `\e[A`).
  - Okla başlayan çok karakterli girdi yutuluyor.
- **`TERM` ve terminfo değişmedi.** Değişiklik `xterm-256color`'ın
  `smkx`/`kcuu1=\EOA`'sına uyum.
- Yeni bağımlılık yok (`Cargo.toml`/`Cargo.lock` el değmedi). Ayar şeması,
  tema, shell entegrasyonu, `.metal` ve bundle: el değmiyor.
- **Ölçüm bekleyen iddia yok.** Gönderimin kare istemediği sınamayla bağlı.
  Bir olayda bir sayfalık tekrar sınırı ölçülmüş bir sayı değil; `[elle]` göz
  kontrolü hızlı jestte kesilme görürse gevşetilir.
- `CLAUDE.md`: `bt-core` satırına "girdi kodlaması" eklenmesi WAIVE önerisi
  olarak yukarıda. `docs/MIMARI.md` yok.
- `[elle]` göz kontrolü bekliyor (kullanıcı).

---

## Checklist

- [x] phase-3'ün DECSET 1007 taslağı okundu, ondan başlandı
- [x] Karar tablosu `bt-core`'da: fare kipi → rapor (geçmiş satırında rapor yok); alternate screen + 1007 + Shift yok → ok; `\e[?1007l`/Shift → yoksay; birincil ekran değişmedi
- [x] Baytlar gönderildiğinde `scroll_carry` korunuyor (ayrım tipte, pub API'de alacritty tipi yok)
- [x] İşaretçi hücresi `point_to_cell`'den geliyor
- [x] Ok kodlaması DECCKM'e uyuyor; tekerlek ve klavye aynı kaynaktan
- [x] Tekerlek raporu: SGR, UTF-8 ve düz kodlama; düz kipte sığmayan koordinatta rapor yok
- [x] Test: alternate screen'de tekerlek → PTY'ye `\e[A`/`\e[B` (sayı ve yön doğru)
- [x] Test: DECCKM açıkken klavye oku `\eOA`, kapalıyken `\e[A` gönderiyor; tekerleğin ok biçimi seçilen kurala uyuyor
- [x] Test: `\e[?1000h\e[?1006h` altında tekerlek → `\e[<64;{c};{r}M` satır başına; aşağı yönde 65
- [x] Test: `\e[?1000h` (SGR yok) altında düz kodlama doğru; `\e[?1005h` altında `pos >= 95` iki bayt; sınırı aşan koordinatta hiçbir şey gitmiyor
- [x] Test: fare kipi alternate scroll'dan önce geliyor; `\e[?1007l` ve Shift altında hiçbir şey gitmiyor
- [x] Test: rapor/ok göndermek kare istemiyor (kirli bayrağı dikilmiyor)
- [ ] `[elle]` göz kontrolü: `man ls` ve `less` tekerlek + trackpad'le kayıyor; `less`'te klavye okları çalışıyor; `nvim` (ya da `.vimrc`'siz `vim`) ve `htop` tekerlekle kayıyor; birincil ekranda geçmişe kaydırma bozulmadı
- [x] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**, kilit yolu değişirse `make test-yaris`)
- [x] `/simplify` çalıştırıldı, bulgular uygulandı
- [x] `/code-review` çalıştırıldı, bulgular giderildi
- [x] `/audit` çalıştırıldı, bulgular giderildi
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
