# Sekmeler — Tartışma

Karar-listesi biçimi: bir mimari soru (Karar 1) ve ona bağlı, birbirinden
ayrı çözülebilen ürün/teknik kararlar. "Temiz tasarım"ın beş ölçütü
`context.md` → Motivasyon'da; seçenekler onlara karşı tartılıyor.

## Karar 1: Sekme nerede yaşıyor

### Seçenek A — macOS'un kendi sekmeleri (native `NSWindow` tabbing)

Her sekme bir `NSWindow` ve kendi `Session` + `DisplayLink` + yüzeyi. Aynı
`tabbingIdentifier`'ı taşıyan pencereleri AppKit tek pencerede sekme olarak
gösteriyor. Bedava gelenler: sekme çubuğu, `+` düğmesi (`newWindowForTab:`),
sürükleyip sıralama, sekmeyi pencereden koparma, Merge All Windows, Move Tab
to New Window, Show All Tabs (⇧⌘\), sistemin açma/kapama/sıralama
animasyonları, VoiceOver, "Prefer tabs" sistem ayarı ve tam ekranda
sekmeler. Kodda: tabbing'i kapatan satır açılıyor, pencere kurulumu bir
fonksiyona iniyor, tekil `Ivars` alanları pencere başına bir nesneye
taşınıyor.

**Artıları:**
- **"Bir pencere = bir oturum" varsayımı bozulmuyor** — kayıtlı bedel
  ödenmiyor, çünkü sekme gerçekten bir pencere. Dock, blok şeridi, doldurma
  bandı, öteleme, fare eşlemesi ve kare yolu **el değmeden** kalıyor.
- Arka plandaki sekme zaten sıfır kare: seçili olmayan sekmenin penceresi
  görünür değil ve `windowDidChangeOcclusionState:` → `set_visible(false)`
  yolu bugün var (doğrulaması phase'te, gözle).
- Odak da bedava: `windowDidBecomeKey/ResignKey` pencere başına geliyor.

**Eksileri:**
- Sekme çubuğunun rengi ve biçimi **sistemin**; temanın zemin rengini alıp
  almadığı AppKit'e bağlı ve genel API yok. Referansın #24'ü tam bu şikâyet.
- Sekme başlığı `NSWindow.title` ile sınırlı; sekmenin içine özel gösterge
  (koşan komut noktası, zil) koymak `NSWindowTab.accessoryView` gibi dar
  bir kapıdan geçer.
- AppKit kararları: kapanan sekmeden sonra hangisinin seçileceği, çubuğun
  ne zaman göründüğü — bizim sınanabilir kodumuz değil.

### Seçenek B — tek pencere, kendi çizdiğimiz sekme çubuğu

Tek `NSWindow`, içinde N oturum; çubuk Metal'de (referansın `chrome` +
`ui_text` yolu, `context.md` → Motivasyon'daki çıkarım) ya da AppKit'te
çizilen bir `NSView` (iTerm2 emsali).

**Artıları:**
- Çubuğun her pikseli bizim: temanın zemini, dokuz rolü, materyal yüzey
  geldiğinde onun grain/sheen'i. Referansa görsel sadakat en yüksek bu yolda.
- Sekmenin içine istediğimiz gösterge.

**Eksileri** (pahalı, imkânsız değil):
- **Kayıtlı bedel ödeniyor**: `Ivars`'ın tekil alanları N'e çıkarken
  ızgara, dock ve doldurma bandının `setViewport` aritmetiğine (`bt_gpu::dock_px`,
  `Frame::fill_origin_px`) bir de çubuk payı giriyor; fare eşlemesi
  (`point_to_cell`, `Origin`) çubuğun altından başlamalı. Metal yolunda
  **beşinci bir yüzey** kare yoluna ekleniyor — pahalı karar sınıfının "her
  karede CPU hesabı" satırı.
- Bedava gelenlerin hepsi yeniden yazılıyor ya da kayboluyor: sürükleyip
  sıralama, pencereden koparma, birleştirme, Show All Tabs, VoiceOver,
  sistem animasyonları. Animasyonları kendimiz yazarsak her biri bir durma
  koşulu ve Hareketi Azalt indirgemesi ister.
- Referansın kendi çubuğu da temaya uymuyor (#24): kendi çizmek temizliği
  **kendiliğinden** getirmiyor, yalnız mümkün kılıyor.

### Seçenek C — A + temaya boyanmış pencere kromu (hibrit)

A'nın bütün mekanizması, üstüne pencerenin kendi yüzeyleri temaya bağlanır:
`titlebarAppearsTransparent`, `titlebarSeparatorStyle = none`, pencere
`backgroundColor`'ı temanın `background`'ı, pencerenin `appearance`'ı
(Aqua / DarkAqua) temanın zemininin açıklığından. Sonuç: tek sekmede başlık
çubuğu ile içerik **tek yüzey** (dikiş yok), çok sekmede sistemin çubuğu
temanın açık/koyuluğunda.

**Artıları:** A'nın bütün artıları + ölçüt 1, 2 ve 3 karşılanıyor. Kod farkı
küçük ve tek yerde (pencere kurulumu + tema değişiminin fan-out'u).

**Eksileri:** çok sekmede çubuğun **kendi** rengi hâlâ sistemin (koyu
temada sistemin koyu grisi, temanın saf siyahı değil). Bu, ölçüt 2'nin
yalnız tek sekmede tam karşılandığı anlamına geliyor; çubuğun zemin rengini
alıp almadığı deneysel ve phase'in kabulünde **gözle** ölçülüyor, iddia
edilmiyor.

→ ✅ **C** (aşağıda `## Karar`).

## Karar 2: Sekmeler renderer'ı paylaşır mı

Bugün tek `Rc<Renderer>` var ve paylaşıma hazır görünüyor (`&self` API,
`surface()` her çağrıda yeni layer). Ama atlasın anahtarı ölçek ve punto
içeriyor ve anahtar değişince atlas baştan kuruluyor (`context.md` →
Mevcut Durum). Paylaşılan tek renderer iki şeyi kırar: Retina ekrandaki
pencere ile harici 1x ekrandaki pencere atlası birbirine çevirir (her
geometri olayında bütün glyph'ler yeniden rasterize, ve arada öbür ölçeğin
glyph'leriyle çizilen bir kare), ve sekme başına punto (Karar 3) mümkün
olmaz.

- **(a) Sekme başına `Renderer`** — `Renderer::system_default()` pencere
  kurulumunda çağrılır; `bt-gpu` el değmez. Bedeli adıyla: her sekme
  metallib'i yükler, dört pipeline'ı kurar ve kendi atlasını (kendi glyph
  rasterizasyonunu, kendi dokusunu) taşır. Ölçülmedi; `docs/OLCUMLER.md`'nin
  açılış ve bellek sütunları `/measure`'la sorulabilir.
- **(b) Tek renderer, atlas haritası** — device, kuyruk ve pipeline'lar
  paylaşılır, atlaslar (aile, punto, ölçek) anahtarlı bir haritada. Bellek
  ve açılış ucuzlar, ama `bt-gpu`'nun atlas sahipliği (`RefCell<Option<AtlasTexture>>`)
  ve encode yolu değişir.
- **(c) Tek renderer, tek atlas** — punto uygulama genelinde, farklı
  ölçekli ekranlardaki pencereler kusurlu.

→ ✅ **(a)**. (c) bir kusuru ürün kararı diye yazmak olurdu; (b) (a)'nın
ölçülmüş bir bedeli çıkarsa açılacak bir optimizasyon ve (a)'dan (b)'ye geçiş
`bt-shell`'e dokunmuyor.

## Karar 3: Cmd +/−/0 sekme başına mı, uygulama genelinde mi

Terminal.app, iTerm2 ve Ghostty'de **sekme başına**: sunum için büyütülen
sekme ötekileri etkilemez. Yeni sekme etkin sekmenin geçici farkını
**devralır** (Ghostty'nin `window-inherit-font-size` varsayılanı) — büyütüp
yeni sekme açan kullanıcı küçük yazıya düşmemeli. Ayar dosyasındaki `size`
değişince her sekmenin farkı bugünkü kuralla sıfırlanır (`Zoom::after_reload`).

→ ✅ Sekme başına, yeni sekme devralır.

## Karar 4: Yeni sekme/pencere kabuğu hangi dizinde açar

Etkin sekmenin **OSC 7 dizininde** (Terminal.app'in sekme varsayılanı,
Ghostty'nin `window-inherit-working-directory`'si); dizin bilinmiyorsa
(entegrasyon `off`, zsh olmayan kabuk, kabuk henüz ilk prompt'unu basmamış)
bugünkü kural: ev dizini (`child::working_directory`). Aynı kural ⌘N'e de
uygulanıyor — iki kural ("sekme devralır, pencere evden") kullanıcının
ezberleyeceği bir ayrım ve bugün ayarla açılabilecek bir seçim değil.
Dock'tan ilk açılış ev dizininde.

→ ✅ Etkin sekmenin dizini, yoksa ev.

## Karar 5: Kabuk çıkınca ne kapanır, son pencere kapanınca ne olur

- Kabuk çıkınca **o sekme** kapanır; son sekmesiyse pencere (native'de
  aynı şey).
- Son pencere kapanınca uygulama **açık kalır** — macOS'un çok pencereli
  uygulama geleneği ve Terminal.app, iTerm2, Ghostty'nin varsayılanı. Dock
  ikonuna tıklamak yeni pencere açar (`applicationShouldHandleReopen:`),
  ⌘N menüden çalışır. Bugünkü "son kabuk çıkınca uygulama biter" tek pencere
  varsayımının bir sonucuydu.
- **Tek istisna süreli koşu**: duman reçetesi `BT_RUN_SECONDS`'tan kısa
  bittiğinde rapor `ChildExit` → `terminate:` yolundan basılıyor
  (`will_terminate`'in doc'u). Orada `terminate:` ve "son pencerede çık"
  aynen kalıyor.
- ⌘Q her oturumu **paralel** kapatır: toplam bekleme N × `SHUTDOWN_GRACE`
  değil bir `SHUTDOWN_GRACE`. Tek sekmeyi kapatmak ana thread'i **hiç**
  beklemez — kapanış arkada biter.
- **Kapatma onayı yok**, bugünkü ⌘Q kararıyla aynı
  (`.tasks/007-ayarlar-ve-tema/discussion.md` → Kapsam dışı); ⌘W'nin koşan
  komutu sormadan kapatması bilinen bedel. Referansın never/running/always
  ayarı ayrı bir set.

→ ✅ Yukarıdaki dört madde.

## Karar 6: Kısayollar ve yolları

Hepsi **menü öğesi**; `keyDown:`'ın Cmd izin listesi (⌘⌫ ⌘← ⌘→) el değmez —
menü `performKeyEquivalent:` ile önce yakalıyor, liste kapalı kalıyor.

| Kısayol | İş | Menü |
|---|---|---|
| ⌘N | Yeni pencere | Shell ▸ New Window |
| ⌘T | Yeni sekme | Shell ▸ New Tab |
| ⌘W | Sekmeyi kapat (son sekmede pencere) | Shell ▸ Close Tab (`performClose:`) |
| ⇧⌘W | Pencereyi bütün sekmeleriyle kapat | Shell ▸ Close Window |
| ⇧⌘] / ⇧⌘[ | Sonraki / önceki sekme | Window ▸ Show Next / Previous Tab |
| ⌃⇥ / ⌃⇧⇥ | Sonraki / önceki sekme | Window ▸ (AppKit'in eklediği ya da bizim) |
| ⌘1…⌘8 | n. sekme (yoksa no-op) | Window ▸ Select Tab ▸ |
| ⌘9 | Son sekme | Window ▸ Select Tab ▸ Last Tab |
| ⇧⌘\ | Bütün sekmeler | AppKit'in (View ▸ Show All Tabs) |
| — | Show Tab Bar, Move Tab to New Window, Merge All Windows | AppKit'in |

- Menü adı **Shell** (Terminal.app ve iTerm2'nin adı); Window menüsü
  `setWindowsMenu` ile kaydedilir, AppKit tabbing öğelerini oraya ve View'a
  kendisi ekler.
- **⌃⇥ bir menü kısayolu olarak bağlanır**: `performKeyEquivalent:` ana
  menüye yalnız Command'lı değil değiştiricili her tuşta sorar (Safari'nin
  ⌃⇥'i düz bir menü öğesi). Phase bunu **doğrular**; menü yakalamıyorsa
  yedek, `keyDown:`'ın Control kolunun başında tek bir dal (Ctrl+Tab →
  sekme geçişi) ve gerekçesi Uygulama Notları'na. Bedel: Ctrl+Tab artık
  terminale gitmiyor — Terminal.app ve iTerm2 de öyle.
- ⌘1…⌘9 **Select Tab ▸** alt menüsünde: Window menüsü kalabalıklaşmaz,
  kısayollar yine etkin.
- Türkçe Q düzeninde `[`/`]` Option katmanında; ⇧⌘]/[ AppKit'in kısayol
  yerelleştirmesine kalıyor ve gözle kontrolde sınanıyor. ⌃⇥ ve ⌘1…9
  düzenden bağımsız.

→ ✅ Tablo.

## Karar 7: Sekme başlığı

Öncelik: **uygulamanın OSC 0/2 başlığı** (vim, ssh, Claude Code ve
oh-my-zsh'in `termsupport`'u — sonuncusu komut koşarken komutun adını
basıyor) → **çalışma dizininin son bileşeni** (ev dizini `~`) → `bateri`.
Başlık pencerenin başlığı, yani native sekme de tek sekmede başlık çubuğu
da onu gösteriyor.

Koşan komutun adını **kendimiz** göstermek betiğin `preexec`'inin komut
metnini göndermesini ister: shell betiği pahalı karar sınıfında ve üç
kabuk kuralına bağlı (`proje.md` → Jüri mercek notları). Bilinçli olarak
dışarıda; OSC 0/2 onu basan kurulumlarda bu boşluğu zaten kapatıyor. Koşan
komut noktası, zil göstergesi ve arka plandaki sekmede etkinlik işareti de
dışarıda (Karar 1'in A eksisi: gösterge dar bir kapıdan geçiyor ve kendi
tasarım işi).

→ ✅ OSC 0/2 → dizin → `bateri`.

## Karar 8: Animasyon

Sekme açma, kapama, sıralama ve koparma sistemin animasyonuyla geliyor.
**bateri hiçbir animasyon eklemiyor:**

- Sekme geçişinde içerik **crossfade yok** — Terminal.app, Safari ve
  Finder'da geçiş anlık ve kullanıcının gözü hangi sekmeye geçtiğini zaten
  çubuktan okuyor; crossfade her geçişte iki yüzeyi aynı anda çizmeyi ve
  yeni bir durma koşulunu isterdi.
- Öne gelen sekmede **animasyon tekrarı yok**: gizlenirken uçuştaki kayma
  hedefinde bitiriliyor (`DisplayLink::set_visible`), dönüşte tek kare ve
  imleç yerinde. Yeni sekmenin ilk karesi de bir yerden kaymıyor.
- Hareketi Azalt açıkken sistemin kendi animasyonları sistemin kararına
  kalıyor.

→ ✅ Ek animasyon yok.

## Karar 9: Süreli koşu ve jetonlar

Süreli koşu **tek pencereyle** kalıyor; ⌘N/⌘T/⌘W hiçbir koşuda
tetiklenmiyor, yani `hucre=8 glif=6 kural=15` ve `IDLE_FRAME_LIMIT`
dokunulmadan. `kapanis=` jetonu çok oturumlu kapanışta **en ağır** sonucu
**değişmiyor**: süreli koşu tek pencerenin `shutdown()` sonucunu bugünkü
gibi basıyor. Etkileşimli ⌘Q'nun paralel kapanışı sonuç **toplamıyor** —
`will_terminate` sonucu yalnız süreli koşuda okuyor, yani birleştirme kuralı
hiç ikinci girdi görmeyecek bir fonksiyon olurdu (Muhakeme). Duman bekçisi
yalnız süreli koşuda ve kapsamı değişmiyor.

→ ✅ Tek pencere, jeton aynı, toplama yok.

## Karar 10: Kapsam dışı

- **Bölme** — kendi satırı (`docs/YOL-HARITASI.md`).
- **Pasif sekmenin drawable'larını bırakmak** (referansın satır 36'sı) —
  görünmeyen sekme zaten kare çizmiyor; drawable havuzunun bellek bedeli
  ölçülmedi ve ölçülmeden yapılan bir bellek optimizasyonu iddiasız bir
  değişiklik olurdu. `/measure` bedeli gösterirse ayrı iş.
- **Pencere/sekme geri yükleme** (referansın "pencere geri yükleme"si),
  **komut paleti**nin sekme listesi, **sekme yeniden adlandırma**, **kapatma
  onayı**.
- **Çubuğun kendi rengini boyamak** — genel API yok; Karar 1'in C eksisi.

## Muhakeme (2026-09-23)

| Mercek | Verdict |
|---|---|
| Sadelik | TEMİZ |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üç mercek de yönü (C, sekme başına `Renderer`, katman yönü) doğruladı;
itirazlar doğrulukla ve phase düzeniyle ilgili. KIRMIZI yok, tek tur.

**Kabul edilen itirazlar → plan değişikliği:**
- **"En ağır `Teardown`" ölü kod** (Sadelik, İşletme): `will_terminate`
  sonucu yalnız süreli koşuda okuyor (`app.rs` → `will_terminate`) ve süreli
  koşu tek pencere → R2.5 kalktı; Karar 9 daraldı. ⌘Q paralel kalıyor,
  sonucu atıyor.
- **Kapanan pencerenin `Waker`'ı** (Codebase-fit, İşletme): teardown thread'i
  `drop(tail)`'i `done.send`'den önce yapıyor (`session.rs` → `shutdown`),
  son `ShellWake` orada düşerse `Waker`'ın `MainThreadBound`'u ana kuyruğa
  senkron iş atıyor (`link.rs` → `Waker`); bugün tek koruma `Ivars`'ın
  `app.run()`'ı aşması. ⌘Q önceden kapatılmış bir oturumu beklerse sahte bir
  `SHUTDOWN_GRACE` beklemesi doğar. → Pencere kapanırken `Waker` ana
  thread'de `ShellWake`'ten **sökülüyor** (`Mutex<Option<Waker>>`,
  yaprak kilit); `ShellWake` hangi thread'de düşerse düşsün `wake.rs`
  sözleşmesine yapısal olarak uyuyor, pencere nesnesi kapanışta **hemen**
  düşüyor (mezarlık yok) ve ⌘Q yalnız listedeki pencereleri bekliyor.
  `Wake`'in "kilit almaz" cümlesi yaprak istisnasıyla güncelleniyor
  (`Theme`'in yaprak kilidi emsali).
- **Süreli koşuda kabuk erken çıkarsa rapor boş listeyle koşardı** (İşletme):
  `report_and_exit` sayaçları pencere nesnesinden okuyacak → süreli koşuda
  `child_exit` doğrudan `terminate:` çağırıyor ve pencere rapordan önce
  listeden çıkmıyor; `applicationShouldTerminateAfterLastWindowClosed:` →
  `run.is_some()`. İkisi sınama ya da doc ile çivileniyor.
- **Hedefsiz eylemlerin yönü** (Codebase-fit): yayılan eylemler
  (`settingsDidChange:`, `appearanceDidChange:`, `selectTheme:`) `AppDelegate`'te
  kalıyor ve pencere nesnesi bu seçicileri **uygulamıyor**; pencereye ait
  eylemler (Bigger/Smaller/Actual Size, Select Tab) pencere delegate'ine
  iniyor — yoksa sekme başına punto yanlış pencereye iner.
  `altScreenDidChange:` responder zincirini **bırakıyor**: haberci pencere
  kimliğini yakalıyor ve `exec_async` içinde listeden buluyor — arka
  sekmede vim'den çıkış key pencereyi boyutlandırmasın.
- **Görünüm zincirinin yeniden ateşlemesi** (Codebase-fit): pencereye tema
  görünümü kurmak her view'da `viewDidChangeEffectiveAppearance` →
  `appearanceDidChange:` doğuruyor, yani tema değişimi başına N×N no-op tema
  seçimi. → `apply_appearance` son görülen `NSApp` koyu/açık bitini tutup
  değişmediyse erken dönüyor; `dark_appearance`'ın doc'u `NSApp`
  okumasının artık **zorunlu** olduğunu söylüyor.
- **Phase düzeni** (Sadelik: beş fazla; İşletme: `bt-core` yarısı ayrı ve
  riskli) → dört phase: 1 pencereyi nesneye çıkar; 2 `bt-core` yarısı
  (kapanışın bölünmesi, dizin okuyucusu, başlık yuvası) + tek pencerede
  başlık; 3 çok pencere + sekmeler + kapanış + menüler; 4 krom + set kapısı.
  Riskli işaretler: phase-2 ve phase-3 `make test-yaris`.
- **Belge kendi commit'inde** (İşletme): her phase yanlışladığı sözleşme
  cümlesini (`CLAUDE.md` katman tablosu ve Kapanış, `lib.rs` başlığı)
  kendi commit'inde düzeltiyor; `CLAUDE.md`'ye kural + tek cümle + işaretçi.
- **Gözle kontrol somut** (İşletme): arka plan sekmesinde `sleep 60` (süre
  sayacı saat ister) + örtülme olayının geldiğini gösteren geçici bir log;
  iki pencerede tema değişimi; birinde vim; ⌘W'den sonra o sekmenin zsh'i
  `ps`'te yok.
- **Başlık haberinin iki kaynağı** (Sadelik, Codebase-fit): OSC 0/2
  (`Adapter`) **ve** OSC 7 tarayıcısı (`TappedPty`); haber yük taşımıyor,
  kuyrukta en çok bir iş (`PendingCopy` örüntüsü), alıcı `Session::title()`'ı
  yeniden okuyor.

**Reddedilenler:**
- **Üç phase'e indirmek** (Sadelik) — `bt-core`'un thread/kanal işi kendi
  `test-yaris`'li commit'ini hak ediyor (İşletme); dört, iki itirazın ortası.
- **Başlıkta dizin yedeğini kaldırmak** (Sadelik'in işaretlediği ürün
  sorusu) — kalkarsa oh-my-zsh'siz kullanıcı her sekmede `bateri` görür;
  bariz beklenti dizin adı, yedek kalıyor.

## Karar (2026-09-23, otonom akış)

- **Seçilen:** C — macOS'un kendi sekmeleri, her sekme kendi `NSWindow` +
  `Session` + `DisplayLink` + `Renderer`'ı; başlık çubuğu saydam, ayırıcısız
  ve temanın zemininde, pencerenin görünümü temanın açıklığından. Kayıtlı
  bedel ("bir pencere = bir oturum") ödenmiyor çünkü sekme gerçekten bir
  pencere; sistemin sürükleme, koparma, birleştirme, VoiceOver ve
  animasyonları bedava; tek sekmede çubuk yok ve başlık ile içerik tek yüzey.
  Karar 2–9 yukarıdaki okları ve Muhakeme'nin kabul ettikleriyle.
- **Reddedilen:** A (yalın native) — sistemin gri başlığı ile temanın zemini
  arasındaki dikiş kalıyor, C'nin farkı birkaç satır. B (kendi çubuğumuz) —
  kayıtlı bedeli (ızgara/dock/doldurma `setViewport` aritmetiği, fare
  eşlemesi, beşinci yüzey) ve bedava gelenlerin yeniden yazımını ödüyor;
  referansın kendi çizdiği çubuğun da temaya uymadığı şikâyeti (#24) temizliği
  kendiliğinden getirmediğini gösteriyor. Çubuğun rengi bir gün şart olursa B
  ayrı bir set; C'nin pencere başına nesnesi ona engel değil.
