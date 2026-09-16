# Shell entegrasyonu ve komut durumu — Tartışma

## Karar 1: İşaretleri nereden yakalıyoruz?

OSC 133 `vte`'de yok ve `Handler`'da "bilinmeyen OSC" kancası da yok
(`context.md` → Kanıt). Bayt akışını görmenin dört yolu var.

### Seçenek A: PTY'yi saran bir dinleyici

`EventLoop` PTY tipinde jenerik. Gerçek `Pty`'yi saran, `reader()`'ı kendi
okuyucusuyla değiştiren bir tip yazıyoruz; okuyucu baytları olduğu gibi
geçiriyor ve geçerken küçük bir durum makinesiyle OSC 133'ü tarıyor.

**Artıları:**
- Fork yok, yama yok, upstream beklemek yok.
- Küçük yüzey: iki trait'in altı metodu delege edilir, tarayıcı saf ve
  sınanabilir (chunk sınırında bölünen dizi dahil).
- Kapanış tasarımına **dokunmuyor**: `EventLoop`'un kanalı, `join`'i ve
  `SHUTDOWN_GRACE` ölçümü olduğu gibi kalıyor.
- Okuma okuyucu thread'inde; render yolu görmüyor.

**Eksileri:**
- **Çıpa kesinliği yok.** `pty_read` birden çok `read()`'i tek bir
  `advance()`'te işleyebiliyor (`event_loop.rs:120-155`), yani "işaret
  görüldü" ile "ızgara o noktaya geldi" aynı an değil. Durum sorusu (prompt'ta
  mıyız) bundan etkilenmiyor — sıra korunuyor — ama "prompt hangi satırda
  başladı" sorusu **yaklaşık** oluyor.
- Yaklaşıklığın ne zaman bozulduğu ölçülebilir: işaretten sonra aynı tamponda
  bayt varsa çıpa şüphelidir. Olağan hâlde yoktur (kabuk prompt'u yazıp
  girdiye bloklanır); sel gibi çıktının ortasındaki işaretlerde vardır.
- Baytlar iki kez geziliyor (bir kez tarayıcı, bir kez ayrıştırıcı). Maliyeti
  ölçülmedi; tarayıcı bayt başına tek karşılaştırma mertebesinde.

### Seçenek B: Kendi okuma döngümüz

`EventLoop`'u bırakıp PTY'yi kendimiz okuyoruz, `vte::ansi::Processor`'ı
kendimiz sürüyoruz ve ayrıştırmayı işaretin **tam üstünde** bölüyoruz.

**Artıları:**
- Çıpa kesin: işaretin geldiği anda `Term`'ün imleci tam o noktada.
- Akış üstünde başka işler de mümkün olur (ileride kayıt, tekrar oynatma).

**Eksileri:**
- `EventLoop` yalnız okuma değil: yazma kanalı (`Msg`), `OnResize`, çocuk
  olayları, `drain_on_exit` ve kapanış sırası onda. Hepsi bize geçer.
- **Kapanış tasarımı ölçülmüş bir denge** (`CLAUDE.md` → kapanış maddesi:
  `SIGHUP`, ayrı thread, `SHUTDOWN_GRACE`, `kapanis=` jetonu). Yeniden yazmak
  o dengeyi ve onu tutan ölçümü çürütür.
- Bu setin kendi ihtiyacı (durum) için **gereksiz**; bedeli sonraki setin
  (bloklar) ihtiyacı için şimdiden ödemek olur.

### Seçenek C: `vte`/`alacritty_terminal` fork'u

**Eksileri:** bağımlılık mimari karardır ve fork bakım borcudur; her sürüm
yükseltmesi bizim yamamızı taşır. Reddedilmeli.

### Seçenek D: İşaretleri `vte`'nin taşıdığı bir dizinin içine saklamak

Betik OSC 133 yerine başlık (OSC 0/2) gibi zaten `Handler`'a ulaşan bir dizi
bassın.

**Eksileri:** anlam çakışması (gerçek başlıkla yarışır), standart dışı
(başka terminallerin/kullanıcının mevcut entegrasyonlarının bastığı **gerçek**
OSC 133'ü yine göremezdik) ve her yeni işaret için yeni bir kaçamak.

**Önerim:** **A**, çıpa sorusunu açıkça kayda geçerek. Tarayıcı işaretin
tamponun sonunda mı geldiğini bilsin; bloklar setine "çıpa kesin mi" sinyali
ölçülebilir bir veri olarak geçsin. Kesinlik gerçekten gerekirse **B** açık
kapı olarak kalsın — A'nın yüzeyi küçük olduğu için geçiş de ucuz.

## Karar 2: Durum nerede yaşıyor, sınırı nasıl geçiyor?

Kabuktan öğrenilen şey bir **oturum durumu**: prompt'ta mıyız, komut mu
koşuyor, sonuncusu hangi kodla bitti.

- **2a — `frame()`'in dönüşüne eklemek** (`Cursor`'un yanına). Tek kilit,
  tek sınır; çizen taraf zaten her karede oradan okuyor.
- **2b — ayrı bir sorgu** (`Session::shell_state()`), `frame()`'den bağımsız.

İkisinin de çözmesi gereken ortak sorun: **işaret tek başına kareyi
uyandırmıyor.** Komut bitip `D;0` geldiğinde ızgarada hiçbir hücre
değişmeyebilir; UI'ın o anı göstermesi gerekiyorsa hasar dikilmeli. Bu, boşta
sıfır kare sözleşmesine dokunan bir karar: her işaret kare istemez, **durumu
değiştiren** işaret ister.

**Önerim:** 2a (sınır zaten orada) + "durum değiştiyse hasar dik" kuralı.

## Karar 3: Bu set hangi seviyeye kadar kod indiriyor?

Seviye modeli (kullanıcının çerçevesi): 0 = entegrasyon yok, 1 = OSC 133
işaretleri, 2 = + satır editörünün canlı durumu (dock).

- **3a — 0 ve 1 kodlanır, 2 yalnız *kapı* olarak kalır** (tip ve karar var,
  veri yolu yok).
- **3b — 2'nin veri yolu da bu sette kodlanır** (zsh ZLE tamponunu bildirir,
  çekirdek saklar), tüketicisi sonra gelir.

**Önerim:** 3a. Tüketicisi olmayan bir protokol, yanlış protokol yazmanın en
bilinen yolu; dock'un ne istediği 014'ün `/rfc`'sinde netleşir. Bu sette
yapılacak iş, seviye 2'nin **mümkün** kalmasını sağlamak: UI hiçbir yerde
dock'un varlığını varsaymaz, seviye tipi baştan üç değerlidir.

## Karar 4: Betik nerede duruyor, geliştirmede nasıl bulunuyor?

Betik pakette (`bateri.app/Contents/Resources`) durmalı ki uygulamayla
birlikte güncellensin. Ama geliştirme `cargo run` ile koşuyor ve orada paket
**yok**.

- **4a — yalnız paket**: `cargo run`'da entegrasyon seviye 0'a düşer, yani
  geliştirirken özelliğin kendisi hiç koşmaz.
- **4b — debug derlemede depo yoluna düş** (`assets/shell/`), release'te
  yalnız paket.

**Önerim:** 4b. 4a, en çok koştuğumuz yolda özelliği kapatırdı.

Yanında iki kenar hâli: kullanıcının kendi `ZDOTDIR`'ı varsa sarmalayıcı onu
**korumalı** (bizim `.zshrc`'miz kullanıcının gerçek dosyasını yükler),
`zsh -f`/`--no-rcs` ile açılan kabukta entegrasyon **hiç** kurulmamalı.

## Karar 5: Ayar ve hermetiklik

- Anahtar `[shell] integration = "auto" | "off"`, varsayılan `"auto"`;
  kabuğu tanımıyorsak zaten seviye 0.
- **Süreli koşu (`BT_RUN_SECONDS`) entegrasyonu hiç okumaz ve kurmaz.**
  Ayar dosyası ve Hareketi Azalt için geçerli olan kural
  (`Inputs::Hermetic`) buraya da uygulanır: `make duman`'ın sonucu ölçen
  makinenin kabuk yapılandırmasına bağlanamaz.

## Karar 6: Kapsam sınırı

Bu sette **yok**: bash/fish betikleri (seviye 1'e sonraki sette çıkar), blok
UI'si (şerit, katlama, süre eşiği, kalkma animasyonu), Input Dock, prompt'u
terminalin çizmesi, tema değişiminde `LS_COLORS`/prompt renklerinin
güncellenmesi, gömülü tamamlama sözlüğü.

SSH'ın öte tarafı da yok ve bu kalıcı bir sınır: uzak makinede bizim
betiğimiz olmadığı için orada seviye 0'dır. UI bunu bir arıza gibi değil,
sessiz bir geri düşüş olarak göstermeli.

## Muhakeme (2026-09-16)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üçü de yaklaşımı (Karar 1 → Seçenek A) onayladı; itirazlar tasarımın
çevresinde toplandı.

**Kabul edilen itirazlar → plan değişikliği:**

- **Karar 2'nin "durum değişince hasar dik" kuralı siliniyor — premisi
  yanlıştı.** alacritty işlenen her bayt için `Wakeup` gönderiyor
  (`event_loop.rs:165`, `sync_bytes_count() < processed`), o da bizde hasarı
  dikip uyandırıyor (`session.rs:579`). OSC 133 de bir PTY baytı olduğuna göre
  işaret zaten bir kareyle geliyor. Dahası kural ters teperdi: tarayıcı
  `read()` içinde, yani `advance()`'ten **önce** koşuyor — kare isteseydi eski
  ızgara + yeni durumla fazladan bir kare çizerdi. **Tarayıcı hiçbir kare
  istemez**; bu bir kural değil, yapının kendisi ve sınaması da böyle yazılır.
  (Bilinen kenar: senkronize güncellemenin (DCS 2026) içinde kalan bir işaret
  o turda `Wakeup` üretmez; durum yine doğru, yalnız bir sonraki karede
  görünür.)
- **Karar 2 → 2b.** Durum `frame()` sınırına girmiyor; `Session::shell_state()`
  ayrı bir sorgu olarak, yaprak kilitte duruyor. Emsali `Session::theme()`
  (`session.rs:1461`): `frame()` kopyayı `Term` kilidinden önce alıyor ve
  tipin ayrıca bağımsız bir sorgusu var. Bu sette durumun **hiçbir tüketicisi
  yok** (blok UI'si kapsam dışı); sıfır tüketici için sınır imzasını
  değiştirmek `bt-gpu`'nun `Cursor` literalli kare sınamalarını da bedelsiz
  yere oynatırdı.
- **Seviye modeli tipte yaşamıyor.** Ayrımcı kanıt setin kendi metniydi:
  SSH'ın öte tarafı "seviye 0" derken yerel entegrasyon kurulu ve seviye 1 —
  iki kayıt, tek gerçeğin iki kaynağı. Durumun **yokluğu** seviye 0'dır
  (`Option<ShellState>`), seviye 2 sonradan yeni bir varyant değil bir **alan**
  olarak gelir. "Kurdum mu" bilgisi zaten `bt-shell`'de, çünkü kuran o.
  Seviye dili ürün ve belge düzeyinde kalıyor — kullanıcının çerçevesi
  korunuyor, ikinci bir kayıt doğmuyor.
- **`ZDOTDIR` beş dosyayı birden yönlendiriyor**, tasarım yalnız `.zshrc`'yi
  konuşuyordu. Oturum login kabuk, yani `.zprofile` gerçekten okunuyor; tek
  dosyalık bir sarmalayıcı kullanıcının Homebrew/nvm PATH'ini sessizce
  düşürürdü. Sarmalayıcı beşini de devreder, kullanıcının özgün `ZDOTDIR`'ını
  geri koyar (yoksa `unset`) ve **hiçbir kolda ölümcül olmaz** (`|| true`,
  `exit` yok) — çocuk ölürse uygulama kapanıyor (`app.rs`'in `child_exit` →
  `terminate:` yolu), yani bozuk bir sarmalayıcı kullanıcıyı Settings…'e bile
  ulaşamaz hâlde bırakırdı.
- **Paketleme kapısı kör.** Betiği taşımayan bir release, tasarımın bilerek
  sessiz yaptığı geri düşüşle **aynı belirtiyi** veriyor. Üstelik 4b bunu
  tersine çeviriyor: bozulabilen tek derleme (release) hiçbir kapının
  dokunmadığı tek derleme. `make kur`'un kopya ve içerik denetimine satır,
  `proje.md`'nin doğrulama tablosuna `assets/shell/*` satırı, girdi denetimi
  için `bundle_assets.rs` örüntüsü — üçü de bu setin işi.
- **`make denetim`'in rc dosya adı listesi eksik:** `.zshenv`, `.zlogin`,
  `.zlogout` yok, oysa bu setin yönlendirdiği dosyalar tam olarak onlar.
- **`[shell] integration` kayıt anında uygulanamaz** — kabuk çoktan doğmuş.
  Bu, "ayar kayıt anında uygulanır" sözleşmesinin **ilk istisnası**: anahtar
  "sonraki oturumda geçerli" olarak belgelenir, `Settings::changes`'e kol
  takılmaz. Anahtarın anlamı **"sarmalayıcıyı kurma"**; işaretleri ayrıştırmak
  serbest kalır (başka bir aracın bastığı gerçek OSC 133'ü görmek zarar değil
  kazanç). Uygulama açılmıyorken nasıl kapatılacağı `docs/AYARLAR.md`'ye yazılır.
- **Kabuğu spawn'dan önce kendimiz çözmeliyiz** ("bu zsh mi?"), çünkü alacritty
  `$SHELL`'i `tty::new`'un içinde çözüyor. İkinci bir çözüm doğuyor ve
  alacritty'nin sırasıyla paritesi yazılmalı; emsali `child::home()`
  (`child.rs:24-44`).
- **Tarayıcının akış maliyeti ölçülmemiş bir iddia**; phase'in
  `## Yayın Etkisi`'ne "ölçüm bekliyor: tarayıcının akış maliyeti" olarak
  girer, `/measure` oradan kapatır.

**Reddedilenler:**

- **"Sarmalayıcı `reader()`'ı delege edemez, `pty.file().try_clone()`
  zorunlu"** (Codebase-fit) — hayır: `Reader` sahipli bir ilişkili tip ama
  sarmalayıcının **kendisi** o tip olabilir (`type Reader = Self`,
  `fn reader(&mut self) -> &mut Self { self }`) ve `io::Read`'i içerideki
  `Pty`'ye delege eder. `Pty::reader()` `&mut File` döndürüyor
  (`unix.rs:371`), yani delege etmek için ödünç yeterli; ikinci bir fd'ye
  gerek yok. Bu red aynı zamanda bir kazanç: dup açılmadığı için `Pty::drop`'un
  `SIGHUP` penceresi ve `kapanis=` ölçümü gerçekten dokunulmamış kalıyor.
- **"Durum değişince `request_frame()` çağır"** (Codebase-fit) — yukarıdaki
  ilk kabul bunu gereksiz kılıyor; mekanizmanın var olması onu gerekli
  yapmıyor.
- **Bash/fish'in bu sette olmaması** panelde itiraz konusu olmadı; kural
  (`/audit` → shell entegrasyonu üç kabuğu birden kapsamalı) kapsam kararını
  değil **gerekçesizliği** yasaklıyor. Gerekçe kayda giriyor: seviye 1'i üç
  kabuğa yaymak, seviye 2'nin veri yolu tasarlanmadan yapılırsa iki kez
  yazılır.

## Karar (2026-09-16, kullanıcı onayı)

- **Karar 1 → Seçenek A (PTY'yi saran dinleyici).** `EventLoop` PTY tipinde
  jenerik; gerçek `Pty`'yi saran tip **kendisi** okuyucu oluyor
  (`type Reader = Self`), yani ikinci bir fd yok ve kapanış tasarımına
  dokunulmuyor. Reddedilenler: **B** (kendi okuma döngümüz) — ölçülmüş kapanış
  dengesini yeniden yazardı ve bu setin ihtiyacı için gereksiz; **C** (fork) —
  bağımlılık mimari karardır, fork bakım borcudur; **D** (işareti başka bir
  dizinin içine saklamak) — anlam çakışması ve standart dışılık, üstelik başka
  araçların bastığı gerçek OSC 133'ü yine göremezdik.
- **Karar 2 → 2b, "hasar dik" kuralı yok.** Durum `Session::shell_state()` ile
  yaprak kilitten okunuyor (`Session::theme()` emsali); `frame()` imzası
  değişmiyor. Tarayıcı **hiçbir kare istemiyor**: işaret zaten bir PTY baytı
  ve alacritty onun için `Wakeup` gönderiyor.
- **Karar 3 → 3a, seviye tipte yaşamıyor.** Seviye 0/1/2 bir **ürün dili** ve
  bu kaydın kavramı olarak kalıyor; kodda karşılığı durumun kendisi
  (`Option<ShellState>`; yokluk = seviye 0), seviye 2 sonradan yeni bir
  varyant değil bir **alan** olarak gelir. Ölçüt kullanıcının koyduğu ölçüttü
  — "sonradan başka kabuk eklenebilsin": bu şekilde fish eklemek yalnız bir
  betik yazmak oluyor, UI'da tek satır değişmiyor. Reddedilen: açık `enum`
  seviye — "kurdum mu" ile "işaret geliyor mu" iki ayrı kayıt olurdu ve SSH'ta
  çelişirlerdi.
- **Karar 4 → 4b**, beş dosyalık devirle: sarmalayıcı `.zshenv`, `.zprofile`,
  `.zshrc`, `.zlogin`, `.zlogout`'u devreder, kullanıcının özgün `ZDOTDIR`'ını
  geri koyar (yoksa `unset`) ve hiçbir kolda ölümcül olmaz.
- **Karar 5 → kabul**, bir düzeltmeyle: `[shell] integration` **kayıt anında
  uygulanamaz** (kabuk çoktan doğmuş), "sonraki oturumda geçerli" olarak
  belgelenir ve `Settings::changes`'e kol takılmaz. Anahtarın anlamı
  "sarmalayıcıyı kurma"; işaretleri ayrıştırmak her hâlde serbest.
- **Karar 6 → kabul.** bash/fish ve blok UI'si kapsam dışı; gerekçe
  `/audit`'in üç kabuk kuralına karşı kayda geçiyor: seviye 1'i üç kabuğa
  yaymak, seviye 2'nin veri yolu tasarlanmadan yapılırsa iki kez yazılır.
- **Panelden gelen işletme işleri sete giriyor:** paketleme kapısı (`make kur`
  kopya + denetim, `proje.md` doğrulama satırı, girdi denetimi),
  `make denetim`'in rc dosya listesinin genişletilmesi, uygulama açılmıyorken
  entegrasyonun nasıl kapatılacağının belgelenmesi.
