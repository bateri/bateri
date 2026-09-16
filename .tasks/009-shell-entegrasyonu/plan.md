# Shell entegrasyonu ve komut durumu — zsh

## Hedef

Kabuk, ne yaptığını terminale söylesin: prompt nerede başladı, hangi metin
kullanıcının komutu, komut ne zaman koştu ve hangi kodla bitti. Bu set o
kanalı zsh için açıyor ve kanalın **kabuktan bağımsız** yarısını çekirdeğe
koyuyor — sonraki kabuklar yalnız bir betik yazmakla eklenebilsin.

Ürün yüzeyi bu sette **yok**: ne blok çizimi, ne dock. Çıktısı, sonraki iki
setin (bloklar, Input Dock) üstüne kurulacağı durum ve onun sınanmış yolu.

## Gereksinimler

- **R1 — Bayt akışı görülebilir olur.**
  - **R1.1** — `Pty`'yi saran bir tip `EventLoop`'a olduğu gibi verilir;
    okuyucu **kendisidir** (`type Reader = Self`), yani ikinci bir fd
    açılmaz ve kapanış yolu (`SIGHUP`, `SHUTDOWN_GRACE`, `kapanis=`)
    değişmez.
  - **R1.2** — Sarmalayıcı baytları **değiştirmez**; akış aynen ayrıştırıcıya
    gider. Tarayıcı **hiçbir kare istemez** ve `Session`'a geri girmez.
- **R2 — OSC 133 ayrıştırılır ve durum tutulur.**
  - **R2.1** — `A`/`B`/`C`/`D` işaretleri tanınır; `D`'nin çıkış kodu okunur.
    Tanınmayan alt-işaret ve bozuk yük **yoksayılır**, panik yok.
  - **R2.2** — Tarayıcı chunk sınırında bölünen diziyi doğru birleştirir ve
    taşıma tamponunun bir **üst sınırı** vardır (kötü niyetli ya da bozuk bir
    akış belleği büyütemez).
  - **R2.3** — Durum `Session::shell_state()` ile yaprak kilitten okunur;
    `frame()` imzası **değişmez**.
  - **R2.4** — Durum kabuktan bağımsızdır: `bt-core`'da zsh'e (ya da herhangi
    bir kabuğa) özgü tek bir dal yoktur.
- **R3 — zsh sarmalayıcısı.**
  - **R3.1** — `ZDOTDIR` bizim dizinimizi gösterir; sarmalayıcı zsh'in
    **beş** dosyasını da (`.zshenv`, `.zprofile`, `.zshrc`, `.zlogin`,
    `.zlogout`) devreder ve kullanıcının gerçek dosyalarını yükler.
  - **R3.2** — Kullanıcının özgün `ZDOTDIR`'ı geri konur (yoksa `unset`), yani
    içeride açılan kabuklar ve tmux bizim dizinimize yeniden girmez.
  - **R3.3** — Sarmalayıcı **hiçbir kolda ölümcül değildir**: kullanıcının
    dosyası yoksa ya da hata verirse kabuk yine açılır.
  - **R3.4** — Kullanıcının rc dosyalarına **yazılmaz** (`make denetim`
    kapısı; listesi `.zshenv`/`.zlogin`/`.zlogout`'u kapsayacak şekilde
    genişletilir).
- **R4 — Kabuk kararı ve hermetiklik.**
  - **R4.1** — Hangi kabuğun koştuğu **spawn'dan önce** `bt-shell`'de çözülür
    (`child`); alacritty'nin `$SHELL` sırasıyla paritesi doc'ta yazılıdır.
    zsh değilse sarmalayıcı kurulmaz ve terminal bugünkü gibi çalışır.
  - **R4.2** — Süreli koşu (`BT_RUN_SECONDS`) entegrasyonu **hiç kurmaz**;
    kapıyı closure'ı panikleyen bir sınama tutar (`Inputs::Hermetic` emsali).
- **R5 — Ayar.** `[shell] integration = "auto" | "off"`, varsayılan `"auto"`.
  **Sonraki oturumda geçerlidir** — kabuk çoktan doğduğu için kayıt anında
  uygulanamaz; `Settings::changes`'e kol takılmaz ve `docs/AYARLAR.md` bunu
  kendi satırında söyler (uygulama açılmıyorken nasıl kapatılacağı dahil).
- **R6 — Betik ürüne girer ve girdiği denetlenir.**
  - **R6.1** — `assets/shell/` pakete kopyalanır; `make kur` kopyayı **ve**
    içerik denetimini yapar, `proje.md`'nin doğrulama tablosu `assets/shell/*`
    satırını kazanır.
  - **R6.2** — Geliştirmede (`cargo run`, paket yok) betik depo yolundan
    bulunur; release'te yalnız paketten.

## Yaklaşım

1. **Tarayıcı ve durum** (`bt-core`, saf): OSC 133 durum makinesi, `ShellState`
   ve `Session::shell_state()`. Pencere istemez, tamamı birim sınamasıyla
   kapanır.
2. **Sarmalayıcı** (`bt-core`, özel): `Pty`'yi saran tip, `Session::spawn`'da
   devreye girer. `EventLoop`'a giden yol dışında hiçbir şey değişmez.
3. **Kabuk kararı ve betik** (`bt-shell` + `assets/shell/`): `child::shell()`,
   `ZDOTDIR` politikası, zsh sarmalayıcı betiği, betiğin bulunması.
4. **Ayar, paketleme ve belgeler**: `[shell] integration`, `make kur`'un iki
   satırı, `make denetim`'in liste genişletmesi, `docs/AYARLAR.md`.

## Kapsam Dışı

bash ve fish betikleri; blok UI'si (şerit, katlama, süre eşiği, kalkma
animasyonu); Input Dock ve satır editörünün canlı durumu; prompt'u terminalin
çizmesi; tema değişiminde `LS_COLORS`/prompt renklerinin güncellenmesi; gömülü
tamamlama sözlüğü; komut işaretlerinin **satıra çıpalanması** (bloklar setinin
işi — bu sette yalnız durum tutulur).

SSH'ın öte tarafı kalıcı olarak kapsam dışıdır: uzak makinede betiğimiz yok,
orada durum hiç doğmaz ve bu bir arıza değil sessiz geri düşüştür.

## Göç

Yeni anahtar (`[shell] integration`); eski anahtar yok, silinen anahtar yok.
Kullanıcının makinesine **hiçbir dosya yazılmaz**: `ZDOTDIR` paketin içini
gösterir, yani entegrasyonu kaldırmak iz bırakmaz. Açık oturumlar
etkilenmez — ayar sonraki oturumda geçerlidir.

## Akış

```
bt-shell (child)                     bt-core
────────────────                     ───────
kabuk zsh mi? ──► evet               Session::spawn
  ZDOTDIR=<paket>/shell                 tty::new  ──►  Pty
  (ayar "off" ya da                      │
   süreli koşu ise atla)                 ▼
                                    TappedPty { pty, scan }   ◄── Reader = Self
                                         │  read(): baytlar aynen geçer,
                                         │          geçerken taranır
                                         ▼
                                    EventLoop ──► parser ──► Term
                                         │
                                    scan: OSC 133 ──► ShellState (yaprak kilit)
                                                            │
                                    Session::shell_state() ─┘   (tüketicisi
                                                                 sonraki set)
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | |
| phase-4 | |
| kapı | |
