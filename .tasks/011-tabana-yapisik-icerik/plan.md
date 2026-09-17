# Tabana yapışık içerik ve yumuşak kayma

## Hedef

İçerik tabandan tavana doğru büyüsün — ekran dolmadan önce de tabana yapışık
dursun, yukarıda birikmesin — ve yeni satır geldiğinde yumuşak kaysın. Bugün
dolmamış ekran ile dolmuş ekran iki ayrı his; bu set ikisini tek hisse
indiriyor.

**Saf yerleşim işi:** kabuk betiğine, OSC 133'e ve 010'un komut bloklarına
**hiç dokunulmuyor**. Prompt'un devri, `>` ve Input Dock 012'de
(`discussion.md` → Karar 2, 2. tur).

## Gereksinimler

- **R1** — İçerik pencerenin tabanına yaslanır.
  - **R1.1** — Ofset **çizim zamanı** uygulanır; `Frame::pos_at` ve
    `Frame::push_block` değişmez. Birincil yol `setViewport`, kanaryası
    tutmazsa vertex uniform'u. Dört liste (arka plan, glyph, kural, şerit) ve
    imleç tek yerden kayar.
  - **R1.2** — Alternatif ekranda ofset **0**'a zorlanır (vim/htop ızgaranın
    tamamını sahipleniyor). Bayrak `frame()` içinde zaten elde.
  - **R1.3** — Fare eşlemesi **çizilen** ofseti okur. Origin'in tek sahibi
    `DisplayLink`; hesap `frame()`'de (`Cursor.content_rows`), `view` onu
    doğrudan okur. İkisi de ana thread'de, yani atomik ve kilit yok
    (`discussion.md` → Karar 4 eki). Dikey çıkarma **`f64`'te** yapılır; boş
    alan **üstte** ve oraya yapılan tıklama `u16`'da taşar.
  - **R1.4** — Üç bekçi. İkisi CPU→GPU dikişi: offscreen render +
    `set_origin_rows(k)`, boyanan satır **iki pipeline için de** okunur
    (emsal `cell_bg_paints_pixels_on_the_gpu`). Üçüncüsü imlecin `rect.y`'si
    ile aynı satırdaki hücrenin `pos.y`'sinin eşitliği.
- **R2** — Yeni satır geldiğinde içerik yumuşak kayar.
  - **R2.1** — **Tek animatör**: imlecin hedefi **ekran satırı**
    (`row + origin_rows`) olur ve imleç origin ötelemesinden **muaftır**.
    Enter'da imlecin ekran hedefi değişmez, yani salınım doğmaz — imleç dipteki
    satırda kalır, içerik arkasından yukarı akar.
  - **R2.2** — Kayma `cursor_motion`'ın animasyonlu/snap ayrımını izler;
    **yeni ayar anahtarı yok**. `"snap"` içeriği de anında yerine koyar, yani
    `docs/AYARLAR.md`'nin "hareketi tamamen kapatmanın yolu bu" cümlesi ayakta
    kalır.
  - **R2.3** — Hareketi Azalt'ta origin **snap**'ler, belirme **değil**:
    her yeni satırda bütün ekranın belirmesi indirgemeye çalıştığı hareketten
    beter olurdu.
  - **R2.4** — Durma koşulu yazılıdır ve girdisinin (`content_rows`) **monoton
    olmadığı** hesaba katılır: imleci yukarı taşıyıp alt satırı `\e[K` ile
    silen bir program onu daraltıp genişletebilir.
  - **R2.5** — Origin animatörü `Motion::settled()`'ın **içinde** olur — dışında
    kalırsa hasarsız kolda link kayma ortasında uyur ve içerik donar. Jetona
    `kayma=` eklenir; `hareket` yalnız imleç animatörünün tanığı kalır.
  - **R2.6** — `display_offset` bir önceki kareden farklıysa origin **snap**'ler:
    tekerlek parmağı takip eder, 008 Karar 5 ayakta kalır.
  - **R2.7** — Fare (R1.3) origin'i `DisplayLink`'ten okuduğu için kayma
    boyunca da **çizilen** değeri görür; çağrı yeri phase-2'de değişmez.
    Bekçisi: kayma ortasında `point_to_cell`'in döndürdüğü satır, o karede
    çizilen origin'le tutarlı.
- **R3** — Duman kapısı anlamlı kalır.
  - **R3.1** — Reçetenin hedefi **satırı korur, yalnız sütunu oynatır** ve
    mesafesi **tam bir hücre** (`\033[2G`); değişiklik **kod phase'lerinden
    ayrı commit**'le iner. Mesafe sözleşmenin parçası: üç sütun `sessiz`i
    `QUIET_FLOOR`'un türetme kuralının altına indiriyor (phase-0 → Uygulama
    Notları).
  - **R3.2** — `hucre=8 glif=6 kural=15` oynamaz; `icerik ≤ IDLE_FRAME_LIMIT`
    ve `sessiz ≥ QUIET_FLOOR` korunur.

## Yaklaşım

1. **Reçete önce, tek başına.** `smoke_shell`'in ikinci `printf`'i `\033[2G`
   olur. Bugünkü kodda da hareket üretiyor (imleç satır 1'de kalıyor, sütun
   0→1), yani tek başına doğrulanabilir; ve tabana yapışma indiğinde `\033[H`
   sıfır ekran hareketi üreteceği için kapıyı **kod doğruyken** kırmızıdan
   kurtarır. Ayrı commit: ölçülmüş bir sözleşme kod değişikliğiyle aynı
   commit'te oynarsa regresyonu maskeler.
2. **Tabana yapışma, animasyonsuz.** `frame()` `content_rows`'u döngüde
   toplar — atlama kapısından geçen en büyük `row` ile `cursor_row`'un
   maksimumu; ikisi birden gerekli, çünkü imleç tek başına yetmez (imleci
   yukarı taşıyan ilerleme çubuğu içeriği aşağı iterdi) ve kapı tek başına
   yetmez (boş prompt satırı kapıdan geçmiyor). Değer `Cursor` ile döner;
   `link.rs` `frame()` **döndükten sonra** origin'i kurar. Çizim
   `setViewport` ile kayar: iki pipeline birden, shader'a dokunmadan.
   `CursorBlock.rect`'e origin **CPU'da** eklenir, çünkü `[[position]]`
   viewport dönüşümünden sonraki koordinattır.
3. **Kayma.** Origin `Motion`'ın içine girer. İmlecin hedefi ekran uzayına
   taşınır, böylece iki animatör birbirini götürür. Hasarsız kolda origin'in
   **ikinci yazma noktası** açılır — o kolda `frame()` de `clear` de
   çağrılmıyor, yani 3c'nin "korunur" tespiti doğru ama animasyon
   *değişmeyi* gerektiriyor. Jetona `kayma=` eklenir.

## Kapsam Dışı

- **Prompt'un devri, `>` şekli ve Input Dock** → 012. Bu setin bulguları
  orada kullanılacak: `anchor_close`'un preexec'e taşınması (alacritty
  kaynağında doğrulandı), PS1/RPS1 sıfır genişlik, safha kapısı, ayna
  modelinin ölçülmüş bedeli.
- **Tuş vuruşu ve silme animasyonları** (`keypress`, `delete_mode`) — aynanın
  üstüne kurulur, 012.
- **Ekran dolduktan sonraki besleme kayması** — grid satırlarının kayması
  başka bir mekanizma (yumuşak kaydırma borcu, `docs/YOL-HARITASI.md`).
  **Dikiş adıyla konur:** ekranın dolduğu anda kayma durur.
- **Ayar şeması** — yeni anahtar yok (R2.2).
- **`ShellPhase`'in üretim tüketicisi** — 012'nin işi; bu sette hâlâ yok.

## Akış

```
Session::frame()                      bt-core
  ├─ döngü: content_rows = max(çizilen en büyük row, cursor_row) + 1
  │         alt ekranda 0'a zorlanır                          (R1.2)
  └─ Cursor { …, content_rows } ──┬──► Adapter atomiği ──► point_to_cell (R1.3)
                                  │    (animasyonlu origin yazılır, R2.7)
                                  ▼
DisplayLink                       bt-gpu
  ├─ Motion: imleç hedefi EKRAN satırı (row + origin_rows)    (R2.1)
  │          origin aynı settled() kapısında                  (R2.5)
  │          display_offset oynadıysa snap                    (R2.6)
  ├─ hasarlı kol  : frame() + clear + origin yaz
  └─ hasarsız kol : move_cursor + origin'in İKİNCİ yazma noktası
                                  │
                                  ▼
encode_pass: setViewport(originY)  ──► cell_bg + cell, ikisi birden   (R1.1)
             CursorBlock.rect'e origin CPU'da eklenir
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-0 | ✅ |
| phase-1 | ✅ |
| phase-2 | ✅ |
| kapı | ✅ |
