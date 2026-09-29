# Phase 4 — `glyph_fx` + `selection`, tamamlanma modeli; bekçiler wgpu'ya döner

## Özet

Son iki pipeline grubunu, tamamlanma modelini ve GPU damgasını wgpu
renderer'ına taşımak, ardından `render_offscreen`'i wgpu'ya çevirmek. Phase
sonunda `renderer.rs`'in bütün bekçileri wgpu'da koşuyor ve Metal yalnız
kâhin sahne listesinde kalıyor. wgpu hâlâ `cfg(test)`.

_Requirements: R3.2, R3.3, R3.4_

_Kısıt: yazılan/taşınan kodun yorumları, doc-comment'leri ve tanı metinleri İngilizce (plan.md → Yaklaşım, dil kısıtı)._

## Değişiklikler

### Kalan shader'lar

- **`crates/bt-gpu/shaders/glyph_fx.wgsl` (yeni)** — `glyph_fx.metal`'in
  karşılığı.
  - Kendi vertex'i ve 48 baytlık instance'ı var.
  - Ters dönüşüm glyph uzayına çeviriyor. İki doku da bağlı, düzlem
    instance'tan geliyor.
  - Ölçekleyen dallar doğrusal örnekliyor ama nokta **texel merkezine
    kırpılıyor**, yani süzgeç komşu yuvaya değmiyor (`CLAUDE.md` → 030
    paragrafı; bekçisi dolu/boş komşulu iki atlasın aynı kareyi vermesi).
  - `heat`'in kızgın rengi kare başına tek değer.
  - `t = 1`'de statik glyph'le piksel piksel aynı olma kapısı korunuyor.
- **`selection` pipeline'ı** (`cell_bg.wgsl`'e ya da ayrı dosyaya).
  - `Instance`'ı **aynen** okuyan kendi vertex'i; `rgba` yuvası köşe maskesi.
  - Renk ve yarıçap tek değer.
  - Arama vurgusu aynı pipeline'dan, rol başına bir encode. Sıra: zemin →
    eşleşme → geçerli eşleşme → seçim → caret → glyph.

### Tamamlanma modeli (`discussion.md` → Karar 6)

- **Gönderim indeksi + tik başında engellemeyen `poll`.** Kare başına
  closure kurulmuyor.
- **`kare=`** yalnız hatasız biten indeksleri sayıyor. Hata error scope'tan ve
  uncaptured/device-lost geri çağrısından geliyor.
- **Hata** `Retry::draw_failed`'in politikasına gidiyor: senkron ve asenkron
  tek yol.
- **`acilis=`** (`Stats::mark_startup`) ilk karenin **bittiği** anı ölçüyor.
- **Uykudan önceki bitmemiş kare** tek bir engellemeyen poll'u gecikmeli
  uyandırmaya kuruyor. Durma koşulu: kuyruk boş. Bağlantısı phase-5'te
  `Pacer` gelince kuruluyor; bu phase yalnız renderer tarafını ve sınamasını
  veriyor.
- **GPU damgası.** `TIMESTAMP_QUERY` varsa pass başı/sonu damgası çözülüp
  eşzamansız okunuyor ve `Stats::record_gpu`'ya veriliyor. Yoksa değer
  `unsupported`, jeton anahtarı kalıyor. Jetonun basıldığı yer
  (`bt-shell` → `Report::token_line`) bu phase'te değişmiyorsa değer eşlemesi
  phase-5'e not.

### `render_offscreen` wgpu'ya

- `renderer.rs`'in sınama modülündeki ortak gövde wgpu'yu çağırıyor.
- phase-2/3/4 ikizleri asıllara katılıyor, ikizlik kalkıyor.
- Doğrudan Metal'e inen sınamaların wgpu karşılığı yazılıyor (tamamlanma,
  atlas dokusu, elle kurulan pass). Metal hâlleri kâhin modülüne ya da
  silinmeye.
- **Sınamaların iddiaları değişmiyor.** Bir bekçi wgpu'da yalnız eşiği ya da
  beklenen değeri değişerek geçiyorsa bu bir bulgu: Uygulama Notları'na ve
  kâhin sahnesine yazılıyor, sessizce gevşetilmiyor.

### Kâhin sahne listesi

Eklenen sahneler: yazım efektlerinden birer geliş ve hayalet (`t` ortası ve
`t = 1`), seçim köşeleri (dışbükey/içbükey/basamak), arama eşleşmesi +
geçerli eşleşme, odaksız soluk seçim, doldurma bandı + dock ile üç viewport'lu
tam bir kare.

## Kabul

- `make hepsi` ve `make shader` yeşil. `renderer.rs`'in bütün bekçileri
  (bugün 60) wgpu'da geçiyor.
- Kâhin sahne listesi bütün pipeline'ları kapsıyor ve toleransta.
- Tamamlanma sınamaları geçiyor:
  - hatasız kare sayılıyor, hatalı kare sayılmıyor;
  - `acilis` bitiş anından;
  - uykudan önceki kare tek bir poll'la sayılıyor ve kuyruk boşalınca poll
    kurulmuyor.
- Ürün grafı hâlâ wgpu'suz.

## Checklist

- [ ] Yazılan/taşınan kodun yorumları ve tanı metinleri İngilizce
- [ ] `glyph_fx.wgsl` (texel merkezine kırpma, `t = 1` eşitliği)
- [ ] `selection` pipeline'ı + arama vurgusu, encode sırası
- [ ] Tamamlanma modeli: indeks + poll, `kare=` hatasız, `Retry`, `acilis=`, uykudan önceki kare
- [ ] GPU damgası `TIMESTAMP_QUERY` / `unsupported`
- [ ] `render_offscreen` wgpu'ya; ikizler birleşti; Metal'e doğrudan inen sınamaların karşılığı
- [ ] Kâhin sahne listesi tamam
- [ ] Doğrulama geçti (`make hepsi` + `make shader`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
