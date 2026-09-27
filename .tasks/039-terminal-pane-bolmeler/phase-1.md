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

- [ ] `pane.rs`: `TerminalPane` ve taşınan ivar/yöntemler
- [ ] `window.rs`: tek pane'e yönlendirme, pencere bildirimlerinin dağıtımı
- [ ] `app.rs`: `pane(id)`, süreli koşunun tek pane'i, ayar dağıtımı
- [ ] `view.rs`: sahip araması pane'e
- [ ] Test: taşınan sınamalar; `pane(id)`'nin kapanmış pane'i bulmadığı
- [ ] Doğrulama geçti (`make hepsi`, `make duman`, `make test-yaris`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
