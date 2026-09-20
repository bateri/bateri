# Fare raporlama — Tartışma

Tek bariz yaklaşım var, ama birbirinden bağımsız **beş karar noktası**
taşıyor. Bu yüzden seçenek biçimi değil karar-listesi.

**Yaklaşım:** raporu `bt-core`'da genelleştir, **kararı da `bt-core`'da ver**
(`input::button_route`, `wheel_route`'un yanında), cevabı enum olarak döndür;
`view.rs` yalnız AppKit çevirisi kalsın. Kapının yeri panelden **sonra**
değişti — gerekçesi `## Muhakeme` → İtiraz 1.

## Karar 1: Seçim ile uygulamanın faresi arasındaki arbitraj

Uygulama fareyi isterse tıklama kime gider? Bugün cevap koşulsuz "seçime";
setten sonra koşulsuz "uygulamaya" olursa Claude Code'un ya da vim'in
**içinde metni fareyle hiç seçemeyiz**.

**A. Shift terminali geri alır.** Fare kipi açıkken tıklama uygulamaya gider;
Shift basılıysa gitmez ve seçim başlar. xterm'in konvansiyonu; iTerm2, kitty,
WezTerm ve ghostty aynısını yapıyor.

- **Artı:** kullanıcının başka terminallerden bildiği refleks; uygulama
  içinde metin seçmenin **bir** yolu kalıyor.
- **Eksi:** depodaki tekerlek kuralıyla **asimetrik** — `wheel_route`'un
  bekçisi `mouse_mode_comes_first_on_either_screen` (`input.rs:157`) "1007 ve
  Shift fare kipini geçersiz kılmaz" diyor.
- Asimetri savunulabilir ve bekçinin öznesi **yalnız `wheel_route`**:
  tekerlekte Shift'in üstüne binecek bir şey yok (kaydırma zaten terminalin),
  düğmede iki gerçek tüketici yarışıyor. Ayrıca macOS klasik farede
  Shift+tekerlek yatay deltaya dönüyor (`view.rs:469-471`), yani o koldaki
  Shift zaten güvenilmez.

**B. Fare kipi her zaman kazanır.** Tekerleğin bugünkü kuralını düğmeye de
uygula.

- **Eksi:** uygulama içinde fareyle metin seçmek **imkânsız** olur.
  Kullanıcının bildirdiği sorunu çözerken sahip olduğu bir yeteneği
  kaybettirir.

**C. Bugünkü hâl** — sorunu hiç çözmüyor, tamlık için yazılı.

**Öneri: A**, ve yanında panelden gelen bir **daraltma**: sürükleme yalnız
1002/1003 istendiğinde uygulamanın. Yalnız `1000` açan uygulama sürüklemeyi
zaten istemiyor, yani o kolda seçimi terminale bırakmak hiçbir şey
kaybettirmiyor — deponun kendi ölçütüyle "yanlışın yönü güvenli". Yan kazanç
Karar 2'de: phase-1'in ara durumu kendiliğinden tutarlı oluyor.

**Asimetrinin bekçisi zorunlu:** "Shift düğmeyi geçersiz kılar, tekerleği
kılmaz" sınaması `mouse_mode_comes_first_on_either_screen`'in **yanında**
durmalı. Aksi hâlde altı ay sonra biri tekerleğin bekçisini okuyup kuralı
düğmeye de uygular, `make hepsi` yeşil kalır ve yetenek sessizce ölür.

## Karar 2: Hareket raporu (1002/1003) bu sette mi?

Bildirilen belirti için **yalnız bas/bırak** yeter: tıklanan yere imleç
gelmesi 1000 seviyesidir.

**A. Aynı set, ayrı phase.** Düğme phase-1'de belirtiyi kapatır; hareket
phase-2'de gelir. İkisi de tek başına `make hepsi`'yi yeşil bırakır.

- **Artı:** belirti ilk commit'te kapanıyor. Karar 1'in daraltmasıyla
  birlikte ara durum **katıksız ekleme**: phase-1'den sonra Shift'siz
  sürükleme hâlâ seçim yapıyor (çünkü sürüklemeyi ancak 1002/1003 alır ve o
  phase-2'de geliyor), tıklama ise uygulamaya gidiyor.
- **Eksi:** yok sayılır.

**B. Tek phase.** Phase tek commit'e sığmaz; `bt-core`'un saf tablosu ile
AppKit'in olay yüzeyi aynı incelemede boğulur.

**C. Hareket ayrı sete.** Aynı kancalar iki kez açılır, Karar 1 ikinci sette
yeniden tartışılır.

**Öneri: A.**

## Karar 3: Doldurma bandının üstündeki tıklama

017'nin bandı ekrandayken oraya yapılan tıklama rapora ne olarak girer?
Tekerlek bu soruyu **0. satır** diye çözmüş, ama gerekçesi tekerleğe özel:
reddetmek "band ekrandayken kaydırmanın tamamını" öldürürdü.

**A. Reddet.** Bandın satırları geçmişte ve uygulamanın ekranında yok; 0.
satır demek uygulamaya **yanlış** bir yer söyler. Seçimin oradaki kuralıyla
aynı (`point_to_cell`, `fill > 0` → `None`, gerekçesi yazılı: "yanlış seçilir
ile seçilemez arasında dürüst olan ikincisi").

**B. 0. satır olarak gönder.** Tekerleğin kuralını sürdür — ama tıklamada
reddin bedeli yok: kaydırma ölmüyor, yalnız o birkaç satırdaki tıklama
düşüyor.

**Öneri: A**, ve **bedava geliyor**: `session_cell` bandın üstünde zaten
`None` dönüyor (`view.rs:1028-1034`), `mouseDown:` zaten erken dönüyor. Sıfır
satır kod.

## Karar 4: Hareket olaylarını ne zaman dinleyelim

`mouseMoved:` yalnız pencere `setAcceptsMouseMovedEvents:` ile açıldığında
geliyor. **`NSTrackingArea` gerekmiyor** (panel düzeltmesi): o yalnız
`mouseEntered:`/`mouseExited:` ve cursor rect için gerekli, ikisi de bu sette
istenmiyor; view zaten first responder (`view.rs:345`), yani pencere
seviyesindeki olay ona geliyor.

**A. Her zaman aç, `mouseMoved:` kipe bakıp erken dönsün.**

- **Artı:** tek satır, yeni haberci yok, yeni paylaşılan durum yok.
- **Eksi:** fare istemeyen pencerede de hareket başına bir olay — ve kip
  sorusu `Term` kilidi altında olmak zorunda, yani **hareket başına bir
  kilit**. Ölçülmedi.

**B. Kipe göre aç.** Kare yolu kip değişimini fark edip `bt-shell`'e haber
versin (`notify_alt_screen_changed` örüntüsü), ya da kip `fill_shown:
AtomicU16` örüntüsüyle yayınlansın (`session.rs:1555`).

- **Eksi:** ikinci bir haberci, ya da yeni paylaşılan durum — ikincisi o
  phase'e `make test-yaris` ekler (`proje.md` doğrulama tablosu).

**Öneri: A**, ve bu bir **ölçüm sözü değil adlandırılmış geri dönüş**: fare
hareketi başına ana thread maliyetini ölçecek kanca depoda yok
(`BT_INPUT_LATENCY_SAMPLES` hâlâ borç). Belirti görülürse B — iki yönlü kapı,
tek yönlü değil.

## Karar 5: Raporun seçime, pencereye ve rotaya etkisi

Panelin çıkardığı karar; ilk turda hiç sorulmamıştı. Üç yarısı var ve üçü de
aynı aileden.

**5a — Rapor seçimi temizler mi, pencereyi dibe döndürür mü?** Depoda iki
karşıt ve **bekçili** kural var: `write_owned` girdiyi "seçimi temizler,
dibe döner" diye işliyor (`input_clears_the_selection`), `scroll_wheel`'in
rapor kolu ise `send`'den geçip ikisini de yapmıyor
(`wheel_and_replies_keep_the_selection`). Düğme raporu **üçüncü** vaka.

- **Öneri: tekerleğin kolu.** Rapor kullanıcının yazdığı bir şey değil,
  uygulamaya iletilen bir olay; dibe dönmek geçmişe bakan pencereyi fırlatır.
- Karşı taraf gerçek ve yazılmalı: uygulama rapordan sonra ekranını yeniden
  çizerse vurgu aynı hücrelerde kalır, altındaki metin değişir ve Cmd-C
  yanlış metni kopyalar. Bu `write_owned`'ın doc'undaki gerekçenin ta
  kendisi — ama orada özne **kullanıcının girdisi**, burada uygulamanın
  çizimi, ve aynı şey tekerlek raporunda da oluyor ve kabul edilmiş.

**5b — Rota basışta kilitlenir mi?** `view.rs`'te `dragging: Cell<bool>`
zaten "bu basış seçim başlattı" bilgisini tutuyor. Shift her olayda
okunursa sürüklemenin ortasında Shift'i bırakmak seçim-sürüklemesini
rapor-sürüklemesine çevirir.

- **Öneri: `mouseDown:`'da kilitlen.** Jest başladığı kolda biter.

**5c — Cevap `bool` olamaz.** İki ayrı "hayır" var: *kip kapalı* (seçim
başlamalı) ve *kip açık ama rapor düştü* (koordinat kodlamaya sığmadı ya da
satır geçmişte — `session.rs:3088-3095`'teki bugünkü kapı). `bool` ikisini
birleştirirse geçmişe kaydırılmış pencerede fare kipindeki tıklama sessizce
seçim başlatır.

- **Öneri: `Wheel` emsali** — üç varyantlı enum (`Sent` / `Select` /
  `Ignored`), çünkü `bt-shell` üç cevapta üç ayrı şey yapıyor
  (`Wheel`'in doc'u bu şekli zaten gerekçelendiriyor).
- **Yan sonuç yazılmalı:** geçmişe kaydırılmış pencerede, fare kipinde
  tıklamak artık **hiçbir şey yapmaz** (bugün seçim başlatıyor).

**5d — Kısmanın durumu nerede yaşar?** (phase-2) Rapor hücre değişiminde
kısılmak zorunda. Durum `Session`'a alan olarak düşerse `bt-core` fare
konumunu tutmaya başlar.

- **Öneri: `ViewIvars`'ta bir `Cell<Option<(u16, u16)>>`** (`dragging`
  emsali), karşılaştırma **görünür pencere** hücresinde. xterm de ekran
  konumunda kısıyor; uygulamanın kendi kaydırması işaretçi dururken rapor
  üretmemeli.

## Kapsam dışı

- **`?1004` odak raporu.** İlk turda içerideydi, panel çıkardı: setin kendi
  kapsam kuralı (`?2031` için "konusu fare değil tema") kelimesi kelimesine
  odak için de geçerli — konusu fare değil odak. Dosyaları da ayrı (odak
  `app.rs`'te, fare `view.rs`'te; ortak dosya yalnız `session.rs`) ve bedeli
  "~20 satır" değil: `CLAUDE.md`'de **adıyla yazılı** bir mimari kararın
  ("Odak `bt-core`'a hiç girmiyor") düzeltilmesi ve süreli koşunun
  (`apply_focus`'un `run.is_some()` erken dönüşü) yeni bir mini kararı.
  Kullanıcı bir odak belirtisi de **bildirmedi**. Yol haritasına borç.
- **`?2031` tema değişimi bildirimi.** Aynı ölçümde çıktı, sinyali elimizde,
  ama konusu fare değil tema. Yol haritasına borç.
- **Yatay tekerlek raporu (66/67)** ve **SGR-pixel fare (1016)**.
- **Çift/üçlü tıkla kelime ve satır seçimi** — fare kipinden bağımsız.

## Muhakeme (2026-09-21)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üç jüri de yönü onayladı (rapor gövdesi zaten genel, plan `bt-core`'a
platform sızdırmıyor, ölçüm gerçek) ve üçü de **kenarlara** itiraz etti.
Kabul edilen itirazların hepsi kodda doğrulandı.

**Kabul edilen itirazlar → plan değişikliği:**

- **Kapı `view.rs`'e değil `bt-core`'a** (üç jüri de; Codebase-fit ve İşletme
  ayrı ayrı kanıtladı) → yaklaşım cümlesi değişti. Üç gerekçe: (1) depoda
  "kipi dışarıdan sor" örüntüsü bir kez düşünülüp **reddedilmiş** —
  `bracketed_paste` `pub` değil ve doc'u "kip dışarıdan sorgulansaydı biri
  `write`'a ham bayt vererek kapıyı by-pass edebilirdi" diyor
  (`session.rs:3576`); (2) `view.rs`'in `define_class!` gövdeleri
  **sınanamıyor** — dosyanın sınamalarının tamamı serbest fonksiyon üstünde,
  `mouseDown:` için bekçi yazılamaz; (3) kapı dışarıda olursa kip sorusu ile
  `set_selection` arasında iki ayrı `Term` kilidi doğar — Karar 5'in B'de
  reddettiği yarışın aynısı. İlk turdaki cümle `context.md:111-112` ile de
  çelişiyordu ("düğme de aynı kapıdan geçmeli").
- **Seçim/dibe-dönüş politikası seçilmemiş** (Codebase-fit + İşletme) →
  Karar 5a. Depoda iki karşıt ve bekçili kural var; set üçüncü vakayı
  getiriyor ve seçmemişti.
- **Cevap `bool` olamaz** (Sadelik kendi sketch'inde düzeltti; İşletme aynı
  senaryoyu bağımsız buldu) → Karar 5c. Kaydırılmış pencerede sessiz seçim
  başlatma riski gerçek.
- **Rotanın basışta kilitlenmesi** (İşletme) → Karar 5b.
- **Kısmanın durumu sahipsiz** (Sadelik) → Karar 5d. Yeri `ViewIvars`.
- **`?1004` kapsam dışı** (Sadelik) → Kapsam dışı'na taşındı, Karar 3 silindi.
  İlk turdaki "aynı dosyalara dokunuyor" gerekçem **yanlıştı**: doğrulandı,
  odak `app.rs`'te ve `view.rs`'te sıfır geçiş.
- **`NSTrackingArea` gereksiz** (Sadelik) → Karar 4 sadeleşti. View zaten
  first responder.
- **Sürükleme kapısı daraltılsın** (İşletme) → Karar 1'e fıkra. Sürükleme
  yalnız 1002/1003'ün; yan kazanç phase-1'in ara durumunun katıksız ekleme
  olması.
- **Doğrulama kör** (İşletme) → plana yazılacak: duman kabuğu sabit bir betik
  ve hiçbir fare kipi açmıyor, depoda fare olayı enjekte eden kanca yok, yani
  `make duman` fare yolu tamamen ölü olsa da yeşil düşer. Burada duman bir
  **regresyon alarmı**, kapsama kapısı değil; gerçek doğrulama gözle kontrol
  (012/013 emsali).
- **Sağ/orta tuş fiyatlanmamış** (İşletme) → doğrulandı: depoda
  `rightMouse*`/`otherMouse*` **sıfır**. Altı selector daha demek; phase-1'in
  kapsamına yazılacak.
- **`input.rs` modül başlığı eskiyor** (İşletme) → "ok tuşu ve tekerlek
  raporu" artık doğru değil; aynı commit'te düzelir.

**Reddedilenler:**

- **Kipi `AtomicU16` ile yayınlamak** (İşletme, basitleştirme 2) — Karar 4'ün
  B kolunda kayıtlı ama şimdi alınmıyor: yeni paylaşılan durum demek ve o
  phase'e `make test-yaris` ekler. Ölçülmemiş bir kazanç için ölçülmüş bir
  bedel.
- **alacritty'nin `on_mouse_press`'ine paritenin doğrulanması**
  (Codebase-fit, not 2) — depoda doğrulanamıyor (`alacritty` binary crate
  bağımlılık grafında yok, yalnız `alacritty_terminal` var). Karar 1'in
  dayanağı xterm konvansiyonu olarak kalıyor; parite sorusu açık ve
  gerekçeyi zayıflatmıyor.

## Karar
