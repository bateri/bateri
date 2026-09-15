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

006'dan önce proje bir pencereydi: içinde gerçek shell koşuyor ve metin
kalın/eğik/altı çizili/üstü çizili doğru çiziliyordu. Ama **kopyalanamıyor,
yapıştırılamıyor, seçilemiyor, kaydırılamıyordu.** Yani sınanabilirdi ama
kullanılamazdı.

Eşik şu: **bateri'yi kendi terminalim olarak açabildiğim gün.** Bu tarihin
kendisi bir kilometre taşıdır, çünkü ondan sonra hatalar sınamadan değil
**kullanımdan** gelmeye başlar — ve kullanımın bulduğu hatalar başka türlü
bulunamaz.

| # | İş | Neden burada |
|---|---|---|
| 005 | ölçüm kancaları | 002, 003 ve 004'ün bekleyen on iki iddiası tek bir kanca setine bağlı. Taban, **bir sonraki büyük render değişikliğinden önce** alınırsa "hangi set yavaşlattı" sorusu cevaplanabilir olur; sonra alınırsa o soru kalıcı olarak cevapsız kalır. `docs/OLCUMLER.md` bilerek **kapsam dışı** bırakıldı (onu ilk `/measure` kurar) ve bench (`criterion`) de öyle; **bench'in dışarıda kalması** on ikinin ikisini bu setten sonra da açık bırakıyor — ikisi de saf `cargo bench` iddiası |
| 006 | pano + seçim + kaydırma + bundle | **Eşiği tek hamlede geçmek için bilerek şişirilmiş set.** Cila feda edilir: yapıştır, kopyala, fareyle seçim, tekerlek, `.app` bundle. Bundle burada çünkü bundle'sız süreç öne çıkamıyor, Dock ikonu almıyor ve varsayılan terminal olamıyor. 002'nin ertelenmiş Apache-2.0 attribution'ı da burada kapanır. **Bundle'ın bir yan ödevi var:** görünür pencere meşru kare sayısını değiştirir, yani `IDLE_FRAME_LIMIT` (boşta sıfır kare kapısı) bu sette **yeniden ölçülmeli** — bugünkü değeri görünmeyen bir pencerede ölçüldü *(sonradan: 006 phase-5 yoklamasında bundle'sız `make duman` penceresi de ekranda ve öndeydi; 005'in penceresi ise yoklanmamıştı, yani iki gerekçe de ölçülmüş değildi — sonuç `docs/OLCUMLER.md` → `## Boşta kare`)* |

> **006'nın kapsamı — karar verildi (2026-09-12).** Tek hamlede eşik **(b)**
> tutuldu: panel üç koldan bundle'ın ayrı sete çıkmasını önerdi, kullanıcı
> reddetti — Dock ikonu ve öne çıkma olmadan günlük kullanıma geçilemez, yani
> "kullanımın bulduğu hatalar" argümanı bundle'sız işlemiyor; OSC 52 köprüsü
> ise panelin önerisiyle 007'ye ertelendi. Set sonra yine kullanıcı
> kararlarıyla büyüdü ve beş phase'lik plan dokuza çıktı: tam ekran uygulamada
> tekerlek (3b, Karar 4 eki), Dock açılışında ev dizini ve yerel (4b, Karar 6
> eki), yerel yedeği (4c, aynı ekin son maddesi), seçim ve klavye rötuşu (4d,
> Kapsam eki) — dördü de kalite kapısının (`/code-review`) bulgularından doğdu.
> Kayıt `.tasks/006-gunluk-kullanim-esigi/discussion.md`'de, durumu
> `.tasks/README.md`'de.

## Eşikten sonra

| # | İş | Neden bu sırada |
|---|---|---|
| 007 | ayarlar + sekiz rollü tema + font seçimi | Font bugün sabit (`FALLBACK = "Menlo"`), renkler alacritty'nin varsayılan paletinden geliyor. Eşikten **sonra** çünkü neye ihtiyaç olduğu kullanırken daha iyi görülür. 006'dan ertelenen OSC 52 yazma yönünün `NSPasteboard` köprüsü de burada: ayar anahtarıyla gelir (`006/discussion.md` → Karar) |
| 008 | emoji + geniş glyph + kutu çizim | Üçü tek iş: 003 `teslim.md` B.3 "geniş karakter tek yuvaya kırpılıyor" diyor, 004 `plan.md` ikisini aynı sete bağlıyor. İçinde gerçek bir mimari çatal var: atlas `R8Unorm`, yani tek kanallı **kapsama maskesi**; emoji ise renkli bitmap. İkisi aynı dokuda yaşayamaz → ikinci atlas mı, RGBA mı, sprite başına format bayrağı mı? `/rfc` şart. TUI'ler (htop, tmux, lazygit) bu setten sonra düzgün görünür |
| 009 | sekme + bölme | |
| 010 | shell entegrasyonu (zsh/bash/fish) + OSC 133 | Kullanıcının rc dosyasına **asla** dokunulmaz: zsh `ZDOTDIR` sarmalayıcısı, bash `--rcfile`, fish `vendor_conf.d`. OSC 133 alacritty'de **yok**, `bt-core`'a eklenir |
| 011 | komut blokları | OSC 133 işaretlerinden okunur; `frame()` sınırına kanca ister |
| 012 | Input Dock | **Zincirin en ucu, kısayolu yok.** Pencere altında sabit ayrı satır editörü; zsh ZLE kancalarına, OSC 133'e ve komut bloklarına birden oturuyor. Ayrıca yazmayı devralan uygulamaları (Claude, Codex, REPL) davranıştan tespit edip alanı geri vermesi gerekiyor — bu, blokların çalışıyor olmasını varsayar |

Sonrası (sırasız): hareket/motion, materyal yüzey (`substrate` shader'ı,
grain/sheen), palet ve arama overlay'leri, durum çubuğu, Sparkle ile
güncelleme.

> **Açık soru — hareket/motion nereye oturur?** Kullanıcı 2026-09-14'te
> "animasyon senaryolarına bu rfc bitince bir değerlendireceğiz" dedi;
> değerlendirme henüz yapılmadı. Orkestratörün önerisi (**kullanıcı
> onaylamadı**): hareket toplu bir set değil, parça parça gelsin —
> (1) altyapı + imleç kayması 008'den sonra, çünkü ayarlar (007) ve geniş
> glyph (008) o zaman oturmuş olur ve aşağıdaki "boşta kare kapısı yavaş bir
> animasyonu kaçırır" borcu ilk animasyondan **önce** çözülmeli;
> (2) blok animasyonları 011 (komut blokları) ile; (3) yazma animasyonları
> (tuş vuruşu, silme) 012 (Input Dock) ile. Yukarıdaki sıra bu öneri yüzünden
> **değişmedi**; karar verilince gerekçesiyle değişir.

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
- **Pencereye duyarlı hasar yok.** Geçmişe kaydırılmış pencerede akan çıktı,
  ekranda hiçbir şey değişmediği hâlde her `Wakeup`'ta kare istiyor: `dirty`
  bayrağı pencereyi bilmiyor. **Boşta değil, çıktı akarken** — yukarıdaki
  boşta kare borcundan ayrı bir iş. Kaydırma (006) görünür kıldı, getirmedi;
  maliyeti ölçülmedi. Kaynak: 006 `phase-3.md` → `/audit` mercek 8 ve
  orkestratör kararı; `bt-core`'da `AdapterInner::dirty`'nin doc'u.
- **Fare raporlamasının geri kalanı.** 006 yalnız tekerlek kolunu getirdi
  (SGR, UTF-8, düz kodlama). Eksik: tıklama/sürükleme/hareket raporu,
  değiştirici bitleri (bugün `0`), yatay tekerlek, SGR-pixel. Görünen bedeli:
  `mouse=a` açık vim'de tıklama imleci taşımıyor, seçim yapıyor. Tıklama
  raporu seçimle çakışır, yani 006'nın seçim modelini (Karar 1) yeniden açar.
  Kaynak: 006 `phase-3b.md` → Kapsam dışı; 006 `discussion.md` → Karar 4 eki.
- **Klavye kalanları.** Home/End bilerek yutuluyor (terminfo `khome`/`kend`,
  oklar gibi DECCKM'e bağlı; yutmayı bağlayan sınama duruyor). Değiştiricili
  oklar (`\e[1;5A` vb.) yok. Ctrl+Shift+Tab `0x19` (readline/zle `yank`)
  gönderiyor, Ctrl+numpad Enter aynı yapıdan `0x03` — ikisi de gerçek
  klavyede doğrulanmadı; aday çare `charactersIgnoringModifiers`'ı
  `encode_key`'e geçirmek. Kaynak: 006 `phase-4d.md` → Uygulama Notları ve
  orkestratör kararı (WAIVE (1)); 006 `phase-3b.md` → Kapsam dışı.
- **Yerel ara kolu.** Kabuğun dili `preferredLanguages`'ın yalnız dil alt
  etiketinden alınıyor, etiketin kendi bölgesi atılıyor: `en-GB` dili + `TR`
  bölgesi → `en_TR` kurulu değil → `LANG=en_US.UTF-8`, oysa `en_GB.UTF-8`
  kurulu. `{dil}_{etiket bölgesi}` ara kolu bir ürün kararı; düşüş `en_US`
  olduğu için etkisi görünür (ABD tarih biçimi). Kaynak: 006 `phase-4c.md` →
  `/code-review` WAIVE (5) ve orkestratör kararı.
- **Küçük hijyen.** `make kur` boş hedef dizinini denetlemiyor — bugün
  zararsız. Pano sınamaları oluşturdukları geçici panoları bırakmıyor
  (`releaseGlobally` yok) — kullanıcıya görünmez. İkisi de doğrulanmadı;
  kullanıcı 006'ya almadı. Kaynak: 006 `phase-4c.md` → `/code-review` (8),
  (9); 006 `discussion.md` → Kapsam eki.
- **Paket dumanı.** Paketten açılan duman/ölçüm koşusu bugün elle yazılan bir
  `open` komutu; bir `Makefile` hedefi `docs/OLCUMLER.md` → `## Nasıl yeniden
  ölçülür`'deki dört tuzağın üçünü tasarımla kapatır. Aynı işe bir cümle
  daha: `report_and_exit`'in doc'u kapı düşünce jeton satırının basılmadığını
  ve tanının stderr'e gittiğini söylemiyor. Kaynak: 006 `phase-5.md` →
  `/simplify` takip önerisi ve orkestratör kararı.
