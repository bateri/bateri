# Phase 3 — ssh'ın öbür ucuna terminal kimliği (`LC_` ailesi)

## Özet

Pane'in kabuğuna `LC_TERMINAL=bateri`, `LC_TERMINAL_VERSION` ve
`LC_BATERI_TAB_URL` koy; sarılmış oturumda uzak betik de aynılarını
dışa aktarsın — uzaktaki araçlar terminali ve sekmeyi tek bir addan tanısın
(kullanıcı isteği 2026-10-03).

_Requirements: R6, R6.1, R6.2, R7_

## Değişiklikler

- **`crates/bt-core/src/identity.rs`** (ya da kimlik ortamının kurulduğu
  yer, 038) — `TERM_PROGRAM` ailesinin yanına üç değişken:
  `LC_TERMINAL=bateri`, `LC_TERMINAL_VERSION` (workspace sürümü, mevcut
  `TERM_PROGRAM_VERSION` ile aynı kaynak), `LC_BATERI_TAB_URL` (pane'in
  `BATERI_TAB_URL`'siyle aynı değer). Miras kalan `LC_TERMINAL=iTerm2`
  **ezilir** — `TERM_PROGRAM=Apple_Terminal` mirasının emsali (038 →
  context → Kanıt); `SessionOptions.env`'in ek ortamı bunları ezemez, tıpkı
  `TERM`/`COLORTERM` gibi. Değer hiçbir zaman `iTerm2` taklidi değildir.
- **Taşıma:** macOS'un ssh istemcisi `SendEnv LANG LC_*` ile geliyor,
  Debian/Ubuntu/macOS sshd'leri `AcceptEnv LANG LC_*` ile kabul ediyor
  (ikisi de bu makinede ve Debian imajında okundu) — düz ssh'ta ek bir şey
  gerekmiyor. **Sarılmış oturumda** uzak önyükleme (`assets/shell/remote/boot.sh`)
  üç değişkeni giriş kabuğunu açmadan önce dışa aktarır; değerler sarılmış
  argv'de gider (nonce emsali, `ssh_wrap::wrap`), `ps`'te görünmeleri
  zararsız (sekme adresi yalnız odaklar, 038 Karar 5). Sunucu `LC_*`'ı zaten
  getirdiyse aynı değer, üzerine yazmak sorun değil. `unwrap` bu değerleri de
  argv'den düşürür (gidiş-dönüş sınaması).
- **Düşme kararı (R7)** — `ssh-fell-back`'in kararına "girişten sonra
  kullanıcı yazdı mı" girdisi: pane, sarılmış oturumda 047'nin giriş
  kenarından sonra girdi nesli (`key_gen`) ilerleyince nonce'un yanına
  "kullanıldı" kaydı düşer (nonce kanıtının `remote-hosts.up/{nonce}`
  emsali); kayıt varsa düşme yok. phase-2'nin ölçtüğü `ForceCommand`'lı
  CLI senaryosu sınamaya girer (çıkıştan sonra yeniden bağlanma yok).
- **`CLAUDE.md`** — kimlik paragrafına (`TERM_PROGRAM=bateri`, …) tek cümle:
  `LC_` ailesi ssh'tan geçen kimlik, iTerm2'nin `LC_TERMINAL` emsali; sarılmış
  oturumda uzak betik de koyuyor; `AcceptEnv` kısıtlı sunucuda sarılmayan
  oturumda yok (bilinen sınır).

## Kabul

- Yerel: pane'in kabuğunda üç değişken var; `LC_TERMINAL=iTerm2` mirasıyla
  doğan oturumda değer `bateri` (sınama, `identity`'nin mevcut sınamalarının
  yanında).
- Yerel ve uzakta yan etki yok: tanınmayan `LC_` adıyla `locale`, `perl -e1`
  ve `python3 -c 1` uyarı basmıyor (Docker imajında ve yerelde sınandı,
  sonuç Uygulama Notları'nda).
- Docker sshd: düz ssh'ta üç değişken uzakta görünüyor; `AcceptEnv`'i
  kısıtlı (yalnız `LANG`) bir sshd'de sarılmış oturumda yine görünüyor,
  sarılmamışta görünmüyor (bilinen sınır). mosh'un taşıyıp taşımadığı
  ölçülüp Uygulama Notları'na yazılır (mosh kurulu değilse `[~]`).
- `unwrap(wrap(x)) == x` değişkenlerle birlikte yeşil.

## Checklist

- [ ] Kimlik ortamına üç `LC_` değişkeni; miras ezilir
- [ ] Sarılmış oturumda uzak önyükleme üç değişkeni dışa aktarır; `unwrap` düşürür
- [ ] Test: `LC_TERMINAL=iTerm2` mirasında değer `bateri`
- [ ] Test: Docker sshd — düz, sarılmış, `AcceptEnv` kısıtlı
- [ ] Yan etki kontrolü (`locale`, perl, python)
- [ ] R7: girişten sonra yazılmış oturum düşmüyor (ForceCommand CLI sınaması)
- [ ] CLAUDE.md kimlik cümlesi
- [ ] Doğrulama geçti (`make check`, `make linux`, `make bundle`)
- [ ] Set kapısı: `/code-review` (setin aralığı) + `/audit`
