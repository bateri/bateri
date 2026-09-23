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

- [x] `settings.rs` enum'ları, alanlar, ayrıştırma, düzenleme, `changes`, şablon
- [x] `set_glyph_fx` ve `bt-shell` bağlantısı
- [x] İki popup + devre dışı kuralı
- [x] `docs/AYARLAR.md`
- [x] Test: ayrıştırma, round-trip, `changes`, bilinmeyen anahtar tanığı
- [x] Doğrulama geçti (`make hepsi` + `make duman`)
- [x] Gözle (geçici `.app`, geçici HOME): Motion bölmesinde iki popup
  029'un düzeninde; `snap`'te Keypress/Erase/Smooth scrolling devre dışı
  ve nedenli, Hareketi Azalt açıkken Erase ve Smooth scrolling devre dışı,
  Keypress açık ve "only fade in" notlu, `off` seçilince not açıklamaya
  dönüyor; seçimler dosyaya yazılıyor. Dock'taki 120 ms'lik efekt ekran
  görüntüsüyle yakalanamadı — kullanıcıya: popup'tan Keypress `Off` seçince
  dock'ta harfler anında geliyor.

## Uygulama Notları

- **Tek enum:** `bt-gpu`'nun `KeypressFx`/`EraseFx`'i kalktı; `bt-core`'un
  `Keypress`/`Erase`'i doğrudan kullanılıyor (`CursorMotion` emsali), shader
  kimliği `glyph_fx::Effect` trait'inde (kapsamlı `match`), sınamaların
  efekt listesi `NAMES`'ten türüyor — yeni ad iki yerde değil bir yerde
  giriyor.
- **Hareketi Azalt'ta Keypress satırı devre dışı değil:** Azalt yazmayı
  belirmeye indiriyor ama `off` ile efekt arasındaki seçim orada da fark;
  satırı kilitlemek kullanıcıyı açıp kapatamaz bırakırdı. Satır açık,
  notu "Letters only fade in…" (`off`'ta not yok). Erase tamamen ezildiği
  için devre dışı.
- **Smooth scrolling satırı da aynı kurala girdi** (phase dışı, küçük):
  `snap`/Azalt onu da eziyor (`resolve_smooth_scroll`) ve komşu satırlar
  kilitlenirken onun açık kalması tutarsız görünürdü. Kural tek yerde:
  `settings_window::motion_override` (saf, sınamalı).
- `SettingsWindow::refresh` Hareketi Azalt'ın **çözülmüş** değerini alıyor;
  sistem bildirimi (`accessibilityDisplayDidChange:`) artık açık ayar
  penceresini de tazeliyor.
- `GlyphFx::set_effects` değişimde uçuştakileri bitiriyor ve bir şey
  bittiyse `set_glyph_fx` kare istiyor (`set_cursor_motion` emsali).
- Bilinmeyen anahtar tanığı `keypress = "pop"` → `speed = "brisk"`.
