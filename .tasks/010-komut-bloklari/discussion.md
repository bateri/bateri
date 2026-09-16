# Komut blokları — Tartışma

## Karar 1: Blok satıra nasıl çıpalanır?

009 bu soruyu **yakalama** sorusu olarak bıraktı ("işaret görüldüğünde ızgara
o noktada mı"). Kodun okunması ikinci ve daha ağır bir yarı gösteriyor:
**dayanıklılık** — saklanan çıpa kaydırma, reflow ve temizleme boyunca aynı
satırı göstermeye devam ediyor mu? İkisi ayrı sorunlar ve seçenekleri ayıran
şey ikincisi (`context.md` → Kanıt).

### Seçenek A′: Türetilmiş mutlak indeks

İşaret geldiğinde `abs = history_size + cursor_row` saklanır; her karede
`row = abs - history_size` ile geri çözülür.

**Artıları:**
- En ucuz: yeni bayt yolu yok, betik değişmiyor, `bt-core`'a küçük bir kayıt.
- Yakalama yaklaşıklığı olağan akışta görünmez (kabuk prompt'u basıp girdiye
  bloklanır).

**Eksileri:**
- **Doygunlukta kırılıyor ve doygunluk olağan hâl.** `history_size`
  `scrollback`'e (varsayılan 10 000) dayandıktan sonra kayan her satır bütün
  çıpaları bir satır yanlışlar. Bir kez dolduktan sonra terminal sürekli
  yanlış çizer.
- **Reflow'da kırılıyor.** Pencereyi yatay boyutlandırmak satırları
  birleştirip böler; bütün şeritler kayar.
- **`clear_history`'de kırılıyor.**
- Belirtisi sessiz **değil** ama yanlış: şerit çizilir, yanlış yerde. "Hiç
  çizmemek"ten kötüdür.

### Seçenek M: Çıpa ızgaranın içinde taşınır (OSC 8)

Betik prompt'u kendi kimliğiyle bir hyperlink'e sarar
(`bateri://block/{N}`); `frame()` hücre döngüsünde o kimliği görür ve
bloğun başladığı satırı **ızgaradan okur**, hatırlamaz.

**Artıları:**
- **İnşa gereği dayanıklı.** Hyperlink `CellExtra`'da yaşıyor; reflow
  hücreleri değer olarak taşıdığı için satırla gidiyor, satır sıfırlanınca
  düşüyor, geçmişten atılınca birlikte atılıyor. Üç kırılmanın üçü de
  kapanıyor — kod yazarak değil, doğru yere yazarak.
- **Yakalama sorusu buharlaşıyor.** Çıpa "işaretin göründüğü an"a değil,
  kabuğun bastığı hücreye bağlı; `pty_read`'in bayt birleştirmesi onu
  göremiyor.
- **Hücre bütçesine dokunmuyor.** `set_hyperlink` imleç **şablonunda** bir
  kez koşuyor, yazılan her hücre `template.extra.clone()` ile `Arc` sayacını
  artırıyor (`term/mod.rs:989`) — blok başına tek ayırma, hücre başına değil.
- Bozulma kolu da doğru tarafa düşüyor: tema PS1'i yeniden kurup kapanışı
  silerse hyperlink çıktıya sızar, ama kural (**"N kimliğini taşıyan ilk
  satır bloğun başıdır"**) sızıntıda da doğru cevabı verir; sızıntı bir
  sonraki prompt'un yeni kimliğinde durur.
- Kapanış zaten `B`'nin bindiği `%{…%}`'ye biner: yeni bir kırılma noktası
  doğmuyor, var olanla aynı kaderi paylaşıyor.

**Eksileri:**
- **Yalnız bizim betiğimizin koştuğu yerde şerit var.** SSH'ın öte yakası ve
  başka bir aracın bastığı gerçek OSC 133 durum verir, blok vermez. (009
  Karar 3'ün seviye modeli bunu zaten böyle çerçeveliyor: şerit seviye 1'in
  ürünü.)
- Standart dışı bir kullanım: OSC 8 bir bağlantı protokolü, biz onu kimlik
  taşıyıcı olarak kullanıyoruz. Gelecekteki ⌘-tıkla-aç bu şemayı **süzmek**
  zorunda — kayda geçer.
- `Mark`'lara kimlik alanı (`aid=N`) ve `Scanner`'a onu taşıma işi ekler.
- Alacritty'nin hyperlink'i `CellExtra`'da tutmaya devam etmesine bağlı; bir
  sürüm kanaryası ister.

### Seçenek B: Kendi okuma döngümüz (red)

009 bunu "kesinlik gerekirse açık kapı" diye bıraktı. Bugün bakınca **en
pahalı yoldan en az dayanıklılık**:

- Yakalamayı kesinleştirir (baytlar işaretin üstünde bölünür) ama saklanan
  çıpanın sonraki kaymasına hiçbir şey yapmaz.
- Doygunluk ve `clear_history` ancak `Term`'ü ~90 metotluk bir `Handler`
  delegasyonuyla sarıp `linefeed`/`scroll_up` sayarak kapanır — fork'a yakın
  bir bakım borcu.
- **Reflow o delegasyonla da kırılır**: satırları birleştiren kod grid'in
  içinde ve hiçbir şey yayınlamıyor.
- Bedeli 009'un yazdığı gibi: `Msg` kanalı, `OnResize`, çocuk olayları,
  `drain_on_exit` ve **ölçülmüş kapanış dengesi** (`SHUTDOWN_GRACE`,
  `kapanis=`) yeniden açılır.

### Seçenek G: Kendi grid'imiz (kapsam dışı)

Satır kimliği ancak grid bizimse gerçekten bizim olur (`lib.rs`'in "kendi
hücremize geçiş (00X)" notu; Metalterm'in `screen/reflow`'u da öyle). Doğru
uzun vadeli cevap, ama bir blok şeridi için grid'i devralmak setin
onlarca katı. M seçilirse G geldiğinde çıpa taşınır, geri alınmaz.

**Önerim: M.** Tek seçenek üç kırılmanın üçünü birden kapatıyor ve bedeli
betikte bir satır artı `Mark`'ta bir alan. A′ günlük kullanımda yanlış
çizen bir ürün verir; B en pahalı yoldan eksik kalır.

## Karar 2: Blok sınırı nasıl geçer?

009 Karar 2'yi "sıfır tüketici" gerekçesiyle ayrı sorguya (`shell_state()`)
bağlamıştı. O gerekçe bu sette düştü — ve M seçilirse soru zaten cevaplı:
çıpa keşfi `frame()`'in hücre döngüsünün **içinde** oluyor, yani aynı `Term`
kilidi ve aynı `display_offset`. Ayrı bir sorgu blokları hücrelerden farklı
bir karede okur ve kaydırma karesinde şerit bir kare geride kalırdı.

Sınırdan geçecek şey **çözülmüş** olmalı (`CLAUDE.md` → karar burada, boyama
orada): `bt-gpu`'ya çıkış kodu değil, satır aralığı ve **renk** gider.
`shell_state()` yerinde kalır (011 onu ayrıca kullanacak).

## Karar 3: Şerit nereye çizilir?

Bugün grid pencereyi kenardan kenara dolduruyor; dolgu **yok** ve `cols`
genişliğin bölümünden çıkıyor. İki yol:

- **3a — sol kenardan yer ayır.** `cols` gutter genişliği düşülerek
  hesaplanır. Bedeli: `sync_geometry`, renderer'ın orijini ve **fare
  eşlemesi** (`view.rs`, `view_px → col` bölmesi orijin bilmiyor) birlikte
  kayar; PTY `winsize` da değişir. Kazancı: şerit hiçbir metni örtmez.
- **3b — bölme artığına çiz.** `width - cols * cell_w` artığı genelde birkaç
  piksel ve **sıfır olabilir**; ölçüsü kullanıcının penceresine bağlı.
  Ürün kalitesi tesadüfe kalır.

**Önerim: 3a**, gutter genişliği sabit (ölçek çarpanıyla). `command_gutter`
ayarı kapatınca `cols` geri kazanılır.

## Karar 4: Durum renkleri temaya nasıl girer?

Şerit bir **durum rengi** istiyor ve `Theme`'de yok: bugün dört rol
tüketiliyor (`background`, `foreground`, `dim`, `accent`), dört durum rolü
"013 ile gelir" diye yazılı (`CLAUDE.md`). Bu set durum rollerinin **ilk
tüketicisi**.

- **4a — dördünü birden getir** (başarı, uyarı, hata, bilgi). Tema biçimi bir
  kez açılır; 013 yalnız tüketir.
- **4b — yalnız gerekeni** (`error`, belki `success`). Kullanılmayan rol
  gömülü temalarda ve `docs/AYARLAR.md`'de ölü durur.

**Önerim: 4a.** Tema dosyası biçimi geriye dönük okunan bir sözleşme; iki
kez açmak kullanıcı temalarını iki kez eskitir. Anahtar adları ve gömülü iki
temanın değerleri bu sette kararlaşır.

## Karar 5: Kapsam — ne bu sette, ne değil?

**İçeride:** `A`→`D` aralığı ve çıkış koduna göre renklenen şerit; alternatif
ekranda şerit yok (vim/htop'un kendi ekranı blok değil); şeridin belirmesi
008'in hareket saatinden ve durma koşuluyla, `reduce_motion`'da 90 ms
belirme; `command_gutter` ayarı; süre (`C`→`D`) **kaydedilir**.

**Dışarıda, adıyla:**
- **Sürenin gösterimi.** Izgara dışı metin çizmek yeni bir yetenek (`ui_text`
  pipeline'ı yok) ve kendi işi. Süre bu sette kayda girer, ekrana çıkmaz —
  `command_duration_threshold` de o zaman gelir.
- **Prompt'u terminalin çizmesi** (`B`'nin asıl tüketicisi) → 011.
- **Katlama, `block_depth`, komutlar arası atlama** → şeridin üstüne sonra
  biner.
- **`feed_lift`** (çıktı gelince tamponun kayması): 008'in altyapısının
  ikinci tüketicisi ama bu setin animasyonu şeridin belirmesi, tamponun
  kayması değil.
- **bash/fish** → 009'un bıraktığı yerde; M seçilirse çıpa satırı o
  betiklere de yazılır, ama betikler bu sette doğmuyor.
- **Sekme/bölme.** Blok kaydı "bir pencere = bir oturum" varsayımıyla
  iniyor; 014 onu retrofit edecek (yol haritası bu bedeli zaten yazıyor).

## Karar 6: Ayar ayrıştırmasının beşinci kopyası

`command_gutter` yol haritasının "dördüncü anahtar altıncı kopyayı doğurur"
borcunu tetikliyor. İki yol: yardımcıyı bu set getirir (yeni anahtar
eklemeden yapılırsa hiçbir davranış değişmez, ayrı commit) ya da plan bunu
**açıkça** erteler. Erteleme sessiz olmamalı.

## Karar Noktaları

1. Çıpa: **M** (önerilen) mi, A′ mi?
2. Şerit geometrisi: sütun ayır (3a) mı, artığa çiz (3b) mi?
3. Durum rolleri: dördü birden (4a) mı, yalnız gereken (4b) mi?
4. Süre: kaydedip göstermemek kabul mü, yoksa gösterim de bu sette mi?
5. Ayar yardımcısı: bu sette mi, ertelenip yazılıyor mu?

## Muhakeme (2026-09-16)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üçü de Karar 1'i (M) onayladı — A′ ve B'nin reddi kanıtlı bulundu, katman
yönü korunuyor, `bt-gpu` çıkış kodu görmüyor. İtirazların tamamı **M'nin
fiyatlanmamış yarılarında** toplandı.

**Kabul edilen itirazlar → plan değişikliği:**

- **Defter M'nin taşıyıcı yarısıdır ve tasarlanmamıştı** (üç mercek).
  Şeridin **rengi** ızgarada değil: çıpa satırı verir, kodu vermez. Dahası
  `frame()` yalnız görünür pencereyi geziyor (`session.rs:1083`), yani bir
  ekran dolusu çıktının ortasında hiç çıpa görünmez — setin motive edici
  örneği. Kural buradan doğuyor: **kimlikler monoton, ilk görünür çıpanın
  üstü bir önceki bloğundur.** Defter `aid → çıkış kodu`, yaprak kilitte ve
  **sabit halka** (tahliye sinyali yok: çıpa `Row::reset` ile sessizce
  düşüyor, alacritty hiçbir şey yayınlamıyor — `context.md` → Kanıt 2).
  Süre, safha ve satır defterde **yok**.
- **Kilit sırası çakışıyordu.** `session.rs:1074-1078` yazılı kural: yaprak
  kilit kare boyunca tutulmaz, `Term` kilidinin altına ikinci muteks girmez.
  Çözüm iki faz: `Term` altında yalnız `(aid, ilk_satır)` çiftleri yeniden
  kullanılan bir tampona toplanır, kilit bırakılır, renk defterden çözülür.
- **Betiğin bedeli "bir satır" değildi** (üç mercek). Kimlik her prompt'ta
  değişiyor, yani `bateri.zsh:161`'deki idempotent nöbet ("içeriyorsa
  dokunma") çalışmaz ve PS1 her `precmd`'de yıkıcı biçimde yeniden kurulurdu
  — p10k/starship ile tam da yarışan iş. Açılışı `precmd`'de `print` ile
  basmak **daha kötü**: ZLE yeniden çizimi (SIGWINCH, Ctrl-L,
  `reset-prompt`) prompt'u precmd'siz yeniden basıyor ve çıpa buharlaşıyor —
  yani M'nin seçilme gerekçesi olan senaryo. **Çare `psvar`:** PS1 eki
  **sabit** kalır (`%{\e]8;;bateri://block/%1v\a%}` … `%{\e]8;;\a%}`),
  kimlik `psvar[N]` ile prompt anında genişler; `%1v` `prompt_subst`
  istemiyor (doğrulandı: `zsh -f`). Mevcut nöbet örüntüsü aynen korunuyor.
- **OSC 8 iç içe geçmiyor** (`vte-0.15.0/src/ansi.rs`): boş URI linki
  kapatıyor, yeni URI onu **değiştiriyor**. Kendi prompt'unda OSC 8 kullanan
  bir tema bizim çıpamızı keser — bu sızıntı değil **kayıp**. Ekimiz PS1'in
  **önüne** girdiği için temanın linkinden önceki hücreler bizi taşır;
  temanın linki ilk karakterde başlıyorsa çıpa hiç doğmaz. Bilinen sınır
  olarak yazılır, geri düşüş şeridin **çizilmemesi**dir (yanlış çizilmesi
  değil).
- **Şeridin belirme animasyonu sıfır fiyatlanmıştı** (üç mercek) →
  **animasyon bu setten çıkıyor**, şerit anında belirir. `bt-gpu::motion`
  bugün genel bir saat değil, tek `State` ve imleç hedefi
  (`motion.rs:244`, `link.rs:378`); ikinci bir tüketici duraklama koşulunu,
  `motion_settled()` kapısını (`link.rs:818`), `Mode::Fade` indirgemesinin
  "tek yer" kuralını ve hareket karesinin liste koruma yolunu birden açıyor
  — durağan bir dikdörtgen için. Üstüne `make duman` bu yolu **hiç
  görmüyor** (`smoke_shell` OSC 133 basmıyor), yani boşta sıfır kareyi bozan
  bir hata sessiz kalırdı; kapıyı görür kılmak `QUIET_FLOOR` ve
  `IDLE_FRAME_LIMIT`'in yeniden türetilmesi demekti (`proje.md:34` gereği
  ayrı commit). Yol haritasının "(+ blok animasyonları)" kalemi bu yüzden
  **ertelenmiş borç** olarak yazılır.
- **Şerit `Frame`'e hangi yoldan giriyor tanımsızdı** (İşletme).
  `frame.rs:250` `debug_assert_eq!(bg.len(), bg_count)` ve `frame.rs:361`
  `move_cursor`'ın `bg.truncate(bg_count)`'u: şerit `bg`'ye girip
  `bg_count`'a girmezse imleç her kaydığında silinir ve **titrer**; girerse
  `hucre=` jetonunun anlamı kayar. Karar: şerit `Frame`'de **kendi
  listesi**, kendi draw call'u (aynı `cell_bg` pipeline'ı, yeni shader yok).
  Üçü de korunur.
- **Karar 4 → 4b.** 4a'nın tek gerekçesi ("iki kez açmak temaları eskitir")
  kod tarafından yalanlandı: `theme.rs:85-87` eksik anahtarda yuvaya
  **dokunmadan** dönüyor ve `unknown_keys_are_silent` sınamasının örneği
  birebir bu vaka (`success = "#00ff00"`). Rolü sonradan eklemek bedava;
  bugün eklemek iki gömülü temada ve `docs/AYARLAR.md`'nin iki bloğunda
  tüketicisiz renk uydurmak. Bu set yalnız **çizdiği** rolü getirir.
- **Karar 6 ortadan kalkıyor.** `command_gutter` anahtarı bu sette **yok**:
  kimse istemedi, tetiklediği "altıncı kopya" borcunu doğuruyor ve 3a'nın
  gutter genişliğini **üç ayrı tüketiciye** (`app.rs:400` `cols`,
  `frame.rs` `pos_at` orijini, `view.rs:69` fare eşlemesi) kayıt anında
  uygulanan bir ayar hâline getiriyordu — bir kare boyunca ayrışırlarsa
  belirti "fare bir sütun kayıyor". Şerit hep açık; genişlik tek sabit,
  üç tüketiciye oradan gider. Ayar ve yardımcı gerçekten istenince birlikte,
  kendi commit'inde gelir.
- **Süre bu setten çıkıyor** (Sadelik). Karar 5 onu "kaydedilir,
  gösterilmez" diye yazıyordu: tüketicisi olmayan alan. Gerektiğinde `D`'nin
  yükünden gelir (zsh `EPOCHREALTIME`), `bt-core` saat okumaz —
  `shell.rs`'in "bu modül **saf**" başlığı korunur.
- **Alt ekranda sütun ayrılmış kalır**, yalnız şerit çizilmez. `cols`'u
  alternatif ekranda değiştirmek her vim açılışında bir SIGWINCH doğurur ve
  çalışan uygulamayı yeniden çizdirir.

**Reddedilenler:**

- *"`aid → exit` defteri de M'nin kendi kanıtıyla vurulmuş; halka yerine
  tahliye sinyali aransın"* — aranacak sinyal yok (aynı kanıt) ve sabit
  halka tahliyeyi **tasarımla** çözüyor: satır başına en çok bir prompt, yani
  `scrollback` kadar kayıt bir tavan. Sinyal aramak B'ye geri dönmek olurdu.
- *"Şerit `Frame::push`'tan geçsin, üçüncü liste fazladan kavram"* — üç
  kapının üçü de (assert, truncate, jeton) kırılıyor; ayrı liste tek
  kavramla üçünü birden koruyor.
- *Karar 4a'nın "bedelsiz biniyor" savunması* (Codebase-fit) — doğru ama
  yetersiz: bedelsiz olması eklemek için sebep değil, Sadelik'in
  "tüketicisiz renk" itirazı ağır basıyor.

## Karar (2026-09-16, kullanıcı onayı)

- **Karar 1 → M.** Çıpa ızgaranın içinde, prompt'u saran bir OSC 8
  bağlantısında (`bateri://block/{aid}`) taşınır; satır her karede
  ızgaradan okunur, hatırlanmaz. Kimlik `psvar` ile genişler, PS1 eki
  sabit kalır.
  **Reddedilen A′** (türetilmiş `history_size + row`): doygunluk varsayılan
  `scrollback = 10 000`'de günlük hâl, reflow ve `clear_history` de kırıyor;
  belirtisi "çizilmiyor" değil "yanlış yerde çiziliyor".
  **Reddedilen B** (kendi okuma döngümüz): yalnız yakalamayı çözüyor,
  reflow'u çözmüyor; ölçülmüş kapanış dengesini (`SHUTDOWN_GRACE`,
  `kapanis=`) yeniden açıyor. **Reddedilen G** (kendi grid'imiz): doğru uzun
  vadeli cevap ama bir şerit için grid'i devralmak setin onlarca katı; G
  geldiğinde çıpa taşınır, geri alınmaz.
- **Karar 2 → `frame()` sınırı, iki fazlı.** `Term` kilidi altında
  `(aid, ilk_satır)` çiftleri toplanır, kilit bırakılır, renk defterden
  çözülür. Ayrı sorgu reddedildi: kaydırma karesinde şerit bir kare geride
  kalırdı. `Session::shell_state()` yerinde kalıyor (011 kullanacak).
- **Karar 3 → 3a, ayarsız ve her zaman.** Sol kenardan sabit pay; `cols`
  bir kez hesaplanır, oturum boyunca oynamaz. Entegrasyonsuz oturumda pay
  boş kalır — kabul edilen bedel. **Reddedilen "ilk blokta ayır"**: `cols`'u
  oturum ortasında değiştirmek bir SIGWINCH doğuruyor ve üç tüketiciyi
  (`cols`, çizim orijini, fare eşlemesi) aynı anda güncellemeye zorluyor;
  ayrışırlarsa belirti "fare bir sütun kayıyor". Alternatif ekranda pay
  ayrılmış kalır, yalnız şerit çizilmez.
- **Karar 4 → 4b.** Yalnız çizilen durum rolü gelir; kalanlar 013'e.
- **Karar 5 → süre setten çıktı**, animasyon da: şerit anında belirir.
  Yol haritasının "(+ blok animasyonları)" kalemi ertelenmiş borç olarak
  yazılır. Gerekçe panelin üç merceğinde ortak: `bt-gpu::motion` bugün genel
  bir saat değil ve `make duman` şerit yolunu hiç görmüyor, yani ikinci
  animasyonun boşta-sıfır-kare hatası sessiz kalırdı.
- **Karar 6 → ortadan kalktı.** `command_gutter` bu sette yok, dolayısıyla
  ayar yardımcısı borcu da tetiklenmiyor. İkisi gerçekten istenince birlikte,
  kendi commit'inde gelir.
