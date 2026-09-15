# Hareket altyapısı ve imleç hareketi — Bağlam

## Mevcut Durum

**Kare ritmi.** `CAMetalDisplayLink` **paused** durur. Yeni içerik gelince
(`Wake::wake` → `Waker`) açılır; callback `Session::frame()` `None` dönerse
(`dirty` bayrağı boş) link'i geri uyutur (`bt-gpu/src/link.rs`). Kare istemenin
tek tanımı `Waker::wake` ve o da hasar bayrağını diker — yani **bugün kare
istemenin tek gerekçesi grid'in değişmesidir**. Zamana bağlı, içerikten
bağımsız bir kare talebinin yeri yok.

**İmleç.** `Session::frame()` sınırdan `Cursor { col, row, visible }` veriyor:
tam sayı grid koordinatı, ara konum yok. `Frame::push_cursor` onu arka plan
listesinin **sonuna** opak bir dikdörtgen olarak ekliyor (`accent` rengi,
`bg_count`'a girmiyor); glyph'ler bu bloğun üstüne çiziliyor. İmlecin altındaki
hücrenin **tersine çevrilmesi `bt-core`'da**: `frame()` o hücrenin ön planını
temanın zeminine çeviriyor ve alt çizgi rengini düşürüyor (`session.rs`,
gerekçe "kararın adı terminal semantiğidir"; 003/004 `plan.md` → R4.1).
Şekil (`CursorShape`) yalnız `Hidden` için okunuyor — beam/underline yok,
blink yok.

**Ayarlar.** `settings.toml` bugün `[terminal]`, `[appearance]`, `[font]`,
`[clipboard]` biliyor; bilinmeyen anahtar sessizce yoksayılıyor ve
`bt-core/src/settings.rs`'in başlık yorumu bunun örneği olarak adıyla
`[motion]`'ı veriyor. Sistemin **Reduce Motion** ayarı hiçbir yerde okunmuyor
(açık/koyu görünüm okunuyor, `apply_appearance`). Süreli koşu ayar dosyasını ve
sistem ayarlarını **hiç görmüyor** (`Inputs::Hermetic`, 007 Karar 1).

**Kapı.** `make duman` üç saniyelik hermetik bir koşu açıyor; `kare` jetonu
`IDLE_FRAME_LIMIT = 8`'i aşarsa kırmızı. Sayı iki kez ölçüldü (005 phase-3,
006 phase-5) ve ikisinde de sağlıklı koşu `kare=1–2` verdi.

## Motivasyon

`docs/YOL-HARITASI.md` → Eşikten sonra, 008: Metalterm'i ekranda tanıtan üç
şeyden biri (renk, imleç, yüzey) ve shell entegrasyonuna bağlı **değil**. Renk
007 ile geldi. Referansın envanteri `docs/ARASTIRMA.md` → Hareket
(`[motion]`); bu set onun `cursor_motion` ve `reduce_motion` satırlarını
karşılıyor, geri kalanı (keypress, delete_mode, feed_lift, status bar) sonraki
setlerin.

Setin ikinci yarısı **devralınan bir borç** ve kullanıcı onu bu setin içinde
istedi; yol haritasının borç listesinden buraya taşındı:

> **Boşta kare kapısı yavaş bir animasyonu kaçırır.** 005 phase-3 sınırı
> ölçümle büyüttü; bedeli, kapının algılama tabanının yükselmesi oldu — durma
> koşulu unutulmuş yavaş bir blink bugün yeşil geçer. Bu depoda öyle bir
> animasyon **yok**, ama 008 (hareket altyapısı) tam bunu getirecek. Sayılar,
> mekanizma ve aday çözüm (`istek=`'i orana çevirip kapıya bağlamak; eşiği
> **ölçülmedi**) `bt-shell`'in `IDLE_FRAME_LIMIT` doc'unda. Aynı sabit 006
> phase-5'te görünür pencerede yeniden ölçüldü ve değişmedi; bu madde o
> ölçümle **kapanmadı**, algılama tabanı aynı — **tek sabit, iki ayrı iş**.

Borcun sırası bağlayıcı: **ilk animasyondan önce** çözülür, yoksa ilk animasyon
tam o kör noktaya düşer.

## Kanıt

- **Kapının algılama tabanı.** Kapı `n > 8`'de ateşliyor, yani yakalamak için
  dokuz kare gerekiyor; `make duman`'ın üç saniyesinde bu **3 Hz** demek. Durma
  koşulu unutulmuş 2 Hz'lik bir blink üç saniyede ~6 kare eder ve **yeşil
  geçer** (`IDLE_FRAME_LIMIT` doc'u).
- **Sağlıklı dağılım dar.** İki ölçümde de `kare=1–2`, `istek=2–3`
  (`docs/OLCUMLER.md` → Boşta kare). Yani boşluk sınırın kendisinde değil,
  ateşleme eşiğinin süreye bağlı olmasında.
- **Animasyon kareyi şişirecek.** 120 Hz'de 200 ms'lik tek bir imleç kayması
  ~24 kare eder — bugünkü `kare ≤ 8` kapısı ilk animasyonla birlikte, kod
  doğruyken kırmızı düşer. Yani kapı ya muhasebeyi ayırmalı ya da sınırı
  yükseltmeli; ikincisi algılama tabanını daha da yukarı iter.
- **Ölçüm yükünde `kare` ile `istek` mertebelerce ayrışıyor** (kare=9–21,
  istek=25 000–72 000) ve mekanizması ölçülmedi; bu, `istek=`'i oran olarak
  kapıya bağlama fikrinin bugün dayanaksız olduğunu söylüyor.
- **API'ler yerinde** (doğrulandı, `objc2 0.3.2`): `CAMetalDisplayLinkUpdate`
  `targetTimestamp` / `targetPresentationTimestamp` veriyor (animasyon saati
  `Instant::now()` olmak zorunda değil);
  `NSWorkspace::accessibilityDisplayShouldReduceMotion` ve
  `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification` var.

## Mevcut Mimari

```
okuyucu thread                     ana thread
──────────────                     ──────────
alacritty Wakeup
   └─ Adapter: dirty=true
      └─ Wake::wake ──────────────▶ Waker: istek++, dirty.mark,
                                    link.setPaused(false)
                                          │
                     CAMetalDisplayLink ──┘   (vsync)
                                          ▼
                              LinkDelegate::needs_update
                                 ├─ kapı kapalı → setPaused(true)
                                 ├─ session.frame(sink) == None
                                 │     → setPaused(true)      ← boşta sıfır kare
                                 └─ Frame ▸ push_cursor ▸ renderer.draw
```

Eksik olan üçüncü bir "kare gerekçesi": **zaman**. Animasyon, grid değişmediği
hâlde kare isteyen ilk şey olacak ve her biri bir durma koşulu taşımak zorunda
(`CLAUDE.md` → boşta sıfır kare).
