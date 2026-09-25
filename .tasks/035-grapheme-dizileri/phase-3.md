# Phase 3 — Kümeleme: ızgara ve `bt-core`'un iki düzen yürüyüşü

## Özet

Tek küme fonksiyonu doğar; sarmalayıcının `input`'u, dock'un düzeni,
bastırmanın ızgara yürüyüşü ve tazelik kapısı onu kullanır — hepsi
varsayılan kapalı bir oturum seçeneğinin arkasında.

_Requirements: R3, R3.1, R3.2, R3.3, R3.4_

## Değişiklikler

- **`crates/bt-core/src/` (yeni modül, ör. `cluster.rs`)** — saf iki
  fonksiyon: açık küme `c` ile uzar mı, küme genişliği
  (`UnicodeWidthStr::width`; ızgara yalnız 1 → 2 genişletir, hiçbir ara
  adımda daraltmaz).
  Dört kol `discussion.md` → Muhakeme'nin ilk maddesinde; başka kol yok.
  Üstünde bir dizgiyi kümelere bölen yürüyüş (dock ve tazelik için).
  Sınama listesi scratchpad ölçümünün 17 örneği + `لا` ve `⌚︎`
  (kümelenmez) + RI tek/çift/üçlü.
- **Sarmalayıcının `input`'u** — baş hücre her çağrıda ızgaradan
  (alacritty'nin `zerowidth` dalının yöntemi: imleç − 1, `input_needs_wrap`,
  spacer'dan geri); tek durum "son çağrı `input` mıydı" biti, öteki bütün
  aktarımlar onu düşürür. Uzatmıyorsa `Term::input(c)`; uzatıyor ve genişlik
  aynıysa `push_zerowidth`; 1 → 2 ise (genişlik **her** uzamadan sonra
  sorulur, sıfır genişlikli koldan gelenler dahil — `1` + `FE0F` + `20E3`
  genişlemeyi `FE0F`'te yapar ve kısa devre edilirse hiç yapmaz) imleç baş hücreye, hücre temiz,
  `Term::input(yer tutucu geniş karakter)`, sonra yazılan geniş hücreye taban
  karakter ve `zerowidth` listesi. Yer tutucunun hücreye geçen şablonu
  (renk, bayrak) imlecin şablonu — alacritty'ninki.
- **Oturum seçeneği** — `SessionOptions`'a kümeleme bayrağı, varsayılan
  kapalı; sarmalayıcı ve aşağıdaki üç tüketici aynı değeri okur (tek
  kaynak, kopya değil).
- **`crates/bt-core/src/dock.rs`** — `layout_with` kodu noktasıyla değil
  kümeyle ilerler (bayrak açıkken): kümenin genişliği tek yerden, `place`
  kümenin baş karakterinin indeksiyle ve küme aralığıyla. `CURSOR` bir
  kümenin **içine** düşerse caret kümenin başına oturur (Karar 7). `needed_rows`,
  `grid_span` ve isabet testi aynı yürüyüşü okuduğu için bedavaya gelir —
  ayrı yürüyüş yazılmaz. `column_width`'in doc'u güncellenir.
- **`crates/bt-core/src/shell.rs`** — tazelik kapısının ayna tarafı son
  **kümenin baş** karakterini alır (bayrak açıkken); ızgara tarafı zaten
  baş hücrenin `c`'si. `DockState::last_ink`'in türetildiği yer
  (`dock.rs` ~1902) aynı kurala.

## Kabul

- Bayrak açık bir `Term`'e `🇹🇷 👍🏽 👨‍👩‍👧 ❤️ 🏳️‍🌈` basınca her biri tek geniş
  hücre + spacer, `zerowidth` kümenin kalanı; `لا`, `⌚︎`, `a🏽`, tek RI
  bugünkü hücreleri veriyor.
- `❤` son sütundayken `FE0F` → `LEADING_WIDE_CHAR_SPACER` + alt satırda
  geniş hücre; IRM açıkken sağdaki hücreler kayıyor; kaydırma bölgesinin
  dibinde bölge kayıyor (alacritty'nin kendi sınamalarıyla aynı sonuç).
- Araya `CUP` giren `👍` · `🏽` iki küme.
- `race_*`: resize ile kümeyi bölen iki okuma yarışıyor, panik yok.
- Bayrak açıkken dock'ta aile 2 sütun, `grid_span` ızgarayla eşleşiyor,
  bayrak yazılan satır tazelik kapısında "taze" (024'ün bekçisinin kümeli
  kardeşi).
- Caret kümenin ortasındayken (`CURSOR` iki RI'nin arasında) kümenin
  başında çiziliyor.
- `👍🏽` dock satırının son iki sütununa düşerken bastırılan aralık girişin
  bütün satırlarını kapsıyor — ya da ölçülen sapma Uygulama Notları'na ve
  `discussion.md` → Karar'ın bedel listesindeki 3. maddeye "bilinen sınır"
  olarak iniyor.
- Bayrak kapalıyken bütün mevcut sınamalar bit bit aynı.

## Checklist

- [x] Küme fonksiyonu + yürüyüş + sınama listesi
- [x] Sarmalayıcının `input`'u (üç kol) + bit
- [x] Oturum seçeneği, tek kaynak
- [x] Dock düzeni ve tazelik kümeli
- [x] Test: son sütun, IRM, bölge dibi, `CUP` araya girmesi, `race_*`
- [x] Test: dock sütunu, `grid_span`, tazelik, küme içi caret, sarma sınırında `👍🏽`
- [x] Test: DEC 2026 bloğunda gelen dizi zaman aşımında (`stop_sync`) da kümeleniyor — phase-2'nin sınaması yalnız kolun varlığını görüyor, `Term`'e sarmalayıcıdan gittiğini değil ← phase-2 `/code-review`
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- **Durum `DockState::cluster`'da, ayrı argüman değil**: dock'un çizimi,
  isabet testi, satır sayısı ve tazelik kapısı zaten o kaydı taşıyor;
  tarayıcı açılışta kendi kopyasına yazıyor (`Scanner::cluster`),
  `clone_from` taşıyor, `reset` dokunmuyor. `ShellLog::new`'in kopyası da
  `Session::spawn`'da aynı değerle, yani ilk aynadan önce de tek okunuş.
  `grid_span` bastırmanın `Blocks::input_cluster`'ından (ayna metniyle aynı
  kilit turunda). `SessionOptions::cluster` tek kaynak; `window.rs` `false`
  (açılış phase-5).
- **Hayalet listesinin düzeni kümesiz** (`render_with`'in ikinci
  `layout_with`'i `false`): liste yalnız glyph taşıyor, küme kurulamaz;
  farkın küme hizası R4.2 → phase-4.
- **Genişleme IRM'de önce `delete_chars(1)`**: dar baş hücrenin girişi geri
  alınmasa küme komşuları üç sütun iterdi. DECAWM kapalıyken son sütunda
  alacritty geniş karakteri yazmıyor; küme dar hücrede kalıyor (glyph
  kaybolmuyor, sütun eksik) — sınamalı.
- **`grid_span`'de imlecin içine düştüğü küme sayılmıyor**
  (`/code-review`): ZLE kümeyi bilmiyor, `👍🏽`'den sonra ← `CURSOR`'ı
  `🏽`'nin önüne koyuyor ve ızgaranın imleci kümenin baş sütununda; yarım
  küme sayılsaydı başlangıç sütunu iki sola kayardı. Bekçisi
  `a_caret_inside_a_cluster_counts_like_its_head_in_grid_span`, mutasyonla
  kırmızı gösterildi.
- **Ölçüm — `👍🏽` zsh'in sarma sınırında (bedel 3): sapma yok.** Gerçek
  zsh + sarmalayıcı, `LANG=en_US.UTF-8`, kümeleme açık; 9–12 sütunda
  `echo TOP…` çıktısından sonra `abcdef👍🏽xy` tuş tuş yazıldı. On sütunda
  `👍` son iki sütuna düşüyor (zsh'e göre `🏽` alt satırda), dokuzda
  sığmayıp iniyor; her tuştan sonra giriş ızgarada hiç görünmedi ve üstteki
  çıktı yerinde kaldı. Bekçi olarak kaldı
  (`child::tests::a_clustered_emoji_at_the_wrap_edge_stays_suppressed`,
  9 ve 10 sütun). Sınırı: `grid_span`'in küme bayrağını düşüren mutasyonda
  da yeşil (fazla sayılan satır bu senaryoda görünür bir şeyi örtmüyor;
  nedeni ayrıca ölçülmedi); o bayrağın
  bekçisi birim eşdeğerlik (`clustered_grid_span_counts_a_cluster_as_one_wide_char`
  + ızgara yarısı `a_cluster_takes_the_cells_of_one_wide_char_at_every_width`).
  Aynı bekçi tazelik kapısının içerik yarısını da görmüyor: satır `👍🏽`'de
  biterken de durup sınıyor, ama ayna tuşun cevabıyken kapı zamandan "taze"
  diyor (aynayı kümesiz okuyan mutasyonda yeşil); o yarının bekçisi birim
  sınama `the_clustered_last_ink_is_the_head_of_the_last_cluster`.
  `discussion.md`'nin bedel 3'ü "bilinen sınır"a inmedi.
- **DEC 2026 devri kapandı**: `a_timed_out_synchronized_update_is_clustered`
  `stop_sync` kolundaki sarmalayıcı çağrısının kümeleme bayrağı `false`'a
  çevrilince kırmızı (mutasyonla gösterildi).
- **`/code-review` bulgusu devredildi**: kümeli düzende isabet testinin sağ
  yarısı (`boundary`) kümenin içine düşüyor → phase-4 checklist'i (dock
  seçim uçlarının küme hizası o phase'in işi; bayrak kapalı, kullanıcıya
  görünmüyor).

