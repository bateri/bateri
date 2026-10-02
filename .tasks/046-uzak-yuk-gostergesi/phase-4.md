# Phase 4 — Canlı gösterge: zamanlayıcı ve ayar penceresi (`bt-shell-macos`)

## Özet

Pane `Schedule`'ı ana kuyrukta jetonlu bir zamanlayıcıyla sürer, cevabı
`Session::set_remote_stats`'a yazar; ayar penceresi iki satır kazanır.
Bu phase'den sonra gösterge ekranda.

_Requirements: R2.2, R5.1, R5.2_

## Değişiklikler

- **`crates/bt-shell-macos/src/stats.rs`** (yeni) — pane'in örnekleme
  sürücüsü: `Schedule` + `Sampler`'ı tutar, `Schedule`'ın eylemlerini koşturur
  — tik `DispatchQueue::main().after` ile ve jetonla (bayat tik yok sayılır),
  istek `pane.remote_helper().ask(Query::Load { detail })` ile (`hyperlink::verify_remote`'un
  dönüş emsali: worker'dan ana kuyruğa pane kimliği + nesille, pane yoksa ya
  da nesil değiştiyse düşer), sonuç `Sampler`'dan `Session::set_remote_stats`'a;
  "gizle" `set_remote_stats(nesil, None)`. Popover'ın `Detail`'i için bir
  dinleyici kancası bırakır (phase-5 doldurur).
- **`crates/bt-shell-macos/src/pane.rs`** — ivar olarak sürücü; olaylar
  `Schedule`'a: `remote_or_title_changed` (nesil başladı/bitti — bugün yardımcı
  oturumu kapatan kenar), görünürlük (aşağıda), ayar değişimi (`set_*` yöntemi,
  `PaneLaunch`'ın anlık görüntüsünden ilk değer), etkileşim (`note_interaction`),
  `begin_close` (sürücü durur; jetonlar zaten bayatlar).
- **`crates/bt-shell-macos/src/split_view.rs`** — `apply_visibility` link'in
  yanında pane'e de görünürlüğü söyler (pane'de tek bir `set_visible` kancası;
  büyütülmüş bölmenin arkasındaki pane örtülü sayılır, aynı ifade).
- **`crates/bt-shell-macos/src/view.rs`** — `keyDown:`, fare basışı, tekerlek
  ve `mouseMoved:` pane'in `note_interaction`'ını çağırır (sayaç değil, damga;
  çağrı ucuz ve `Term` kilidine uğramaz). Pencerenin key olması da etkileşim
  (`window.rs`'in `windowDidBecomeKey:` döngüsü).
- **`crates/bt-shell-macos/src/app.rs` / `window.rs`** — ayar değişiminde
  (`Changes::stats`) bütün pane'lere yeni biçim ve aralık; süreli koşu
  (`BT_RUN_SECONDS`) dosya okumadığı için varsayılan, uzak oturum da olmadığı
  için sürücü hiç tik kurmaz.
- **`crates/bt-shell-macos/src/settings_window.rs`** — Remote Files'ta iki
  satır: "Server load" açılır menüsü (Sparkline, Numbers, Alerts only, Off;
  `Choice` deseni) ve "Sample every" sayı alanı + stepper (saniye, aralık
  `bt-core`'un sabitinden). `Key` varyantları, yerleşim yüksekliği (en uzun
  kategori Remote Files; `WINDOW` yüksekliğinin yorumu) ve
  `every_row_receives_its_own_diagnostic`'in metnine iki anahtar.
- **`CLAUDE.md`** — ssh durum çubuğu paragrafına göstergenin bir cümlesi +
  işaretçi; `remote_helper` cümlesindeki "nesil değişince ya da boşta
  kapanır" örneklemenin oturumu açık tuttuğunu söyler; ayar anahtarları
  listesine `stats`, `stats_interval`; "Boşta sıfır kare"ye dokunulmaz (yeni
  yol içerik tadında bir hasar, kural değişmiyor — gerekçe `discussion.md` →
  Karar 6).

## Kabul

- `make check`, `make smoke` (uzak oturumsuz koşu: `content=` ve `quiet=`
  önceki koşularla aynı sınıfta — sürücü tik kurmuyor) yeşil.
- Ayar penceresinin mevcut sınamaları (anahtar envanteri, şablon, kilit)
  yeni satırları kapsıyor.
- Elle: gerçek bir Linux sunucuya ssh'ta gösterge ~1 s içinde CPU'suz, bir
  saniye sonra CPU'lu; sekme değiştirince örnekleme duruyor, dönünce hemen
  yeni örnek; `stats = "off"` kaydı göstergeyi kaldırıyor; `exit` sonrası
  gösterge yok ve 120 s sonra yardımcı ssh süreci kapanmış (`ps`).

## Checklist

- [x] Sürücü (`stats.rs`): jetonlu tik, istek, dönüş, gizleme
- [x] Pane olayları: uzak kenar, görünürlük, ayar, etkileşim, kapanış
- [x] `SplitView::apply_visibility` → pane
- [x] View/pencere etkileşim kancaları
- [x] Ayar penceresinin iki satırı + canlı uygulama
- [x] `CLAUDE.md`
- [x] `docs/AYARLAR.md` → Settings…: "Remote Files `[remote]`'un sekiz önizleme/indirme anahtarını" cümlesi iki satırla on anahtara (phase-2'den devredildi: satırlar bu phase'de doğuyor)
- [x] Doğrulama geçti (`make check` + `make smoke`)

## Uygulama Notları

- **Uzak kenarın iki yolu var, ikisi de sürücüye gidiyor:** `remote_or_title_changed`
  yalnız yoklamadan (`probe_remote`) çağrılıyor; uzak oturumun `C`/`D`/`A`'da
  silinmesi başlık haberinden (`announce_title`'ın kenarı) geliyor ve oraya
  hiç uğramıyordu. İkisi tek bir `TerminalPane::remote_edge`'e (yükleme
  kuyruğunun bağlantısı + `sync_stats_generation`) bağlandı; yoksa biten ssh'ın
  nesli için tik kurulmaya devam ederdi.
- **Uçuştaki istek her zaman cevaplanıyor:** `Request` eylemi koşarken
  oturumun uzak hedefi yoksa ya da nesli `Schedule`'ınkinden farklıysa (kenar
  henüz işlenmedi) cevap kapanışı `Err` ile hemen çağrılıyor ve ana kuyruğa
  `Failed` olarak dönüyor — `Schedule`'ın tek uçuş bayrağı aksi hâlde hiç
  düşmezdi. Cevap her yolda `exec_async` ile ana kuyruğa gidiyor
  (`RemoteHelper::ask` iş parçacığını kuramazsa cevabı eşzamanlı çağırıyor).
- **`Sampler` nesille kapılı:** örnek yalnız `Schedule`'ın nesline aitse
  `take`'e giriyor (başka host'un sayaçları sonraki CPU farkının tabanı
  olurdu); `answered` her durumda çağrılıyor (worker'ı serbest bırakır).
- **Biçim değişimi hemen yeniden çiziyor (Karar 8):** `Schedule::set_form`
  koşarken yeni istek üretmiyor, yani yeni biçim bir aralık gecikirdi.
  Sürücü her örneği `Sparkline` olarak alıp (`last`, geçmiş dahil) gösterileni
  `shaped` ile türetiyor: biçim değişince son değer yeni biçimde hemen
  `set_remote_stats`'a gidiyor; öteki biçimlerde geçmiş sıfır (görünmeyen
  geçmiş değişimi kare istemesin). `bt-shell-common`'a dokunulmadı. `last`
  yeni nesilde, yeniden başlatmada (`restart`) ve `Hide`'da siliniyor: yoksa
  biçim değişimi başka host'un sayılarını ya da gizlenmiş bir değeri geri
  getirirdi (Karar 5'in sızıntısı bir kat yukarıda).
- **Popover'ın kancası** çağrılan boş bir yöntem (`stats_detail_arrived`):
  her örneğin `Detail`'i oraya iniyor; phase-5 doldurur. Popover'ın açılıp
  kapanması (`Schedule::set_detail`) phase-5'in işi.
- **`begin_close`** `stop_stats` ile nesli `None`'a çekiyor (jeton bayatlar);
  kapanan pane'i `lookup` zaten bulmuyor.
- **Görünürlük** `SplitView::apply_visibility`'de link'le aynı ifadeden
  (`window_visible && !pane.isHidden()`); pane listesi kopyalanıp dolaşılıyor
  (sürücünün eylemleri `SplitView`'ın ödüncü altında koşmasın).
- **Etkileşim kancaları:** `keyDown:`, `mouseDown:`/`rightMouseDown:`/
  `otherMouseDown:` (basış), `scrollWheel:`, `mouseMoved:` ve pencerenin
  `windowDidBecomeKey:`'i. Koşarken yalnız damga; `Vec::new()` ayırmıyor,
  `Term` kilidine uğranmıyor.
- **Ayar penceresi:** "Server load" (`Sparkline`/`Numbers`/`Alerts only`/
  `Off`) ve "Sample every" (tam sayı alanı + stepper, aralık
  `STATS_INTERVAL_RANGE`; ondalık ve aralık dışı reddediliyor —
  `parse_interval`, ayrıştırıcının kuralı). `stats = "off"` iken aralık satırı
  devre dışı (`blinks`'in emsali, bağımlı satır).
- **Pencere yüksekliği 680 → 780 tahminle**, ölçülmedi: Remote Files iki
  notlu satır kazandı (satır + not ≈ 50 pt × 2). Gözle kontrol yapılmadı;
  sığmazsa ya da boşluk fazlaysa set kapısının gözle kontrolünde düzelir.
- **Doğrulama:** `make check` ve `make smoke` (`content=2`/`3`, `quiet=1749` ms
  — önceki koşularla aynı sınıf; süreli koşuda uzak oturum yok, sürücü tik
  kurmuyor). `make linux` gerekmedi (yalnız `bt-shell-macos` ve belgeler
  değişti); `make test-race` gerekmedi: worker → ana kuyruk dönüşü mevcut
  kimlik + nesil örüntüsü, yeni paylaşılan durum yok (sürücü `RefCell`, yalnız
  ana thread). `/code-review` koşmadı: riskli phase tetikleyicisi yok (`.wgsl`
  yok, kilit dosyası değişmedi, `make test-race` tetiklenmedi).
- **Elle kabul** (gerçek Linux sunucuya ssh, sekme değişimi, `off`, `exit`
  sonrası 120 s'de yardımcı ssh'ın kapanması) bu ajan kabuğunda yapılmadı;
  set kapısının gözle kontrolüne kalıyor.
