# Phase 1 — OSC 133 tarayıcısı ve oturum durumu

## Özet

Kabuğun bastığı işaretleri tanıyan **saf** bir tarayıcı ve onun doldurduğu
oturum durumu; henüz kimse beslemiyor, kimse okumuyor.

_Requirements: R2.1, R2.2, R2.3, R2.4_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** (ya da yeni bir `bt-core` modülü) — üç
  parça:
  - **Tarayıcı**: bayt dilimleri alır, aralarında durum taşır, OSC 133'ün
    `A`/`B`/`C`/`D` işaretlerini çıkarır. `ESC ]` ile başlar, `BEL` ya da
    `ESC \` ile biter; **bölünmüş dizi** iki `read()` arasında hayatta kalır.
    Taşıma tamponunun bir **üst sınırı** var: sınırı aşan dizi düşürülür ve
    tarayıcı boşa döner — bozuk ya da kötü niyetli bir akış belleği büyütemez.
    Tanımadığı OSC'yi ve bozuk yükü **yoksayar**; `bt-core`'da panik yok.
  - **Durum tipi** (`ShellState`): prompt'ta mıyız, komut mu koşuyor, sonuncusu
    hangi kodla bitti. **Kabuk adı geçmez** — tip zsh'i de bash'i de bilmez
    (R2.4). Seviye `enum`'u **yok**: durumun yokluğu "entegrasyon yok" demek
    (`discussion.md` → Karar 3).
  - **Yuva ve sorgu**: durum `Adapter`'ın yaprak kilidinde durur,
    `Session::shell_state()` onu kopyalayarak verir. Emsal `Session::theme()`:
    `Term` kilidine girmez, `frame()` imzasına dokunmaz.

## Kabul

- Tarayıcı birim sınamalarıyla kapanıyor: dört işaret, `D`'nin çıkış kodu,
  **chunk sınırında ikiye bölünmüş** dizi, iki sonlandırıcı (`BEL` / `ESC \`),
  tanınmayan alt-işaret, bozuk yük, üst sınırı aşan dizi.
- Tarayıcı `Session`'a geri girmiyor ve **hiçbir kare istemiyor** — bu bir
  kural değil, tipin şekli: tarayıcının elinde ne `Wake` ne `Session` var.
- `Session::shell_state()` bugün her koşuda "durum yok" diyor (besleyen yok) ve
  bunu bir sınama çiviliyor.
- `frame()` imzası değişmedi; `bt-gpu`'nun kare sınamaları dokunulmadan geçiyor.

## Yayın Etkisi

shader yok · terminfo yok · ayar şeması yok · tema yok · shell entegrasyonu
yok (betik sonraki phase'lerde) · app bundle yok · yeni bağımlılık yok.

`bt-core`'un platformsuzluğu korunuyor: tarayıcı saf bayt işi, `libc` bile
görmüyor.

## Checklist

- [ ] Tarayıcı: durum makinesi, bölünmüş dizi, üst sınır
- [ ] `ShellState` + yaprak kilitteki yuva + `Session::shell_state()`
- [ ] Test: dört işaret, çıkış kodu, bölünmüş dizi, iki sonlandırıcı, bozuk
      yük, üst sınır aşımı, "besleyen yokken durum yok"
- [ ] Doğrulama geçti (`make hepsi`)
- [ ] Yayın etkisi yazıldı
