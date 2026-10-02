# Phase 2 — Göstergenin modeli, ayarları ve çizimi (`bt-core`)

## Özet

`[remote] stats`/`stats_interval` ayarları, sınırı geçen `RemoteStats` değeri
ve bağlam satırının uzak biçiminde göstergenin yerleşim merdiveni; değeri
henüz yazan yok, yani ekranda değişiklik yok.

_Requirements: R2.1, R3.1, R3.2, R3.3, R3.4, R3.5, R7_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — `[remote]`'a iki anahtar (`discussion.md`
  → Karar 8): `stats` (`RemoteStatsMode`: `Sparkline` varsayılan, `Numbers`,
  `Alerts`, `Off`; `NAMES` deseni) ve `stats_interval` (saniye, `STATS_INTERVAL_RANGE`
  2..=60, varsayılan 3 — tasarım sabitleri, doc'unda gerekçe). Yerleri
  `RemoteFiles`'ın yanında kendi yapıları (`RemoteStatsSettings`); okuma,
  tanı (kabul edilmeyen değer kendi anahtarını değiştirmez), şablon satırları
  (yorumlu, İngilizce), `SettingsEdit` varyantları + yazma yolu, `Changes`'te
  bir alan (`stats`; `remote` alanına katılmaz — desen listesini her oturuma
  yeniden yollamasın). Bilinmeyen anahtarı koruyan round-trip sınaması.
- **`crates/bt-core/src/shell.rs`** —
  - `RemoteStats` (`Copy`, `Eq`, sabit boyut): `mode` (çizilen biçim, `Off`
    yok — `Off`'ta değer `None`), `cpu: Option<u8>` (ilk örnekte henüz yok),
    `mem: u8`, `disk: u8`, `history: [u8; 8]` + `len: u8` (seviye 0–7, yalnız
    `Sparkline`'da dolu).
  - `DockContext::stats: Option<RemoteStats>` ve `clone_from`'a satırı.
  - Uzak hedefin silindiği ya da başka hedefe geçtiği her kolda (`ShellLog::apply`'ın
    `C`/`D`/`A` kolları, `ShellLog::set_remote`'un değişim kolu) `stats = None`
    (Karar 5). Okuyucu thread'inin yolu: `make test-race` satırı.
- **`crates/bt-core/src/session.rs`** — `Session::set_remote_stats(command,
  Option<&RemoteStats>) -> bool`: yaprak kilitte `running_command()` ve uzak
  hedef `command`'la tutmuyorsa no-op; değer aynıysa no-op; değiştiyse yazar,
  kilidi bırakır, `request_frame` (`set_transfer`'in örüntüsü, Karar 5).
- **`crates/bt-core/src/dock.rs`** —
  - `STATS_THRESHOLDS` (cpu 70/90, mem 80/92, disk 85/95; tasarım sabiti)
    ve önem sınıflaması (`StatsLevel`: normal/uyarı/kritik) — `pub`, popover'ın
    çubuk rengi aynı kaynaktan okur.
  - `STATS_GLYPHS = ['▲', '●']` (sparkline blokları yordamsal, listede değil)
    ve atlasın elle kopyasını bağlayan bekçi (`UPLOAD_GLYPHS` emsali).
  - Yerleşim: `stats_layout(host, remote_cwd, stats, available)` — Karar 4'ün
    merdiveni (basamaklar, tam yol + 2 sütun, sağa yaslı; alarmda en kötü
    yoldan önce; host kısalmaz). Sonuç: göstergenin hangi basamağı, yolun
    bütçesi, göstergenin bağlam-yerel sütun aralığı. `render_remote_context`
    yolun bütçesini buradan alır ve göstergeyi `emit_context_at` ile sağa basar
    (renkler Karar 4; boşluk hücre üretmez, sparkline'ın eksik solu boşluk).
  - `pub fn stats_at(context: &DockContext, budget: u16, col: u16) -> bool`
    ve `pub fn stats_span(…) -> Option<(u16, u16)>` — aynı yerleşimden; aktarım
    satırı varken ya da gösterge çizilmediyse `false`/`None` (R3.3). İmzanın
    ayrıntısı kodda seçilir; ölçüt `transfer_button_at`/`transfer_button_span`'in
    ikizliği.
  - `lib.rs` yeni `pub` adları yeniden ihraç eder.
- **`docs/AYARLAR.md`** — `[remote]` bölümüne iki anahtar (değerler, varsayılan,
  aralık, ne zaman uygulandığı) ve şablon; Settings… bölümündeki "Remote Files
  `[remote]`'un sekiz…" cümlesi on anahtara.

## Kabul

- Yerleşim sınamaları, hepsi `render`'ın hücrelerinden okunarak: üç biçimin
  metni ve renkleri; disk %84'te yok, %85'te var; kritik sayıda `▲`;
  `alerts`'te eşik yokken yalnız `●` (`success`); sparkline'ın 8'den az
  örneği sağa yaslı ve sabit genişlikte; daralan bütçede sırayla tam →
  sayılar → en kötü → yok ve her basamakta yol tam; alarmlı en kötü değerle
  yol `…`'la kısalıyor ama gösterge duruyor; `⇄ host` sığmazken yalnız `⇄`;
  host hiçbir genişlikte kısalmıyor; aktarım varken gösterge yok ve
  `stats_at` hep `false`; `stats_span` çizilen hücrelerle aynı aralık.
- `set_remote_stats`: yanlış nesil yazmıyor, aynı değer kare istemiyor,
  `C`/`D`/`A` ve hedef değişimi değeri siliyor.
- `make check`, `make linux` ve `make test-race` yeşil.

## Checklist

- [ ] Ayar anahtarları, şablon, `SettingsEdit`, `Changes::stats`
- [ ] `RemoteStats`, `DockContext::stats`, silinme kolları
- [ ] `Session::set_remote_stats`
- [ ] Eşikler, `STATS_GLYPHS` + bekçi, yerleşim merdiveni, render, `stats_at`/`stats_span`
- [ ] Test: yerleşim ve renk senaryoları (Kabul'deki liste)
- [ ] Test: nesil ve eşitlik kapısı, silinme
- [ ] Test: ayar round-trip, kabul edilmeyen değer
- [ ] `docs/AYARLAR.md`
- [ ] Doğrulama geçti (`make check` + `make linux` + `make test-race`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
