# Phase 3b — Belge uyumu ve yöntem cümleleri

## Özet

Kod nihai hâlini aldıktan **sonra** belgeler ona uyumlanır: kanca adları,
`CLAUDE.md`'nin borç cümleleri, `/measure` skill'i, `context.md` şablonu ve
`## Yöntem`'e geçecek dürüst sınırlar.

_Requirements: R7, R7.1, R7.2, R7.3_

---

## Neden ayrı phase

Phase-3 otuz dört kaleme çıkmıştı ve içinde **dört yeniden ölçüm** vardı
(`IDLE_FRAME_LIMIT`'in dayanağı, boşta ölçütün derinliği, bekçi bütçesi,
GPU sütununun uzunluğu). Bunlar sabitleri değiştirebilir.

Belge aynı commit'te yazılsaydı **bayat doğardı**: `CLAUDE.md`'ye yazılan
sayı, aynı commit'te yeniden ölçülen sayı olurdu. Sıra bu yüzden zorunlu —
belge, gönderilen davranışı anlatır, tasarlanan davranışı değil.

Ad `duzen.md`'ye uygun: doğal sıralamada `phase-3 < phase-3b`.

---

## Kalemler

Aşağıdakiler phase-3'ten taşındı; gövdeleri phase-3'ün ilgili bölümlerinde
(`## 3. Belgeler`) ve devir notlarında duruyor.

---

## Uygulama Notları

## Yayın Etkisi

- **Belge:** `CLAUDE.md` (kanca adları, `make duman` satırı, bench borcunun
  yeniden yazımı, kapanış maddesinin doğrulanması), `.claude/skills/measure/SKILL.md`,
  `.claude/is-akisi/sablonlar/context.md`, `.claude/is-akisi/proje.md`,
  `.tasks/README.md`.
- **`docs/OLCUMLER.md` yazılmıyor** — ilk `/measure` kuracak. Ama `## Yöntem`'e
  geçecek dürüst cümleler kodun doc'unda hazır durur.
- **Ölçüm bekliyor: yok.** Kod değişmiyor; bu phase belge uyumu.
- **Kapanan iddialar:** setin kapattığı ve **kapatmadığı** iddiaların listesi
  bu phase'de `.tasks/README.md` ve `CLAUDE.md`'ye dürüstçe yazılır — bench'e
  bağlı olanlar açık kalıyor.

---

## Checklist

- [ ] `CLAUDE.md`: kanca adları, `make duman` satırı, **bench borcunun yeniden yazımı** (silme değil)
- [ ] `/measure` skill'i, `context.md` şablonu, `proje.md` kanca adlarıyla uyumlandı
- [ ] `.tasks/README.md`: 002/003/004 satırları "ölçüm aracı yok" demiyor
- [ ] Phase-2'nin dürüst sınırı (açılış damgası `main()`'den, süreç başından değil) kodun doc'unda yazılı
- [ ] **phase-2'den devir — `## Yöntem`'e geçecek dürüst sınır.** Açılış damgası `main()`'in **ilk satırında**, `has_aqua_session()`'ın alt sürecinden de önce; ama yine de **süreç başlangıcı değil** (dyld + Rust runtime kurulumu önce bitiyor) ve bittiği yer **ilk tamamlanan kare** (`addCompletedHandler`), sunulan kare değil. `/measure`'ın "process başlangıcından" tarifinden bu kadar sapıyor
- [ ] **phase-2b'den devir — `## Yöntem`'e geçecek üç cümle.** (1) Hiçbir koşu atılmıyor: kapanış artık en çok `SHUTDOWN_GRACE` (500 ms) bekliyor ve jeton satırı her koşuda basılıyor. (2) Ama kusur **iyileşmedi, sınırlandı**: ölçülen sekiz Load koşusunun **ikisinde** sınır doldu, yani çocuk çıkışın içinde (`?Es`) arkada bırakıldı ve onu süreç çıkışı topladı — stderr'de bir satır var (`shell 500ms içinde kapanmadı, arkada bırakıldı`), ölçüm sayılarına etkisi yok ama koşu süresine yarım saniye ekliyor. (3) Kalıcı çare hâlâ açık ve adı belli: `wait` bloklarken master'ı boşaltmak; yolu da belli — `Session::spawn`'da `pty.file().try_clone()` ile master'ın bir kopyası alınabilir (yeni bağımlılık **gerekmiyor**, `Cargo.lock` oynamıyor). Bu phase'in işi değil, `## Yöntem`'in dürüst sınırı
- [ ] **phase-2b'den devir — `CLAUDE.md`'nin kapanış maddesi yeniden yazıldı.** R7'nin "borç cümleleri silinmez, yeniden yazılır" kuralı gereği madde daraltıldı ve içinde **çürütülmüş bir çare** kayda geçti: "`SIGHUP` → süre → `SIGKILL`" işe yaramıyor (o durumdaki çocuk `SIGKILL` almıyor, ölçüldü). Phase-3 aynı dosyaya kanca adlarını yazarken bu maddeyi **yeniden yazmasın**; dokunması gereken satırlar kanca adları ve bench borcu
- [ ] **phase-2'den devir — izlenmeyen belge borcu.** `/audit` (mercek 10) yakaladı: `CLAUDE.md`'nin **katman tablosundaki** `bt-gpu` satırı crate'in yeni sorumluluğunu (ölçüm defteri) anmıyor. `crates/bt-gpu/src/lib.rs` başlık yorumu phase-2'de güncellendi; tablo satırı R7.2'nin kanca adları listesinde **yok**, yani bu satır yazılmasa kimse görmezdi
- [ ] **phase-3'ten devir — jeton satırı büyüdü, belgede yazılı hâli yok.** Kod bugün şunu basıyor ve **hiçbir belge** bunu anlatmıyor: `kare hucre glif kural yuva yuk istek kapanis profil ornek dusen gpu_ornek gpu_elenen taban cpu_kare_p95 cpu_kare_max cpu_encode_p95 cpu_encode_max gpu_p95 gpu_max acilis pipeline=ok`. Yazılması gerekenler: (a) kapı kapalıyken `ornek=off` çıkar ve **ölçüm jetonları hiç basılmaz** — `ornek=0` bilerek seçilmedi, çünkü sıfır "kapı açıktı, hiç örnek toplanmadı" ile karışırdı (R5.2); (b) taban altında p95 **ve** max `insufficient` der, eşik `taban=` jetonunda; (c) `ornek=` CPU sütununun, `gpu_ornek=` GPU'nunki — Metal sıfır damga verirse ikincisi kısa kalır ve farkı `gpu_elenen=` söyler; (d) `dusen=` tek sayı ve CPU'dan geliyor, GPU'nunkinin **tavanı**; (e) `istek=` bir sayaç (**kapı değil**), `kapanis=` ise kısmen kapı: panik kolları (`reader-panicked`, `panicked`) koşuyu kırmızı düşürüyor, `abandoned`/`unbounded` düşürmüyor; (f) jeton **adları** Türkçe, **değerleri** İngilizce ve bu bir kural — depodaki `yuk=smoke|load` deseni
- [ ] **phase-3'ten devir — `proje.md`'nin "Üst sınır neden 2" paragrafı ölü bir teoriyi kelimesi kelimesine taşıyor.** Doğrulama tablosundaki `make duman` satırı hâlâ "sistem display link'i askıya alıyor ve ritim ~3'te doyuyor", "tavan 3", "`2` bu üçünün arasındaki tek anlamlı yer" diyor. Üçü de çürüdü. Yeni sayılar phase-3'ün `## Uygulama Notları → Ölçülenler` başlığında: sağlıklı duman 1–2 (otuz bir koşu), bir kez 4; bozuk duman **49–354**; sınır artık **8**. Paragraf silinmez, **yeniden yazılır** — ve içine eski sınırın doğru bir build'i kırmızıya düşürdüğü kayda geçer
- [ ] **phase-3'ten devir — `Makefile`'ın `duman` hedefindeki yorum bloğu.** Örnek jeton satırını ve "bkz. app.rs IDLE_FRAME_LIMIT, bugün 2" cümlesini taşıyor; ikisi de bayat. Kod değil yorum olduğu için phase-3'te dokunulmadı
- [ ] **phase-3'ten devir — `CLAUDE.md`'nin kapanış maddesine eklenecek bir cümle var.** Sınır dolan koşu artık **görünür**: `Session::shutdown` sonucu döndürüyor (`Teardown`) ve rapor `kapanis=clean|reader-panicked|abandoned|panicked|unbounded|already-done|none` basıyor. Madde "arkada kalan çocuk" borcunu yeniden yazmıyor (phase-2b onu yazdı), yalnız borcun artık ölçülebildiğini söylüyor
- [ ] **phase-3'ten devir — `## Yöntem`'e geçecek iki dürüst sınır.** (1) **Kapanış hâlâ asılıyor, yalnız sınırlı:** ölçüm yükünün on yedi koşusunun **dördünde** `kapanis=asildi` (~%24, phase-2b 2/8 ölçmüştü) — sayılara etkisi yok, koşuya yarım saniye ekliyor. (2) **`kare` ile `istek` iki rejimde tamamen ayrışıyor:** duman yükünde `istek ≈ kare + 2`, ölçüm yükünde `kare=21` iken `istek≈71 000`. Mekanizması **ölçülmedi** (kapı mı yutuyor, ana thread mi doyuyor, sistem mi link'i kısıyor) ve aynı yükün phase-2b'de `kare=594` vermesi de açıklanmadı — pencere görünürlüğü şüpheli ama doğrulanmadı. `/measure` bir kare süresi okurken bunu bilmeli
- [ ] **phase-2'den devir — halkalar açılış yolunda ayrılıyor**, yani `acilis=` sayısının **içindeler**: 3 saniyelik koşuda 8,6 KB, tavanda 1,7 MB. `## Yöntem`'in dürüst sınırlarından biri
- [ ] **phase-3'ten devir — `IDLE_FRAME_LIMIT` `.app` paketiyle yeniden ölçülecek.** Bugünkü `8` görünmeyen bir pencerede ölçüldü; `make kur` gelince meşru kare sayısı artabilir. Sabitin doc'unda yazılı, belge tarafında da anılmalı
- [ ] **phase-3'ten devir — `CLAUDE.md`'nin dil kuralı jeton satırını tam anlatmıyor.** Bugün "`make duman` satırları Türkçe kalır" diyor; kod ise ayrım yapıyor ve ayrımın gerekçesi `Report::token_line`'ın doc'unda: **anahtarlar** Türkçe ve donmuş (sözleşme "silinmez" diyor), **değerler** İngilizce (okuyan taraf bir `match` kolu / CI grep'i), **tanı metni** (stderr, `assert!`) Türkçe. Cümle bu üçe ayrılmalı — `/audit` mercek 10'un bulgusu
- [ ] **phase-3'ten devir — boşta kare kapısının algılama tabanı yükseldi.** `IDLE_FRAME_LIMIT` 2→8 olunca 3 sn'lik koşuda yakalanabilen en yavaş sızıntı ~0,7 Hz'den ~2,7 Hz'e çıktı. Bu sette öyle bir animasyon **yok**, ama motion/imleç fiziği seti (00X) tam bu şekilde gelecek: durma koşulsuz 2 Hz'lik bir blink 3 sn'de ~6 kare eder ve bugün yeşil geçer. `/audit` mercek 8'in notu; motion setinin `context.md`'sine taşınmalı. Yarısı kurulu: `istek=` örtülmeden etkilenmiyor ve **oran** olarak (saniye başına talep) kapıya bağlanabilir — ama eşik ölçülmedi
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; belge-only ise `make hepsi` yeter, gerekçesi notlara)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
