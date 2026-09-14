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
| 005 | ölçüm kancaları | 002, 003 ve 004'ün bekleyen on iki iddiası tek bir kanca setine bağlı. Taban, **bir sonraki büyük render değişikliğinden önce** alınırsa "hangi set yavaşlattı" sorusu cevaplanabilir olur; sonra alınırsa o soru kalıcı olarak cevapsız kalır. `docs/OLCUMLER.md` bilerek **kapsam dışı** bırakıldı (onu ilk `/measure` kurar) ve bench (`criterion`) de öyle; **bench'in dışarıda kalması** on ikinin ikisini bu setten sonra da açık bırakıyor — ikisi de saf `cargo bench` iddiası |
| 006 | pano + seçim + kaydırma + bundle | **Eşiği tek hamlede geçmek için bilerek şişirilmiş set.** Cila feda edilir: yapıştır, kopyala, fareyle seçim, tekerlek, `.app` bundle. Bundle burada çünkü bundle'sız süreç öne çıkamıyor, Dock ikonu almıyor ve varsayılan terminal olamıyor. 002'nin ertelenmiş Apache-2.0 attribution'ı da burada kapanır. **Bundle'ın bir yan ödevi var:** görünür pencere meşru kare sayısını değiştirir, yani `IDLE_FRAME_LIMIT` (boşta sıfır kare kapısı) bu sette **yeniden ölçülmeli** — bugünkü değeri görünmeyen bir pencerede ölçüldü *(sonradan: 006 phase-5 yoklamasında bundle'sız `make duman` penceresi de ekranda ve öndeydi; 005'in penceresi ise yoklanmamıştı, yani iki gerekçe de ölçülmüş değildi — sonuç `docs/OLCUMLER.md` → `## Boşta kare`)* |

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
  körleştiriyor, o yüzden erken yamanmalı. 005 doğal adayıydı — ölçüm
  kancaları aynı enstrümantasyon damarından geçiyor — ama **almadı**: ayrı
  bir bağımlılık kararı (`tracing`) ve seti "ölçüm + gözlemlenebilirlik" diye
  şişirirdi (`005/plan.md` → Kapsam Dışı). Hâlâ sete bağlanmadı.
- **Kapanışta arkada kalan çocuk.** Sınırsız bekleme **kapandı** (005
  phase-2b) ve bu maddenin eski "→ 006" bağı da onunla düştü: 006'ya
  bağlanma gerekçesi "günlük kullanımda donan bir terminal kabul edilemez"
  idi, artık donmuyor. Kalan borç daha dar ve eşiği bloke etmiyor: süre
  dolunca çocuk arkada bırakılıyor, onu süreç çıkışı topluyor. Bu yüzden
  **sete bağlanmadı** — `bt-core`'un kapanış tasarımına meşru biçimde dokunan
  ilk set toplar. Ayrıntısı ve çürütülmüş çaresi `CLAUDE.md`'nin kapanış
  maddesinde; artık **ölçülebilir** de (`kapanis=abandoned`).
- **Ölçeğin `bt-gpu`'ya iki kapısı** (`Surface::set_size` ve `cell_metrics`).
  003'ten devralındı, 004'te bilerek yeniden ertelendi. Ölçek borusuyla
  ilgili; ayar/tema seti ölçeği zaten elleyecek → 007.
- **Boşta kare kapısı yavaş bir animasyonu kaçırır.** 005 phase-3 sınırı
  ölçümle büyüttü; bedeli, kapının algılama tabanının yükselmesi oldu — durma
  koşulu unutulmuş yavaş bir blink bugün yeşil geçer. Bu depoda öyle bir
  animasyon **yok**, ama hareket/motion seti tam bunu getirecek. Sayılar,
  mekanizma ve aday çözüm (`istek=`'i orana çevirip kapıya bağlamak; eşiği
  **ölçülmedi**) `bt-shell`'in `IDLE_FRAME_LIMIT` doc'unda; set açılınca bu
  madde onun `context.md`'sine taşınır → hareket/motion. Aynı sabit 006
  phase-5'te görünür pencerede yeniden ölçüldü ve değişmedi; bu madde o
  ölçümle **kapanmadı**, algılama tabanı aynı — **tek sabit, iki ayrı iş**.
- **About paneli erişilemiyor.** 006 phase-4 atfı AppKit'in standart About
  panelinin okuduğu `Credits.html` ile pakete koydu, ama paneli açan menü
  öğesi yok (menü seti 00X). O set `orderFrontStandardAboutPanel:`'ı bağlar
  ve paneli **gözle** doğrular — bugün hiç görülmedi → menü.
- **Üçüncü taraf bildirimlerinin geri kalanı.** 006 yalnız `alacritty_terminal`'ın
  (Apache-2.0) borcunu kapattı. macOS ağacındaki diğer dış paketlerin çoğu
  MIT ya da MIT seçeneği taşıyor (`objc2` ailesinin dördü yalnız MIT) ve MIT
  de bildirimin kopyalarla gitmesini istiyor. Paket **dağıtılmadan önce**
  toplanmalı → imza/notarization/Sparkle ile gelecek dağıtım seti. Aynı set
  indirilen kopyada Gatekeeper'ın gerçek davranışını da görmeli (kanıt ve
  açık soru: 006 `phase-4.md` → Uygulama Notları).
- **004'ün altı kalemlik bulgu borcu.** Listesi ve gerekçeleri
  `.tasks/004-yazi-bicimleri/teslim.md` ile phase-3'ün `## Uygulama
  Notları`'nda. Sete bağlanmadı; ilgili dosyaya meşru biçimde dokunan ilk set
  toplar.
