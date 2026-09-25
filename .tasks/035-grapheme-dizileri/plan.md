# Grapheme dizileri

## Hedef

Bayrak (`🇹🇷`), ZWJ (`👨‍👩‍👧`), ten rengi (`👍🏽`) ve VS16 (`❤️`, `☺️`, `🌡️`)
dizileri ızgarada, doldurma bandında ve dock'ta **tek glyph** olarak ve
uygulamanın saydığı **sütun sayısıyla** (2) çiziliyor. Claude Code'un
satırları bu dizilerde kaymıyor. Karar ve gerekçe `discussion.md` → Karar.

## Gereksinimler

- **R1** — Atlas bir dizgiyi tek yuva (ya da geniş çift) olarak çiziyor:
  `Sprite::Cluster`, atlasın kendi interner'ı, `CTLine` ile şekillendirme,
  aynı mürekkep kapısı ve renk düzlemi.
  - **R1.1** — Tek glyph'e şekillenmeyen ya da kapıdan dönen küme **taban
    karakterin** glyph'iyle çiziliyor (bugünkü görüntü), kırpılmış glyph yok.
- **R2** — Okuyucu döngünün sahibi `bt-core`; davranış bayt bayt bugünkü.
  - **R2.1** — `Handler` aktarımı tek makro listesinden ve
    `clippy::missing_trait_methods` ile bekçili; DEC 2026 zaman aşımı
    (`stop_sync`) ve `Wakeup` kuralı aynı yoldan.
  - **R2.2** — Kapanış (`SHUTDOWN_GRACE`, `kapanis=`) ve yarış sınamaları
    değişmeden yeşil; `alacritty_terminal` `=0.26.0`; kopyada Apache-2.0
    bildirimi.
- **R3** — Izgara emoji dizisini tek hücrede kümeliyor (oturum seçeneği,
  bu phase'lerde varsayılan kapalı).
  - **R3.1** — Kural tek saf fonksiyon, dört emoji kolu, genişlik
    `UnicodeWidthStr::width`; ızgara yalnız 1 → 2 genişletir. Arapça `لا`,
    VS15 ve eşlenmemiş RI bugünkü gibi.
  - **R3.2** — Genişleme `Term::input` üstünden: son sütun, IRM ve kaydırma
    bölgesinin dibi alacritty'nin kendi davranışıyla.
  - **R3.3** — Baş hücre ızgaradan türetiliyor; araya giren başka bir
    `Handler` çağrısı kümeyi kapatıyor; resize ile yarışan bir `race_*`
    sınaması yeşil.
  - **R3.4** — Dock düzeni (`layout_with`, `needed_rows`), bastırmanın
    ızgara yürüyüşü (`grid_span`) ve tazelik kapısı aynı fonksiyondan
    kümeyle yürüyor.
- **R4** — Üç yüzey ve yazım efektleri kümeyi çiziyor; dock'un düzenlemesi
  kümeyi bölmüyor.
  - **R4.1** — Sınır `Cell`'i küme indeksi taşıyor, tablo `bt-gpu`'nun ve
    listelerle yaşıyor; ızgara, doldurma bandı, dock ve `prepare_fx`.
  - **R4.2** — Dock seçimi, `d;S;E` aralığı ve yazım efektlerinin farkı küme
    sınırına hizalı; ızgara seçiminin kopyası kümenin tamamını veriyor.
  - **R4.3** — Düzenleme kapısı açıkken ⌫/⌦/←/→ kümeyi bütün yürütüyor
    (widget komutu); kapı kapalıyken kod noktası birimi.
- **R5** — Kümeleme varsayılan açık; sözleşme ve yol haritası güncel.

## Yaklaşım

1. **Atlas** kümeyi dizgi anahtarıyla çizmeyi öğrenir; kimse henüz çağırmaz
   (P1).
2. **Okuyucu döngü** `bt-core`'a geçer, `Term` aktaran sarmalayıcının
   arkasına girer; `input` henüz aynen aktarılır (P2).
3. **Kümeleme** sarmalayıcının `input`'una ve `bt-core`'un iki düzen
   yürüyüşüne girer, oturum seçeneği arkasında; sınamalar onu açar (P3).
4. **Çizim ve düzenleme**: sınır, `bt-gpu`'nun küme tablosu, üç yüzey,
   efektler ve dock düzenlemesinin hizası, aynı seçeneğin arkasında (P4).
5. **Açılış**: varsayılan açık, belgeler (P5).

## Kapsam Dışı

- Çıplak 78 tek sütunlu emoji (`🌡` VS16'sız) — küçültme kararı, yol
  haritasının kalemi (`.tasks/023-emoji-ve-genis-glyph/discussion.md` →
  Karar, madde 2).
- Eşlenmemiş tek RI (kutu kalıyor), dock'un bağlam satırı (küçük sınıf,
  karakter birimi — 021/024 emsali).
- Emoji dışı UAX #29 kümeleri (Arapça lam-elif, Hangul jamo, Hint
  SpacingMark) — bugünkü davranış.
- DECSET 2027 / DECRQM.
- Aramanın kümenin ikinci kod noktasını görmesi (`RegexIter` alacritty'nin).
- Tek sütunlu birleştiricinin (aksan, hareke, `⌚︎`) küme olarak çizilmesi —
  bugünkü gibi taban karakter.

## Akış

```
PTY ─ TappedPty (tarama, aynen) ─ bizim döngü ─ Processor ─ ClusterHandler
                                                              │ input(c):
                                                              │  uzatmıyor → Term::input(c)
                                                              │  uzatıyor, genişlik aynı → push_zerowidth
                                                              │  uzatıyor, 1→2 → geri al + Term::input(yer tutucu) + hücreyi yaz
                                                              │ diğer 65 metot → kümeyi kapat + Term'e aktar
Term.grid: tek geniş hücre, c = taban, zerowidth = kalan
   │
Session::frame / dock / doldurma ─→ Cell { ch: taban, cluster: Some(i) } + bt-gpu küme tablosu[i] = "🇹🇷"
   │
bt-gpu prepare/fan ─→ atlas.intern("🇹🇷") → Sprite::Cluster(k) ─→ CTLine → tek glyph → Left/Right yuva
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | |
| phase-2 | |
| phase-3 | |
| phase-4 | |
| phase-5 | |
| kapı | |
