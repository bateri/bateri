# Phase 3 — `[motion] keypress` / `erase` ve ayar penceresi

## Özet

İki anahtar dosyadan ve Motion bölmesindeki iki popup'tan seçilebilir, kayıt
anında uygulanır; adlar bu phase'de yalnız çizilebilenler (`off`/`fade`,
`off`/`recede`).

_Requirements: R7_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — `Keypress` ve `Erase` enum'ları
  (`NAMES` yalnız çizilebilen adlar, `name()` aynı tablodan), `Settings`
  alanları, varsayılan `Fade` / `Recede`, `[motion]` ayrıştırması
  (`named_enum`), `SettingsEdit` kolları, `Changes::motion`'a iki terim,
  `TEMPLATE`'e iki satır yorumuyla. Modül başlığındaki `[motion] keypress`
  örneği ve `unknown_keys_and_sections_are_silent`'in tanığı başka bir sahte
  anahtara taşınır (`intensity` zaten orada).
- **`crates/bt-gpu/src/link.rs`** — `DisplayLink::set_glyph_fx(keypress,
  erase)` (`set_cursor_motion` emsali: ham adlar, indirgeme `GlyphFx`'te;
  değişince uçuştakiler biter).
- **`crates/bt-shell/src/window.rs`**, **`crates/bt-shell/src/app.rs`** —
  pencere doğarken ve `Changes::motion`'da `set_glyph_fx`.
- **`crates/bt-shell/src/settings_window.rs`** — Motion bölmesinde "Keypress:"
  ve "Erase:" popup'ları (`Choice` uygulamaları, tek kaynak `NAMES`);
  `cursor_motion = "snap"` ya da etkin Hareketi Azalt onları ezdiğinde satır
  devre dışı ve açıklamalı (029 Karar 6), Hareketi Azalt'ta geliş satırının
  açıklaması "fade" olduğunu söyler.
- **`docs/AYARLAR.md`** — `[motion]` tablosuna iki satır, şablon bloğu, `snap`
  ve Hareketi Azalt paragraflarına iki efektin akıbeti.

## Kabul

- Ayrıştırma ve round-trip sınamaları: iki anahtarın her adı, kabul edilmeyen
  değer tanı + varsayılan, bilinmeyen anahtar korunur, yazma yalnız o satırı
  değiştirir; `changes` iki anahtarda `motion` diyor.
- Bilinmeyen anahtar sınaması hâlâ bilinmeyen bir anahtarı sınıyor.
- Gözle: popup'tan `off` seçince dock'ta harfler anında; `snap`'e geçince iki
  satır devre dışı.

## Checklist

- [ ] `settings.rs` enum'ları, alanlar, ayrıştırma, düzenleme, `changes`, şablon
- [ ] `set_glyph_fx` ve `bt-shell` bağlantısı
- [ ] İki popup + devre dışı kuralı
- [ ] `docs/AYARLAR.md`
- [ ] Test: ayrıştırma, round-trip, `changes`, bilinmeyen anahtar tanığı
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
