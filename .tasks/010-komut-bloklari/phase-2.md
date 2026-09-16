# Phase 2 — Bloklar `frame()` sınırından geçer

## Özet

`frame()` çıpayı ızgaradan okur, kimlikleri defterden renklendirir ve blok
aralıklarını **çözülmüş** olarak sınırdan verir; çizen taraf henüz yok.

_Requirements: R3, R3.1, R3.2, R3.3, R3.4, R6, R6.1_

## Değişiklikler

- **`crates/bt-core/src/theme.rs`** — **iki** yeni durum rolü: `success` ve
  `error`. Rol burada doğuyor çünkü "renk çözülmüş geçer" kararının ilk
  tüketicisi `frame()`; çizen taraf renk **üretmiyor**, alıyor. Koşan
  bloğun rengi mevcut `accent`'tan gelir, yani üçüncü bir rol eklenmiyor.
  `dim` **yeniden kullanılmıyor**: `CLAUDE.md`'nin "`dim` rolü yalnız
  varsayılan ön planın" cümlesi bağlayıcı. Kalan iki durum rolü (uyarı,
  bilgi) 013'e kalır — çizilmeyen rol eklenmiyor (Karar 4b). Eksik ve
  bilinmeyen anahtarın iki yönde de sessiz kalması korunur;
  `unknown_keys_are_silent` sınamasının örneği artık bilinen bir anahtar,
  sentinel değişir.
- **`crates/bt-core/src/session.rs`** — `frame()` iki fazlı olur.
  **Faz 1**, `Term` kilidi altında: hücre döngüsünde, hücrenin **atlama
  kapısından sonra**, hyperlink'ten kimlik çekilir ve `(aid, ilk_satır)`
  çiftleri çağıranın yeniden kullandığı bir tampona toplanır. Kapıdan sonra
  olması `fg`/`underline_color`'ın kapıdan sonra çözülmesiyle aynı disiplin:
  kapı "çizilecek bir şey var mı" diye sorar. Hyperlink okuması `extra`
  yokken anında dönüyor, yani çizilen hücre başına tek bir boş kontrol.
  **Faz 2**, kilit bırakıldıktan sonra: kimlikler defterden renklendirilir.
  Sıra zorunlu — `session.rs`'in yazılı kuralı yaprak kilidin `Term`
  kilidinin altına girmemesi.
  Blok aralığı: bir kimliğin başladığı satırdan bir sonraki kimliğin bir
  üstüne; son bloğun sonu pencerenin altı. Şerit **prompt satırından**
  başlar, yani blok komutun kendisini de kapsar.
- **`crates/bt-core/src/lib.rs`** — yeni `pub` tip (satır aralığı + renk)
  ihraç listesine ve modül başlığındaki sınır cümlesine girer. Alacritty
  tipi yine görünmez: kimlik `u32`, renk `LinearRgba`.

## Kabul

- Çıkış kodu **sınırı geçmez**: `bt-gpu`'ya yalnız satır aralığı ve renk
  gider (`CLAUDE.md` → karar burada, boyama orada).
- Kaydırma: geçmişe kaydırılmış pencerede blok aralıkları hücrelerle **aynı**
  `display_offset`'ten çıkar; şerit bir kare geride kalmaz.
- Pencere üstü: ilk görünür çıpa `N` ise üstündeki satırlar `N−1`'in.
  `N−1` defterde yoksa (halka dolaştı, sayaç sıfırlandı) o bölge
  **çizilmez**.
- Hiç çıpa görünmüyorken kabuk `Running`'se pencere son `A`'nın bloğuna
  aittir; `Input`'taysa **çizilmez**. Bilinmeyeni yanlış çizmemek bu
  tasarımın savunma tezi ve köşede de tutar.
- **Koşan bloğun rengi defterden değil safhadan gelir:** `D` henüz
  gelmediği için defterde kaydı yok. Bu, "kimlikler defterden
  renklendirilir" kuralının tek istisnası ve tipin şeklinde görünmeli.
  Rengi bugün `accent`; **teslimde kullanıcıya sorulacak tek kalem** — imleç
  rengiyle aynı şerit dikkat dağıtabilir.
- Alternatif ekranda blok verilmez (`TermMode::ALT_SCREEN`; erişim
  `session.rs`'te zaten var).
- Kullanıcının kendi teması okunmaya devam eder; iki yeni anahtarı yazmamış
  tema onları gömülü `bateri`'den miras alır.
- Reflow: pencereyi yatay boyutlandırdıktan sonra aralıklar hâlâ prompt
  satırlarında başlar — çünkü satır ızgaradan okunuyor, hatırlanmıyor.
- Kare başına ayırma yok: tampon çağıranda yaşar ve yeniden kullanılır.

## Yayın Etkisi

- **`CLAUDE.md`** — 009'un erdem diye yazdığı "`frame()` imzası değişmedi"
  cümlesi düşer; `bt-core`'un sorumluluk satırına blok aralığı girer.
- **`crates/bt-core/src/lib.rs`** başlık yorumu: `pub` API listesine yeni
  tip eklenir.
- **tema / materyal biçimi** — iki yeni rol. Gömülü `bateri` ve
  `bateri-light`'ın ikisine de değer girer, `docs/AYARLAR.md`'nin Temalar
  bölümü ve iki tema bloğu aynı commit'te güncellenir
  (`documented_blocks_are_the_embedded_themes` bunu mekanik olarak
  zorluyor). Kendi **açık** temasını yazmış kullanıcı iki rolü koyu
  `bateri`'den miras alır — `dim` ile aynı, kabul edilmiş kusur;
  `docs/AYARLAR.md`'deki o uyarı üç role genişler.
- **`CLAUDE.md`** — "durum rolleri 013 ile gelir" ve "Bugün dördü
  tüketiliyor" cümleleri düzelir; `theme.rs`'in "013'ün durum rolleri"
  notu da.
- `bt-gpu`'nun `Cursor` literalli kare sınamaları mekanik olarak düşebilir;
  aynı commit'te düzelir.
- **shell entegrasyonu** — kapı bulguları zsh betiğine iki düzeltme soktu
  (`emulate -L zsh`, içerme nöbetleri), yani `make kur` bu phase'in de
  doğrulamasına girdi. bash ve fish 009'un bıraktığı yerde; kullanıcının rc
  dosyasına yazılmıyor.
- **Bilinen sınır** (yeni): geçici prompt (`TRANSIENT_PROMPT`) kullanan
  temalarda pencere üstü bölgesi yanlış renk alabilir; gerekçe ve kapatan iş
  (011) `resolve_blocks`'ta.
- Ayar şeması, terminfo, shader, bundle: değişiklik yok. Yeni bağımlılık yok.
- Ölçüm iddiası yok: bu phase kare süresi, gecikme ya da bellek sayısı
  iddia etmiyor, dolayısıyla "ölçüm bekliyor" satırı da yazılmaz.

## Uygulama Notları

- **Çıpasız geri düşüş `display_offset == 0` ile kapılandı.** Phase "hiç çıpa
  yokken kabuk `Running`'se pencere son `A`'nın bloğuna aittir" diyordu ve bu
  yalnız **dipteyken** doğru: geçmişe kaydırılmış bir pencerede görünen
  satırlar koşan komutun değil, çok daha eski bir bloğun çıktısı olabilir ve
  onları `accent`'la boyamak tam da R3.2'nin yasakladığı "yanlış çizme" olurdu.
  Kapı tezin kendisinden türedi, ona rağmen değil.
- **Alternatif ekran bayrağı faz 2'ye taşınıyor.** Yalnız çıpa toplamayı
  kapatmak yetmezdi: vim `Running` safhasında ve çıpası yok, yani geri düşüş
  kolu tam ekranı boyardı. Bayrak `Term` kilidi altında okunup faz 2'ye
  geçiyor (`the_alternate_screen_has_no_blocks` bunu tutuyor).
- **Koşan blok iki koşulun kesişimi:** safha `Running` **ve** defterin son
  kaydı hâlâ açık (`ShellLog::running`). İkincisi olmasaydı kimliksiz bir `A`'dan
  sonra gelen `C` safhayı `Running`'e alır, defterin son kaydı ise bir önceki
  **bitmiş** blok olur ve o blok koşuyormuş gibi boyanırdı.
- **Çizilmeyen dört durum tek `match`'te** (`ShellLog::stripe`): kimlik
  defterde yok, `Pending` ama koşmuyor (boş prompt'a basılan Enter),
  `Finished(None)` (kod okunamadı) ve sıfır yükseklikli aralık. Dördü de aynı
  tezin parçası; ayrı dallara yazılsalardı biri sonradan sessizce
  gevşeyebilirdi. `Finished(None)`'ın çizilmemesinin sebebi nötr bir rolün
  **olmaması**: 013'ün "bilgi" rolü gelene kadar onu `accent` ile taklit etmek,
  koşmayan bloğu koşuyor göstermek olurdu.
- **Tampon tek tip, iki `Vec`** (`Blocks`). Ara defter (çıpalar) ile çıktı
  listesi ayrı ömürlere sahip değil — ikisi de kare başına doluyor ve ikisi de
  ayrılan yeri koruyor — ama ayrı tiplerde: kimlik sınırı geçmediği için tip
  opak, dışarısı yalnız `as_slice()`'ı görüyor.
- **`Blocks` `bt-gpu`'da `Frame`'in yanında, içinde değil.** Aynı çağrıda
  `frame.push` kapatması da tampon da ödünç alınıyor; ikisi tek `RefCell`'de
  olsaydı çalışma zamanında panik ederdi. Şeridi çizecek liste phase-4'te
  `Frame`'in kendi alanı olacak, bu tampon ona **girdi**.
- **`race_shell_state_and_frame` yeni kapının da bekçisi oldu:** `frame()`
  artık `Term`'den sonra `shell`'i de alıyor ve sıra tersine dönerse o sınama
  asılı kalır. Yorumu buna göre güncellendi; ayrı bir yarış sınaması
  eklenmedi, çünkü kapı zaten aynı çifti zorluyor.
- **Kapı üç bulgu verdi, üçü de giderildi** (`/code-review`, bu phase). İkisi
  phase-1'in betiğinde ve ikisi de **tanısız kayıp** üretiyordu, yani bu
  phase'in sınırı doğru çalışsa bile şerit görünmezdi:
  - `psvar[9]` bir dizi indeksi ve kancanın gövdesi kullanıcının
    seçenekleriyle koşuyor: `KSH_ARRAYS` açıkken atama 10. yuvaya düşerken
    `%9v` 9.'yu okuyor, çıpa boş kimlik taşıyor ve bloklar sessizce
    kayboluyordu. Çare `emulate -L zsh`, **`local code=$?`'ın altında**
    (`emulate` de `$?`'ı ezer). `zsh -f` ile doğrulandı.
  - Çıpa nöbetleri konum soruyordu (`== "$açılış"*`, `== *"$kapanış"`); PS1'i
    her `precmd`'de süsleyen bir tema araya girince nöbet tutmuyor ve ek her
    turda bir daha ekleniyordu — PS1 oturum boyunca sınırsız büyürdü. Nöbetler
    `B` ekininkiyle aynı **içerme** biçimine geçti.
  - Üçüncüsü giderilemedi ve **bilinen sınır** olarak koda yazıldı
    (`resolve_blocks`, pencere üstü kolu): çıpa kaybı *aralıklı* olursa
    (p10k'nın `TRANSIENT_PROMPT`'u biten prompt satırını kendi `PROMPT`'uyla
    yeniden basıyor) yalnız canlı prompt'un çıpası kalır ve üstündeki bütün
    pencere `N−1`'in rengine boyanır. `discussion.md`'nin "geri düşüş
    çizilmemedir" cümlesi *tekdüze* kayıp içindi ve o kol doğru çalışıyor;
    aralıklı kayıp ayrı bir mod. Ayırt edecek veri ızgarada yok (çıpasız
    prompt satırı ile çıktı satırı aynı görünüyor) ve "en az iki çıpa iste"
    nöbeti en sık meşru durumu — uzun çıktı, tek görünür prompt — öldürürdü.
    Kapatan iş 011 (prompt'u terminalin çizmesi).
- **Sınamalar gerçek PTY üstünde, sentetik akışla.** zsh gerekmiyor:
  `anchored_prompt`/`ran` yardımcıları betiğin bastığı dizilerin aynısını
  `printf`'le üretiyor, yani ölçüt betiğin değil **sınırın** davranışı.

## Checklist

- [x] `Theme`'e `success` ve `error`; gömülü iki temaya değerler;
      `docs/AYARLAR.md` ve `CLAUDE.md` güncellendi
- [x] Test: tema ayrıştırma — iki yeni rol, eksik anahtar miras alıyor
      (`status_roles_are_read_and_inherited`)
- [x] `frame()` faz 1: kimlik çekme, atlama kapısından sonra, yeniden
      kullanılan tampona
- [x] `frame()` faz 2: kilit bırakıldıktan sonra defterden renk
- [x] Yeni `pub` tip ve `lib.rs` ihracı (`Block`, `Blocks`)
- [x] Test: iki blok, aralıkların sınırı bir sonraki kimliğin bir üstü
      (`blocks_end_one_row_above_the_next_anchor`)
- [x] Test: pencere üstü `N−1` kuralı; `N−1` defterde yokken çizilmiyor
- [x] Test: çıpasız pencere — `Running`'de son bloğa ait, `Input`'ta boş
- [x] Test: alternatif ekranda blok yok
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris`; betik değiştiği için
      ayrıca `make kur`)
- [x] Riskli phase: `/code-review` koştu; iki bulgu giderildi, biri bilinen
      sınır olarak koda ve bu dosyaya yazıldı
- [x] Yayın etkisi yazıldı
