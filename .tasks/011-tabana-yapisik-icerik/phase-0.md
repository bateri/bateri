# Phase 0 — Duman reçetesinin hedefine sütun bileşeni

## Özet

`smoke_shell`'in ikinci `printf`'i imleci satır değiştirmeden yalnız sütunda
oynatsın, böylece `hareket` jetonu tabana yapışma indikten sonra da imleç
yolunun tanığı kalsın.

_Requirements: R3.1, R3.2_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `smoke_shell`'de `\033[H` yerine
  `\033[4G` (CHA, sütun 4). Doc'un "imleci kıpırdatır" paragrafı **neden
  satırın korunduğunu** anlatacak biçimde güncellenir: tabana yapışmada ofset
  `max(çizilen en büyük row, cursor_row)`'dan doğuyor, yani imleci satır 0'a
  taşıyan bir hedef `content_rows`'u daraltıp ofseti aynı miktarda kaydırıyor
  ve **ekran** hareketi sıfır çıkıyor. `Motion::sync` snap'i eksen başına
  değil konumun tamamına uyguladığı için (`motion.rs:283-296`) bu, sütun
  bileşenini de öldürür. Satır sabit kalınca `content_rows` oynamıyor, ofset
  kimliği değişmiyor ve `sync` animasyonlu kolu alıyor.
  "Aradaki uyku cömert (1 s)" gerekçesi **aynen geçerli** ve korunur.

## Kabul

- `make duman` yeşil: `hucre=8 glif=6 kural=15` **oynamadı**, `hareket > 0`,
  `icerik ≤ IDLE_FRAME_LIMIT`, `sessiz ≥ QUIET_FLOOR`.
- Değişiklik **tek başına** doğrulanabilir: bugünkü kodda da hareket üretiyor
  (imleç satır 1'de kalıyor, sütun 0→3).
- `smoke_shell`'e bağlı üç sınama (`hucre/glif/kural` sayılarının sahibi)
  değişmeden geçiyor.

## Yayın Etkisi

- **Ölçülmüş sözleşme değişiyor.** `make duman`'ın reçetesi bir sözleşme;
  değişimi **kendi commit'inde** iner ve kod phase'leriyle karışmaz
  (`proje.md` → `IDLE_FRAME_LIMIT` kuralıyla aynı gerekçe: aynı commit'te
  oynarsa regresyonu maskeler).
- **`docs/OLCUMLER.md`'ye dokunulmuyor.** Reçete hücre yazmadığı için
  `hucre/glif/kural` sabit; `hareket` ve `sessiz` bantlarının **yeniden
  gözlenmesi** gerekir ama sayı yazmak `/measure`'ın işi. Bant kayarsa
  `## Boşta kare`'nin sahibi o.
- shader, terminfo, ayar şeması, tema, shell entegrasyonu, app bundle: **yok**.
- Yeni bağımlılık: **yok**.

## Checklist

- [ ] `\033[H` → `\033[4G`, doc'un gerekçesi güncellendi
- [ ] Test: `make duman` jeton satırı; `hucre=8 glif=6 kural=15` ve `hareket > 0`
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
- [ ] Yayın etkisi yazıldı
