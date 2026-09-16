# Hareket altyapısı ve imleç hareketi — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md) ·
> [phase-3.md](phase-3.md) · [phase-4.md](phase-4.md) · [phase-5.md](phase-5.md) ·
> [phase-6.md](phase-6.md)

İmleç artık hücreler arasında kayıyor ve kaymayı süren altyapı (hareket saati,
üç stil, durma koşulu, Hareketi Azalt) sonraki animasyonların tabanı. Dışarıya
görünen üç şey: imlecin kendisi, `[motion]` bölümündeki iki yeni ayar anahtarı
(`cursor_motion`, `reduce_motion` — bugüne kadar sessizce yoksayılan bölüm artık
uygulanıyor) ve duman kapısının üç yeni jetonu (`icerik=`, `hareket=`,
`sessiz=`). Kapı aynı sette büyüdü: boşta sıfır kareyi bozan yavaş bir sızıntıyı
artık ölçülmüş bir sessizlik tabanı yakalıyor.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi
make shader          # .metal değişti (phase-2 imleç dikdörtgeni, phase-5 alfa karışımı)
make test-yaris      # paylaşılan duruma dokunuldu (hasar bayrağı, hareket durumu)
make duman           # pencereyi açan davranış değişti
make kur             # release paket de aynı kapılara tabi; paket yolunu doğrular
```

### Beklenen çıktı

- `make hepsi`: `denetim: temiz`, clippy uyarısız, sınamalar yeşil.
- `make duman`: jeton satırı basılır ve beş sayaç da sıfırın üstünde olur.
  Bugünkü sabit sayaçlar `hucre=8 glif=6 kural=15 yuva=13/2048`; `istek`
  3–4 (çoğunlukla 4, gerekçesi ölçüm kaydında).
  Kapının iki sınırı `icerik ≤ IDLE_FRAME_LIMIT` ve `sessiz ≥ QUIET_FLOOR`;
  ikisinin de değeri, türetmesi ve ölçüm koşuları
  [`docs/OLCUMLER.md` → `## Boşta kare`](../../docs/OLCUMLER.md) → 2026-09-16
  girişinde. Sayılar buraya kopyalanmıyor: tek sahip o dosya.
- `make kur`: `kur: …/bateri.app (sürüm 0.1.0, taban macOS 14.0)`.

### Doğrulama Checklist

- [x] `make hepsi` yeşil
- [x] `make shader` yeşil
- [x] `make test-yaris` yeşil
- [x] `make duman` yeşil (debug) ve paket koşusu yeşil (release)
- [x] `make kur` yeşil
- [x] Set kapısı: `/code-review` (setin aralığı) + `/audit`

## B. Yayın (doğrulamadan SONRA)

### B.1 Ayar şeması `[oto]`

İki yeni anahtar (`[motion] cursor_motion`, `[motion] reduce_motion`), ikisi de
opsiyonel ve varsayılanı kodda: `spring` ve `"system"`. Eski anahtar yok,
silinen anahtar yok, bilinmeyen anahtar korunuyor. `docs/AYARLAR.md` aynı
setin commit'lerinde güncellendi. Kullanıcıya görünen göç: `[motion]` yazılı
bir dosya bugüne kadar sessizce yoksayılıyordu, artık uygulanıyor; tanınmayan
**değer** yalnız kendi anahtarını etkiler ve tanı bırakır.

### B.2 Shader `[oto]`

`cell.metal` iki kez değişti (imleç dikdörtgeni, alfa karışımı); `make shader`
koştu ve `CursorBlock`'un Rust `#[repr(C)]` karşılığıyla alan alan uyumu
`static_assert`/`offset_of` bağlarıyla yerinde. Türetilmiş dosya yok
(`default.metallib` `target/` altında kalır).

### B.3 Ölçüm `[komut]` — **kapandı**

Setin ölçüm bekleyen iddiaları phase-6'da koşuldu ve
[`docs/OLCUMLER.md`](../../docs/OLCUMLER.md)'ye işlendi: `sessiz=` dağılımı,
`IDLE_FRAME_LIMIT`'in yeni operand (`icerik`) üstünden türetmesi ve
`istek ≈ icerik + 1..2` ilişkisi. Bekleyen adım **yok**.

Kapanmayan borç (yeni set değil, kayıtlı): kapı sızıntıyı ancak periyodu
tabandan kısaysa görüyor; hareket saatini atlayıp daha seyrek kare isteyen bir
kodu yapısal kural (`bt-gpu::link` modül başlığı) ve `/audit` tutuyor.
Hareket karesinin encode maliyeti ve `cell_bg`'nin harmanlı geçişi de ölçüm
bekleyen borç olarak `docs/YOL-HARITASI.md` → Sete bağlanmamış borçlar'da.

### B.4 Belge `[oto]`

`CLAUDE.md` (boşta sıfır kare, hareket, duman jetonları, ayarlar),
`Makefile`'ın `duman` yorumu, `.claude/is-akisi/proje.md`'nin doğrulama
tablosu, `docs/AYARLAR.md`, `docs/OLCUMLER.md` ve `docs/YOL-HARITASI.md` set
içindeki commit'lerde güncellendi.

### Yayın Checklist

- [x] Ayar şeması: varsayılanlar + `docs/AYARLAR.md` + bilinmeyen anahtar korunuyor
- [x] Shader: `make shader` + `#[repr(C)]` uyumu
- [x] Ölçüm: `docs/OLCUMLER.md` → `## Boşta kare` 2026-09-16 girişi
- [x] Belge: `CLAUDE.md`, `Makefile`, `proje.md`, `AYARLAR.md`, `YOL-HARITASI.md`
- [ ] `/ship`: `make hepsi` + commit + `main`'e push

## Geri Alma

- **Tamamı:** setin commit'leri sırayla revert edilir (`ae76470..HEAD`). Kapı
  jetonları geri gider, yani `make duman`'ın satırı kısalır — jeton sözleşmesi
  "silinmez, eklenir" dediği için bu, ileri değil **geri** bir adımdır ve
  okuyan tarafı bozabilir; tek commit'lik bir düzeltme tercih edilir.
- **Yalnız ayarlar:** `[motion]` anahtarlarını okumayı bırakmak dosyayı
  bozmaz — bilinmeyen anahtar korunuyor, yani kullanıcının yazdığı bölüm
  yerinde kalır ve yeniden sessizce yoksayılır.
- **Yalnız sessizlik kapısı:** `QUIET_FLOOR` kolunu düşürmek (phase-6 commit'i)
  kapıyı 008 öncesi duyarlılığına indirir; `sessiz=` jetonu sayaç olarak kalır
  ve satırdan **silinmez**.
- **Yalnız shader:** `cell.metal` ile `renderer.rs`'in uniform'u **birlikte**
  geri alınır; ayrı alınırsa iki taraf sessizce ayrışır ve belirti bir piksel
  kayması kadar sessiz olur.
