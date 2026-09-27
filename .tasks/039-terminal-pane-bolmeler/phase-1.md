# Phase 1 — Pane nesnesi ve oturumun çekirdeği

## Özet

`TerminalPane`'i kur, oturumun çekirdeğini `TerminalWindow`'dan ona taşı;
pencere tam bir pane tutar ve davranış bit bit aynı kalır.

_Requirements: R1.1, R1.2, R1.3, R1.4_

## Değişiklikler

- **`crates/bt-shell/src/pane.rs`** (yeni) — `define_class!` ile `NSView`
  alt sınıfı `TerminalPane` (Karar 2): bugünkü içerik kapsayıcısının yerini
  alıyor (layer-backed, `BateriView` onu autoresizing'le dolduran çocuk —
  033 R4.1'in düzeni aynen). `WindowIvars`'tan taşınanlar: `renderer`,
  `surface`, `view`, `link`, `session`, `shell_parent`, `wake`, `zoom`,
  `dock_rows`, `dock_rows_at_birth`, `tab_id`, kendi `id`'si. Taşınan
  yöntemler: `start`/`start_session`, `sync_geometry`/`refresh_geometry`,
  `alt_screen_did_change`, `change_zoom`/`apply_font`/`request_font`/
  `zoom_after_reload`, `set_theme` (kromsuz kısmı), `set_terminal_options`,
  `set_host_marks`, `set_cursor_motion`, `apply_caret`, `set_reduce_motion`,
  `set_smooth_scroll`, `keyboard_moved`, `apply_focus`, `foreground`,
  `probe_remote`, `begin_close`. `ShellWake` ve `RemoteProbe` pane'in
  yanına iner; `ShellWake::id` pane kimliği. Çerçeve bildiriminin gözlemcisi
  pane (`viewFrameDidChange:` pane'e taşınır) ve pane'in kapanışında
  sökülür — pencere kapanışını beklemez. Bu phase'de pane hâlâ
  `app::delegate`'e uzanabilir; sınır phase-2'de kapanıyor.
- **`crates/bt-shell/src/window.rs`** — `TerminalWindow` pencerenin
  delegate'i olarak kalıyor ve tek bir `Retained<TerminalPane>` tutuyor;
  `new` pane'i `contentView` yapıyor. Pencere düzeyi kalanlar: krom
  (`apply_chrome`), başlık (`refresh_title`/`apply_title`; oturumu pane'den
  okur), sekme noktası, kapatma sorusu ve grubu, `windowShouldClose:`/
  `windowWillClose:`, arama ve yükleme (bu phase'de pane'in oturumuna
  `pane()` üzerinden uzanıyor). `windowDidChangeBackingProperties:`,
  `windowDidChangeOcclusionState:`, `windowDidBecomeKey:`/`ResignKey:`
  pane'e dağıtıyor. `session()`, `link()`, `renderer()`, `view()`, `zoom()`,
  `tab_id()` erişimcileri pane'e yönlenir (çağıranlar değişmesin diye ya da
  çağıranlar `pane()`'e geçer — hangisi daha az kopya bırakıyorsa).
- **`crates/bt-shell/src/app.rs`** — `app.pane(id)`: pencerelerin pane'lerinde
  kimlikle arama; `ShellWake`'in dönüşleri, alternatif ekran habercisi ve
  `uploader::on_window` pane'i ya da pane'in penceresini buradan buluyor.
  `window_by_tab` pane'in kimliğine bakar (bu phase'de pencere başına tek
  pane, sonuç aynı). Süreli koşunun `quiet_since`/`shutdown`/`report_and_exit`
  pencerenin tek pane'inin renderer ve link'ini okur (Karar 12; doc'u
  "tek pencere, tek pane" der). `reload_settings`/`apply_appearance`/
  `apply_reduce_motion` dağıtımı pane'e varır.
- **`crates/bt-shell/src/view.rs`** — `keyboard_moved()` ve `terminal_window()`
  bu phase'de pane'i bulan bir yola geçer (`superview()` downcast'i ya da
  pane → pencere); yükleme çağrıları hâlâ pencerede.
- **`crates/bt-shell/src/lib.rs`** — `mod pane;`.

## Kabul

- `make hepsi` yeşil; mevcut sınamalar (kapatma metinleri, sekme kimliği,
  uzak yoklama) dokunulmadan geçiyor ya da yalnız taşınmış.
- `make duman`: jeton satırının değerleri öncekiyle aynı (`hucre=8 glif=6
  kural=15`, `kapanis=clean`, `pipeline=ok`).
- `make test-yaris` iki profilde yeşil (`ShellWake`'in `Waker` yuvası taşındı).
- Gözle: sekme açma/kapama, ⌘T'nin dizini, Cmd +/−/0, pencereyi başka ekrana
  taşıma, arka sekmede sıfır kare, odaksız pencerede içi boş caret — hepsi
  değişmemiş.

## Checklist

- [x] `pane.rs`: `TerminalPane` ve taşınan ivar/yöntemler
- [x] `window.rs`: tek pane'e yönlendirme, pencere bildirimlerinin dağıtımı
- [x] `app.rs`: `pane(id)`, süreli koşunun tek pane'i, ayar dağıtımı
- [x] `view.rs`: sahip araması pane'e
- [x] Test: taşınan sınamalar; `pane(id)`'nin kapanmış pane'i bulmadığı
- [x] Doğrulama geçti (`make hepsi`, `make duman`, `make test-yaris`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (bulgu yok)

## Uygulama Notları

- **Kimlik ad alanı tek**: pane kimliği pencereninkiyle aynı sayaçtan
  (`open_window` iki kez çekiyor). `ShellWake` ve alternatif ekran habercisi
  pane kimliğini taşıyor; pencereye ait işi (başlık, arama sayımı, kapanış)
  `AppDelegate::window_of_pane`'den, pane işini `AppDelegate::pane`'den
  buluyor. Kapatma sorusu, `forget_window` ve arama sürücüsü pencere
  kimliğinde kaldı.
- **"Kapanmış" pane'in bayrağı** `TerminalPane::begin_close`'ta; üç arama
  (`pane`, `window_of_pane`, `window_by_tab`) tek saf kuraldan
  (`app::find_open`, bekçisi `lookup_skips_closed_panes` — bt-shell'de AppKit
  nesnesi kuran sınama yok, kural o yüzden saf). Pencerenin eski `closed`
  bayrağı kalktı: `window_by_tab` artık pane'inkine bakıyor ve bayrak
  `windowWillClose:`'da `begin_close`'tan hemen önce kuruluyordu, yani aynı an.
- **`probe_remote` iki bit döndürüyor** (`RemoteProbeOutcome`): kararsızlık
  silahı geri kuruyor (pane), uzak durumun değişmesi pencerenin
  `refresh_title`'ını çağırıyor (sekme noktası, yükleme bağlantısı).
- **`start_session`'daki `refresh_title`** pencerenin `start`'ına taşındı
  (pane'in `start`'ı döner dönmez, aynı ana thread çağrısı — araya run loop
  turu girmiyor). Pane pencereye takılı değilse `sync_geometry` `None`
  veriyor: `start` hata, `refresh_geometry` no-op; ölçek uydurulmuyor.
- **Çerçeve gözlemcisi** pane'in, kurulumu `TerminalWindow::new`'in son adımı
  (`observe_frame`, eski sıra), sökümü `TerminalPane::begin_close`'ta;
  `windowWillClose:`'daki `removeObserver` kalktı.
- **Pencere pane'i tutuyor, pane pencereyi tutmuyor** (`contentView` zaten
  güçlü; geri referans çember olurdu): ölçek ve key biti `NSView::window`'dan.
- **Sapma — `uploader::on_window` pencere kimliğinde kaldı**: yükleme kuyruğu
  bu phase'de pencerede, anahtarı kuyruğundan önce taşımak yarım bir taşıma
  olurdu; kuyrukla birlikte phase-2'de.
- Punto seçicileri (`makeFontBigger:` …) bu phase'de pencerede ve
  `pane().change_zoom`'u çağırıyor; pane'e taşınmaları phase-2'nin (R2.1).
- Pencerede `session()`/`view()` ileticileri kaldı (arama ve yükleme bu
  phase'de pencerede ve oturuma buradan uzanıyor); `app.rs`'in ayar
  dağıtımı ve süreli koşu doğrudan `window.pane()` çağırıyor.
- Duman: öncesi ve sonrası `kare=29 hucre=8 glif=6 kural=15 yuva=13/1984
  yuva2=0/1984 yuk=smoke istek=4 icerik=2 hareket=27 kayma=0
  kapanis=clean pipeline=ok` (yalnız `sessiz` zamanlama).
- Gözle kontrol bu otonom koşuda yapılmadı (sekme açma/kapama, ⌘T dizini,
  Cmd +/−/0, ekran taşıma, arka sekme, odaksız caret) — set kapısında.

