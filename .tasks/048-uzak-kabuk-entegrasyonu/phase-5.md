# Phase 5 — Paylaşılan bağlantı

## Özet

Sarılmış etkileşimli oturum 047'nin soketinde master olsun; kullanıcı
parolayı terminalde bir kez yazar, dosya işleri pencere açmadan onun
üstünden geçer.

_Requirements: R7_

**Ön koşul:** 047 bitmiş olmalı — `ssh_route`, soket dizini ve `host_key`'e
dayalı soket adı (047 `discussion.md` → Muhakeme). Bu phase o düzeni
kullanır, ikinci bir düzen kurmaz.

## Değişiklikler

- **`crates/bt-shell-common/src/ssh_wrap.rs`** — `wrap` `-o
  ControlMaster=auto -o ControlPath=<047'nin yolu> -o ControlPersist=<047'nin
  süresi>` ekler; `ssh -G` kullanıcının kendi `controlmaster`/`controlpath`'ini
  gösteriyorsa eklemez. `unwrap` bunları da geri alır (gidiş-dönüş).
- **`crates/bt-shell-common/src/ssh_route.rs`** (047'nin) — kullanıcının
  oturumunun master'ı `Route::Ours` olarak tanınır; ömrü oturuma bağlı,
  oturum kapanınca 047'nin kendi master'ını açma yolu devralır.

## Kabul

- `wrap`/`unwrap` gidiş-dönüşü yeni seçeneklerle; kullanıcının
  `ControlMaster`'ı varken ek yok.
- Yerel `sshd` (`#[ignore]`, 047'nin altyapısı) ile: sarılmış oturum açıkken
  bir yükleme yeni bağlantı kurmadan geçiyor; oturum kapanınca sonraki iş
  047'nin yoluna düşüyor.
- `make check`, `make linux` yeşil.
- Gözle kontrol: parolalı sunucuya `ssh` (parola terminalde) → Finder'dan
  bırakılan dosya parola sayfası açmadan yükleniyor.

## Checklist

- [ ] `wrap`'e ControlMaster seçenekleri, kullanıcı config'inde atlama
- [ ] `ssh_route`'ta kullanıcının oturumunun tanınması
- [ ] Test: gidiş-dönüş, yerel sshd senaryosu
- [ ] Doğrulama geçti (`make check` + `make linux`)
