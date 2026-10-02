# Uzak dosyayı indirme ve önizleme — Tartışma

Karar-listesi biçimi. Karar 1–2 ürün sorusuydu (kullanıcı, 2026-10-01); Karar 3–9
konuşmada (2026-10-01) kullanıcıyla verildi, ekranları tasarım tuvalinde
(`context.md` → Motivasyon); Karar 10–15 teknik.

## Karar 1: Uzak yol ⌘ basılıyken nasıl doğrulanır? → ✅ B (kullanıcı)

044'ün sözleşmesi "doğrulanmamış yol vurgulanmıyor". Uzakta doğrulama bir
ssh gidiş-dönüşü.

- **A — Doğrulamadan vurgula.** ⌘ altındaki her kelime adayı anında
  vurgulanır; `ls -l`'de `deploy`, `www`, `Sep` de. Tık var olmayana giderse
  hata. Hızlı ama gürültülü ve 044'le çelişir.
- **B — Fare altındakini sor, cevabı hatırla.** ⌘-hover'da o noktanın
  adayları tek sorguda sorulur; vurgu cevap gelince belirir (ilk seferde bir
  ağ gecikmesi kadar), aynı dizindeki aynı ad bir daha sorulmaz. 044 ile
  aynı davranış, yalnız gecikmeli.
- **C — Ekranı topluca sor.** ⌘'ye basılınca görünür satırların bütün
  adayları tek sorguda; sonra hover anında. Ekran değişince yeniden.

## Karar 2: Uzak kabuk OSC 7 basmıyorsa göreli adlar ne olur? → ✅ A (kullanıcı)

`ls` çıktısındaki `backups` göreli; hangi dizinde olduğunu yalnız OSC 7
söylüyor ve çoğu sunucu onu varsayılan olarak basmıyor.

- **A — Göreli ad bağlantı olmaz.** Mutlak (`/var/log/x`) ve `~/…` yollar
  çalışır. ⌘ basılıyken göreli bir adın üstünde pane'in sol altındaki
  etikette (044'ün hedef etiketi) tek satır: "Remote folder unknown — enable
  OSC 7 on the server". `docs/AYARLAR.md`'ye sunucuda OSC 7'yi açan tek
  satırlık rc örneği; uzak rc'ye biz yazmıyoruz.
- **B — Ev dizinine göre dene.** Upload'un emsali. Yanlış dosya riski: hem
  `~/backups` hem `/var/www/app/backups` varsa sessizce yanlışı önizler ya
  da indirir.
- **C — Uzak kabuğun dizinini bul.** Yardımcı ssh oturumu, etkileşimli
  kabuğun dizinini `/proc` üstünden arar (yalnız Linux sunucu; ControlMaster
  çoklamasında ve `sudo -i`/tmux'ta belirsiz). Kod açar ama güvenilmez.

**Ek (kullanıcı, 2026-10-02):** gerçek sunucuda (Ubuntu, OSC 7 yok) etiket
çıktı; başlık `root@host: ~` biçimindeydi. OSC 7 yokken başlığın
`kullanıcı@host: dizin` biçimi dizin sayılır (`shell::title_directory`,
`Session::remote_link_directory`; `~` yardımcının bildiği uzak eve açılır);
biçime uymayan başlık yok sayılır ve A'nın etiketi kalır. İki noktadan
sonraki boşluk isteğe bağlı (kullanıcı, 2026-10-02: Fedora + oh-my-zsh,
`termsupport` başlığı `%n@%m:%~` — `tdgunes@tdg-fw13:~` — kuruyor ve
etiket çıkıyordu).
Durum çubuğunun yolu da aynı yedekten (046 sonrası, kullanıcı bildirdi:
Hetzner Ubuntu, `ssh kararla_hetzner`, başlık `root@kararla-production: ~`,
çubukta yalnız `⇄ kararla_hetzner`): yedek yalnız bağlantı tabanındaydı,
bağlam satırı ve göstergenin yerleşimi `remote_cwd`'yi ham okuyordu
(sınama: `the_status_bar_reads_the_remote_folder_from_the_title`). Tek
yerden (`Session::title_folder_into`) üç tüketiciye; başlığın host'u ssh
hedefiyle eşlenmiyor — hedef çoğu zaman bir takma ad ve uzak oturum
sürerken başlık zaten uzak kabuğun.

## Karar 3: ⌘-tık uzakta "önizle" demek → ✅ kullanıcı

Dosyaya ⌘-tık geçici, salt okunur bir kopyayı önizleme klasörüne indirir ve
açar (Downloads'a değil). Klasöre ⌘-tık no-op (vurgu da yok). Sınırın
(varsayılan 100 MB) üstünde önce sorar: düğme "Open Preview", sol bağlantı
"Save to Downloads instead"; metin kopyanın geçici ve salt okunur olduğunu
söyler. İnerken dock satırında, bitince kendiliğinden açılır. Uzaktaki boyut
ve tarih değişmediyse önbellekten açılır.

## Karar 4: Betik ve program önizlemesi düz metin → ✅ kullanıcı

044'ün "betik açılmaz, Finder'da gösterilir" kuralı önizlemeye taşınmıyor:
önizlemede kullanıcı **okumak** istiyor ve önbellek klasörünü göstermek
anlamsız. `public.script`/`public.executable` ve `x` bitli dosya varsayılan
düz metin uygulamasıyla açılır; çalıştırma riski yok. Bilinen içerik tipi
(044'ün `DOCUMENT_TYPES`'ı) kendi varsayılan uygulamasıyla.

## Karar 5: Kalıcı indirme iki yoldan, onay yalnız gerektiğinde → ✅ kullanıcı

⌘-sürükle (Finder, Masaüstü dahil) ve sağ tık menüsü: dosyada Open Preview ·
Download to Downloads · Download To… · Copy Path · Copy as scp Path;
klasörde Open Preview yok. Tek dosya sorusuz iner; onay sayfası yalnız
klasörde (dosya sayısı, boyut), hedefte ad çakışmasında (Keep both /
Replace; ayarla değişir) ya da yerel diskte yer yokken (düğme kapalı +
neden). Gerekçe: yerel diske indirmek sunucuya yüklemek kadar riskli değil.
İnen dosya karantina etiketi taşır.

## Karar 6: Aktarım listesi iki yönlü → ✅ kullanıcı

"Show files (N)" → "Show transfers (N)"; ok her yerde ↑ sunucuya, ↓ bu
Mac'e (satır özeti `↑1 ↓2`, başlık öneki, liste). Önizleme sırayı beklemez;
sağ tık indirmeleri ve upload'lar tek sıralı kuyruk; biten indirmede "Show in
Finder", biten önizlemede "Open". Durdurma sorusu, ⌘., bildirim ve Dock
simgesi iki yönü birlikte sayar.

## Karar 7: Finder'a bırakılan indirme sıra beklemez → ✅ kullanıcı (doğrulanacak)

Bırakma anında Finder yer tutucuyu koyuyor; sırada beklerse Masaüstü'nde %0'da
duran bir simge kalır. Bırakılan aktarım önizleme gibi hemen başlar.
**Doğrulanmadı:** Finder'ın uzun yazıma tahammülü ve simgedeki ilerleme
(`NSProgress` yayını) — gerçek pencerede denenir; tutmazsa Uygulama
Notları'na ve kullanıcıya.

## Karar 8: Ayar penceresinde "Remote Files" kategorisi → ✅ kullanıcı

Önizleme sınırı (100 MB), salt okunur, önizleme klasörü (Change… / Show in
Finder), saklama (Until next launch / 1 / 7 / 30 gün), boyut sınırı (2 GB),
kullanım + Clear Now, indirme klasörü (~/Downloads, Change…), ad çakışması
(Ask / Keep both / Replace), arka planda bitince bildirim. Hepsi
`settings.toml` `[remote]` anahtarı; pencere yalnız dosyaya yazar (029).
Sayılar tasarım sabiti başlangıç değeri, ölçülmüş değil.

## Karar 9: Temizlik bateri açıkken boyut yüzünden silmez → ✅ kullanıcı

Danışman bulgusu üstüne (okunan dosya kullanıcının altından silinmesin):

- Açılışta: saklama süresini aşanlar ve boyut sınırını aşan kısım, en eski
  önce.
- Günde bir kez: yalnız saklama süresini aşanlar.
- Clear Now: hemen.
- Çıkışta silinmez ("Until next launch" bir sonraki açılışta siler).
- Sınırdan büyük önizleme istisna gerektirmiyor: oturum boyunca durur.
- bateri'nin yazdığı hâlden (boyut/tarih) farklılaşmış önizleme hiçbir
  temizlikte silinmez: Downloads'a taşınır ve bildirilir — salt okunur bir
  ipucu, garanti değil (TextEdit'te Unlock).

**Kapsam dışı (kullanıcı, 2026-10-01):** önizlemeyi düzenleyip sunucuya geri
yükleme — ayrı set.

## Karar 10: Uzak sorgu tek uzun ömürlü yardımcı oturumdan (teknik)

Her soru için yeni ssh bağlantısı ControlMaster'sız sunucuda yüzlerce ms.
Pane başına, ilk uzak sorguda tembel açılan bir yardımcı ssh oturumu
(`upload::ssh_argv` + uzakta `sh`, satır tabanlı soru/cevap; adlar indeksle,
upload'un `probe_script` kuralı) varlık, tür, boyut, tarih ve klasör içeriği
sorularını taşır. Uzak oturum bitince (`clear_remote`'un nesli) ya da boşta
bir süre sonra kapanır (tasarım sabiti). Aktarımların kendisi upload gibi
ayrı süreçlerde kalır, çünkü iptalin tek yolu süreci öldürmek.

## Karar 11: İndirme `ssh … tar c | tar x` (teknik)

Upload'un aynası: uzakta `tar c -C dir ./ad`, yerelde `/usr/bin/tar x`;
baytlar bizden geçtiği için `TarWatcher` aynen ilerleme sayar; toplam
boyut yardımcı oturumun sorusundan. Yerelde önce gizli geçici bir ada (aynı
dizinde) yazılır, bitince tek `rename`; iptal ve hata geçiciyi siler. Tar
uzaktaki mtime'ı korur — önbelleğin "değişti mi" sorusunun yerel yarısı o.
Mosh'ta ssh seçeneği yok, `ssh host`'a düşer (upload'un emsali).

## Karar 12: Kuyruk yönden bağımsız, metin yönle (teknik)

`Uploads` → iki yönü taşıyan `Transfers` (`bt-shell-common`); iş `Job`'a yön
ve şerit (kuyruk / önizleme / Finder) eklenir, metinler (`titled`, sonuç
satırı, durdurma sorusu, liste) yönü okur. `bt-core`'un `Transfer`'ı zaten
yönsüz; yalnız `ButtonLabel::ShowFiles` metni "Show transfers". Upload'un
bugünkü davranışı bayt bayt aynı kalır (sınamaları değişmeden geçmeli).

## Karar 13: Uzak bağlantının kapısı doğrulama kaynağıyla açılır (teknik)

`link_allowed` uzakta düz metin yolu artık reddetmiyor ama hit'i **uzak**
diye işaretliyor (`LinkKind::Path`'in yanında uzaklık); `file://` uzakta
kapalı kalır (anlamı belirsiz). Doğrulama `hyperlink.rs`'te: yerel kuyruk
yerine yardımcı oturuma sorulur; `links::resolve_first`'ün enjekte edilen
`stat`'ı uzak cevaptan beslenir, göreli taban `remote_cwd`. Açma politikası
uzakta ayrı ve saf (`links`): dosya → önizleme, klasör → yok.

## Karar 14: Sürükleme eşiği jest defterinde (teknik)

`Gesture` basış konumunu tutar; bağlantı basışında eşiği aşan hareket
`Drag::Link`'e (yeni kol) döner ve view `beginDraggingSession` ile
`NSFilePromiseProvider` başlatır; eşik altı bırakma bugünkü gibi açar.
Eşik AppKit'in kendi sürükleme eşiği değil, sabit küçük bir mesafe (tasarım
sabiti). Yerel bağlantıda sürükleme bu sette **yok** (kapsam dışı; yerel
dosya zaten Finder'da).

## Karar 15: objc2-app-kit özellik bayrakları (teknik)

`NSFilePromiseProvider`, `NSDraggingSession`, `NSDraggingItem`,
`NSPasteboardItem` başlıkları mevcut `objc2-app-kit` sürümünün kapalı
özellikleri; yeni crate değil, sürüm oynamıyor. Karantina ve önbellek
`objc2-foundation`'ın `NSURL`/`NSFileManager`'ıyla (bayrak), düz metin
uygulaması `NSWorkspace.URLForApplicationToOpenContentType` ile. `Cargo.lock`
değişirse (bayrak yeni bir alt crate çekerse) phase'de durulur.

## Karar (2026-10-01, kullanıcı onayı + teknik karar)

- **Seçilen (ürün, kullanıcı):** Karar 1 → B: fare altındaki aday sorulur,
  var olan bağlantı olur; cevap (dizin, ad) başına hatırlanır — 044'ün
  "doğrulanmamış yol vurgulanmıyor" sözleşmesi uzakta da geçerli. Karar 2 →
  A: OSC 7 yokken yalnız mutlak ve `~/` yollar; göreli adda hedef etiketi
  nedeni söyler, belge sunucuda OSC 7'yi açmayı anlatır. Karar 3–9 tasarım
  tuvalindeki akışlar.
- **Seçilen (teknik):** Karar 10–15.
- **Reddedilen:** 1-A (044'le çelişir, `ls -l`'in her sütunu vurgulanır),
  1-C (ekran değişince yeniden sorma karmaşası, kazancı ilk hover'ın
  gecikmesi); 2-B (sessiz yanlış dosya), 2-C (`/proc` eşleştirmesi
  ControlMaster çoklaması, `sudo -i` ve tmux'ta belirsiz); önizlemeyi
  düzenleyip geri yükleme (ayrı set).
