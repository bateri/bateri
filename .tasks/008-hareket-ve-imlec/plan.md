# Hareket altyapısı ve imleç hareketi

## Hedef

İmleç hücreler arasında **kayarak** gitsin; kaymayı süren altyapı (saat, yay
fiziği, her animasyonun durma koşulu, Reduce Motion) sonraki animasyonların da
tabanı olsun. Aynı sette boşta sıfır kare kapısı, yavaş bir animasyonu
görebilecek hâle gelsin — **ilk animasyondan önce**.

## Gereksinimler

- **R1 — Kapı, hareket kodu inmeden önce hazır olur.**
  - **R1.1** — Duman kapısı GPU sayacı yerine `needs_update`'te artan
    **`icerik=`** sayacına bakar (`icerik ≤ IDLE_FRAME_LIMIT`); `kare=` GPU
    sayacı olarak değişmeden basılır. Çıkarma, pay ve `u64` sarması yok.
  - **R1.2** — Satır üç yeni jeton kazanır: `icerik=`, `hareket=`, `sessiz=`
    (son kareyle deadline arasındaki süre; hiç kare yoksa `sessiz=none`).
    Eski jetonların hiçbiri silinmez, anlamı değişmez.
  - **R1.3** — Zaman tabanı display link'in damgasıdır; kare başına saat
    okuması eklenmez, deadline'da bir okuma yapılır.
- **R2 — İmlecin altındaki metin piksel işi olur.**
  - **R2.1** — "Blok altındaki metnin rengi" kararı `bt-core`'da kalır ve
    `Cursor` ile sınırdan geçer; `bt-gpu` rengi temadan türetmez.
  - **R2.2** — `cell` pipeline'ı imlecin piksel dikdörtgenini uniform olarak
    alır; dikdörtgenin içindeki fragment o rengi kullanır. Glyph ve kural
    çizgileri aynı yoldan geçer.
  - **R2.3** — Bu phase'in görsel sonucu bugünküyle **birebir aynıdır** ve
    bunu offscreen sınama kanıtlar. `hucre=` sayacı değişmez (imleç
    dikdörtgeni `bg_count`'a girmez).
- **R3 — Hareket altyapısı.**
  - **R3.1** — Animasyon tipi saf ve sınanabilir (ObjC'siz, kilitsiz);
    `Gate`/`FailureStreak` emsali. Her animasyon `settled` sorusunu yanıtlar.
  - **R3.2** — Hareket karesi `Waker`'a **dokunmaz**: `needs_update`'in "hasar
    yok" dalı yerleşmemiş hareket varken uyumaz. Hareket karesi grid'i yeniden
    taramaz, son `Frame`'in imleç dikdörtgenini tazeler.
  - **R3.3** — Durma koşulu iki katlı: konum+hız eşiği **veya** süre tavanı.
    `dt` kırpılır (örtülmeden dönüşte sınırsız fark gelir).
  - **R3.4** — Snap hâlleri: ilk kare, görünmezken açılan imleç, geometri
    (pencere/font/zoom) ve geçmişte kaydırma. Durum **hücre biriminde** tutulur.
- **R4 — Kapının animasyon yarısı.** Deadline'da yerleşmemiş animasyon varsa
  koşu kırmızı (`Verdict::MotionUnsettled`); duman reçetesi bir imleç hareketi
  içerir ve `hareket > 0` gerekli sayaç olur.
- **R5 — Ayarlar.** `[motion] cursor_motion = "snap"|"ease"|"spring"`
  (varsayılan `spring`) ve `reduce_motion = "system"|"on"|"off"`. Kayıt anında
  uygulanır; kabul edilmeyen değer kendi anahtarını değiştirmez ve tanı
  bırakır. Süreli koşu `[motion]`'ı **okumaz**.
- **R6 — Reduce Motion.** `"system"` iken `NSWorkspace`'ten okunur ve canlı
  izlenir; açıkken her animasyon 90 ms'lik belirmeye iner. Süreli koşu sistem
  ayarını **okumaz**. *(Kapıda daraldı: belirme **duraksamadan sonraki**
  harekete ait. Belirmeden sık gelen hareketler — akan çıktı — opaklığı
  sıfırlamıyor, yoksa imleç görünmez oluyordu; phase-5 → Uygulama Notları.)*
- **R7 — Süreli kapı ölçümle iner.** `sessiz=` dağılımı ve
  `IDLE_FRAME_LIMIT`'in yeni türetmesi ölçülür, `docs/OLCUMLER.md`'ye yazılır;
  kapı (`sessiz ≥ T`) kod phase'lerinden **ayrı** commit'le iner.

## Yaklaşım

1. **Muhasebe önce** (phase-1): jetonlar ve `icerik` kapısı, hareket kodu
   yokken. `hareket=0` ve `sessiz=` bu commit'te sayaç; davranış değişmez.
2. **İmleç dikdörtgeni shader'a** (phase-2): renk `Cursor`'da, dikdörtgen
   uniform'da; görsel sonuç sabit. Riskli phase.
3. **Hareket** (phase-3): `bt-gpu::motion`, link'in "hasar yok" dalının
   genişlemesi, `Frame` tazeleme, yerleşme kararı, duman reçetesinin imleç
   hareketi. Varsayılan stil sabit kodlu `spring`.
4. **Stiller ve ayar** (phase-4): üç stil, `[motion] cursor_motion`, canlı
   uygulama, `docs/AYARLAR.md`.
5. **Reduce Motion** (phase-5): `reduce_motion`, sistem ayarı ve bildirimi,
   90 ms belirme.
6. **Ölçüm ve süreli kapı** (phase-6): iki kollu ölçüm, `T`, `sessiz ≥ T`.

## Kapsam Dışı

İmleç şekilleri (beam/underline, DECSCUSR), blink, Smear/Squash/Phosphor/Arc,
`intensity`/`duration` çarpanları, yumuşak kaydırma ve `feed_lift`, yazma ve
silme animasyonları (013–014), odak kaybında içi boş imleç, hareket için menü
yüzeyi, geometri karelerini sayaç dışında tutma borcu (kayıtlı, `sessiz=`'in
yanlış pozitifi onunla kapanır).

## Göç

Yeni anahtarlar; eski anahtar yok, silinen anahtar yok. `[motion]` bugüne
kadar "bilinmeyen bölüm" olarak sessizce yoksayılıyordu — elinde o bölümü
taşıyan bir dosya olan kullanıcı için davranış "yoksayılıyor"dan
"uygulanıyor"a geçer; tanınmayan **değer** tanı bırakır, dosyayı bozmaz.

## Akış

```
içerik karesi (hasar var)          hareket karesi (hasar yok, yerleşmedi)
─────────────────────────          ─────────────────────────────────────
frame.clear()                      frame.bg.truncate(bg_count)
session.frame(sink) → Cursor       motion.position(t)  (hücre birimi)
motion.retarget(cursor)            push_cursor(ara konum, renk)
push_cursor(...)                   renderer.draw(...)
renderer.draw(...)                 icerik değişmez, hareket++
icerik++                           link uyanık kalır (yerleşene kadar)
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| phase-4 | ✅ |
| phase-5 | ✅ |
| phase-6 | ✅ |
| kapı | ✅ |

phase-2'nin `/code-review`'undan çıkan tek bulgu phase-1'in kodundaydı ve
kendi commit'iyle indi (phase açmadı): `applicationWillTerminate:` ölçüm
kapısı kapalıyken de saat okuyordu.

Set kapısının bulguları `kapı` commit'ine girdi; kapıdan sonra çıkan tek
düzeltme (belge çelişkisi: "yazarken imleç opak kalır" normal yazma hızında
doğru değil) phase açmadan ayrı bir commit'le indi.
