# Uzak entegrasyon ilk bağlantıdan itibaren — Plan

## Hedef

Kullanıcı düz `ssh` yazınca uzak entegrasyon (dizin, komut blokları, süre)
**ilk bağlantıdan itibaren** gelsin; kabuksuz uçta bağlantı kendiliğinden düz
açılsın; hiçbir yeni arayüz yazılmasın. Karar ve gerekçe `discussion.md` →
`## Karar (2026-10-03, kullanıcı onayı)`.

## Gereksinimler

- **R1 — Bilinmeyen host da sarılır.** `ssh_wrap::decide` `posix`'i şart
  koşmaz; yalnız `plain` olarak kayıtlı host'u sarmaz. `ssh -G` kuralları,
  uzak komut, tty ve ayar kuralları aynen.
- **R2 — Başarı işareti.**
  - **R2.1** Sarılmış argv deneme başına rastgele bir nonce taşır; `boot.sh`
    **ilk iş** olarak `ESC ] 8133 ; i ; up ; {nonce} BEL` basar (motd'dan ve
    her olası hatadan önce).
  - **R2.2** `bt-core` tarayıcısı `i;up`'u uzak 8133 savunmasının dar
    istisnası olarak kabul eder (`8133;f` emsali), içeriğe değil biçime
    bakar; son görülen nonce uzak nesle bağlı tutulur.
  - **R2.3** Pane, oturumun hedefinin argv'sindeki nonce (`unwrap`'tan) ile
    görülen nonce eşleşince host'u `posix` kaydeder; eşleşmeyen ya da
    sarılmamış oturumda görülen `up` yok sayılır.
- **R3 — Sessiz düşme.**
  - **R3.1** Sarılmış ssh bitince zsh `ssh` fonksiyonu çıkış kodu 255
    değilse `bateri ssh-fell-back -- <kullanıcının argv'si>`'ye sorar.
  - **R3.2** Alt komut, host bu denemede `touched` ama `posix` değilse
    `plain` kaydeder ve yeniden koşulacak argv'yi (sarmasız, aynı `u-<key>`
    ControlPath'iyle) NUL ayrımlı basar; aksi hâlde boş. Fonksiyon argv
    boş değilse `command ssh` ile yeniden koşturur.
  - **R3.3** 255 (bağlantı/kimlik hatası) hiçbir şey kaydetmez ve yeniden
    koşturmaz.
- **R4 — 048'in selamdan öğrenmesi kalkar.** `RemoteHelper::with_greeted` /
  `Greeted`, `remote_helper_for(learning)` ve `ssh_wrap::learn`'in çağrısı
  sökülür; `posix` yalnız R2.3'ten yazılır. `Fact::Plain` eklenir,
  `touched` kalır. Shell menüsünde host için entegrasyonu açmak o host'un
  `plain` kaydını siler (yanlış öğrenilmiş bir kaydın tek geri dönüş yolu).
- **R5 — Sınama ve belge.**
  - **R5.1** Saf: nonce'un `wrap`/`unwrap` gidiş-dönüşü, `decide`'ın üç
    durumu (bilinmeyen/`posix`/`plain`), `ssh-fell-back`'in karar tablosu
    (255, `posix`, `touched`-değil-`posix`), tarayıcının `i;up` kabulü ve
    uzak oturum dışında/yanlış biçimde reddi.
  - **R5.2** Uçtan uca (`#[ignore]`, Docker sshd): oh-my-zsh'li parolalı
    kullanıcıyla **ilk** bağlantıda dizin ve blok; giriş kabuğu komut
    kabul etmeyen bir kullanıcıyla düz yeniden bağlanma ve ikinci parolanın
    sorulup sorulmadığı (ölçülür, sonuç Uygulama Notları'na).
  - **R5.3** `CLAUDE.md`'nin 048 cümleleri ("ilk bağlantıda öğren" →
    "her zaman sar, sessiz düşme"), `docs/AYARLAR.md`'nin `integration`
    satırı gerekiyorsa.

## Yaklaşım

1. `ssh_wrap`: nonce üretimi `wrap`'ta, `unwrap` onu geri verir; `decide`
   bilinmeyen host'u sarar; `Fact::Plain`; yeni saf karar fonksiyonu
   (`fell_back`) ve onun alt komutu (`bateri ssh-fell-back`, `ssh-argv`'nin
   kardeşi, aynı `main` dalında Aqua denetiminden önce).
2. Sinyalin fonksiyona dönüşü **durum dosyasından**: `up`'u yalnız pane
   görüyor ve `posix`'i o yazıyor; fonksiyon ssh bitince alt komuta soruyor,
   alt komut dosyayı okuyor. Gerekçe: fonksiyon ssh'ın çıktısını göremez,
   çıkış kodu kullanıcının son komutunundur; dosya iki süreç arasında zaten
   paylaşılan tek kanal (048 phase-2) ve kilitli yazımı var. Yeni argv'yi
   de bateri üretiyor, yani sarma ve düşme tek ikiliden.
3. `boot.sh` ilk satırda `up`; `bt-core` `i;up` olayını uzak 8133 kapısının
   istisnası olarak yayar; pane nonce'u hedefin argv'sinden doğrular ve
   `posix`'i kendi thread'inde yazar.
4. zsh `__bateri_ssh`: sarılmış koşudan sonra 255 değilse `ssh-fell-back`,
   boş değilse düz yeniden koşu.
5. 048'in öğrenme yolu sökülür; menünün "aç"ı `plain`'i siler.

## Kapsam Dışı

- Girişten sonra yazma (taslak B, reddedildi).
- `ssh` fonksiyonunun görmediği ssh'lar: betik, `exec ssh`, alias, yerel
  bash/fish kabuğu.
- mosh; uzak dock.
- `posix` öğrenilmiş bir host'un sonradan kabuksuz olması için ayrı yol:
  `boot.sh` her hatada düz giriş kabuğuna düşüyor (048 R3), bağlantı kırılmaz.

## Akış

```
ssh host ──► __bateri_ssh ──► bateri ssh-argv
                               │ plain kayıtlı?  → boş → command ssh "$@"
                               │ değilse         → touched yaz, wrap(+nonce, u-<key>)
                               ▼
                         command ssh <sarılmış>
                               │
          sunucu kabuklu ──────┼────── kabuksuz uç
          boot.sh: up;nonce    │       exec sh -c başarısız, oturum kapanır
          pane: nonce eşleşir  │
          → posix yaz          │
          … kullanıcı çalışır  │
          exit (rc ≠ 255)      ▼
                    bateri ssh-fell-back -- "$@"
                      posix  → boş (hiçbir şey)
                      touched, posix değil → plain yaz, düz argv (+u-<key>)
                               ▼
                         command ssh <düz>   (2 sn içindeyse master'a biner)
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | |
| phase-2 | |
| kapı | |
