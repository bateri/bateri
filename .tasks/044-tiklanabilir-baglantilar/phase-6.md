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

- [x] ⌘'siz OSC 8 kesikli hover
- [x] Hedef etiketi
- [x] Sağ tık menüsü (politikadan geçerek)
- [x] `CLAUDE.md` güncellendi
- [x] Doğrulama geçti (`make check` + `make smoke`)

## Uygulama Notları

- **SAPMA — kod `view.rs`'te değil `hyperlink.rs`'te.** phase-4'ün emsali:
  `view.rs` yalnız kancaları taşıyor (sağ basışta `link_menu`, menünün üç
  seçicisi, her basışta `forget_link_menu`). Çentik artık **(hücre, ⌘)**:
  ⌘'ye basmak/bırakmak aynı hücrede yeniden soruyor, Shift/Option'un
  `flagsChanged:`'i sormuyor. Stil tek fonksiyondan (`hover_style`): ⌘'de her
  bağlantı `Single`, ⌘'siz yalnız OSC 8 `Dashed`; ⌘'nin bırakılması artık
  topyekûn temizlik değil, aynı noktayı ⌘'siz yeniden soruyor (OSC 8 kesikliye
  iner, düz metin kalkar). phase-4'ün bilinen sınırı (4) korunuyor:
  bulunamayan aday ⌘ bırakılınca unutuluyor (`link_flags`).
- **⌘'siz hover'ın bedeli.** ⌘'siz hareket hücre değiştirince bir `link_at`
  (Term kilidi) — eskiden hiç yoktu; hücre başına bir kez (`motion_event`'in
  emsali). Akan çıktıda `link_hover_lost` artık pencere key'se ⌘'siz de
  yeniden buluyor: fare bir OSC 8 bağlantısının üstünde dururken phase-2'nin
  "çıktı başına iki kare"si ⌘'siz de geçerli — çıktıyla sınırlı, boşta sıfır.
- **Doğrulama dönüşte stilini o anki ⌘'den alıyor** (`link_verified`): ⌘
  arada bırakıldıysa OSC 8 `file://` kesikli çizilir, düz yol hiç çizilmez.
- **El imleci yalnız ⌘ vurgusunda** (`LinkState::command_hover`); kesikli
  hover bir ipucu, tık seçim.
- **Hedef etiketi** `pane::LinkLabel` (`NSBox`, `hitTest → nil`) + içinde
  `NSTextField` (`ByTruncatingMiddle`); temanın zemini ve ayraç tonu, metin
  `secondaryLabelColor` (görünüm temadan). Sol altta, `dim` örtüsünün altında.
  Bağlantı en alt satırda ve etiketin altındaysa etiket onu örtüyor (Ghostty
  da öyle) — adıyla bilinen sınır.
- **Sağ tık menüsünün kapsamı.** Izgarada rapor önce soruluyor: `Click::Select`
  (kip kapalı **ya da Shift**) → menü; Shift'li sağ tık fare kipinde de menü
  açıyor — Shift'in "terminalin kaçış yolu" kuralının okuması. Bant ve
  dock'ta fare kipi hiç uygulanmadığı için her zaman. Yol adayı arka planda
  doğrulanıyor (ana thread'de senkron `stat` yok, R7), menü dönüşte açılıyor;
  geç dönüşü bir sonraki basış ya da `clear_link` iptal ediyor
  (`menu_pending`). Menü `NSMenu::popUpMenuPositioningItem` ile, öğelerin
  hedefi view; "Open" tıkın `open_link`'i (onay sayfası ve `bateri://` yutma
  aynen), "Reveal in Finder" her zaman güvenli, "Copy Path" çözülmüş mutlak
  yol. Politikanın bir şey yapmadığı bağlantıya (`Swallow`) menü yok.
- **Dock'ta OSC 8 yok** (dock ZLE'nin `BUFFER`'ı): kesikli hover ve hedef
  etiketi orada yapısal olarak konusuz; sağ tık menüsü dock'un URL'sinde ve
  var olan yolunda çalışıyor.
- **Set kapısı `/code-review` — iki bulgu, ikisi de giderildi.** (1) Odak
  pencere key'liğini kaybetmeden view'dan giderse (⌘F'nin arama alanı, ⌘D /
  ⌘] ile başka pane) `flagsChanged:`/`mouseMoved:` artık başka yere gidiyor ve
  vurgu, el imleci, hedef etiketi asılı kalıyordu → `resignFirstResponder`
  `clear_link` çağırıyor. (2) `//`'siz `file:` URL'si (`file:/x.command`)
  `local_path`'ten geçmiyor, `Confirm` ile `NSWorkspace`'e URL olarak gidip
  betiği koşturabiliyordu ve `bt-core`'un uzak/yetki kapısı onu görmüyor →
  `links::action` her okunamayan `file:`'ı yutuyor (tablo satırı + bekçi).
  `/audit`: mekanik temiz, mercekler temiz (1, 2, 6 ilgisiz).
- `make linux` (links.rs) ve `make smoke` yeşil; `make test-race`/`make
  shader` gerekmedi: `bt-core` ve `.wgsl`'e dokunulmadı.
