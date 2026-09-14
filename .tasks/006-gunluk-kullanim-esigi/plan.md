# Günlük kullanım eşiği

## Hedef

bateri'yi günlük kullanılabilir terminal yapmak: kopyala/yapıştır/seç/kaydır
çalışsın, uygulama açılabilir bir `.app` olsun. Eşik tanımı: metin
kopyalanıp yapıştırılabiliyor, geçmişe kaydırılabiliyor ve uygulama Dock'tan
açılıp öne çıkabiliyor.

## Gereksinimler

- **R1** — Fareyle metin seçilebiliyor
  - **R1.1** — Seçim modeli `bt-core`'da (grid'i gören yer); piksel→hücre
    çevirisi `bt-shell`'de (`cell_metrics` ölçüsüyle)
  - **R1.2** — Seçim değişimi `DirtyFlag::mark()` + link uyandırma yapıyor,
    yoksa seçim hiç boyanmaz (`session.rs:803`)
  - **R1.3** — Vurgu mevcut `cell_bg` borusundan geçiyor; yeni
    shader/uniform yok (emsal: imleç tersine çevirme, `session.rs:768-773`)
- **R2** — Cmd-C kopyalar, Cmd-V yapıştırır
  - **R2.1** — `keyDown:`'da iki tuşluk dal (Karar 2 (a)); diğer Command
    tuşları yutulmaya devam eder, menü günü (00X) dal silinir
  - **R2.2** — Kopya metin `bt-core`'dan (`selection_text()`), panoya yazan
    `bt-shell` (`NSPasteboard`)
  - **R2.3** — Yapıştırma `session.write` yolundan ama yeni bir `paste()`
    ile sarılarak; DECSET 2004 tutulmaz, `Term`'den kilit altında sorgulanır,
    set ise `\e[200~…\e[201~` sarılır
- **R3** — Tekerlek ve Shift+PgUp geçmişe kaydırır
  - **R3.1** — İnce `scroll_display` API'si (`display_offset` zaten
    `frame()`'de tüketiliyor, `session.rs:603-617`); kaydırınca kirli bayrağı
    dikilir
  - **R3.2** — Tetikleyiciler: tekerlek + Shift+PgUp; kaydırma çubuğu yok
  - **R3.3** — Alternate screen'de tekerlek yoksayılır; karar `bt-core`'da
    `Term` kipine bakılarak verilir (katman korunur)
- **R4** — En küçük çalışan `.app` bundle
  - **R4.1** — `Info.plist` + ikon + `make kur` gerçek olur
  - **R4.2** — 002'nin Apache-2.0 attribution'ı kapanır (lisans metni +
    panel; eksik kalırsa hiçbir kapı kızarmaz, o yüzden bundle fazına içerik
    denetimi konur)
  - **R4.3** — İmza, notarization, Sparkle girmez; Gatekeeper tek seferlik
    onayı eşik tanımına yazılır
- **R5** — `IDLE_FRAME_LIMIT` bundle'lı pencerede yeniden ölçülür
  - **R5.1** — Ölçüm ayrı commit + gerekçeyle dondurulur; kapı değişikliği
    kod fazlarından ayrıktır (aynı sette kodla kapı birlikte inerse
    regresyon maskelenir — 005 phase-3'te `2`→`8` bunun uyarısı)
- **R6** — Her phase tek başına doğrulanabilir ve göz kontrolü taşır
  - **R6.1** — Faz sırası: seçim → kopyala → yapıştır → kaydırma → bundle;
    seçim olmadan pano doğrulanamaz, o yüzden sıra bağlayıcıdır
  - **R6.2** — Her faza `[elle]` göz kontrolü (pano/seçim/kaydırma
    `make duman` reçetesinde yok)

## Yaklaşım

1. **Phase-1 `bt-core` + `bt-shell` + `bt-gpu`** — seçim: `bt-core`'da aralık
   modeli + metin çıkarımı, değişimde kirli bayrağı + uyandırma, `bt-shell`'de
   fare→hücre çevirisi, vurgu `cell_bg` borusundan.
2. **Phase-2 `bt-shell` + `bt-core`** — pano: `keyDown:`'da Cmd-C/V dalı,
   `NSPasteboard` köprüsü, `paste()` + 2004 sorgulu bracketed sarma.
3. **Phase-3 `bt-core` + `bt-shell`** — kaydırma: `scroll_display` API'si,
   tekerlek + Shift+PgUp, alternate screen'de `bt-core` kipiyle yoksayma.
4. **Phase-4 `bateri` + `Makefile` + `assets/`** — bundle: `Info.plist`,
   ikon, gerçek `make kur`, attribution içerik denetimiyle.
5. **Phase-5 `bt-shell`** — `IDLE_FRAME_LIMIT` yeniden ölçümü: görünür
   pencerede ölç, ayrı commit + gerekçeyle dondur. Kod değişmez; bu phase
   belge + sayı fazıdır.

## Kapsam Dışı

OSC 52 `NSPasteboard` köprüsü (ayrıştırma `bt-core`'da durabilir, köprü ayar
anahtarıyla 007'de); kaydırma çubuğu; menü (00X) — Karar 2 (a) menü günü
silinir; imza, notarization, Sparkle; sekme/bölme (009); fare raporlaması
(SGR-pixel); `TERM`/terminfo değişikliği.

## Göç

`make kur` ilk kez gerçek oluyor: `.app` paketi `target/` altına kurulur.
Mevcut `cargo run` akışı aynen çalışmaya devam eder; `BT_RUN_SECONDS` yolu
bundle'lı açılışta da aynen işler. Kullanıcının makinesinde taşınması gereken
ayar/tema yok.

## Akış

```
Fare (AppKit, ana thread)
  ├─ mouseDown/Drag → hücreye çevir (bt-shell, cell_metrics ölçüsüyle)
  │     → bt-core seçim aralığı → DirtyFlag::mark + uyandır
  │     → frame() → cell_bg vurgusu (yeni shader yok)
  ├─ tekerlek / Shift+PgUp → bt-core scroll_display (+ kirli)
  │     → alternate screen ise Term kipine bak, yoksay
  └─ Cmd-C → core.selection_text() → NSPasteboard
     Cmd-V → oku → paste() → [2004 setse sar] → session.write → PTY

Bundle: bateri (bin) + Info.plist + ikon → make kur → target/*.app
Kapı : IDLE_FRAME_LIMIT görünür pencerede yeniden ölçülür (phase-5, ayrı commit)
```

## Durum

| Phase | Durum | Commit |
|-------|-------|--------|
| phase-1 | ✅ | 77b4afd | `[elle]` göz kontrolü tamam (2026-09-14, `097f155` üstünde) |
| phase-2 | ✅ | 97b9c2c | Doğrulama tamam: `make hepsi` 0 · `make test-yaris` 0 · `make duman` yeşil (Xcode lisansı + Metal Toolchain kullanıcı tarafından çözüldü, `phase-2.md` → "Doğrulama Durumu"); `[elle]` tamam (2026-09-14) |
| phase-3 | ✅ | 6a7a92d | `[elle]` göz kontrolü bekliyor; WAIVE önerisi: DECSET 1007 (`phase-3.md`) |
| phase-4 | | |
| phase-5 | | |
