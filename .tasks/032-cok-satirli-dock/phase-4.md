# Phase 4 — Dönüşüm: çok satır dock'ta

## Özet

`Multiline`'ı kaldır: satır sonlu görüntü ve `PREBUFFER` dock'ta çizilir,
bastırma ızgaradaki bütün giriş satırlarını kapsar, caret dock'ta kalır;
`Multiline`'ın arkasında erişilemez duran sessiz kırılmalar aynı commit'te
bekçiyle kapanır.

_Requirements: R3.1, R3.2, R3.3, R3.4, R3.5, R4.1, R4.2_

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — `DockStatus::Multiline` ve bütün kolları
  kalkar (`caret_home_raw`, tutma istisnası, eşleşmeler); `\n` kontrol
  karakteri sayılmaz, `Control` kalan kontrol karakterleri için aynen.
  `last_ink` süzgeci `\n`'i atlar. `apply_dock::End` (`e`): safha `Input`'ta
  kaldıkça son görüntü, bant ve bastırma `HANDOVER_HOLD` kadar tutulur; `u`
  gelirse yeni ayna, 133 `C` ya da süre dolunca bugünkü sıfırlama
  (`discussion.md` → Karar 11). `SuppressedInput` satır farkında girdileri
  taşır; `blank_mirror` "görüntünün hiç karakteri yok" ve `PREBUFFER`'ı
  hesaba katar (025'in "karaktersiz ayna imleci prompt'un satırından aşağı
  itemez" öncülü `PREBUFFER` varken yanlış; orada çıpa sınaması
  sorulmaz).
- **Bilinen sınır, adıyla yazılacak:** `PS2` satırında ızgara kullanıcının
  `for> ` mürekkebini taşıyor, ayna taşımıyor; `BUFFER` boşken içerik kapısı
  "bayat" der. Bugün olduğu gibi zamansal kapı (`line-init`'in `u`'su ⏎'e
  cevap) kurtarıyor; yönü güvenli (satır iki yerde görünür).
- **`crates/bt-core/src/session.rs`** — bastırmanın `to`'su imlecin altındaki
  satırları (`BUFFER`'ın imleçten sonraki satırları, sarmalarıyla) kapsar;
  `PREBUFFER` doluysa `floor` çıpanın satırı. Doluluk bastırılan satırları
  saymaz (tek yüklem, dört tüketici korunur). `can_be_typed` değişmez.
- **`crates/bt-core/src/dock.rs`** — `PREBUFFER` satırları düzenlenebilir
  satırların üstünde, aynı girintide; seçilebilir, kopyalanır; isabet
  `PREBUFFER`'a düşerse caret taşınmaz. 031'in düzenleme kapısı `PREBUFFER`'a
  değen seçimde komut göndermez (seçim kalkar, tuş bugünkü yoldan).
- **`CLAUDE.md`** — `Multiline`'ın bütün anılışları, "dock'u çok satırlı
  girişe göre büyütmek bilerek yapılmadı" paragrafı, "dock'un giriş satırı
  bir tane", `DOCK_ROWS * cell_h` ve `split_into_grid` cümlesi (PTY payı /
  çizilen bant ayrımı), saç çizgisi paragrafları (024'ün pencereleme cümlesi
  phase-3'te sarmaya çevrildi), kaymanın yön kuralına bandın istisnası, `bt-gpu` satırındaki "kaç satır
  olduğu `DOCK_ROWS`" — kural + tek cümle gerekçe + işaretçi.
- **`docs/YOL-HARITASI.md`** — borç kalemi (şimdiden sete bağlı) kapanış
  notuyla tek satır.

## Kabul

- Çevrilen bekçiler: `a_newline_anywhere_in_the_display_marks_the_mirror_multiline`
  → `Live`; `a_multiline_mirror_is_never_held` → tutma kuralı;
  `a_multiline_mirror_draws_nothing_and_keeps_the_caret_in_the_grid` →
  çok satır çiziliyor; `a_bracketed_multiline_paste_leaves_the_line_and_the_caret_in_the_grid`
  → satır ve caret dock'ta (ayna tazeyken); Control sınamasındaki "ikisi
  birden → Multiline" → `Control`.
- Yeni bekçiler: `last_ink` satır sonuyla biten yapıştırmada; `blank_mirror`
  (025'in senaryosu: sondaki satır sonunda duran caret); imlecin altındaki
  satırların bastırılması; canlı zsh'le (`Session::spawn` + sarmalayıcı)
  `for i in 1 2; do` ⏎ `echo $i` — `PREBUFFER` dock'ta, ızgarada bastırılmış,
  ⏎'de bant pompalamıyor.
- `make hepsi`, `make test-yaris`, `make duman` yeşil.
- Gözle kontrol (üç yüzey): çok satırlı yapıştırma, geçmişten `for` döngüsü,
  heredoc — dock büyür, ızgara yukarı süzülür (dolu ızgarada tepe kırpılır,
  boşken doldurma bandı kısalır), Enter'da komut ızgarada yerinde belirir,
  bant geri çekilir; vim'e girip çıkınca dock doğru boyda.

## Checklist

- [x] `Multiline` kalkıyor; `\n` → `Live`
- [x] Bastırma bütün satırlarda; `PREBUFFER` tabanı
- [x] `e` tutması (`HANDOVER_HOLD`)
- [x] `PREBUFFER` çizimi, salt okunur seçim
- [x] `last_ink`, `blank_mirror`
- [x] Bekçiler çevrildi / eklendi; canlı zsh sınaması
- [x] `CLAUDE.md`, `docs/YOL-HARITASI.md`
- [x] phase-1'den devir: imleç bir `\n`'in arkasındayken ızgara başlangıcı ve caret kuralı (phase-1 → Uygulama Notları) — bu phase'in dock tarafına etkisini uygula ya da gerekçesiyle kapat
- [x] Orkestratör (phase-3'ün waive 2'sinden, kullanıcı tarafı): tavanı aşan girişte dikey pencerenin dışındaki satırlara fare de ulaşabilsin — işaretçi dock'un üstündeyken tekerlek/trackpad dock'un dikey penceresini kaydırsın (ızgarayı değil); bırakınca caret'i izleme yeniden başlasın (yazınca/caret hareket edince pencere caret'e döner). Sürükleyerek seçim pencerenin kenarına değince pencere kaysın.
- [x] Orkestratör (phase-3'ün gözlemi): punto büyütmeden (resize) sonra ızgarada eski bir satır kalıyordu — HEAD'de (032 öncesi, 9a0dd82) de oluyor mu ölç; 032'nin getirdiğiyse düzelt, değilse Uygulama Notları'na bilinen sınır olarak yaz.
- [x] Doğrulama geçti (`make hepsi`, `make test-yaris`, `make duman`)
- [x] Riskli phase: `/code-review` koştu, iki bulgu giderildi, ikisi waive (aşağıda)

## Uygulama Notları

- **Tutma `DockStatus` değil aynanın yanında bir damga** (`ShellLog::end_since`):
  tutma boyunca ayna `Live` kalıyor, devir ise `Idle` görüyor
  (`caret_status`), yani caret'in tutması `e` anından sayıyor ve ikisi aynı
  saatte bitiyor. Süreyi kare yolu çözüyor (`expire_end`, `frame()`'in
  kilit turunun başında) ve kalan `next_tick`'e `sooner`'la giriyor;
  `Session::dock` çözmüyor, `frame()`'in kararından ayrışmasın. Her OSC 133
  işareti tutmayı bitiriyor (`C` komut koştu, `A` yeni prompt). `e`'nin
  damgası (`answers`) tutulan aynaya yazılıyor: ⏎'den sonra imleç `PS2`
  satırına iniyor ve içerik kapısı bayat derdi. Tutma sürerken bastırmanın
  tabanı çıpa (`from_anchor`), yoksa kabul edilen satır `u` gelene kadar
  ızgarada belirirdi. Tutma sürerken düzenleme kapısı kapalı
  (`dock_editable` `e`'de siliniyor).
- **Bilinen sınır:** `CORRECT`'in `[nyae]` sorusu tutma kadar (150 ms) geç
  görünüyor — `e` ⏎'in cevabı sayıldığı için bastırma o süre sürüyor.
  Gözle sınanmadı (geçici HOME'da `setopt correct` yok).
- **`last_ink` son satırın mürekkebi** (son `\n`'den sonrası), yalnız `\n`'i
  atlamak değil: kapının öteki yarısı ızgaranın **son** giriş satırını
  tarıyor ve `echo a\necho b\n` yapıştırmasında o satır boş; `'b'` deseydi
  cevapsız her karede bayat derdi. Bekçi canlı zsh'te nesli ileri alıp
  içerik kapısını tek başına soruyor.
- **`SuppressedInput`'ın sütun alanları kalktı** (`cols_before/after_cursor`,
  `DockState::cursor_col/display_cols` — tek tüketicileri boş ayna
  sorusuydu ve `\n`'i sütun sayıyorlardı); yerine `blank` (hiç karakter
  yok, `PREBUFFER` dahil) ve `from_anchor`.
- **phase-1 devri kapandı (ızgara tarafı):** imleç bir `\n`'in arkasındayken
  ilk satırın başı `0` değil `TEXT_COL` (dayatılan `PS1`'in genişliği;
  bastırma yalnız dock'lu kademede koşuyor). `PS2` satırında bu sayı
  kullanılmıyor, taban çıpa.
- **`Control` `PREBUFFER`'ı da soruyor** ve kontrol kararı çözümün sonuna
  taşındı (`PREBUFFER` son gövde); `\n` kontrol sayılmıyor.
- **Seçimin uzayı `PREBUFFER ++ BUFFER`** (`dock::selectable`, `Cow`: boş
  `PREBUFFER`'da ayırma yok). `DockPoint`, `DockSelection`, isabet ve
  kopya o uzayda; düzenleme yolları `DockEditLine::buffer_range` ile
  `BUFFER`'a iniyor ve `PREBUFFER`'a değen aralıkta `None` → tuş bugünkü
  yoldan, seçim kalkıyor. **Plandan sapma:** ⌘A R4.1'in "bütün `BUFFER`"ı
  yerine ekrandaki bütün komutu (`PREBUFFER ++ BUFFER`) seçiyor — Karar 2'nin
  "bütün döngüyü kopyalamak beklenen şey"i ve kullanıcı tarafı; adımı
  `Simple` (phase-3'ten beri `Line` mantıksal satır, çok satırda yalnız ilk
  satırı alırdı — gizli bir kusurdu).
- **Tekerlek (orkestratör maddesi):** tepe `ShellLog::dock_scroll`'da
  (seçimin gerekçesiyle aynanın yanında), `Session::dock_scroll(lines)`
  izden (`DockWindow`, artık `rows`'u da taşıyor) hesaplayıp ize hemen
  yazıyor. `BUFFER`/`PREBUFFER`/caret değişince kalkıyor; öneri değişimi
  kaldırmıyor. Taşmayan dock'ta `false` → olay ızgaranın. Caret pencerenin
  dışındaysa çizilmiyor. Sürükleme kenarı aşınca olay başına bir satır
  (periyodik zamanlayıcı yok — fare kıpırdadıkça). Yalnız giriş bloğunun
  üstündeki tekerlek dock'un; bağlam satırının üstündeki ızgaranın.
- **Resize artığı (orkestratör maddesi) 032'nin değil:** uzun, sarılan bir
  giriş yazılıyken punto büyütülünce (⌘:) sarmanın ilk satırı ızgarada
  bastırılmadan kalıyor; 9a0dd82'yi (032 öncesi) ayrı bir geçici pakette
  aynı sahnede koşunca da aynı satır kaldı. **Bilinen sınır:** zsh'in
  SIGWINCH yeniden çizimi prompt'u yeni satıra basıyor ve eski ilk satır
  bastırmanın tabanının (`floor`) üstünde kalıyor.
- **Gözle kontrol** (geçici paket, açık ve koyu tema): `for` döngüsü satır
  satır dock'ta, ızgarada iz yok, ⏎'de komut ızgarada `for>` satırlarıyla
  beliriyor; geçmişten çok satırlı komut dock'ta; heredoc'ta
  `PREBUFFER`+`BUFFER` üstüne sürükleme iki koşu, `PREBUFFER`'a tık caret'i
  oynatmıyor, `BUFFER`'a tık oynatıyor, `PREBUFFER`'a değen seçimde ⌫ seçimi
  kaldırıp bir harf siliyor, ⌘A bütün komut; tavanı aşan heredoc'ta
  tekerlek pencereyi yukarı taşıyor, işaret geri geliyor, bir harf yazınca
  caret'e dönüyor; dolu ızgarada bant büyürken tepe kırpılıyor. Görülen iki
  sapma: resize artığı (yukarıda, 032 öncesi de var) ve 16 satırlık
  yapıştırmadan sonra ızgaranın tepe satırında kalan glyph artığı (→ phase-5).
  **Ajan sapması:** 16 satırı yazdırırken computer-use aracı çok satırlı
  metni **panodan** yapıştırdı — kullanıcının panosu değişti.
- **`/code-review` bulguları:** (1) giderildi — trackpad jestinin başı ve
  sonu (`GestureBegan`/`Settle`) dock'un üstünde de ızgaranın; momentumu
  dock'ta biten ızgara kaydırması yarım satırda kalırdı. (2) giderildi —
  yapıştırmanın sarma istisnası (`can_be_typed`) tutma sürerken kapalı
  (`ShellLog::holding_end`): tutmayı yalnız kare çözüyor ve örtülmüş sekme
  kare çizmiyor. **Waive:** `PS2` satırında boş `BUFFER`'da içerik kapısı
  bayat diyor (ızgara `for> `'yi taşıyor, ayna taşımıyor) — plan bunu
  adıyla bilinen sınır yazdırıyordu (`last_ink`'in yorumunda); zamansal kapı
  kurtarıyor, kurtaramadığı an satır iki yerde görünür (güvenli yön).
  `CORRECT`'in 150 ms gecikmesi yukarıda bilinen sınır.
- **Ajan sapması:** orkestratörün kalıcı `bateri-dev.app` isteği (sabit
  kimlikli geçici paket) izin sınıflandırıcısınca reddedildi; bu phase'in
  gözle kontrolü ondan önce bitmişti, paket kurulmadı.
- **bt-shell sınama ikilisinin çöküşü (bilinen, açık):** `/code-review`
  düzeltmelerinden sonra iki `make hepsi` koşusunda `bt-shell` sınama süreci
  sinyalle düştü (SIGTRAP, SIGSEGV; ikisi de `make duman`'ın hemen
  yanında); tek başına paralel altı koşu ve tek thread bir koşu yeşil,
  ardından `make hepsi` yeşil. Diff `bt-shell`'de yalnız olay işleyicisine
  dokunuyor, sınamalara değil; HEAD'de sınanmadı.
- **Duman:** ilk koşular HEAD'de de aynı "animasyon yerleşmedi" ile
  kırmızıydı (pencere görünmüyordu — ortam); pencere açıkken yeşil.
