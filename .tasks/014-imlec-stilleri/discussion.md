# İmleç stilleri ve blink — Tartışma

Tek tasarım sorusu var ve her şey ona bağlı: **blink'i hangi kare kaynağı
sürüyor?** Şekiller (beam/underline) bu soruyu hiç sormuyor — onlar yeni bir
kare istemiyor, yalnız çizilen dikdörtgeni değiştiriyor. Bu yüzden seçenekler
yalnız blink içindir.

Ölçüt `bt-gpu::link`'in modül başlığındaki **yazılı üç şart**: içerik gerçekten
değişecek, periyot ekran hızından çok düşük olacak, adlandırılmış bir durma
koşulu taşıyacak. Blink ikinci ve üçüncüyü geçiyor, **birincisinden kalıyor** —
ızgara değişmiyor, yalnız caret'in alfası. Seçenekleri ayıran şey bu.

## Seçenek A: Hareket animatörü (`bt-gpu::motion`)

Blink `Motion`'a dördüncü bir kip olarak girer, `alpha()` onu da üretir,
`settled()` blink sürerken **hiç `true` dönmez**.

**Artıları:**
- Tek yeni tip yok; `alpha()` zaten var ve `Mode::Fade` onu zaten sürüyor.
- Caret'in tek animatörü olması korunur; dock'ta da kendiliğinden çalışır.

**Eksileri:**
- **Link hiç uyumaz.** 2 Hz'lik bir değişim için **tazeleme hızında** kare.
  Setin tamamının gerekçesini çürütür.
- `make duman` `Verdict::MotionUnsettled` ile **kırmızı** düşer
  (`link.rs:1355`); kapı kod doğruyken kızarır.
- "Her animasyon bir durma koşulu taşır" kuralını, koşulu olmayan ilk
  animasyonla bozar.

## Seçenek B: Saat, içerik tadında (`Waker::wake`)

Blink'in bir sonraki faz değişimi `Cursor::next_tick`'e katılır; süre dolunca
bugünkü `arm_clock` `Waker::wake()` çağırır ve **içerik** karesi doğar.

**Artıları:**
- **Hiç yeni mekanizma yok.** 013'ün saati olduğu gibi kullanılır.

**Eksileri:**
- **Sözleşme onu adıyla yasaklıyor** ve bu yasak 013'te yazıldı
  (`link.rs:18-23`): *"animasyonun zamana bağlı kare talebi hareket saatinden
  geçer. Yeni bir animasyon (**blink**, yumuşak kaydırma) oraya girer,
  `Waker`'a değil."* Yani bu sette verilecek **yeni bir karar değil**, 013'te
  verilmiş bir kararın uygulanması. B'yi eleyen şey budur.
- Gerekçesi de yerinde: üç şartın **birincisini** çiğniyor. İçerik değişmiyor
  — her yarım periyotta `Term` kilidi alınır, ızgara yeniden taranır, dört
  liste yeniden kurulur; hepsi caret'in alfası için.
- **`icerik=` jetonunu kirletir.** Kapının üst sınırı içerik karesi sayıyor;
  blink onu içerik olmayan karelerle şişirir ve ölçülen bir koşuda başka
  sızıntıları maskeler.

> **Kapı B ile C'yi ayırt etmiyor** ve bu yazılsın, yoksa C'ye hak etmediği bir
> kredi verilir: `last_frame_at` **hareket kolunun** `Ok` dalında da
> tazeleniyor (`link.rs:730`), yani 2 Hz'lik blink `sessiz=`'i iki seçenekte de
> `QUIET_FLOOR`'un altına indirir. `IDLE_FRAME_LIMIT = 8`'i de 2 Hz zaten
> göremiyor (`app.rs:110-111`). Kapıyı koruyan şey seçenek değil **varsayılan**
> (aşağıda Karar 6).

## Seçenek C: Saat, hareket tadında (`Waker::resume`) — önerilen

Saat ikinci bir uyandırma **tadı** kazanır: hasar dikmeden link'i açan bir kol.
Uyanan callback bugünkü "hasar yok" dalına düşer, listeleri korur ve yalnız
caret'i yeni alfayla yeniden çizer — yani **hareket karesi**, ama tetikleyicisi
ekran hızı değil saat.

**Gerektirdiği beş şey** *(ilk yazımda üç sanılmıştı; panel ikisini daha
çıkardı ve o hâliyle tasarım **hiç kare çizmiyordu** — aşağıda Muhakeme M1)*:

1. **`Waker::resume()`** — `wake()`'in üç işinden `dirty.mark()` çıkarılmış
   hâli. Aynı kapı, aynı `pending` birleştirmesi, aynı dispatch.
2. **Uyku testi üç soru sorar, iki değil.** Bugün `!take_damage()` dalı
   `motion.settled()` doğruysa **çizmeden** uyuyor (`link.rs:652-660`). Blink
   `Motion`'ın dışında yaşadığı için `settled()` hep doğru döner, yani
   `resume()` tek başına kare üretmeyen bir uyan/uyu fırdöndüsü yaratır. Uyku
   koşuluna faz değişimini taşıyan **tek atımlık** bir terim girer ve o terim
   `settled()`'ın erken dönüşünden **önce** tüketilir.
3. **Saat süre değil son tarih tutar.** Bugünkü `arm_clock` her uyku noktasında
   `next_tick`'i baştan kuruyor; blink uyandıkça 013'ün sayacı sonsuza itiliyor
   (context.md → Kanıt). İçerik tiki bir deadline olarak saklanır, her uyku
   `min(içerik deadline, bir sonraki faz)` kurar ve hangisinin dolduğuna göre
   `wake()` ya da `resume()` çağırır. Geçmişte kalan son tarih sıfıra doyar
   (`shell::next_tick`'in sıfır savunması emsal).
4. **Blink fazı `bt-gpu`'da yaşar ve mutlak son tarihtir.** Yeri zorunluluk:
   hareket karesi `bt-core`'a hiç gitmiyor (`link.rs:652-745` arasında ne
   `session.frame()` var ne `Term` kilidi), yani `bt-core`'un ürettiği bir faz
   o kola ulaşamaz. **`dt` biriktirmez:** `Motion`'ın `advance` biçimini
   taklit etseydi `DT_MAX = 0.1` kırpması yüzünden 500 ms'lik uyku 100 ms
   sayılır, imleç ~5 uyandırmada bir döner ve arada dört **birebir aynı** kare
   çizilirdi. Belirtisi sessiz.
5. **Şekil `Frame`'de saklanır.** `move_caret` bugün yalnız `at, text, rgba,
   alpha` taşıyor ve `link.rs` şekli hatırlamıyor — blink karesi tam o yoldan
   geçtiği için beam ilk sönüp yanışta **bloğa dönerdi**. `push_caret` yazar,
   `move_caret` korur; böylece kaybolması temsil edilemez olur.

`bt-core` yalnız **blink'in açık olup olmadığını** söyler (`Cursor`'da tek
`bool`); fazın ne zaman döneceği boyayan tarafın kararıdır. "Karar burada,
boyama orada"nın doğru okunuşu bu ve depoda emsalli: `CursorMotion`'ın doc'u
(`settings.rs:97-104`) "buradaki tek bilgi hangi stil; sürelerin ve yay
katsayılarının sahibi `bt_gpu::motion`" diyor.

**Artıları:**
- Üç şartın üçünü de geçer; sözleşmeyi bozmaz, **tamamlar**.
- `Term` kilidi yok, ızgara taraması yok, `bt-core` yolculuğu yok
  (`link.rs:652-699`). B'den **ucuz olduğu yönü koddan kanıtlı; büyüklüğü
  ölçülmedi** → ölçüm bekliyor.
- `icerik=` temiz kalır.
- **013'ün sayacındaki bayat-süre kusurunu da kapatır** (3. madde); bu set onu
  düzeltmek **zorunda**, çünkü blink onu ölümcül yapıyor.

**Eksileri:**
- `Waker`'a ikinci bir giriş noktası; modül başlığı, `Waker` doc'u ve
  `requests`'in "Ne saymıyor" cümlesi yeniden yazılacak.
- 013'ün taze kodu (`arm_clock`) bu sette değişiyor.
- Blink karesinin **hiçbir CPU sayacı yok** — aşağıda Karar 5.

## Karar Noktaları

### 1. Blink'in durma koşulu ne? *(asıl ürün sorusu — açık)*

Bu depo on iki set boyunca "boşta sıfır kare"yi savundu. 013 ilk istisnayı
getirdi ve **adlandırdı**: "koşan komutu olan pencere boşta değildir" — ama o
istisna komut bitince kapanıyor. Blink kapanmıyor.

- **S1 — Yalnız uygulama kontrolü.** Durma koşulu: uygulama kapatır, imleç
  gizlenir, pencere örtülür (`Gate`).
  *Bedel:* vi modunda `\e[5 q` gönderen bir zsh kurulumu pencereyi **kalıcı
  olarak boşta-değil** yapar.
- **S2 — S1 + hareketsizlik zaman aşımı.** Blink, son içerik karesinden N
  saniye sonra durur ve ilk hasarda geri gelir. 008'in `discussion.md`'si bu
  kolu **adıyla** kaydetmişti: *"kitty'nin 'N saniye sonra dur' kolu"*
  (kitty'nin `cursor_stop_blinking_after` varsayılanı 15 sn). `bt-gpu`'da
  tamamen uygulanabilir, yeni sınır alanı istemez.
  *Bedel:* seçilmiş bir sabit daha (`OMEGA`, `FADE_DURATION` emsali).
- **S3 — S2 + odak.** Odakta olmayan pencere yanıp sönmez; içi boş imlecin
  (`HollowBlock`) doğal ikizi.
  *Bedel:* odak bugün sınırda **yok** ve içi boş imleç yeni bir çizim
  primitifi ister — ikisi birlikte ayrı bir set.

**Şart (hangisi seçilirse seçilsin):** durma koşulu fazı **"açık"a zorlar ve
son bir kare çizdirir.** Sönük fazda durursa imleç bir sonraki hasara kadar
**kaybolur** ve belirtisi sessizdir. Aynısının küçük hâli `Gate` kapanışında
da var (`link.rs:629-632` erken dönüşü `arm_clock`'a hiç varmıyor), orada bedel
en çok yarım periyot.

**Önerim: S1 + S2.** S2, blink'i bu deponun merkezî vaadiyle barıştıran tek
koldur ve bedeli bir sabit. S3 içi boş imleçle birlikte sonraki sete.

### 2. Sert aç/kapa mı, sınırlı geçiş mi? *(açık — referans bakışı istiyor)*

Mimari bir seçeneği **eliyor**: sürekli nefes (alfa sinüs gibi akar) her karede
değişir, yani Seçenek A'ya geri döner. Kalan iki yol:

- **Sert.** Alfa 1 ↔ 0. Saniyede iki kare.
- **Sınırlı geçiş.** Her faz değişiminde kısa bir solma, sonra yerleşme ve
  saatin beklemesi. Kare sayısı solmanın süresine bağlı ve serttekinin katları.

`docs/ARASTIRMA.md` yalnız `cursor_blink` anahtarını listeliyor, blink'in
**neye benzediğine** dair veri yok. İlk iş referansa bakmak (materyaldeki
yöntemin aynısı).

**Önerim: sert.** Sınırlı geçiş sonradan `cursor_blink`'in üçüncü değeri
olarak eklenebilir, ölçülmüş bir istek doğarsa.

### 3. Beam/underline nasıl çizilir? → ✅ karar

`cell.metal:124` ters çevirmeyi bir **karışım** olarak yapıyor ve karışımın
alanı `CursorBlock.rect`. Ama çizilen **iki** şey var ve ilk yazımda biri
atlanmıştı: boyanan dörtlü `Caret::instance` (`frame.rs:121-127`) boyutu
**koşulsuz** `cell_px`'e çiviliyor. Yalnız `rect` daraltılsaydı sonuç tam
hücrelik opak accent bloğu + ince şeritte ters çevirme olurdu — bugünkünden
beter.

Karar dört parçalı:

- **İkisi birlikte daralır** (`Caret`'in ölçüsü ve `CursorBlock.rect`);
  shader'a bayrak eklenmiyor, yani `CursorBlock`'un doc'undaki "bayrak ile
  dikdörtgen ayrışabilen iki gerçek olurdu" reddi korunuyor.
- **Daraltma instance kurulurken yapılır** (`grid_caret`/`dock_caret`),
  `push_caret` içinde değil: yuva seçimi (`frame.rs:674`) **hücre ayak izine**
  bakıyor ve daraltma ondan önce olursa underline caret'i banda değmeyip
  ızgara yuvasında kalır, dock'un opak zemini onu örter.
- **Kalınlık `bt_atlas`'ın alt çizgi metriğinden** gelir — chevron emsali,
  ikinci bir sayı uydurulmaz. (İlk yazımdaki "bir-iki piksel" ölçülmemiş bir
  sayıydı.)
- **Ters çevirme kalır.** Kural tek olur ("caret'in dikdörtgeni altındaki
  mürekkebi çevirir"), ikinci bir dal doğmaz.

### 4. Şekil hangi ayarın altına girer? → ✅ karar

`Changes`'e **yeni alan gerekmiyor** (ilk yazımda gerekir sanılmıştı). Şekil
`term_config`'e gidiyor, onun tek girdisi `TerminalOptions` (`session.rs:456`)
ve `Changes::terminal` zaten *"seçenekler `Session`'a tamamıyla gider"* diyor.
Blink de aynı yere iniyor (`default_cursor_style.blinking`). Yani:
`TerminalOptions`'a iki alan, `Changes` dokunulmamış.

Bölüm adı **`[cursor]`**: `shape` ve `blink`. Referansın düz adlarını bölümle
niteleyip `[motion] cursor_motion` tekrarına düşmüyor.

### 5. Blink karesi jeton satırında nasıl görünür? → ✅ karar: **görünmez, ve bu yazılır**

Blink karesi hiçbir CPU sayacına yazmıyor: `hareket=` yalnız
`!motion.cursor_settled()` iken artıyor (`link.rs:679-681`), `istek=` `resume()`
yolunda artmıyor, `icerik=` zaten artmamalı. Geriye `kare=` ve `sessiz=`'in
sıfırlanması kalıyor.

Yeni bir jeton (`saat=` / `blink=`) **eklenmiyor**. Gerekçe deponun kendi
emsali (`app.rs:1185-1196`, `cpu_elenen=`): *"ölçülmüş bir ihtiyaç beklemeden
atılmadı."* Jeton satırı makine sözleşmesi ve **jeton silinmez** — varsayılan
kapalıyken gözlenebilir her koşuda `0` basacak bir sayaç, geri alınamaz bir
genişleme olurdu.

Bunun yerine **dürüst cümle teslim.md'ye yazılır:** varsayılan kapalıyken
kapının **hiçbir katı** bozuk bir blink'i görmez — `icerik` görmez (blink
içerik karesi değil), `MotionUnsettled` görmez (blink `Motion`'ın içinde
değil), `sessiz` ancak blink gerçekten koşarsa görür. **Koruma bir jeton değil,
varsayılanın kendisi.** `docs/YOL-HARITASI.md:112-124`'teki borcun **kapsamı**
büyüyor (artık yalnız saat değil, "meşru periyodik kare") ama **vadesi
gelmiyor**: borç entegrasyonlu bir ölçüm yüküne bağlı ve bu set öyle bir yük
doğurmuyor.

### 6. Ayar ile DECSCUSR nasıl birleşir, varsayılan ne? *(açık)*

Bu, ilk yazımda "varsayılan ne" diye sorulmuştu; asıl soru daha derin ve
1. ile 5.'i de belirliyor: **`blink = false` bir ana şalter mi, yoksa yalnız
`default_cursor_style`'ın değeri mi?**

- **Yalnız varsayılan** (alacritty semantiği): `blink = false` diyen kullanıcı
  vim'de `\e[5 q` gelince **yine de** yanıp sönen bir imleç görür. Şaşırtır.
- **Ana şalter:** `false` = asla yanıp sönme. Kullanıcının yazdığı şeyin
  karşılığı bu.
- **Üç değerli** (`reduce_motion` emsali): `"auto"` uygulamayı izler, `"on"`
  hep söner, `"off"` asla. Varsayılan `"auto"`.

Duman kapısı hangi cevapta ne olur: hermetik koşu ayar dosyasını **hiç
okumuyor** (`decide_inputs`, `app.rs:254-261`) ve `smoke_shell`
(`session.rs:564-576`) `/bin/sh` koşup DECSCUSR göndermiyor — yani kullanıcı
blink'i açtığında `make duman`'a **hiçbir şey olmaz**. Muafiyet gerçek, ama
**cinsi 013'ünkinden zayıf** ve bu yazılsın: 013'ünki bütün bir alt sistemin
yokluğuydu (OSC 133 hiç basılmıyor), buradaki "reçetede dört baytlık bir kaçış
dizisi yok" — bir `printf` uzaklıkta. Kabul edilebilir, çünkü kırılırsa
**sesli** kırılıyor (`sessiz < QUIET_FLOOR` → kırmızı), sessizce değil.

**Önerim: `shape = "block"`, `blink` üç değerli ve varsayılan `"off"`.**
Şekil varsayılanı alacritty paritesi; blink varsayılanı kapalı, çünkü
"pencere kalıcı olarak boşta değil" durumu kullanıcının **seçtiği** bir şey
olmalı, sessizce gelen bir varsayılan değil.

> **Uyarı, kayda geçsin:** `app.rs:1122-1132` `hareket > 0` kapısının
> `Settings::default().cursor_motion`'ın animasyonlu olmasına dayandığını
> söylüyor. Yeni imleç ayarlarının varsayılanı aynı tuzağın içinde: `shape`
> varsayılanı caret'i çizdirmeyen bir değer olamaz.

### 7. Reduce Motion açıkken blink ne olur? → ✅ karar: **kapalı**

İlk yazımda bu hiç sorulmamıştı ve varsayılan yolda yazılı bir sözü bozuyordu.
`CLAUDE.md`: *"`cursor_motion = "snap"` bunun üstündedir: hareketi zaten
kapatmış olana erişilebilirlik ayarı animasyon **eklemez**."* Naif tasarımda
Hareketi Azalt'ı açık bir kullanıcı vim'de `i`'ye basınca imleci yanıp sönmeye
başlıyor — hiçbir şey seçmeden.

Karar hem sözü koruyor hem tasarımı sadeleştiriyor: `Mode::Fade` `alpha()`'yı
zaten yazıyor; blink Reduce Motion'da kapalı olunca **ikinci yazar hiç
doğmuyor** ve bir birleştirme kuralı (çarpım mı, öncelik mi) icat edilmiyor.

## Muhakeme (2026-09-18)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üçü de yaklaşımı (C) reddetmedi; üçü de **mekanizmanın eksik** olduğunu buldu.
Sadelik ile codebase-fit aynı kusuru **birbirinden bağımsız** çıkardı (M1).

**Kabul edilen itirazlar → plan değişikliği:**

- **M1 — C tarif edildiği hâliyle hiç kare çizmiyor** (sadelik + codebase-fit,
  kodla doğrulandı: `link.rs:652-660`). → Parça listesi 3'ten **5**'e çıktı;
  uyku testine tek atımlık faz terimi ve `Frame`'de şekil saklama eklendi.
- **M2 — B'nin elenme gerekçesi yanlıştı.** Kapı B ile C'yi ayırt etmiyor
  (`last_frame_at` hareket kolunda da yazılıyor, `link.rs:730`). → Gerekçe
  yazılı sözleşmeyle (`link.rs:18-23`, blink **adıyla** anılıyor) değiştirildi;
  kapı farkı iddiası geri çekildi ve bir uyarı bloğu olarak yazıldı.
- **M3 — Blink karesinin CPU tanığı yok.** İki mercek **çelişti**: codebase-fit
  "`saat=` yapısal zorunluluk", işletme "ekleme, ölçülmüş ihtiyaç yok". →
  **İşletme kazandı** (Karar 5): emsal `app.rs:1185-1196`'da yazılı ve jeton
  satırı geri alınamaz. Gerçek teslim.md'ye dürüst cümle olarak giriyor.
- **M4 — Reduce Motion × blink hiç sorulmamıştı** (işletme). → Yeni Karar 7;
  hem yazılı sözü koruyor hem ikinci alfa yazarını doğurmuyor.
- **M5 — Beam için `rect` daraltmak yetmiyor** (codebase-fit). → Karar 3 dört
  parçaya çıktı: `Caret` ölçüsü, daraltmanın yeri (yuva seçimi!), atlas
  metriğinden kalınlık, ters çevirmenin kalması.
- **M6 — `Changes`'e alan gerekmiyor** (codebase-fit). → Karar 4 kapandı.
- **M7 — Durma koşulu sönük fazda yakalarsa imleç kaybolur** (işletme). →
  Karar 1'e şart olarak eklendi.
- **M8 — Blink fazı `dt` biriktirmemeli** (işletme; `DT_MAX = 0.1` kırpması).
  → C'nin 4. maddesine girdi.
- **M9 — Ölçülmemiş sayılar.** "tazeleme hızında 120 kare", "60 katı", "15
  katı" ve "bir mertebe ucuz" (işletme). → Çarpanlar düşürüldü; ucuzluk **yön**
  olarak bırakıldı ve "ölçüm bekliyor" diye işaretlendi.
- **M10 — Duman muafiyetinin cinsi 013'ünkinden zayıf** (işletme). → Karar
  6'ya yazıldı: bir `printf` uzaklıkta, ama kırılırsa sesli kırılıyor.
- **M11 — `term_config_keeps_every_other_field` sessizce zayıflıyor**
  (codebase-fit): fixture yeni alanı **varsayılan** değerle doldurursa guard'ın
  vaadi sessizce yalan olur. → plan.md'de phase-1'in kabul ölçütü;
  fixture varsayılan **olmayan** bir şekil taşıyacak.
- **M12 — `Cursor`'a `Hidden` taşınmamalı** (codebase-fit): `visible` onu zaten
  tüketiyor. `bt-core` kendi enum'unu verir (alacritty tipi `pub` API'de
  görünemez); `HollowBlock`'un akıbeti adlandırılmış karar olur.
- **M13 — Doc borcu listesi eksikti** (işletme): `Counters::motion` doc'u,
  `Waker` doc'u, `requests`'in "Ne saymıyor: hareket karesini" cümlesi ve
  `Cursor::next_tick`'in "'Ne zaman' sorusunun cevabı burada" cümlesi (saat iki
  deadline'ı `min()`'leyince daralacak).
- **M14 — phase-1 `blink` anahtarını şemaya/belgeye koymamalı** (işletme):
  okunmayan ama belgelenmiş anahtar en kötü ara durum.

**Reddedilenler:**

- **Seti bölmek (014 = şekiller, 015 = blink)** — sadelik. *Biçimde red,
  özünde kabul.* Merceğin istediği risk ayrımını **tek set içinde iki phase**
  zaten veriyor (işletme merceği phase-1'in tek başına yeşil ve tutarlı
  olduğunu doğruladı). Bölmeye karşı iki sebep: (1) kullanıcının istediği şey
  **blink**, şekiller benim eklemem — blink'i ayrı sete atmak kullanıcının
  önceliğini tersine çevirir; (2) sınır işi ortak: tek `Term::cursor_style()`
  çağrısı hem `shape` hem `blinking` veriyor, yani blink'i önce yapmak
  phase-1'in işinin yaklaşık tamamını yine gerektirirdi.
- **`arm_clock`'ın son tarih düzeltmesi YAGNI** — sadelik. Yalnız blink
  ertelenirse YAGNI olurdu; blink sette olduğu için düzeltme spekülasyon değil
  **ön koşul**. Üstelik `link.rs:1059`'daki yazılı kusuru bedavaya kapatıyor.
- **`saat=` jetonunu bu sette eklemek** — codebase-fit. M3'te çözüldü.
- **Karar 2, 3, 6'nın "sahte çatal" olduğu** — sadelik. Kısmen: 3 karara
  dönüştü (haklı), ama 2 gerçek bir zevk sorusu olarak kalıyor (referans
  verisi yok) ve 6 yeniden çerçevelendi — soru "açık mı kapalı mı" değil,
  "ayar DECSCUSR ile nasıl birleşiyor", ve o gerçek bir çatal.
- **`Event::CursorBlinkingChange`'in hasar dikmesi şart** — işletme. Şart
  değil **doğrulama kalemi**: DECSCUSR baytları PTY'den geliyor ve alacritty'nin
  `Wakeup`'ı zaten `Waker::wake()` tetikliyor, yani hasar muhtemelen kendiliğinden
  var. phase-2'nin checklist'inde "doğrula" olarak duracak, gereklilik olarak değil.

## Karar (2026-09-18, kullanıcı onayı)

- **Seçilen: Seçenek C** (saat, hareket tadında) — panelin tamamladığı beş
  parçasıyla. Reddedilenler: **A** (link hiç uyumaz, kapı `MotionUnsettled`
  ile kırmızı düşer), **B** (`link.rs:18-23` blink'i adıyla `Waker`'dan
  men ediyor; bu sette verilecek yeni bir karar değil, 013'te verilmiş bir
  kararın uygulanması).
- **Durma koşulu: S1 + S2** — uygulama kontrolü **artı** hareketsizlik zaman
  aşımı. Blink son içerik karesinden N saniye sonra durur, ilk hasarda geri
  gelir; durma fazı "açık"a zorlar ve son bir kare çizdirir. **S3 (odak)
  reddedildi:** odak bugün sınırda yok ve içi boş imleç yeni bir çizim
  primitifi istiyor — ikisi birlikte sonraki set.
  *Neden S1 tek başına yetmedi:* blink'i bu deponun merkezî vaadiyle
  barıştıran tek şey bir durma koşuludur; S1'de vi modunda bekleyen pencere
  kalıcı olarak boşta-değil kalırdı.
- **Ayar: `[cursor] blink` üç değerli** (`"auto" | "on" | "off"`),
  **varsayılan `"off"`**; `[cursor] shape` varsayılan `"block"`.
  Reddedilenler: **basit bool** ("uygulamayı izle" hiç mümkün olmazdı, vi
  modunun isteği tamamen yutulurdu), **varsayılan `"auto"`** (kullanıcı
  seçmeden boşta-değil bir pencere doğururdu).
  *Not:* varsayılan `"off"` olduğu için S2'nin zaman aşımı yalnız **açan**
  kullanıcıyı ilgilendiriyor — ama açmayı güvenli kılan şey tam da o.
- **Görüntü (sert / sınırlı geçiş): açık bırakıldı, önce referansa bakılacak.**
  `docs/ARASTIRMA.md` yalnız anahtarı listeliyor, blink'in neye benzediğine
  dair veri yok; materyaldeki yöntemin aynısı uygulanacak. Bakış **phase-2'nin
  ön koşulu** ve kullanıcıda (`[elle]`); sonucu bu bölüme ikinci bir madde
  olarak yazılır. Mimari zaten bir seçeneği eledi: sürekli nefes her karede
  değişir, yani Seçenek A'ya geri döner.
- **Set bölünmedi.** Sadelik merceğinin "014 = şekiller, 015 = blink" önerisi
  biçimde reddedildi, özünde kabul: risk ayrımını tek set içinde iki phase
  veriyor ve kullanıcının istediği şey blink.
