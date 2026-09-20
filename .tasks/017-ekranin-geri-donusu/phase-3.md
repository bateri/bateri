# Phase 3 — Doldurma: çizim tarafı

## Özet

Doldurulan satırlar ötelemenin üstündeki alana çizilir ve ızgarayla
**birlikte** kayar.

_Requirements: R3.1, R3.2_

## phase-2'den devralınan

Sınır **hazır ve bugün tüketilmiyor** — bu phase'in ilk işi o iki ucu bağlamak
(`phase-2.md` → Uygulama Notları §1):

1. **`frame()`'in ikinci sink'i.** İmza `frame(sink, fill_sink, blocks)`;
   doldurma hücreleri oradan geçiyor ve satır numaraları **fill-yerel**
   (`0..fill`, `0` en eski, `fill - 1` içeriğin hemen üstü). Ekran satırına
   çeviren taraf bu phase. `bt-gpu::link` bugün `|_| ()` veriyor, yeri
   yorumla işaretli.
2. **`Cursor::fill`.** Kaç satır geldiğini o söylüyor; `content_rows`'a
   **girmiyor**, yani öteleme aritmetiği phase-2'de dokunulmadan kaldı (R2.3).
3. **Geri alma şeridi kurulu.** `Session::fill_rows` sıfır dediğinde ikinci
   sink hiç çağrılmıyor; `fill == 0` iken kareyi bit bit aynı tutmanın
   `bt-core` yarısı bitti, `bt-gpu` yarısı bu phase'in Kabul'ünde.
4. **Doldurmada seçim yok** (phase-2 §5) — vurgusuz hücreler bekleniyor, bu
   bir eksik değil karar.

## Kol seçimi phase-0'dan gelir

Push anında aritmetik (`origin - fill + row`) **elendi** ve gerekçesi repoda
yazılı: `renderer.rs:715-719` dock'un muafiyetinin aritmetikle kurulamadığını
söylüyor (`set_origin_rows` sink'ten sonra çağrılıyor), ve hareket karesi
listeleri koruyup yalnız `origin_px`'i yeniden yazdığı için (`frame.rs:1774`)
push anında pişmiş bir konum her kayma karesinde bayat olur — 200 ms boyunca
ızgara süzülürken doldurma yerinde donar. Ayrıca `[0, origin_px)` ızgara
viewport'unun **üstünde** kalıyor ve Metal orayı kırpıyor
(`renderer.rs:643-647`), yani negatif satır da çare değil.

- **2b-i — üçüncü `setViewport`**, `originY = origin_px - fill_px`. Dock
  emsalinin birebiri (`renderer.rs:734`).
- **2b-ii — okuma anında çeviri**, caret emsali (`frame.rs:1215`).

## Değişiklikler

- **`crates/bt-gpu/src/frame.rs`** — doldurma listeleri, dock örüntüsünde:
  kendi `bg`/`glyph`/`rule` listeleri, `clear` temizler, `move_caret` korur.
  **Sayaçlardan muaf** (R3.2): `hucre=`/`glif=`/`kural=` jetonlarına girmez.
  Kardeş bekçi `frame.rs:2191`'in (`the_dock_keeps_its_own_lists_and_stays_out_of_the_counters`)
  eşi yazılır — duman sözleşmesi `hucre=8 glif=6 kural=15` bit bit korunmalı.
- **`crates/bt-gpu/src/renderer.rs`** — doldurmanın encode'u, **ızgaradan
  sonra dock'tan önce**. Sıra keyfi değil: kayma boyunca ızgaranın üst satırı
  doldurma bandına taşıyor ve dock'un opak zemini en altta kalmalı.
- **`crates/bt-gpu/src/link.rs`** — `Cursor::fill` `Frame`'e geçer; hareket
  karesinin yolu (`frame.rs:1774` emsali) doldurmayı da tazeler ya da
  viewport'unu yeniden hesaplar — hangisi phase-0'ın koluna bağlı (R3.1).

## Kabul

- Ekran dolu → Tab → Ctrl-C: doldurulan satırlar **içeriğin hemen üstünde**,
  ekran tam dolu, delik yok.
- Kayma boyunca (200 ms) doldurma ızgarayla birlikte kayıyor; ikisinin
  arasında dikiş yok. Bekçi hareket karesini taklit eder (yalnız `origin_px`
  değişir, listeler korunur).
- `fill == 0` iken çizilen kare bugünküyle bit bit aynı; `hucre=8 glif=6
  kural=15` oynamıyor.
- Dock'u olmayan pencerede doldurma encode'u **hiç kurulmuyor** (dock
  emsali, `renderer.rs:743`).
- `make hepsi` ve `make shader` (dokunulduysa) yeşil; `make duman` jetonları
  değişmemiş.

## Yayın Etkisi

- **shader:** yeni `.metal` beklenmiyor — doldurma mevcut `cell_bg`/`cell`
  pipeline'larını kullanır. Dokunulursa `make shader` koşar ve `#[repr(C)]`
  ↔ `.metal` alan alan karşılaştırılır. `stride 32` assert'leri değişmiyor
  (yeni alan yok).
- **`CLAUDE.md`:** "`setViewport` dört listeyi birden kaydırıyor" ve
  `frame.rs:575`'in `origin_px` doc'u — liste sayısı ve uzay sayısı
  güncellenir.
- **Duman kapısı bu özelliğe yapısal olarak kör:** süreli koşu `/bin/sh`
  koşuyor, dock yok, doldurma hiç tetiklenmiyor. `icerik`/`sessiz`/`kayma`
  oynamıyor — iyi haber ve aynı zamanda uyarı: doldurmadan doğan bir kare
  sızıntısını duman göremez. Tek koruma birim bekçi + gözle kontrol.
  "Duman'a dock ekleyelim" önerisi kapıyı gevşetir, açılmaz.
- terminfo / ayar şeması / tema / shell entegrasyonu / app bundle / yeni
  bağımlılık: yok.

## Checklist

- [ ] phase-0'ın seçtiği kol uygulandı
- [ ] Doldurma listeleri sayaçlardan muaf, kardeş bekçi yazıldı
- [ ] Encode sırası: ızgara → doldurma → dock
- [ ] Test: hareket karesinde doldurma ızgarayla birlikte kayıyor
- [ ] Test: `fill == 0` iken çizim bit bit aynı
- [ ] Test: dock'u olmayan pencerede encode kurulmuyor
- [ ] Doğrulama geçti (`make hepsi`; `.metal` değiştiyse `make shader`)
- [ ] Riskli phase: `/code-review` koştu (`#[repr(C)]` ↔ `.metal` düzeni),
      bulgular giderildi
- [ ] Yayın etkisi yazıldı
