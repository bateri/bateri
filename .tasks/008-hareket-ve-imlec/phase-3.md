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
cümlesi bu commit'te düzeldi: hareket, kirli satır olmadan kare çizen ilk şey
ve her animasyon bir durma koşulu taşıyor. Aynı commit'te güncellenen öteki
belgeler: `CLAUDE.md`'nin duman jeton satırı ve kırmızı koşulu, `Makefile`'ın
`duman` yorumu (`hareket` gerekliliği + jetonda görünmeyen `MotionUnsettled`
kapısı), `.claude/is-akisi/proje.md`'nin doğrulama tablosu, `link.rs`'in modül
başlığı ile `Waker`/`requests` doc'ları, `app.rs`'in `IDLE_FRAME_LIMIT` ve
`Measured` doc'ları, `frame.rs`'in `Frame` doc'u, `session.rs`'in `frame()`,
`Cursor` ve `smoke_shell` doc'ları.

**Ölçüm bekliyor (R7, phase-6):** yeni duman reçetesinin sağlıklı dağılımı,
`sessiz=`'in eşiği (`T`) ve `IDLE_FRAME_LIMIT`'in yeniden türetmesi. Bu
phase'in duman koşusu tek bir gözlem, taban değil.

**Ölçüm bekliyor (bu phase'in kendi borcu):** `istek ≈ icerik + 2` — eski
`istek ≈ kare + 2` ölçümü hareket kareleriyle çürüdü, yerine yazılan ilişki
tek koşuluk bir gözlem ve üç doc'ta öyle işaretli.

## Checklist

- [x] `motion.rs`: konum/hız, `settled`, süre tavanı, `dt` kırpma
- [x] `link.rs`: üç yol, `clear` içerik karesine, `Waker`'a dokunmama ve iki
      sözleşme cümlesinin güncellenmesi
- [x] `frame.rs`: `f32` konum + liste koruma; doc düzeltmesi
- [x] `session.rs`: hasar sorusu dışarı, `frame()` koşulsuz tarar
- [x] Snap hâlleri (ilk kare, görünürlük, geometri, kaydırma)
- [x] `app.rs`: `MotionUnsettled` + `hareket > 0` gerekliliği
- [x] `smoke_shell`: imleç hareketi + doc
- [x] Test: motion birim sınamaları; bozuk durma koşulu kapıyı kırmızı düşürür
- [x] Doğrulama geçti (`make hepsi` + `make duman` + `make test-yaris`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi
- [x] Yayın etkisi yazıldı

## Uygulama Notları

Sapmalar; planın söylemediği ya da başka türlü öngördüğü yerler.

- **Kaydırma snap'ı sınırdan yeni bir alan istedi.** `bt-gpu` "imleç mi
  hareket etti, ızgara mı kaydı" sorusunu `Cursor.row`'dan **ayırt edemiyor**:
  geçmişe üç satır kaydırmak `row`'u üç artırır, tıpkı üç kez enter'a basmak
  gibi. `Cursor`'a `display_offset` eklendi ve `Motion` onu iki kare arasında
  karşılaştırıyor. `Cursor`'un kendi doc'u bunu zaten öngörüyordu ("konuma
  güvenen ilk tüketici sözleşmeyi genişletmeli"); genişleten tüketici IME
  değil hareket oldu.
- **Hareket karesi ölçüm örneği yazmıyor.** `cpu_kare` `session.frame`'in
  kilit beklemesini ölçüyor ve hareket karesinde o iş hiç yok; bir `truncate`
  + `push_cursor`'un mikrosaniyesi aynı sütuna girseydi p95'i aşağı çekerdi —
  boştaki karenin "encode = 0 ns sahte örnek" yasağının aynısı. `sessiz=`'in
  damgası ise **yazılıyor**: hareket karesi de yola çıkan bir kare ve kuyruk
  yerleşmeden sonra başlamalı.
- **Yay Euler değil kapalı form.** Kritik sönümlü çözümün analitik hâli her
  `dt` için doğru; Euler'de kararlılık `OMEGA * dt`'ye bağlı olurdu ve `DT_MAX`
  o zaman bir kemer değil bir **şart** olurdu. Kırpma yine duruyor, ama artık
  yalnız süre tavanı için.
- **`Session::frame`'in `Option`'ı kalkınca 64 sınama çağrısı kırıldı.**
  Hepsi hasarı `is_some()/is_none()` ile soruyordu. Üretimde ikisi ayrı
  (`take_damage()` + `frame()`), sınamalarda bir test yardımcısı
  (`frame_if_damaged`) eski şekli koruyor — yoksa her sınama aynı iki satırı
  kopyalar ve hasarı iki kez tüketen bir sınama kendini sessizce yeşile
  çevirirdi.
- **Ölçülmüş bir cümle çürüdü:** `istek ≈ kare + 2`. Hareket kareleri
  `Waker`'a dokunmadığı için `kare` o ilişkiden koptu; sağlıklı duman koşusu
  artık `kare=26` iken `istek=4` veriyor. Üç doc düzeltildi ve yerine yazılan
  `istek ≈ icerik + 2` **ölçülmüş bir iddia değil**, tek koşuluk bir gözlem
  diye işaretlendi (phase-6 ölçecek).
- **Duman koşusu (bu makine, debug):** `kare=26 hucre=8 glif=6 kural=15
  istek=4 icerik=3 hareket=23 sessiz=1745.97ms kapanis=clean`. Kabulün
  istediği üç şey de yerinde: `hareket > 0`, `icerik ≤ 8`, kuyruk ≥ 1,5 s.
  Yirmi üç kare ≈ 190 ms, `OMEGA`'nın ~220 ms hedefiyle uyumlu.
- **Mutasyon koşuldu** (`settled()` → `false`): koşu `MotionUnsettled` ile
  kırmızı düştü, `hareket=353`, `icerik` sınırı **aşılmadan**. İki kapının
  gerçekten bağımsız olduğunun kanıtı — yavaş bir sızıntıyı `icerik` görmezdi.
- **Gözle kontrolü kullanıcıdan geçti**: imleç kayıyor, blok harflerin
  üstünden geçiyor, harf kaybolmuyor.

### `/code-review` bulguları (beşi de işlendi)

1. **Uçuşta hedef değişimi hedefi aşıyordu** — ζ = 1 "taşma yok"u yalnız
   durgun hâlden veriyor; `sync` hızı bilerek koruduğu için uzun bir
   sıçramanın ortasındaki küçük bir düzeltme **0,87 hücre** taşıyordu, yani
   008 Karar 6 ile açık çelişki. `advance`'a eksen başına taşma kırpması
   girdi (`a_retarget_in_flight_does_not_overshoot`; kırpma kaldırılınca
   sınama kırmızı düşüyor, doğrulandı).
2. **"GPU deltası hareket karesinden etkilenmiyor" yorumum yanlıştı** —
   tamamlanma bloğu komut tamponuna bağlı ve hareket karesi de commit
   ediyor, yani `record_gpu` onları görüyor. Ayrılık kasıtlı değil
   **yapısal** (blok `FailureStreak`'i de besliyor). Yorum düzeltildi ve
   sonucu bir ölçüm kapsamı kalemi olarak yazıldı: `ornek=` ile `gpu_ornek=`
   farklı kare popülasyonu sayıyor, p95'leri imleç kayan bir koşuda doğrudan
   karşılaştırılamaz → phase-6'nın `## Yöntem`'ine.
3. **Süre tavanının payı belgelendiği kadar geniş değildi** — eşikler mutlak
   olduğu için yerleşme süresi `ln(mesafe)` ile büyüyor: 400 hücrelik bir
   sıçrama ~460 ms, eski tavan `0.5` idi, yani meşru bir kaymayı kesmeye
   %10 kalmıştı ve belirtisi görünür bir snap olurdu. Tavan `0.7`'ye çıktı,
   `OMEGA`'nın doc'u mesafe bağımlılığını yazıyor
   (`even_the_longest_jump_settles_before_the_ceiling`).
4. **Örtülen pencere kapıyı yanlış kırmızı düşürebiliyordu** — kayma
   ortasında link duruyor, `advance` bir daha koşmuyor ve deadline
   "bir durma koşulu bozuk" diyordu, kod doğruyken. `Motion::finish()` eklendi
   ve `set_visible(false)` onu çağırıyor; snap politikasının zaten söylediği
   şey (görünürlük dönüşü animasyonsuz) bir kare erken uygulanıyor.
5. **`MotionUnsettled` ile kapanış paniği birlikteyken sıra sınanmamıştı** —
   yeniden sıralanmadı (yalnız bu kolu paniğin üstüne almak `ExcessFrames`'in
   bugünkü sırasını bozardı) ama gerekçe `verdict`'e yazıldı ve kombinasyon
   sınamaya bağlandı (`motion_and_panic_report_the_more_fundamental_fault`).
   Koşu her iki hâlde de kırmızı; mesele yalnız hangi tanının basıldığı.
