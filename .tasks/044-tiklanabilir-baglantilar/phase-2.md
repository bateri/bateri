# Phase 2 — Hover yuvası ve alt çizgi (`bt-core`)

## Özet

Hover yuvası kurulunca bağlantının hücreleri üç yüzeyde alt çizgiyle
çiziliyor; bayat yuva çizilmiyor, düşüyor ve ana kuyruğa haber veriyor.

_Requirements: R4, R4.1, R4.2_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `Session::set_link_hover(Option<LinkHover>)`
  (`LinkHover`: phase-1'in `LinkHit` aralığı + damga + stil
  `Single`/`Dashed`). Yaprak kilit, `Theme` örüntüsü: `frame()` kopyayı
  `Term` kilidinden **önce** alır; aynı değerle kurmak no-op ve uyandırmaz,
  değişim hasar diker (`Waker::wake`). `frame()` kilit altında damgayı
  (`LedgerMark` + OSC 8'de hücrenin `Hyperlink`'i) sınar; tutmazsa aralığı
  çizmez, yuvayı düşürür ve `Wake::link_hover_lost` (yüksüz, kenarda) ile
  haber verir. Alt çizgi ezmesi **tek yardımcıda** (hücre aralıktaysa
  `underline` = stil, `underline_color` = `None`) ve üç sink'ten çağrılır:
  ızgara, doldurma bandı, `Session::dock` (dock'un yuvası phase-5'te dolar).
  Hover yokken maliyet tek dal.
- **`crates/bt-core/src/wake.rs`** — `link_hover_lost` (varsayılan gövdesi
  boş, mevcut `Wake` uygulayıcıları değişmeden derlenir).

## Kabul

- Sınama: hover kurulu karede aralığın hücreleri `UnderlineStyle::Single`,
  dışındakiler değişmemiş; aynı satır yerinde yeniden yazılınca (`\r` +
  başka metin) bir sonraki karede ezme yok ve `link_hover_lost` bir kez
  çağrıldı; OSC 8 aralığında hücrenin bağlantısı değişince aynı; doldurma
  bandının hücreleri de eziliyor; aynı hover'ı ikinci kez kurmak uyandırma
  saymıyor.
- `make check`, `make linux`, `make test-race` yeşil.

## Checklist

- [ ] Hover yuvası, `set_link_hover`, no-op kuralı
- [ ] `frame()`'de damga denetimi, düşürme ve `Wake::link_hover_lost`
- [ ] Tek ezme yardımcısı, üç sink
- [ ] Test: ezme, yerinde yeniden yazım, OSC 8 değişimi, bant, no-op
- [ ] Doğrulama geçti (`make check` + `make linux` + `make test-race`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
