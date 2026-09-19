# İmleç ayarları

## Hedef

İmlecin görünüşünü ve blink ritmini belirleyen sayılar **ayar dosyasından**
değiştirilebilsin: zevk sayısı için derleme döngüsü yanlış araç.

## Gereksinimler

- **R1 — Üç anahtar, `[terminal]` bölümünde.**

  | anahtar | tür | varsayılan | aralık |
  |---|---|---|---|
  | `cursor_radius` | ondalık | `0.10` | `0.0..=0.5` |
  | `cursor_glow` | ondalık | `1.0` | `0.0..=3.0` |
  | `cursor_blink_interval` | ondalık, saniye | `0.5` | `0.05..=5.0` |

  - **R1.1** — `cursor_glow` bir **çarpan**: `pay = gutter_px * 0.4 * glow`,
    `alfa = 0.10 * glow`. Bugünkü iki sabit **taban olarak yerinde kalıyor**,
    yani "ikinci bir tasarım sabiti yok" kuralı korunuyor; `1.0` bugünkü
    görüntü, `0.0` gölgesiz.
  - **R1.2** — Birim **oran**: yarıçap hücre yüksekliğinin, hale sol payın.
    Piksel verilseydi Cmd +/− ile imleç orantısız kalırdı.
  - **R1.3** — Kabul edilmeyen değer **kendi anahtarını değiştirmez** ve tanı
    bırakır (`Settings::parse_keeping`). **Kırpma yok**: `1.5` yazan kullanıcı
    sessizce `1.0` almamalı.
  - **R1.4** — Bölüm `[terminal]` ve gerekçesi **yeni**: bölüm kullanıcının
    neyi ayarladığını adlandırıyor, hangi struct'ın taşıdığını değil.
    `CaretShape`'in doc'undaki "dosya kodu aynalıyor" cümlesi **betimleyici**
    olarak işaretleniyor — emsal `osc52`, `[clipboard]`'da olduğu hâlde
    `TerminalOptions`'a giriyor.
- **R2 — Varsayılanın tek sahibi var.** Değerler `bt-core`'da `pub const`
  doğuyor; `Settings`'in `Default`'u onları okuyor, `bt-gpu` **aynı
  const'ları import ediyor** (`Frame::default()` ve iki piksel bekçisi).
  İki literal olsaydı bekçi kendi tutarlılığını sınar, sevk edilen hale başka
  ölçüde olsa da yeşil geçerdi — sessiz kırılmanın tarifi.
  - **R2.1** — `CursorMotion`'ın "sayılar ayar modelinde durmasın" kuralı
    **bilerek** deliniyor; delik tek yerde ve gerekçesi `discussion.md`'de.
- **R3 — Akış `cursor_motion` emsali.** `Settings` alanı → `Settings::changes`
  → `bt-shell` → `DisplayLink::set_*`. `TerminalOptions`'a **girmiyor**,
  `Session` görmüyor, `Term` kilidine uğramıyor.
  - **R3.1** — `CellMetrics` **taşıyıcı değil**: 32 çağrı yeri var ve
    `GUTTER_PT`'nin doc'u zaten "ayar değil sabit" diyor.
  - **R3.2** — Değer `LinkIvars`'ta `Cell<_>` olarak yaşıyor ve içerik
    karesinde `clear`'ın **ikinci argümanı** olarak `Frame`'e giriyor; emsal
    `cell`, `motion`, `blink`. Hareket karesi `clear` çağırmıyor, yani değeri
    koruyor.
  - **R3.3** — Her setter **değişimde kare istiyor**, aynı değerde **no-op**.
    Yazılmazsa boştaki pencerede kaydedilen yarıçap ekrana hiç düşmez; blink
    periyodunda daha keskin, çünkü kurulmuş `after` iptal edilemiyor.
- **R4 — `ranged_float` yardımcısı doğuyor.** Aralıklı ondalık ayrıştırıcı
  depoda **iki kez elle** yazılmış (`line_height`, `font_size`). Adlandırılmaz
  ise üç kopya daha eklenir. Mevcut ikisinin taşınması **ayrı karar**;
  taşınmazsa depo aynı işin iki yolunu bilerek taşır.
- **R5 — Blink periyodunun sınırı `0.05..=5.0` ve kapının onu görmediği
  yazılı.** Süreli koşu ayar dosyasını hiç okumuyor, yani bozuk bir periyot
  `make duman`'a görünmez; tek koruma alt sınır. 014'ün defteri bunu zaten
  kaydetmişti ("koruma bir jeton değil varsayılanın kendisi") — bu set o
  korumanın üstüne kullanıcının elini koyuyor.
- **R6 — `caret_sdf_override` yalnız **kenar** için kalıyor.** Doc'u test-only
  varlığını "üretimde bir kurucusu olsaydı ölü kod olurdu" diye savunuyor;
  üretimde setter doğunca o gerekçe düşüyor. Yarıçap ve hale sınamaları
  **gerçek yoldan** sürülecek.
- **R7 — Anahtar başına tam iniş.** Bir anahtar altı yeri **birden** taşıyan
  bir commit'te iniyor: ayrıştırıcı, `Changes` alanı, açılış tohumu, kayıt
  anı, `TEMPLATE`, `docs/AYARLAR.md`. Yarısı inen anahtar hiçbir kapıda
  görünmez — şablona girip ayrıştırılmayan anahtar **sessizce** yoksayılır,
  `Changes` alanı unutulan anahtar yalnız sonraki açılışta uygulanır.
- **R8 — Belgeler kodla aynı commit'te.** `TEMPLATE` (kullanıcının diskine
  yazılıyor), `docs/AYARLAR.md`'nin Şablon bloğu **bayt bayt aynı**
  (`documented_template_is_the_template`), `[terminal]` tablosu ve prozası,
  `CaretShape`'in doc'u (R1.4), `CLAUDE.md`'nin ayar cümlesi.

## Yaklaşım

**Önce yüzey, sonra ritim.**

1. **Yüzey ikilisi** (`cursor_radius`, `cursor_glow`): tek tesisat
   (`CaretStyle` → `DisplayLink` → `Frame`), shader'a hiç dokunmuyor, zaten
   uniform olarak akan değerlerin kaynağını değiştiriyor. Kullanıcının
   ölçülmüş sürtünmesi burada.
2. **Blink periyodu**: `blink.rs`'in `const`'unu alana çeviriyor ve saatin
   yeniden kurulmasını istiyor. Ayrı phase, çünkü ayrı modül ve ayrı risk
   (kurulmuş `after` iptal edilemiyor).

## Kararlar

Gerekçeleri `discussion.md` → Muhakeme + Karar.

1. Üç anahtar; hale payı ile alfası **tek çarpana** indi.
2. `IDLE_STOP` kapsam dışı — üç merceğin de kararı.
3. Varsayılanın tek sahibi `bt-core`'da `pub const`.
4. Akış `cursor_motion` emsali, `TerminalOptions` değil.
5. Bölüm `[terminal]`, gerekçesi yenilendi.
6. Kırpma yok, tanı var.

## Kapsam Dışı

- **Blink'e opacity / fade** — kullanıcı bedeli duyunca çıkardı: geçiş demek
  ekran hızında kare demek ve `QUIET_FLOOR` ile çarpışma gerçek olurdu.
- **`cursor_blink_stop`** (yukarıda).
- **Kenar kalınlığı** — `rule_px`, fontun kendi metriği.
- **Tema başına imleç görünüşü** — tema dosyası renk taşıyor, geometri değil.
- **Enum ayrıştırıcılarının beş kopyası** (`docs/YOL-HARITASI.md` borcu) — bu
  set **ondalık** tarafına bir yardımcı getiriyor (R4); enum tarafı ayrı.

## Göç

Yok. Yeni anahtarlar **opsiyonel** ve varsayılanları bugünkü görüntü, yani
dosyası olan kullanıcı hiçbir fark görmüyor. `TEMPLATE` büyüyor ama
`create_if_missing` yalnız **dosya yokken** yazıyor.

## Akış

| Phase | İş | Neden bu sırada |
|-------|-----|-----------------|
| phase-1 | Yüzey ikilisi (`cursor_radius`, `cursor_glow`) | Kullanıcının ölçülmüş sürtünmesi burada; shader'a dokunmuyor ve tek tesisat |
| phase-2 | `cursor_blink_interval` | Ayrı modül (`blink.rs`) ve ayrı risk: saat yeniden kurulmalı |

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | |
| kapı | |
