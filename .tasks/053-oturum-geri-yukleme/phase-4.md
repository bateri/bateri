# Phase 4 — Ayar penceresi ve belgeler

## Özet

`restore_windows` ayar penceresinde seçilebilir olur ve davranış belgelere
girer.

_Requirements: R4.2_

## Değişiklikler

- **`crates/bt-shell-macos/src/settings_window.rs`** — Terminal
  kategorisinde `restore_windows` satırı (`ConfirmClose`'un `Choice` emsali:
  `Key`, anahtar yolu, tanı satırı).
- **`docs/AYARLAR.md`** — şablona ve `### [terminal]` bölümüne anahtar: üç
  değer, varsayılan, kullanılamayan dosyada `"layout"`, nereye yazıldığı,
  geçmişin düz metin olarak diske indiği ve Time Machine'in onu yedekleyeceği,
  uzak pane'in davranışı.
- **`CHANGELOG.md`** — kullanıcının gördüğü dille tek madde.
- **`CLAUDE.md`** — bugünkü hâl paragrafına kural + tek cümle gerekçe +
  işaretçi (`.tasks/053-oturum-geri-yukleme/discussion.md` → Karar): kayıt
  `shutdown`'da, tek kurulum yolu `TerminalWindow::restore`, geçmiş VT
  baytı olarak oynatılıyor, uzak satır çalıştırılmadan; `settings.toml`'u
  okuyan anahtar listesine `restore_windows`.

## Kabul

- `every_row_receives_its_own_diagnostic` ve ayar penceresinin mevcut
  sınamaları yeşil, yeni satır dahil.
- Pencereden yazılan değer dosyada yalnız o anahtarı değiştirir.

## Checklist

- [x] Ayar penceresi satırı
- [x] `"layout"` ve `"off"` açılışta önceki bir `"all"` kaydının geçmişini **oynatmaz**, okumadan siler (`"layout"` düzeni yine kurar): kullanıcı geçmişi istemediğini söyledikten sonra eski geçmişi bir kez daha göstermek beklenmedik (phase-3 → Uygulama Notları'ndaki davranışın düzeltmesi; orkestratör kararı 2026-10-03)
- [x] `restore_windows`'un kabul edilmeyen değeri (ör. `"Off"`) `"layout"`'a düşer, `"all"`'a değil: düzen gelir, geçmiş diske yazılmaz (phase-1 → Waive'in kararı; `osc52` emsalinin güvenli yönü). Tanı yine satırda; bekçi `Settings` sınamasında
- [x] `docs/AYARLAR.md`
- [x] `CHANGELOG.md`
- [x] `CLAUDE.md`
- [x] Doğrulama geçti (`make check` + `make smoke`)

## Uygulama Notları

- Satır General kategorisinde, `confirm_close`'un altında ("Reopen after
  quitting:"); ayar penceresinde "Terminal" kategorisi yok, `confirm_close`
  da General'da.
- `"layout"`'ın açılışı: `restore::take(lock, histories)` — `false`'ta bütün
  geçmişler okunmadan süpürülüyor ve dönen pane'lerin `history` biti
  düşüyor; bekçisi `a_layout_only_take_deletes_the_histories_unread`.
  phase-3'ün notundaki davranış bununla düzeldi.
- Yanlış `restore_windows` değeri `fallback`'i okumuyor, `osc52` emsali:
  bozuk `[terminal]` bölümü de `"layout"`. Bilinen sınır: `"off"` iken
  yanlış yazılmış kayıt da `"layout"`'a çıkar (düzen yazılır, geçmiş yine
  yazılmaz).
- Şablonun başlık yorumu istisnaları sayıyordu ve yalnız `osc52`'yi
  diyordu; `remote.integration` ile `restore_windows` eklendi (belge kopyası
  aynı commit'te).
- Set kapısı `/code-review`'ının üç bulgusu giderildi: (1) geri yüklenen uzak
  pane'de prompt'tan önce yazılan tuşlar hazır satırın arkasına yapışıyordu
  (`ssh prodgit st`) — hazır satır gönderilmemiş yazı varsa düşüyor, kullanıcının
  satırı kazanıyor (`a_ready_initial_input_gives_way_to_keys_typed_before_the_prompt`);
  (2) işaretsiz kabukta kesim imlecin satırı yerine **mantıksal** satırının
  tepesi (sarılan yarım komut da gidiyor; çok satırlı prompt'un üst satırı
  bilinen sınır, `final_history`'nin doc'unda); (3) ⌘Q anında Settings
  penceresi key iken öndeki terminal penceresi kayboluyordu — key/main bizim
  değilse z-sırasında önümüzdeki pencere (`front_terminal_window`).
  `/audit`: `make audit` temiz, mercekler temiz (6 ilgisiz).
- Gözle kontrol (gerçek pencerede kapat-aç) koşulmadı.

