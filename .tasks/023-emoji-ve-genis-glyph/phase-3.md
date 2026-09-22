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

## Checklist

- [ ] `CLAUDE.md`: pipeline sayısı, doku formatı, emoji paragrafı, iki
      sorumluluk satırı
- [ ] `renderer.rs`: blend yorumu dördüncü sebebi ve parametrenin dönüşünü
      söylüyor
- [ ] `docs/OLCUMLER.md`: envanter + yöntem + kapsam etiketleri
- [ ] `docs/YOL-HARITASI.md`: dört kalem (kapanan, büyüyen ikisi, yeni)
- [ ] `.tasks/README.md`: 023'ün notu
- [ ] Doğrulama geçti (`make hepsi`)
