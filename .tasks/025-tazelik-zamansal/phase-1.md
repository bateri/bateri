# Phase 1 — Kapı önce "cevap geldi mi" diye sorar

## Özet

Tazelik kapısı iki yollu oluyor: kullanıcının son girdisinin aynası geldiyse
taze, gelmediyse bugünkü içerik kapısı. Dock'un gösteremediği satır (sekme
dışında kontrol karakteri) `DockStatus::Control` ile ızgarada kalıyor. Tek
commit (`discussion.md` → Karar 3).

_Requirements: R1, R1.1, R1.2, R1.3, R2, R2.1, R2.2, R2.3, R3, R3.1, R3.2, R3.3, R4, R5_

## Değişiklikler

- **`crates/bt-core/src/shell.rs`**
  - `DockStatus::Control` — `Multiline`'ın kardeşi, doc'u aynı cümleyi
    kuruyor: veri sağlam, yüzey dar. `decode_line` satır sonu kontrolünün
    yanında `c.is_control() && c != '\t'` ile kuruyor (`\n` zaten
    `Multiline`; sıra: önce `Multiline`, sonra `Control`, ikisi birden
    varsa satır sonu kazanıyor — ikisi de ızgara, fark yalnız ad).
  - **Sekme istisnasının gerekçesi yoruma** (`discussion.md` → Karar 1):
    sekme bilgi taşımıyor, istisnasız Ctrl-V Tab satırı dock'tan ızgaraya
    düşerdi.
  - `DockState` + `answers: u64`; `clone_from` ve `reset` taşıyor (024'te
    elle yazılmış `clone_from`'un yeni alanı kaçırdığı görüldü).
  - `apply_scan` ayna olayında damgayı alıp `answers`'a yazıyor. Damga
    argüman: `ShellLog` `Session`'ı görmüyor ve görmemeli.
  - `SuppressedInput` + `answers`; `suppressed_input` aynı turda dolduruyor.
  - `caret_home` ve `HANDOVER_HOLD`'un arıza listesi `Control`'ü
    `Multiline` gibi sayıyor (tutma yok — gösteremediğimiz satırın caret'i
    ızgarada).
  - `^A` yorumu (`decode_line`, "Kalan sınır, adıyla") kalkıyor: sınır
    kapandı, yerine `Control`'ün gerekçesi.
- **`crates/bt-core/src/dock.rs`** — `DockStatus` eşleşmelerinde `Control`
  `Multiline`'ın kolunda (metinsiz satır, caret satır başında). `cell`'in
  kontrol karakteri yorumu güncelleniyor: artık dock'a ulaşan tek kontrol
  karakteri sekme.
- **`crates/bt-core/src/session.rs`**
  - `Session.key_gen: Arc<AtomicU64>` ve `TappedPty`'de kopyası
    (`screen_clears` emsali; doc'u "tek yazar ana thread, okuyucu yalnız
    okur" diyor).
  - `send_input`: artış boş-bayt erken dönüşünden **sonra**, `self.send`'den
    **önce**. `Ordering`: yazma `Release`, okumalar `Acquire` — damga ile
    kapının okuması ayrı thread'lerde.
  - `TappedPty::read`'in `apply_scan` kapanışı damgayı geçiriyor.
  - Kapı: `let answered = input.answers == self.key_gen.load(..);`
    `let fresh = answered || (last_ink_in_row(..) == input.last_ink && at_anchor);`
    — `||`'nin sol kolu önde, tarama kısa devreyle atlanıyor. Yorum bloğu
    "iki kesin veri" anlatısının önüne cevap terimini koyuyor ve iki bilinen sınırı
    adıyla yazıyor.
- **Bekçiler (`session.rs`)**
  - `a_stale_mirror_leaves_the_input_line_in_the_grid` ve
    `a_blank_mirror_below_the_anchor_is_stale`: bayatlık bir **girdiyle**
    kuruluyor (aynadan sonra `session.write(..)`/`paste(..)` → `key_gen`
    ilerliyor, ayna gelmiyor). İkincisinin adı da değişiyor
    (`an_unanswered_blank_mirror_below_the_anchor_is_stale` gibi): gerçekten
    gelmiş boş bir ayna (`zle -I` sonrası redisplay) çıpanın altında olsa da
    artık taze ve bu **doğru**.
  - **Yeni** `a_transformed_char_keeps_the_caret_in_the_dock`: ayna `🥰`,
    ızgara `<0001f970>`, girdi yok (ya da girdi + ayna) ⇒ bastırma açık,
    `caret_in_dock`.
  - **Yeni** `a_control_char_the_dock_cannot_draw_stays_in_the_grid`: ayna
    `\x01foo` **ve** `foo\x01` (ortada ve sonda), cevap gelmiş ⇒ bastırma
    yok, caret ızgarada. Ortadaki hâl bugün sessizce kayboluyordu.
  - **Yeni** `a_key_answered_by_an_older_mirror_is_a_known_limit`: tuş gönder
    → ayna inmeden `paste` → ayna o anki nesille damgalanır ⇒ kapı "cevap
    geldi" der. Sınırı **adıyla** tutuyor; bir gün kapanırsa bekçi kırmızıya
    döner ve cümle güncellenir. **Damgalama elle taklit ediliyor**: pty'siz
    düzenekte ZLE yok, yani "ayna yoldayken" hâli üretmek uygulamanın
    kendisi olurdu — bekçinin doc'u bunu söylüyor.
  - **Yeni** `race_key_gen_and_mirror_stamp` (`#[ignore = "make test-yaris ile koşar"]`):
    bir thread `send_input`'u döverken öteki **ayrı içerikli** aynaları ayrı
    damgalarla basıyor; kare tarafı `suppressed_input`'u alıp
    `answers == key_gen` ise içeriğin (`last_ink`, `cursor_col`) **o damganın
    aynasından** olduğunu iddia ediyor. ("Damga `key_gen`'i aşmaz" değişmez
    değil totoloji — damga `load()`'un kendisi.)
- **`shell.rs` bekçileri** — `decode_line`'ın durumu: düz metin, emoji,
  `\t` ve boş → `Live`; `\x01` (ortada, sonda, `PREDISPLAY`/`POSTDISPLAY`
  içinde) → `Control`; `\n` + `\x01` → `Multiline`; kontrol karakteri
  silinince bir sonraki aynada `Live`.
- **Belgeler** — `CLAUDE.md` bayatlık paragrafı ("iki kesin veriyi
  karşılaştırarak" → önce cevap, sonra içerik; `Control` kolu yanına; iki
  sınır),
  `docs/YOL-HARITASI.md` (borç kalemi kapanıyor, `^X` kalemi açılıyor,
  on beşinci kayma: 025 tazelik / 026 materyal / 027 sekme), `.tasks/README.md`.

## Kabul

- Kullanıcının sahnesi: dock'lu pencerede `🥰` yazmak caret'i dock'ta
  bırakıyor; ızgaranın `<0001f970>`'i bastırılıyor (gözle, `make kur`).
- `Ctrl-V Ctrl-A` satırı, `^A` nerede olursa olsun ızgarada ve okunur.
- `Ctrl-V Tab` satırı bugünkü gibi dock'ta.
- Yapıştırmanın bayat ayna kolu (`bracketed-paste-magic`) bugünkü gibi
  ızgarada; iki eski bekçi girdiyle kurulmuş hâlde yeşil.
- `make hepsi` yeşil, `make test-yaris` yeşil, `make duman` yeşil.

## Uygulama Notları

_(uygulama sırasında doldurulur)_

## Checklist

- [ ] `DockStatus::Control` + `decode_line` (sekme istisnası, gerekçesiyle)
- [ ] `caret_home`/tutma ve `dock` eşleşmeleri `Control`'ü `Multiline` gibi sayıyor
- [ ] `DockState::answers`, `clone_from` + `reset`; `apply_scan` damga
- [ ] `SuppressedInput::answers`
- [ ] `key_gen`: `send_input` artırıyor, `TappedPty` okuyor
- [ ] Kapı yeni biçim, kısa devre
- [ ] İki eski bekçi girdiyle bayatlık kuruyor; ikincisinin adı değişti
- [ ] Yeni bekçiler: `<hex>` taze, `^A` ortada/sonda ızgara, tuş→yapıştırma sınırı, `race_*`
- [ ] `shell.rs` durum bekçileri
- [ ] `CLAUDE.md`, yol haritası (borç kapanır, `^X` kalemi), indeks
- [ ] `make hepsi`, `make test-yaris`, `make duman`
- [ ] Gözle: `🥰` yazınca caret dock'ta; `Ctrl-V Ctrl-A` ızgarada
