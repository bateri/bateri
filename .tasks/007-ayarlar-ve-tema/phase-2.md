# Phase 2 — Tema altyapısı

## Özet

Paleti `const`'lardan `Theme` tipine taşı: tek kaynaktan okunan, `Adapter`'da
yaprak kilit altında duran, kullanıcı tema dosyasından kısmen ezilebilen tema.
Ekran bit bit aynı kalır.

_Requirements: R3, R3.1, R3.2, R3.3, R3.4, R2, R10_

## Değişiklikler

Sıra commit'in içinde de önemli: **bekçiler önce**, bugünkü kodda yeşil.

- **Bekçiler (önce):**
  - Palet bekçisi: bugünkü 19 değer (16 ANSI, zemin, ön plan, imleç)
    **sabit** bir beklenen listeye bağlanır — tablonun kendisinden
    hesaplanmaz.
  - Sönük renk bekçisi **çizim yolundan**: `frame()` üzerinden SGR 2'li
    varsayılan ön plan, SGR 2'li kırmızı ve ters videolu sönük hücrenin
    çıkan renkleri sabit değerlere bağlanır. Bugünkü `session.rs:2088`
    beklenen değeri `color::dim` ile kendisi hesaplıyor, `color.rs:331`
    çizim yolunda okunmayan bir girdiye bakıyor — ikisi de kural değişimini
    görmez.
- **`crates/bt-core/src/color.rs`** — `Theme`: `background`, `foreground`,
  `dim`, `accent`, 16 ANSI. `pub` yüzünde alacritty tipi yok (`0xRRGGBB`
  ya da `LinearRgba`). `Theme::BATERI` `const` (bugünkü değerler; `dim` =
  bugünkü `FG × 2/3` sonucu). `default(index)` temanın yöntemi olur.
  `DEFAULT_BG`/`DEFAULT_CURSOR`/`BG_RGB` kalkar. `color.rs:52-53` ve
  `:172-175` yorumları yeni sahibe göre düzelir (sRGB tablosunun `const`
  gerekçesi `Theme::BATERI` ile yaşar).
- **`crates/bt-core/src/theme.rs` (yeni)** — tema metni → `Theme` + tanılar,
  **tabanın üstüne**: eksik anahtar tabandan, bozuk renk (`#rrggbb` değil) →
  tabandaki değer + tanı. Ayrıştırılamayan metin → ayrı sonuç.
- **`crates/bt-core/src/session.rs`** —
  - `SessionOptions` temayı alır.
  - Tema `Adapter`'ın paylaşılan gövdesinde, `size`'ın yanında **yaprak
    kilit** altında: tutulurken başka kilit alınmaz (doc cümlesi).
  - `frame()` temanın paylaşılan kopyasını `Term` kilidinden **önce** alır;
    zemin atlaması (`:895`, `:999`) ve renk çözümü ondan.
  - **`dim` rolü:** ön plan `Named(Foreground)` ve sönükse, çözümden **önce**
    `dim` rolü — normal ve ters videolu dal (`:890-893`, `:934-937`) ikisi de.
  - Renk sorgusu (`ColorRequest`) temadan; `:473-478` yorumu sınırın gerçek
    kökünü (alacritty'nin kilit altındaki `Colors` tablosu) söyleyecek
    şekilde yönlenir.
  - `Session::theme()` — link'in okuyacağı paylaşılan kopya.
  - `default_background_has_one_source` yeni sahibine taşınır.
- **`crates/bt-core/src/lib.rs`** — `Theme` dışa açılır, sabitler düşer;
  başlık listesi.
- **`crates/bt-gpu/src/link.rs`** — `:358`, `:371` sabit yerine
  `session.theme()`'den imleç ve clear rengi. **`frame.rs:323`**, renderer
  sınamaları `Theme::BATERI`'den; `renderer.rs:1206-1210`'daki "clear rengi
  hücre renginden farklı ve ara ton" özelliği korunur.
- **`crates/bt-shell`** — `[appearance] theme = "{ad}"` (varsayılan
  `"bateri"`; `"system"` phase-3'te). Ad çözümü: `themes/{ad}.toml` → gömülü
  → açılışta `bateri` + görünür hata. **Tema yuvası** alt başlığa eklenir
  (tema dosyası hatası, bulunamayan tema). Hermetik dalda `Theme::BATERI`.
- **`docs/AYARLAR.md`** — tema biçimi, tema dizini, `[appearance] theme`,
  `bateri`'nin tam TOML bloğu ("kopyala, değiştir"). Blok bir sınamayla
  `Theme::BATERI`'ye bağlanır: belge değer kopyalayınca drift eder.
- **`CLAUDE.md`** — "Tema = sekiz rol" maddesine 007'deki dört rol ve dosya
  biçimi; "Renk uzayı" maddesinde `MTLClearColor`'ın kaynağı artık tema.

## Kabul

- Bekçiler bu phase'in kodundan **önce** yeşil, sonra da aynı değerlerle
  yeşil.
- Tema ayrıştırma: kısmi tema tabanla dolar; bozuk hex tanı + taban;
  ayrıştırılamayan metin.
- Ad çözümü geçici dizinde: kullanıcı teması gömülüyü gölgeler; olmayan ad
  → `bateri` + tanı.
- Renk sorgusu yanıtı varsayılan olmayan bir temadan döner (`TestWake` /
  PTY yanıtı üzerinden).
- `make test-yaris` iki profilde yeşil (okuyucu thread yeni kilide dokunuyor).
- `make duman` jetonları değişmez; göz: ekran bugünküyle aynı, bir kullanıcı
  temasıyla açılış renkleri değiştirir.

## Yayın Etkisi

- **tema / materyal biçimi** — ilk tema biçimi; gömülü tek tema `bateri`.
- **ayar şeması** — `[appearance] theme`.
- **ölçüm bekliyor:** `frame()`'e giren tema kilidinin kare süresine etkisi
  (`cpu` aralıkları).
- `CLAUDE.md` iki madde; `bt-core` `lib.rs` başlık yorumu.

## Checklist

- [ ] Palet ve çizim yolu sönük renk bekçileri (önce, yeşil)
- [ ] `Theme`, `Theme::BATERI`, sabitlerin kalkması, yorumlar
- [ ] Tema ayrıştırıcı (taban üstüne)
- [ ] `Adapter` yaprak kilidi, `frame()` sırası, `dim` rolü dalı, renk sorgusu
- [ ] `link.rs` ve bt-gpu sınamaları temadan
- [ ] `bt-shell` ad çözümü ve tema yuvası
- [ ] Test: ayrıştırma, ad çözümü, renk sorgusu, `AYARLAR.md` bloğu
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi (paylaşılan durum)
- [ ] Yayın etkisi yazıldı
