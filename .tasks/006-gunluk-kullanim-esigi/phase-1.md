# Phase 1 — Seçim: model, fare çevirisi, vurgu

## Özet

Fareyle metin seçilir ve seçili aralık ters video ile boyanır.

_Requirements: R1, R1.1, R1.2, R1.3, R6.1, R6.2_

---

## 1. Seçim modeli `bt-core`'da

`crates/bt-core/src/session.rs`

Seçim "hangi hücreler" sorusudur ve grid'i gören tek yer `bt-core`. Model
iki uç nokta + aktif uç; metin çıkarımı satır sarma ve geniş karakter
spacer'larını (`SPACERS`, `frame()`'de tanımlı) atlayarak yapar.

```rust
/// Seçim aralığı — grid hücresi cinsinden, iki uç dahil.
/// `bt-core`'da yaşar çünkü "hangi hücreler" grid bilgisidir.
/// Fare pikselini buraya indiren `bt-shell`'dir (R1.1).
```

Metin çıkarımı için tek kaynak `selection_text()`: Cmd-C de (phase-2), test
de aynı fonksiyonu çağırır. İki ayrı metin yolu, iki ayrı sarma hatası demek
olur.

## 2. Değişim kirli bayrağını diker

Seçim PTY içeriği değildir, `Wakeup` üretmez. Kimse `DirtyFlag::mark()`'ı
çağırmazsa (`session.rs:182-184`) seçim hiç boyanmaz — sessiz kusur.
Değişimde `mark()` + link uyandırma (codebase-fit 2'nin bulgusu; plan
yazmıyordu, checklist'e eklendi).

`DirtyFlag` `dirty_flag()` ile zaten dışarıda (`session.rs:803-804`);
link uyandırma yolunu `send_event`'in `Wakeup` kolu gösteriyor
(`session.rs:415`, `session.rs:378` — bayrağı `Event::Wakeup` diker).

## 3. Fare→hücre çevirisi + vurgu

`crates/bt-shell/src/view.rs` — `mouseDown:`/`mouseDragged:` işleyicileri,
pikseli `cell_metrics` ölçüsüyle (`renderer.rs:253`) hücreye indirir.
Vurgu `cell_bg` borusundan ters video ile; yeni shader/uniform yok (emsal:
imleç tersine çevirme, `session.rs:768-773`).

---

## Uygulama Notları

- **Yan semantiği (alacritty `Side`)**: `Selection::new` + `update` yanları
  hücrenin kendisine bakmalı (başlangıç `Left`, bitiş `Right`); tersi
  `range_simple`'da iki ucu da birer hücre içten kırpar. İlk hâlde
  `(0,0)-(4,0)` seçimi `"hello"` yerine `"ell"` verdi.
- **Vurguda `contains` → `contains_cell`**: seçim tam spacer hücresinden
  başlarsa geniş karakterin baş hücresi `contains` ile vurgulanmıyor; metin
  yolu doğruyken highlight pikseli eksik kalıyordu. Şekil `frame()` başında
  bir kez okunuyor (`cursor_shape`), imleç hücresindeki blok-imleç istisnası
  korunuyor.
- **Gizli metin seçimde vurgulanmaz** (`/code-review` bulgusu): `HIDDEN`
  hücrede `selected` kapısı `!hidden` ile koşullanıyor, yoksa gizli hücrenin
  yeri boyalı blok olarak görünürdü. Metin yolu ayrı — phase-2 gizli metni
  `selection_text()`'ten okuyabilir.
- **Eşitlik kapısı** (`/code-review` bulgusu): `set_selection` aynı aralığı
  tekrar alınca sessiz dönüyor (`Selection: PartialEq`); aynı hücrede kalan
  `mouseDragged` yağmuru kare istemiyor.
  > **Aşıldı (phase-2 sonrası, seçim yarısı düzeltmesi):** kapı artık uçları
  > değil **ekranda çizilen aralığı** karşılaştırıyor (`visible_range`) —
  > yarılı uçlarda aynı aralık farklı uç çiftlerinden doğabiliyor.
- **`buttonNumber` + `mouseUp:`** (`/code-review` bulguları): sağ/orta tık
  seçim başlatmıyor; bırakış çapayı düşürüyor. Tek tık kalıcı tek-hücrelik
  seçim bırakır — phase-2'de Cmd-C onu kopyalar, temizleme davranışı (tıklayınca
  seçim kalksın mı) phase-2'nin kararı.
  > **Aşıldı (phase-2 sonrası, seçim yarısı düzeltmesi):** tek tık artık **boş**
  > seçimdir. Uçlar yarısını taşıyınca sürüklemesiz tıkın iki ucu birebir eşit
  > olur ve alacritty onu boş sayar (`is_empty`); kopyalanacak metin yok.
  > Ayrıntı → `phase-2.md` → `## Uygulama Notları` → "Kullanıcı bildirimi".
- **Çapa viewport cinsinden** (`/code-review` bulgusu, phase-3'e devir):
  basılı-sürükleme sırasında kaydırma olursa çapa bayat kalır; aralık grid
  mutlağında tutulduğu için içerikle taşınıyor ama view'daki çapa taşınmıyor.
  Kaydırma tetikleyicisi bu fazda yok (tekerlek phase-3), yani bugün
  ulaşılamaz — tetikleyiciyle birlikte çözülecek.
- **`point_to_cell` kenar asimetrisi** (iki inceleme de buldu): sol/üst
  dışarısı 0'a kırpılır (`f64 as u16` doygun), sağ/alt dışarısı yutulur
  (`None`). Doc'ta yazıyor; kırpma yönü `to_range`'ın kırpmasıyla aynı.
  > **Aşıldı (phase-2 sonrası, seçim yarısı düzeltmesi):** sağ/alt dışarısı
  > artık yutulmuyor, son hücreye yapışıyor (sağda sağ yarı). Yarı seçimi
  > belirleyince yutmak satır sonunda son harfi kaybettiriyordu.
- **Vurgu test çapası dersi**: `wait_cells` bg sayıyor, kare mürekkebi de
  taşıyor — `h…o` reçetesinde üçlük bg çapası `hello` mürekkepli karede erken
  dönüp seçim aralığını kaydırıyordu. Çapa mürekkep dizesine (`"hello"`) +
  bg sayısına bağlandı.
- **`/simplify`**: ölü if/else kolu (iki kol da aynı side çifti), `cell_under`/
  `drag_cells` ortak `session_cell` yardımcısı, test `scene()` sahnesi,
  `set_metrics(Grid)` bütün-argüman, `clear_selection` kilit-erken-bırakma.
- **`/audit`**: 1 (katman — `cargo tree` + kaynak grep temiz), 2 (bağımlılık
  yok), 3 (üretim kodunda unwrap/expect/panik yok — yalnız `#[cfg(test)]`),
  6 (ölçüm sayısı yok), 7 (kilit sırası + Retained döngüsüzlüğü), 8 (eşitlik
  kapısı + sessiz temizleme + `set_metrics` uyandırmıyor), 10 (tanımlayıcılar
  İngilizce, `#[allow]` gerekçeli) temiz; 4 (ayar), 5 (shell), 9 (hücre/shader)
  ilgisiz.

- **sadakat: makas yok.** `git show --stat 77b4afd` checklist ile eşleşiyor: phase-1.md (kılavuz + notlar), session.rs (model + vurgu), app.rs/lib.rs (kablo), view.rs (fare). `plan.md ## Durum` damga commit'inde (`f17ea69`).

## Yayın Etkisi

- Fare tıklaması artık seçim başlatır; tek tıkla odaklanma değişmez.
- Yeni bağımlılık yok. `.metal`, terminfo, ayar şeması, tema: el değmiyor.
- Ölçüm bekleyen iddia yok — seçim yolu kare başına bir aralık testi,
  `/measure`'lık büyüklük değil.

---

## Checklist

- [x] Seçim modeli `bt-core`'da + `selection_text()` tek metin yolu
- [x] Değişimde `DirtyFlag::mark()` + link uyandırma
- [x] `view.rs`'de `mouseDown:`/`mouseDragged:` + fare→hücre çevirisi
- [x] Vurgu `cell_bg` borusundan; yeni shader/uniform yok
- [x] Test: seçim aralığı metin çıkarımı (sarma + spacer atlama)
- [x] Test: seçim değişimi kirli bayrağını dikiyor
- [ ] `[elle]` göz kontrolü: fareyle seç, ters video vurguyu gör
- [x] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu** — pencere davranışı değişti)
- [x] `/simplify` çalıştırıldı, bulgular uygulandı
- [x] `/code-review` çalıştırıldı, bulgular giderildi
- [x] `/audit` çalıştırıldı, bulgular giderildi
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: 77b4afd
