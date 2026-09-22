# Phase 1 — Kapı önce "cevap geldi mi" diye sorar

## Özet

Tazelik kapısı iki yollu oluyor: kullanıcının son girdisinin aynası geldiyse
ve dock satırı gösterebiliyorsa taze; değilse bugünkü içerik kapısı. Tek
commit (`discussion.md` → Karar 3).

_Requirements: R1, R1.1, R1.2, R1.3, R2, R2.1, R2.2, R2.3, R3, R3.1, R3.2, R3.3, R4, R5_

## Değişiklikler

- **`crates/bt-core/src/dock.rs`** — `cell`'in glyph koşulundaki kontrol
  karakteri yarısı adlı bir fonksiyona çıkıyor (`column_width`'in yanında;
  ör. `draws_glyph_for_control` değil, "dock bu karakteri gösterebilir mi"
  sorusunun adı). `cell` onu kullanıyor; `' '`'nin mürekkepsizliği ayrı
  kalıyor — boşluk gösterilebilir, yalnız glyph istemiyor.
- **`crates/bt-core/src/shell.rs`**
  - `DockState` + `answers: u64`, `drawable: bool`; `clone_from` ve `reset`
    ikisini de taşıyor (024'te elle yazılmış `clone_from`'un yeni alanı
    kaçırdığı görüldü — ikisine de bak). `reset` `drawable = false`.
  - `decode_line` `drawable`'ı üç gövdenin zinciri üzerinden, `last_ink`'in
    yanında hesaplıyor; yüklem `dock`'taki fonksiyon.
  - `apply_scan` ayna olayında damgayı alıp `answers`'a yazıyor. Damga
    argüman: `ShellLog` `Session`'ı görmüyor ve görmemeli.
  - `SuppressedInput` + `answers`, `drawable`; `suppressed_input` ikisini
    aynı turda dolduruyor.
  - `^A` yorumu (`decode_line`, "Kalan sınır, adıyla") yeni gerçeğe dönüyor:
    artık "kapı yanlışlıkla düşüyor" değil "dock gösteremiyor, tasarım gereği
    ızgara".
- **`crates/bt-core/src/session.rs`**
  - `Session.key_gen: Arc<AtomicU64>` ve `TappedPty`'de kopyası
    (`screen_clears` emsali; doc'u "tek yazar ana thread, okuyucu yalnız
    okur" diyor).
  - `send_input`: artış boş-bayt erken dönüşünden **sonra**, `self.send`'den
    **önce**. `Ordering`: yazma `Release`, okumalar `Acquire` — damga ile
    kapının okuması ayrı thread'lerde.
  - `TappedPty::read`'in `apply_scan` kapanışı damgayı geçiriyor.
  - Kapı: `let answered = input.answers == self.key_gen.load(..);`
    `let fresh = (answered && input.drawable) || (last_ink_in_row(..) == input.last_ink && at_anchor);`
    — `||`'nin sol kolu önde, tarama kısa devreyle atlanıyor. Yorum bloğu
    "iki kesin veri" anlatısını üç terime genişletiyor ve iki bilinen sınırı
    adıyla yazıyor.
- **Bekçiler (`session.rs`)**
  - `a_stale_mirror_leaves_the_input_line_in_the_grid` ve
    `a_blank_mirror_below_the_anchor_is_stale`: bayatlık bir **girdiyle**
    kuruluyor (aynadan sonra `session.write(..)`/`paste(..)` → `key_gen`
    ilerliyor, ayna gelmiyor). İddiaları değişmiyor.
  - **Yeni** `a_transformed_char_keeps_the_caret_in_the_dock`: ayna `🥰`,
    ızgara `<0001f970>`, girdi yok (ya da girdi + ayna) ⇒ bastırma açık,
    `caret_in_dock`.
  - **Yeni** `a_control_char_the_dock_cannot_draw_stays_in_the_grid`: ayna
    `\x01`, ızgara `^A`, cevap gelmiş ⇒ bastırma yok, caret ızgarada.
  - **Yeni** `a_key_answered_by_an_older_mirror_is_a_known_limit`: tuş gönder
    → ayna inmeden `paste` → ayna o anki nesille damgalanır ⇒ kapı "cevap
    geldi" der. Sınırı **adıyla** tutuyor; bir gün kapanırsa bekçi kırmızıya
    döner ve cümle güncellenir.
  - **Yeni** `race_key_gen_and_mirror_stamp` (`#[ignore = "make test-yaris ile koşar"]`):
    ana thread `send_input`, okuyucu ayna basıyor; değişmez "damga hiçbir zaman
    `key_gen`'i aşmaz" ve "cevap geldi dediği karede ayna içeriği o damgayla
    aynı turdan".
- **`shell.rs` bekçileri** — `decode_line`'ın `drawable`'ı: düz metin ✓,
  emoji ✓, `\x01` ✗, `\t` ✗, boş ✓.
- **Belgeler** — `CLAUDE.md` bayatlık paragrafı ("iki kesin veriyi
  karşılaştırarak" → önce cevap, sonra içerik; `drawable`; iki sınır),
  `docs/YOL-HARITASI.md` (borç kalemi kapanıyor, `^X` kalemi açılıyor,
  on beşinci kayma: 025 tazelik / 026 materyal / 027 sekme), `.tasks/README.md`.

## Kabul

- Kullanıcının sahnesi: dock'lu pencerede `🥰` yazmak caret'i dock'ta
  bırakıyor; ızgaranın `<0001f970>`'i bastırılıyor (gözle, `make kur`).
- `Ctrl-V Ctrl-A` satırı bugünkü gibi ızgarada.
- Yapıştırmanın bayat ayna kolu (`bracketed-paste-magic`) bugünkü gibi
  ızgarada; iki eski bekçi girdiyle kurulmuş hâlde yeşil.
- `make hepsi` yeşil, `make test-yaris` yeşil, `make duman` yeşil.

## Uygulama Notları

_(uygulama sırasında doldurulur)_

## Checklist

- [ ] `dock` yüklemi tek fonksiyon, `cell` onu kullanıyor
- [ ] `DockState::answers`/`drawable`, `clone_from` + `reset`
- [ ] `decode_line` `drawable`; `apply_scan` damga
- [ ] `SuppressedInput` iki alan
- [ ] `key_gen`: `send_input` artırıyor, `TappedPty` okuyor
- [ ] Kapı yeni biçim, kısa devre
- [ ] İki eski bekçi girdiyle bayatlık kuruyor
- [ ] Yeni bekçiler: `<hex>` taze, `^A` ızgara, tuş→yapıştırma sınırı, `race_*`
- [ ] `shell.rs` `drawable` bekçileri
- [ ] `CLAUDE.md`, yol haritası, indeks
- [ ] `make hepsi`, `make test-yaris`, `make duman`
- [ ] Gözle: `🥰` yazınca caret dock'ta
