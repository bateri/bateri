# İmleç stilleri ve blink — Bağlam

## Mevcut Durum

İmleç bugün **her koşulda dolu bir bloktur ve hiç yanıp sönmez**.

- **Şekil sınırı hiç geçmiyor.** `bt_core::Cursor` (`session.rs:202-288`)
  dokuz alan taşıyor — konum, görünürlük, altındaki metnin rengi, kaydırma
  ofseti, doluluk, `next_tick` — ama **şekil alanı yok**. `CursorShape` bütün
  workspace'te iki yerde geçiyor ve ikisi de aynı soruyu soruyor:
  `session.rs:1401` `cursor.shape != CursorShape::Hidden` (çizilsin mi) ve
  `session.rs:1580` seçim vurgusunun sınır hücresi. Yani şekil **okunuyor ama
  yayılmıyor**.
- **Çizim tam hücreye çivili.** `Caret::instance` (`frame.rs:121-128`) boyutu
  `cell_px`'e sabitliyor, `push_caret` (`frame.rs:657`) ters çevirme
  dikdörtgenini `[x, y, x + cell_w, y + cell_h]` diye kuruyor. Shader tarafında
  ters çevirme bir ezme değil **karışım**: `cell.metal:124`
  `mix(in.rgba.rgb, cursor.rgba.rgb, inside ? cursor.rgba.a : 0.0)`.
- **Blink sinyali yutuluyor.** `Event::CursorBlinkingChange` `session.rs:851`'de
  boş kolda duruyor. Alacritty `RenderableCursor`'da blink bilgisi **yok**
  (`term/mod.rs:2365-2367` yalnız `shape` ve `point`); `Term::cursor_style()`
  (`term/mod.rs:942`) `CursorStyle { shape, blinking }` veriyor ve o çağrılmıyor.
- **Varsayılan hiçbir yerde kurulmuyor.** `Session::term_config`
  (`session.rs:456-465`) yalnız `scrolling_history` ve `osc52` kuruyor, gerisi
  `..Config::default()` — yani `default_cursor_style` alacritty'nin
  varsayılanında (`CursorShape::Block`, `blinking: false`) duruyor ve bunu
  `term_config_keeps_every_other_field` sınaması çiviliyor.
- **Kare talebinin üç sebebi var** (`bt-gpu::link` modül başlığı): **hasar**
  (`Waker::wake`), **hareket** (uyanık callback'in kendi kararı) ve **saat**
  (013'ün getirdiği, uyku noktasında kurulan tek gecikmeli uyandırma).

## Motivasyon

Referansta iki ayrı ayar var: `cursor` ve `cursor_blink`
(`docs/ARASTIRMA.md:100`). Site'nin saydığı yedi "imleç stili" (Snap, Ease,
Spring, Smear, Squash, Phosphor, Arc) bunlar **değil** — onlar `cursor_motion`,
yani **hareket** stilleri ve üçü 008'de indi. Bu set o listenin değil,
`cursor` + `cursor_blink` çiftinin karşılığı.

İş 008'de **adıyla** ertelenmişti (`.tasks/008-hareket-ve-imlec/plan.md:73-77`
Kapsam Dışı: "İmleç şekilleri (beam/underline, DECSCUSR), blink") ve gerekçesi
de yazılıydı (`discussion.md:197-203`): *"blink (ilk **süresiz** animasyon
olurdu; boşta sıfır kareyle barışması ayrı bir karar: kitty'nin 'N saniye sonra
dur' kolu)"*. Bu set o ertelenmiş kararı açıyor.

İkisi **tek sette**, çünkü protokol onları tek dizide birleştiriyor: DECSCUSR
(`CSI Ps SP q`) altı değer taşıyor ve altısı 3 şekil × {sabit, yanıp sönen}
(`vte-0.15.0/src/ansi.rs:1716-1725`; tek sayı = blink). Yalnız şekli uygulamak
`\e[5 q` gelen bir terminalde blink'i **sessizce yutmak** demek.

## Kanıt

**Yukarı akış bedava.** `vte-0.15.0` hem DECSCUSR'ı (`ansi.rs:1716-1725`), hem
DECSET/DECRST 12'yi (`term/mod.rs:1987-2039`), hem de iTerm2'nin OSC 50
`CursorShape=` biçimini (`ansi.rs:1468-1473`) çözüyor. Beş şekil tanımlı
(`Block`, `Underline`, `Beam`, `HollowBlock`, `Hidden`), `Term::cursor_style()`
`pub`. **Ayrıştırıcıya tek satır yazılmıyor, yeni bağımlılık yok.**

**Özellik iner inmez görünür.** İlan ettiğimiz `TERM=xterm-256color`
terminfo'da `Ss=\E[%p1%d q` ve `Se=\E[2 q` **zaten var** (bu makinede
`infocmp -x` ile doğrulandı), yani vim/neovim DECSCUSR'ı kutudan gönderiyor.
Bugün o dizi sessizce yutuluyor: vim'de insert moda geçen kullanıcı beam
istiyor, blok görüyor.

**Aynı terminfo iki yeteneği daha ilan ediyor ve ikisini de tutmuyoruz:**
`Cs=\E]12;%p1%s\007` ve `Cr=\E]112\007` — imleç renginin OSC 12 ile
ayarlanması. `Event::ColorRequest` (`session.rs:791`) sorguyu **yanıtlıyor**,
ama caret'i boyayan yol rengi tablodan değil `theme.accent_linear()`'dan
alıyor, yani OSC 12 ile yazılan renk çizime hiç ulaşmıyor. Bu setin konusu
değil; kayda geçiyor (→ Kapsam Dışı).

**Depo blink'i adıyla bekliyor.** `bt-shell/src/app.rs:110-111`,
`IDLE_FRAME_LIMIT`'in doc'unda: *"Durma koşulu unutulmuş 2 Hz'lik bir blink üç
saniyede ~6 kare eder — sınırın altında, yani bu sayı onu tek başına
**göremez**."* Kapının ikinci katı (`QUIET_FLOOR = 868 ms`) onu görür — ve tam
bu yüzden blink'in varsayılanı bir kapı sorusudur.

**Saatin blink'le somut bir çarpışması var ve izi sürüldü.** `arm_clock`
(`link.rs:1045-1085`) her uyku noktasında `Cursor::next_tick`'i **süre** olarak
yeniden kuruyor. Bugün bunun bedeli küçük ve doc'unda yazılı (`link.rs:1059`):
*"Hareket karesi `Cursor`'ı tazelemiyor… uzun bir animasyondan sonra kurulan
tik bir animasyon boyu geç kalabilir. Yol kendini düzeltiyor."* Kendini
düzeltmesinin sebebi, imleç kaymasının **biten** bir animasyon olması
(~230 ms'de bir kez geciktirir, sonra `next_tick` tam saniyeye oturur).

Blink bitmiyor. Yarım periyot 0,5 sn ise her uyku noktası sayacın 1 sn'lik
tikini **1 sn ileri itiyor** ve tik **hiç ateşlemiyor**:

```
t=0.00  içerik karesi, next_tick=1.00   → uyku, kur min(1.00, blink 0.50)
t=0.50  blink karesi                    → uyku, kur min(1.00, blink 0.50)   ← sayaç 1.50'ye itildi
t=1.00  blink karesi                    → uyku, kur min(1.00, blink 0.50)   ← 2.00'ye itildi
...                                       sayaç asla ateşlemez
```

013'ün canlı sayacı, blink açıkken **durur**. Çare süre değil **son tarih**
tutmak; aynı düzeltme `link.rs:1059`'un yazılı "geç kalabilir" kusurunu da
kapatıyor.

## Mevcut Mimari

```
                      ┌─ içerik karesi ────────────────────────────────┐
  hasar (Waker::wake) ┤  dirty.mark() → take_damage() true             │
                      │  session.frame(sink) → Cursor  (Term kilidi)   │
                      │  motion.sync(...)  ·  push_caret(...)          │
                      │  icerik++                                      │
                      └────────────────────────────────────────────────┘

                      ┌─ hareket karesi ───────────────────────────────┐
  hareket (callback)  ┤  take_damage() false, motion.settled() false   │
                      │  listeler KORUNUYOR, bt-core'a HİÇ gidilmiyor  │
                      │  move_caret(motion.position(), motion.alpha()) │
                      │  hareket++ / kayma++                           │
                      └────────────────────────────────────────────────┘

                      ┌─ uyku ─────────────────────────────────────────┐
  saat (arm_clock)    ┤  take_damage() false, motion.settled() true    │
                      │  setPaused(true); arm_clock()                  │
                      │  → süre dolunca Waker::wake()  = İÇERİK karesi │
                      └────────────────────────────────────────────────┘
```

Blink'in doğal yeri **ortadaki kutu** (ızgara değişmiyor, yalnız caret'in
alfası) ama tetikleyicisi **alttaki kutu** (saniyede iki kez, ekran hızında
değil). Bugün o ikisini birleştiren bir yol yok: `arm_clock`'ın elindeki tek
uyandırma `Waker::wake()` ve o **hasar dikiyor**, yani ızgarayı yeniden
taratıp kareyi `icerik=` diye saydırıyor.

`Waker::wake()` (`link.rs:181-207`) üç iş yapıyor: `requests.fetch_add` →
`dirty.mark()` → kapı → birleştirilmiş dispatch → `setPaused(false)`.
Blink'in **ortadakine** ihtiyacı yok.
