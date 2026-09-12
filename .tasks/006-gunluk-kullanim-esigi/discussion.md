# Günlük kullanım eşiği — Tartışma

## Karar 0: Kapsam — tek hamlede eşik mi, düzenli sıra mı?

Yol haritası iki seçeneği yazmıştı: (a) pano, kaydırma, ayar, bundle ayrı
setler; (b) tek hamlede eşik. Kullanıcı tercihi **(b)** — gerekçe "kullanımın
bulduğu hatalar" argümanı, bedeli setin normalden büyük olması
(`docs/YOL-HARITASI.md:33-37`).

Bu karar `/rfc 006`'da kesinleşmek üzere buraya taşındı. Kesinleşirse bu set
dört işi birlikte taşır: pano, seçim, kaydırma, bundle. Aşağıdaki kararlar
o varsayımla yazıldı; kapsam daralırsa kararlar da daralır.

## Karar 1: Seçim modeli nerede yaşar?

Üç aday var ve katman sözleşmesi ikisini zorluyor:

- **`bt-core`** — grid'i gören tek yer. Seçim "hangi hücreler" sorusuysa
  cevabı burada. Ama `bt-core` platformsuz: fare pikselini hücreye çevirme
  (hücre ölçüsü, ölçek, pencere kenar boşluğu) AppKit/Metal bilgisidir.
- **`bt-shell`** — olayı gören tek yer (`mouseDown:`, `mouseDragged:`).
  Ama grid'i görmüyor; her seçim değişiminde `bt-core`'a sormak gerekir.
- **`bt-gpu`** — pikseli gören yer, ama sözleşme açık: "Renderer'a terminal
  semantiği eklenmez" (proje.md → tuzaklar). Seçim modeli semantik taşır
  (satır sarma, geniş karakter, alternate screen) — burası **elendi**.

**Öneri: model `bt-core`'da, koordinat çevirisi `bt-shell`'de.** Fare
pikseli `bt-shell`'de hücreye iner (ölçüyü `cell_metrics`'ten alıyor), seçim
aralığı `bt-core`'da tutulur, çizim için `frame()` sınırından geçer. Deseni
hazır: tuş yolu bugün aynen böyle çalışıyor (`view.rs` olayı alır,
`keys.rs` saf çeviriyi yapar, `bt-core` yazar).

Açık alt soru: seçim alternate screen'de ve satır sarmada nasıl davranır?
Cevap `bt-core`'un grid bilgisine dayanır — panel baksın.

## Karar 2: Cmd-C / Cmd-V menüsüz nasıl çalışır?

Bugün `view.rs:61-63` Command'lı her tuşu yutuyor ve yorum "menü gelene
kadar" diyor. Menü **bu sette yok** (00X). Üç yol:

- **(a) `keyDown:`'da ele al.** Command+C/V gelirse panoya yaz/okut, yutmayı
  yalnız o iki tuşta kaldır, geri kalanı yutmaya devam. Dar, ama iki tuşa
  özel dal demek.
- **(b) `performKeyEquivalent:`'i doldur.** AppKit'in tasarlanmış yolu;
  menü gelince aynı seçiciler menüye bağlanır, bugün yazılan kod yarın
  çöpe gitmez. Biraz daha fazla iskelet.
- **(c) Menüyü de getir.** Kapsam şişer; yol haritasında menü 00X'te.

**Öneri: (b).** (a) bugünü kurtarır ama menü gününde çöpe gider; (b)'nin
iskeleti menüye devredilir. Cmd-C/V dışındaki Command tuşları yutulmaya
devam eder.

## Karar 3: Yapıştırma PTY'ye nasıl girer — bracketed paste şart mı?

Ham yapıştırma (baytları dümdüz yazmak), kabuk satırında çalışan bir
uygulamaya (vim, REPL, `read`) yapıştırınca satırları tek tek **çalıştırır**.
Bunun çaresi bracketed paste (`\e[200~` … `\e[201~`): uygulamaya "bu bir
yapıştırma" denir.

Metalterm bracketed paste destekliyor (`ARASTIRMA.md:42`). alacritty tarafı
`bt-core`'da — uygulamanın isteyip istemediğini (DECSET 2004) **o** bilir.

**Öneri: bracketed paste bu sete girer, ama yalnız `bt-core`'un bildiği
kadarıyla.** Uygulama istemişse sar, istememişse ham yaz. İkinci hâl
kullanıcının sorumluluğunda — uygulamanın istemediğini terminalin
bilemeyeceği bir şeyi terminal çözemez.

## Karar 4: Kaydırmanın kapsamı — viewport mu, scrollback mi?

İki ayrı şey: scrollback deposu (`bt-core`'da var) ve viewport'un geriye
gitmesi (yok). Tekerlek, Shift+PgUp, kaydırma çubuğu — üçü de ayrı iş.

**Öneri: tekerlek + Shift+PgUp girer, kaydırma çubuğu girmez.** Çubuk AppKit
kroniğidir (thumb boyutu, orantı, sürükleme), eşik için gerekli değil;
tekerlek ve tuş, aynı "viewport kaydır" yolunun iki tetikleyicisi.

Açık alt soru: alternate screen'de (vim, less, tmux) tekerlek ne yapar?
Doğru davranış uygulamaya fare dizisi göndermektir — ama fare raporlaması
(SGR-pixel) kendisi ayrı bir iş. Bu sette tekerlek alternate screen'de
**yoksayılır** önerisi var; panel baksın.

## Karar 5: OSC 52 (panodan okuma/yazma dizisi) girer mi?

Uzak makinedeki bir uygulama (`ssh` üstünden vim) panoya OSC 52 ile yazar.
`bt-core` OSC 7/8/9/52'yi zaten tanıyor (katman tablosu). Ama dizinin ucu
`NSPasteboard`'da — yani `bt-core` ayrıştırır, `bt-shell` yazar.

**Öneri: girer, ama yalnız yazma yönü (uygulama → pano).** Okuma yönü
(panoyu uygulamaya verme) güvenlik sorusudur: herhangi bir uzak uygulama
kullanıcının panosunu okuyabilir. Metalterm'de `clipboard.osc52` bir ayar
(`ARASTIRMA.md:103`) — ayar sistemi ise 007'de. Ayarı olmadan okuma yönünü
açmak ya hep-açık (riskli) ya hep-kapalı (işlevsiz) olur; ikisi de yanlış.
Yazma yönü risksizdir.

## Karar 6: Bundle'ın sınırı nerede?

`make kur` bugün "henüz yok". Bundle demek en az: `Info.plist`, ikon,
`BT_RUN_SECONDS`-sız normal açılış, Dock davranışı, öne çıkma hakkı.

**Öneri: en küçük çalışan bundle.** İmza ve notarization **girmez** (dağıtım
işi, eşik değil); Sparkle **girmez** (güncelleme, eşik değil); `Info.plist` +
ikon + `make kur`'un gerçek olması girer. 002'nin Apache-2.0 attribution'ı
("lisans metni ve paneli **bundle**") bu sete girer — yeri hazır, borcu
kapanır.

**Bundle'ın yan ödevi kayıtlı:** görünür pencere meşru kare sayısını
değiştirir, `IDLE_FRAME_LIMIT` bu sette **yeniden ölçülür**. Bugünkü `8`,
görünmez pencerede ölçüldü; sayısı `bt-shell`'de sabitin doc'unda duruyor.

## Karar Noktaları

Kullanıcıya sorulacak tek şey **Karar 0**: kapsam (b) olarak kesinleşsin mi?
Önerim evet; gerekçesi yol haritasında yazılı ve kullanıcı daha önce bu yönde
tercih bildirdi. Kalan altı karar teknik ve panelin işi.

## Muhakeme (2026-09-12)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | Karar 5 revize edilmeden geçemez; 1/3/4 küçük düzeltmeyle geçer; 2 ve 6 temiz |
| İşletme | ŞARTLI GEÇER |

Üçü de aynı yöne vuruyor: kapsam küçülsün, faz sırası yazılsın, kapı-kod
ayrıklığı korunsun. Hiçbiri KIRMIZI değil — yön doğru, parçalar sorunlu.

**Kabul edilen itirazlar → plan değişikliği:**
- Bundle kapsamdan çıkıyor (sadelik 1) → ayrı set olur; `IDLE_FRAME_LIMIT`
  yeniden ölçümü de onunla gider. Gerekçe: pano/seçim/kaydırma aynı yola
  dokunuyor (`view.rs` → `bt-core` → `frame()`), bundle hiçbirine değmiyor;
  eşik "kopyala/yapıştır/seç/kaydır" ile geçilir. **Karar 0 revizyonu,
  kullanıcı onayı bekliyor.**
- OSC 52 yazma yönü 007'ye erteleniyor (sadelik 3) → ayarsız açılan yazma
  yönü sonradan kapı ekletir; ayrıştırma `bt-core`'da hazır durabilir,
  `NSPasteboard` köprüsü ayar anahtarıyla gelir. **Kullanıcı onayı bekliyor.**
- Karar 2: (b) yerine (a) — `keyDown:`'da iki tuşluk dal. (b) teknik olarak
  temiz (codebase-fit doğruladı: çakışma yok) ama menü 00X'te ve çağrısı
  olmayan soyutlamaya iskelet yazılmaz (YAGNI). Menü günü (a) silinir.
- Karar 5 sınırı (codebase-fit 1): `frame()` kirli kapılıdır
  (`session.rs:594-596`), pano olayı asenkron — aradaki kanal `ShellEvent`
  kuyruğu (`mpsc::Sender`), `Adapter::send_event` içinde `ClipboardStore`
  kolu. `frame()` el değmeden kalır. (Kapsamda kalırsa; erteleme hâlinde
  yalnız ayrıştırma durur.)
- Karar 1 eksiği (codebase-fit 2): seçim değişiminde `DirtyFlag::mark()`
  (`session.rs:803`) + link uyandırma yazılmalı, yoksa seçim hiç boyanmaz.
  Vurgunun kendisi sorunsuz — imleç tersine çevirme emsali
  (`session.rs:768-773`), `cell_bg` borusu yeter.
- Karar 3/4 düzeltmesi (codebase-fit 3): DECSET 2004 tutulmaz, `Term`'den
  kilit altında sorgulanır; yapıştırma `session.write`'tan (`session.rs:811`)
  ama yeni bir `paste()` ile sarılarak; viewport için `display_offset`
  zaten var (`session.rs:603-617`), eksik yalnız ince `scroll_display` API'si.
- `IDLE_FRAME_LIMIT` ayrıklığı (işletme 2): bundle fazı sayıyı ölçüp ayrı
  commit + gerekçeyle dondurur; kapı değişikliği kod fazlarından ayrık.
  (Bundle ayrı sete çıkınca bu kendiliğinden sağlanır.)
- Faz sırası + `[elle]` göz kontrolü (işletme 1): seçim→kopyala→yapıştır→
  kaydırma, her faza göz kontrolü; seçim olmadan pano doğrulanamaz.
- İmzasız bundle notu (işletme 3): bundle ayrı sete çıkınca eşik tanımı
  "tek seferlik Gatekeeper onayı" gerçeğine göre yazılır.
- Attribution içerik denetimi (işletme 3): bundle fazına Info.plist + lisans
  dosyası varlığı konur — eksik kalırsa hiçbir kapı kızarmaz.

**Reddedilenler:**
- *"Bundle eşik için şart"* (önceki varsayım) — sadelik çürüttü: Dock
  ikonu/öne çıkma hata buldurmaz, paketler. Eşik tanımı daralıyor.
- *"OSC 52 yazma yönü risksiz, girsin"* (Karar 5 önerisi) — sadelik çürüttü:
  ayarsız açılan yön sonradan kapı ekletir, üstelik azınlık senaryo.

## Karar (2026-09-12, kullanıcı onayı)

- **Seçilen: kapsam (b) — tek hamlede eşik tutuldu.** Panel (üç koldan)
  bundle'ın çıkmasını önerdi; kullanıcı reddetti. Gerekçe: eşik tanımı
  "kopyala/yapıştır/seç/kaydır **+ açılabilir, öne çıkabilen bir uygulama**" —
  Dock ikonu ve öne çıkma olmadan günlük kullanıma geçilemez, dolayısıyla
  hata-buldurma argümanı bundle'sız işlemiyor. Bedeli kabul edildi: set
  normalden büyük olur; işletme jürisinin şartları (faz sırası, `[elle]`
  kapıları, `IDLE_FRAME_LIMIT` ölçümünün kod fazlarından ayrık tutulması)
  aynen uygulanır.
- **Seçilen: OSC 52 yazma yönü 007'ye ertelendi (panel önerisi).** Ayrıştırma
  `bt-core`'da hazır durabilir; `NSPasteboard` köprüsü ayar anahtarıyla
  gelir. Codebase-fit'in `ShellEvent` kuyruğu tasarımı park edildi —
  çöpe gitmedi, 007'de kullanılacak.
- **Seçilen: Karar 2 (a) — `keyDown:`'da iki tuşluk dal.** (b) teknik olarak
  temizdi (codebase-fit: çakışma yok) ama menü 00X'te ve çağrısı olmayan
  soyutlamaya iskelet yazılmaz. Menü günü (a) silinir.
- **Reddedilen:** *"Bundle ayrı sete çıksın"* (sadelik 1, işletme 2'nin
  varsayımı) — kullanıcı reddetti, gerekçe yukarıda. *"OSC 52 yazma yönü
  girsin"* (Karar 5 önerisi) — sadelik çürüttü, kullanıcı onayladı.
