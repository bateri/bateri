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

## Uygulama Notları

- **`motion_route`'un cevabı `ButtonRoute` değil `Option<MouseEncoding>`.**
  Düğme yolunun `Select` varyantı burada anlamsız — hareket bir jest
  başlatmıyor — ve tipi paylaşmak "seçim de olabilir" derdi.
- **Sağ ve orta tuşun sürüklemesi de bağlandı** (`rightMouseDragged:`,
  `otherMouseDragged:`); plan yalnız `mouseDragged:` diyordu. Onlarsız R7
  yarım kalıyordu: AppKit bir düğme basılıyken `mouseMoved:` **göndermiyor**,
  yani sağ tuş basılı sürüklemede 1002/1003'e hiç rapor gitmezdi. Üçü aynı
  gövdede (`drag_event`), `otherMouse*` yine `buttonNumber() == 2` kapılı.
- **Kabul kriteri düzeldi: "kip kapalıyken `Term` kilidine hiç uğramadan
  dönüyor" tam değil.** Kip `bt-core`'da ve dışarıdan sorulamıyor (R3), yani
  `mouseMoved:` kipi göremez; kısma **hücre değişimine** bakıyor, kipe değil.
  Doğrusu: *aynı hücrede kalan* hareket kilide hiç uğramıyor, hücre değişimi
  başına bir sonuçsuz çağrı kalıyor. Karar 4'ün kendi metni zaten bunu
  söylüyordu ("hücre değişmeyen olay kilide hiç uğramadan dönüyor"); kabul
  satırı onu "kip kapalıyken" diye kısaltmıştı.
- **Kısma serbest bir fonksiyon** (`moved_to_new_cell`), metod değil:
  `define_class!` gövdeleri sınanamıyor ve kuralın ("ilk görüşte `true`,
  tekrarda `false`") bir bekçisi olmalı. Bekçi yarının okunmadığını da
  çiviliyor — rapor hücre çözünürlüğünde.
- **Hareketin hücresi `fill_rows = 0` ile alınıyor**, basışın kapısıyla
  değil: bandın üstündeki nokta bir seçim ucu değil rapora giden koordinat
  (tekerleğin ve bırakmanın gerekçesinin aynısı).
- **Duman bir tur yalancı kırmızı verdi ve sebebi ortamdı.** "Animasyon
  yerleşmedi, hareket karesi 1" — ekran uykudaydı, pencere görünmeyince
  `CAMetalDisplayLink` kare vermiyor. Phase-1 commit'i de aynı koşulda
  kırmızı düştü (`git stash` ile doğrulandı), yani kod değil ekran. Ekran
  açıkken `hareket=27` ile yeşil. Duman'ın **görünür pencere** istediği
  `CLAUDE.md`'de yazılı değil; jetonların anlamı `Report::token_line`'da ve
  bu ortam koşulu oraya not olarak girmedi — borç değil, bilgi.

### Set kapısının değiştirdikleri

`/code-review` on bulgu verdi; sekizi düzeltildi, ikisi waive.

- **Rapora giden koordinat artık ızgaranın dışında reddediliyor**
  (`OutOfGrid`). Kapı tek kurala indi: **jest başlatan olay reddedilir,
  süren jestin devamı kırpılır.** Bu olmadan başlık çubuğunda gezinen
  işaretçi uygulamaya 0. satırı, dock bandına yapılan tıklama son ızgara
  satırını bildiriyordu — ikisi de `point_to_cell`'in yapışma
  semantiğinden. R8 (doldurma bandı) artık bu kuralın **özel hâli**, ayrı
  bir dal değil.
- **Kayıp `mouseUp:` uygulamada takılı düğme bırakıyordu**
  (`flush_lost_releases`). Basıştaki bayat-bit temizliği terminalin kendi
  defterini düzeltiyordu ama bırakma raporu hiç gitmiyordu; kanıt
  selector'ın kendisi — AppKit `mouseMoved:`'ı yalnız hiçbir düğme basılı
  değilken gönderiyor.
- **Çentik yalnız raporlanan olayda damgalanıyor** (`report_button`):
  `Select`/`Ignored` kollarında damgalamak o hücredeki ilk hover raporunu
  yutuyordu. `motion_cell`'in doc'u buna göre düzeldi.
- **`reaches_terminal`'ın doc'u çalınmıştı:** `modifiers` fonksiyonu doc
  bloğu ile gövdesinin **arasına** girmiş, Command izin listesinin gerekçesi
  fare değiştirici çevirmenini belgeliyordu. Yol haritasında adıyla yazılı
  tuzağın ("doc yorumunun altına kod sokmak sessizce doc çalıyor") üçüncü
  tekrarı.
- **`session.rs`'in iki kopyası tek gövdeye indi** (`mouse_report_at` +
  `send_report`); `mouse_button`'ın iki `button_route` match'i bire indi ve
  asimetri (`let shift = pressed && modifiers.shift`) tek satırda görünür
  oldu.
- **`docs/YOL-HARITASI.md`:** 020 araya girince kaçan bayat `021` referansı
  düzeldi.

**Waive edilen iki bulgu:**

- **Hücre başına `Term` kilidi** (ana thread). `discussion.md` → Karar 4A'nın
  kabul ettiği bedel; geri dönüşü (4B, kipe göre aç) adlandırılmış ve
  ölçülmemiş bir kazanç için ölçülmüş bir bedel (`make test-yaris`)
  istiyor. `/audit` mercek 4 de aynı yeri gördü ve kilit sırasını temiz
  buldu.
- **Jest durum makinesi sınanamıyor.** `sent_buttons`/`dragging`/`Click`
  geçişleri `define_class!` komşusu `impl` gövdesinde ve yalnız
  `moved_to_new_cell` ile `button_bit` saf fonksiyona çıkarıldı. Bulgunun
  önerisi (jest durumunu saf bir struct'a taşımak) doğru ama set kapısının
  kapsamını aşıyor — borç olarak yazılıyor.

## Checklist

- [x] `setAcceptsMouseMovedEvents:` + `mouseMoved:` (erken dönüş kipe bakıyor)
- [x] `mouseDragged:` rotaya göre ayrışıyor (`Sent` → rapor, `Select` → seçim)
- [x] `ViewIvars`'ta son raporlanan hücre; kısma `bt-core` çağrısından **önce**
- [x] `input::motion_route` + hareket düğme kodları (`32 + n`, düğmesiz `35`)
- [x] `Session::mouse_motion` — phase-1'in kilit ve gönderim kuralıyla aynı
- [x] Test: `motion_route` — 1003/1002/1000 × basılı/değil
- [x] Test: hareket düğme kodları üç kodlamada
- [x] Test: kısma — aynı hücrede ikinci hareket rapor üretmiyor
- [x] Doğrulama geçti (`make hepsi`)
- [x] `make duman` — phase-1'deki not aynen geçerli (regresyon alarmı)
- [x] **Gözle kontrol**: `vim` içinde `mouse=a` ile sürükleyerek seçim ·
      `htop`'ta sütun başlığına tıklama · Claude Code'da sürükleyerek seçim ·
      boşta pencerede fareyi gezdirmek kare üretmiyor (kip kapalıyken)
- [x] Yayın etkisi yazıldı
