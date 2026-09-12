# Phase 3b — Belge uyumu ve yöntem cümleleri

## Özet

Kod nihai hâlini aldıktan **sonra** belgeler ona uyumlanır: kanca adları,
`CLAUDE.md`'nin borç cümleleri, `/measure` skill'i, `context.md` şablonu ve
`## Yöntem`'e geçecek dürüst sınırlar.

_Requirements: R7, R7.1, R7.2, R7.3_

---

## Neden ayrı phase

Phase-3 otuz dört kaleme çıkmıştı ve içinde **dört yeniden ölçüm** vardı
(`IDLE_FRAME_LIMIT`'in dayanağı, boşta ölçütün derinliği, bekçi bütçesi,
GPU sütununun uzunluğu). Bunlar sabitleri değiştirebilir.

Belge aynı commit'te yazılsaydı **bayat doğardı**: `CLAUDE.md`'ye yazılan
sayı, aynı commit'te yeniden ölçülen sayı olurdu. Sıra bu yüzden zorunlu —
belge, gönderilen davranışı anlatır, tasarlanan davranışı değil.

Ad `duzen.md`'ye uygun: doğal sıralamada `phase-3 < phase-3b`.

---

## Kalemler

Aşağıdakiler phase-3'ten taşındı; gövdeleri phase-3'ün ilgili bölümlerinde
(`## 3. Belgeler`) ve devir notlarında duruyor.

---

## Uygulama Notları

- **Açık çelişki kapandı.** `CLAUDE.md`'nin `make duman` satırı artık
  `IDLE_FRAME_LIMIT = 8` diyor; ikinci yer `proje.md`'nin doğrulama tablosuydu
  ("bugün 2") ve o da düzeldi. `Makefile`'ın yorumu sayıyı hiç yazmıyor,
  `app.rs`'e yönlendiriyor — kılavuz onun da "bugün 2" taşıdığını söylüyordu,
  taşımıyordu; bayat olan yalnız örnek jeton satırıydı. `8` bugün üç belgede
  daha geçiyor (`CLAUDE.md`, `proje.md`, `.tasks/README.md`) ve bu **bilinçli**:
  oralarda geçen şey kapının **sözleşmesi** (sınır kaç), onu doğuran ölçüm
  değil — türetme, koşu sayıları ve rejim uyarıları yalnız sabitin doc'unda ve
  `/simplify` sonrası oradan da kopyalanmıyor. Ayrım önemli, çünkü `make kur`
  gelince değişecek olan **ölçüm**, sözleşme ise zaten değiştiği yerde
  güncellenir.

- **Kodun doc'unda dört bayat cümle bulundu ve düzeltildi (sapma, ama R7'nin
  ta kendisi).** Phase-3'ün `/audit` mercek 10'u aynı aileden dört bulgu
  kapatmıştı; bu dördü ondan kaçmış:
  1. `stats.rs`'te `MIN_SAMPLES`'ın doc'u "jeton `yetersiz` der" diyordu —
     kod `insufficient` basıyor. Tam olarak mercek 10'un kapattığı kusurun
     (`kapanis=asildi` → `abandoned`) kardeşi.
  2. `Stats::startup`'ın doc'u "**Süreç başından** ilk tamamlanan kareye"
     diyordu; `main.rs` ise aynı damga için "süreç başlangıcı **değil**"
     yazıyor. Belgeye taşınacak dürüst sınır, kodun kendi içinde çelişikti.
  3. `Stats::new`'in doc'u "`since` süreç başındaki damga" diyordu (aynı
     çelişki, ikinci yer).
  4. `Run::stats_since`'in doc'u da "damga süreç başında alınmış" diyordu
     (üçüncü yer).
  Dördü de yorum; kod satırı değişmedi.

- **`## Yöntem`'in bir evi oldu ve `/measure` oraya bakıyor.** Dürüst sınırlar
  dört phase'in notlarına dağılmıştı; `docs/OLCUMLER.md` ise R7.3 gereği bu
  sette yazılmıyor. Hepsi tek yere toplandı — `bt-shell`'de `Measured`'ın
  doc'unda, **"Ölçümün dürüst sınırları"** başlığı altında — ve her kalem
  **kapsam** ya da **açık kalem** diye etiketlendi. Liste sayıyla anılmıyor,
  bilerek: bu phase içinde bir kez beşten altıya çıktı (`/code-review`
  CPU'nun elenen örneğini bulunca) ve sayıyı tekrar eden her cümle o anda
  bayatladı. `/measure` skill'i artık dosyanın yokluğunda bu üç doc'a
  (ve `IDLE_FRAME_LIMIT` ile `MIN_SAMPLES`'a) yönlendiriyor. Yönlendirme
  olmasaydı bu kutuların `[x]`'i "bir yere yazıldı ama kimse bulamaz"
  demek olurdu.

- **Bench borcunun bedeli plan.md'nin yazdığından bir kalem büyük (sapma).**
  Plan "003 #1 ile #2'nin bench yarısı açık kalıyor" diyor; `teslim.md`
  tabloları okununca **002 #2**'nin gereken araçları da "`BT_FRAME_LOG`,
  `cargo bench`" çıkıyor, yani onun da bench yarısı açık. Belgeye üçü birden
  yazıldı: 003 #1 ve #2 **tamamen** bench'e bağlı (`/measure` onlara hâlâ
  "ölçüm aracı yok" der), 002 #2'nin yalnız yarısı. Aynı okuma 004 #4'ün
  `Atlas::slot` yarısının 003 #2'ye yaslandığını da gösterdi.

- **On iki iddianın bugünkü tablosu** (`teslim.md`'lerin kendi tablolarından
  türetildi, plan.md'nin özetinden değil): 2 kapandı (003 #3 ve 004 #2 —
  atlas doluluğu, phase-1'in `yuva=` jetonu), 8 **ölçülebilir** oldu, 2 açık
  (003 #1 ve #2, ikisi de saf bench). `.tasks/README.md`'nin üç satırı bunu
  söylüyor; hiçbirine sayı yazılmadı.

- **`docs/YOL-HARITASI.md` iki yerden bayattı ve kimse istememişti.**
  (1) "Kapanışta sınırsız bekleme → 006" maddesi phase-2b ile geçersizleşmiş,
  üstelik içindeki kalıcı çözüm önerisi (`SIGHUP` → süre → `SIGKILL`)
  **ölçümle çürütülmüştü** — `CLAUDE.md` bunu yazıyor, yol haritası hâlâ
  öneriyordu. Madde daraltıldı (borç kalktı değil, **kapsamı** daraldı) ve
  çürütme kayda geçti. (2) "Logger → 005 doğal adayı" satırı: 005 logger'ı
  bilerek almadı. Kutuda adı geçmiyor ama ikisi de bu phase'in tanımı olan
  "kodla çelişen belge cümlesi" sınıfından.

- **Motion setinin `context.md`'si yok, o yüzden not iki yere yazıldı.**
  Algılama tabanının yükselmesi (kutuda ~0,7 → ~2,7 Hz yazıyordu; doğrusu
  **1 → 3 Hz**, aşağıya bak) `IDLE_FRAME_LIMIT`'in
  doc'una (mekanizma ve aday çözümle) ve `docs/YOL-HARITASI.md`'nin "sete
  bağlanmamış borçlar" listesine girdi — set açılınca oradan `context.md`'ye
  taşınır. Var olmayan bir sete dosya uydurulmadı.

- **Phase-3'ün `## Yayın Etkisi`'ndeki örnek satır bayat** (`kapanis=temiz
  ornek=kapali`): o satır jeton değerleri İngilizceye çevrilmeden önce
  yazılmış. Phase-3'ün gövdesine dokunulmadı — kapanmış bir phase'in kaydı
  o günün kaydıdır — ama sonraki okuyucu oradan kopyalamasın diye burada
  duruyor. Belgelere giren satırlar phase notundan değil **gerçek koşudan**
  alındı.

---

### `/simplify` — dört mercek, dördü döndü

**Tek bulguda üçü de birleşti ve bulgu benimdi:** ilk geçiş **fazla
kopyalamıştı**. Her belgeye hem işaretçi hem içeriğin tamamı konmuştu, yani
"tek sahip" kuralını anlatan cümlelerin altına ikinci bir sahip yazılmıştı.
Kanıtı ölçülebilir: `CLAUDE.md`'nin `Measured`'a işaret eden cümlesi beş
dürüst sınırdan **dördünü** sayıyordu (beşincisi düşmüştü) — daha commit
olmadan drift. Sekiz yerde işaretçi bırakılıp içerik silindi: `CLAUDE.md`'nin
dil kuralı ve ölçüm maddesi, `Makefile`'ın `ornek=off`/`kapanis` gerekçesi,
`proje.md`'nin "Üst sınır neden 8" paragrafı (ki içinde **"buraya
kopyalanmıyor"** yazıp iki cümle sonra kopyalıyordu), `/measure`'ın sütun
paragrafı, `YOL-HARITASI`'nın algılama tabanı bloğu.

**Kendi soktuğum iki kusur** (dördü de yakaladı, biri "BLOCKING" dedi):

1. `Report::token_line`'ın doc'u *"`CLAUDE.md` bu ayrımı **henüz yapmıyor**;
   ayrılması phase-3b'ye yazıldı"* diyordu — bu commit tam olarak o ayrımı
   yapıyor. Yani bayat doc yorumu düzelten phase, aynı commit'te **beşincisini
   üretmişti**. Cümle güncellenmedi, **silindi**: bir doc'un başka bir
   belgenin durumunu izlemesi zamansal bir gerçektir ve yeniden çürür.
2. `Stats::startup`'ın yeni paragrafı `/measure`'ın tarifini alıntılıyordu —
   aynı commit'in `SKILL.md`'de **değiştirdiği** satırı. Alıntı kalktı.

**Katman yönü bulgusu (altitude):** `MIN_SAMPLES`'ın doc'una koyduğum
"`insufficient` der" düzeltmesi doğruydu ama `bt-gpu`'yu `bt-shell`'in jeton
yazımına bağlıyordu — bir alt katmanın üst katmanı işaret etmesi. Artık
yalnız "taban altında hesaplanmaz, nasıl söylendiği raporu basan katmanın
işi" diyor; bir sonraki yeniden adlandırma onu **kıramaz**.

**`Measured`'ın listesi ikiye ayrıldı.** Beşi de "kapsam" diye yazılmıştı,
oysa ikisi eylem bekliyor: `kapanis=abandoned` kayıtlı bir **kusur** (çaresi
adı konmuş), `kare`↔`istek` **açık bir soru**. Hepsini "kapsam" diye
etiketlemek okuyana "bilip geç" derdi. Koşu sayılarına tarih/profil damgası
da eklendi ve doc kendini **emanetçi** ilan ediyor — sahibi `docs/OLCUMLER.md`.

**Doğrulama merceği dört kalemin dördünü de onayladı** (jeton adları ve
değerleri `token_line`/`teardown_token`/`push_span` ile birebir; sabitler;
halka aritmetiği 8 640 B ve 1 728 000 B; env sözleşmesi). Ayrıca kutuda
olmayan **üç bayat cümle** buldu ve üçü de düzeltildi: `proje.md`'nin
"`Cell` 16 baytı geçmez (hedef; 002 ölçüp sabitler)" tuzağı (kod `24`
assert'liyor), aynı dosyanın "bilinmeyen dizi `tracing` ile loglanır"
tuzağı (logger yok, borç) ve `YOL-HARITASI`'nın 005 satırı (setin adında
`docs/OLCUMLER.md` yazıyordu, oysa bilerek kapsam dışı).

**Uygulanmayan, gerekçesiyle:** 002/003/004'ün `teslim.md`'lerindeki *canlı*
checklist satırları hâlâ "kanca yok" diyor. Kutu kapsamı `.tasks/README.md`
demişti ve `teslim.md`'ler kapanmış setlerin `/ship` tarafından okunan
kayıtları — birini açmak set kapanışı semantiğine dokunur. Rapora
**AÇIK KALAN** olarak yazıldı; kararı orkestratörün. Aynı sebeple
`docs/MIMARI.md`'nin var olmayan bir dosya olarak iki yerde anılması
(005'ten önce de vardı, bu setin damarında değil) bırakıldı.

### `/code-review` — iki koşucu, altı bulgu

`Skill` fork'u koştu ama geç döndü; `proje.md`'nin iniş sırası gereği
beklerken **ikinci basamak** da koşuldu (`code-reviewer` subagent'ı) —
phase-3'ün aynı durumda yaptığı şey. Fork'un raporu geldi: **kodda hata yok**
(watchdog bütçesi ile grace arası pay, ayrık kapanış thread'i ve `Drop`
tehlikesi, `Ring::snapshot` yarışı, p95'in indeks aritmetiği, `record_gpu`'nun
NaN/∞ kapıları, `let-else` ömrü — hepsi kovalandı ve temiz). Bulguların hepsi
sözleşme/belge sınıfından ve **dördü benim bu phase'de yazdığım cümlelerdi**:

1. **Sıra tersti (benim hatam).** `Measured`'ın doc'una "kapanış, halka
   okunduktan **sonra** koşuyor" yazmıştım; gerçek tam tersi —
   `run_deadline` önce `shutdown()`, sonra `report_and_exit`. Sonuç
   (örneklere etki yok) doğruydu ama **gerekçesi yanlıştı**; doğrusu
   `shutdown()`'ın ilk işinin `link.stop()` olması, yani bekleme başlarken
   kare akışının çoktan durmuş olması. Üstelik yazdığım cümle aynı dosyada
   iki yüz satır aşağıdaki "Sıra bilinçli" doc'uyla çelişiyordu.
2. **Algılama tabanı sayıları yanlıştı (benim hatam).** ~0,7 Hz / ~2,7 Hz'i
   `limit / 3 sn` diye hesaplamıştım, oysa kapı `n > limit`'te ateşliyor:
   yakalamak `limit + 1` kare istiyor, yani gerçek taban **1 Hz** ve **3 Hz**.
   Bu depoda türetilmiş sayı yük taşır; düzeltildi (`YOL-HARITASI`'nda sayı
   yok, orası niteliksel — dokunulmadı).
3. **`stats.rs` kendi kendisiyle çelişiyordu** (bu phase'in ürünü değil, ama
   tam onun sınıfı): bir test yorumu "CPU sütununda eleme **ulaşılmaz**"
   diyor, üç test aşağıda `cpu_rejects_zero_spans` onu ateşliyor. Ulaşılmaz
   olan yalnız taşma kolu; yorum ayrıştırıldı.
4. **`zero_second_run_still_has_a_ring`'in öncülü bayattı:**
   "`BT_RUN_SECONDS=0` meşru" diyordu, oysa phase-2'den beri `main.rs` sıfırı
   eliyor. Alt sınır hâlâ gerekli — ama binary'den değil, kütüphane
   API'sinden; yorum bunu söylüyor artık.
5. **rustdoc uyarısı eklemişim.** `[`Stats::since`]` (iki yer) ve
   `[`MAX_CAPACITY`]` private öğelere intra-doc link — `make hepsi`
   `cargo doc` koşmadığı için kapı bunu **göremezdi**. Ölçüm birimi de bir
   kez yanlış sayıldı ve düzeltildi: `cargo doc`'un "generated N warnings"
   özet satırı sayaca giriyordu. Doğru okuma
   `cargo doc --no-deps --workspace` → `bt-gpu` **4** uyarı (hepsi bu
   phase'den önce vardı); üçü eklendiğinde 7 olmuştu, de-link sonrası yine
   **4**. `bt-shell` `--document-private-items` ile **0**.
6. **CPU'nun elenen örneği sayılıyor ama hiçbir jeton basmıyor** — GPU'da
   R5.2 için eklenen sayacın CPU'da eksik kalmış hâli. **Kapatılmadı:** çare
   bir jeton eklemek, yani makine sözleşmesine dokunmak ve bu bir belge
   phase'inin işi değil. `Measured`'ın "açık kalemler"ine dürüstçe yazıldı
   (bugün zararsız: eleme yalnız sıfır uzunluklu aralıkta oluyor ve ölçülen
   koşuların hiçbirinde görülmedi) ve rapora **WAIVE** olarak gitti.

**Uygulanmayan (7. bulgu), gerekçesiyle:** `bt-gpu`'nun doc'ları hâlâ
`bt-shell`'i anıyor (`report_and_exit`, `ShellWake.waker`, "raporu `bt-shell`
yazar"). Bunlar **prose**, bağımlılık kenarı değil; hepsi bu phase'den önce
yazılmış ve desen `bt-gpu` genelinde yerleşik (yalnız `renderer.rs`'te on iki
yer). Phase-3'ün `/audit` mercek 1'i bu ağacı temiz bulmuştu ve testi kenar
taramasıydı. `MIN_SAMPLES`'ta düzelttiğim şeyin **cinsi farklıydı**: orası üst
katmanın *biçim dizgisini* (`insufficient`) iddia ediyordu ve gerçekten
bayatlamıştı. Bir tasarım kararını "çünkü tüketicisi şunu yapıyor" diye
gerekçelendirmek aynı şey değil. Yerleşik prose'u bir belge phase'inde baştan
yazmak kapsam kaymasıdır.

### `/audit` — on mercek elendi, ikisi yargı

**İlgisiz (kanıtıyla):** 2 (`Cargo.toml`/`Cargo.lock` diff'te **yok**),
3 (`bt-core` diff'te yok), 4 (`settings.rs`/tema yok), 5 (`assets/shell/`
yok), 7 ve 9 (yürütülen kod satırı hiç değişmedi; `.metal`, `build.rs` ve
`Cell` assert'i el değmedi).

**Mekanik, inline — temiz:** 1 (`cargo tree -p bt-core` ve `-p bt-atlas`
temiz, `bt-gpu → bt-shell` kenarı yok, `bt-core/src`'de `objc2|core_text|
core_graphics` yok), 8 (diff yeni animasyon/zamanlayıcı/`wake` çağrısı
eklemiyor — eklenen satırların tamamı yorum).

**Mercek 6 (ölçüm sahipliği), inline — bir bulgu, düzeltildi:**
`proje.md`'nin tuzak listesine yazdığım "Grid hücresi bugün **24 bayt**"
cümlesi, sahibi `CLAUDE.md` olan bir sayıyı ikinci bir yere kopyalıyordu —
üstelik aynı cümle `CLAUDE.md`'yi sahip ilan ediyordu. Sayı düştü, işaretçi
kaldı. (Düzeltmenin kendisi de bir bulgunun ürünüydü: oradaki eski cümle
"`Cell` 16 baytı geçmez" diyordu ve kod `24` assert'liyor.) Eklenen
satırlarda ölçülmemiş başarım iddiası taraması **boş**; jeton satırı
örneklerindeki sayılar yapısal sabitler (`hucre=8 glif=6 kural=15`) ve
gerçek koşudan.

**Mercek 10 (belge ve üslup), fan-out (`opus`) + `/code-review`'un ikinci
koşucusu:** ikisi aynı zemini taradı. Yedi uyarı ve yedi öneri geldi;
**on biri uygulandı**. En değerlileri yine bu phase'in kendi ürünüydü:
`phase-3b.md`'nin anlatısı çürütülmüş Hz sayılarını hâlâ taşıyordu (aynı
dosya iki bölüm aşağıda onları geri çektiği hâlde), "beşi tek yere
toplandı" listesi altıya çıkmıştı, `proje.md` eksik kancaları **iki** sayarken
diğer üç belge **üç** sayıyordu, `CLAUDE.md`'de bir satır sarılmamıştı.
Bir ölçüm hatam da yakalandı ve düzeltildi: rustdoc uyarı sayarken
`cargo doc`'un özet satırını sayaca katmışım (bkz. yukarıdaki 5. madde).
Kutulardaki iki bayat değer (`~0,7/~2,7 Hz`, `kapanis=asildi`) **silinmedi**,
yanlarına düzeltme notu düşüldü — kutu metni tarihsel kayıt.

Uygulanmayanlar gerekçeli: `bt-gpu` prose'unun üst katmanı anması (yerleşik,
bağımlılık kenarı değil), `Measured`'ın private oluşu yüzünden
`cargo doc`'ta görünmemesi (`/measure` dosya yoluyla işaret ediyor, sorun
değil) ve 002/003/004 `teslim.md`'lerinin canlı checklist satırları (kapsam
kararı orkestratörün).

## Yayın Etkisi

- **Belge (sekiz dosya):** `CLAUDE.md` (kanca adları, `make duman` satırı,
  bench borcunun yeniden yazımı, katman tablosunun `bt-gpu` satırı, kapanış
  maddesine ölçülebilirlik cümlesi, dil kuralının üçe ayrılması), `Makefile`
  (`duman` yorum bloğu), `.claude/skills/measure/SKILL.md`,
  `.claude/is-akisi/sablonlar/context.md`, `.claude/is-akisi/proje.md`
  (doğrulama satırı ve "Üst sınır neden **8**" paragrafı),
  `.tasks/README.md`, `docs/YOL-HARITASI.md`,
  `.claude/skills/audit/SKILL.md`.
- **Sekizinci dosya `/audit`'in kendi merceği ve sebebi bu phase.** Mercek 10
  "Türkçe kalması gereken **iki** şey" diye soruyordu; `CLAUDE.md`'nin dil
  kuralı bu commit'te üçe ayrılınca (jeton **anahtarları** da Türkçe ve
  donmuş) merceğin paraphrase'i bayatladı — yani denetim aracı, denetlediği
  kuralın eski sürümünü soracaktı. Kutuda yoktu; kuralı uygulayan cümlenin
  kendisi kuralı ihlal edemez.
- **Kod: yalnız yorum, üç dosya.** Doc yorumları (`///`) ve `stats.rs`'te iki
  test-içi satır yorumu (`//`); **yürütülen tek satır değişmedi** ve bu
  ölçüldü: `git diff HEAD -- crates/ | grep '^[+-]' | grep -v '///' |
  grep -v '^[+-][+-][+-]'` yalnız `//` yorumları döndürüyor.
  `crates/bt-gpu/src/stats.rs`
  (`MIN_SAMPLES`'ın bayat jeton değeri, `Stats::new` ve `Stats::startup`'ın
  "süreç başından" çelişkisi, halka ayırmasının açılış süresinin içinde
  kaldığı),
  `crates/bt-shell/src/app.rs` (`Measured`'a "Ölçümün dürüst sınırları"
  başlığı, `IDLE_FRAME_LIMIT`'e algılama tabanı notu),
  `crates/bt-shell/src/lib.rs` (`Run::stats_since`'in aynı çelişkisi).
  `Cargo.lock` oynamadı.
- **Sözleşme değişmedi:** jeton eklenmedi, silinmedi, adı değişmedi. Belgeler
  bugün basılanı anlatıyor ve **belgelere giren her jeton satırı örneği
  gerçek koşuyla doğrulandı**, ezberden yazılmadı. `make duman` üç kez
  koşuldu, üçü de çıkış 0 ve satır bit bit şu biçimde:
  `kare=N hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke istek=3
  kapanis=clean profil=debug ornek=off pipeline=ok`. `kare` koşudan koşuya
  `1` ya da `2` çıktı — sabitin doc'undaki 1↔2 oynamasının ta kendisi ve
  sınırın (`8`) neden onun **üstünde** durduğunun günlük kanıtı; belgeler
  bu yüzden `kare=N` yazıyor, bir sayı değil. Ölçüm koşusu (`BT_FRAME_STATS=1
  BT_SCROLL_TEST=1`) aynı satırı `ornek= dusen= gpu_ornek= gpu_elenen=
  taban=20` + altı süre jetonu + `acilis=` ile genişletti, çıkış 0; oradan
  hiçbir süre değeri belgeye kopyalanmadı (sahibi `docs/OLCUMLER.md`).
- **`docs/OLCUMLER.md` yazılmadı** — ilk `/measure` kuracak (R7.3). Ama artık
  `## Yöntem`'in **kaynağı belli**: `Measured`'ın doc'u, ve `/measure` skill'i
  dosyanın yokluğunda oraya yönlendiriyor.
- **Ölçüm bekliyor: yok.** Kod değişmedi; bu phase belge uyumu.
- **Kapanan iddialar:** on ikinin ikisi 005 phase-1'de kapandı (atlas
  doluluğu), sekizi **ölçülebilir** oldu. **Açık kalan ve bedeli yazılı:**
  003 #1 ve #2 saf bench, 002 #2 ile 004 #4'ün bench yarısı; giriş gecikmesi
  ve düşen kare sayımı bilerek kapsam dışı. Hiçbir belgeye sayı kopyalanmadı.

---

## Checklist

- [x] `CLAUDE.md`: kanca adları, `make duman` satırı, **bench borcunun yeniden yazımı** (silme değil)
- [x] `/measure` skill'i, `context.md` şablonu, `proje.md` kanca adlarıyla uyumlandı
- [x] `.tasks/README.md`: 002/003/004 satırları "ölçüm aracı yok" demiyor
- [x] Phase-2'nin dürüst sınırı (açılış damgası `main()`'den, süreç başından değil) kodun doc'unda yazılı
- [x] **phase-2'den devir — `## Yöntem`'e geçecek dürüst sınır.** Açılış damgası `main()`'in **ilk satırında**, `has_aqua_session()`'ın alt sürecinden de önce; ama yine de **süreç başlangıcı değil** (dyld + Rust runtime kurulumu önce bitiyor) ve bittiği yer **ilk tamamlanan kare** (`addCompletedHandler`), sunulan kare değil. `/measure`'ın "process başlangıcından" tarifinden bu kadar sapıyor
- [x] **phase-2b'den devir — `## Yöntem`'e geçecek üç cümle.** (1) Hiçbir koşu atılmıyor: kapanış artık en çok `SHUTDOWN_GRACE` (500 ms) bekliyor ve jeton satırı her koşuda basılıyor. (2) Ama kusur **iyileşmedi, sınırlandı**: ölçülen sekiz Load koşusunun **ikisinde** sınır doldu, yani çocuk çıkışın içinde (`?Es`) arkada bırakıldı ve onu süreç çıkışı topladı — stderr'de bir satır var (`shell 500ms içinde kapanmadı, arkada bırakıldı`), ölçüm sayılarına etkisi yok ama koşu süresine yarım saniye ekliyor. (3) Kalıcı çare hâlâ açık ve adı belli: `wait` bloklarken master'ı boşaltmak; yolu da belli — `Session::spawn`'da `pty.file().try_clone()` ile master'ın bir kopyası alınabilir (yeni bağımlılık **gerekmiyor**, `Cargo.lock` oynamıyor). Bu phase'in işi değil, `## Yöntem`'in dürüst sınırı
- [x] **phase-2b'den devir — `CLAUDE.md`'nin kapanış maddesi yeniden yazıldı.** R7'nin "borç cümleleri silinmez, yeniden yazılır" kuralı gereği madde daraltıldı ve içinde **çürütülmüş bir çare** kayda geçti: "`SIGHUP` → süre → `SIGKILL`" işe yaramıyor (o durumdaki çocuk `SIGKILL` almıyor, ölçüldü). Phase-3 aynı dosyaya kanca adlarını yazarken bu maddeyi **yeniden yazmasın**; dokunması gereken satırlar kanca adları ve bench borcu
- [x] **phase-2'den devir — izlenmeyen belge borcu.** `/audit` (mercek 10) yakaladı: `CLAUDE.md`'nin **katman tablosundaki** `bt-gpu` satırı crate'in yeni sorumluluğunu (ölçüm defteri) anmıyor. `crates/bt-gpu/src/lib.rs` başlık yorumu phase-2'de güncellendi; tablo satırı R7.2'nin kanca adları listesinde **yok**, yani bu satır yazılmasa kimse görmezdi
- [x] **phase-3'ten devir — jeton satırı büyüdü, belgede yazılı hâli yok.** Kod bugün şunu basıyor ve **hiçbir belge** bunu anlatmıyor: `kare hucre glif kural yuva yuk istek kapanis profil ornek dusen gpu_ornek gpu_elenen taban cpu_kare_p95 cpu_kare_max cpu_encode_p95 cpu_encode_max gpu_p95 gpu_max acilis pipeline=ok`. Yazılması gerekenler: (a) kapı kapalıyken `ornek=off` çıkar ve **ölçüm jetonları hiç basılmaz** — `ornek=0` bilerek seçilmedi, çünkü sıfır "kapı açıktı, hiç örnek toplanmadı" ile karışırdı (R5.2); (b) taban altında p95 **ve** max `insufficient` der, eşik `taban=` jetonunda; (c) `ornek=` CPU sütununun, `gpu_ornek=` GPU'nunki — Metal sıfır damga verirse ikincisi kısa kalır ve farkı `gpu_elenen=` söyler; (d) `dusen=` tek sayı ve CPU'dan geliyor, GPU'nunkinin **tavanı**; (e) `istek=` bir sayaç (**kapı değil**), `kapanis=` ise kısmen kapı: panik kolları (`reader-panicked`, `panicked`) koşuyu kırmızı düşürüyor, `abandoned`/`unbounded` düşürmüyor; (f) jeton **adları** Türkçe, **değerleri** İngilizce ve bu bir kural — depodaki `yuk=smoke|load` deseni
- [x] **phase-3'ten devir — `proje.md`'nin "Üst sınır neden 2" paragrafı ölü bir teoriyi kelimesi kelimesine taşıyor.** Doğrulama tablosundaki `make duman` satırı hâlâ "sistem display link'i askıya alıyor ve ritim ~3'te doyuyor", "tavan 3", "`2` bu üçünün arasındaki tek anlamlı yer" diyor. Üçü de çürüdü. Yeni sayılar phase-3'ün `## Uygulama Notları → Ölçülenler` başlığında: sağlıklı duman 1–2 (otuz bir koşu), bir kez 4; bozuk duman **49–354**; sınır artık **8**. Paragraf silinmez, **yeniden yazılır** — ve içine eski sınırın doğru bir build'i kırmızıya düşürdüğü kayda geçer
- [x] **phase-3'ten devir — `Makefile`'ın `duman` hedefindeki yorum bloğu.** Örnek jeton satırını ve "bkz. app.rs IDLE_FRAME_LIMIT, bugün 2" cümlesini taşıyor; ikisi de bayat. Kod değil yorum olduğu için phase-3'te dokunulmadı
- [x] **phase-3'ten devir — `CLAUDE.md`'nin kapanış maddesine eklenecek bir cümle var.** Sınır dolan koşu artık **görünür**: `Session::shutdown` sonucu döndürüyor (`Teardown`) ve rapor `kapanis=clean|reader-panicked|abandoned|panicked|unbounded|already-done|none` basıyor. Madde "arkada kalan çocuk" borcunu yeniden yazmıyor (phase-2b onu yazdı), yalnız borcun artık ölçülebildiğini söylüyor
- [x] **phase-3'ten devir — `## Yöntem`'e geçecek iki dürüst sınır.** (1) **Kapanış hâlâ asılıyor, yalnız sınırlı:** ölçüm yükünün on yedi koşusunun **dördünde** `kapanis=asildi` [**gönderilen değer İngilizce: `abandoned`; bkz. `teardown_token`**] (~%24, phase-2b 2/8 ölçmüştü) — sayılara etkisi yok, koşuya yarım saniye ekliyor. (2) **`kare` ile `istek` iki rejimde tamamen ayrışıyor:** duman yükünde `istek ≈ kare + 2`, ölçüm yükünde `kare=21` iken `istek≈71 000`. Mekanizması **ölçülmedi** (kapı mı yutuyor, ana thread mi doyuyor, sistem mi link'i kısıyor) ve aynı yükün phase-2b'de `kare=594` vermesi de açıklanmadı — pencere görünürlüğü şüpheli ama doğrulanmadı. `/measure` bir kare süresi okurken bunu bilmeli
- [x] **phase-2'den devir — halkalar açılış yolunda ayrılıyor**, yani `acilis=` sayısının **içindeler**: 3 saniyelik koşuda 8,6 KB, tavanda 1,7 MB. `## Yöntem`'in dürüst sınırlarından biri
- [x] **phase-3'ten devir — `IDLE_FRAME_LIMIT` `.app` paketiyle yeniden ölçülecek.** Bugünkü `8` görünmeyen bir pencerede ölçüldü; `make kur` gelince meşru kare sayısı artabilir. Sabitin doc'unda yazılı, belge tarafında da anılmalı
- [x] **phase-3'ten devir — `CLAUDE.md`'nin dil kuralı jeton satırını tam anlatmıyor.** Bugün "`make duman` satırları Türkçe kalır" diyor; kod ise ayrım yapıyor ve ayrımın gerekçesi `Report::token_line`'ın doc'unda: **anahtarlar** Türkçe ve donmuş (sözleşme "silinmez" diyor), **değerler** İngilizce (okuyan taraf bir `match` kolu / CI grep'i), **tanı metni** (stderr, `assert!`) Türkçe. Cümle bu üçe ayrılmalı — `/audit` mercek 10'un bulgusu
- [x] **phase-3'ten devir — boşta kare kapısının algılama tabanı yükseldi.** `IDLE_FRAME_LIMIT` 2→8 olunca 3 sn'lik koşuda yakalanabilen en yavaş sızıntı ~0,7 Hz'den ~2,7 Hz'e çıktı [**iki sayı da yanlıştı: kapı `n > limit`'te ateşliyor, yani yakalamak `limit + 1` kare istiyor — doğrusu 1 Hz → 3 Hz. `/code-review` bulgusu; sabitin doc'unda düzeltildi**]. Bu sette öyle bir animasyon **yok**, ama motion/imleç fiziği seti (00X) tam bu şekilde gelecek: durma koşulsuz 2 Hz'lik bir blink 3 sn'de ~6 kare eder ve bugün yeşil geçer. `/audit` mercek 8'in notu; motion setinin `context.md`'sine taşınmalı. Yarısı kurulu: `istek=` örtülmeden etkilenmiyor ve **oran** olarak (saniye başına talep) kapıya bağlanabilir — ama eşik ölçülmedi — **yapıldı, ama başka adrese:** motion setinin `context.md`'si **yok**, o yüzden mekanizma ve aday çözüm `IDLE_FRAME_LIMIT`'in doc'una, borcun kendisi `docs/YOL-HARITASI.md`'nin "sete bağlanmamış borçlar" listesine yazıldı (oradan set açılınca `context.md`'ye taşınır). Var olmayan bir sete dosya uydurulmadı
- [x] **AÇIK ÇELİŞKİ — `CLAUDE.md:43` `IDLE_FRAME_LIMIT = 2` diyor, kod `8`.**
      Phase-3 sabiti ölçümle 2→8 taşıdı (31 sağlıklı duman → `kare` 1–2, bir
      kez 4; 9 bozuk koşu → 49–354) ama belgeye dokunması **orkestratör
      tarafından yasaklanmıştı** (bölmenin gerekçesi: belge nihai davranışı
      anlatmalı). Sonuç, `CLAUDE.md`'nin kendi "kodla çelişen cümle aynı
      commit'te düzelir" kuralının **bir commit boyunca** ihlali. Bilinçli,
      kayıtlı ve süresi bu phase ile doluyor — `/ship`'ten **önce** kapanmalı,
      yoksa çelişki `main`'e iner.
- [x] Doğrulama geçti (`proje.md` → Doğrulama) — `make hepsi` yeşil. `make duman` **kod değişmediği için kapı olarak gerekmiyordu**, yine de koşuldu: bu phase belgelere jeton satırı **örneği** yazıyor ve ezberden yazılmış bir örnek tam da bu phase'in kapattığı kusur olurdu. İki koşu da gerçek: duman (`yuk=smoke`, çıkış 0) ve ölçüm (`BT_FRAME_STATS=1 BT_SCROLL_TEST=1`, çıkış 0)
- [x] `/simplify` çalıştırıldı, bulgular uygulandı — `Skill` ile, dört mercek ajanı paralel; dördü de döndü. Tek bulguda birleştiler (**fazla kopyalama**) ve sekiz yerde içerik işaretçiye indirildi; kendi soktuğum iki bayat doc cümlesi silindi, bir katman-yönü ihlali (`bt-gpu` → `bt-shell` işaretçisi) kaldırıldı, `Measured`'ın listesi kapsam/açık kalem diye ikiye ayrıldı. Doğrulama merceği ayrıca kutuda olmayan üç bayat cümle buldu, üçü de düzeltildi. Bir kalem gerekçeli bırakıldı (`teslim.md`'lerin canlı satırları — kapsam kararı orkestratörün)
- [x] `/code-review` çalıştırıldı, bulgular giderildi — iki koşucu (`Skill` fork'u geç döndü, beklerken `code-reviewer` subagent'ı da koşuldu; phase-3'ün deseni). Kodda hata **yok**; altı sözleşme/belge bulgusundan beşi uygulandı — dördü bu phase'in kendi ürettiği hatalardı (ters sıra iddiası, yanlış türetilmiş Hz sayıları, eklenen üç rustdoc uyarısı) — biri gerekçeli WAIVE (CPU'nun elenen örneği için jeton yok: çare sözleşmeye dokunmak), biri gerekçeli waive (`bt-gpu` prose'unun `bt-shell`'i anması: yerleşik ve bağımlılık kenarı değil)
- [x] `/audit` çalıştırıldı, bulgular giderildi — `Skill` ile. 2/3/4/5/7/9 ilgisiz (kanıtıyla), 1 ve 8 inline temiz, 6 inline (bir bulgu: `proje.md`'ye kopyalanmış `24 bayt`, düzeltildi), 10 fan-out (`opus`) + `/code-review`'un ikinci koşucusu aynı zemini taradı — on dört kalemden on biri uygulandı, üçü gerekçeli waive
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: `8be6f68`
