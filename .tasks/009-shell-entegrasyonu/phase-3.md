# Phase 3 — zsh sarmalayıcısı, kabuk kararı ve kaçış kapısı

## Özet

Kabuk işaretleri gerçekten basmaya başlıyor: zsh `ZDOTDIR` ile bizim
dizinimize yönlendiriliyor, betik kullanıcının dosyalarını devrediyor ve
kullanıcının kapatma anahtarı aynı phase'de iniyor.

_Requirements: R3.1, R3.2, R3.3, R3.4, R4.1, R4.2, R5, R6.2_

## Değişiklikler

- **`assets/shell/` (yeni)** — zsh sarmalayıcısı. Zorunlu davranışı:
  - zsh'in **beş** dosyasını da devreder (`.zshenv`, `.zprofile`, `.zshrc`,
    `.zlogin`, `.zlogout`). Oturum **login kabuk**, yani `.zprofile` gerçekten
    okunuyor; yalnız `.zshrc`'yi devreden bir betik kullanıcının PATH'ini
    (Homebrew, nvm) sessizce düşürürdü.
  - Kullanıcının **özgün `ZDOTDIR`'ını geri koyar** (yoksa `unset`) ve gerçek
    dosyalarını oradan yükler; içeride açılan kabuklar bizim dizinimize
    yeniden girmez.
  - **Hiçbir kolda ölümcül değildir**: eksik ya da hata veren kullanıcı
    dosyası kabuğu düşürmez, `exit` yok. Gerekçe sert: çocuk ölünce uygulama
    kapanıyor (`child_exit` → `terminate:`), yani bozuk bir betik kullanıcıyı
    Settings…'e bile ulaşamaz bırakırdı.
  - Kullanıcının rc dosyalarına **yazmaz** — `make denetim`'in kapısı.
  - İşaretleri zsh'in kendi kancalarıyla basar (`precmd`/`preexec` ve çıkışta
    kod); ürettiği dizi **standart** OSC 133'tür.
- **`crates/bt-shell/src/child.rs`** — `child::shell()`: hangi kabuğun
  koşacağı **spawn'dan önce** burada çözülür, çünkü alacritty `$SHELL`'i
  `tty::new`'un içinde çözüyor. İkinci bir çözüm doğuyor; doc'u alacritty'nin
  sırasıyla **paritesini** ve ayrıştığı kenarı yazar (emsal: aynı dosyadaki
  `home()`). zsh değilse sarmalayıcı kurulmaz.
- **`crates/bt-shell`** — betiğin yolu: pakette `Contents/Resources`, debug
  derlemede depo yolu. Geliştirme `cargo run` ile koşuyor; yalnız pakete
  bakan bir çözüm özelliği en çok koştuğumuz yolda kapatırdı.
- **`crates/bt-core/src/settings.rs` + `crates/bt-shell`** — `[shell]
  integration = "auto" | "off"`, varsayılan `"auto"`. **Kayıt anında
  uygulanmaz** ve bu sözleşmenin ilk istisnası: kabuk çoktan doğmuş, anahtar
  **sonraki oturumda** geçerli. `Settings::changes`'e kol takılmaz. Anahtarın
  anlamı "sarmalayıcıyı kurma"; işaretleri ayrıştırmak her hâlde serbest
  kalır — başka bir aracın bastığı gerçek OSC 133 de görülsün.
- **Hermetiklik** — süreli koşu (`BT_RUN_SECONDS`) sarmalayıcıyı **hiç
  kurmaz**. `make duman`'ın sonucu ölçen makinenin kabuk yapılandırmasına
  bağlanamaz; emsal ve kapı `Inputs::Hermetic`.

## Kabul

- Gerçek bir zsh oturumunda işaretler geliyor: prompt'ta `shell_state()`
  "prompt'ta", komut koşarken "çalışıyor", bittikten sonra çıkış kodu
  yerinde. Bu, **gözle** doğrulanır (gerçek pencere) ve kullanıcıdan geçer.
- Kullanıcının yapılandırması yaşıyor: kendi `.zprofile`/`.zshrc`'si
  yükleniyor, `$ZDOTDIR` kabuğun içinde özgün değerinde, `echo $PATH`
  entegrasyonsuz oturumla aynı.
- Bozuk kullanıcı dosyası kabuğu düşürmüyor (kasıtlı hata veren bir
  `.zshrc` ile denenir).
- `integration = "off"` yazıp **yeni** oturum açınca `ZDOTDIR` hiç
  kurulmuyor; açık oturum etkilenmiyor.
- `make duman` yeşil ve entegrasyon orada **hiç** kurulmuyor; kapıyı
  closure'ı panikleyen bir sınama tutuyor.
- zsh olmayan bir kabukla oturum bugünkü gibi açılıyor (`child::shell()`
  sınaması).

## Yayın Etkisi

**ayar şeması** — yeni anahtar `[shell] integration`, varsayılan `"auto"`;
eski anahtar yok, silinen anahtar yok, bilinmeyen anahtar korunur.
`docs/AYARLAR.md` bu phase'de güncellenir ve **"sonraki oturumda geçerli"**
ile "uygulama açılmıyorken nasıl kapatılır" satırlarını içerir.

**shell entegrasyonu** — üç kabuktan yalnız **zsh**; gerekçe kayıtlı
(`discussion.md` → Karar 6): seviye 1'i üç kabuğa yaymak, dock'un veri yolu
tasarlanmadan yapılırsa iki kez yazılır. Kullanıcı rc dosyasına dokunulmaz.

**`CLAUDE.md`** — `bt-shell` satırındaki `libc` tarifi bu phase'de çelişti
(`getpwuid_r` eklendi) ve aynı commit'te düzeldi; `bt-shell/Cargo.toml`'daki
gerekçe de. "Shell entegrasyonu" maddesinin kendisi phase-4'te güncelleniyor.

shader yok · terminfo yok (`TERM` `xterm-256color` kalıyor) · tema yok ·
app bundle **sonraki phase'de** (betik pakete orada girer) · **yeni bağımlılık
yok**: `libc` grafta ve `bt-shell`'in listesinde zaten vardı, `Cargo.lock`
değişmedi.

## Uygulama Notları

- **Sarmalayıcı beş değil dört dosya, çünkü R3.1 ile R3.2 aynı anda
  sağlanamıyor.** ZDOTDIR bizi gösterdiği sürece bizim dosyalarımız okunuyor;
  geri koyduğumuz an kalanları zsh kullanıcının dizininden okuyor. Yani "beş
  dosyamız da koşsun" ile "ZDOTDIR oturum etkileşimli olmadan geri konsun"
  birbirini dışlıyor — beşincisini koymak, ZDOTDIR'ı bütün oturum boyunca
  çocuklara (tmux, iç içe kabuk) sızdırmak olurdu. Çözüm gereksinimin
  **amacını** tam olarak karşılıyor: kullanıcının beş dosyası da yükleniyor,
  dördü bizim devrimizle, `.zlogout` zsh'in kendisiyle. Geri koyma `.zshrc`
  ile `.zlogin`'de, hangisi önce okunursa; üçüncü bir kol `.zshenv`'de, çünkü
  `setopt no_rcs` diyen bir kullanıcı dosyasından sonra zsh başka hiçbir
  dosya okumuyor. Üç kolun dördü de gerçek bir zsh'te tek tek koşturuldu.
- **`__bateri_load` kullanıcının dosyasını yükledikten sonra `ZDOTDIR`'ı
  yeniden okuyor** ve bu bir kenar hâli değil **ana** yol: bir kullanıcının
  ZDOTDIR'ı olmasının en yaygın sebebi `~/.zshenv` içindeki
  `export ZDOTDIR=…` satırı. Okumasaydık kalan dosyaları eski dizinden arar,
  yani tam da taşınmış yapılandırmayı kaçırırdık. Sınamanın sahte evi bu
  yüzden `.zshenv`'de ZDOTDIR atıyor.
- **`child::shell()` passwd'ye düşmek *zorunda*, plan bunu sormamıştı.**
  Ölçüldü: `launchctl getenv SHELL` **boş**, yani Dock'tan açılan pakette
  `$SHELL` yok. Yalnız `$SHELL`'e bakan bir çözüm entegrasyonu tam da sevk
  edilen üründe kapatır, geliştirmede (`cargo run`) açık bırakırdı — ve
  belirti, tasarımın bilerek sessiz yaptığı geri düşüşle aynı olurdu.
  `libc::getpwuid_r` yeni bağımlılık değil (`libc` zaten listede) ama
  `CLAUDE.md`'nin `bt-shell` satırı libc'yi "yalnız bekçi + `O_EVTONLY`" diye
  tarif ediyordu; çelişen cümle aynı commit'te düzeldi. Aynı tuzağın `LANG`
  hâli 006'da yaşanmıştı.
- **Kapı `child`'da değil `app`'te** (`shell_integration_env`): kapının ilk
  katı `Inputs` ve o `app`'e özel. `resolve_reduce_motion` emsali — orada da
  sistemi okuyan katman `bt-shell`, kararı `Inputs` kapılıyor.
- **Sözleşmenin ikinci yarısı `BATERI_ZDOTDIR`.** Plan yalnız `ZDOTDIR`'ı
  konuşuyordu, oysa betiğin geri koyacağı değeri bir yerden alması gerek ve
  o değer kabuk doğmadan **önce** biliniyor. Yokluğu bilgi taşıyor:
  "kullanıcının da yoktu", yani geri koyarken `unset` — `$HOME`'a eşitlemek
  ihraç edilen bir değişken yaratmak olurdu.
- **`docs/AYARLAR.md`'nin "uygulama açılmıyorken nasıl kapatılır" tarifi
  dosyanın sonuna ekleme *olamaz*.** Şablon artık `[shell]` taşıyor ve TOML
  aynı bölümün tekrarını reddediyor: `>>` ile eklenen iki satır dosyanın
  **tamamını** okunamaz yapardı — yani kullanıcıyı çıkarmaya çalıştığı
  delikten daha derinine sokardı. Tarif "satırı düzelt, bölüm yoksa ekle"ye
  döndü ve tekrarın sonucu açıkça yazıldı.
- **`is_zsh("/bin/zsh/")` `true` ve bu bilerek sınanıyor.** `Path::file_name`
  sondaki eğik çizgiyi yutuyor; soru sorulmaya değmez, çünkü o yol bir dizin
  ve exec düşüyor — oturum hiç açılmıyorsa entegrasyonun kurulup
  kurulmadığının anlamı yok. Sınama yanlış bir iddiayı çivilemek yerine
  gerçeği yazıyor.
- **Sistemin rc dosyası bizden önce okunuyor ve `HISTFILE`'ı kaçırıyordu —
  bu bir kusurdu, kodda oluştu ve izini bıraktı.** macOS'un `/etc/zshrc`'si
  her aşamada bizim dosyamızdan **önce** koşuyor, yani `ZDOTDIR` hâlâ bizi
  gösterirken `HISTFILE=${ZDOTDIR:-$HOME}/.zsh_history` diyor. İlk yazımdan
  sonra `assets/shell/zsh/.zsh_history` gerçekten doğdu (dört `false`/`exit`
  satırıyla): kendi rc'sinde `HISTFILE` yazmayan her kullanıcının geçmişi
  uygulamanın paketine yazılır, kendi dosyası donar ve hiçbir yerde uyarı
  çıkmazdı. `__bateri_load` artık değeri kullanıcının dosyası yüklenmeden
  **önce** düzeltiyor (önce, çünkü kullanıcının rc'si `HISTFILE`'ı okuyup
  üstüne kurabiliyor) ve yalnız bizim dizinimizi gösteren değere dokunuyor.
  Sınama önce kırmızı koşturuldu: düzeltme kapatıldığında `.zlogin`'in gördüğü
  yol depo dizinini gösteriyor.
- **Aynı kökün ikinci kalemi bilerek bırakıldı:** `/etc/zshrc`
  `${ZDOTDIR:-$HOME}/.zkbd/${TERM}-${VENDOR}` de arıyor, yani `~/.zkbd` ile
  tuş bağlaması üretmiş bir kullanıcı onu yükleyemiyor ve terminfo
  varsayılanına düşüyor. Salt okuma, veri kaybı yok; çaresi sistemin rc
  mantığını kopyalamak olurdu — macOS sürümüne bağlı, kırılgan bir tekrar.
  Kayıt `bateri.zsh`'in başlığında.
- **Gözle doğrulamanın ilk turu bir kusur değil, kör bir ölçüm buldu.**
  `printf '%s' "$PS1"` komutu `B` işaretini göstermiyor ve gösteremez: iTerm2
  kabuk entegrasyonu (kullanıcının `.zshrc`'sinde) **preexec'te** `PS1`'i ham
  değerine geri koyuyor (`~/.iterm2_shell_integration.zsh:149`), yani komut
  koşarken ne bizim ekimiz ne onunki `PS1`'de. Ölçüm ham PTY akışına taşındı:
  gerçek yapılandırmayla açılan bir oturumda `A`, `B` ve `D` akıyor.
- **Aynı turda idempotanlık kuralının değeri ölçüldü.** `A` iki kez geliyor
  (bizimki + iTerm2'nin `PS1`'e gömdüğü), `B` **bir** kez: iTerm2
  `$(iterm2_prompt_end)` ile gerçek baytları `PS1`'in içine koyuyor ve bizim
  `[[ $PS1 == *…B…* ]]` koşulumuz onu görüp ikinci bir ek yapmıyor. Koşul
  "tema PS1'i yeniden kurarsa" diye yazılmıştı; başka bir aracın işaretine
  saygı duyması bedava çıkan ikinci bir kazanç. Yinelenen `A` zararsız —
  durum geçişi idempotan (`ShellState::apply`).
- **`PATH` ikilenmesi sarmalayıcıdan değil.** Ölçüldü: aynı dizinden
  `zsh -l -i -c 'echo $PATH'`, sarmalayıcılı ve sarmalayıcısız, **birebir
  aynı** (2042 bayt) ve ikilenme ikisinde de var. Kaynağı iç içelik —
  `cargo run` dış kabuğun `PATH`'ini miras veriyor, çocuk zsh kullanıcının
  koşulsuz ekleyen yapılandırmasını bir kez daha koşturuyor. Terminal.app'te
  görünmemesinin sebebi onun launchd'den (temiz ortamla) açılması; paketten
  açılan bateri de aynı yerden açılıyor.
- **`login -flp` ortamı gerçekten geçiriyor**, elle doğrulandı: alacritty
  macOS'ta kabuğu `/usr/bin/login -flp $USER /bin/zsh -fc "exec -a -zsh …"`
  ile açıyor ve `ZDOTDIR` ile `BATERI_ZDOTDIR` öbür uçta yerinde. Aradaki
  `zsh -f` rc dosyası okumuyor, yani sarmalayıcı iki kez koşmuyor.

## Checklist

- [x] `assets/shell/` zsh sarmalayıcısı (dört dosya + ortak gövde; `ZDOTDIR`
      geri konur, hiçbir kolda ölümcül değil — beşinci dosyanın neden
      olmadığı Uygulama Notları'nda)
- [x] `child::shell()` + parite doc'u
- [x] Betiğin yolu: paket + debug geri düşüşü
- [x] `[shell] integration` + `docs/AYARLAR.md`
- [x] Hermetik koşu entegrasyonu kurmuyor
- [x] Test: `child::shell()` kolları (`zsh_is_recognized_by_name_not_by_path`,
      `this_user_has_a_resolvable_shell` — ikincisi passwd yarısını tek başına
      koruyor); hermetiklik ve `"off"` (panikleyen closure'lar);
      sarmalayıcının kullanıcı dosyalarını yüklemesi, bozuk `.zshrc`'de
      düşmemesi ve işaretlerin gelmesi tek gerçek zsh turunda
      (`the_zsh_wrapper_loads_the_users_files_and_reports_marks`)
- [x] Gözle doğrulama (kullanıcı, gerçek pencere): `$ZDOTDIR` boş, `$HISTFILE`
      kendi dizininde, `$PATH` bozulmamış (ikilenmesi ölçülerek sarmalayıcıdan
      değil iç içelikten olduğu gösterildi), `__bateri_precmd` kancalarda ve
      **en sonda**. İşaretlerin aktığı ham PTY akışından doğrulandı — `PS1`
      üstünden bakan ilk ölçüm kördü (Uygulama Notları)
- [x] Doğrulama geçti (`make hepsi` + `make duman`; duman'ın sabitleri
      phase-2'dekiyle birebir: `hucre=8 glif=6 kural=15 yuva=13/2048`,
      `kapanis=clean`)
- [x] Yayın etkisi yazıldı
