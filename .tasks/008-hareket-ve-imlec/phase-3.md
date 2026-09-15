# Phase 3 — Hareket altyapısı ve imlecin kayması

## Özet

`bt-gpu`'ya saf bir hareket modülü girer; link "hasar yok" dalında yerleşmemiş
hareket varken uyumaz ve imleç hücreler arasında kayar. Kapı aynı commit'te
animasyonun **durduğunu** sorar.

_Requirements: R3.1, R3.2, R3.3, R3.4, R4_

## Değişiklikler

- **`crates/bt-gpu/src/motion.rs` (yeni)** — saf, ObjC'siz, kilitsiz; emsali
  `Gate` ve `FailureStreak`. Tuttuğu şey **hücre biriminde** konum ve hız,
  hedef hücre ve stil. Üç soru: `advance(dt)`, `position()`, `settled()`.
  Durma koşulu **iki katlı**: konum+hız eşiği **veya** süre tavanı (seçilmiş
  bir üst sınır, ölçülmüş değil — doc'u bunu söyler). `dt` kırpılır: örtülme
  kalkınca iki damga arası sınırsız olabilir ve kırpılmazsa süre tavanı vahşi
  kareyi önlemek yerine ondan sonra ateşler. Bu phase'de stil sabit kodlu
  `spring` (seçim phase-4'te). Saat, phase-1'in `sessiz` damgasıyla **aynı
  taban** olmalı (display link'in damgası); iki taban iki ayrı zaman
  yaratırdı.
- **`crates/bt-gpu/src/link.rs`** — `needs_update` üç yol: hasar varsa içerik
  karesi (bugünkü yol + hedefin hareket saatine bildirilmesi), hasar yok ama
  hareket yerleşmediyse **hareket karesi**, ikisi de yoksa `setPaused(true)`.
  Hareket karesi `Waker`'a **dokunmaz** — link zaten uyanıkken kendini
  sürdürür; `wake()` hasarı koşulsuz diktiği için oradan istenen bir kare
  kendini "içerik" diye saydırırdı. `frame.clear()` artık yalnız içerik
  karesinde. Modül başlığı ve `Waker`'ın "kare istemenin tek tanımı" doc'u
  bu ikinci gerekçeyi yazar: **zamana bağlı kare talebinin tek yolu hareket
  saatidir**. `Waker::requests`'in ve `app.rs`'in ölçülmüş `istek ≈ kare + 2`
  cümlesi artık geçerli değil, ikisi de düzelir.
- **`crates/bt-gpu/src/frame.rs`** — `push_cursor` ara konumu `f32` alır
  (`pos()` bugün `u16`). Hareket karesi için listeyi **koruyan** bir uç:
  arka plan listesi `bg_count`'a kırpılır, imleç yeniden eklenir; glyph ve
  kural listelerine dokunulmaz. `Frame`'in "her kare `clear` ile yeniden
  doldurur" cümlesi düzelir; `push`'un `debug_assert`'i (imleçten sonra arka
  plan eklenmesin) kırpma sayesinde geçerli kalır.
- **`crates/bt-core/src/session.rs`** — hasar sorusu `frame()`'in içinden
  çıkar: çağıran önce "hasar var mı" diye sorar (`Session`'a yeni bir uç),
  `frame()` koşulsuz tarar. Sebep: hareket karesinde listeyi **temizlemeden**
  önce karar verilmeli. `frame()`'in ve hasar bayrağının doc'ları bu ayrımı
  yazar.
- **Snap hâlleri** — ilk kare, `visible` kapalıyken açılan imleç, geometri
  (`DisplayLink::resize`: pencere, font, zoom) ve geçmişte kaydırma hareketi
  atlar: imleç hareket etmedi, altındaki ızgara hareket etti. Durum hücre
  biriminde tutulduğu için hücre ölçüsü değişimi kendiliğinden doğru yere düşer.
- **`crates/bt-shell/src/app.rs`** — `Verdict::MotionUnsettled`: deadline'da
  yerleşmemiş animasyon varsa koşu kırmızı, kendi tanı iletisiyle. **Yalnız
  `Smoke` kolunda**, `hareket > 0` gerekliliğiyle aynı cümlede: `Load` tam
  deadline'a kadar çıktı basıyor, yani son satırla birlikte imleç hedef
  değiştiriyor ve deadline yayın ortasına düşüyor — o kola bağlansaydı her
  ölçüm koşusu kod doğruyken kırmızı düşerdi (bugünkü `ExcessFrames`
  muafiyetinin aynı gerekçesi: orada kare akışı işin kendisi). Jeton satırı
  değişmez — karar `verdict`'te, çünkü jeton satırı yalnız yeşil koşuda
  basılıyor.
- **`crates/bt-core/src/session.rs` → `smoke_shell`** — reçeteye bir uykudan
  sonra imleci taşıyan tek bir dizi eklenir. Sayaçlar korunur (`hucre=8
  glif=6 kural=15`); `sleep` printf'ten **sonra** kalmalı, yoksa üç sınamanın
  ilk karesi gecikir. **Uyku cömert seçilir (≥ 1 s) ve gerekçesi doc'a
  yazılır:** açılış süresi (`acilis=`) **ölçülmedi** ve ilk içerik karesi
  imleç hareketinden sonra düşerse imleç zaten hedefte doğar, hareket hiç
  başlamaz ve `hareket > 0` gerekliliği kod doğruyken kırmızı düşer —
  `kare=1↔2` oynaması bu hizanın bugün bile kararsız olduğunu söylüyor. Üç
  saniyelik koşuda 1 s uyku + ~200 ms yerleşme, `sessiz`e ≥ 1,5 s kuyruk
  bırakır; payı phase-6'nın ölçümü doğrular. Doc'un "ikinci bir printf yok"
  cümlesi gerekçesiyle birlikte yeniden yazılır.
- **Kapının gizli bağı yazılır** — `hareket > 0` gerekliliği hermetik koşunun
  stilinin **animasyonlu** olmasına dayanıyor; bugün tutuyor çünkü varsayılan
  `spring` (phase-4). Varsayılan bir gün `snap` olursa kapı sessizce düşer:
  ya hermetik koşunun stili koşuda açıkça sabitlenir ya bu bağ kapının
  doc'unda adıyla durur.

## Kabul

- `make duman` yeşil: `hareket > 0`, `icerik ≤ IDLE_FRAME_LIMIT`, `sessiz`
  koşunun boşta geçen kuyruğu kadar büyük.
- Durma koşulu bozulursa (geçici mutasyon: `settled` hep `false`) koşu
  `MotionUnsettled` ile kırmızı düşer — hızdan bağımsız.
- `motion` birim sınamaları: her başlangıç durumundan sonlu adımda yerleşir;
  süre tavanı devreye girer; uçuşta hedef değişince (retarget) hız korunur;
  kırpılmış `dt` tek adımda ışınlamaz.
- Gözle: ok tuşlarıyla metin üstünde gezinirken blok harflerin üstünden
  geçiyor, harf kaybolmuyor (phase-2'nin verdiği davranış).

## Yayın Etkisi

shader yok · terminfo yok · ayar şeması yok (anahtar phase-4'te) · tema yok ·
shell entegrasyonu yok · app bundle yok · yeni bağımlılık yok.

`CLAUDE.md`'nin "Boşta sıfır kare. **Kirli satır yoksa frame gönderilmez**"
cümlesi bu commit'te düzelir: hareket, kirli satır olmadan kare çizen ilk şey
ve her animasyon bir durma koşulu taşıyor. `docs/OLCUMLER.md`'nin duman
reçetesi ve sabit jetonları phase-6'da yenilenir — ölçüm bekliyor: yeni
reçetenin sağlıklı dağılımı (R7).

## Checklist

- [ ] `motion.rs`: konum/hız, `settled`, süre tavanı, `dt` kırpma
- [ ] `link.rs`: üç yol, `clear` içerik karesine, `Waker`'a dokunmama ve iki
      sözleşme cümlesinin güncellenmesi
- [ ] `frame.rs`: `f32` konum + liste koruma; doc düzeltmesi
- [ ] `session.rs`: hasar sorusu dışarı, `frame()` koşulsuz tarar
- [ ] Snap hâlleri (ilk kare, görünürlük, geometri, kaydırma)
- [ ] `app.rs`: `MotionUnsettled` + `hareket > 0` gerekliliği
- [ ] `smoke_shell`: imleç hareketi + doc
- [ ] Test: motion birim sınamaları; bozuk durma koşulu kapıyı kırmızı düşürür
- [ ] Doğrulama geçti (`make hepsi` + `make duman` + `make test-yaris`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
- [ ] Yayın etkisi yazıldı
