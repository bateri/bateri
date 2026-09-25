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

- [ ] Küme fonksiyonu + yürüyüş + sınama listesi
- [ ] Sarmalayıcının `input`'u (üç kol) + bit
- [ ] Oturum seçeneği, tek kaynak
- [ ] Dock düzeni ve tazelik kümeli
- [ ] Test: son sütun, IRM, bölge dibi, `CUP` araya girmesi, `race_*`
- [ ] Test: dock sütunu, `grid_span`, tazelik, küme içi caret, sarma sınırında `👍🏽`
- [ ] Test: DEC 2026 bloğunda gelen dizi zaman aşımında (`stop_sync`) da kümeleniyor — phase-2'nin sınaması yalnız kolun varlığını görüyor, `Term`'e sarmalayıcıdan gittiğini değil ← phase-2 `/code-review`
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
