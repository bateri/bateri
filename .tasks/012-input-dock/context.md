# Input Dock ve prompt'un devri — Bağlam

## Mevcut Durum

**Giriş satırı bugün tamamen kabuğun.** Prompt'u zsh çiziyor, satır düzenlemeyi
ZLE yapıyor, terminal yalnız baytları taşıyor:

- **Girdi tek huniden geçiyor.** `view.rs:412` `keyDown:` → `keys.rs:42`
  `encode_key` (saf, AppKit'siz) → `view.rs:448` `session.write` →
  `session.rs:2050` `send_input`. Ok tuşları ve yapıştırma da aynı noktada
  buluşuyor. **IME yolu yok:** `NSTextInputClient`, `insertText:` ve marked-text
  kolu yazılmadı; `keys.rs:38-41` IME'yi ve ölü tuşları adıyla kapsam dışı
  ilan ediyor.
- **Kabuk betiği ZLE'ye hiç dokunmuyor.** `assets/shell/zsh/bateri.zsh` iki
  `add-zsh-hook` kuruyor (`precmd`, `preexec`, `:154-155`) ve OSC 133 A/B/C/D
  basıyor. `zle -N`, `add-zle-hook-widget` yok. `PS1`'e üç ek yapılıyor (çıpa
  açılışı `:212`, `B` soneki `:218`, çıpa kapanışı `:221`), **`RPS1`/`RPROMPT`
  dosyanın hiçbir yerinde geçmiyor**.
- **Safha okunuyor ama sınırı geçmiyor.** `shell.rs:91-100` dört safha taşıyor
  (`Prompt`/`Input`/`Running`/`Finished`). `Running` crate **içinde**
  tüketiliyor (`shell.rs:324` `running()` → `:353` `stripe()` →
  `session.rs:1585` `resolve_blocks`), yani ayrım ölü değil — ölü olan **`pub`
  sınırı**: `Session::shell_state()`'in (`session.rs:1894`) üretimde tek
  çağıranı yok, iki çağıranı da `#[cfg(test)]` içinde. `Input`, `Prompt`,
  `Finished` ve `last_exit` hiçbir üretim kolunda okunmuyor.
- **Prompt işareti diye bir şey yok.** 011 Karar 8 "safha `Input` iken
  `frame()` `cursor_row`'a bir `Block` verir" diye bir yedek bırakmıştı;
  **uygulanmadı**. `resolve_blocks` yalnız `stripe(id, running)` çağırıyor.
- **İçerik 011'den beri tabana yapışık** ve öteleme tek `setViewport`'la iniyor
  (`renderer.rs:592`), yani **imlecin satırı zaten pencerenin dibinde**.

## Motivasyon

Referans üründe giriş satırı terminalin: `docs/ARASTIRMA.md:53` prompt'u
terminalin çizdiğini (OSC 133 `B`), `:54-56` ise **Input Dock**'u — "prompt
pencere altında sabit ayrı satır editörü; zsh ZLE kancalarıyla; `Claude`,
`Codex`, REPL gibi yazmayı devralan uygulamalar davranışla tespit edilip dock
alanı geri alınıyor" — söylüyor.

**Envanterin söylediği bu kadar.** Görünüm (çerçeve, zemin, materyal, renk
rolü), boyut (kaç satır, hangi pay) ve "davranışla tespit"in **kuralı**
`ARASTIRMA.md`'de **yok**. Ayar tarafında tek anahtar var — `input_dock`
(`:100`) — ve envanter hiçbir anahtarın tipini/varsayılanını vermiyor. Yani bu
set kopyalanacak bir reçeteye değil, **kendi tasarım turuna** dayanıyor; bu,
setin en önemli tek gerçeği.

Zincirin en ucu ve kısayolu yok (`docs/YOL-HARITASI.md:54`): prompt'u terminal
çizmeden `>` çizilemiyor, `>` çizilmeden prompt'u devralmak kullanıcının
prompt'unu alıp yerine hiçbir şey koymamak oluyor — 011 bunu **ikinci turda
yaşadı** ve seti saf yerleşim işine daralttı.

## Kanıt

**011'in ikinci turu bu setin hazırlığını yaptı** ve bulguları jüriyle
doğrulanmış hâlde duruyor
(`.tasks/011-tabana-yapisik-icerik/discussion.md` → Karar 8, 9, 10, 12 ve
`## Muhakeme — 2. tur`):

- **Dock'un modeli seçildi: ayna (8b).** Tuşlar yine PTY'ye gider, ZLE
  `BUFFER`/`CURSOR`'ı geri bildirir, terminal dock'ta çizer; Tab, geçmiş ve
  Ctrl-R ZLE'de kalır. Elenenler: **8a** (ayrı alan yok, yalnız imleç satırına
  işaret) — Input Dock'un tanımını karşılamıyor; **8c** (asıl tampon) — iki
  ölçülmüş gerekçe: ZLE susturulmadan çift çizim kaçınılmaz, susturulunca
  Tab/geçmiş/Ctrl-R ölüyor; ve zsh'in tek-tuş prompt'ları (`CORRECT`'in
  `[nyae]?`'ı, `RM_STAR_WAIT`) ölümcül, çünkü dock Enter'a kadar tamponluyor.
- **Aynanın ödenmemiş bedeli adıyla kayıtlı:** ayna yalnız `BUFFER`/`CURSOR`
  taşıyor; ZLE'nin geri kalan çıktısı — tamamlama listesi, `menu-select`,
  `bck-i-search:`, `zle -M`, `CORRECT`'in `[nyae]`'i — aynada **yok**, ızgaraya
  düşüyor. 011 bunu "kendi tasarım turunu hak ediyor" diye **bu sete** bıraktı.
- **Çıpa yolu doğrulandı (10a).** Sıfır genişlikli PS1'de çıpa hayatta kalıyor;
  alacritty kaynağında üç satır kontrol edildi (`term/mod.rs:1876`, `:989`,
  `:1888-1893`) ve mekanizma iddiadan sağlam — `zle reset-prompt`, Ctrl-L,
  geçmişte gezinme ve `TRANSIENT_PROMPT` kırmıyor. *Bilinen sınır:* "ilk çıpalı
  satır = komutun satırı" **yapısal değil sıra** garantisi; `zle -I` ile basılan
  bir iş bildirimi şeridi bir satır yukarı kaydırabilir.
- **Kapsam daraltması (Karar 12):** `>` ve safha görünürlüğü ile çıkış kodu
  rengi girer; **çalışma dizini girmez** — OSC 7 bugün hiç bağlı değil
  (`Event::Title`/`ResetTitle` sessizce düşüyor). Git dalı, prompt ayar şeması
  ve prompt'un tema alt-rolleri de bu set değil.
- **Zorunlu ayrıntı:** PS1 tek başına yetmez, **`RPS1`/`RPROMPT` de** boşalmalı;
  p10k/starship PS1'i precmd'den **sonra** kendi ZLE kancalarından yeniden
  kuruyor, yani sıfır genişlik aynı yerden dayatılmazsa tema kazanır.

**`>` bugünkü `Frame`'de hâlâ temsil edilemiyor** (2026-09-17'de yeniden
doğrulandı): `frame.rs:594` `pos_at` sol payı ekleyen **tek** yer ve doc'u
ikinci bir yeri adıyla yasaklıyor; `frame.rs:60-67` `GlyphInstance`'ta `size`
alanı yok, glyph boyu kare başına tek uniform (`cell.metal:60`); `stride 32`
assert'i **iki tarafta** (`frame.rs:70-72` ve `cell.metal:22-24`), yani alan
eklemek ikisini birden kırıyor. `GUTTER_PT = 8.0` `private`
(`renderer.rs:135`). Eksik olan atlas sprite'ı **değil** (`>` ASCII, dört yüzde
de rasterize) — eksik olan **yerleştirme**. Payda dikdörtgen çizilebiliyor
(`frame.rs:409` `push_block`, `pos_at`'i bilerek atlayarak).

**Bir dock satırının sınırdaki bedeli dört dokunuş:**

1. **Öteleme aritmetiği** — `link.rs:838` `origin_target = rows - content_rows`
   ve tek `setViewport` dört listeyi birden kaydırıyor. Sabit bir dip satırı
   viewport'tan **muaf** olmak zorunda; emsali hazır: imlecin instance'ı
   ötelemeyi CPU'da geri veriyor (`frame.rs:458-473`).
2. **Liste ömrü** — dock `bg` listesine girerse `frame.rs:495` `move_cursor`'ın
   `truncate(bg_count)`'u onu her hareket karesinde siler ve dock **titrer**;
   `stripes`'in ayrı liste olmasının gerekçesi tam bu (`frame.rs:200-211`).
3. **Izgara yüksekliği** — `app.rs:403` `split_into_grid` sol payı
   **sütunlardan** düşüyor, satırlar payı görmüyor; dock satır yiyecekse
   `rows`'u küçülten yer orası ve `Cursor::rows`/`content_rows` sözleşmesi
   (`1..=rows`) ızgaranın, pencerenin değil.
4. **Sınır kaydı** — dock içeriği `frame()` sınırından **çözülmüş** geçmeli;
   `session.rs:271-274` renderer'da çıkış kodu tanıyan bir dalı yasaklıyor.

**Tarayıcının iki sınırı** (`BUFFER` taşınacaksa doğrudan bağlayıcı):
`shell.rs:486` OSC numarasını `133`'e kilitliyor ve başkasını tampona hiç
uğratmadan eliyor; `shell.rs:372` `PAYLOAD_LIMIT = 256` bayt, çıplak `ESC`
diziyi bitiriyor (`shell.rs:536`) — yani uzun bir `BUFFER` **sessizce düşer** ve
base64 zorunlu.

**Bayat referans uyarısı:** 011'in `discussion.md`'sindeki üç satır numarası
kaydı (`frame.rs:519-523` → bugün `585-598`; `session.rs:1792` → `1894`;
`session.rs:1361` → `1425`). Planlayıcı onları kovalamasın.

**Belge kayması (aynı commit'te düzelir):** `shell.rs:87-89` `ShellPhase`'in
doc'u ayrımı "Input Dock'un **(014)** ilk sorusu" diye anıyor; set bugün
**012**.

## Mevcut Mimari

```
KLAVYE                                   bt-shell
  view.rs:412 keyDown: ──► keys.rs:42 encode_key (saf)
                              └──► view.rs:448 session.write ─────┐
                                                                  ▼
PTY                                                          bt-core
  send_input (session.rs:2050) ──► çocuk ──► zsh + ZLE
                                              │  (satır düzenleme burada;
                                              │   terminal yalnız bayt taşıyor)
                                              ▼
  okuma yolu ──► Scanner (shell.rs:486, YALNIZ OSC 133, 256 bayt)
                    ├─► ShellLog.state  (safha; pub sınırı ÖLÜ)
                    └─► BlockLog        (çıpa → Block { row, stripe })
                                              │
  Session::frame() ─────────────────────────┬─┘
    Cell / Cursor / Blocks                  │
                                            ▼
bt-gpu   link.rs:838 origin_target ──► renderer.rs:592 tek setViewport
         frame.rs:409 push_block (payın içi, pos_at'i ATLAR)
         frame.rs:594 pos_at      (payı ekleyen TEK yer → `>` buradan geçemez)
```
