# Phase 5 — Çok satırlı yapıştırmanın tazelemesi

## Özet

Satır sonlu bracketed yapıştırmanın arkasına aynayı tazeleyen tek komut
gönder, böylece `bracketed-paste-magic`'li kabukta yapıştırılan satır bir tuş
boyunca ızgarada görünmesin; sonucu gerçek pencerede ölç.

_Requirements: R5_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `Session::paste`: yük satır sonu
  taşıyor ve 031'in tam düzenleme kapısı (`dock_edit_line`: dock sahibi,
  ekleme keymap'i, ayna güncel nesle cevap, `w`) **yapıştırmadan önce**
  açıksa, bracketed yükün arkasına aynı yazımda `CSI 8133 ~ r BEL`. Kapı
  kapalıysa hiçbir şey eklenmez — `vicmd`'de baytlar komut olurdu.
- **`assets/shell/zsh/bateri.zsh`** — widget'ın davranışı değişmez (tanımadığı
  yükte `BUFFER`'a dokunmuyor, aynayı her koşulda basıyor); tel başlığına `r`
  adıyla yazılır.

## Kabul

- Sınama: kapı açıkken yazıma `r` ekleniyor; `vicmd`'de, cevapsız aynada ve
  `w` yokken eklenmiyor; tek satırlık yapıştırmada (`can_be_typed` yolu)
  eklenmiyor.
- Canlı zsh (`Session::spawn` + sarmalayıcı, oh-my-zsh'siz): çok satırlı
  yapıştırmadan sonra ayna yapıştırmanın sonucunu taşıyor.
- **Ölçüm, gerçek pencere, oh-my-zsh'li kabuk** (`bracketed-paste-magic`):
  çok satırlı yapıştırmada satır ızgaraya çıkmadan dock'ta görünüyor mu.
  Tutmazsa kod geri alınmaz ama kalem Uygulama Notları'na ve
  `docs/YOL-HARITASI.md`'ye bilinen sınır olarak adıyla yazılır (güvenli
  yol: tazelik kapısı satırı bir tuş boyunca ızgarada tutuyor).
- `make hepsi`, `make kur`.

## Checklist

- [ ] `paste`'e `r` eki, tam kapıyla
- [ ] Tel başlığı
- [ ] Test: kapının dört kolu; canlı zsh
- [ ] Ölçüm: oh-my-zsh'li gerçek pencere
- [ ] phase-4'ten devir (gözle görülen): dolu ızgarada 16 satırlık çok satırlı yapıştırmadan sonra ızgaranın **tepe satırında** ekrana sabit bir glyph artığı kaldı (`l7` üstünde yarım bir harf; içerik kaydıkça yerinde durdu, tema değişimi silmedi, `clear` sildi). Yapıştırmanın bayat aynası (satır bir tuş ızgarada, standout'lu) sürerken oluştu; yazarak kurulan aynı büyüklükte heredoc'ta tekrar etmedi, HEAD'de sınanmadı (pano gerekiyordu). `r` ölçümüyle aynı sahnede yeniden üret, 032'nin getirdiğiyse düzelt
- [ ] Doğrulama geçti (`make hepsi`, `make kur`)
