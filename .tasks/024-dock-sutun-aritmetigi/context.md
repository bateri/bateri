# Dock sütun saysın — Bağlam

## Mevcut Durum

`dock::render` giriş satırını aynanın dizgilerinden çiziyor ve **karakter
indeksini sütun sanıyor**. Aynı `index` dört işe birden giriyor:

| kullanım | bugünkü hâl | doğru birim |
|---|---|---|
| hücrenin sütunu | `TEXT_COL + (index - skip)` | **sütun** |
| yatay kaydırma penceresi | `skip = (cursor + 1) - available` | **sütun** |
| `region_highlight` araması | `style_at(state, index)` | **karakter** ✓ |
| caret'in sütunu | `TEXT_COL + (cursor - skip)` | **sütun** |

Dördüncü sütun kritik: `region_highlight`'ın aralıkları **ZLE'nin kendi
birimi**, yani karakter indeksi — `style_at` olduğu gibi doğru ve öyle
kalmalı. Bozuk olan yalnız **indeks → sütun** eşlemesi.

Izgara tarafı zaten sütun sayıyor: alacritty `Flags::WIDE_CHAR`'ı
`unicode_width` ile kuruyor ve 023'ten sonra `bt-core` o bayrağı sınırdan
geçiriyor (`Cell::wide`). Dock ise aynı bayrağı **hiç kurmuyor** ve bu 023'ün
yazılı bir değişmezi (`the_dock_never_marks_a_cell_wide`) — gerekçesi tam
olarak yukarıdaki tablo: sütun karakterden türüyorsa iki hücrelik bir glyph
komşusunun üstüne boyar.

## Motivasyon

**Kullanıcı bildirdi (2026-09-22):** yazılan emoji dock'ta görünmüyor ve bazı
emojilerde giriş satırı dock'tan ızgaraya, prompt'un yanına fırlıyor. İkisi de
beklenen davranış değil; ikisinin de kökü yukarıdaki tablo.

023'ün kapanışında bu söylenmedi ve söylenmemesi bir süreç kusuruydu: setin
gözle kontrol satırındaki dört sahnenin dördü de `echo`'ydu, yani dördü de
ızgara — emojinin **yazıldığı** yüzey listede yoktu. Gerekçesi ve çaresi
`.claude/skills/rfc` → Bulguyu işleme yolu ile `proje.md` → Kalite kapısı 4'te
yazılı.

### Kanıt — ölçüldü (2026-09-22, geçici prob, geri alındı)

Aynı ızgara ve aynı ayna, iki emoji:

```
🎉  U+1F389 (tek kod noktası)  → caret_in_dock=true,  ızgara imleci gizli
❤️  U+2764 U+FE0F (VS16)       → caret_in_dock=false, ızgara imleci görünür
```

İki ayrı belirti, iki ayrı kol:

1. **`🎉` dock'a düşüyor ama kutu çiziliyor.** Bastırma çalışıyor, satır
   dock'ta — ama dock `wide` kurmadığı için kapı `Half::Whole` soruyor ve
   emojinin mürekkebi hücrenin **1.66 katı** (023 envanteri), yani tek hücreye
   sığmıyor ve tofu dönüyor. Aynı karakter ızgarada renkli, dock'ta kutu:
   sessiz bir tutarsızlık.
2. **`❤️` satırı ızgaraya fırlatıyor.** Bastırmanın tazelik kapısı iki kesin
   veriyi karşılaştırıyor — aynanın son mürekkebi ile ızgaranın son mürekkebi
   — ama **iki taraf farklı birim okuyor**: ayna tarafı son boşluk olmayan
   `char`'ı alıyor (`❤️`'de **U+FE0F**), ızgara tarafı hücrenin `c`'sini
   (**U+2764**; birleştirici alacritty'de `CellExtra`'da yaşıyor). İkisi
   hiçbir zaman eşleşmiyor, kapı "bayat" diyor, bastırma bırakılıyor ve satır
   caret'iyle ızgarada kalıyor. Kapının yanlış yönü **bilinçli ve güvenli**
   (satırı iki yerde gösterir, kaybetmez) ama bu kol bir kusur.

İkisi de **023'ten eski** — birincisi 023'ün kendi değişmezinden, ikincisi
012'den. 023 onları görünür kıldı: emoji artık çizilebildiği için yazılıyor.

### Aynı kökten gelen üçüncü kalem

`docs/YOL-HARITASI.md`'nin "Aynanın `CURSOR`'u karakter indeksi, sütun değil"
borcu bu setin ta kendisi: CJK taşıyan bir satırda caret yanlış hücrede durur
ve ondan sonraki her harf bir sütun kayar. Üç belirtinin tek çaresi var.
