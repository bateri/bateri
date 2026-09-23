# Dock'ta yazma ve silme animasyonları — Bağlam

## Mevcut Durum

Dock ZLE'nin aynasıdır (`CLAUDE.md` → Bugünkü hâl, OSC 8133): kabuk her satır
çiziminde `PREDISPLAY`/`BUFFER`/`POSTDISPLAY`/`region_highlight`/`CURSOR`'ı
**bütün olarak** yollar, okuyucu thread onu `ShellLog::apply_dock`'ta
(`crates/bt-core/src/shell.rs`) `DockState`'e yazar ve bir önceki aynayı
ezer. Aynanın içinde "bu glyph'i kullanıcı az önce yazdı / sildi" diye bir bilgi
yok — yalnız satırın son hâli var.

Kare yolunda `Session::dock` (`crates/bt-core/src/session.rs`) aynayı kopyalar
ve `dock::render` (`crates/bt-core/src/dock.rs`) onu çözülmüş hücrelere çevirir
(sütun, renk, biçim, pencereleme `skip`'i); `bt-gpu` hücreleri
`Frame::push_dock` ile `dock_glyphs`'e koyar ve ikinci `setViewport`'ta
`cell` / `emoji` pipeline'larıyla çizer (`Renderer::encode_dock`). Liste
**yalnız içerik karesinde** kurulur: hareket karesi (`link.rs`'in "hasar yok"
dalı) listeleri korur, yalnız caret'i taşır (`Frame::move_caret`). Yani bugün
dock'un glyph'leri kareden kareye değişemez; değişen tek şey caret.

Hareketin altyapısı `bt-gpu::motion`'da: üç animatör (imleç, öteleme,
süzülme), tek uyku kapısı (`Motion::settled`), zamana bağlı kare talebi
hareket saatinden (`link.rs` modül başlığı). Ayarların `[motion]` bölümü bugün
`cursor_motion`, `reduce_motion`, `smooth_scroll` taşır ve kayıt anında
`DisplayLink`'e iner (`Changes::motion`); 029'un ayar penceresinde Motion
bölmesi bu üçünü gösterir (`crates/bt-shell/src/settings_window.rs`).
`settings.rs`'in başlığı `[motion] keypress`'i "sonraki setlerin anahtarı"
diye zaten adıyla anıyor.

Glyph pipeline'ının düzeni sabit ve iki taraftan assert'li: `GlyphInstance`
32 bayt, **boyutu yok** (dörtlü her zaman tam bir hücre, `cell_px` uniform'u)
— yani bugünkü aritmetikle bir glyph ölçeklenemez, kaydırılamaz ya da
parçalanamaz, yalnız rengi ve alfası instance'tan gelir. Dört pipeline'ın
**blend'i aynı** (`SourceAlpha`, `renderer.rs`'in "Dört pipeline da ön
çarpımsız fragment veriyor" yorumu): emoji dokusunun ön çarpımı yüklemeden önce
geri alınıyor (`emoji_fragment`'in yorumu). *`CLAUDE.md` emoji için "RGB kaynak
çarpanı `One`" diyor; kod ve shader yorumu bunun tersini söylüyor, yani sözleşme
cümlesi bayat — bu setin CLAUDE.md güncellemesiyle düzelir.*

## Motivasyon

**Kullanıcı isteği (2026-09-23):** "karakterlerin docktaki harflerin oluşumu
ve silinmesi sırasında animasyon ekleyebilir miyiz? metalterm için bunlar
animasyonlu ayarlanmış, oradaki seçenekler gibi istiyorum".

Referansın Motion panelinde iki efekt var — **Keypress** ("per-glyph arrival
while you type") ve **Erase** ("the glyph you delete with Backspace") — ve
değer listeleri `docs/ARASTIRMA.md` → Hareket (`[motion]`) tablosunda. Kullanıcı
listelerin **hepsini** istiyor. Metalterm kapalı kaynak: adları biliyoruz,
matematiği bilmiyoruz; her efekt adının çağrıştırdığı hareketten tasarlanıyor.

İş 012'de adıyla ertelenmişti (`docs/YOL-HARITASI.md` → 012 satırı: "tuş vuruşu
ve silme animasyonları — `keypress`, `delete_mode` — setten çıkarıldı …
animasyonlar aynanın üstüne kurulur ve kendi setine gider"). Ayna, sütun
aritmetiği (024) ve tazelik (025) artık yerinde; eksik olan üç şey:

1. **"Hangi glyph geldi / gitti" bilgisi.** Ayna tuş başına bütün satırı
   yeniden yolluyor; eklenen ve silinen glyph iki ayna arasındaki farktan
   bulunmak zorunda. Silinen glyph'i ayna artık taşımıyor, yani onun rengi ve
   yeri farkın alındığı anda yakalanmalı ("hayalet").
2. **Kareden kareye değişen dock glyph'i.** Hareket karesinin bugün dock
   listesine dokunan bir yolu yok.
3. **Ölçeklenebilen, kayabilen, parçalanabilen glyph.** `GlyphInstance`'ın
   aritmetiği yalnız alfa ve renk veriyor; `pop`, `rise`, `iris`, `shatter`
   gibi adlar geometri istiyor.
