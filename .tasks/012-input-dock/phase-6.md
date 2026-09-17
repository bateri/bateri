# Phase 6 — Bağlam satırı: dizin ve git dalı

## Özet

Dock'un alt satırında, sol altta `[tam klasör yolu] | [git dalı]` yan yana
dursun.

_Requirements: R2.4_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — **OSC 7 bağlanır.** Bugün
  `Event::Title`/`ResetTitle` sessizce düşüyor ve dizin hiçbir yerde tutulmuyor;
  011 Karar 12 dizini **tam bu yüzden** kapsam dışı bırakmıştı, bu phase o
  kararı bilerek geri alıyor.
  - OSC 7 `file://host/path` biçiminde geliyor; **yüzde çözme** ve yabancı
    host'un elenmesi burada. Bozuk URI **panik değil** yoksayma.
  - Dizin `DockState`'in yanında yaşar, `Term` kilidinin dışında.
- **`assets/shell/zsh/bateri.zsh`** — **git dalı `precmd`'den** gelir; terminal
  kendi `git` sürecini **doğurmaz** (pahalı ve tasarımı ayrı bir iş).
  - Bedeli **prompt başına bir fork** (`git rev-parse --abbrev-ref HEAD`).
    Büyük depoda hissedilir — p10k'nın `gitstatusd` daemon'ı bu yüzden var.
    Hızlandırma **kapsam dışı** ve borç olarak yazılır.
  - Depo değilse dal **boş**; ayraç da çizilmez.
  - Detached HEAD'de dal yerine kısa SHA.
- **`crates/bt-gpu/src/frame.rs`** — dock'un alt satırı çizilir.
  - **Taşma kuralı:** yol **soldan** kısaltılır (kuyruk daha bilgilendirici:
    `…/bateri-term/bateri`), dal **asla** kısalmaz. Kısaltma sınırı dock'un
    genişliğinden türetilir, sabit değil.
  - Alt satır sönük (`dim` rolü); ayraç `|`.

## Kabul

- Dock'un alt satırında sol altta tam yol ve dal yan yana duruyor.
- `cd` yapınca yol **anında** güncelleniyor (OSC 7 prompt'ta basılıyor).
- Depo olmayan dizinde dal ve ayraç yok, yalnız yol.
- Detached HEAD'de kısa SHA görünüyor.
- Dar pencerede yol soldan kısalıyor, dal tam kalıyor.
- Bozuk ya da yabancı host'lu OSC 7 yoksayılıyor; panik yok.

## Yayın Etkisi

- **`CLAUDE.md`:** `bt-core` satırı OSC listesinde 7'nin artık **tüketildiğini**
  söyler (bugün "7/8/9/52" yazıyor ama 7 düşüyordu).
- **`make kur` zorunlu** (`assets/shell/*` değişti).
- **Ölçüm bekliyor + araç da borç:** prompt başına `git` fork'unun maliyeti.
  `BT_INPUT_LATENCY_SAMPLES` yok, yani `/measure` bugün kapatamaz; belirti
  "büyük depoda prompt gecikmesi" ve kullanıcının göreceği tek yüzey o.
- **Borç:** dal için daemon/önbellek (`docs/YOL-HARITASI.md`'ye kalem).
- Ayar şeması: **yok** (yol ve dal her zaman görünür; gizleme anahtarı bu setin
  işi değil). shader, terminfo, tema, app bundle: yok. Yeni bağımlılık: yok.

## Checklist

- [ ] OSC 7 bağlandı; yüzde çözme, yabancı host elenmesi, bozuk URI yoksayılıyor
- [ ] Dal `precmd`'den geliyor; terminal `git` doğurmuyor
- [ ] Depo değilse dal ve ayraç yok; detached HEAD'de kısa SHA
- [ ] Alt satır çiziliyor: yol solda, ayraç, dal — yan yana, sönük
- [ ] Taşmada yol **soldan** kısalıyor, dal kısalmıyor
- [ ] Test: OSC 7 ayrıştırma (yüzde kodlu yol, yabancı host, bozuk URI)
- [ ] Test: taşma kısaltması
- [ ] Doğrulama geçti (`make hepsi` + `make kur`)
- [ ] Yayın etkisi yazıldı ("ölçüm bekliyor + araç da borç" dahil)
