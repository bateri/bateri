# Phase 1 — Yazma yolu, değer tabloları ve aile listesi

## Özet

Pencerenin ihtiyaç duyduğu üç veri yolunu görünür davranışı değiştirmeden
kurmak: tipli ayar düzenlemesi, geçerli değerlerin `pub` tabloları ve
eşaralıklı ailelerin listesi.

_Requirements: R1, R1.1, R1.2, R2, R3_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`**
  - Her dizge enum'u (`CaretShape`, `CursorBlink`, `CursorMotion`,
    `ReduceMotion`, `SmoothScroll`, `ShellIntegration`; `UnfocusedCaret` ve
    `ConfirmClose` zaten) tek `pub const NAMES: &[(&str, Self)]` taşır;
    `name()` `pub` olur ve tablodan okur. Ayrıştırıcı kolları **tablodan
    arar**; her anahtarın tanı cümlesi bayt bayt aynı kalır (mevcut tanı
    sınamaları bekçi). Aralıklar `pub`: `SCROLLBACK_MAX`,
    `CURSOR_BLINK_RANGE`, `CURSOR_RADIUS_RANGE`, `CURSOR_GLOW_RANGE`,
    `MAX_LINE_HEIGHT`'in alt ucu dahil bir `LINE_HEIGHT_RANGE`. Doc'lardaki
    "tek sahip" gerekçeleri korunur, `pub(crate)` gerekçesi güncellenir.
  - `pub enum SettingsEdit` — anahtar başına bir varyant, tipli değer
    (`Scrollback(usize)`, `Cursor(CaretShape)`, `CursorBlink(..)`,
    `CursorRadius(f64)`, `CursorGlow(f64)`, `CursorUnfocused(..)`,
    `BlinkInterval(f64)`, `ConfirmClose(..)`, `Theme(String)`,
    `LightTheme(String)`, `DarkTheme(String)`, `FontFamily(String)` — boş
    dizge varsayılan, `FontSize(f64)`, `LineHeight(f64)`, `Osc52(..)`,
    `CursorMotion(..)`, `ReduceMotion(..)`, `SmoothScroll(..)`,
    `ShellIntegration(..)`); bölümü ve anahtarı varyanttan türer.
  - `Settings::with_edit(text, &SettingsEdit) -> Result<String, Diagnostic>`:
    `with_theme`'in gövdesi genelleşir (bölüm/anahtar reddi, kuyruk yorumu,
    süs, CRLF — hepsi aynı kod). `with_theme(text, name)` =
    `with_edit(text, &SettingsEdit::Theme(..))`; imzası ve sınamaları yerinde.
    Ondalık iki basamağa yuvarlanıp yazılır; tamsayı TOML tamsayısı.
- **`crates/bt-core/src/session.rs`** — `Osc52` aynı `NAMES` örüntüsüne
  (ayrıştırıcısı `settings.rs`'te, tablosu tipin yanında).
- **`crates/bt-core/src/lib.rs`** — yeni `pub` adların yeniden ihracı (mevcut
  örüntüyle).
- **`crates/bt-shell/src/settings.rs`** — `write_theme` → `write_edit(root,
  &SettingsEdit)`; oku-yarat-yerinde yaz yolu ve "…; the setting was not
  saved" iletisi aynı yapıda. Menü çağıranı (`save_theme`) `write_edit`'i
  `Theme` ile çağırır; mevcut `write_theme` iletisi (`the theme was not
  saved`) menü yolunda korunur.
- **`crates/bt-atlas/src/font.rs`** — `pub fn monospaced_families() ->
  Vec<String>`: `CTFontManagerCopyAvailableFontFamilyNames` + her ailenin
  `open`'ı + `open_chain`'in `TraitMonoSpace` testi (ortak yardımcıya
  çıkarılır, ölçüt tek yerde); nokta ile başlayan sistem aileleri (`.AppleSystemUIFont`)
  elenir; sıra harf duyarsız. `bt-gpu/src/lib.rs` yeniden ihraç eder
  (`FontNotice` emsali).
- **`Cargo.toml` (workspace)** — `objc2-core-text`'e `CTFontManager` bayrağı,
  satırında gerekçe. `Cargo.lock` **değişmemeli**; değişirse dur ve eskale et.

## Kabul

- `cargo test -p bt-core`: her `SettingsEdit` varyantı için yaz → `parse` →
  aynı değer; yorumlu, bilinmeyen anahtarlı, satır içi tablolu, noktalı
  anahtarlı, CRLF ve kuyruk yorumlu şablon metinlerinde yalnız o satırın
  değiştiği (`with_theme`'in mevcut sınamaları olduğu gibi yeşil, genelleme
  onları her varyant için tekrar eder — tablo sürücülü tek sınama yeterli).
- Her enum için `NAMES`'in her yazılışı ayrıştırıcıdan o varyantı verir ve
  `name()` ile döner (tablo ↔ ayrıştırıcı bekçisi).
- Tanı metinleri değişmedi (mevcut sınamalar yeşil, beklenen dizgeleri
  dokunulmadı).
- `cargo test -p bt-atlas`: liste boş değil, `Menlo` içinde, `Helvetica`
  dışında; listedeki her aile `open_chain`'den `NotMonospaced` uyarısı almaz.
- Görünür davranış değişmedi: View ▸ Theme ▸ aynı yazıyor.

## Checklist

- [x] Yazılış tabloları + `pub` aralıklar; ayrıştırıcı tablodan okuyor
- [x] `SettingsEdit` + `with_edit`; `with_theme` sarmalayıcı
- [x] `write_edit`; menü yolu ona geçti
- [x] `monospaced_families` + ihraç; ~~`CTFontManager` bayrağı gerekçeli~~ bayrak gerekmedi (Uygulama Notları)
- [x] Test: varyant başına round-trip, tablo ↔ ayrıştırıcı, aile listesi
- [x] `docs/YOL-HARITASI.md` → "Ayar ayrıştırmasının beş kopyası" borcu: `name()` ↔ kol eşleşmesinin kapandığı, kalan yarısı (kopya ayrıştırıcı gövdeleri) varsa o yazılır
- [x] Doğrulama geçti (`make hepsi`; `Cargo.lock` değişmedi)

## Uygulama Notları

- **Borcun iki yarısı da kapandı.** Elle yazılmış yedi ayrıştırıcı
  (`caret_shape`, `cursor_blink`, `cursor_motion`, `reduce_motion`,
  `smooth_scroll`, `shell_integration`, `osc52`) silindi, hepsi `named_enum` +
  `NAMES`: yardımcının kurduğu cümle onlarınkiyle zaten bayt bayt aynıydı
  (mevcut tanı sınamaları dokunulmadan yeşil). `named_enum`'un
  `fallback_name` parametresi düştü, "using …" de tablodan (`name_in`).
- **Aile listesi `CTFontManager` ile değil, CoreText'in eşleştirmesiyle.**
  Her aileyi açıp sormak bütün aileleri açmak demekti; yerine `TraitMonoSpace`
  bitli bir tanımlayıcının eşleşmeleri aday veriyor, son süzgeç yine
  `open` + `same_family` + `is_monospaced` (ölçüt tek yerde). Bu makinede iki
  yolun listesi aynı çıktı (5 aile). Gereken feature'lar
  (`CTFontDescriptor`'ın CF'leri) zaten açık, yani `Cargo.toml`'a dokunulmadı.
  Not: bu ortamda sınama sürecindeki **ilk** CoreText çağrısı ~25 s sürüyor
  (mevcut `bt-atlas` sınamaları da öyle; aynı çağrı yerel bir Swift
  ikilisinde anında) — bu yolun maliyeti değil.
- **Tanının "must be …" kısmı değerin türünden:** bölüm olan bir anahtara
  yazma reddi `a string` / `an integer` / `a number` diyor; tema için metin
  eskisiyle aynı.
- **`write_edit`'in iletisi neyin kaydedilmediğini adlandırıyor:** `Theme`
  için "the theme", öteki anahtarlar için "the setting" — menünün iletisi
  aynı kaldı, sarmalayıcı gerekmedi.
- `FontSize` her zaman ondalık yazılıyor (`size = 14.5`, `13.0`); şablondaki
  `size = 13` tamsayısı okunurken aynı değer.
