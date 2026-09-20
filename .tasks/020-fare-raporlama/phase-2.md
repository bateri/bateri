# Phase 2 — Hareket raporu

## Özet

1002 (basılıyken) ve 1003 (her zaman) hareket raporu; rapor hücre değişiminde
kısılıyor.

_Requirements: R7_

## Neden ayrı phase

Phase-1 saf bir tablo ve tek bir `Session` metodu; bu phase yeni bir **AppKit
olay yüzeyi** açıyor (`setAcceptsMouseMovedEvents:`) ve kendi durumunu
(`ViewIvars`'ta son raporlanan hücre) getiriyor. İkisi tek incelemede
boğulurdu. Phase-1 tek başına `make hepsi`'yi yeşil bırakıyor ve belirtiyi
kapatıyor.

## Değişiklikler

- **`crates/bt-shell/src/view.rs`**
  - Pencere `setAcceptsMouseMovedEvents:` ile açılıyor. **`NSTrackingArea`
    yok**: o yalnız `mouseEntered:`/`mouseExited:` ve cursor rect için
    gerekli, ikisi de istenmiyor, ve view zaten first responder — pencere
    seviyesindeki `mouseMoved:` ona geliyor.
  - `mouseMoved:` ve `mouseDragged:` aynı hareket yolunu çağırıyor.
    `mouseDragged:` bugün yalnız seçimi taşıyor; rota `Sent` ise hareket
    raporu üretmeli, `Select` ise bugünkü yolda kalmalı (rota phase-1'de
    kilitleniyor).
  - `ViewIvars`'a son raporlanan hücre (`Cell<Option<(u16, u16)>>`,
    `dragging` emsali). Karşılaştırma **görünür pencere** hücresinde ve
    `bt-core` çağrısından **önce**: hücre değişmediyse `Term` kilidi hiç
    alınmıyor. Kısma olmadan piksel başına rapor boşta duran bir uygulamayı
    sürekli çizdirirdi.
  - Kısmanın çentiği basışta ve bırakmada da tazeleniyor, yoksa jestten sonra
    ilk hareket yutulabilir.
- **`crates/bt-core/src/input.rs`** — hareketin düğme kodu: `32 + n`
  (basılıyken) ve düğmesiz hareket `35`. `button_route`'un kardeşi bir
  `motion_route(mode, pressed)`: 1003 her zaman, 1002 yalnız basılıyken, 1000
  hiç.
- **`crates/bt-core/src/session.rs`** — `Session::mouse_motion(...)`;
  phase-1'in kilidi ve gönderim kuralı aynen (`send`, `send_input` değil).

## Kabul

- 1003 açık uygulamada fareyi gezdirmek rapor üretiyor; aynı hücrede kalan
  hareket **hiç** rapor üretmiyor.
- 1002 açık uygulamada basılı olmayan hareket rapor üretmiyor.
- Yalnız 1000 açan uygulamada hareket hiç rapor üretmiyor.
- Fare kipi kapalıyken `mouseMoved:` `Term` kilidine hiç uğramadan dönüyor.
- `make hepsi` yeşil.

## Yayın Etkisi

- **`CLAUDE.md`:** `bt-core` satırı hareket raporunu da anmalı (phase-1'de
  düğme eklendi, burada hareket).
- **Ayar şeması: yok.**
- **`make test-yaris` gerekmiyor:** yeni paylaşılan durum yok — kısmanın
  durumu `ViewIvars`'ta ve yalnız ana thread'den görülüyor.
- **Ölçüm bekliyor: yok.** Karar 4'ün "hep açık dinle" kolu bir ölçüm sözü
  değil **adlandırılmış geri dönüş**: belirti görülürse kipe göre açmaya
  geçilir (`discussion.md` → Karar 4B). Fare hareketi başına maliyeti ölçecek
  kanca depoda yok.

## Checklist

- [ ] `setAcceptsMouseMovedEvents:` + `mouseMoved:` (erken dönüş kipe bakıyor)
- [ ] `mouseDragged:` rotaya göre ayrışıyor (`Sent` → rapor, `Select` → seçim)
- [ ] `ViewIvars`'ta son raporlanan hücre; kısma `bt-core` çağrısından **önce**
- [ ] `input::motion_route` + hareket düğme kodları (`32 + n`, düğmesiz `35`)
- [ ] `Session::mouse_motion` — phase-1'in kilit ve gönderim kuralıyla aynı
- [ ] Test: `motion_route` — 1003/1002/1000 × basılı/değil
- [ ] Test: hareket düğme kodları üç kodlamada
- [ ] Test: kısma — aynı hücrede ikinci hareket rapor üretmiyor
- [ ] Doğrulama geçti (`make hepsi`)
- [ ] `make duman` — phase-1'deki not aynen geçerli (regresyon alarmı)
- [ ] **Gözle kontrol**: `vim` içinde `mouse=a` ile sürükleyerek seçim ·
      `htop`'ta sütun başlığına tıklama · Claude Code'da sürükleyerek seçim ·
      boşta pencerede fareyi gezdirmek kare üretmiyor (kip kapalıyken)
- [ ] Yayın etkisi yazıldı
