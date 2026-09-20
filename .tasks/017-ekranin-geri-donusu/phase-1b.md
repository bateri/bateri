# Phase 1b — Bayrağın iki kör noktası

## Özet

Alternatif ekranın `CSI 2 J`'si bayrağı **kurmaz**, ve bayrağın düşme ölçütü
dock'lu pencerede erişilebilir olur.

_Requirements: R1.2 (tadil)_

## Neden ayrı phase

phase-2 iki waive getirdi ve **orkestratör ikisini de reddetti** (2026-09-20).
Tek tek her biri "yönü güvenli" görünüyor (doldurma yapmamak bugünkü
davranış), ama **birlikte** özelliği ilk `vim` kullanımından sonra **kalıcı
olarak** kapatıyorlar:

1. `vim` açılışta `CSI 2 J` basıyor → bayrak kuruluyor.
2. Dock'lu pencerede düşme ölçütü (`content_rows == rows`) **erişilemez**:
   caret dock'a devrildiğinde doluluk giriş satırını saymıyor
   (`content_rows = drawn_rows.max(1)`), yani tavanı `rows - 1`.
3. Sonuç: bayrak oturum boyunca asılı kalıyor, altı phase'lik iş ekranda hiç
   görünmüyor.

`duzen.md` → Ek phase eşiği: iki dosyaya yayılıyor, kendi doğrulaması
(`make test-yaris`) ve kendi bekçileri var → tek commit'lik düzeltme değil,
phase.

## Değişiklikler

- **`crates/bt-core/src/session.rs` — Fix A: alternatif ekranda kurma yok.**
  `observe_screen_clear` zaten `Term` kilidi altında ve `alt_screen` aynı
  okumadan geliyor. Nesil **tüketilir** (yoksa alt ekrandan çıkışta birikmiş
  sayaç bayrağı kurardı), bayrak **kurulmaz**.
  Semantik alacritty'den doğrulanıyor: `ClearMode::All` ALT_SCREEN altında
  `reset_region(..)` çağırıyor, `clear_viewport()` **değil** — yani geçmiş
  büyümüyor ve birincil ekranın durumuna hiç dokunulmuyor. Geri getirilmeyecek
  bir şey yok, dolayısıyla bayrağa da gerek yok.
- **`crates/bt-core/src/session.rs` — Fix B: düşme ölçütü.**
  `content_rows == rows` yerine **`history_size` bayrağın kurulduğu andakinden
  büyük mü**. Damga bayrak kurulurken alınır (`Term` kilidi altında,
  `history_size()` zaten elde).
  Ölçüt "ekran doğal yoldan doldu"nun **gerçek** karşılığı: temizlemeden
  **sonra** geçmişe bir satır düştüyse, geçmişin en yeni satırları artık
  temizleme öncesine ait değildir — doldurma o noktadan sonra Ctrl-L'i geri
  almaz. `content_rows`'un dock körlüğünden de etkilenmiyor.
  **Bu bir borç sayacı değil** (panelin elediği 1b): tek `usize` damga, tek
  karşılaştırma, `fill` formülü (R2.1) **değişmiyor**.
- **`display_offset == 0` koşulu kalır.** Fix B onu gereksiz kılıyor (kaydırma
  `history_size`'ı değiştirmiyor) ama bekçisi
  (`scrolling_into_history_never_drops_the_flag`) yeşil kalmalı ve koşul
  ucuz — kaldırmak ikinci bir sınama borcu doğururdu.
- **R1.2 tadil edilir** (`plan.md`) ve `CLAUDE.md`'nin bayrak cümlesi düzeltilir.

## Kabul

- Birincil ekranda bayrak kurulu → `\e[?1049h` → `\e[2J` → `\e[?1049l`:
  bayrak **kurulduğu gibi** kalıyor (ne yeniden kuruluyor ne düşüyor).
- Birincil ekranda bayrak temiz → aynı dizi: bayrak **temiz kalıyor**
  (vim'den çıkışta doldurma çalışır).
- Ctrl-L → 20 satırlık çıktı → Tab → Ctrl-C: **`fill > 0`**. Bu, waive'lerin
  reddinin ölçüldüğü yer; dock'lu oturumda koşar
  (`race_screen_clear_and_frame` phase-2 §3'te dock'lu oldu).
- Ctrl-L → `echo hi` (ekran kaymıyor, geçmiş büyümüyor) → Tab: `fill == 0`.
  Doğru cevap bu: geçmişin en yenileri hâlâ temizleme öncesine ait.
- phase-1'in bekçileri yeşil kalıyor, özellikle
  `scrolling_into_history_never_drops_the_flag` ve
  `race_screen_clear_and_frame`.
- `make hepsi` ve `make test-yaris` yeşil.

## Yayın Etkisi

- **`CLAUDE.md`:** bayrağın ömrünü anlatan cümle — düşme ölçütü artık geçmişin
  büyümesi, ekranın dolması değil — ve doldurmanın formülü (üçüncü terim).
- **`plan.md`:** R1.2 **ve** R2.1 tadil (üçüncü terim, `/code-review`).
- shader / terminfo / ayar şeması / tema / shell entegrasyonu / app bundle /
  yeni bağımlılık / ölçüm borcu: yok.

## Checklist

- [x] Fix A: alternatif ekranda nesil tüketiliyor, bayrak kurulmuyor
- [x] Fix B: düşme ölçütü `history_size` damgası (aynı damga `fill`'i de
      kırpıyor — `/code-review`, R2.1 tadil)
- [x] R1.2 ve `CLAUDE.md` tadil edildi
- [x] Test: alt ekran turu bayrağı ne kuruyor ne düşürüyor (iki başlangıç
      durumu için de)
- [x] Test: Ctrl-L → 20 satır → Tab → Ctrl-C → `fill > 0`
- [x] Test: Ctrl-L → `echo hi` → Tab → `fill == 0`
- [x] phase-1 bekçileri yeşil
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris`)
- [x] Riskli phase: `/code-review` koştu (paylaşılan durum); üç bulgunun üçü
      de giderildi (`fill` kırpması, `full` kolunun kaldırılması, doymuş
      defterin bilinen sınır olarak yazılması)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

### 1. Ölçüm: reçete düzeltmeden **önce** kırmızı

Kabul'ün en kritik maddesi (`Ctrl-L → 20 satır → Tab → Ctrl-C`) hermetik
tanığıyla önce **düzeltilmemiş** kodda koştu
(`a_grown_history_lets_the_gap_fill_again`, dock'lu oturum, 2026-09-20):

| an | ölçülen |
|---|---|
| Ctrl-L sonrası | `screen_cleared = true` |
| 20 satır çıktıdan sonra | `screen_cleared` **hâlâ** `true` |
| delik açıldıktan sonra | `gap = 5`, `fill = 0`, ikinci sink hiç çağrılmadı |

Düzeltmeden sonra aynı sahne `fill = 5`, yani boşluğun tamamı doluyor.

İkinci ölçüm aynı sınamanın kurulumundan çıktı ve §7b'yi **sayıyla**
doğruluyor: beklemenin ölçütü `content_rows == rows` yazıldığında sınama
zaman aşımına düşüyor. Dock'lu pencerede `seq 1 20`'den sonra imleç boş bir
alt satırda duruyor, `drawn_rows` dokuzda kalıyor ve doluluk `rows`'a **hiç**
çıkmıyor. Bekleme bu yüzden son satırın mürekkebine bakıyor.

### 2. **Damga bir kare geç alınıyor** — plandan sapma

Plan "damga bayrak kurulurken alınır" diyordu; uygulamada damga bayrak
kurulduktan **sonraki** ilk karede alınıyor (`Session::UNSTAMPED` sentinel'i,
`usize::MAX`). Gerekçe phase-1'in kendi yarışı: okuyucu thread nesli
`advance`'ten **önce** artırıyor, yani bayrağı kuran kare ızgarayı
temizlenmeden önce görebiliyor. `CSI 2 J` birincil ekranda görünen satırları
geçmişe **itiyor** (alacritty `clear_viewport` → `scroll_up`), yani o karede
alınan damga bir sonraki karede anında aşılır ve bayrak hemen düşerdi —
Ctrl-L'i geri alan dizi tam olarak bu. Bedeli o bir karede gelmiş fazladan
satırların damgayı biraz yükseltmesi, yani bayrağın biraz **uzun** yaşaması;
yanlışın yönü güvenli.

Aynı sebeple **damga alternatif ekranda alınmıyor**: `history_size()` etkin
ızgaradan geliyor ve alternatif ekranda sıfır. Ctrl-L'den hemen sonra `vim`
açılsaydı damga sıfır olur, çıkışta birincil ekranın defteri onu anında aşar
ve bayrak yanlışlıkla düşerdi. Damgasız bayrak düşmüyor, çünkü sentinel bir
**tavan**.

Tanığı `the_stamp_waits_for_the_frame_that_sees_the_clear` ve
`the_alternate_screen_neither_arms_nor_stamps_the_flag`; ikisi de damga
kurulurken alınınca kırmızıya düşüyor (ölçüldü).

### 3. `/code-review`'un bulgusu: **`fill` formülü bir terim kazandı** (R2.1 sapması)

Bayrağın düşmesi "boşluğun tamamı geri verilebilir" demiyor: ölçüt **tek
satırlık** bir büyüme, doldurma ise `gap` satır çekiyor. Aradaki fark doğrudan
kullanıcının sildiği ekran ve ölçüldü (2026-09-20, dock'lu oturum, 10 satır):

| adım | ölçülen |
|---|---|
| `seq 1 30` | defter 21 |
| Ctrl-L (`\e[2J\e[H`) | defter 30, bayrak kurulu |
| `seq 1 12` | defter 33 (üç satır), bayrak **düştü** |
| `\e[6A\e[J` | `gap = 7`, `fill = 7` |
| doldurulan satırlar | `["27","28","29","30","1","2","3"]` |

İlk **dördü** temizleme öncesine ait — yani Ctrl-L kısmen geri alınmış. Çare
damganın **ikinci kez** okunması: `fill_rows` onu "temizlemeden beri gelen
satır" olarak alıp `fill`'i kırpıyor. Aynı sahne kırpmayla `["1","2","3"]`
veriyor. Yeni sayaç yok, yeni alan yok — aynı `usize`, ikinci tüketici.

Bu R2.1'den sapma ve plandaki "`fill` formülü değişmiyor" cümlesini tadil
ediyor; R2.1 aynı commit'te düzeltildi. Hiç temizleme olmamış oturumda kırpma
**no-op**: damga `UNSTAMPED` ve taze satır sayısı defterin tamamı, yani
`min(history_size, gap)`. Tanığı
`the_fill_stops_at_the_rows_that_arrived_after_the_clear`.

**Bayrak kırpmayla birlikte de gerekli:** kurulduğu karede damga henüz
alınmamış (`UNSTAMPED`, bkz. §2) ve orada "taze satır" hesaplanamıyor —
bayrak o kareyi kapatan kapı. Kapı "hiç" der, kırpma "ne kadar".

### 3b. `content_rows == rows` kolu **kalktı** (ilk uygulamada eklenmişti)

İlk uygulama onu ikinci bir kol olarak tutmuştu; gerekçe defterin `scrollback`
tavanında doyması ve damga karşılaştırmasının orada ölü kalmasıydı.
`/code-review` iki kusurunu gösterdi ve ikisi de ölçülebilir:

1. Gerekçe tutmuyor — `content_rows == rows` dock'lu **boşta** promptta
   erişilemez (§7b'nin ta kendisi), yani doymuş defterde de güvenilmez.
2. Kırpma geldikten sonra kol **ölü**: doymuş defterde taze satır sayısı sıfır,
   yani `full` bayrağı düşürse bile `fill` sıfır çıkıyor. Kırpmasız hâlde ise
   zararlı olurdu — doymuş defterde bayrağı düşürüp **tamamen** temizleme
   öncesi satırları geri getirirdi.

Kol kaldırıldı ve kod plandaki ölçüte döndü. Doymuş defter bir **bilinen
sınır** olarak kaldı: tek damgayla kapatılamıyor, çünkü gereken şey doymuş
defterde de artan bir "geçmişe itilen satır" sayacı ve `history_size` ondan
türetilemiyor. Yönü güvenli ve phase-1'e göre gerileme değil (bayrağın
phase-1'deki ömrü dock'lu pencerede zaten erişilemezdi). Gözcüsü
`a_saturated_history_never_drops_the_flag` — iddia etmiyor, **gözlüyor**.

### 4. Fix A'nın bilinen sınırı: `alt_screen` de tek okumadan

phase-2 §7a'nın uyardığı hâl duruyor ve kapatılmadı: `alt_screen` da nesil
gibi `Term` kilidinin altındaki tek okumadan geliyor, yani mod değişimiyle
`CSI 2 J`'yi **aynı** PTY okumasında taşıyan bir tur iki yönde de yanılabilir.

- `?1049h` + `2J` (her `vim` açılışı): bir kere fazladan kurabilir.
  **Kendi kendini onarıyor** — birincil ekranda defter büyüyünce bayrak
  düşüyor, yani Fix B bu kolu kapatıyor. phase-2'nin elinde olmayan özellik bu.
- `?1049l` + `2J`: gerçek bir temizlemeyi atlayabilir. Yönü kötü ama böyle
  basan uygulama yok ve kapatmanın bedeli uygulanan bayta kanca takmak
  (alacritty'de kanca yok).

İkisi de `Session::observe_screen_clear`'ın doc'unda "Bilinen sınır 2" olarak
duruyor. Yanına bir üçüncüsü yazıldı: `2J` ile `3J` ayrı PTY okumalarına
düşerse damga `3J` öncesi boydan alınır ve `3J` defteri sıfırladığı için
bayrak defterin o eski damgayı yeniden aşmasını bekler. Pratikte olmuyor
(`clear(1)` üçünü tek `write` ile basıyor) ve çaresi `>` yerine `!=`
**değil**: pencereyi büyütmek geçmişten satır çekiyor, yani defter
küçülebiliyor ve `!=` orada bayrağı düşürüp temizleme öncesi satırları geri
getirirdi.

### 5. `race_screen_clear_and_frame`'in yüklemi **daraldı** — plandan sapma

Phase "phase-1'in bekçileri yeşil kalmalı, özellikle
`race_screen_clear_and_frame`" diyordu; bekçi yeşil kalıyor ama **yüklemi
değişti** ve değişmek zorundaydı. Eski yüklem "ilk temizlemeden sonra ekran
dolu değilse bayrak kurulu olmak zorundadır" (`content_rows < rows`) —
yani bayrağın **düşme koşulunu** yazıyordu. O koşul dock'lu pencerede
erişilemezdi (§7b), yani bayrak yarış boyunca asılı kalıyor ve yüklem
bedavaya doğru çıkıyordu. Fix B'den sonra betiğin `seq 1 12`'si defteri her
turda büyütüyor ve bayrak **meşru olarak** düşüyor; ölçülen kırmızı tam olarak
bu (`content_rows = 9`, `rows = 10`, `fill = 1`).

Yeni yüklem "ekran **taze temizlenmişse** bayrak kurulu olmak zorundadır"
(`content_rows == 1`, betiğin `sleep 0.01` penceresi) ve **daha güçlü**:
defterin büyümesi ekranın dolmasını gerektirdiği için o pencerede bayrağın
düşmesinin meşru bir yolu yok, ve kayıp bir bayrak artık bir önceki turdan
kalma kurulu bayrakla örtülemiyor. Bekçi çevrilebilir kaldı — bayrağı tek kare
yaşatan bir düzenlemede 3/3 kırmızı (ölçüldü, 2026-09-20).

