# Phase 6 — Duman sabitlerinin yeniden gözlemi

## Özet

Kare yolu değişti:

- sessizliğin saati artık `Pacer`'ın damgası;
- kare sayımı gönderim indeksi + `poll`.

Bu yüzden `IDLE_FRAME_LIMIT` ve `QUIET_FLOOR`, kendi doc'larının istediği gibi
yeniden gözleniyor. Değer ya da doc değişikliği kod phase'lerinden **ayrı**
commit'le iniyor (`proje.md` → Doğrulama).

_Requirements: R5_

_Kısıt: yazılan/taşınan kodun yorumları, doc-comment'leri ve tanı metinleri İngilizce (plan.md → Yaklaşım, dil kısıtı)._

## Değişiklikler

- **Gözlem** (kod değil). `docs/OLCUMLER.md` → `## Yöntem` → Boşta kare'nin
  reçetesi uygulanıyor:
  - sağlıklı dağılım, debug ve release;
  - aynı bölümün bozuk (sızıntılı) reçetesiyle bozuk dağılım.
  - Sonuç `docs/OLCUMLER.md` → `## Boşta kare`'ye tarihli bir blok olarak
    giriyor.
- **`crates/bt-shell/src/app.rs`** — yalnız gözlem gerektiriyorsa:
  - `IDLE_FRAME_LIMIT` / `QUIET_FLOOR`'un değeri ya da türetmesini taşıyan
    doc'u;
  - sınamadaki sağlıklı örnek (`HEALTHY_QUIET`).
  - Kural (`CLAUDE.md` → Komutlar): sessiz'in kuralı ters, iki sınır da
    ölçülmüş ve türetmesi doc'ta.
- Değer değişmiyorsa: doc'a "040 geçişinden sonra yeniden gözlendi, aynı"
  tek satırı ve `docs/OLCUMLER.md` bloğu. Commit yine ayrı.

## Kabul

- İki dağılım `docs/OLCUMLER.md`'de. Sağlıklı dağılım iki sınırın doğru
  tarafında, bozuk dağılım kırmızı tarafta. Öyle değilse sabit yeniden
  türetiliyor, tahmin yazılmıyor.
- `make hepsi` ve `make duman` yeşil.

## Checklist

- [x] Yazılan/taşınan kodun yorumları ve tanı metinleri İngilizce
- [x] Sağlıklı dağılım (debug + release)
- [x] Bozuk dağılım
- [x] `docs/OLCUMLER.md` bloğu; gerekiyorsa sabit/doc ve `HEALTHY_QUIET`
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)

## Uygulama Notları

- **Sabitler değişmedi.** Sağlıklı: debug ve release 3 sn'de `icerik=2` ×20,
  `sessiz` 1746,88–1759,09 ms (yarısı 873,44 ≥ 868). Bozuk: hızlı sızıntı
  `MissingCounter` (`icerik` 342–356), yavaş sızıntı `QuietTooShort` (`sessiz`
  ≤ 155,21), durma koşulu `MotionUnsettled`. İki sabitin doc'una İngilizce
  "yeniden gözlendi, aynı" satırı; `HEALTHY_QUIET` (1742) dokunulmadı —
  sınamanın örneği, bandın ~5 ms altında ama tabanın çok üstünde.
- **Debug kolu `make duman` yerine `open` + geçici paket** (`dev.bateri.duman`,
  URL şeması silinmiş): `make duman` iki kez `kare=0` — pencere ekranda ama
  Safari ön planda ve onu örtüyordu; `cargo run` etkinleşme almıyor, wgpu
  örtülü pencereye drawable vermiyor (phase-5 notu). Ortam, kod değil; tarif
  `docs/OLCUMLER.md` → `## Nasıl yeniden ölçülür` → Boşta kare'ye yazıldı.
- **Tarifin mutasyon gövdeleri bayattı** (`needs_update`, `link.setPaused`,
  `DispatchQueue::main().after`): 040'ın koduna göre yeniden yazıldı
  (`Core::tick`'in sonu, `motion_tick`'in `at_rest` dalı, `Pacer::after`).
- `make kur` dört kez `target/release/bateri.app`'i yeniden üretti; kullanıcının
  açık örneği `/Applications/bateri.app`, etkilenmedi.
- **Doğrulama:** `make hepsi` yeşil. `make duman`'ın kendisi bu oturumda
  ortam yüzünden kırmızı (`kare=0`; son denemede ön planda kullanıcının
  `/Applications/bateri.app`'i vardı ve pencereyi örtüyordu); aynı debug
  binary'sinin `open` ile koşan 13 sağlıklı koşusu yeşil. Checklist'in son
  maddesi bu yüzden işaretlenmedi — ekran boşken `make duman` bir kez koşulmalı.

