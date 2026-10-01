# Phase 6 — OSC 8 hover'ı, hedef etiketi, sağ tık menüsü

## Özet

⌘'siz OSC 8 hover'ında kesikli alt çizgi, ⌘ ile OSC 8 üstündeyken hedefin
sol alttaki etiketi ve bağlantı üstünde sağ tık menüsü.

_Requirements: R9_

## Değişiklikler

- **`crates/bt-shell-macos/src/view.rs`** — ⌘ basılı değilken hareket
  hücre değiştirdiğinde yalnız OSC 8 hit'i `Dashed` stille kurulur (düz
  metin bağlantı ⌘'siz vurgulanmaz); el imleci yalnız ⌘'de. Sağ tık (fare
  kipi kapalıyken, bağlantı üstünde): `NSMenu` — URL'de "Open Link" / "Copy
  Link", yolda "Open" / "Reveal in Finder" / "Copy Path"; eylemler phase-4'ün
  açma yolu ve politika tablosu (menü politikayı atlamaz: onay isteyen şema
  menüden de sorar). Fare kipinde sağ tık bugünkü gibi uygulamanın;
  bağlantısız yerde değişiklik yok.
- **`crates/bt-shell-macos/src/pane.rs`** — hedef etiketi: pane'in sol
  altında küçük bir AppKit etiketi (`hitTest → nil`, odaksız örtünün
  emsali), yalnız ⌘ + OSC 8 hover'ında, hedefin tamamı (uzunsa ortadan
  `…`); hover kalkınca gizlenir. Kare yolunun dışında.
- **`CLAUDE.md`** — bağlantı paragrafına üç cümle.

## Kabul

- Gözle kontrol: `printf '\e]8;;https://example.com\e\\tıkla\e]8;;\e\\\n'` →
  ⌘'siz üstüne gel kesikli alt çizgi; ⌘ ile düz alt çizgi + el + sol altta
  `https://example.com`; sağ tık → Open Link / Copy Link; `ls` çıktısında
  sağ tık → Reveal in Finder. Izgara, bant ve dock'ta.
- `make check` + `make smoke` yeşil.

## Checklist

- [ ] ⌘'siz OSC 8 kesikli hover
- [ ] Hedef etiketi
- [ ] Sağ tık menüsü (politikadan geçerek)
- [ ] `CLAUDE.md` güncellendi
- [ ] Doğrulama geçti (`make check` + `make smoke`)
