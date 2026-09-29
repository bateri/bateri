# Yedek glyph'in küçültülmesi — Bağlam

## Mevcut Durum

Seçili fontta olmayan karakter sistemin cascade'inden geliyor ve tek bir
geometrik kapıdan geçiyor: adayın **mürekkebi** hücreye (iki sütun ilan
edilmişse iki hücreye) sığıyorsa çiziliyor, sığmıyorsa kutu
(`font::accept` → `font::ink_fits_box`; sözleşme `CLAUDE.md` → Proje,
"kutu ya da tam glyph"). Kapının ara hâli yok: hücreyi %1 aşan aday da %100
aşan aday da aynı kutuya düşüyor. Kararı atlasın ömründe anahtar başına bir
kez veriliyor (`Atlas::slot`'un `Sprite::Char` kolu, ret negatif önbellekte).

Kapının kalibrasyonu iki kez bir kümeye bakılarak yapıldı ve ikisinde de
sınır dışındaki karakteri kullanıcı buldu: 019'un örnekleri 1.0'ın
yakınında aday taşımıyordu (`⏺`, ilerleme/mürekkep ayrışması), 021 `⎿`'yi
kullanıcıdan öğrendi. Bugün **hangi karakterlerin kutu çıktığını** söyleyen
bir envanter yok; tek istisna 023'ün geniş karakter sayımı ve CLAUDE.md'nin
"tek sütunlu emojinin 78'i … çaresi küçültme ve o ayrı bir karar" cümlesi.

## Motivasyon

Kullanıcı bateri'nin içinde koşan Claude Code'un çıktısında kutu gördü
(2026-09-29): artifact satırı `Updated ⧉ https://…`. Karakter `⧉` (U+29C9,
TWO JOINED SQUARES). Menlo'da yok, cascade Apple Symbols'tan getiriyor ve
aday hücreyi az aşıyor — ölçüldü, bu makine, Menlo 16pt @1x:

| | px |
|---|---|
| hücre ilerlemesi | 9.63 |
| aday ilerlemesi | 12.73 |
| aday mürekkebi (`origin.x` 1.0, genişlik) | 10.73 |

Mürekkep hücrenin ~1.11 katı; tek sütunlu olduğu için iki hücrelik kol
açılmıyor ve kutu kalıyor. Aynı satırdaki `→` Menlo'nun kendi glyph'i.

İkinci motivasyon kullanıcının cümlesi: "ben bunları kullanırken
buluyorum" — kusurlu karakterin önceden bilinmesi isteniyor. Setin ilk işi
bu yüzden düzeltme değil **envanter**: kapıyı bütün sembol aralıklarında
koşturan bir tarama. Küçültmenin üst sınırı o dağılımdan seçilecek, uydurulmayacak
(`CLAUDE.md` → Ölçülmemiş sayı yazılmaz).
