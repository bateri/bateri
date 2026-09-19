# Phase 2 — Blink periyodu: `cursor_blink_interval`

## Özet

Blink'in yarım periyodu ayardan gelsin; `blink.rs`'in `const`'u alana dönsün
ve canlı değişimde saat **yeniden kurulsun**.

_Requirements: R1, R1.3, R1.4, R2, R3, R3.2, R3.3, R4, R5, R7, R8_

> **phase-1'e bağlı.** `ranged_float`, `Changes.caret` alanı ve iki uygulama
> yeri orada doğuyor; bu phase üçünü de **tüketiyor**, yeniden kurmuyor.

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — üçüncü anahtar, aynı tesisat.
  Varsayılan yine `pub const` (R2); aralık `0.05..=5.0` ve **iki ucu da
  gerekçeli** (R5).
- **`crates/bt-gpu/src/blink.rs`** — `HALF_PERIOD` `const`'tan **alana**
  dönüyor.
  - `Blink` `Copy` ve `Cell<Blink>` içinde yaşıyor; emsal `Motion::set_style`
    (Copy struct'ı `Cell`'den al, değiştir, geri koy). Modülün saflığı ve
    kilitsizliği **bozulmuyor**.
  - **`next_flip` mutlak son tarih**, yani canlı değişen periyot bir flip
    gecikir. Setter bunu kapatmak için `next_flip`'i **yeniden kuruyor**.
  - `IDLE_STOP` **`const` kalıyor** — kapsam dışı (`discussion.md` → Muhakeme).
- **`crates/bt-gpu/src/link.rs`** — periyot `set_caret_style`'ın taşıdığı
  kayda giriyor ya da kardeş bir setter alıyor; hangisi olursa olsun
  **değişimde kare istiyor** (R3.3). Burada gerekçe en keskin hâlinde:
  kurulmuş `after` **iptal edilemiyor** (`arm_clock`), yani uyurken değişen
  periyot yeniden kurulmazsa bir sonraki flip eski ritimde gelir.
- **`CLAUDE.md`** — blink cümlesi periyodun artık ayardan geldiğini anmalı;
  "saniyede iki kare" **varsayılanın** hâli oluyor.
- **`docs/AYARLAR.md`** — üçüncü satır, proza ve Şablon bloğu.

## Kabul

- `cursor_blink_interval = 0.15` blink'i hızlandırıyor, `2.0` yavaşlatıyor;
  ikisi de **kayıt anında**, yeniden başlatma yok.
- Değişim **uyurken** de geçerli: blink kapalıyken ya da pencere boştayken
  kaydedilen periyot bir sonraki flip'te değil **hemen** geçerli.
- Aralık dışı değer kendi anahtarını değiştirmiyor + tanı.
- `IDLE_STOP` davranışı **değişmiyor**: 15 saniye sessizlikten sonra blink
  duruyor ve fazı **açık** bırakıyor.
- Hareketi Azalt ve odak kapıları periyottan **bağımsız** çalışmaya devam
  ediyor.

## Uygulama Notları

<!-- Kodlanırken doldurulacak. -->

## Yayın Etkisi

- **ayar şeması** — üçüncü anahtar; aynı göç notu (mevcut kullanıcı dosyası
  değişmiyor).
- **belge** — `CLAUDE.md`'nin blink cümlesi, `docs/AYARLAR.md`.
- **seçilmiş sayı** — `0.05` ve `5.0`; `const` doc'unda gerekçesiyle.
- **bilinen sınır, yazılacak:** **kapı bozuk bir periyodu göremiyor.** Süreli
  koşu ayar dosyasını hiç okumuyor ve blink varsayılanı kapalı, yani
  `0.05` bir periyot `sessiz < QUIET_FLOOR`'u **hiçbir koşulda** kızartmaz.
  014'ün defteri bunu zaten kaydetmişti ("koruma bir jeton değil varsayılanın
  kendisi"); bu set o korumanın üstüne kullanıcının elini koyuyor ve
  karşılığında **sıfır otomatik kapsama** alıyor. Tek koruma alt sınır.
- **ölçüm iddiası yok.** "Periyot ucuz" yön olarak koddan kanıtlı (ekran
  hızına çıkmıyor), büyüklüğü ölçülmedi.
- shader / terminfo / app bundle / yeni bağımlılık: yok.
- **geri alma:** phase commit'ini revert; anahtar emekli edilir, `HALF_PERIOD`
  `const`'a döner.

## Checklist

- [ ] `settings.rs`: üçüncü anahtar, `ranged_float` ile (phase-1'inkini
      tüketiyor), varsayılan `pub const`
- [ ] `blink.rs`: `HALF_PERIOD` alana döndü, modül saf ve kilitsiz kaldı
- [ ] `blink.rs`: setter `next_flip`'i **yeniden kuruyor** (mutlak son tarih)
- [ ] `blink.rs`: `IDLE_STOP` `const` kaldı
- [ ] `link.rs`: değişimde kare isteniyor (kurulmuş `after` iptal edilemiyor)
- [ ] `app.rs`: açılış tohumu ve kayıt anı (phase-1'in yolunu tüketiyor)
- [ ] Test: periyot değişince `next_flip` yeniden kuruluyor, eski ritimde
      bir flip gelmiyor
- [ ] Test: aralık dışı değer kendi anahtarını değiştirmiyor + tanı
- [ ] Test: `IDLE_STOP` davranışı değişmedi
- [ ] Test: şablon/varsayılan çifti (`template_is_the_defaults`)
- [ ] `CLAUDE.md`, `docs/AYARLAR.md`
- [ ] Doğrulama geçti (`make hepsi`)
- [ ] `make duman` (kullanıcıda)
- [ ] **Gözle kontrol:** `cursor_blink = "on"` iken periyodu değiştirip kaydet
      — ritim **hemen** değişmeli, bir flip gecikmeden
- [ ] Yayın etkisi yazıldı
