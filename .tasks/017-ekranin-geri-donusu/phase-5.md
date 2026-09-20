# Phase 5 — Doldurulan alanda seçim

## Özet

Doldurulan satırların üstüne yapılan tıklama **reddedilir**; bugünkü kırpma
oraya yanlış bir çapa koyuyor.

_Requirements: R5.1, R5.2_

## Neden zorunlu

`view.rs:94` (`((y / cell_h) as u16).min(rows - 1)`) orijinin üstündeki her
tıklamayı ızgaranın **0. satırına doyuruyor**; bekçisi bunu kendi diliyle
söylüyor: *"Boş alanın tamamı 0. satıra yapışır"* (`view.rs:741`). Bugün
görünmez, çünkü orası boş. Doldurma gelince kullanıcı oraya **metin görüyor**
ve üstünden sürüklüyor: çapa 0. satıra düşüyor, vurgu gözün gördüğü yerde
değil içeriğin tepesinde beliriyor. `CLAUDE.md`'nin seçim sözleşmesi bunu
adıyla yasaklıyor — *"gözün gördüğü ile panonun verdiği ayrışmıyor"*.

## Değişiklikler

- **`crates/bt-shell/src/view.rs`** — `point_to_cell` doldurma yüksekliğini
  öğrenir ve `fill > 0` iken orijinin üstünü `None` döner.
  - **Kırpma koşulsuz kaldırılamaz** (R5.2): boş alanda 0. satıra yapışmak
    **istenen** davranış (`view.rs:741`'in gerekçesi: `u16` taşmasının asıl
    yeri). Yalnız doldurma varken reddedilir.
  - Doldurma yüksekliği `Origin` ile aynı yoldan gelir — kare yolu yazar,
    fare yolu okur, ana thread (`link.rs:288-315`). İkinci bir senkronizasyon
    kurulmaz.
- **`crates/bt-shell/src/view.rs`** — bekçi `view.rs:741` **yeniden yazılır**:
  öncülü ("boş alan") ölüyor, iki hâl ayrılıyor.

## Kabul

- `fill > 0` iken doldurulan satırların üstüne tıklama `None` — sürükleme
  başlamıyor, çapa 0. satıra düşmüyor.
- `fill == 0` iken bugünkü kırpma **birebir** korunuyor (üst kenar, ortası ve
  içeriğin bir öncesi hepsi 0. satır).
- Sürükleme sırasında doldurma alanına girilirse seçim ucu **son geçerli
  hücrede** kalıyor (`follow_pointer`, `view.rs:552`).
- `make hepsi` yeşil.

## Yayın Etkisi

- **`CLAUDE.md`:** seçim sözleşmesinin cümlesi korunuyor; doldurulan alanın
  neden **seçilemez** olduğu (ve "seçilemez" ile "yanlış seçilir" farkı)
  bir cümleyle yazılır.
- **Bilinen sınır, `teslim.md`'ye:** doldurulan satırlar **görünür ama
  seçilemez**. Seçilebilir olmaları `Cell.row` sözleşmesini negatife açmayı
  gerektiriyor ve bu setin kapsamı dışında.
- shader / terminfo / ayar şeması / tema / shell entegrasyonu / app bundle /
  yeni bağımlılık: yok.

## Checklist

- [x] `point_to_cell` doldurma yüksekliğini alıyor ve `None` dönüyor
- [x] `fill == 0` yolu bit bit korunuyor
- [x] `view.rs:741` yeniden yazıldı (iki hâl ayrıldı)
- [x] Test: doldurma alanına tıklama `None`
- [x] Test: boş alanda kırpma hâlâ 0. satır
- [x] Test: sürükleme doldurma alanına girince uç son hücrede kalıyor —
      **doğrudan yazılamadı** (AppKit'e bağlı); kanıt `None` bekçisi + iki
      çağrı yerinin `if let Some`'ı (aşağıda)
- [x] Doğrulama geçti (`make hepsi`)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

- **Doldurma yüksekliği `Origin`'in gövdesine girdi** (`link.rs`), yanına
  değil: `Origin(Rc<Cell<Drawn>>)` ve `Drawn { px, fill_rows }`. Tek gövde,
  tek yayın noktası (`publish_origin`, `draw` `Ok` kolu) — ayrı bir hücre
  olsaydı fare, orijini yeni bandı eski bir kareye göre çevirebilirdi. Hareket
  karesinde band `Frame`'de korunuyor, yani kayma boyunca yayınlanan değer
  sabit.
- **Ret `y < 0.0`**, "bandın içi" değil: `fill = min(gap, taze satır)` yüzünden
  bandın üstünde hâlâ boşluk kalabilir ve iki bölgeyi ayırmak farenin `fill`'i
  bir de piksele çevirmesini isterdi. Reddin yönü güvenli
  (`a_click_over_the_filled_area_is_rejected_instead_of_clamped`'in son iki
  iddiası).
- **Tekerleğin işaretçisi reddin dışında** ve sıfır bilerek geçiyor
  (`scrollWheel:`): oradaki nokta bir seçim ucu değil fare raporuna giden
  koordinat, ve `point_to_cell` `None` dönse `let ... else` kaydırmanın
  **tamamını** düşürürdü — band ekrandayken işaretçiyi oraya götüren kullanıcı
  hiç kaydıramazdı. Kuralın tek evi yine `point_to_cell`; ayrışan şey çağrı
  yeri.
- **Sürükleme ucunun sınaması yazılamadı**: `mouseDragged:` ile
  `follow_pointer` AppKit olayına ve pencereye bağlı, `bt-shell`'de bunu kuran
  bir seam yok. Kanıt ikiye ayrıldı — `None`'ın kendisi sınanıyor, iki çağrı
  yerinin `if let Some`'ı da yorumla adlandırıldı.
- **Bilinen davranış** (kapsam dışı bırakıldı): reddedilen `mouseDown:`
  `set_selection`'a hiç varmıyor, yani band üstüne tıklamak **duran seçimi
  temizlemiyor**. Temizlemek `mouse_down`'ın `None`'ın *sebebini* bilmesini
  isterdi; kabul ölçütü de temizlemeyi istemiyor.
