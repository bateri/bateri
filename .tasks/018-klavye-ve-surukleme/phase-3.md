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

- [ ] `registerForDraggedTypes` + `draggingEntered:` + `performDragOperation:`
- [ ] Okuma `readObjectsForClasses:` + `NSURL::class()`; yol `NSURL.path`'ten
- [ ] `shell_quote` saf, AppKit'siz, kendi sınamalarıyla
- [ ] `Cargo.toml`'a `NSDragging` + gerekçe yorumu; `Cargo.lock` değişmedi
- [ ] Test: kabul tablosunun tamamı elle geçti (boşluklu ad ve çok dosya dahil)
- [ ] Test: `shell_quote`'un kaçış kümesi hermetik olarak çivilendi
- [ ] `view.rs` başlığı ve `CLAUDE.md` güncellendi
- [ ] Doğrulama geçti (`make hepsi`; `make duman` kullanıcıda)
- [ ] Yayın etkisi yazıldı
