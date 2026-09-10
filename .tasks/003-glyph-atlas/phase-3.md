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
