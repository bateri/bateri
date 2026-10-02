# Uzak sunucunun yük göstergesi — Tartışma

Ürün kararları kullanıcınındır ve `context.md` → Motivasyon'da; burada
yalnız onları gerçekleştirmenin açık noktaları var. Hepsi teknik (kullanıcı
iki sonucu ekranda ayırt etmiyor) ya da ürün kararının doğrudan sonucu;
ekranda ayrışan iki karar (Karar 6'nın "duraklayan gösterge" kolu ve Karar
3'ün dikey yerleşimi) gerekçesiyle aşağıda ve gözle kontrole bağlı.

## Karar 1: Örnekler uzaktan nasıl gelir? → ✅ A

**A — Yardımcı oturumda yeni bir istek türü, yoklamayla.** `helper_script`'e
`bt_load {seq} [p]` eklenir; bir zamanlayıcı her `stats_interval`'da bir
istek yollar, cevap `BT-R`/`BT-END` çerçevesinde gelir. Bağlantı aynı ssh.

**B — Aynı oturumda akış komutu** (`while :; do …; sleep N; done`, taslağın
"Veri nereden gelir" kutusu). Seri `eval` döngüsü akış sürerken başka satır
okumaz: ⌘-hover'ın `bt_stat`'ı akışın arkasında sonsuza kadar bekler, akışın
satırları da `parse_reply`'ın çerçevesine girip cevabı `Malformed` yapar.
Çalıştırmak protokolün yeniden tasarımı demek (satır başına kanal etiketi,
arka plan işi + kilit).

**C — İkinci kanal / ikinci ssh.** ControlMaster varsayılan değil
(`upload::ssh_argv` `ControlMaster=no` yazıyor, kullanıcının açık bir
master'ı varsa onu kullanır); olmayan makinede bu yeni bir bağlantı ve
"yeni bağlantı açılmaz" kararına aykırı.

A seçildi: protokol aynı kalıyor, istek başına maliyet bir `cat`'lik
gidiş-dönüş ve durdurmak "istek yollamamak". Üç sonucu açıkça kabul ediliyor:

- **045 Karar 10'un "boşta kapanır"ı daralıyor.** Örnekleme sürerken oturum
  hiç boşta değil, açık kalır; örnekleme durunca `IDLE` (120 s) sonra kapanır.
  `CLAUDE.md`'nin `remote_helper` cümlesi phase-4'te buna göre düzelir.
- **Açılış hatası örneklemeyi o nesil için durdurur.** `RETRY_AFTER` 10 s;
  3 s'lik yoklama başarısız açılışın üstüne her 10 s'de yeni bir ssh denemesi
  yapardı — sunucunun auth günlüğünde gürültü, fail2ban'li makinede ban.
  Kural: **açılış** hatası (parola isteyen sunucu, zaman aşımı, erişilemeyen
  host) → o nesilde örnekleme biter, gösterge hiç çıkmaz (045'in sınırının
  aynısı). **Açık** oturumda hata (bağlantı koptu, cevap zaman aşımı,
  okunamayan cevap) → gösterge gizlenir, bir sonraki tikte bir kez yeniden
  açılır; o da açılamazsa ilk kural.
- **Worker seri.** Bir `Count` (indirme sayfasının klasör sayımı, 120 s'ye
  kadar) sürerken örnek bekler. Uçuşta en çok **bir** örnek isteği var,
  istekler yığılmaz; `Count` boyunca gösterge son değeri gösterir — bilinen
  sınır. Her hata ya da zaman aşımı göstergeyi gizler (`None`), dondurmaz.

## Karar 2: Ne ölçülür, nasıl hesaplanır? → ✅

- **CPU %** iki `/proc/stat` örneğinin farkı (`idle + iowait` boşta sayılır).
  İlk örnek yalnız sayaçları verir; ilk tiki bir tasarım sabiti
  (`FIRST_FOLLOW`, 1 s) sonra ikinci izler, sonrası `stats_interval`.
  Reddedilen: betiğin içinde `sleep 1` — seri worker'ı bir saniye kilitler,
  o arada ⌘-hover bekler.
- **Bellek %** `(MemTotal − MemAvailable) / MemTotal`; `MemAvailable` yoksa
  (3.14 öncesi çekirdek) `MemFree + Buffers + Cached`. Swap `SwapTotal −
  SwapFree`.
- **Disk %** `df -P /`'nin kapasite sütunu (df'in kendi yuvarlaması).
- **Load, uptime** `/proc/loadavg`, `/proc/uptime` — ucuz, her örnekte.
- **Yalnız popover açıkken (`p`)**: `/etc/os-release`'in `PRETTY_NAME`'i,
  çekirdek sayısı (`/proc/stat`'ın `cpuN` satırları) ve `ps -eo
  comm,pcpu --sort=-pcpu`'nun ilk üçü. `--sort` procps'a özgü: başarısızsa
  süreç listesi boş, örnek yine geçerli.
- **`/proc` yoksa** betik `BT-NOPROC` der, o nesilde örnekleme sessizce biter
  (ilk sürüm yalnız Linux uzak).
- Yüzdeler **tam sayıya yuvarlanır** ve gösterilen değer odur: eşitlik
  kapısı (Karar 6) yuvarlanmış değeri karşılaştırır.

Hesap `bt-shell-common`'da (yeni `remote_stats` modülü: protokolün ayrıştırması,
fark, yuvarlama, geçmiş), çünkü platformsuz ve `make linux` onu **gerçek
`/proc`'lu** bir makinede `/bin/sh` üstünden koşturuyor; macOS'taki sınama
`BT-NOPROC` kolunu görüyor.

## Karar 3: Sparkline küçük sınıfta nasıl çizilir? → ✅ A (kodu aç)

**A — Yordamsal kapıyı küçük sınıfta küçük ölçüyle aç.** `Atlas` küçük
yüzden ikinci bir `Metrics` türetir (aynı `rules::metrics` kuralı; genişlik
`context_cell_w`), yordamsal karakteri o ölçüyle ayrı bir tampona çizer ve
büyük yuvaya **taban çizgisi hizalı** taşır: küçük hücrenin taban satırı
büyük hücreninkine oturur, yani `▁` küçük metnin alt payında, `█` küçük
metnin satır yüksekliği kadar. Döşeme ölçüsü sütun adımına eşit olduğu için
yan yana bloklar boşluksuz birleşir. Kapının gerekçesi (ölçü ayrışması)
ortadan kalktığı için **bütün yordamsal aile** küçük sınıfta açılır (blok,
Braille, çizgi, teknik küme) — bağlam satırında çizgi karakteri taşıyan bir
yol da artık hücreyi aşmıyor. Atlas kapasitesi açısından maliyet tembel:
yalnız gerçekten çizilen karakter yuva alır (sparkline sekiz yuva), ailenin
tamamı değil.

**B — Kapı kapalı, Menlo'nun bloğu.** Kod değişmez, sparkline Menlo'nun
hücreyi doldurmayan, döşemeyen bloklarıyla çıkar — 021'in varlık sebebi
olan kusur, kullanıcının istediği grafikte.

**C — Yalnız U+2581–2588 için özel bir kural sprite'ı.** Çalışır ama bağlam
satırında ikinci bir "yordamsal" yol doğurur ve aile içindeki öteki
karakterler (aynı satırdaki bir yolun `─`'si) hâlâ büyük ölçüyle bozuk kalır.

A seçildi — "Boşlukta kullanıcı tarafı seçilir": kısıt bugünkü aritmetiğin,
sparkline ise kullanıcının ürün kararı. Dikey hizanın kendisi (taban çizgisi
hizalı küçük satır kutusu) ekranda görülen bir tasarım seçimi ve gözle
kontrolün konusu; seçilen hiza küçük metinle aynı satırda oturan grafik
veriyor, alternatifi (büyük hücrenin tam yüksekliği) metinden uzun çubuklar.

## Karar 4: Gösterge ve yerleşim nerede kararlaşır? → ✅

`bt-core`'da, `render_remote_context`'in yanında, `transfer_layout`
örüntüsüyle: tek bir yerleşim fonksiyonu çizim, fare (`stats_at`) ve
popover çıpası (`stats_span`) için okunur.

- **Biçimler.** `sparkline`: `cpu {8 hücre} {n}%  mem {n}%`; `numbers`:
  `cpu {n}%  mem {n}%`; `alerts`: eşik aşan yoksa `●` (`success`), varsa
  yalnız aşanlar. Disk her biçime **yalnız** %85 eşiğini geçince eklenir
  (`  disk {n}%`). Etiket, sparkline ve eşik altı sayı `dim`; eşiği aşan sayı
  `warning`/`error`, kritikte başında `▲` (`context.md` → brief geçerli).
- **Sparkline** son 8 CPU örneği; seviye `⌊v / 12.5⌋` 7'ye kırpılı →
  U+2581 + seviye. Sekizden az örnekte **sabit 8 sütun**, eksikler solda
  boşluk: grup sağa yaslı ve genişliği her örnekte oynasaydı yolun bütçesi
  de oynardı.
- **Merdiven** (taslağın betiğinin ta kendisi): basamaklar `[tam, sayılar,
  en kötü]` (`alerts`'te `[uyarılar-ya-da-●, en kötü]`), her biri **tam yol**
  ve aradaki en az 2 sütunla denenir, sağa yaslı. Hiçbiri sığmazsa ve en kötü
  değer eşik aşıyorsa `⇄ host + 2 + en kötü` sığdığı sürece en kötü kalır,
  yol kalan bütçeye soldan kısalır; değilse gösterge düşer, yol bugünkü
  kuralla. Host asla kısalmaz; `⇄ host` sığmazsa bugünkü gibi yalnız `⇄`.
  En kötü = önce önem (kritik > uyarı > normal), sonra değer.
- **Aktarım sürerken** aktarım satırı bağlam satırının yerini alıyor ve
  gösterge çizilmiyor (`render_transfer` kolu); `stats_at`/`stats_span`
  aktarım varken `None`.
- **Eşikler** `bt-core`'da tasarım sabiti (`STATS_THRESHOLDS`): çizimin rengi
  ve popover'ın çubuk rengi aynı sabitten okur.

## Karar 5: Göstergenin değeri sınırdan nasıl geçer? → ✅

`DockContext::stats: Option<RemoteStats>` (aktarımın emsali: bağlam
kilit turunda `clone_from`'la alınıyor). `RemoteStats` `Copy` ve sabit
boyutlu: gösterim biçimi, `cpu`/`mem`/`disk` yüzdeleri, geçmiş seviyeleri
(`[u8; 8]` + sayı) — kare başına ayırma yok. Yazarı
`Session::set_remote_stats(command, Option<&RemoteStats>)`:

- **Nesil kapılı.** `command` o anki uzak oturumun nesli değilse yazılmaz:
  biten ssh'ın geç cevabı yeni host'a düşmez (`link_remote_verified`'ın emsali).
- **Eşitlik kapılı.** `set_transfer`'in örüntüsü: değer aynıysa kare yok.
- **Uzak durumla silinir.** `ShellLog`'un uzak hedefi sildiği ya da başka bir
  hedefe geçtiği her yerde (`C`/`D`/`A` kolları, `set_remote`) `stats` da
  `None`: yoksa yeni host ilk örneğe kadar öncekinin sayılarını gösterirdi.
- Geçmiş yalnız `sparkline` biçiminde taşınır, öteki biçimlerde boş: görünmeyen
  bir geçmiş değişimi kare istemesin.

## Karar 6: Örnekleme ne zaman koşar, ne zaman durur? → ✅

Zamanlayıcı ana kuyrukta, pane başına; karar saf bir durum makinesinde
(`bt-shell-common::remote_stats::Schedule`: olay → sonraki eylem, Linux'ta da
sınanır), `bt-shell-macos` yalnız `dispatch` `after`'ını koşturur.
`after` iptal edilemediği için her kurulan tik bir **jeton** taşır, bayat
jeton yok sayılır.

- **Koşar:** uzak oturum etkin, `stats != "off"`, pane görünür, son
  etkileşimden beri `STATS_IDLE`'dan (2 dk, tasarım sabiti) az geçti, nesilde
  açılış hatası yok, `BT-NOPROC` gelmedi.
- **Durur:** uzak oturum biter; `off`; pane örtülür (arka sekme, küçültme,
  büyütülmüş bölmenin arkası — `SplitView::apply_visibility`'den pane'e bir
  kanca); `STATS_IDLE` boyunca etkileşim yok; Karar 1'in açılış hatası;
  `BT-NOPROC`.
- **Etkileşim** = pane'in view'ına tuş, fare basışı, tekerlek ya da fare
  hareketi, ve pencerenin key olması.
- **Geri gelince ilk örnek hemen** (CPU için `FIRST_FOLLOW` sonra ikincisi);
  geçmiş baştan dolar — boşluklu bir zaman ekseni sürekli bir grafik gibi
  okunurdu.
- **Duraklayan gösterge son değeri gösterir, gizlenmez.** Örtülü pane'de
  görünmüyor zaten; etkileşimsiz kalan pencerede ilk tuş/fare hareketi taze
  örneği hemen getiriyor ve gizlenip yeniden belirmek çubukta zıplama olurdu.
  Bedeli adıyla: 2 dakikadan uzun süre dokunulmadan izlenen bir `tail -f`'te
  CPU donmuş görünür — bilinen sınır.

**Boşta sıfır kare — saatin üç şartı** (`CLAUDE.md` → "Boşta sıfır kare"):
(1) içerik gerçekten değişiyor — kare yalnız `set_remote_stats`'ın eşitlik
kapısı değişim gördüğünde, `Waker::wake`'le (içerik tadı, `icerik=` sayması
doğru); (2) periyot ekran hızından çok düşük — `stats_interval` ≥ 2 s,
sparkline'da en çok örnek başına bir kare; (3) adlandırılmış durma koşulları
yukarıdaki liste. Zamanlayıcı link'i uyandırmıyor, kare yolunda saat kurmuyor.

## Karar 7: Popover → ✅

`uploader`'ın listesinin emsali, ayrı bir modülde (`bt-shell-macos::stats_popover`):
`transient`, göstergenin sütun aralığına (`stats_span` → `context_span_rect`)
bağlı, Esc'i aynı yerel izleyici örüntüsü yutuyor. Açıkken örnek isteği `p`
bayrağıyla gidiyor (OS, çekirdek, süreçler) ve içerik yerinde tazeleniyor;
kapanınca bayrak düşüyor. `TerminalPane` iki popover'ın da delegate'i:
`popoverWillClose:` bildirimin nesnesini iki popover'la karşılaştırıp kendi
kapanış zamanı yuvasına yazar (bugünkü tek `list_closed_at` iki yuvaya
ayrılır). Gösterge kaybolursa (aktarım başladı, uzak oturum bitti, `off`)
popover kapanır. El imleci göstergenin aralığı için aynı cursor-rect listesine
girer.

## Karar 8: Ayarlar → ✅

`[remote] stats` (`sparkline` varsayılan | `numbers` | `alerts` | `off`) ve
`[remote] stats_interval` (saniye, varsayılan 3, kabul aralığı 2–60 — tasarım
sabiti; kabul edilmeyen değer kendi anahtarını değiştirmez,
`Settings::parse_keeping`). İkisi `bt-core::settings`'te (`RemoteFiles`'ın
yanında kendi `RemoteStatsSettings`'i), `SettingsEdit` varyantları, şablon,
`docs/AYARLAR.md`; ayar penceresinin Remote Files kategorisinde iki satır
(açılır menü + stepper'lı sayı alanı). Kayıt anında uygulanır: biçim
değişince gösterge yeniden yazılır, `off` örneklemeyi durdurup göstergeyi
kaldırır, aralık değişimi bir sonraki tikten geçerli.

## Karar (2026-10-02, otonom akış)

Panel koşmadı: discussion'da çok yaklaşımlı iki seçim var (Karar 1 veri
yolu, Karar 3 küçük sınıf kapısı), ama ikisi de pahalı karar sınıfının
dosyalarına dokunmuyor — yardımcı betik `crates/bt-shell-common/src/remote_files.rs`'te,
`assets/shell/`'de değil; yeni crate yok; `Cell`'e alan yok; dock'un kare
başına çizim maliyeti her seçenekte aynı (`transfer_layout` emsali). Ürün
kararları kullanıcının (`context.md` → Motivasyon), aşağıdakiler onları
gerçekleştirmenin teknik kararları.

- **Seçilen:** Karar 1–8'in ✅ kolları — yardımcı oturumda `bt_load`
  yoklaması (açılış hatası nesli durdurur, uçuşta tek istek, hata gizler);
  hesap `bt-shell-common::remote_stats`'ta, ilk CPU örneğine hızlı takip;
  küçük sınıfta yordamsal kapı küçük ölçüyle ve taban çizgisi hizalı açılır
  (bütün aile); yerleşim merdiveni `bt-core`'da `transfer_layout`
  örüntüsüyle; `DockContext::stats` nesil + eşitlik kapılı, uzak durumla
  silinir; saf `Schedule` + jetonlu ana kuyruk zamanlayıcısı, duraklayan
  gösterge son değeri tutar; popover ayrı modülde, iki kapanış yuvası; iki
  ayar anahtarı Remote Files'ta.
- **Reddedilen:** aynı oturumda akış komutu — seri `eval` döngüsünü ve
  `parse_reply`'ın çerçevesini bozar; ikinci kanal/ssh — ControlMaster
  varsayılan değil, yeni bağlantı açar; betik içinde `sleep` — seri worker'ı
  kilitler; küçük sınıfta Menlo'nun bloğu — döşemeyen sparkline; yalnız
  sparkline için özel sprite — ikinci yordamsal yol, ailenin geri kalanı bozuk
  kalır; açılış hatasında `RETRY_AFTER`'la yeniden denemek — 10 s'de bir ssh
  denemesi; duraklayınca gizlemek — çubukta zıplama.
