# Geçmişte arama (⌘F)

## Hedef

⌘F sağ üstte temaya uyan bir arama paneli açar; yazdıkça bütün geçmişteki
eşleşmeler vurgulanır, "3 of 17" sayılır, ⏎/⌘G ile eşleşmeler arasında
pencere süzülerek gezilir, Esc paneli kapatıp eşleşmeyi seçim olarak bırakır.
Düz metin ya da regex, akıllı büyük/küçük harf. PTY boyutu, render yolu ve
boşta sıfır kare bozulmaz.

## Gereksinimler

- **R1 — Sorgu.** Düz metin (metakarakterler kaçırılır) ya da regex; `Aa`
  kapalıyken akıllı, açıkken duyarlı; geçersiz regex panik değil durum; boş
  sorgu ve boş eşleşme vurgu üretmez (`discussion.md` → Karar 11).
- **R2 — Vurgu.** Arama etkinken her içerik karesinde görünür satırların
  eşleşmeleri — ızgara, doldurma bandı, kesrin tepe satırı — `search_match`,
  geçerli eşleşme `search_current` ile, 031'in yuvarlak köşeli şekliyle ve
  eşleşme başına köşeyle çizilir; metin kendi ön planında (Karar 7, 8).
  - **R2.1** — Bastırılan giriş satırı ve dock vurgu, sayım ve gezinmenin
    dışında; alternatif ekranda görünür ızgara aranır (Karar 8).
  - **R2.2** — Hareket karesinde tarama yok; panel kapalı ya da sorgu boş/
    geçersizken tarama yok (durma koşulu).
- **R3 — Tema.** `search_match` ve `search_current` rolleri, eksikse gömülü
  tabandan; iki gömülü temada değer; odaksız pencerede zemine doğru soluklaşma
  (031 Karar 9'un kuralı); `docs/AYARLAR.md` → Temalar.
- **R4 — Panel.** İçerik view'ı kapsayıcı, `BateriView` çocuğu; panel sağ
  üstte yüzen `NSSearchField` + `Aa` + `.*` + sayım + iki ok + kapatma; PTY
  boyutu değişmez; açılış/kapanış animasyonu Hareketi Azalt'ta yok (Karar 1,
  7).
  - **R4.1** — İçerik view'ının kapsayıcıya dönüşü tek başına davranış
    değiştirmez: first responder, sürükleme hedefi, geometri, fare eşlemesi
    bugünkü gibi.
- **R5 — Klavye ve menü.** Edit ▸ Find ▸ Find…/Find Next/Find Previous/Use
  Selection for Find (⌘F/⌘G/⇧⌘G/⌘E); alanda ⏎/⇧⏎ gezinir, Esc kapatır;
  Cmd izin listesi üç tuşta kalır; ⌘A/⌘C/⌘V/⌘X alandayken alana gider (Karar
  6, 10).
- **R6 — Gezinme.** ⏎ yukarı (daha eski), sarar; görünür (panelin altında
  değil) eşleşmede pencere oynamaz, değilse eşleşme ortalanır; bir ekran
  içinde süzülür, uzakta son ekran süzülür; `smooth_scroll`/Hareketi
  Azalt/`snap` anında; Esc pencereyi yerinde bırakır ve geçerli eşleşme seçim
  olur (Karar 3, 4, 5).
- **R7 — Odak.** Alan odaktayken terminal caret'i odaksız görünür; vurgu ve
  seçim yalnız pencere key değilken soluklaşır (Karar 7, Muhakeme).
- **R8 — Sayım.** Bütün defterin sayımı ve geçerli eşleşmenin sırası (en
  yeni = 1); dizin çıpasız, dipten yukarı parça parça, sorgu ya da defter
  değişince baştan; sürerken "…"; parçalar kare istemez; geçerli eşleşme akan
  çıktıda içeriğine yapışık kalır ya da en yakına geçer (Karar 2, 9,
  Muhakeme).

## Yaklaşım

1. `bt-core`'a arama çekirdeği: sorgunun derlenmesi, kaçırma, görünür
   eşleşmelerin `frame()`'in `Term` turunda koşulara çevrilmesi (ızgara +
   fill-yerel), bastırmanın tek yükleminden dışlama. Desen kopyası kilitsiz
   sahipli (yaprak yuvadan `Term`'den önce alınır, sonra geri konur).
2. Tema rolleri ve `bt-gpu`'nun çizimi: `selection` pipeline'ına ızgara ve
   bant viewport'unda ikişer encode, eşleşme başına köşe.
3. İçerik view'ı kapsayıcıya döner, davranış değişmeden.
4. AppKit paneli, menü, klavye, odak biti, gezinme (`search_next` + 027'nin
   süzülmesi), Esc→seçim, ⌘E ve find panosu; etiket bu phase'de görünür
   eşleşmeleri sayar.
5. Parça parça sayım dizini, `Wake` haberiyle tazelenmesi, geçerli
   eşleşmenin kayması; etiket "3 of 17".

## Kapsam Dışı

`discussion.md` → Karar 12: dock'ta arama, sekmeler arası arama,
bul-değiştir, son aramalar menüsü, anahtarların kalıcılığı ve ayar anahtarı,
URL/yol algılama, doldurma bandında seçim çizimi. Metal'de çizilen arama
alanı ve IME preedit borcu.

## Akış

```
⌘F ─► TerminalWindow.findInScrollback: ─► panel görünür, alan first responder
                                          DisplayLink: klavye terminalde değil
yazım ─► controlTextDidChange ─► Session::set_search(query, regex, case)
          │                        nesil++ , desen derlenir (geçersiz → durum)
          ├─► Waker::wake ─► içerik karesi ─► frame(): görünür satırlarda RegexIter
          │                                     → SearchRuns (ızgara + fill-yerel)
          │                                     → bt-gpu: search_match / search_current encode
          └─► ana kuyruk: Session::search_step() … (parça, nesil eskiyse düşer)
                                ─► etiket "3 of 17…" → "3 of 17"
Wake haberi (defter değişti, arama açıkken, kenarda) ─► sayım baştan
⏎ / ⌘G ─► Session::search_next(yukarı) ─► geçerli eşleşme + reveal
            görünürse no-op │ ≤ ekran: glide │ uzak: konup son ekran glide
Esc ─► panel kapanır, pencere yerinde, geçerli eşleşme → seçim, klavye terminale
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| phase-4 | ✅ |
| phase-5 | ✅ |
| kapı | ✅ |
