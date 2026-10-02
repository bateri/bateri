# Uzak sunucunun yük göstergesi

## Hedef

ssh/mosh durum çubuğunun sağında bağlanılan Linux makinenin CPU, bellek ve
(dolunca) disk yükü — varsayılan sparkline, tıkla ayrıntı popover'ı — 045'in
yardımcı oturumundan, yeni bağlantı açmadan ve boşta kare üretmeden.

## Gereksinimler

- **R1** — Bağlam satırında yordamsal karakterler küçük sınıfın ölçüsüyle
  çizilir.
  - **R1.1** — U+2581–2588 küçük sınıfta yordamsal: yan yana bloklar
    boşluksuz döşer, komşu hücreye taşmaz, taban çizgisi küçük metninkiyle
    hizalı; aile (blok, Braille, çizgi, teknik küme) küçük sınıfta da fonta
    sorulmuyor.
  - **R1.2** — Büyük sınıfın raster'ı bit bit aynı.
  - **R1.3** — Göstergenin fonttan gelen karakterleri (`▲`, `●`) küçük
    sınıfta kutu değil (sözlük bekçisi, `UPLOAD_GLYPHS` emsali).
- **R2** — Ayarlar.
  - **R2.1** — `[remote] stats` (`sparkline`|`numbers`|`alerts`|`off`,
    varsayılan `sparkline`) ve `stats_interval` (2–60 s, varsayılan 3)
    ayrıştırılır, tanılanır, şablonda ve `docs/AYARLAR.md`'de; yazma yolu
    bilinmeyen anahtarı korur.
  - **R2.2** — Ayar penceresinin Remote Files kategorisinde iki satır; dosya
    ya da pencereden değişiklik kayıt anında uygulanır.
- **R3** — Gösterge (`bt-core`).
  - **R3.1** — Üç biçim (`sparkline`, `numbers`, `alerts`), disk yalnız %85
    üstünde; etiket, sparkline ve eşik altı sayı `dim`, eşik aşan sayı
    `warning`/`error`, kritikte `▲`.
  - **R3.2** — Düşme merdiveni: tam → sayılar → en kötü → hiç; eşik aşan en
    kötü değer yoldan önce gelir; yol soldan `…`; host asla kısalmaz.
  - **R3.3** — Aktarım sürerken gösterge çizilmez ve tıklanmaz.
  - **R3.4** — Çizim, fare isabeti ve popover çıpası tek yerleşimden.
  - **R3.5** — Değer nesil ve eşitlik kapılı; uzak durum silinince ya da
    değişince değer de silinir.
- **R4** — Veri (`bt-shell-common`).
  - **R4.1** — Yardımcı oturumda `bt_load` isteği; CPU iki örnek farkından,
    bellek, swap, disk, load, uptime; `p` ile OS, çekirdek, ilk üç süreç.
  - **R4.2** — `/proc` yoksa sessizce gösterge yok; `ps --sort` yoksa süreç
    listesi boş, örnek geçerli; bozuk cevap panik değil hata.
  - **R4.3** — Açılış hatası o nesilde örneklemeyi bitirir (parola isteyen
    sunucuda gösterge yok); açık oturumda hata göstergeyi gizler, bir kez
    yeniden dener.
  - **R4.4** — Saf zamanlama makinesi: koşma/durma koşulları, ilk örnek
    hemen, CPU için hızlı takip, uçuşta tek istek, jetonlu tik.
- **R5** — Yaşam döngüsü (`bt-shell-macos`).
  - **R5.1** — Örnekleme uzak oturumla başlar; biter, `off`, örtülme ve
    `STATS_IDLE` etkileşimsizlikte durur; geri gelince ilk örnek hemen.
  - **R5.2** — Boşta sıfır kare: kare yalnız gösterilen değer değişince;
    zamanlayıcı link'i uyandırmaz.
- **R6** — Popover.
  - **R6.1** — Göstergeye tık popover'ı açar/kapatır; içerik host + OS,
    çekirdek + CPU %, load 1/5/15, bellek, swap, disk `/`, uptime, ilk üç
    süreç; açıkken tazelenir.
  - **R6.2** — Göstergede el imleci; Esc ve dışarı tık kapatır, Esc kabuğa
    gitmez; gösterge kaybolunca popover kapanır.
- **R7** — Platformsuz kısımlar (`bt-atlas`, `bt-core`, `bt-shell-common`)
  `make linux`'ta derlenir ve sınanır.

## Yaklaşım

1. `bt-atlas`: küçük yüzden ikinci `Metrics`; yordamsal kapı küçük sınıfta
   açılır, çizim ayrı tampona, büyük yuvaya taban çizgisi hizalı taşıma.
2. `bt-core`: ayar anahtarları, `RemoteStats` + `DockContext::stats`,
   `Session::set_remote_stats`, `render_remote_context`'in yanında yerleşim
   merdiveni ve `stats_at`/`stats_span`.
3. `bt-shell-common`: `helper_script`'e `bt_load`, `remote_helper`'a
   `Query::Load`; yeni `remote_stats` modülü (ayrıştırma, fark, yuvarlama,
   geçmiş, `Schedule`).
4. `bt-shell-macos`: pane başına jetonlu zamanlayıcı, durma sinyalleri, ayar
   penceresinin iki satırı.
5. `bt-shell-macos`: tık, el imleci ve canlı popover.

Gerekçeler `discussion.md` → Karar 1–8.

## Kapsam Dışı

- Linux dışı uzak (macOS/BSD: `sysctl` + `vm_stat` kolu) — ilk sürümde
  gösterge yok.
- Parola isteyen sunucu (045'in sınırı) ve iç içe ssh'ın iç makinesi
  (algılanan hedef ölçülür).
- `/` dışındaki diskler, ağ ve GPU ölçüleri, eşiklerin ayarlanması.
- Bildirim ya da sesli uyarı; göstergenin yerel oturumda gösterilmesi.

## Akış

```
TerminalPane (ana kuyruk)
  remote edge / visibility / interaction / settings ──▶ Schedule (saf) ──▶ after(jeton)
  tik ──▶ RemoteHelper.ask(Query::Load{p}) ──(worker, aynı ssh)──▶ bt_load seq [p]
                                                    ◀── BT-R … BT-LOAD … BT-END
  cevap ──▶ remote_stats::Sampler (fark, yuvarlama, geçmiş) ──▶ RemoteStats
        ──▶ Session::set_remote_stats(nesil, değer)  [nesil + eşitlik kapısı]
                 └─ değiştiyse Waker::wake ──▶ frame() ──▶ dock::render_remote_context
                                                  └─ merdiven: ⇄ host  yol  ·· gösterge
  tık ──▶ dock::stats_at ──▶ stats_popover (p bayrağı açık, yerinde tazelenir)
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| phase-4 | |
| phase-5 | |
| kapı | |
