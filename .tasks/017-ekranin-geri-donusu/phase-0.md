# Phase 0 — Ölçüm: negatif viewport ve prompt kayması

## Özet

İki sayı ölçülür ve biri phase-3'ün kolunu seçer: Metal negatif `originY`'yi
kabul ediyor mu, ve Ctrl-C iptalinde yeni prompt kaç satır aşağı düşüyor.

_Requirements: R3.1_

## Neden ayrı phase

`renderer.rs`'in `encode_dock`'unda yazılı kayıt — *"negatif bir `originY`
Metal'in doğrulamasına düşerdi — süreci öldüren bir istisna"* — **ölçülmüş mü
varsayım mı belli değil** ve phase-3'ün iki kolundan hangisinin inebileceğini
o cevap belirliyor. Yanlış kolu seçip geri dönmek bir phase'i çöpe atardı;
ölçmek bir sınama dosyası.

## Değişiklikler

- **`crates/bt-gpu/src/renderer.rs`** — offscreen sınama ailesine bir tanık:
  negatif `originY` ile bir kare encode edilir. Emsali
  `cell_bg_paints_pixels_on_the_gpu` ve `content_sticks_to_the_bottom_*`;
  gerçek Metal aygıtı isteyen sınamaların koşma koşulu aynen geçerli.
  Beklenen iki sonuçtan biri **kabul**: çizim doğru yere düşer (kol 2b-i
  açılır) ya da doğrulama düşer (kol 2b-ii). Sonuç ne olursa olsun sınama
  **kalır** ve `encode_dock`'un `.max(0.0)` kırpmasının gerekçesi ya
  doğrulanmış ya da düzeltilmiş olur.

## Kabul

- Negatif `originY` sorusunun cevabı `## Uygulama Notları`'na **sayıyla**
  yazılır ve phase-3'ün kolu adıyla seçilir.
- **Ölçüm kayan kareyi de kapsar, yalnız yerleşiği değil:** `origin_px`'in
  `fill_px`'ten **küçük** olduğu ara değerlerle de bir kare encode edilir.
  Kanarya yalnız yerleşik hâli görürse dinlenmede geçer, 150 ms'te düşer —
  negatif `originY` tam da kaymanın ortasında doğuyor.
- Kolların motion karesindeki bedeli de yazılır: 2b-i'de viewport her kare
  `frame.origin_px()`'ten yeniden hesaplanıyor (ızgaranın kendi viewport'u
  `renderer.rs:651`'de zaten böyle okuyor), yani hareket karesi bedava;
  2b-ii'de her kare `fill × cols` instance'ın konumu yeniden türetiliyor.
- Ctrl-C iptalindeki prompt kayması gerçek `zsh -i` ile ölçülür (reçete
  `context.md` → Kanıt 2) ve `fill` aritmetiğinin +1 taşıyıp taşımadığı
  yazılır.
- `make hepsi` yeşil.

## Yayın Etkisi

- shader: yok (yeni `.metal` yok; sınama mevcut pipeline'ı kullanıyor).
- `CLAUDE.md` / `docs`: yok — bu phase soru soruyor, sözleşme değiştirmiyor.
  Düzelen cümle `encode_dock`'un kod yorumunda yaşıyordu ve kopyası başka
  yerde yok (`CLAUDE.md` ve `docs/` arandı).
- **Ölçüm bekleyen iddia:** yok; bu phase'in **kendisi** ölçüm.
- Yeni bağımlılık: yok.

## Checklist

- [x] Negatif `originY` tanığı yazıldı ve koşturuldu
- [x] `encode_dock`'un `.max(0.0)` yorumu ölçüme göre düzeltildi ya da
      doğrulandığı yazıldı
- [x] Ctrl-C'nin prompt kayması ölçüldü, `## Uygulama Notları`'na yazıldı
- [x] phase-3'ün kolu (2b-i / 2b-ii) seçildi ve `plan.md`'ye not düşüldü
- [x] Doğrulama geçti (`make hepsi`)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

### 1. Negatif `originY` — **meşru**

Ölçüm 2026-09-20, **Apple M1 Pro / macOS 26.4.1**; tanık
`bt_gpu::renderer::tests::a_negative_viewport_origin_draws_and_clips_from_the_top`.
Soru `MTLViewport`'un `originY` alanına ait ve hangi listenin çizildiği Metal'i
ilgilendirmiyor, bu yüzden tanık ızgaranın **kendi** viewport'undan geçiyor
(`Frame::set_origin_rows` negatif satırı kabul ediyor) ve ikinci bir encode
yolu icat etmiyor.

`fill_px = 8` (bir hücre), doku 16×16, `originY = origin_px − fill_px`:

| `origin_px` | `originY` | kare | boyanan |
|---|---|---|---|
| 16 | **+8** | yerleşik (kontrol) | doldurma [8,16); içerik kırpıldı |
| 8 | **0** | kayma | doldurma [0,8), içerik [8,16) |
| 5 | **−3** | kayma | doldurma [0,5) — üstten 3 px kırpıldı; içerik [5,13); [13,16) clear |
| 3 | **−5** | kayma | doldurma [0,3), içerik [3,11); [11,16) clear |
| 0 | **−8** | kayma | doldurma tamamen kırpıldı; içerik [0,8) |

Beş karenin **dördü yerleşik değil** ve **üçünde `originY` negatif**
(`origin_px < fill_px`), yani Kabul'ün istediği ara değerler kapsandı. Her
karede dokunun 16 satırının **tamamı** beklenen renkle
karşılaştırılıyor: viewport'un dışında kalan her satır clear rengiyle duruyor,
yani üstten kırpma sarmıyor, kaydırmıyor.

İki koşu, ikisinin de çıkışı **0**:

| koşu | komut | sonuç |
|---|---|---|
| doğrulama kapalı (üretimin hâli) | `cargo test -p bt-gpu a_negative_viewport_origin` | geçti |
| doğrulama **açık** | `MTL_DEBUG_LAYER=1 MTL_DEBUG_LAYER_ERROR_MODE=assert cargo test -p bt-gpu a_negative_viewport_origin` | `Metal API Validation Enabled` bastı, **itiraz etmedi**, geçti |

İkinci koşu şart: Metal'in doğrulama katmanı varsayılan `cargo test`'te
**kapalı** ve yalnız birincisiyle ölçülseydi cevap "bu sürücü çiziyor" olurdu,
"meşru" değil — oysa `encode_dock`'un iddiası tam olarak *doğrulamaya*
aitti.

Altıncı kare aynı encoder'da negatif viewport'un **ardından** ikinci bir
`setViewport` kuruyor (dock): 2b-i'nin şekli tam olarak bu ve dock'un zemini
çizildi, yani pass negatif orijinde düşmüyor.

**`encode_dock`'un yorumu yanlıştı ve düzeltildi.** *"Negatif bir `originY`
Metal'in doğrulamasına düşerdi — süreci öldüren bir istisna"* ölçülmemiş bir
varsayımdı. `.max(0.0)` **kaldı**: kırpmanın kendi gerekçesi duruyor —
dock'tan alçak pencerede doğru cevap dejenere (dock pencereyi kaplar) ve
negatif bırakılsaydı dock ızgaranın alanına taşardı.

### 2. Seçilen kol — **2b-i** (üçüncü `setViewport`)

Negatif `originY` meşru olduğu için phase-3 dock'un birebir emsalini alıyor:
`originY = origin_px − fill_px`, ızgaradan sonra dock'tan önce. Okuma anı
çeviri (2b-ii) **elendi**.

### 3. Kolların hareket karesindeki bedeli

Hareket karesi listeleri koruyup yalnız `origin_px`'i yeniden yazıyor
(`frame.rs` → `a_motion_frame_keeps_the_lists_and_moves_only_the_cursor`).

- **2b-i** — viewport encode anında `frame.origin_px()`'ten türüyor
  (ızgaranınki `renderer.rs:652`'de zaten böyle okuyor), yani hareket karesinin
  bedeli **bir çıkarma** ve doldurmanın boyundan bağımsız. Listelere
  dokunulmuyor.
- **2b-ii** — çeviri okuma anında olurdu (`frame.rs:1215`, caret emsali:
  `instance.pos[1] -= self.origin_px`), yani her hareket karesinde doldurmanın
  **her instance'ının** y'si yeniden türerdi: `fill × cols` arka plan, o kadar
  glyph ve kural. Aşağıda ölçülen senaryoda `fill = 3`, `cols = 80` → liste
  başına 240 instance, kare başına (aritmetik, ölçüm değil).

Sayı tek başına kolu seçmiyor — ikisi de kare bütçesinin içinde kalırdı;
seçimi **meşruluk** yapıyor. Bedel farkı kararı yalnız pekiştiriyor.

### 4. Ctrl-C'nin prompt kayması — **+1 satır**

Ölçüm 2026-09-20; gerçek `zsh -i` + `pty.fork`, `ROWS=12 COLS=80`, boş
`ZDOTDIR` (`PS1='%% '`, `RPS1=''`, `setopt NO_BEEP`, `compinit`), dizinde
`alfa_00`…`alfa_29`, komut `ls alfa_` + Tab, ekran **dolu** (`seq 1 30`).
Reçete `context.md` → Kanıt; betik ekranın durumunu minimal bir VT izleyiciyle
takip ediyor (imleç satırı, kayma sayısı, satır içerikleri).

| ölçülen | değer |
|---|---|
| Tab'ın bastığı bayt | **312** — `context.md`'nin sayısıyla birebir |
| prompt satırı, Tab öncesi | 11 (ekranın dibi) |
| Tab'ın kaydırdığı satır | **4** (geçmişe düşen) |
| komut satırı, Tab sonrası | 7 |
| liste satırları | 8, 9, 10, 11 → **4 satır** |
| Ctrl-C'nin bastığı bayt | **149** — `context.md`'nin sayısıyla birebir: `\e[?2004l` `\r\r\n` `\e[J` + `PROMPT_SP`'nin satır sonu işareti + prompt + `\e[?2004h` |
| prompt satırı, Ctrl-C sonrası | **8** = komut satırı **+1** |
| ekranın sonunda kalan boş satır | **3** |
| `content_rows` | 9 → `gap = rows − content_rows = 3` |

**`fill` aritmetiği +1 taşımıyor ve taşımamalı.** `\r\r\n` yeni prompt'u komut
satırının bir altına indiriyor, yani listenin açtığı 4 satırın **biri** prompt
tarafından tüketiliyor ve geriye 3 satırlık delik kalıyor. Ama `gap` bu olayın
**sonrasında** ölçülüyor (`rows − content_rows`), yani +1 zaten `gap`'in
içinde: `fill = min(history_size, gap) = min(24, 3) = 3` doğru sayıyı veriyor
ve R2.1'in formülü düzeltme istemiyor.

Bedelin göründüğü yer başka ve phase-2'nin bilmesi gereken cümle bu:
**doldurma ekranı Tab öncesine birebir değil, bir satır eksiğine döndürüyor.**
Ölçülen koşuda Tab öncesi tepe satır `20`, doldurmadan sonraki tepe satır `21`
olur — `20` scrollback'te kalır. Sebep zsh'in `\r\r\n`'si, bir aritmetik seçim
değil: yeni prompt eski komut satırının **üstüne** basılmadığı sürece o satır
geri gelemez ve prompt'un üstüne basmak iptal edilen komutu silerdi. Yanlışın
yönü güvenli — ekran dolu görünür, yalnız bir satır yukarıdan başlar.
