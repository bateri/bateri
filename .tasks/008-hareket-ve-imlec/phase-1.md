# Phase 1 — Kapının muhasebesi ve üç yeni jeton

## Özet

Duman kapısı GPU sayacı yerine `needs_update`'te artan içerik sayacına
bağlanır; satır `icerik=`, `hareket=` ve `sessiz=` jetonlarını kazanır —
hareket kodu **inmeden önce**, davranış değişmeden.

_Requirements: R1.1, R1.2, R1.3_

## Değişiklikler

- **`crates/bt-gpu/src/link.rs`** — `LinkIvars`'a ana thread'e bağlı iki sayaç
  (`Cell<u64>`): içerik karesi (`session.frame()` `Some` döndü) ve hareket
  karesi (bu phase'de hep `0`). Yanına son çizilen karenin damgası:
  `update.targetTimestamp()` — **alan kopyası**, saat okuması değil, yani
  R4.1'in "kapı kapalıyken tek bir saat okuması bile yok" cümlesine
  dokunmuyor. `DisplayLink` üçünü de dışarı verir (`content_frames`,
  `motion_frames`, ve deadline'da `CACurrentMediaTime()` ile farkı veren bir uç).
  Damga **çizilen** karede alınır: encode edilemeyen kare sessizlik saymaz.
- **`crates/bt-shell/src/app.rs`** — `Counters`'a `content` alanı (dördü gibi
  yapıda, konumsal değil). `Report` `motion` ve `quiet` taşır; `token_line`
  sırayla `icerik=`, `hareket=`, `sessiz=` basar. `sessiz=` değeri `ms()`'in
  biçiminde; hiç kare çizilmediyse **`sessiz=none`** (`ornek=off` emsali —
  uydurulmuş bir sıfır değil, çünkü sıfır "hemen şimdi kare çizildi" demek).
  `verdict` `Smoke` kolunda `frames` yerine `content ≤ IDLE_FRAME_LIMIT`
  sorar; `Load` kolu değişmez. `ExcessFrames` iletisi hangi sayıyı
  söylediğini güncellemeli.
- **`crates/bt-shell/src/app.rs` → `IDLE_FRAME_LIMIT` doc'u** — sınırın
  **operandı** değişti: artık GPU'nun bitirdiği kare değil, çizilmeye karar
  verilen içerik karesi. Ölçülmüş iki dağılım (005, 006) geçerliliğini
  koruyor, çünkü `icerik ≤ kare`: eski sınırdan geçen her sağlıklı koşu yeni
  ifadeden de geçer, yani bu commit yanlış pozitif üretemez ve ölçüm
  beklemez. "O set açıldığında kapı ya süreye ya `istek=`'e bağlanmalı"
  cümlesi düşer; yerine bu setin iki katlı kapısı yazılır.
- **`Makefile` → `duman` yorumu**, **`.claude/is-akisi/proje.md`** → doğrulama
  tablosunun duman satırı, **`CLAUDE.md`** → `make duman` jeton satırı: kapının
  yeni ifadesi ve üç yeni jeton. Jeton sözleşmesi "silinmez, eklenir" —
  `kare=` yerinde ve anlamı aynı kalır (GPU'nun hatasız bitirdiği kare).

## Kabul

- `make duman` yeşil ve satır şu şekli taşır: `kare=N hucre=8 glif=6 kural=15
  … icerik=N' hareket=0 sessiz=…ms …`. `icerik` bugünkü `kare` ile aynı
  mertebede (1–2), `hareket=0`.
- Sınamalar: `token_line_preserves_old_tokens` eski jetonların hepsini
  görmeye devam eder ve üç yenisini de sorar; `idle_limit_catches_excess_frames`
  içerik sayacı üstünden kurulur; `sessiz=none` kolunun kendi sınaması olur.
- `verdict`'in `Load` kolu ve `MissingCounter` iletisi davranış olarak
  değişmez.

## Yayın Etkisi

shader yok · terminfo yok · ayar şeması yok · tema yok · shell entegrasyonu yok
· app bundle yok.

**Yeni bağımlılık yok, ama `Cargo.toml` değişti:** `objc2-quartz-core`'un
`CABase` feature'ı bt-gpu'da açıldı (`CACurrentMediaTime`). Mevcut paketin
özelliği, `Cargo.lock` oynamadı; `make denetim`'in uyarısının karşılığı bu.

Jeton sözleşmesi genişledi: üç yeni anahtar kalıcıdır, bu yüzden adları ve
anlamları bu commit'te doğru olmalı. `docs/OLCUMLER.md`'nin "Sabit jetonlar"
bloğu artık eksik kalıyor; yeni satır biçimi ve sayıların türetmesi
**phase-6'nın ölçümünde** yenilenir — ölçüm bekliyor: `sessiz=` dağılımı ve
`IDLE_FRAME_LIMIT`'in yeni türetmesi (R7).

## Uygulama Notları

- **Eşitsizlik plan dosyasında tersti; doğrusu `kare ≤ icerik`.**
  `renderer.frames` tamamlanma bloğunda artıyor (`renderer.rs:426`), `icerik`
  ise `Some(cursor)` anında — yani hatasız biten her kare bir içerik
  karesiydi, tersi değil. "Ölçüm beklemez" sonucu **duruyor** ama gerekçesi
  başka: yeni ifade eskisinden *daha sıkı* ve sıkılaşma ölçülmüş paya
  giriyor — sağlıklı koşuda ikisi eşit (encode hatası yok, `FailureStreak`
  zaten en çok bir yeniden deneme veriyor; rapor `shutdown()`'dan sonra
  koşuyor ve o `link.stop()` çağırdığı için uçuşta kare kalmıyor), en kötü
  bir karelik kayma ve ölçülen tavan `4` ile sınır `8` arasındaki pay onu
  yutuyor. Dört duman koşusunda dördünde de `kare == icerik` (2, 2, 2, 1).
  `IDLE_FRAME_LIMIT` doc'una doğru yönüyle yazıldı.
  *(Sonradan çürüdü — phase-3 hareket karesini getirince "hatasız biten her
  kare bir içerik karesiydi" yanlışlandı: hareket karesi de commit ediyor,
  yani `kare`'yi artırıp `icerik`'i artırmıyor. Ölçülen sağlıklı koşu
  `kare` 27–30 iken `icerik` 2–3. Sınır bu yüzden taşınamadı, phase-6'da
  **yeniden ölçüldü**; iki doc kapı commit'inde düzeltildi.)*
- **`sessiz=` saati kapanıştan önce okunuyor**, planın söylemediği bir yer.
  `shutdown()` `SHUTDOWN_GRACE` kadar (yarım saniye) bekliyor ve ölçüm
  koşularının dörtte birinde gerçekten bekliyor (`kapanis=abandoned`); sonra
  okunsaydı jeton "son kare → deadline" değil "son kare → kapanışın sonu"
  olur ve phase-6'nın dağılımına kapanış değişkenliği karışırdı. **İki**
  rapor yolu da düzeltildi: `runDeadline:` ve `applicationWillTerminate:`
  (shell erken çıkarsa rapor oradan basılıyor).
- **Damga bir *hedef sunum* anı**, yani gelecekte bir nokta: deadline son
  kareden bir tazeleme içinde düşerse fark negatif çıkar. Değer sıfıra
  doyuruluyor ve `quiet_since`'in doc'u `sessiz=0.00ms`'in "deadline anında
  kare akıyordu" diye okunacağını söylüyor.
- **`objc2-quartz-core`'a `CABase` feature'ı eklendi** (bt-gpu). Yeni
  bağımlılık değil, mevcut paketin `default-features = false` yüzünden kapalı
  kalan bir özelliği; `CACurrentMediaTime` display link damgasıyla aynı zaman
  tabanını veren tek okuma ve `Instant`'ın tabanı belgelenmiş değil.
  `Cargo.lock` **değişmedi** (feature kilide yazılmıyor); `make denetim`'in
  `Cargo.toml` uyarısının karşılığı bu satır.
- **`sessiz=none` jeton satırında erişilemez:** kare çizilmemişse `kare=0` ve
  kapı `MissingCounter` diyor, satır hiç basılmıyor. Sınama bu yüzden
  `Report` üstünden kuruldu, koşu üstünden değil.
- Ölçülen `sessiz=` dört koşuda 2947–2958 ms (3 sn'lik koşu), yani son kare
  ~42–53 ms'de düşüyor. Phase-6'nın `T`'si bu dağılımın üstüne seçilecek.

## Checklist

- [x] `link.rs`: iki sayaç + son kare damgası, `DisplayLink` uçları
- [x] `app.rs`: `Counters.content`, üç jeton, `sessiz=none` kolu, kapının
      yeni ifadesi
- [x] `IDLE_FRAME_LIMIT` doc'u: operand, "ölçüm beklemez" gerekçesi, düşen
      aday çözüm cümlesi
- [x] `Makefile` + `proje.md` + `CLAUDE.md` duman cümleleri
- [x] Test: jeton satırı (eski + yeni), içerik kapısı, `sessiz=none`
- [x] Doğrulama geçti (`make hepsi` + `make duman`)
- [x] Yayın etkisi yazıldı
