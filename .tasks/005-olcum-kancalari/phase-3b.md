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
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; belge-only ise `make hepsi` yeter, gerekçesi notlara)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
