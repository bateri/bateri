# Yol haritası

Bu dosya **sırayı ve gerekçesini** tutar: hangi iş neden o sırada, neyin neye
dayandığı. **Durumu tutmaz** — o `.tasks/README.md`'nin işidir (`duzen.md` →
İndeks) ve iki yerde durum tutmak drift üretir. Buradaki bir satır açılmış bir
sete dönüşünce, ayrıntısı o setin `context.md`'sine taşınır ve burada tek
satıra iner.

**Tahmin değil sıra.** Takvim, süre ve efor tahmini bilerek yok: ölçülmemiş
sayı yazılmaz kuralı buraya da geçer (`CLAUDE.md` → Dil ve ölçüm). Sıra
değişebilir; değişince **gerekçesiyle** değişir.

Referans ürünün envanteri `docs/ARASTIRMA.md`'dedir. Orası Metalterm'in **ne
yaptığını** söyler, burası **bizim hangi sırayla yapacağımızı**.

## Günlük kullanım eşiği

Projenin bugünkü hâli bir pencere: içinde gerçek shell koşuyor ve metin
kalın/eğik/altı çizili/üstü çizili doğru çiziliyor. Ama **kopyalanamıyor,
yapıştırılamıyor, seçilemiyor, kaydırılamıyor.** Yani sınanabilir ama
kullanılamaz.

Eşik şu: **bateri'yi kendi terminalim olarak açabildiğim gün.** Bu tarihin
kendisi bir kilometre taşıdır, çünkü ondan sonra hatalar sınamadan değil
**kullanımdan** gelmeye başlar — ve kullanımın bulduğu hatalar başka türlü
bulunamaz.

| # | İş | Neden burada |
|---|---|---|
| 005 | ölçüm kancaları + `docs/OLCUMLER.md` | 002, 003 ve 004'ün bekleyen on iki iddiası tek bir kanca setine bağlı. Taban, **bir sonraki büyük render değişikliğinden önce** alınırsa "hangi set yavaşlattı" sorusu cevaplanabilir olur; sonra alınırsa o soru kalıcı olarak cevapsız kalır |
| 006 | pano + seçim + kaydırma + bundle | **Eşiği tek hamlede geçmek için bilerek şişirilmiş set.** Cila feda edilir: yapıştır, kopyala, fareyle seçim, tekerlek, `.app` bundle. Bundle burada çünkü bundle'sız süreç öne çıkamıyor, Dock ikonu almıyor ve varsayılan terminal olamıyor. 002'nin ertelenmiş Apache-2.0 attribution'ı da burada kapanır |

> **006'nın kapsamı henüz kesin değil.** İki seçenek tartışıldı: (a) düzenli
> sıra — pano, kaydırma, ayar, bundle ayrı setler; (b) tek hamlede eşik.
> Şu anki tercih **(b)**, gerekçesi yukarıdaki "kullanımın bulduğu hatalar"
> argümanı; bedeli setin normalden büyük olması. Karar `/rfc 006` açılırken
> kesinleşir ve o setin `discussion.md`'sine damgalanır.

## Eşikten sonra

| # | İş | Neden bu sırada |
|---|---|---|
| 007 | ayarlar + sekiz rollü tema + font seçimi | Font bugün sabit (`FALLBACK = "Menlo"`), renkler alacritty'nin varsayılan paletinden geliyor. Eşikten **sonra** çünkü neye ihtiyaç olduğu kullanırken daha iyi görülür |
| 008 | emoji + geniş glyph + kutu çizim | Üçü tek iş: 003 `teslim.md` B.3 "geniş karakter tek yuvaya kırpılıyor" diyor, 004 `plan.md` ikisini aynı sete bağlıyor. İçinde gerçek bir mimari çatal var: atlas `R8Unorm`, yani tek kanallı **kapsama maskesi**; emoji ise renkli bitmap. İkisi aynı dokuda yaşayamaz → ikinci atlas mı, RGBA mı, sprite başına format bayrağı mı? `/rfc` şart. TUI'ler (htop, tmux, lazygit) bu setten sonra düzgün görünür |
| 009 | sekme + bölme | |
| 010 | shell entegrasyonu (zsh/bash/fish) + OSC 133 | Kullanıcının rc dosyasına **asla** dokunulmaz: zsh `ZDOTDIR` sarmalayıcısı, bash `--rcfile`, fish `vendor_conf.d`. OSC 133 alacritty'de **yok**, `bt-core`'a eklenir |
| 011 | komut blokları | OSC 133 işaretlerinden okunur; `frame()` sınırına kanca ister |
| 012 | Input Dock | **Zincirin en ucu, kısayolu yok.** Pencere altında sabit ayrı satır editörü; zsh ZLE kancalarına, OSC 133'e ve komut bloklarına birden oturuyor. Ayrıca yazmayı devralan uygulamaları (Claude, Codex, REPL) davranıştan tespit edip alanı geri vermesi gerekiyor — bu, blokların çalışıyor olmasını varsayar |

Sonrası (sırasız): hareket/motion, materyal yüzey (`substrate` shader'ı,
grain/sheen), palet ve arama overlay'leri, durum çubuğu, Sparkle ile
güncelleme.

## Sete bağlanmamış borçlar

Bunlar kendi setlerini hak etmiyor; yukarıdaki setlerden birine yamanırlar.
Yamandıkları yer belli olunca buradan silinip o setin dosyasına geçerler.

- **Logger yok.** `tracing` bağlanmadı; yoksayılan olaylar (başlık, zil, pano)
  ve alacritty'nin `log` satırları **sessizce** düşüyor. Hata ayıklamayı
  körleştiriyor, o yüzden erken yamanmalı — 005 doğal adayı, ölçüm kancaları
  zaten aynı enstrümantasyon damarından geçiyor.
- **Kapanışta sınırsız bekleme.** `SIGHUP`'ı yutan çocuk (`trap '' HUP`) ana
  thread'i süresiz bekletiyor; duman koşusunda bekçi kesiyor, etkileşimli
  kullanımda **kesen yok**. Kalıcı çözüm `bt-core`'da sınırlı bekleme
  (`SIGHUP` → süre → `SIGKILL`). Eşikten önce kapanmalı: günlük kullanımda
  donan bir terminal kabul edilemez → 006.
- **Ölçeğin `bt-gpu`'ya iki kapısı** (`Surface::set_size` ve `cell_metrics`).
  003'ten devralındı, 004'te bilerek yeniden ertelendi. Ölçek borusuyla
  ilgili; ayar/tema seti ölçeği zaten elleyecek → 007.
- **004'ün altı kalemlik bulgu borcu.** Listesi ve gerekçeleri
  `.tasks/004-yazi-bicimleri/teslim.md` ile phase-3'ün `## Uygulama
  Notları`'nda. Sete bağlanmadı; ilgili dosyaya meşru biçimde dokunan ilk set
  toplar.
