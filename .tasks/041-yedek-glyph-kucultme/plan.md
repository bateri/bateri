# Yedek glyph'in küçültülmesi

## Hedef

Hücreyi aşan tek sütunlu yedek glyph (`⧉`, tek sütunlu renkli emoji) kutu
yerine hücreye sığacak kadar küçültülerek çizilsin. Hangi karakterlerin
kutu çıktığı kullanıcı bulmadan önce bir taramayla bilinsin.

## Gereksinimler

- **R1** — Tarama: sembol ve emoji aralıklarındaki her karakter atlasın
  kendi kapısından geçer ve dört gruptan birine düşer (fontta / yedekten
  sığdı / hiçbir fontta yok / kapıdan döndü + taşma oranı). Kaynak font adı
  da raporlanır.
  - **R1.1** — 13pt ve 16pt'de, @1x ve @2x ölçekte koşar. Taban aile
    varsayılan zincirdir ve bir env değişkeniyle değiştirilebilir.
  - **R1.2** — `make` hedefiyle çağrılır ve `make hepsi`'ye girmez.
- **R2** — Bekçi: gerçek araçların bastığı karakterler listesi
  `make hepsi`'de sınanır. Listede kutu çıkan karakter kırmızıdır.
  - **R2.1** — Bugün kutu çıkanlar listede adıyla "beklenen kutu"
    olarak işaretlidir. phase-2 bu kümeyi boşaltır.
- **R3** — Küçültme: kapıdan tek hücre ve iki hücre olarak dönen aday,
  mürekkebi sınırın altında taşıyorsa küçük puntolu kopyasıyla kabul
  edilir ve çizilir. Sınırın üstündeki aday kutu kalır.
  - **R3.1** — Sınır, tarama dağılımından seçilen adlı bir sabittir. Tek
    sütunlu renkli emojiyi kapsar.
  - **R3.2** — `.LastResort` küçültülerek kabul edilmez.
  - **R3.3** — Bugün kapıyı geçen her aday bit bit aynı çizilir
    (küçültme yalnız reddedilen adaya uygulanır).
  - **R3.4** — Küçültülen glyph ızgarada, dock'ta ve doldurma bandında
    aynı görünür. Tek yerden geldiği için ayrı iş gerekmez. Gözle
    kontrolün sahnesi üçünü de sayar.

## Yaklaşım

1. Tarama, `bt-atlas`'ın içinde bir `#[ignore]` sınamasıdır. `Atlas::slot`'un
   karar yolunu değil kapının kendisini (`font::fallback_font`'un adımlarını)
   karakter başına çağırır ve sınıflamayı bir tablo olarak basar. Sonuçlar
   phase-1'in Uygulama Notları'na özet olarak girer (grup başına sayı,
   dönenlerin oran dağılımı, `.LastResort`'un yeri).
2. Bekçi listesi aynı modülde bir `const` dizisidir ve normal bir sınamadır.
3. phase-2, `font::accept`'e üçüncü bir kol ekler: iki kapı da reddettiyse
   oran = kutu / mürekkep hesaplanır. Oran sınırın içindeyse adayın küçük
   puntolu kopyası kurulur ve `ink_fits_box` ile **yeniden** sınanır.
   Seçim ve gerekçe discussion.md → Karar 5'te.

## Kapsam Dışı

- Yeni font ya da gömülü sembol fontu dağıtmak.
- Grapheme dizilerinin (`shape_cluster`) küçültülmesi. Aynı `accept`'ten
  geçtikleri için kol onlara da uygulanır, ama tarama onları saymaz.
- Bağlam satırının küçük boy sınıfı için ayrı bir tarama. Kapı sınıf başına
  zaten ayrı koşuyor.
- Dikeyde taşan adayın kırpılması (bugünkü sınır, `font::ink_fits_box`'un
  doc'u).
- Linux font yığını (`docs/YOL-HARITASI.md` → font sistemi soyutlaması).

## Akış

```
aday (cascade) ──► tek hücre sığar? ── evet ──► bugünkü yol (bit bit aynı)
                        │ hayır
                        ▼
               iki sütun ve iki hücre sığar? ── evet ──► bugünkü geniş yol
                        │ hayır
                        ▼
               .LastResort değil ve mürekkep/kutu ≤ sınır?  ── hayır ──► kutu
                        │ evet
                        ▼
               küçük puntolu kopya ──► ink_fits_box (yeniden) ──► çiz
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | |
| kapı | |
