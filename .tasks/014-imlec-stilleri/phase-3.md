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

## Uygulama Notları

- **Altın iki ton, tek değil.** İmleç bloğu opak ve altındaki harf **zemin
  rengiyle** çiziliyor, yani renk hem zemine hem kendi üstündeki zemin renkli
  harfe karşı okunur olmalı: koyu temada açık altın (`#d9b063`, paletin kendi
  sarı ailesinden ama ondan ayrık), açık temada koyu bronz (`#8a6512`) —
  koyunun tonu beyaz zeminde harfi yutardı. İkisi de zevk kararı, ölçüm değil.
- **Eksik anahtarın `accent`'e düşmesi `parse`'ın sırasına yazıldı**, ayrı bir
  "var mı" sorusuna değil: `accent` okunduktan sonra yuva ona eşitleniyor,
  anahtar varsa `read_color` üstüne yazıyor. Kabul edilmeyen değer yuvayı
  bırakıyor, yani yine `accent` — yönü güvenli.
- **Bir sınamanın adı değişti** ve bu bilinçli: `empty_theme_is_the_base`
  artık `empty_theme_is_the_base_except_the_cursor`. "Boş dosya tabanın
  aynısıdır" değişmezi `cursor` için **tasarım gereği** geçerli değil ve eski
  ad bunu gizlerdi.
- **Yedi sınama fixture'ı güncellendi** (`bt-core` beşi, `bt-shell` ikisi):
  hepsi "yalnız şunu yazan tema dosyası" kuruyordu ve `cursor`'ın `accent`'i
  izlemesi beklentilerini kaydırdı. Hiçbiri zayıflatılmadı — beklenen değer
  kuralın kendisiyle değiştirildi.
- **Belgedeki tema blokları da güncellendi** ve bu bir sınamanın söylediği
  şeydi (`documented_blocks_are_the_embedded_themes`): blok `cursor` taşımasaydı
  onu kopyalayan kullanıcı gömülü paletin imlecini **alamazdı**.

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

- [x] `bt-core`: `Theme.cursor` + `cursor_linear()`; iki gömülü tema dolduruldu
- [x] `bt-core`: ANSI 258 yeni role bağlandı, OSC 10/11 `accent`'te kaldı
- [x] `bt-core`: tema dosyası `cursor`'ı opsiyonel okuyor, eksikte `accent`
- [x] `bt-gpu`: iki caret çağrı yeri `cursor_linear()` okuyor
- [x] Test: anahtarsız tema → `cursor == accent` (geriye dönük okuma)
- [x] Test: anahtarlı tema → rolü kullanıyor, `accent` değişmiyor
- [x] Test: ANSI 258 yeni rolü veriyor
- [x] `docs/AYARLAR.md`, `CLAUDE.md`
- [x] Doğrulama geçti (`make hepsi`)
- [x] Yayın etkisi yazıldı
