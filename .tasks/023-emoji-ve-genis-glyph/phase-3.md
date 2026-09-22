# Phase 3 — Sözleşmeyi ve defterleri gerçeğe uydur

## Özet

Kodun değiştirdiği yazılı cümleler düzeltilir, envanter ölçümü sahibine
işlenir ve kapanan/daralan borçlar yol haritasına yazılır.

_Requirements: R9_

## Değişiklikler

- **`CLAUDE.md`** — üç cümle gerçeğe uymuyor olacak: **"Pipeline üç"**
  (dördüncü doğdu), **"`bt-gpu` atlası `R8Unorm` dokuya bağlar"** (ikinci
  düzlem var), ve **"Emoji ve geniş glyph henüz yok… kutu çiziliyor"**
  paragrafının tamamı. `bt-atlas` ile `bt-gpu`'nun sorumluluk satırları da
  ikinci düzlemi ve `Half` eksenini anmalı. Yeni cümleler **kararın gerekçesini**
  taşır, kodun kopyasını değil: kapı sırasının niye tek hücreyle başladığı
  (65 çalışan çizim oynamamalı), sütun sayısının niye `bt-core`'da olduğu
  (ikinci genişlik yetkilisi doğmasın), ikinci düzlemin niye `Atlas`'ın içinde
  olduğu (ikinci `Metrics` doğmasın).
- **`crates/bt-gpu/src/renderer.rs`** — "Blend **parametre değil**: üç
  pipeline da onu istiyor ve sebepleri ayrı" yorumu dördüncü sebebini kazandı
  ve `pipeline()` parametresini geri aldı; yorum bunu söylemek zorunda.
- **`docs/OLCUMLER.md`** — envanter taraması `## Atlas yuva ayak izi` altına.
  Dosya kendini ölçülmüş sayıların **tek sahibi** ilan ediyor ve bu 3521 kod
  noktalı bir tarama; 019/015'in phase dosyasındaki tek tük sayı emsalleri
  bunu taşımaz. Yöntem de girer: hangi aralıklar, hangi punto/ölçek, sütun
  sayısının kaynağı (`East_Asian_Width`), ve hangi sayıların **kapsamı**
  olduğu — özellikle "1.66× bu makinenin Menlo'su için tek değer", makinelerin
  tamamı için bir yasa değil.
- **`docs/YOL-HARITASI.md`** — dört kalem: (a) "41'inin adayı Apple Color
  Emoji → emoji seti" **kapandı**; (b) 190 karakterlik küçültme kalemi
  **büyüdü** — 78 tek sütunlu emoji ona katıldı ve gerekçesi aynı; (c) "Atlas
  dolunca geri dönüşü yok" borcu **büyüdü**: geniş karakter iki yuva harcıyor
  ve 988'lik sınır adıyla yazılı; (d) **yeni kalem** — aynanın `CURSOR`'u
  karakter indeksi, dock'ta CJK varsa caret yanlış hücrede durur, "Dock çok
  satırlı girişi göstermiyor" maddesinin yanına.
- **`.tasks/README.md`** — 023'ün notu sonuç diliyle.

## Kabul

- `CLAUDE.md`'de kodla çelişen cümle kalmadı — özellikle pipeline sayısı,
  doku formatı ve "emoji henüz yok" paragrafı.
- `docs/OLCUMLER.md` envanteri ve **yöntemini** taşıyor; kapsam etiketleri
  yazılı.
- Yol haritasının dört kalemi işlendi; kapanan kalem kapandı diye, büyüyen
  kalem sayısıyla yazıldı.
- `make hepsi` yeşil (`make denetim`'in belge taramaları dahil).

## Uygulama Notları

- **`CLAUDE.md`'nin çelişen cümleleri phase-2'ye alındı.** Planın bu phase'e
  bıraktığı iş, set kapısının (`/code-review`) beşinci bulgusuyla yerinden
  oynadı: `CLAUDE.md`'nin kendi kuralı "buradaki bir cümle kodla çelişirse
  ikisinden biri **aynı commit'te** düzelir" diyor ve phase-2 aksi hâlde
  kodla çelişen bir sözleşme bırakırdı. Pipeline sayısı, iki düzlemin
  dokuları, emoji paragrafının tamamı ve jeton sözleşmesi (`CLAUDE.md` +
  `Makefile`) o commit'e girdi. Bu phase'e kalan: katman tablosunun iki
  sorumluluk satırı, `docs/OLCUMLER.md` envanteri ve yol haritasının borç
  kalemleri.
- **Envanter `## Atlas yuva ayak izi`'nin altına "ikinci tür" olarak girdi**,
  ayrı bir bölüm açılmadı: ikisi de bir sayım ve ikisi de fontun metriğinden
  türüyor, ayrışan şey sorulan soru. Yöntemin kapsam etiketi de yazıldı —
  "1.66×" bu makinenin Menlo'su için tek bir değer, karara giren şey oranın
  **2.0'ın altında** olması.
- **Yol haritasına planda olmayan bir kalem eklendi:** `bt-shell`'in beş
  sınaması release profilinde düşüyor ve `git stash` ile taban commit'te de
  doğrulandı, yani bu setin kusuru değil. Kapının profili debug olduğu için
  bugün hiçbir şeyi bloke etmiyor ama `cargo test --release` koşturan biri
  yanlış yere bakar.

## Checklist

- [x] `CLAUDE.md`: pipeline sayısı, doku formatı ve emoji paragrafı
      **phase-2'de** (kuralın kendisi aynı commit'i istiyor); iki sorumluluk
      satırı burada
- [x] `renderer.rs`: blend yorumu dördüncü sebebi ve parametrenin dönüşünü
      söylüyor (phase-2'de, kodla aynı commit)
- [x] `docs/OLCUMLER.md`: envanter + yöntem + kapsam etiketleri
- [x] `docs/YOL-HARITASI.md`: beş kalem — emoji kapandı, küçültme kalemi
      büyüdü/daraldı, atlas doyması büyüdü, `CURSOR` eklendi, release
      profili eklendi
- [x] `.tasks/README.md`: 023'ün notu
- [x] Doğrulama geçti (`make hepsi`)
