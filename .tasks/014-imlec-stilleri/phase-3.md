# Phase 3 — İmlecin tema rolü

## Özet

Caret rengini `accent`'ten ayır: tema dokuzuncu bir rol kazansın (`cursor`) ve
gömülü iki temada **altın** olsun; koşan komut bloğunun şeridi `accent`'te
kalsın.

_Requirements: R13, R13.1, R13.2, R13.3_

## Değişiklikler

- **`crates/bt-core/src/color.rs`** — `Theme`'e `cursor: u32` alanı ve
  `cursor_linear()` erişimcisi (`accent_linear` emsali). Gömülü iki tema
  (`bateri`, `bateri-light`) rolü dolduruyor.
  - **ANSI 258 yeni rolü söylüyor** (R13.2): bugün `rgb(self.accent)` dönen
    kol `cursor`'a bağlanıyor. O yuva zaten "imleç rengi"nin standart yeri ve
    `accent`'e takma ad olması bir kalıntıydı.
  - **OSC 10/11 `accent`'te kalıyor**: onlar ön plan/arka plan sorusu, imleç
    değil. Karıştırılırsa uygulamanın sorduğu şey değişir.
- **`crates/bt-core/src/theme.rs`** — dosya biçimi: `cursor` **opsiyonel**
  anahtar. **Eksikse `accent`'e düşüyor** (R13.1) ve bu, geriye dönük okumanın
  tamamı: bugün yazılmış bir kullanıcı teması tek harf değişmeden, aynı
  görüntüyle çalışmaya devam ediyor. Eksik anahtarın varsayılana değil
  **`accent`'e** düşmesi şart — gömülü varsayılana düşseydi koyu bir kullanıcı
  temasında imleç birden altın olurdu.
- **`crates/bt-gpu/src/link.rs`** — iki çağrı yeri `theme.accent_linear()`
  yerine `theme.cursor_linear()` okuyor (içerik karesi ve hareket karesi).
  Başka tüketici yok; blok şeridi kendi yolundan `accent`'i almaya devam
  ediyor.
- **`docs/AYARLAR.md` → Temalar** — yeni anahtar, opsiyonelliği ve eksikte
  `accent`'e düşmesi.
- **`CLAUDE.md`** — "Tema = sekiz rol" cümlesi **dokuza** çıkıyor ve `accent`'in
  tarifi daralıyor: artık yalnız koşan komut bloğunun şeridi.

## Kabul

- İmleç altın, koşan komutun şeridi **bugünkü mavi** kalıyor — ikisinin
  ayrıştığının tek bakışta görülen kanıtı bu.
- `cursor` anahtarı **olmayan** bir kullanıcı teması bugünküyle **birebir**
  aynı görünüyor (imleç `accent` rengi). Sınama bunu çiviliyor: rolü olmayan
  bir tema dosyası ayrıştırılıp `cursor == accent` doğrulanıyor.
- `cursor` anahtarı **olan** bir tema onu kullanıyor ve `accent`'i etkilemiyor.
- OSC 10/11 cevabı değişmiyor; ANSI 258 yeni rolü veriyor.
- Gömülü açık tema (`bateri-light`) kendi altınını taşıyor: koyu temanın
  tonu açık zeminde okunmaz olurdu.

## Yayın Etkisi

- **tema biçimi** — `cursor` anahtarı eklendi, **opsiyonel**, eksikte
  `accent`'e düşüyor. Kullanıcının `~/.config/bateri/themes/*.toml`
  dosyalarında **hiçbir değişiklik gerekmiyor**; silinen anahtar yok.
- **`CLAUDE.md`** — rol sayısı ve `accent`'in tarifi.
- **`docs/AYARLAR.md`** — Temalar bölümü.
- shader / ayar şeması / terminfo / app bundle / yeni bağımlılık: yok.
  `make kur` gerekmiyor.
- ölçüm bekleyen iddia: yok. Altın tonu **seçilmiş bir zevk kararı**, ölçüm
  değil — `docs/OLCUMLER.md`'nin konusu değil.

## Checklist

- [ ] `bt-core`: `Theme.cursor` + `cursor_linear()`; iki gömülü tema dolduruldu
- [ ] `bt-core`: ANSI 258 yeni role bağlandı, OSC 10/11 `accent`'te kaldı
- [ ] `bt-core`: tema dosyası `cursor`'ı opsiyonel okuyor, eksikte `accent`
- [ ] `bt-gpu`: iki caret çağrı yeri `cursor_linear()` okuyor
- [ ] Test: anahtarsız tema → `cursor == accent` (geriye dönük okuma)
- [ ] Test: anahtarlı tema → rolü kullanıyor, `accent` değişmiyor
- [ ] Test: ANSI 258 yeni rolü veriyor
- [ ] `docs/AYARLAR.md`, `CLAUDE.md`
- [ ] Doğrulama geçti (`make hepsi`)
- [ ] Yayın etkisi yazıldı
