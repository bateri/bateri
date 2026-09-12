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

## Yayın Etkisi

- Fare tıklaması artık seçim başlatır; tek tıkla odaklanma değişmez.
- Yeni bağımlılık yok. `.metal`, terminfo, ayar şeması, tema: el değmiyor.
- Ölçüm bekleyen iddia yok — seçim yolu kare başına bir aralık testi,
  `/measure`'lık büyüklük değil.

---

## Checklist

- [ ] Seçim modeli `bt-core`'da + `selection_text()` tek metin yolu
- [ ] Değişimde `DirtyFlag::mark()` + link uyandırma
- [ ] `view.rs`'de `mouseDown:`/`mouseDragged:` + fare→hücre çevirisi
- [ ] Vurgu `cell_bg` borusundan; yeni shader/uniform yok
- [ ] Test: seçim aralığı metin çıkarımı (sarma + spacer atlama)
- [ ] Test: seçim değişimi kirli bayrağını dikiyor
- [ ] `[elle]` göz kontrolü: fareyle seç, ters video vurguyu gör
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu** — pencere davranışı değişti)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
