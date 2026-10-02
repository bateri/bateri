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

- [ ] Sürücü (`stats.rs`): jetonlu tik, istek, dönüş, gizleme
- [ ] Pane olayları: uzak kenar, görünürlük, ayar, etkileşim, kapanış
- [ ] `SplitView::apply_visibility` → pane
- [ ] View/pencere etkileşim kancaları
- [ ] Ayar penceresinin iki satırı + canlı uygulama
- [ ] `CLAUDE.md`
- [ ] Doğrulama geçti (`make check` + `make smoke`)
