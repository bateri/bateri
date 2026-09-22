# Dock sütun saysın

## Hedef

Dock'ta yazılan geniş karakter (emoji, CJK, fullwidth) ızgarada göründüğü
**gibi** görünsün: iki hücre, doğru sütunda caret, tam boyanmış vurgu — ve
zsh'in **ham geçirdiği** hiçbir emoji giriş satırını ızgaraya fırlatmasın.

**Hedef cümlesi teslimden sonra daraltıldı** (kullanıcı ekran görüntüsüyle
bildirdi, ölçüldü): ilk yazımı "hiçbir emoji" diyordu ve o fazla güçlüydü.
zsh bazı kod noktalarını **kendisi** `<hex>` diye yazıyor (`🥰` U+1F970 →
`ESC[7m<0001f970>ESC[27m`, `zsh -f` ile saf bir pty'de doğrulandı) ve o hâlde
ayna ham emojiyi, ızgara on ASCII hücresini taşıyor — hiçbir **içerik**
karşılaştırması ikisini eşleştiremez, yani bastırma kalkıyor ve satır iki
yerde görünüyor. Kusur bu setin kolunda değil: kapının ölçütü içerik yerine
**zaman** olmalı ve o ayrı bir set (`docs/YOL-HARITASI.md` → tazelik
kapısının ölçütü). Ölçüldü ki daralmanın sınırı da belli — 🎉 😀 📁 ❤ 漢 Ａ █
hepsi ham geçiyor, yani setin vaadi onlarda tutuyor.

## Gereksinimler

- **R1** — Genişliğin tek kaynağı `unicode-width`, ızgaranın kullandığı crate.
  - **R1.1** — `bt-core`'un listesine giriyor; `Cargo.lock`'ta sürüm
    **oynamıyor** (grafta zaten var).
  - **R1.2** — `bt-atlas` ile `bt-gpu` onu **görmüyor**: genişlik `bt-core`'un
    yetkisi ve sınırdan `Cell::wide` olarak geçiyor (023'ün kuralı).
- **R2** — `dock::render` sütunu **biriktiriyor**, indeksten türetmiyor.
  - **R2.1** — `region_highlight` araması **karakter indeksinde** kalıyor
    (`style_at`), çünkü ZLE'nin birimi o.
  - **R2.2** — Yatay kaydırma penceresi (`skip`) sütun cinsinden.
  - **R2.3** — Caret'in sütunu imleçten önceki karakterlerin genişlik
    toplamı.
  - **R2.4** — Pencerenin iki kenarında da geniş glyph **yarılanmıyor**:
    sığmayan karakter hiç çizilmiyor, sütun boş kalıyor.
- **R3** — Geniş karakterin baş hücresi `Cell::wide` taşıyor ve **spacer
  sütununa arka plan hücresi** düşüyor (vurgu iki hücreye yayılsın).
  - **R3.1** — Spacer hücresi **glyph vermiyor**, yalnız zemin: ızgaranın
    `WIDE_CHAR_SPACER` kolunun aynısı.
  - **R3.2** — 023'ün `the_dock_never_marks_a_cell_wide` bekçisi **tersine
    çevriliyor**; kutu silinmiyor, iddiası değişiyor.
- **R4** — Bastırmanın tazelik kapısının iki tarafı aynı birimi okuyor.
  - **R4.1** — `DockState::last_ink` **sıfır genişlikli** kod noktalarını
    atlıyor (VS16, ZWJ, ten rengi), çünkü onlar ızgara hücresine hiç
    girmiyor.
  - **R4.2** — Kapının yanlış yönü korunuyor: şüpheli hâl bastırmayı
    bırakıyor.
- **R5** — Caret'in **hareketi** değişmiyor: karakter başına, ZLE'nin işi.
- **R6** — Yazılı sözleşme aynı sette gerçeğe uyuyor: `CLAUDE.md`'nin dock
  paragrafı, `docs/YOL-HARITASI.md`'nin `CURSOR` kalemi (kapanıyor) ve
  bağımlılık kararının kaydı.

## Yaklaşım

1. **`bt-core/Cargo.toml`** — `unicode-width` workspace bağımlılığı.
2. **`bt-core/src/dock.rs`** — çizim döngüsü indeks yerine **sütun**
   biriktiriyor: karakter başına genişlik okunuyor, `skip` sütun penceresine
   çevriliyor, sığmayan geniş karakter atlanıyor, baş hücreye `wide` ve
   spacer sütununa zemin hücresi düşüyor, caret'in sütunu toplamdan geliyor.
   `style_at` **indeksle** aranmaya devam ediyor.
3. **`bt-core/src/shell.rs`** — `last_ink` sıfır genişlikli kod noktalarını
   atlıyor.
4. **Bekçiler** — 023'ün dock bekçisi tersine çevriliyor; yeni bekçiler:
   emoji iki hücre, caret doğru sütun, kenarda yarılanma yok, vurgu iki
   hücre, VS16'lı satır bastırılıyor.
5. **Belgeler** — `CLAUDE.md`, yol haritası, indeks.

## Kapsam Dışı

- **Grapheme dizileri** (ZWJ, ten rengi, VS16'nın *renkli* hâli) — atlas
  anahtarı `Sprite::Char(char)` ve bir diziyi ifade edemiyor; ayrı set.
  Bu set yalnız **atlamayı** düzeltiyor, dizinin kendisini çizmiyor.
- **Caret'in geniş hücrede iki hücre olması** — ızgarayla parite bilinçli;
  ikisini birden genişletmek `caret_rect`'in tek yerinden geçer ve ayrı bir iş.
- **Dock'un çok satırlı girişi** — `DockStatus::Multiline` kolu duruyor;
  bandın boyu ızgaranın satırlarından düşüldüğü için her yeni satır bir PTY
  resize'ı ve o ayrı bir karar (yol haritasında kayıtlı).
- **Bağlam satırı** (`DOCK_CONTEXT_ROW`) — küçük sınıf ve sütun adımı küçük
  yüzün ilerlemesi; geniş yol orada **kapalı** kalıyor (021'in emsali) ve
  gerekçesi ölçü ayrışması.

## Akış

```
ayna (ZLE)                bt-core::dock                      bt-gpu
BUFFER + CURSOR ────────▶ for (index, ch)                     Frame::push
region_highlight          │  w = width(ch)   ◀── unicode-width  │
(karakter indeksi)        │  style_at(index) ◀── indeks kalıyor ▼
                          │  col += w                    GlyphCell{wide}
                          │  wide = (w == 2)                    │
                          └─ caret = Σ w (imleçten önce)  prepare yelpazeler
                                                          (023'ten geliyor)
kenar kuralı: sığmayan geniş karakter çizilmiyor, sütun boş kalıyor
tazelik kapısı: last_ink sıfır genişliklileri atlıyor → iki taraf aynı birim
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| kapı | ✅ |

### Kapı (2026-09-22)

`/code-review` setin aralığında (`27c94d2^..HEAD`) **tek** bulgu verdi ve o
bir **panik**: `available == 1` iken caret'in altında iki sütunluk bir
karakter varsa pay pencereden büyük oluyor, `caret_col - skip` negatife
düşüyor ve çıkarma taşıyor — debug'da `bt-core`'da kare yolunda panik (depo
bunu adıyla yasaklıyor), release'de sarma ve caret prompt işaretinin payına
düşüyor. Kökü **bir önceki kapının düzeltmesiydi**: phase-2'de eklenen
`caret_width` payı kendi kenarını doğurdu. Pay `min(available)` ile
kırpıldı ve bekçisi `a_window_narrower_than_the_caret_char_does_not_underflow`
(panik birebir üretildi, sonra geçti). Karar 2 bozulmuyor: sığmayan karakter
yine çizilmiyor, yalnız caret son sütuna sabitleniyor.

Kapı ayrıca üç şeyi **denetleyip temiz buldu** ve ikisi bu setin özüne
dokunuyor: `unicode-width` tek sürüm (0.2.2) ve `alacritty_terminal`'la
paylaşımlı (`cargo tree -d` temiz), yani Karar 1'in öncülü ayakta; bastırma
aritmetiği ızgaranın `LEADING_WIDE_CHAR_SPACER` sarmasında da doğru kalıyor
(78/79/80 sütun sınırları sınandı); ve tazelik kapısının iki yarısı artık
`❤️` ile `漢` için aynı cevabı veriyor.

`/audit` **bir** bulgu verdi (mercek 1 kayıtlı, mercek 2/5/6 ilgisiz —
`settings.rs`, `link.rs`, `.metal` ve sınır `Cell`'i diff'te yok; mercek 3 ve
7 temiz):

- **Mercek 4 (thread ve blokaj)** — kare yolunda **iki** önek gezinti vardı
  (`take(cursor).sum()` ve `nth(cursor)`) ve birincisi aynı zamanda
  `cursor_col`'un **ikinci üreticisiydi**; `DockState::cursor_col` phase-2'de
  doğmuştu ve tek sahip o olmalıydı. `render` artık onu okuyor: bir gezinti
  ve bir üretici eksildi. Bu, bu setin kaçınmak için var olduğu "iki
  yetkili" kokusunun **kendi kodumdaki** hâliydi ve bu oturumda ikinci kez
  oldu (ilki `shell.rs`'in `width_of` kapanışıydı, phase-2'de aynı şekilde
  birleştirildi).

**Teslimden sonra bir kalem açıldı ve setin vaadi daraltıldı** — kullanıcı
ekran görüntüsüyle bildirdi: zsh bazı kod noktalarını kendisi `<hex>` diye
yazıyor ve o hâlde bastırma kalkıyor. Ayrıntısı yukarıda `## Hedef`'te ve
`docs/YOL-HARITASI.md` → tazelik kapısının ölçütü.
