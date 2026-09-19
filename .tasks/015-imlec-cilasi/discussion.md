# İmleç cilası — Tartışma

Yedi karar noktasıyla açıldı; panel **dördünü kapattı, ikisini yeniden yazdı,
bir tanesini de yeni bir soruyla değiştirdi**. Aşağısı panel sonrası hâl;
nelerin nasıl değiştiği `## Muhakeme`'de.

## Karar 1: Caret nasıl çizilecek? → ✅ kardeş fragment, yeni tip yok

Yarıçap, hale ve içi boş kenar üçü de fragment'te bir mesafe alanı (SDF)
istiyor. İlk taslak "ayrı `.metal` dosyası + yeni `CaretInstance`" diyordu;
panel daha ucuzunu gösterdi ve gerekçemin kendi belgemle çeliştiğini yakaladı.

**Karar:** `cell_bg.metal`'de **kardeş bir fragment** (`caret_fragment`),
`cell_bg_vertex` ve `Instance` **aynen** kullanılıyor; yeni pipeline yalnız bir
`pipeline()` çağrısı ve bir `encode_*`. Şekil parametreleri (yarıçap, hale
payı, kenar kalınlığı) küçük bir **uniform**'dan geliyor; iç dikdörtgen olarak
zaten var olan `CursorBlock` okunuyor — o pencere uzayında ve `[[position]]`
ile aynı uzayda (`cell.metal`'in ters çevirmesi de öyle yapıyor).

Kazandıkları: **yeni `#[repr(C)] ↔ .metal` çifti yok**, yani hizalama tuzağı
(`[f32;4]` Rust'ta 4, MSL'de 16 hizalı) hiç doğmuyor; yeni dosya yok; caret
zaten ayrı draw call ve ayrı tampon olduğu için değişen tek şey pipeline takası.

## Karar 2: Yuvarlak köşe ters çevirmeyi bozar mı? → ✅ hayır, kapandı

İlk taslak "harfin köşelerinde ters çevrilmiş lekeler" diye bir kusur öngörüp
`CursorBlock`'u 32 → 48 bayta çıkarmayı öneriyordu. **Kusur yok** ve bu
aritmetikle gösterildi, gözle değil:

- Ters çevirme o pikseli `cursor.rgba.rgb` ile boyuyor (`cell.metal`).
- O renk `Cursor::text` ve `Cursor::text` **bugün temanın zemini**
  (`session.rs`).
- Yuvarlanan köşeyi caret boyamıyor, arkada duran da temanın zemini.

Zeminin üstünde zemin: görünmez. **`cell.metal` hiç açılmıyor**, 32 baytlık
sözleşme duruyor.

**Geçerlilik koşulu yazılı olmalı:** eşitlik `Cursor::text == tema zemini`
olduğu sürece geçerli. İmlecin altındaki metne ayrı bir tema rolü verilirse bu
karar **yeniden açılır**. Bugün görünür olabildiği tek yer, caret'in altındaki
hücrenin varsayılan dışı bir zemini olması (seçim) ve orada da alan köşe başına
birkaç piksel — göz kontrolünün listesine giriyor.

*Yeniden açılırsa çare `CursorBlock`'a alan eklemek **değil**:* o yol köşe
matematiğini `cell.metal`'e ikinci kez yazdırır ve iki dosya alt piksel
hassasiyetinde anlaşmak zorunda kalır — hiçbir assert görmez. Doğru yol paylaşılan
bir başlık (`shaders/caret_sdf.h`); `build.rs` dizini izlediği için bedeli sıfır.

## Karar 3: Hale hangi yuvada çizilecek? → ✅ bugünkü yuva

Sıra: şeritler → arka planlar → **caret** → glyph + kural → dock. Glyph'lerden
sonraya taşımak komşu metni karartır, kendi geçişini vermek bir encoder turu
daha ister. Hale "hafif bir dokunuş" olarak istendi, metnin üstüne basması
istenen şey değil.

**Bedeli kayda geçiyor:** caret arka planlardan **sonra** çizildiği için hale
komşu hücrelerin **arka planını** da (seçim vurgusu) soldurur. Göz kontrolünde
bakılacak.

## Karar 4: Şekil sayıları nereden gelecek? → ✅ türetilecek, uydurulmayacak

İlk taslak yarıçap ve hale payını yeni sabitler olarak kuruyordu. Panel bunu
**yeniden icat** diye işaretledi ve haklı: `CLAUDE.md`'nin dock payı kuralı
"payın kaynağı sol payın ta kendisi — ikinci bir tasarım sabiti yok" diyor,
chevron da kalınlığını alt çizgi metriğinden alıyor, 014'ün caret kalınlığı da
öyle.

**Karar:** kenar kalınlığı `CellMetrics::rule_px`'ten, hale payı
`gutter_px`'ten, yarıçap hücre ölçüsünün bir oranından. Yan kazanç bedava:
Cmd +/− ile punto büyüyünce üçü de kendiliğinden ölçekleniyor — piksel olarak
yazılan bir pad puntoyla sessizce yanlışlaşırdı. Türetilemeyen kalırsa `const`
doc'unda "seçilmiş, ölçülmemiş" + **hangi metriğin neden yetmediği**.

**Şart:** yuva seçimi (`Frame::push_caret`) **şişmemiş** dikdörtgene bakmaya
devam etmeli. Hale payı ayak izini büyütüp caret'i dock yuvasına kaydırırsa,
caret ızgaranın glyph'lerinden **sonra** çizilir ve altındaki harfi boyar —
`frame.rs`'in doc'unun adıyla uyardığı kusur, 014 phase-1'de aynı tuzağa
düşülmüştü.

**Kırpma sınırları, beşi birden:** ızgara caret'inin dock bandına taşanı opak
`dock_ground` ile **örtülür**; dock caret'inin yukarı taşanı `origin_y`'de
**kırpılır**; tabana yapışık içerikte üst satırın yukarı halesi viewport'ta
**kırpılır**; ızgaranın son satırı ile `origin_y` arasındaki artık şeritte ne
kırpılır ne örtülür — **kalır**; ve yukarıdaki komşu arka plan solması. Beşi de
kusur değil **sınır**, ama yazılı olacak.

## Karar 5: Hale blink alfasıyla sönecek mi? → ✅ zaten garantili, bekçi gerekli

Renkler tek bir `with_alpha` noktasından geçiyor ve `push_caret`'e gelen alfa
`motion.alpha() * blink.alpha()`. Yani hale de çarpılır — yeni bir mekanizma
değil, **regresyon bekçisi** gerekiyor.

**Şart:** bekçi dikdörtgenin **dışını** örneklemeli. Mevcut
`cursor_alpha_is_blended_on_the_gpu` yalnız caret'in kendi hücresine bakıyor,
yani "caret söndü ama hale kaldı" belirtisini **göremez** — onu çoğaltmak
bekçi taklidi olurdu.

## Karar 6: Odak nereden geçecek, içi boş imleç nasıl temsil edilecek?

**Karar:** odak `bt-core`'a **hiç girmiyor**; `DisplayLink::set_focused(bool)`
ile doğrudan `bt-gpu`'ya iniyor. `AppDelegate` zaten `NSWindowDelegate`, yani
`windowDidBecomeKey:` / `windowDidResignKey:` iki yeni metot.

*Emsal düzeltildi:* `set_visible` **yanlış emsaldi** — o bir **ritim** kolu
(`Gate` + `setPaused`) ve odak ikisine de dokunamaz; odaksız pencere çıktı
çizmeye devam eder. Doğru emsal `blink.content_frame(now, cursor.blink &&
!motion.reduce())`: `bt-gpu` orada zaten `bt-core`'un bir kararını shell→gpu bir
`bool` ile `AND`'liyor.

**İçi boş imleç `CaretShape`'e eklenmiyor:** o enum ayar dosyasının sözlüğü
(`"block" | "underline" | "beam"`) ve odak şekle dik bir eksen. `bt-core`'un
`caret_shape_of`'undaki `HollowBlock` kolu da bugün ölü (alacritty onu
kendiliğinden üretmiyor).

**Duman kapısı için zorunlu iki koşul** (panel: odak, kapının **dördüncü**
yanlış pozitifi olur ve üçünden kolay tetiklenir — bildirim, Spotlight, başka
pencereye tıklama):
- Hermetik koşuda odak **hiç okunmuyor**; emsali `resolve_reduce_motion`
  ("kapı bir makinede yeşil bir makinede kırmızı düşerdi").
- `set_focused` **aynı değerde no-op**; emsali `Session::set_theme`. Yoksa
  açılıştaki key olayı bedava bir içerik karesi yazar.

### Çözülen soru: odaksız caret söner mi? → ✅ sönmez, sabit kalır

**Karar (2026-09-19, kullanıcı):** *"Dursun, sabit kalsın."* Gerekçe: blink
dikkat çekmek için var; kimsenin bakmadığı pencerede koşması boşa yanan pil.
İçi boş imleçle de aynı sinyali paylaşıyor — ikisi "bu pencere seni
beklemiyor" diyor.

**Bedel kabul edildi:** `bt-gpu`, `Cursor::blink`'in ikinci sahibi oluyor.
Karşılığında odaksız boş pencere **saat kurmuyor**; sınırın delinmesi boşta
sıfır kare tarafında bir kazançla ödeniyor. Gereksinim karşılığı R7.4.

## Karar 7: Sıçrama nasıl düzelecek? → ✅ eşik değil histerezis

İlk taslak iki hata yapıyordu ve panel ikisini de gösterdi.

**Birincisi: iki aday yok, bir tane var.** `CLAUDE.md` yazıyor — "imlecin
hedefi de **ekran satırıdır** (`row + origin`)" — yani içeriğin ötelemesi
caret'i tanım gereği kıpırdatmıyor. Üstelik kaymanın inişi zaten **snap**
(tek yönlü kayma kuralı: yalnız düşen hedef süzülür). Kullanıcının gördüğü
animasyonlu gidiş-dönüş bu yüzden caret'in **kendi yayı**. Ölçüme gerek yok;
012'nin `display: none` kararı da yeniden açılmıyor.

**İkincisi ve daha ağırı: devir `CommandStart`'ta başlamıyor.** Zincir
betikten söküldü:

```
Enter → line-finish (ZLE)  → 8133;e  → ayna Idle
                                     → caret_home(Input, Idle) = GRID   ← devir BURADA
     → preexec              → 133;C   → safha Running   (hâlâ Grid)
     → precmd               → 133;D   → safha Finished  → DOCK
```

`running_since` ilk devrin anında **`None`** (ancak `C`'de dikiliyor). Yani
"komut N ms koştuysa devret" eşiği ilk geçişi **hiç görmüyor** ve daha beter
yapıyor: Grid (`line-finish`) → Dock (`C`, süre eşiğin altında) → Grid (N ms).
44 ms'lik pencerede iki devir yerine **üç**. "Malzeme hazır" cümlesi yanlıştı.

**Karar: yüklemde histerezis.** Dock→Grid geçişi N ms tutuluyor; o süre içinde
geri dönerse geçiş hiç olmamış sayılıyor. Damganın yeri `ShellLog` — geçiş
`apply_scan`'de gözleniyor, tek giriş noktası ve yaprak kilidin altında.
`caret_home` üçüncü bir argüman alıyor.

**Yan kazanç, panelin bulduğu:** `caret_in_dock` **üç** tüketiciyi birden
besliyor — imlecin görünürlüğü, **doluluk sayısı** (yani içeriğin kayması) ve
dock'un caret'i. Devir ile kayma iki şüpheli değil **tek yüklemin iki yüzü**;
tek histerezis ikisini birden kapatıyor.

**Saat:** eşiğin kalan süresi `Cursor::next_tick` ile isteniyor ve üç şartı da
karşılıyor (içerik gerçekten değişiyor, tek atımlık, adlandırılmış durma
koşulu). **Şart:** `caret_home` ile **aynı kilit turunda** hesaplanmalı ve
`resolve_blocks`'unkiyle `min`'lenmeli — bugün o yol değeri **eziyor** ve koşan
bloğun çıpasının görünür olmasına bağlı, devir buna bağlanamaz.

**İddianın biçimi:** "sıçrama kalktı" **denmeyecek**. Animasyon ~230 ms'de
yerleşiyor, yani onun altındaki her eşik belirtiyi **küçültür, bitirmez**.
Phase `## Yayın Etkisi`'ne "ölçüm bekliyor" yazar ve iddiayı **azaltma** diye
kurar; doğrulaması önce/sonra göz kontrolü.

## Karar 8: Ayar anahtarı olacak mı? → ✅ hayır

Bir kez inen anahtar silinmiyor, **emekli** oluyor (009'un `prompt` anahtarı:
"dosyada korunuyor, okunmuyor, görülünce tanı bırakıyor"). Henüz kimse
pikselleri görmeden alınan bir zevk kararı için kalıcı bir şema girdisi satın
almak yanlış olur; üstelik kuyruğu çizim değişikliğinden büyük.

Geri alma yolu **phase commit'ini revert etmek** ve bunu gerçek kılmak için
`yarıçap = 0` / `hale alfası = 0` **desteklenen ve sınanan** bir yol olacak.
Bir gün gerçekten düğme gerekirse dürüst yeri **tema** (`cursor` rolü 014'te
indi) — ama yalnız kullanıcı isteyince.

## Muhakeme (2026-09-19)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üçü de yönü (caret'e kendi çizim yüzeyi) reddetmedi; üçü de **parça sayısını**
ve **sıçramanın teşhisini** düzeltti.

**Kabul edilen itirazlar → tasarım değişikliği:**

- **M1 — Devir `line-finish`'te başlıyor, `CommandStart`'ta değil**
  (codebase-fit, betikten sökülerek). Önerdiğim `running_since` eşiği ilk
  geçişi görmüyor ve **üçüncü bir devir** ekliyordu. → Karar 7 baştan yazıldı:
  eşik değil **histerezis**, damga `ShellLog`'da.
- **M2 — Sıçramada iki aday yok, bir tane var** (sadelik; `CLAUDE.md`'nin
  "hedef ekran satırıdır" cümlesi). İşletme merceği ikinci bir kanıt verdi:
  kaymanın inişi zaten snap. → "Önce ölçüm" kaldırıldı, açık soru silindi.
- **M3 — `caret_in_dock` üç tüketiciyi besliyor** (codebase-fit): devir ile
  kayma tek yüklemin iki yüzü, tek histerezis ikisini kapatıyor. → Karar 7'ye
  yan kazanç olarak girdi.
- **M4 — Karar 2'nin çözdüğü kusur yok** (sadelik, aritmetikle). → `cell.metal`
  hiç açılmıyor, `CursorBlock` 32 baytta kalıyor; yerine geçerlilik koşulu
  yazıldı. Codebase-fit ikinci kanıtı verdi: o yol köşe matematiğini
  ikizlerdi.
- **M5 — Ayrı `.metal` + yeni instance tipi gereksiz** (sadelik + codebase-fit
  Q3). Elemem için kullandığım "hücre başına dal" argümanı **kendi
  context.md'mle** çelişiyordu (`bg` seyrek). → Kardeş fragment + uniform;
  yeni `#[repr(C)]` çifti doğmuyor, hizalama tuzağı hiç oluşmuyor.
- **M6 — Şekil sayıları türetilmeli** (codebase-fit Q5 + işletme). Yedi
  seçilmiş sayı adayı vardı (benim saydığım dört değil). → `rule_px`,
  `gutter_px` ve hücre ölçüsünden türetiliyor; punto ölçeklemesi bedava.
- **M7 — Yuva seçimi şişmemiş dikdörtgene bakmalı** (sadelik). → Karar 4'e
  şart olarak girdi; 014 phase-1'de aynı tuzağa düşülmüştü.
- **M8 — `set_visible` yanlış emsal** (codebase-fit): o bir ritim kolu, odak
  ikisine de dokunamaz. → Emsal `blink.content_frame`'in `AND`'i olarak
  düzeltildi; sonuç (yalnız `bt-gpu`) değişmedi.
- **M9 — Odak, duman kapısının dördüncü yanlış pozitifi** (işletme). →
  Hermetik koşuda okunmuyor + `set_focused` aynı değerde no-op; ikisi de
  emsalli.
- **M10 — İki piksel sınaması gürültülü düşecek ve tamiri iddiayı ikiye
  ayırmalı** (işletme), toleransa çevirmek `bt-core`'dan inmiş iki iddiayı
  sessizce zayıflatır. Ayrıca Karar 5'in bekçisi dikdörtgenin **dışını**
  örneklemeli. → Karar 5'e ve phase kabul ölçütlerine girdi.
- **M11 — Kırpma listesi eksikti** (işletme): artık şerit ve komşu arka plan
  solması. → Karar 4'te beşe çıktı.
- **M12 — `caret_rect` ikisini birden döndürmeli** (codebase-fit Q2): içi boş
  imleçte boyanan dikdörtgen var ama opak iç yok. Değişmez kırılmıyor,
  **eksik tanımlıydı**. → phase'in şartı.
- **M13 — Üçüncü bayat doc** (codebase-fit): `shell::caret_home`'un serbest
  fonksiyon gerekçesi. Ayrıca `session.rs`'in "odak bugün sınırdan geçmiyor"
  cümlesi odak phase'iyle aynı commit'te düzelecek.
- **M14 — `43.9 ms`'in yöntemi hiçbir yerde yazılı değil** (işletme), üstelik
  indeks satırında da duruyor. → Yöntem `context.md`'ye yazılacak (PTY altında
  gerçek zsh, OSC 133 damgaları) ya da sayı indeksten düşecek.
- **M15 — `docs/YOL-HARITASI.md` borcu listede yoktu** (işletme): bu set
  015'i aldığına göre **altıncı kayma notu** ve kendi satırı gerekiyor.

**Reddedilenler:**

- **Sıçramayı setten çıkarıp tek commit yapmak** (sadelik). Kullanıcı onu bu
  setle birlikte istedi; ve M1'den sonra iş "tek satırlık eşik" değil —
  yüklemde histerezis, `ShellLog`'da damga, `next_tick`'te `min`'leme ve kendi
  sınamaları. Kendi phase'ini hak ediyor. **Ama sırası birinci**: shader işine
  bağlı değil ve kullanıcıyı günlük rahatsız eden o.
- **İçi boş imleci kırpmak** (sadelik'in adayı). Setin tek crate'ler arası
  sinyali ve kullanıcının 18-19 Eylül isteklerinde yok — doğru gözlem. Yine de
  **kalıyor**, çünkü 008'den beri kayıtlı bir borç ve aynı fragment onu
  neredeyse bedavaya getiriyor. **Son phase** olarak duruyor ve set uzarsa
  doğal kesme çizgisi orası; bu cümle o yüzden yazılı.
- **Karar 2'yi "yarıçap ters çevirme düzelmeden sevk edilemez" diye
  eşleştirmek** (işletme). İşletme merceği benim yanlış öncülümü kabul etmişti;
  sadeliğin aritmetiği onu çürüttü (M4). Yarıçap tek başına sevk edilebilir.

## Karar (2026-09-19, kullanıcı onayı)

- **Yön onaylandı:** caret'e kendi fragment'i; yarıçap, hale ve içi boş imleç
  tek yetenekten. Panelin düzelttiği hâliyle — yeni instance tipi yok, yeni
  `.metal` dosyası yok, `cell.metal` açılmıyor.
- **Sıçramanın çaresi histerezis**, eşik değil (M1). Devir Enter'daki
  `line-finish`'te başlıyor ve `running_since`'e bağlanan bir eşik üçüncü bir
  devir doğururdu.
- **Sayılar `CellMetrics`'ten türetiliyor** (M6); yeni tasarım sabiti
  uydurulmuyor.
- **Ayar anahtarı yok**; geri alma yolu revert ve dejenere kol.
- **Sıra:** histerezis → yüzey → odak. Birincisi shader'a dokunmuyor ve
  kullanıcıyı günlük rahatsız eden o.
- **Odaksız caret sönmez, sabit kalır** (2026-09-19, kullanıcı: *"Dursun,
  sabit kalsın"*). `phase-3.md` bu karar verilene kadar **yazılmadı** — 014'ün
  phase-2'sinin referans bakışını beklemesiyle aynı disiplin. Gereksinim
  karşılığı R7.4.
