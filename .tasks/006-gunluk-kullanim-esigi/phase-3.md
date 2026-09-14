# Phase 3 — Kaydırma: tekerlek + Shift+PgUp

## Özet

Tekerlek ve Shift+PgUp geçmişe kaydırır; alternate screen'de tekerlek yoksayılır.

_Requirements: R3, R3.1, R3.2, R3.3, R6.1, R6.2_

---

## 1. İnce `scroll_display` API'si

`crates/bt-core/src/session.rs` — viewport kayması icat edilmiyor:
`display_offset` zaten `frame()`'de tüketiliyor (`session.rs:603-617`,
`offset` hesabı ve imleç görünürlüğü orada). Eksik olan yalnız onu hareket
ettiren ince bir API + kaydırınca kirli bayrağını dikmek.

```rust
/// Görünen pencereyi kaydırır; artı değer geriye. Kaydırınca kirli bayrağı
/// dikilir, yoksa kaydırma hiç boyanmaz (seçimdeki R1.2 ile aynı tuzak).
```

## 2. İki tetikleyici, çubuk yok

`crates/bt-shell/src/view.rs` — `scrollWheel:` + Shift+PgUp (tuş yolu
`encode_key` üzerinden, AppKit fonksiyon tuşu `F1/Home/PageUp…` aralığında
yutuluyor — `keys.rs`'te PgUp dizisi tanımlı olmalı, yoksa eklenir).
Kaydırma çubuğu **yok**: AppKit kroniği (thumb, orantı, sürükleme), eşik için
gerekli değil.

## 3. Alternate screen'de yoksayma — karar `bt-core`'da

Tekerlek vim/less/tmux'ta uygulamaya fare dizisi göndermelidir — ama fare
raporlaması (SGR-pixel) ayrı bir iş ve bu sette yok. O yüzden bu sette
tekerlek alternate screen'de **yoksayılır**. Karar `bt-core`'da `Term`
kipine bakılarak verilir (katman korunur); `view.rs` körü körüne
kaydırmaz.

---

## Uygulama Notları

## Yayın Etkisi

- Tekerlek ve Shift+PgUp artık viewport'u kaydırır; alternate screen'de
  tekerlek susar.
- Yeni bağımlılık yok. Ölçüm bekleyen iddia yok.

---

## Checklist

- [ ] **(phase-1'den devir)** Sürükleme **basılıyken** kaydırma olursa view'daki
      çapa bayat kalır: `set_selection` aralığı grid mutlağında tutuyor ve
      alacritty döndürmesi onu içerikle taşıyor, ama `ViewIvars::anchor`
      viewport cinsinden. Kaydırma tetikleyicisi phase-1'de yoktu, bu phase'de
      geliyor — göz kontrolü bu hâli de kapsamalı (`view.rs` → `mouseUp:`
      doc'unda kayıtlı)
- [ ] `scroll_display` API'si + kaydırınca kirli bayrağı
- [ ] **Devir (seçim yarısı düzeltmesinin `/audit`'i):** kirli bayrağı **elle**
      dikilmeli — alacritty'nin `Term::scroll_display`'i yalnız
      `Event::MouseCursorDirty` gönderiyor ve bizim `Adapter` onu yutuyor;
      bayrak dikilmezse kaydırma hiç çizilmez. Seçim kapısı (`visible_range`)
      iki tarafı aynı `display_offset`'le hesaplıyor, yani kaydırmanın kendi
      karesini kendisi istemesi yeterli; kapıya dokunmak gerekmiyor
- [ ] `scrollWheel:` ve Shift+PgUp tetikleyicileri; çubuk yok
- [ ] Alternate screen'de yoksayma `bt-core` kipiyle
- [ ] Test: kaydırma `display_offset`'i oynatıyor + kirli dikiliyor
- [ ] Test: alternate screen'de tekerlek yoksayılıyor
- [ ] `[elle]` göz kontrolü: tekerlekle geçmişe git, alternate screen'de sus
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
