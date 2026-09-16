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

shader yok · terminfo yok (`TERM` `xterm-256color` kalıyor) · tema yok ·
app bundle **sonraki phase'de** (betik pakete orada girer) · yeni bağımlılık yok.

## Checklist

- [ ] `assets/shell/` zsh sarmalayıcısı (beş dosya, `ZDOTDIR` geri konur,
      ölümcül değil)
- [ ] `child::shell()` + parite doc'u
- [ ] Betiğin yolu: paket + debug geri düşüşü
- [ ] `[shell] integration` + `docs/AYARLAR.md`
- [ ] Hermetik koşu entegrasyonu kurmuyor
- [ ] Test: `child::shell()` kolları; hermetiklik (panikleyen closure);
      sarmalayıcının kullanıcı dosyalarını yüklemesi ve hata hâlinde
      düşmemesi
- [ ] Gözle doğrulama: gerçek zsh oturumunda durum oynuyor, `$PATH` ve
      `$ZDOTDIR` bozulmuyor
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
- [ ] Yayın etkisi yazıldı
