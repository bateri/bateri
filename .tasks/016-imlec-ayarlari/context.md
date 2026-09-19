# İmleç ayarları — Bağlam

## Mevcut durum

014 imlece şekil ve blink verdi, 015 yüzeyini (köşe yarıçapı + hale) ve odak
davranışını. **İkisi de sayıları koda gömdü** ve 015 bunu bilerek yaptı
(R8: "ayar anahtarı yok", gerekçe geri alma yolunu basit tutmaktı).

Bugün kullanıcının dokunabildiği iki anahtar var, ikisi de `[terminal]`:

| anahtar | ne yapıyor |
|---|---|
| `cursor` | varsayılan şekil (`block` / `underline` / `beam`) |
| `cursor_blink` | `auto` / `on` / `off` |

Görünüşü ve ritmi belirleyen **dört sayı ise `const`**:

| sabit | yeri | değeri | ne |
|---|---|---|---|
| `CARET_RADIUS_RATIO` | `bt-gpu::frame` | 0.10 | köşe yarıçapı, hücre yüksekliğinin oranı |
| `CARET_GLOW_RATIO` | `bt-gpu::frame` | 0.4 | hale payı, sol payın oranı |
| `CARET_GLOW_ALPHA` | `bt-gpu::frame` | 0.10 | halenin tepe alfası |
| `HALF_PERIOD` | `bt-gpu::blink` | 0.5 s | blink'in yarım periyodu |

Beşincisi `IDLE_STOP` (15 s): blink hareketsizlikten sonra duruyor.

## Motivasyon

**Kullanıcı isteği** (2026-09-19): *"köşe ve shadow'u ayarlardan değiştirmeli
yapabilir miyiz? … amacım cursor üzerinde güzel tanımlı animasyonlar
yapabilmek."*

İstek bir kaprisin değil **yaşanmış bir sürtünmenin** sonucu: 015'in yüzey
değerleri iki tur **gözle** ayarlandı ve her turda kod değişip yeniden
derlenmesi gerekti — ilk hâli için kullanıcının cümlesi *"bu nasıl shadow
pavyona döndü ortalık"*, ikincisi için *"radius'u da azalt"*. Bir zevk sayısı
için derleme döngüsü yanlış araç; o sayıların yeri ayar dosyası.

## Kapsamdan **çıkarılan**: blink'e opacity

Kullanıcı önce "blink'e opacity ve farklı zamanlamalar" istedi, bedeli
anlatılınca **kendisi çıkardı** (*"tamam opacity'i boşver"*). Bedel şu:

- Bugün blink **sert açık/kapalı** ve saniyede **2 kare** ediyor; boşta sıfır
  kare sözleşmesi ayakta.
- Opacity demek blink'in bir **geçiş animasyonu** olması demek: her sönme ve
  yanmada ekran hızında kare. 150 ms'lik bir geçiş yarım saniyelik periyotta
  zamanın kabaca yarısını 60 fps'e çıkarır.
- 014'ün defterinde yazılı: `QUIET_FLOOR` (868 ms) ile blink periyodu
  "ilkesel olarak uyumsuz… kırılırsa **sesli** kırılıyor". Opacity o
  çarpışmayı gerçek yapar ve `make duman`'ın `sessiz=` katını kızartır.

**Periyot bunun tersine ucuz:** süre bir aralık, geçiş değil. 250 ms'de
saniyede 4, 1 s'de saniyede 1 kare — ekran hızına hiç çıkmıyor.

## Kanıt

- `bt-gpu/src/frame.rs` — üç sabit ve doc'ları ("seçilmiş, ölçülmemiş"; hale
  oranının doc'u iki turluk gözle inişi kaydediyor).
- `bt-gpu/src/blink.rs` — `HALF_PERIOD`, `IDLE_STOP`.
- `.tasks/015-imlec-cilasi/plan.md` → R8 ve `teslim.md` → Bedeli, açıkça.
- `.tasks/014-imlec-stilleri/teslim.md` → Bilinen sınırlar (`QUIET_FLOOR`).

## Mevcut tesisat — izlenecek emsal

Ayarın `bt-gpu`'ya inişinin **iki ayrı yolu** var ve doğru olanı seçmek bu
setin ilk kararı:

1. **`TerminalOptions` üzerinden `bt-core`'a** (`cursor`, `cursor_blink`):
   terminalin durumuyla ilgili olanlar. `cursor_blink` burada, çünkü DECSCUSR
   ile birleşiyor (`CursorBlink::resolve`).
2. **Doğrudan `DisplayLink`'e** (`cursor_motion`, `reduce_motion`): saf çizim
   ve ritim kararları. `bt-core` bunları **hiç görmüyor**;
   `Settings::changes` farkı buluyor, `bt-shell` `set_*` ile iletiyor, kayıt
   anında uygulanıyor.

Yarıçap, hale ve blink periyodu **ikinci gruba** benziyor: hiçbiri terminalin
durumu değil, üçü de yalnız boyamayı ilgilendiriyor.
