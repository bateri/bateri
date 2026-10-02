# Phase 1 — Başarı işareti, `plain` olgusu ve düşme kararı (davranış değişmez)

## Özet

Sarmanın başarısını kanıtlayan nonce'lu `up` işaretini uçtan uca kur, durum
dosyasına `plain` olgusunu ve düşme kararını veren alt komutu ekle; sarma
kuralı (`decide`) ve zsh fonksiyonu henüz değişmez.

_Requirements: R2, R2.1, R2.2, R2.3, R3.2, R3.3, R5.1_

## Değişiklikler

- **`crates/bt-shell-common/src/ssh_wrap.rs`** — `wrap` deneme başına rastgele
  bir nonce üretip önyükleme komutuna argüman olarak koyar; nonce'u sarılmış
  argv'den okuyan saf bir fonksiyon (`unwrap`'ın kardeşi; `unwrap`'ın
  imzası ve konumla çalışma kuralı değişmez, gidiş-dönüş sınaması nonce'la
  genişler). `Fact::Plain` (`plain` satırı; eski dosyalar aynen okunur).
  Saf `fell_back(args, rc, state, …) -> Option<Vec<String>>`: rc 255 → `None`;
  host `posix` → `None`; host `touched` ve `posix` değil → `Some(düz argv)`
  ve çağıran `plain` yazar. Düz argv sarmasız ama `decide`'ın verdiği aynı
  `u-<key>` paylaşım seçenekleriyle (tek üretici: `Control`).
- **`crates/bt-shell-macos/src/lib.rs`, `crates/bateri/src/main.rs`** —
  `bateri ssh-fell-back --rc N [--instance I] -- <args…>` alt komutu,
  `ssh-argv` ile aynı dalda (Aqua denetiminden önce, `bt_shell_macos`
  üzerinden; bin'e yeni kenar yok); tel biçimi `ssh-argv`'nin ki (NUL ayrımlı
  ya da boş, çıkış hep 0).
- **`assets/shell/remote/boot.sh`** — ilk eylem `printf '\033]8133;i;up;%s\007'`
  (nonce `$1`'in yanında ikinci argüman), motd'dan ve her `bt_fault`'tan önce.
  Nonce yalnız `[0-9a-f]` olmalı; değilse işaret basılmaz (048'in tırnak
  kuralı).
- **`crates/bt-core`** — tarayıcı `8133;i;up;{nonce}`'u uzak 8133
  savunmasının dar istisnası olarak tanır (`8133;f` → `RemoteSetupFault`
  emsali; biçim dışı nonce reddedilir); son görülen nonce uzak nesle bağlı
  `ShellLog`'da, `C`/`set_remote` temizler; okuyucu `Wake` ile yüksüz haber
  verir (başlık/OSC 7 emsali). Yerel oturumda `i;up` yok sayılır.
- **`crates/bt-shell-macos/src/pane.rs`** — uzak oturum hedefi çözülünce ve
  `up` haberi gelince: hedefin argv'sindeki nonce görülenle eşleşirse
  `ssh_wrap::record(Fact::Posix, key)` kendi thread'inde (anahtar `ssh -G`'den,
  `learn`'in gövdesi). Eşleşmeyen ya da sarılmamış oturumda hiçbir şey.

## Kabul

- Saf sınamalar (R5.1): nonce gidiş-dönüşü (kullanıcının kendi `-t`'si
  dahil), `fell_back`'in tablosu, `Fact::Plain`'in okunup yazılması,
  tarayıcının `i;up`'u uzak oturumda kabul edip yerelde ve biçim dışında
  reddetmesi.
- PTY simülasyonu: sarılmış önyükleme (048'in mevcut kabuk simülasyonları)
  ilk çıktısı olarak `up;{nonce}` basıyor.
- Kullanıcının gördüğü değişmiyor: `decide` hâlâ yalnız `posix` host'u
  sarıyor, fonksiyon `ssh-fell-back`'i henüz çağırmıyor.

## Checklist

- [ ] `ssh_wrap`: nonce, `Fact::Plain`, `fell_back`
- [ ] `bateri ssh-fell-back` alt komutu
- [ ] `boot.sh`: ilk eylem `up;{nonce}`
- [ ] `bt-core`: `i;up` kabulü, uzak nesle bağlı nonce, `Wake`
- [ ] pane: nonce eşleşince `posix` kaydı
- [ ] Test: nonce gidiş-dönüşü, `fell_back` tablosu, tarayıcı kabul/ret
- [ ] Doğrulama geçti (`make check`, `make linux`, `make bundle` — `assets/shell/*`, `make test-race` — okuyucu yolu)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi (okuyucu tarayıcısı)
