# Phase 3 — Finder'dan dosya sürükleme

## Özet

Pencereye bırakılan dosyanın kaçırılmış yolu giriş satırına düşer.

_Requirements: R5, R6 (bu fazın dokunduğu doc'lar)_

## Değişiklikler

- **`crates/bt-shell/src/view.rs`** — view `NSPasteboardTypeFileURL` için
  sürükleme hedefi olur (`registerForDraggedTypes`, `new`'in içinde) ve
  `NSDraggingDestination`'ın **iki** metodunu uygular: `draggingEntered:` →
  `.copy`, `performDragOperation:` → yolları yazar. Protokolün **bütün**
  metotları `#[optional]`, yani `NSTextInputClient`'ın tersine ikisi yeter.
  Okuma API'si **seçili**: `readObjectsForClasses:options:` + `NSURL::class()`
  — ek feature istemiyor (`pasteboardItems()` `NSPasteboardItem` isterdi).
  Yol `NSURL.path`'ten alınır; **yüzde çözme ikinci kez yazılmaz**
  (`bt-core`'un kendi çözücüsü OSC 7 için var, o `bt-core`'da kalır).
- **`crates/bt-shell/src/keys.rs`** (ya da kardeş bir saf modül) —
  `shell_quote`: ters bölüyle kaçar, çok dosya boşlukla ayrılır. Kaçacak küme
  boşlukla bitmiyor: kabuğun metakarakterlerinin tamamı + sekme + satır sonu.
  **Saf ve AppKit'siz**, kendi sınamalarıyla — `keys.rs`/`reaches_terminal`/
  `wheel_lines` emsali. Terminal.app paritesi: `/Users/…/İki\ Kelime/a.txt`.
- **`crates/bt-shell/Cargo.toml`** — `NSDragging` feature'ı, gerekçe
  yorumuyla. `Cargo.lock` değişmez.
- **`crates/bt-shell/src/view.rs` modül başlığı** — view'ın artık bir
  sürükleme hedefi olduğu yazılır.
- **`CLAUDE.md`** — `bt-shell` satırı: sürükleme de onun.

## Kabul

**Elle, gerçek pencere:**

| damla | beklenen |
|---|---|
| tek dosya | kaçmış mutlak yol, giriş satırında |
| **adında boşluk olan dosya** | `İki\ Kelime` — kabuk tek argüman görmeli |
| **birden çok dosya** | boşlukla ayrılmış, her biri ayrı kaçmış |
| klasör | yolu yazılır (yalnız yol, `cd` yok — kapsam dışı) |
| dock satırı sahibiyken tek dosya | dock'a "yazılmış gibi" girer (`can_be_typed`), **doğru davranış** |

Yapıştırılan yol `Session::paste`'ten geçtiği için bracketed paste sarması ve
dock istisnası bedavaya geliyor; `can_be_typed`'ın ham dalı ters bölüyü
sorunsuz geçirir (`\` kontrol karakteri değil).

**Sınama (hermetik):** `shell_quote` kendi testleriyle — boşluk, tırnak, ters
bölü, `$`, `` ` ``, `;`, `&`, satır sonu, Türkçe karakter, çok dosya.

**Kapı:** `make hepsi` yeşil. `make duman` **kullanıcı koşar**.

## Yayın Etkisi

- **shader / terminfo / ayar şeması / tema / shell entegrasyonu / app
  bundle** — yok. (Sürükleme tipi kodda kaydediliyor, `Info.plist`'te
  `CFBundleDocumentTypes` **gerekmiyor**: damla pencereye düşüyor, uygulama
  ikonuna değil.)
- **yeni bağımlılık** — yok. `NSDragging` var olan `objc2-app-kit`'in bayrağı.
- **ölçüm bekliyor** — yok.
- `view.rs` modül başlığı ve `CLAUDE.md` güncellenir.

## Checklist

- [x] `registerForDraggedTypes` + `draggingEntered:` + `performDragOperation:`
- [x] Okuma `readObjectsForClasses:` + `NSURL::class()`; yol `NSURL.path`'ten
- [x] `shell_quote` saf, AppKit'siz, kendi sınamalarıyla (`quote.rs`)
- [x] `Cargo.toml`'a `NSDragging` + gerekçe yorumu; `Cargo.lock` değişmedi
- [~] Test: kabul tablosunun tamamı elle geçti (boşluklu ad ve çok dosya dahil)
      — **kullanıcı koşacak**: gerçek pencere ve Finder'dan sürükleme
      gerekiyor, ajan kabuğunda fare/damla sentezi yok
- [x] Test: `shell_quote`'un kaçış kümesi hermetik olarak çivilendi (altı
      sınama; önce stub'a karşı düştükleri görüldü)
- [x] `view.rs` başlığı ve `CLAUDE.md` güncellendi (+ `docs/YOL-HARITASI.md`,
      R6'nın son kalemi)
- [x] Doğrulama geçti (`make hepsi` → exit 0)
- [~] `make duman` — **kullanıcı koşacak** (gerçek pencere gerekiyor)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

- **`shell_quote` `keys.rs`'e girmedi, kardeşi `quote.rs` doğdu.** Phase
  dosyası ikisine de izin veriyordu; ayıran şey `keys.rs`'in kendi başlığı
  ("tuş vuruşu → PTY baytları ya da ok"): buradaki soru bir tuş değil bir
  damla ve modülü o cümleyle çelişir hâle getirmek, bir dosya kazanıp bir
  sözleşme kaybetmek olurdu. `view.rs`'e koymak da elendi — dosya zaten 1350
  satır ve fonksiyonun AppKit'e hiç bakmayan yarısı orada saklanırdı.
- **Kaçacak küme kara liste değil, beyaz listenin tümleyeni.** Plan "kabuğun
  metakarakterlerinin tamamı" diyordu; o listeyi tek tek saymak `~`, `=`,
  `#`, `!`, `%`'in hangi kabukta kelimenin neresinde özel olduğunu tartışmaya
  açıyordu ve listeden düşen tek karakter sessiz bir hata olurdu. Geçen küme
  ASCII harf/rakam + `/ . _ -` + **ASCII olmayan her şey**; kalan her ASCII
  kaçıyor. Fazladan kaçırmanın bedeli yok (`\+` kabukta `+`), yani yanlışın
  yönü güvenli — ve plandaki küme bunun **içinde**.
- **Satır sonunun bilinen sınırı yazıldı ve çivilendi.** `\` + satır sonu
  zsh'te de bash'te de *satır devamı*, yani adında satır sonu taşıyan dosya
  iki parçası birleşmiş yazılır. Kaçmamak daha ağır (ham satır sonu tamponda
  bir komut sınırı) ve `$'\n'` tek kuralı ikiye bölerdi (Karar 4: tek tip,
  tek kural), yani kol değil **doc** ve **sınama** eklendi.
- **`performDragOperation:`de erken `return` yok ve olamaz.** `define_class!`
  cevabı ObjC'nin `BOOL`'una yalnız **kuyruk ifadesinde** çeviriyor; ilk
  yazımdaki `return false` `expected Bool, found bool` ile derlemeyi kırdı.
  Gövde `match`'e döndü ve gerekçe metodun doc'unda.
- **Damlada `false`'ın iki sebebi birleşti:** oturum yok ya da okunabilen yol
  yok. İkisi de "yazacak bir şey yok" ve AppKit ikisini de damlanın reddi
  olarak gösteriyor.
- **Yol haritası maddesi "tek madde, yalnız borç"a indi**, tek fiziksel
  satıra değil. Home/End'in **şekli** (yeni `pub enum`, ölçülmüş
  `\EOH`/`\EOF`/`smkx`) `plan.md` → Kapsam Dışı'nın ve ikinci tur panelin
  Reddedilenler'inin oraya **emaneti**; silmek iki onaylı belgeyle çelişirdi.
  Çıkan şey **durum** oldu ("018 şunu kapattı", panel tarihleri) — `CLAUDE.md`
  durumun tek sahibini `.tasks/README.md` diye yazıyor. Otuz satır yirmiye
  indi ve içinde yalnız borç kaldı.

## Set kapısı (2026-09-20)

`/code-review` (aralık `05ff10c^..HEAD`, yüksek efor) **8 bulgu**, `/audit`
**0 bulgu** (mekanik yarı `make denetim` temiz; mercek 1/3/4/5/7 temiz,
2/6 ilgisiz — ayar/tema şeması ve `Cell`/shader diff'te yok). Kabul edilmiş
iki waive (phase-1'in tablosu) kapıda **yeniden açılmadı**.

| # | bulgu | karar |
|---|---|---|
| 1 | `insertText:` `replacementRange`'i yoksayıyor; bastıran tek şey `registerDefaults` ve o **en düşük öncelikli** domain — NSGlobalDomain'de ya da MDM'de `ApplePressAndHoldEnabled = 1` olan kullanıcıda popover dönüyor ve kabuğa `eé` gidiyor | **waive** — düzeltme PTY'ye `\x7f` yazar, yani tuşun ne yazdığını değiştirir |
| 2 | `dropped_paths`'in doc'u "`http://` sessizce düşüyor" diyor ama `NSURL.path` `/foo` döndürüyor: karışık damlada uydurma yol giriş satırına giriyor | **düzeltildi** (`e0e4660`, orkestratör) — waive edilemezdi: `CLAUDE.md` çelişen iki cümleden birinin aynı commit'te düzelmesini istiyor ve hangisinin düzeleceğini **plan çoktan söylemiş** (Karar 4 "yalnız dosya URL'si", Kapsam Dışı "metin/URL damlası"), yani kayıt doğru koddu eksik. `isFileURL` kademesi eklendi |
| 3 | `attributedSubstringForProposedRange:` hep `nil`, oysa `markedRange` bileşimde `{0, len}` ilan ediyor | **waive** — `nil` phase-1'de gerekçeli bir karar ("geri okunacak belge yok"); değiştirmek o kararı bozar |
| 4 | `setMarkedText:` bilinmeyen tipte erken dönüyor: ne `consumed` ne durum temizleniyor (`insertText:`'in tam tersi sırası) | **waive** — bileşim durumuna dokunuyor, yani tuşun ne yazdığına |
| 5 | "tam tek karakter" ölçütü **üç** ayrı yazımda (`encode_key`'in `single`'ı, `page_scroll`, `reaches_terminal`) | **düzeltildi** — `keys::only_char` tek sahip, üçü de ona bağlandı; sözleşmeyi `only_char_is_the_single_owner_of_the_one_character_test` çiviliyor |
| 6 | ⌘⌫ izin listesi iki yerde (`reaches_terminal` + `encode_key`'in guard'ı) | **waive** — doğru ama kapının işi değil: klavye arbitrajını set kapanırken yeniden şekillendirmek, elle tuş turu **henüz koşmamışken** riski kazancın üstüne çıkarıyor |
| 7 | `characters().to_string()` her tuşta bir `String` ayırıyor, oysa baskın yol yığının tüketmesi | **waive** — kazanç **ölçülmemiş** ve ertelemenin yolu yok: Cmd izin listesi ile `page_scroll` `chars`'ı yığın kolundan **önce** istiyor |
| 8 | `draggingEntered:` koşulsuz `Copy` diyor, `performDragOperation:` oturumsuzda `false` — imleç kabul gösterip damla reddediliyor | **düzeltildi (doc)** — asimetri `draggingEntered:`'in doc'unda adıyla duruyor; koşul eklemek yanlış ölçüt olurdu (metot sürüklemenin **başında** koşuyor) |

**Kapının düzelttiği iki kalem de davranışa dokunmuyor:** biri saf bir
çıkarma (`only_char`, üç çağrı yeri bit bit aynı cevabı veriyor), öteki bir
doc satırı. Kalanı **hissedilir davranışa** dokunuyor (tuşun ne yazdığı,
damlanın ne ürettiği) ya da yapısal bir yeniden şekillendirme istiyor; ikisi
de kullanıcının/orkestratörün kararı, kapının değil.

### Orkestratörün kararı (2026-09-20)

**#2 düzeltildi** (yukarıda), **kalan beşi kabul edildi.** Gerekçeler ayrı,
ortak değil:

- **#1 (`replacementRange` / press-and-hold)** kabul **ama ölçüme bağlandı**.
  Kapı search order'da haklı — `registerDefaults` en düşük öncelikli halka ve
  NSGlobalDomain ya da MDM onu ezebilir. Ne var ki `plan.md` R1.6 tam bu
  belirsizliği **zaten** "ölçüm bekliyor" diye yazmış: bastırmanın tutup
  tutmadığı hiç ölçülmedi. Bastırma tutuyorsa kalem konusuz, tutmuyorsa
  düzeltme gerekli — ve düzeltme tuşun ne yazdığını değiştirdiği için
  kullanıcının kararı. `teslim.md` → B.2'de on saniyelik bir gözle kontrol ve
  iki çare (uygulamanın kendi kalıcı domain'i / `replacementRange` kadar
  `\x7f`) olarak duruyor.
- **#3, #4 (IME sözleşmesi)** kabul: ikisi de `plan.md`'nin **Kapsam Dışı**
  maddesinin içinde ("Tam IME — CJK ve altı çizili preedit'in çizilmesi").
  Sözleşmenin tam hâli o yüzey doğduğunda yazılır; bugün tüketicisi yok.
- **#6, #7 (çift sahiplik, tuş başına `String`)** kabul: ikisi de temizlik ve
  kapının kendi gerekçesi doğru — klavye arbitrajını **elle tuş turu
  koşmadan** yeniden şekillendirmek riski kazancın üstüne çıkarır. #7 ayrıca
  "ölçülmemiş sayı yazılmaz"ın kapsamında: kazanç ölçülmedi.

**Kapının kendi bulduğu kalem** (`/code-review`'un listesinde yok) phase-2'de:
⌘⌫ ile Finder damlası yığını atlayarak bekleyen bir bileşimin üstünden yazıyor
— `phase-2.md` → "Set kapısının önerdiği waive".

`make hepsi` → exit 0 (düzeltmelerden **sonra**; `bt-shell` 126 → 127 sınama).
