# Phase 4 — Kromu boya ve seti kapat

## Özet

Başlık çubuğu temanın zeminine ve açıklığına bağlanır, tema değişimi bütün
pencerelere uygulanır; yol haritası güncellenir ve set kapısı koşar.

_Requirements: R4, R5_

## Değişiklikler

- **`crates/bt-shell/src/window.rs`** — pencere kurulumunda ve tema
  değişiminde tek bir `apply_chrome(&Theme)`: `titlebarAppearsTransparent(true)`,
  `titlebarSeparatorStyle` = none, `backgroundColor` temanın `background`'ının
  **sRGB** hâlinden (`NSColor` sRGB; lineer değer `bt-gpu`'nun, buraya
  gitmez — `CLAUDE.md` → Renk uzayı), `appearance` temanın zemininin
  açıklığından (koyu → DarkAqua, açık → Aqua). Açıklık eşiği ve onu veren saf
  fonksiyon `bt-core`'un `Theme`'inde değil `bt-shell`'de; sınanıyor.
- **`crates/bt-shell/src/app.rs`** — temayı değiştiren iki yol
  (`reload_settings`, `apply_appearance`) `set_theme`'den sonra her pencereye
  `apply_chrome`. `apply_appearance` son görülen `NSApp` koyu/açık bitini
  tutar ve değişmediyse erken döner: pencere görünümünü kurmak her view'da
  `viewDidChangeEffectiveAppearance` doğuruyor ve bit sorulmasa tema
  değişimi başına N×N no-op tema seçimi olurdu. `dark_appearance`'ın doc'u
  "`NSApp`'ten okunması artık **zorunlu** — view'ın görünümü temayı
  yansıtıyor, sistemi değil" diye düzeltilir. Krom çağrısı `settings` ve
  `notices` ödünçleri bırakıldıktan sonra.
- **`docs/YOL-HARITASI.md`** — 026 satırı teslim diliyle tek satıra; bölme
  satırındaki bedel notu "026 sekmeyi pencereye koydu" diye güncel.
- **`CLAUDE.md`** — Bugünkü hâl'in sekme cümlesine krom (tek cümle +
  işaretçi).

## Kabul

- Açıklık fonksiyonunun sınaması (siyah → koyu, beyaz → açık, gömülü
  `bateri` ve `bateri-light`).
- `make duman` jetonları aynı.
- **Set kapısı** (`duzen.md` → Kalite kapısı): `/code-review` setin
  aralığına, `/audit` (bt-core ve bt-shell değişti; mercek 4 thread ve
  blokaj, mercek 5 boşta sıfır kare, mercek 7 belge), bulgular sonrası
  doğrulama yeniden.
- **Gözle kontrol sahnesi** (devir mesajına tek satır, üç yüzey: ızgara,
  dock, doldurma bandı her sekmede aynı çalışıyor):
  1. Tek sekme: çubuk yok, başlık çubuğu temanın zemininde, içerikle
     arasında çizgi yok; tema `bateri-light`'a geçince başlık çubuğu ve
     trafik ışıkları açığa dönüyor.
  2. ⌘T ile iki sekme: çubuk beliriyor (sistem animasyonu), ikinci sekme
     birincinin dizininde, dock ve bağlam satırı ikisinde de doğru; sekme
     başlıkları dizin adı, birinde vim açınca vim'in başlığı.
  3. İki sekmede tema değişimi: ikisi de aynı anda.
  4. Ctrl-Tab / Ctrl-Shift-Tab, ⇧⌘]/⇧⌘[ (Türkçe Q'da da), ⌘1, ⌘9; Ctrl-I
     zsh'te hâlâ sekme ekliyor.
  5. Sekmeyi sürükleyip sıralama ve pencereden koparma; View ▸ Show All
     Tabs.
  6. Arka plandaki sekmede `sleep 60`: öne gelince süre sayacı güncel,
     imleç kaymadan yerinde; iki pencereden birinde vim'e girip çıkmak
     yalnız o pencerenin dock'unu oynatıyor.
  7. ⌘+ yalnız o sekmede; yeni sekme büyütülmüş puntoyu devralıyor.
  8. Son sekmede `exit`: pencere kapanıyor, uygulama açık; Dock ikonu yeni
     pencere; ⌘Q üç sekmeyle beklemeden kapanıyor ve `ps`'te zsh kalmıyor.

## Checklist

- [x] `apply_chrome` kurulumda ve iki tema yolunda, bütün pencerelere
- [x] `apply_appearance` değişmeyen sistem bitinde erken dönüyor; `dark_appearance` doc'u
- [x] Test: açıklık fonksiyonu
- [x] `docs/YOL-HARITASI.md` ve `CLAUDE.md`
- [~] Doğrulama geçti (`make hepsi` yeşil; `make duman` [~] ortam (bkz.
      phase-1): aynı `MotionUnsettled`, `hareket=7`, sessiz ~920 ms; geçici
      bir satırla basılan jetonlar `hucre=8 glif=6 kural=15 kapanis=clean
      pipeline=ok`)

## Uygulama Notları

- **Görünüm değişiminin kaynağı view'dan `NSApp`'in KVO'suna taşındı**
  (planı değil tetiği değiştiriyor): pencereye `appearance` kurulunca view
  sistemin açık/koyu değişimini **hiç görmüyor** — ölçüldü, küçük bir Swift
  sınamasıyla: görünümü kurulmuş pencerenin view'ı `NSApp.appearance`
  değişiminde 0 `viewDidChangeEffectiveAppearance` aldı, KVO 2 haber verdi;
  miras alan pencerede view 1 aldı. Plan tetiğin view'da kalacağını
  varsayıyordu; kalsaydı sistemi izleyen varsayılan tema krom geldiği gün
  canlı izlemeyi bırakırdı. `AppDelegate::observe_appearance`
  (`effectiveAppearance` KVO'su, `NSKeyValueObserving` bayrağı) geldi,
  view'ın kancası ve `appearanceDidChange:` eylemi kalktı. Erken dönüş biti
  (`Ivars::appearance_dark`) planındaki gibi duruyor ama artık bir
  tasarruf: KVO görünümün adı değişince de (vurgu, kontrast) geliyor.
  Sistem görünümünün kendisi elle değiştirilmedi (kullanıcının makinesi) —
  gözle kontrolde sistemi açık/koyu arasında çevirmek bu yolu sınar.
- **Açıklık eşiği uydurulmadı**: `is_dark_background` WCAG kontrast
  oranıyla "beyaz metin mi siyah mı daha okunur" diye soruyor; eşik iki
  oranın eşitliğinden (sRGB'de #757575 ile #767676 arası).
- **Kromu tema yolundan ayrı çağırmak yerine `TerminalWindow::set_theme`
  ikisini birden yapıyor** (oturum + krom): iki tema yolundan biri kromu
  unutamasın. Yeni pencerede krom **görünmeden önce** (`open_window`'da
  tema çözümü yerleşimin önüne alındı) — sonra boyamak her ⌘T'de bir kare
  sistemin gri çubuğunu gösterirdi.
- **Gözle görülen ve düzeltilen kusur (phase-3'ten):** sekme çubuğu
  belirip kaybolunca içerik view'ı boy değiştiriyor ama pencere değişmiyor,
  yani `windowDidResize:` gelmiyor ve drawable eski boyda kalıyordu. Ölçüldü
  (ekran görüntüsünde caret sütunu): iki sekmeden tek sekmeye dönünce caret
  33 px'ten ~35 px'e gerildi, dock'un saç çizgisi yarım piksele yayıldı
  (`515152` → `1a1a1a`+`3f3f40`) ve dock bandı 8 px yukarı kaydı. Geometri
  artık içerik view'ının `NSViewFrameDidChangeNotification`'ından
  (`viewFrameDidChange:`); `windowDidResize:` kalktı, çünkü o bildirim
  pencere boyutlandırmasını da kapsıyor. Düzeltmeden sonra üç hâlde (iki
  sekme ön, iki sekme arka, tekrar tek sekme) caret ve saç çizgisi piksel
  piksel aynı.
- **Kendi başlattığım pencerede görülen (ekran görüntüsü, `HOME` ayrı bir
  dizinde, temayı o dizindeki `settings.toml` değiştirdi):** tek sekmede
  başlık çubuğu ile içerik aynı piksel (koyu: `000000`/`000000`; açık:
  `f7f8f9`/`f7f8f9` — ekran görüntüsünün renk profili `f5f6f8`'i böyle
  okuyor, iki bölge aynı), ayırıcı çizgi yok, açık temada trafik ışıkları
  ve başlık metni açık görünüme döndü. İki sekmede çubuk temanın zemininde
  duruyor ama **çubuğun kendi hapı sistemin rengi** (koyuda `212121`, açıkta
  `dbdbdb`/`dfe0e1`) — Seçenek C'nin kayıtlı eksisi, dikiş değil ama
  temanın saf siyahı da değil. Tema değişimi arka sekmeye de indi (sekmeye
  dönünce `000000`). Sekme başlıkları kabuğun OSC 2'sinden
  (`user@host:/tam/yol`) geldiği için uzun ve kırpılıyor — başlık kuralı
  phase-2'nin, oh-my-zsh'in başlığı kazanıyor; dizin yedeği yalnız OSC 2
  basmayan kabukta görünüyor.
- **Set kapısı `/code-review` (7 bulgu):** dördü düzeltildi — kapanan
  pencerenin çerçeve gözlemcisi `windowWillClose:`'da sökülüyor (kalsaydı
  kapanan oturum resize alırdı); krom aynı zeminde yeniden boyanmıyor
  (`WindowIvars::chrome`); sRGB baytları `Theme::background_srgb`'den (ikinci
  bir hex açma kuralı yok); `lib.rs` başlığı KVO'ya göre. phase-2'nin başlık
  sınaması uykularla yarışıyordu, adımları artık `read` + `session.write`
  ile ilerliyor. İkisi waive — aşağıda.
- `make hepsi` kapı sonrası bir koşuda `bt-shell` lib sınamalarında bilinen
  SIGSEGV ile düştü (phase-3 notu); ardından `bt-shell` üç koşuda ve
  `make hepsi` yeşil. `make test-yaris` yeşil (başlık sınaması değişti).

## Waive

- **Her pencere `Renderer::system_default()` ile Metal kurulumunu baştan
  ödüyor** (device, metallib, dört pipeline, kuyruk): paylaşımın doğru sınırı
  (atlas pencere başına, geri kalanı ortak) `bt-gpu`'nun API'sini değiştirir
  ve R1.4 "`bt-gpu`'nun API'si değişmez" diyor; Karar 2a bilinçli olarak
  bütün renderer'ı pencereye koydu. ⌘T gecikmesi ölçülmedi — ölçülürse ayrı
  bir iş.
- **`begin_close`'un `Option<Closing>` + `Closing::AlreadyDone` iç içeliği**:
  bir sadeleştirme önerisi, davranış kusuru değil; iki "yok" hâli rapor
  jetonunda zaten ayrışıyor (`teardown_token`).

## Set kapısı `/audit`

Mekanik denetim temiz (yalnız Cargo.toml uyarısı: iki bayrak, kaydı
`crates/bt-shell/Cargo.toml` yorumunda, `Cargo.lock` oynamadı). Mercek 4
temiz. Mercek 5 temiz. Gözlemi (örtülü doğan pencerede görünürlük
tohumlanmıyor, tabandan beri) denendi ve **geri alındı**: `start_session`
anında `occlusionState` henüz `Visible` değil (`makeKeyAndOrderFront`'tan
hemen sonra bile), tohum kapıyı kapattı ve duman `kare=0` ile düştü — sonraki
örtülme bildirimi gelmedi. Tohumun doğru anı ilk örtülme bildirimi; bugünkü
yol zaten o. Bedeli adıyla: gizli uygulamada açılan pencere ilk örtülme
bildirimine kadar görünmezken hasar karesi çizebilir. Mercek 7: `CLAUDE.md`'deki değişiklik anlatısı sözleşme
diline, yol haritasındaki durum sözcüğü çıkarıldı, `shutdown`'ın doc'u
etkileşimli çıkışa göre. Mercek 1, 3 temiz; 2 ve 6 ilgisiz.
