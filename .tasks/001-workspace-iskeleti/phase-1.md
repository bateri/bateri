# Phase 1 — Workspace ve Makefile

## Özet

Beş crate'lik workspace, dış bağımlılığı olmayan iskelet ve `make hepsi`;
Xcode olmayan makinede de yeşil. Belgeler kodla aynı commit'te düzeltilir.

_Requirements: R2, R2.1, R2.2, R3, R8_

---

## 1. Workspace

`Cargo.toml`

```toml
[workspace]
resolver = "3"
members = [
    "crates/bt-core",
    "crates/bt-atlas",
    "crates/bt-gpu",
    "crates/bt-shell",
    "crates/bateri",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.88"
license = "MIT"

# Dış crate sürümleri tek yerden; üyeler `{ workspace = true }` der.
# Bu phase'de hiçbiri kullanılmaz — phase-2 ve phase-3 ekler.
[workspace.dependencies]
bt-core  = { path = "crates/bt-core" }
bt-atlas = { path = "crates/bt-atlas" }
bt-gpu   = { path = "crates/bt-gpu" }
bt-shell = { path = "crates/bt-shell" }
```

`workspace.dependencies`'e dış crate'ler bu phase'de **girmez**: `Cargo.lock`
yalnız beş üyeyi taşır ve phase-2/3'ün kilit farkı tek tek okunabilir.

## 2. Crate'ler

Her crate'in `Cargo.toml`'u `[package] name, version.workspace, edition.workspace,
rust-version.workspace, license.workspace` taşır. Bağımlılıklar yalnız
katman yönünü kurar:

| crate | `[dependencies]` |
|---|---|
| `bt-core` | — |
| `bt-atlas` | — |
| `bt-gpu` | `bt-core`, `bt-atlas` |
| `bt-shell` | `bt-gpu` |
| `bateri` | `bt-shell` |

`crates/bt-core/src/lib.rs`

```rust
//! bt-core — terminal modelinin platformsuz çekirdeği.
//!
//! Buraya VT durum makinesi, grid ve scrollback, PTY, OSC ayrıştırma, komut
//! blokları, seçim ve ayar modeli gelir (002+). Sözleşme: bu crate hiçbir
//! platform kütüphanesi görmez — `objc2*`, `core-text`, `metal` yok — ve
//! Linux'ta derlenebilir kalır; Vulkan kapısı bu ayrımın üstüne kurulur.
//! Denetim: `cargo tree -p bt-core -e normal | grep objc2` boş dönmeli
//! (`.claude/skills/audit` mercek 1). Bu yalnız bağımlılık düzeyinde bir
//! vekildir; gerçek kapı `--target x86_64-unknown-linux-gnu` ile derlemedir
//! ve `rustup` gelene kadar kapalıdır.
```

`crates/bt-atlas/src/lib.rs`

```rust
//! bt-atlas — glyph rasterizasyonu ve atlas paketleme.
//!
//! CoreText ile rasterizasyon, atlas paketleyici, kutu çizim karakterleri ve
//! font seti burada yaşar (003+). Sözleşme: yalnız `core-text` ve
//! `core-graphics` görür; AppKit ve Metal görmez.
```

`crates/bt-gpu/src/lib.rs` ve `crates/bt-shell/src/lib.rs`: aynı biçimde
başlık yorumu (sırasıyla "Metal renderer, shader'lar, hareket, overlay'ler" ve
"AppKit kabuğu: pencere, sekme, bölme, menü, klavye"); gövde phase-2 ve
phase-3'te dolar. **Sahte sınama yok** — `cargo build` derlendiğini kanıtlar.

`crates/bateri/src/main.rs`

```rust
//! bateri — uygulama girişi. Kabuk phase-3'te `bt_shell::run` ile bağlanır.

fn main() {
    println!("bateri {}", env!("CARGO_PKG_VERSION"));
}
```

## 3. Makefile

`Makefile`

```make
CARGO ?= cargo

.PHONY: hepsi test terminfo test-yaris kur

# Definition of done. rustc sürümü başta basılır: Homebrew rustc pin'li değil,
# bir `brew upgrade` sonrası gelen clippy kırmızısını kod kırmızısından ayırır.
hepsi:
	@rustc --version
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	$(CARGO) test --workspace

test:
	$(CARGO) test --workspace

# Girdisi henüz olmayan hedefler. Var olurlar ki `proje.md` sözleşmesi yalan
# söylemesin; koşarlarsa "koşamadı" derler, "geçti" değil (çıkış 78 = EX_CONFIG).
terminfo:
	@echo "henüz yok: assets/terminfo bir shell/TERM setiyle gelir"; exit 78
test-yaris:
	@echo "henüz yok: ThreadSanitizer nightly ister ve paylaşılan durum PTY setiyle gelir"; exit 78
kur:
	@echo "henüz yok: .app paketi bundle setiyle gelir"; exit 78
```

`shader` phase-2'de, `duman` phase-3'te eklenir — her hedef onu gerçek yapan
phase'le gelir.

`rustfmt.toml` yok; varsayılan biçim. `clippy.toml` yok.

## 4. Belge düzeltmeleri (R8)

Kodla **aynı commit'te**:

| dosya | değişiklik |
|---|---|
| `CLAUDE.md` → katman tablosu, `bt-shell` satırı | `objc2-app-kit` → `objc2-app-kit`, `objc2-quartz-core` (yalnız `CALayer` takma) |
| `CLAUDE.md` → Bilinmesi gerekenler, hücre maddesi | "hedefimiz 16, **001** ölçer ve sabitler" → "**002** ölçer ve sabitler" |
| `.claude/is-akisi/proje.md` → tuzaklar, hücre maddesi | "(hedef; 001 ölçüp sabitler)" → "(hedef; 002 ölçüp sabitler)" |
| `.claude/is-akisi/proje.md` → doğrulama tablosu, `make shader` satırı | gerekçe "hata shader'da kalır, cargo test onu görmez" → "`build.rs` shader hatasını `cargo build`'de zaten yakalar; bu hedef cargo'nun bayatlık takibini atlayan kanaryadır (`touch` + `cargo build -p bt-gpu`)" |
| `.claude/is-akisi/proje.md` → doğrulama tablosu | `terminfo`, `test-yaris`, `duman` satırlarına "(hedef girdisi gelene kadar `exit 78` ile 'henüz yok' der)" notu; `duman` phase-3'te gerçek olur |

---

## Uygulama Notları

- **`exit 78` make'ten geçmez.** GNU make reçete hatasını her zaman 2 ile
  döndürür; 78 yalnız "Error 78" satırında görünürdü. Ayırt edici sinyal
  stdout'taki "henüz yok" metni oldu, stub'lar `exit 1` ile düşer.
  `proje.md`, `CLAUDE.md` ve phase-3'ün `duman` notu buna göre yazıldı
  (`/simplify` altitude + `/code-review`).
- **`shader` ve `duman` stub'ları bu phase'de eklendi**, kılavuz "phase-2/3'te
  gelir" diyordu: `proje.md` tablosu beş hedef adı taşıyor, ikisi eksik
  kalınca `No rule to make target` ile tablo yalan söylüyordu. Phase-2 ve
  phase-3 dosyaları "stub'ı yerinde değiştir + `proje.md` listesinden çıkar"
  adımıyla güncellendi.
- `hepsi` alt hedeflere bölündü (`fmt`, `clippy`, `test`; `.NOTPARALLEL`),
  `surum` hedefi ve `RUSTC` değişkeni kılavuzdan sonra eklenip `/code-review`
  ile geri alındı; sürüm satırı `hepsi` reçetesinde. `settings.json` iznine
  `make fmt`/`make clippy` eklendi.
- `proje.md`'deki üç satır içi "henüz yok" notu tek bir başlık alıntısına
  toplandı; tablo satırları ona atıf yapıyor. Homebrew rustc kayması
  tuzaklara girdi.
- `bt-core`/`bt-atlas` `Cargo.toml`'larında boş `[dependencies]` yok.
- `/audit` mercek 1'in kaynak grep'i yorum satırlarını dışlayacak şekilde
  düzeltildi (yanlış pozitif bu phase'de çıktı).
- Doğrulama: `make hepsi` → 0 (rustc 1.88.0), `make -j4 hepsi` → sürüm başta;
  `cargo tree` beş crate'te yön doğru; stub'lar "henüz yok" + sıfır dışı.
- sadakat: makas yok.

## Yayın Etkisi

- `CLAUDE.md` ve `proje.md` düzeltmeleri bu commit'te (yukarıdaki tablo).
- Yeni bağımlılık: yok (`Cargo.lock` yalnız beş üye).
- Ölçüm bekleyen iddia: yok.

---

## Checklist

- [x] `Cargo.toml` workspace, beş crate, katman yönü tablodaki gibi
- [x] `Makefile`: `hepsi` (`fmt`/`clippy`/`test`); beş stub "henüz yok" + `exit 1` (78 iddiası düştü, bkz. notlar)
- [x] Belge düzeltmeleri (R8) aynı commit'te (+ `CLAUDE.md` komut bloğu, tuzak maddesi)
- [x] Test: `cargo tree -p bt-core -e normal` ve `-p bt-atlas` yalnız kendilerini listeler; `cargo tree -p bt-gpu` `bt-shell` içermez
- [x] Test: `make terminfo` stdout'a "henüz yok" basar ve sıfır dışı çıkar (make reçete hatasını 2 ile döndürür; 78 iddiası düşürüldü)
- [x] Doğrulama geçti (`make hepsi`, kapı sonrası yeniden koşuldu)
- [x] `/simplify` çalıştırıldı, bulgular uygulandı (4 mercek; efficiency temiz)
- [x] `/code-review` çalıştırıldı, 7 bulgu giderildi
- [x] `/audit` çalıştırıldı — mercek 1, 2, 6, 10 temiz; 3, 4, 5, 7, 8, 9 ilgisiz
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: 3e006ba
