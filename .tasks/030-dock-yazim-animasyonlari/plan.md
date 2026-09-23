# Dock'ta yazma ve silme animasyonları

## Hedef

Dock'ta yazılan her glyph seçilen efektle gelsin, Backspace'le silinen glyph
seçilen efektle gitsin — Metalterm'in Keypress (9 efekt + `off`) ve Erase
(8 efekt + `off`) listelerinin hepsi. Yapıştırma, geçmiş, toplu silme ve
tamamlama anında olur; boşta sıfır kare korunur; `snap` ve Hareketi Azalt
efektleri kapatır ya da en sade hâline indirir. Kararlar ve gerekçeleri
`discussion.md`'de.

## Gereksinimler

- **R1** — Eklenen/silinen glyph `bt-core`'da, `Session::dock`'ta çağıranın
  tamponuna (son çizilen ayna) karşı bulunur (Karar 1).
  - **R1.1** — Fark yalnız `BUFFER` üzerinde, `CURSOR` ile yönü belirlenmiş;
    damga (`answers`) ilerlemediyse fark alınmaz; taban `Live` ya da damgalı `Idle`.
  - **R1.2** — Yalnız tek bitişik ekleme/silme ve glyph sayısı ≤ girdi sayısı
    (`answers` farkı) canlanır; tablo Karar 2'de (her satırı bir sınama).
  - **R1.3** — `Reset`: canlanmayan değişim, yeni taraf `Live` değil ya da eski taraf `Live`/`Idle` değil.
    **Pencereleme kayması `Reset` değil** (kullanıcı kararı, 2026-09-23 —
    phase-1'in sapma 9'u): dock'a sığmayan satırda da tuş canlanır; kayma
    sınırdan sütun farkı olarak geçer, uçuştaki efektler o kadar kayar ve
    yeni glyph yine `Arrive`/`Erase` alır. Panelin reddettiği `bt-gpu`
    sütun defteri geri gelmiyor: fark `bt-core`'un `window_skip`'inden.
  - **R1.4** — Hayalet hücreleri eski tamponun vurgu stiliyle, temaya
    çözülmüş (karakter, renk, biçim, `wide`).
- **R2** — Sınır: `Session::dock` / `dock::render` ikinci bir sink alır, karede
  en çok bir `DockEdit` (`Arrive`/`Erase`/`Reset`) basar; `bt_core::Cell`'e
  alan eklenmez, `Session::dock`'un doc'u yeni sözleşmeyi söyler (Karar 3).
- **R3** — `GlyphFx` (Karar 4).
  - **R3.1** — Ayrı `RefCell` ivar'ı; `advance(dt)`; boş liste yerleşik;
    uyku testinde adlı terim; `FX_MAX` tavanı.
  - **R3.2** — `Motion::finish`'in üç çağıranı fx'i de bitirir.
  - **R3.3** — Düzenlemeler tampona akar; uçuştaki gelişin statik glyph'i
    `dock_glyphs`'ten çıkarılır (sütun + karakter eşleşmesiyle); yeni
    düzenlemenin sütununa eşit ya da sağındaki gelişler biter; `Reset` hepsini
    bitirir.
  - **R3.4** — Hareket karesi yalnız fx listesini yeniden basar; `hareket=`,
    `kayma=`, `icerik=` anlamını korur.
- **R4** — `glyph_fx` pipeline'ı (Karar 5): `FxInstance` iki tarafta assert'li;
  iki doku, düzlem instance'tan; geniş glyph `prepare`'in yolundan, yarı biti
  ve iki hücrelik kutunun merkezi; `CursorBlock` okunur; hayaletler dock
  glyph'lerinden önce, gelişler sonra.
- **R5** — Hermetik değişmezler, her efekt için döngüyle: geliş `t = 1`'de
  statik glyph'le piksel piksel aynı; hayalet `t = 1`'de düz zemin; komşu yuva
  hiç örneklenmez (yuvası dolu komşuyla sınanır); geniş glyph iki yarısında
  tek kutu olarak dönüşür.
- **R6** — İndirgeme `bt-gpu`'da, `Motion::mode`'un yanında: `snap` → ikisi
  kapalı; Hareketi Azalt → geliş `fade`, hayalet kapalı (Karar 7).
- **R7** — Ayarlar: `[motion] keypress` / `erase`, varsayılan `fade` / `recede`,
  `NAMES` yalnız çizilebilen adları taşır; kayıt anında; ayar penceresinde iki
  popup, ezildiğinde devre dışı + açıklama; `docs/AYARLAR.md` + şablon;
  bilinmeyen anahtar tanığı taşınır.
- **R8** — Keypress'in kalan sekiz efekti (`rise`, `pop`, `extrude`, `heat`,
  `echo`, `drop`, `ink`, `squeeze`), Karar 6'daki tanımlarla.
- **R9** — Erase'in kalan yedi efekti (`iris`, `undertow`, `echo`, `bleed`,
  `unravel`, `sublime`, `shatter`), Karar 6'daki tanımlarla.
- **R10** — `CLAUDE.md`: dock animasyonlarının sözleşmesi (kural + gerekçe +
  işaretçi), pipeline sayısı beş, emojinin blend cümlesi düzeltilir (kod
  `SourceAlpha` diyor); `settings.rs` başlığındaki `[motion] keypress`
  örneği güncellenir.

## Yaklaşım

1. `bt-core`: saf `dock::diff` (eski/yeni `DockState` → düzenleme ya da
   `Reset`), `Session::dock`'ta `clone_from`'dan önce damga eşitliği kapısının arkasında (`End` kolu `Idle` aynayı damgalar);
   `dock::render` düzenlemeyi ekran sütununa çevirip ikinci sink'e basar,
   hayaletleri çözer, eski/yeni `skip`'i karşılaştırır. `bt-gpu` ikinci sink'i
   bağlar ama düzenlemeleri henüz tüketmez — görünür değişim yok.
2. `bt-gpu`: `glyph_fx.rs` (`GlyphFx`), `FxInstance` + `shaders/glyph_fx.metal`
   + beşinci pipeline, `Frame`'de fx listeleri ve susturma, `link.rs`'te iki
   kare yolu, uyku terimi ve `finish`; iki efekt (`fade`, `recede`) sabit
   varsayılanla, indirgeme `Motion`'ın yanında. `CLAUDE.md`.
3. Ayarlar: iki anahtar (`off`/`fade`, `off`/`recede`), `Changes::motion`
   kolu, `DisplayLink`'e ham adlar, Motion bölmesinde iki popup, belgeler.
4. Keypress'in kalan sekiz efekti: shader dalları + `NAMES` + belgeler.
5. Erase'in kalan yedi efekti: shader dalları (parçalılar dahil) + `NAMES` +
   belgeler.

## Kapsam Dışı

- **Speed çarpanı** (Metalterm `TIMING`): bütün hareket altyapısının süre
  çarpanı, bu setin değil.
- Satır ortasında silinince sağdaki metnin **kayarak** yerine gelmesi: metin
  bugünkü gibi anında akar, hayalet onun altında söner.
- Izgara ve doldurma bandında animasyon: satır ızgaradayken (dock sahibi
  değil, `blocks` kademesi, entegrasyonsuz oturum, `Multiline`) efekt yok —
  efektlerin konusu dock'ta yazmak.
- Yapıştırma, geçmiş, toplu silme, IME'nin çok karakterli onayı için kademeli
  animasyon (Karar 2).
- Bağlam satırı (yol | dal) — kullanıcı oraya yazmıyor.
- `make duman`'a yeni jeton (Karar 7).

## Akış

```
okuyucu thread: OSC 8133 → ShellLog.dock (bugünkü gibi; `End` kolu `Idle` aynayı damgalar)

içerik karesi (link.rs):
  Session::dock(cols, &mut into, …, sink_cells, sink_edit)
    ├─ yaprak kilit: into.answers == yeni.answers? ── evet → düzenleme yok
    │                         └─ hayır → dock::diff(into, yeni) → Arrive/Erase/Reset
    ├─ into.clone_from(yeni)
    └─ dock::render: hücreler → sink_cells, düzenleme (sütun, hayalet) → sink_edit
  edits tamponu → GlyphFx.apply(edit, now)   (Reset → finish all;
                                              col ≤ geliş.col → o geliş biter)
  Frame: uçuştaki gelişlerin statik glyph'i dock_glyphs'ten çıkar
  Frame.set_dock_fx(GlyphFx → FxInstance'lar)

hareket karesi:
  GlyphFx.advance(dt) → Frame.set_dock_fx(...)   (statik listeler korunur)

uyku: motion.settled() && !blink_flip && GlyphFx.is_empty()

encode_dock: zemin → dock_bg → caret → HAYALETLER (glyph_fx) → emoji
             → dock_glyphs → GELİŞLER (glyph_fx) → dock_rules
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | |
| phase-4 | |
| phase-5 | |
| kapı | |
