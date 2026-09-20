# Ekranın geri dönüşü

## Hedef

Tab tamamlama listesi kapandığında ekran **Tab'dan önceki hâline dönsün**, ve
dönüş **kayarak** olsun — bugün yeni içerik gelince aşağıdan yukarı akan
hareketin tam tersi yönde. Kullanıcı ekranı **kasten** temizlediyse (Ctrl-L,
`clear`) dönmesin.

## Gereksinimler

- **R1 — Kasten temizleme ayırt edilir.**
  - **R1.1** — PTY tarayıcısı `CSI 2 J`'yi tanır ve "ekran kasten temizlendi"
    bayrağını kurar. `3J` ve RIS için kol **yok**: ikisi de geçmişi siliyor,
    doldurma kendiliğinden kapanıyor.
  - **R1.2** — Bayrak, ekran doğal yoldan yeniden dolunca düşer
    (`content_rows == rows`, alternatif ekranda değilken **ve**
    `display_offset == 0`). Üçüncü koşul phase-1'de eklendi
    (`/code-review`): `content_rows` görünür pencereden doğuyor, yani
    geçmişe kaydırılan pencere dolu **görünür** ve onsuz tek bir tekerlek
    jesti bayrağı kalıcı olarak düşürürdü.
  - **R1.3** — Tarayıcının yeni durumu vte'nin iptal kurallarını taşır: `ESC`
    → `Escape`, `0x18`/`0x1A` → `Ground`, parametre uzunluğuna tavan. Bozuk
    bir CSI'da takılıp arkasından gelen `ESC ] 133;…`'ü **yutmaz**.
  - **R1.4** — Tarayıcı baytlara dokunmaz; akış aynen geçer.
- **R2 — Boşluk geçmişle dolar.**
  - **R2.1** — `fill = min(history_size, gap)`, geçmişin **en yeni** satırları,
    içeriğin hemen üstüne.
  - **R2.2** — Koşul: pencerenin dock'u var **∧** alternatif ekranda değil
    **∧** bayrak temiz **∧** `display_offset == 0`. Safha kapısı **yok**.
  - **R2.3** — `content_rows` ve `origin = rows - content_rows` aritmetiği
    **değişmez**; `session.rs:6283`'ün bekçisi dokunulmadan geçer.
  - **R2.4** — `fill_rows()` sıfır dönerken sınırdan geçen kare bugünküyle
    **bit bit** aynıdır (geri alma şeridi, 016'nın "yarıçap 0, hale 0"
    örüntüsü).
  - **R2.5** — `bt-core`'da panik yok: geçmişe iniş `grid_clamp`/`Boundary`
    ile sınırlı, negatif satır sınırdan geçmez.
- **R3 — Doldurma encode zamanı konumlanır.**
  - **R3.1** — Hareket karesinde (listeler korunur, yalnız `origin_px`
    değişir) doldurma ızgarayla **birlikte** kayar; push anında pişmiş bir
    konum bırakılmaz.
  - **R3.2** — Doldurma `hucre=`/`glif=`/`kural=` sayaçlarına **girmez**
    (dock örüntüsü); duman sözleşmesi `hucre=8 glif=6 kural=15` bit bit
    korunur.
- **R4 — Dönüş kayarak olur.**
  - **R4.1** — `fill > 0` iken yükselen öteleme hedefi de süzülür; istisna
    `motion.rs`'teki guard'ın **`!snap` içinde** üçüncü terimidir.
  - **R4.2** — Tekerlek ve geometri (pencere/font/punto) **snap'lemeye devam
    eder**.
  - **R4.3** — Yeni animatör ve yeni durma koşulu yok; `Slide::settled()`
    kapısı aynen geçerli, boşta sıfır kare korunur.
- **R5 — Doldurulan alanda seçim yanlış çalışmaz.**
  - **R5.1** — `fill > 0` iken orijinin üstündeki tıklama **reddedilir**
    (`None`); 0. satıra kırpılmaz.
  - **R5.2** — `fill == 0` iken bugünkü kırpma davranışı korunur.
- **R6 — Belgeler kodla birlikte düzelir.** `CLAUDE.md`'nin "kayma tek
  yönlüdür" cümlesi ve liste sayısı, `docs/AYARLAR.md`'nin "yalnız yukarı
  kayar" maddesi, 011'in karar kaydı, `docs/YOL-HARITASI.md`'nin borç maddesi.

## Yaklaşım

1. **phase-0 — ölçüm ve kanarya.** Metal negatif `originY`'yi kabul ediyor
   mu? Cevap 2b'nin alt kolunu seçiyor: meşruysa üçüncü `setViewport`
   (dock'un birebir kopyası), değilse okuma anı çeviri (caret emsali,
   `frame.rs:1215`). Ayrıca Ctrl-C'deki `\r\r\n`'den gelen +1 prompt kayması
   ölçülür.
   **Ölçüldü (2026-09-20): negatif `originY` meşru** — Apple M1 Pro /
   macOS 26.4.1, API doğrulama katmanı açıkken de kabul ediliyor ve üstte
   kalanı kırpıyor. **phase-3 kolu 2b-i** (üçüncü `setViewport`); 2b-ii
   elendi. Ctrl-C prompt'u **+1 satır** indiriyor ama `gap` onu zaten
   içerdiği için `fill` aritmetiği düzeltme istemiyor — sayılar
   `phase-0.md` → Uygulama Notları.
2. **phase-1 — bayrak.** Tarayıcıya CSI kolu, `2J` bayrağı, ömrü ve yaşadığı
   yer (yarış kararı). Doldurmadan **önce** iner: ters sırada tek commit
   boyunca Ctrl-L geri alınmış görünürdü.
3. **phase-2 — doldurma, sınır tarafı.** `fill` hesabı, geçmişten okuma,
   sınırdan ayrı liste olarak geçiş, `Cursor`'a `fill` alanı.
4. **phase-3 — doldurma, çizim tarafı.** phase-0'ın seçtiği kol; `Frame`
   listeleri, sayaç muafiyeti, encode sırası.
5. **phase-4 — kayma.** `motion` guard'ının üçüncü terimi, bekçinin yeniden
   adlandırılması, belge tadilleri.
6. **phase-5 — seçim.** `point_to_cell`'in reddi, `view.rs:741`'in yeniden
   yazımı.

## Kapsam Dışı

- Listeyi ızgaraya hiç düşürmemek (overlay / aynanın altıncı kanalı) —
  `docs/YOL-HARITASI.md:389`. Bu set onu **küçültüyor**, kapatmıyor.
- Doldurulan satırların seçilebilmesi (`Cell.row`'u negatife açmak).
- Ötelemeyi `setViewport`'tan pass uniform'una taşımak.
- Bastırmanın tazelik kapısının prompt hücrelerini sayması
  (`docs/YOL-HARITASI.md:203`).

## Akış

```
PTY baytları
  └─ Scanner::feed ──┬─ ESC ] … OSC kolları (133, 8133, 7)      [bugün]
                     └─ ESC [ … 2J → cleared bayrağı             [R1]
                              └─ baytlar aynen Term'e

frame()  (Term kilidi)
  ├─ hücreler + content_rows                                     [değişmiyor]
  ├─ gap = rows - content_rows
  ├─ fill = min(history, gap)   ← dock ∧ !alt ∧ !cleared ∧ ofset 0   [R2]
  └─ geçmişin son `fill` satırı → ayrı sink

bt-gpu
  ├─ Frame: fill listeleri (dock örüntüsü, sayaçlardan muaf)     [R3]
  ├─ motion: origin hedefi yükselirken `fill > 0` ise süzülür    [R4]
  └─ encode: ızgara → doldurma → dock

bt-shell
  └─ point_to_cell: fill > 0 iken orijinin üstü None             [R5]
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-0 | ✅ |
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | |
| phase-4 | |
| phase-5 | |
| kapı | |
