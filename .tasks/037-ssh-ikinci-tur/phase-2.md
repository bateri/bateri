# Phase 2 — Menüden işaretleme ve sekmenin noktası

## Özet

ssh sekmesinde Shell ▸ "Mark “prod” as ▸" host'u tek tıkla işaretleyip
`settings.toml`'a yazıyor; işaretli host'un sekmesi başlığının yanında
işaret renginde bir nokta taşıyor.

_Requirements: R4, R5_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — `SettingsEdit`'e host işareti kolu
  (host + `Option<HostMark>`; `None` menünün "None"u). Yazım kuralı saf ve
  Karar 5'teki gibi: eşit desenli girdi yerinde, yoksa dizinin başına;
  None tam girdiyi siler, ardından bir glob hâlâ eşleşiyorsa başa
  `mark = "none"`; geçerli çözüm seçilenle aynıysa no-op. `[remote]` tablosu
  ya da `hosts` yoksa yaratılıyor; dizinin satır düzeni ve yorumları
  `toml_edit` ile korunuyor (`with_edit`'in bugünkü sözleşmesi).
- **`crates/bt-shell/src/menu.rs`** — Shell menüsüne dinamik öğe: başlık ve
  alt menü bir delegate'ten (`menuNeedsUpdate:`, Theme ▸ emsali — başlık
  etkin sekmeye göre değişiyor), dört öğe ve onay işareti. Aynı menüye
  phase-3'ün New Local Tab'ı bu phase'de değil.
- **`crates/bt-shell/src/app.rs`** — öğelerin eylemi (etkin pencerenin uzak
  hedefinden host'u alıp `with_edit`'le yazar; uygulayan `watch` yolu) ve
  `validateMenuItem:`'ın kolu (uzak değilse gri, başlık "Mark Host as").
  Ayrıştırılamayan dosyaya yazılmıyor, tanı bugünkü yoldan.
- **`crates/bt-shell/src/window.rs`** — sekmenin noktası:
  `window.tab().setAccessoryView` ile küçük, dolu, yuvarlak bir `NSBox`
  (033 panelinin çizim emsali), işaretin renginde; işaretsizde ve yerelde
  `None`. Tazeleme `refresh_title`'ın kenarlarında (`set_remote`,
  `title_changed`), ayar uygulamasında ve `apply_chrome`'da (tema). Renk
  oturumun çözdüğü işaret + oturumun teması, Karar 3'ün tek fonksiyonundan.
- **`crates/bt-shell/Cargo.toml`** — `objc2-app-kit` özellik listesine
  `NSWindowTab` (yalnız başlık bayrağı; yorum satırı 029/033 emsaliyle,
  `Cargo.lock` oynamamalı — oynarsa dur).

## Kabul

- Saf yazım sınamaları: boş dosyaya ilk işaret `[remote] hosts`'u yaratıyor;
  var olan eşit girdi yerinde değişiyor, sıra korunuyor; yeni host başa;
  None tam girdiyi siliyor; glob eşleşiyorsa başa `none`; aynı işaret no-op;
  yorum ve bilinmeyen anahtar korunuyor.
- Menü: uzak olmayan sekmede öğe gri; uzakta başlık host'u taşıyor ve onay
  geçerli çözümde (glob'dan gelse de) — saf bir "menü modeli" fonksiyonuyla
  sınanabildiği kadar; AppKit yarısı gözle.
- `Cargo.lock` değişmedi (`make denetim` uyarmıyor).

## Checklist

- [ ] `SettingsEdit` kolu ve saf yazım kuralı
- [ ] Shell ▸ Mark … as ▸ (delegate, eylem, `validateMenuItem:`)
- [ ] Sekmenin noktası ve tazeleme kenarları
- [ ] `NSWindowTab` bayrağı
- [ ] Test: yukarıdaki Kabul maddeleri
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
- [ ] Gözle kontrol (devir mesajının cümlesi): iki sekme aç, birinde
  `ssh <host>` — Shell ▸ "Mark “host” as ▸ Production": **sekme çubuğunda**
  ssh sekmesinin başlığının yanında kırmızı nokta, **dock**'ta `⇄ host` ve üst
  çizgi kırmızı, `settings.toml`'da dizinin başında yeni satır; "None" noktayı
  kaldırıp camgöbeğine döndürüyor; yerel sekmede öğe gri. `exit` noktayı
  kaldırıyor. Tema değişince nokta yeni temanın rengini alıyor.
