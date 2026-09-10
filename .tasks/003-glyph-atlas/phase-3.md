# Phase 3 — Metrik geçişi: `CELL_PX` ölür

## Özet

`bt-gpu` `bt-atlas`'ı görür ve metriği yeniden yayınlar; `bt-shell`'in
`CELL_PX` yer tutucusu silinir ve grid ölçüsü gerçek font metriğinden türer.
Glyph hâlâ yok — değişen yalnız hücrelerin boyu.

_Requirements: R5, R7 (kısmi: `CELL_PX` yorumu)_

---

## 1. `bt-gpu` metriği yeniden yayınlar

`crates/bt-gpu/src/renderer.rs`

Karar 2(a): `bt-shell` `bt-atlas`'ı **görmez**, metrik `bt-gpu` üzerinden
geçer ve `CLAUDE.md`'nin katman tablosu değişmez.

```rust
/// Hücre ölçüsü; `bt-shell` grid boyutunu ve `TIOCSWINSZ`'i bundan türetir.
///
/// `scale` parametre çünkü ekran ölçeği değişebilir
/// (`windowDidChangeBackingProperties:`) ve atlas önbelleği ölçeği anahtarının
/// parçası olarak taşır: aynı `Renderer` iki ölçekte iki farklı metrik verir.
pub fn cell_metrics(&self, scale: f64) -> CellMetrics
```

`Renderer` atlası ölçek başına önbellekler (`RefCell<HashMap<..>>` ya da son
ölçek için tek girdi — uygulamada karar, gerekçe `## Uygulama Notları`'na).
`CellMetrics` `bt-gpu`'nun `pub` tipidir ve `bt_atlas::Metrics`'i yeniden ihraç
etmez: `bt-shell`'in `bt-atlas` tipini görmesi katman tablosunu bulanıklaştırır.

---

## 2. `CELL_PX` silinir

`crates/bt-shell/src/app.rs`

```rust
// SİLİNİR:
// const CELL_PX: (f64, f64) = (9.0, 18.0);
```

`metrics()` bugün `CELL_PX.0 * scale` ile hesaplıyor; artık
`renderer.cell_metrics(scale)` çağırır. `scale` zaten elde
(`window.backingScaleFactor()`), yani çarpma **atlas tarafına** taşınır —
metriğin ölçekle ilişkisi tek yerde kalır.

Hücreler boy değiştirir; `hucre=8` durur (900×600 penceresine sekiz hücre her
makul metrikte sığar) ama `make duman` yine de koşturulur.

---

## Phase-2'den devir

Phase-2'nin kalite kapısı (reuse merceği) bu phase'e ait üç bulgu üretti;
uygulanmadılar çünkü hepsi `CELL_PX`'in ölmesiyle aynı anda çözülüyor.

**1. İki yuvarlama kuralı yan yana duruyor ve biri silinmezse kalıcı olur.**
`bt-atlas` hücre ölçüsünü `ceil()` ile yukarı yuvarlıyor (`font::yukari`),
`app.rs:38` ise `CELL_PX.0 * scale` sonucunu `.round()` ile yuvarlıyor.
`cell_metrics(scale)` bağlandığında `app.rs`'teki yuvarlama bloğu **silinmeli**
— kalırsa depoda iki kural olur, hangisinin kazandığı çağrı sırasına bağlanır
ve belirti bir piksellik hücre kayması, yani sessiz.

**2. İki `Metrics` tipi aynı dosyada buluşacak.** `app.rs`'in `Metrics`'i
aslında "grid ölçüsü + hücre" (`cols`, `rows`, `cell_px`); `bt_atlas::Metrics`
ise yalnız hücre. `cell_metrics` geldiğinde ikisi yan yana okunacak, biri
yeniden adlandırılmalı — `app.rs` tarafı için `GridMetrics`/`Geometry`.
`CellMetrics`'in `bt_atlas::Metrics`'i yeniden ihraç **etmemesi** kararı
(yukarıda, 1. bölüm) bu yüzden ayrıca değerli: ad çakışması katmanı da
bulanıklaştırırdı.

**3. `cell_px: (u16, u16)` adsız demeti dördüncü kez dolaşıyor**
(`bt_core::SessionOptions.cell_px`, `bt_gpu::DisplayLink::resize`,
`bt_gpu::Frame::clear`, `bt-shell::app::Metrics`). Katman sözleşmesi ortak bir
tipe izin vermiyor (`bt-atlas` `bt-core`'u göremez) ama `bt-gpu` ikisini birden
görüyor: `cell_metrics` bu demetin **tek geçiş noktası** olsun, beşincisi elle
kurulmasın.

**Ayrıca — yukarıdaki 1. bölümün "ölçek önbelleği" tasarımı değişti.**
Phase-2 `Atlas::yenile(punto, scale) -> bool` ile çıktı: anahtar değiştiyse
atlası yeniden kurar ve `true` döner, `true` aynı zamanda "dokuyu yeniden
ayır" demektir. Yani `RefCell<HashMap<ölçek, Atlas>>` **gerekmiyor**; tek bir
atlas + `yenile` çağrısı yeter ve ölçek başına ikinci bir atlas taşımaz.
Karar gerekçesiyle `## Uygulama Notları`'na yazılır.

---

## Uygulama Notları

## Yayın Etkisi

---

## Checklist

- [ ] `bt-gpu` → `bt-atlas` bağımlılık kenarı (`Cargo.toml`)
- [ ] `Renderer::cell_metrics(scale)` + `CellMetrics`, ölçek önbelleği
- [ ] `bt-shell`: `CELL_PX` silindi, `metrics()` renderer'dan okuyor
- [ ] Test: iki ölçek iki metrik verir; `bt-shell` yer tutucuyu artık taşımıyor
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make hepsi` + `make duman`)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
