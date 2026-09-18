# Phase 11 — Izgarayı dock'a hizala: işaret 0. sütunda, komut 2.'de

## Özet

Izgaranın blok işareti ile dock'un prompt işareti aynı sütuna gelsin, komut
metni ikisinde de aynı sütundan başlasın, çıktı ise işaretle hizalı kalsın.
Kaydırma **çizimde değil terminalde**: `PS1` sıfır genişlikten iki sütuna
çıkıyor.

_Requirements: R2.3 eki, R4.1 (genişlik düzeltmesi)_

## Bağlam

Kullanıcı ekran görüntüsüyle sordu: ızgaradaki `>` ile dock'taki `>` aynı
hizada değil, komutla işaret arası dar, ve çıktılar işaretle hizalanmıyor.

Sebebi koddan net:

| | işaret nerede | metin nerede |
|---|---|---|
| ızgara | sol payın **içinde**, ortalanmış | 0. sütun |
| dock | 0. sütun | 2. sütun (`TEXT_COL`) |

Yani dock'un işareti tam olarak ızgaranın **metninin** durduğu yerde.

Kullanıcı iki seçenek arasından **dock'un aynası**nı seçti: işaret 0. sütunda,
komut 2.'de, çıktı ve bağlam satırı 0.'da. Seçerken bedeli de kabul etti —
komut satırının terminalin dediği yerden iki sütun sağa **çizilmesi**, fare
seçiminin o satırda kayması ve tam genişlikteki komutun taşması.

**O bedel ödenmiyor.** Kaydırmayı çizimde yapmak yerine prompt'u gerçekten iki
sütun geniş yapmak aynı görüntüyü veriyor ve üç sorunun üçünü birden
doğuruyormuş gibi görünen tek sebebi (çizimin terminale yalan söylemesi)
ortadan kaldırıyor.

## Kararlar

- **Kaydırma terminalde, çizimde değil.** `PS1` = çıpa + **iki boşluk** + `B`
  işareti. Komut gerçekten 2. sütundan başlıyor, yani:
  - fare eşlemesi (`bt-shell::view`, tek yer) **dokunulmuyor**;
  - zsh satır sarmayı doğru hesaplıyor, tam genişlikteki komut taşmıyor;
  - seçim kopyalandığında başta iki boşluk çıkıyor ve bu **doğru** — ekranda
    duran şey o.
- **İşaret 0. sütuna geçiyor.** `Frame::push_block` chevron'u sol payın
  ortasına değil `pos(0, row)`'a koyuyor; dock'un işareti zaten orada, yani
  hizalama **hesaplanmıyor**, aynı formülden doğuyor.
- **Sol pay işaret taşımayı bırakıyor**, saf sol kenar boşluğu oluyor. Payın
  kendisi duruyor — `CellMetrics::GUTTER_PT` hem ızgaranın hem dock'un sol
  kenarı ve ikisi aynı olmak zorunda.
- **İki boşluk `dock::TEXT_COL` ile aynı sayı olmak zorunda** ve ikisi ayrı
  yerlerde yaşıyor (biri zsh betiğinde, biri Rust'ta). Sabit paylaşılamıyor,
  bu yüzden bir sınama betiği okuyup sayıyı bağlıyor: ayrışırlarsa ızgara ile
  dock'un metni farklı sütundan başlar ve belirti sessiz olurdu.
- **Yan kazanç adıyla yazılıyor:** boşluklar çıpayı taşıyor, yani boş
  prompt'ta da çıpalı bir hücre var. phase-8'in "boş promptta hiçbir hücre
  çıpayı taşımıyor" durumu kapanıyor.

## Değişiklikler

- **`assets/shell/zsh/bateri.zsh`** — `__bateri_ps1`'e iki boşluk; sıra çıpa →
  boşluklar → `B`, çünkü `B` prompt'un **sonu**.
- **`crates/bt-gpu/src/frame.rs`** — `push_block` chevron'u 0. sütuna koyuyor;
  payın ortasına yerleştiren aritmetik kalkıyor.
- **`CLAUDE.md`** — "sıfır görünür genişlik" cümlesi düzeliyor; hizanın tek
  formülden doğduğu yazılıyor.

## Kabul

- Izgaradaki `›` ile dock'taki `›` aynı x'te; punto değişince de aynı kalıyor.
- Komut metni ikisinde de işaretten bir sütun sonra.
- Çıktı satırları ve bağlam satırı işaretle aynı sütunda.
- Fareyle komut satırından seçim **kaymıyor**.
- Tam genişlikte bir komut taşmıyor (zsh iki sütunu biliyor).
- `[shell] integration = "blocks"` etkilenmiyor: orada prompt kullanıcının.

## Yayın Etkisi

- **`make kur` zorunlu** (`assets/shell/*` değişti).
- **Göç:** prompt iki sütun genişledi, yani her komut satırı iki sütun içeriden
  başlıyor. Açık pencereler eski betikle koşmaya devam eder ve orada işaret
  0. sütunda komutun **üstüne** düşer — bir karelik değil kalıcı bir uyumsuzluk,
  ama yalnız yeni terminal + eski betik çiftinde ve o çift `make kur` ile
  kapanıyor.
- **`make duman`**: reçete `/bin/sh` koşuyor, entegrasyon yok, jeton
  sözleşmesi dokunulmadan kalıyor.
- shader, ayar şeması, tema, terminfo, yeni bağımlılık: yok.

## Checklist

- [x] `PS1` iki sütun; sıra çıpa → boşluk → `B`
- [x] İşaret 0. sütunda (`push_block`)
- [x] Test: betiğin prompt genişliği `dock::TEXT_COL` ile aynı
- [x] Test: işaretin x'i ile dock işaretinin x'i aynı
- [ ] Gerçek pencerede gözle: hiza, seçim, tam genişlikte komut
- [ ] Doğrulama geçti (`make hepsi` + `make kur`)
- [x] Yayın etkisi yazıldı
