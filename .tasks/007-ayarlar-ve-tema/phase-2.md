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

- **tema / materyal biçimi** — ilk tema biçimi: kökte `background`,
  `foreground`, `dim`, `accent`; `[ansi]`'de `black`…`white`,
  `bright_black`…`bright_white`; renk `"#rrggbb"`, her anahtar opsiyonel,
  eksiği gömülü `bateri`'den. Gömülü tek tema `bateri`; kullanıcı dizini
  `~/.config/bateri/themes/`. Geriye dönük okunacak eski biçim yok.
- **ayar şeması** — `[appearance] theme`, varsayılan `"bateri"` (phase-3
  `"system"` yapar); `docs/AYARLAR.md` → `[appearance]` ve Temalar.
- **ölçüm bekliyor:** `frame()`'e giren tema kilidinin ve link'in dolu
  karede `session.theme()` ile aldığı ikinci kilidin kare süresine etkisi
  (`cpu` aralıkları).
- `CLAUDE.md`: bugünkü hâl, "Renk uzayı" (`MTLClearColor` temadan) ve "Tema
  = sekiz rol" maddeleri; `bt-core` `lib.rs` başlık yorumu (`Theme`).

## Checklist

- [x] Palet ve çizim yolu sönük renk bekçileri (önce, yeşil)
- [x] `Theme`, `Theme::BATERI`, sabitlerin kalkması, yorumlar
- [x] Tema ayrıştırıcı (taban üstüne)
- [x] `Adapter` yaprak kilidi, `frame()` sırası, `dim` rolü dalı, renk sorgusu
- [x] `link.rs` ve bt-gpu sınamaları temadan
- [x] `bt-shell` ad çözümü ve tema yuvası
- [x] Test: ayrıştırma, ad çözümü, renk sorgusu, `AYARLAR.md` bloğu
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (paylaşılan durum)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

- **Bekçiler eski kodda yeşildi**, sonra aynı literal değerlerle yeşil
  kaldı. Sönük bekçisi mutasyonla sınandı (`DIM = 0.5` → kırmızı); var olan
  `dim_flag_darkens_background_in_inverse_video` da literal değere bağlandı.
- **`Theme` alanları `pub u32`** (`0xRRGGBB`), lineer karşılıkları
  `background_linear`/`accent_linear` `const fn`; `default(index)`
  crate-içi (alacritty `Rgb` döner). `renderer.rs`'in beklenen baytları
  artık elle değil `Theme::BATERI`'nin alanlarından. Gömülü temalar
  `Theme::embedded(ad)`.
- **`Session::set_theme` yazılmadı:** tüketicisi yok; kilit takasa hazır,
  yazan ilk yol phase-3'ün görünüm değişimi.
- **Sönük kural tek yardımcıda** (`color::resolve_fg`), normal ve ters videolu
  dal ondan geçiyor. **Bit bit aynı olmayan tek köşe:** OSC 10 ile değişmiş
  ön plan + SGR 2 artık tablonun renginin `× 2/3`'ü değil `dim` rolü
  (alacritty uygulamasının `DimForeground`'ı da böyle).
- **Tema adının biçimi `bt-core`'da sınanıyor:** boş, `/` ya da NUL içeren ad
  `"bateri"` + tanı (`"../settings"` ayar dosyasını tema diye okuturdu).
- **Kullanıcı tema dosyası var ama kullanılamıyorsa gömülüye düşülmüyor:**
  okunamayan ya da ayrıştırılamayan `themes/bateri.toml` gömülü `bateri`'yi
  sessizce açmıyor; açılışta `bateri` + dosyayı söyleyen tanı. Gömülüye
  yalnız dosya **yoksa** bakılıyor.
- **Ortak kod:** `Diagnostic` ve TOML yardımcıları (`document`, `section`,
  `line_of`, `kind`) `bt-core`'da iki ayrıştırıcıya ortak; `Theme::parse`
  `(Theme, Vec<Diagnostic>)` döner. `bt-shell`'de dosya okuma tek kapı
  (`read_text`), tema çözümü `ThemeLoaded::{Found, Failed}` + `at_launch`.
- **Renk sorgusu sınaması PTY yolundan:** `EventLoopSender` dışarıdan
  kurulamıyor (alanları private). Çocuk `stty -icanon -echo` + `od -N` ile
  yanıtı döküyor; mutasyonla sınandı (yanıt `Theme::BATERI`'den → kırmızı).
  Ek yarış sınaması `race_color_request_and_frame`.
- **Duman:** ilk koşu `glif=7` (006 phase-4c'de kayıtlı gürültü); HEAD ve
  sonraki üç koşu `kare=1 hucre=8 glif=6 kural=15`.
- **Göz kontrolü geçici `HOME`'la yapıldı:** `theme = "paper"` + kısmi tema
  dosyası açılışta açık zemin ve kırmızı imleçle açıldı, bozuk `ansi.red`
  alt başlıkta dosya adıyla göründü; ayarsız ev dizininde ekran bugünkü koyu
  tema, alt başlık boş.
- **`/code-review` (high) kararları.** Düzeltilen: yarış sınamasının arka
  plan `cat`'i stdin olarak `/dev/null` alıyordu (etkileşimsiz kabuk), yanıtlar
  hiç emilmiyordu → `cat </dev/tty`, PTY'de denendi; kilitlenmede sınamanın
  düşmeyip asılı kaldığını söyleyen yorum düzeldi. Kilit sırası, sönük kural,
  renk sorusu yanıtları, ad biçimi ve süreli koşunun teması temiz bulundu.
  **Not (bulgu değil):** `dim`'i yazmayan kullanıcı teması `bateri`'nin
  `#909093`'ünü alıyor, kendi ön planının sönüğünü değil — Karar 3 (A), belgede
  yazılı.
