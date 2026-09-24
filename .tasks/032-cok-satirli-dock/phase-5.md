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

- [x] `paste`'e `r` eki, tam kapıyla
- [x] Tel başlığı
- [x] Test: kapının dört kolu; canlı zsh
- [x] Ölçüm: oh-my-zsh'li gerçek pencere
- [x] phase-4'ten devir (gözle görülen): dolu ızgarada 16 satırlık çok satırlı yapıştırmadan sonra ızgaranın **tepe satırında** ekrana sabit bir glyph artığı kaldı (`l7` üstünde yarım bir harf; içerik kaydıkça yerinde durdu, tema değişimi silmedi, `clear` sildi). Yapıştırmanın bayat aynası (satır bir tuş ızgarada, standout'lu) sürerken oluştu; yazarak kurulan aynı büyüklükte heredoc'ta tekrar etmedi, HEAD'de sınanmadı (pano gerekiyordu). `r` ölçümüyle aynı sahnede yeniden üret, 032'nin getirdiğiyse düzelt
- [x] Doğrulama geçti (`make hepsi`, `make duman`)
- [~] `make kur` — otonom şeridin talimatı koşturmuyor (`target/release/bateri.app` kullanıcının açık örneği olabilir); betiğin pakete kopyası sabit kimlikli geçici pakette `cmp` ile denetlendi

## Uygulama Notları

- **Tazeleme kararı `paste`'in ilk satırında**, `dock_delete_selection`'dan
  önce: seçimin silinmesi nesli ilerletiyor ve kapının "ayna cevap verdi"
  koşulunu kapatırdı. Satır sonu ölçütü `\n` **ya da** `\r`.
- **Canlı zsh sınaması `bracketed-paste-magic`'i yüklüyor** (oh-my-zsh'siz,
  sistem fonksiyonu, komut satırından): `r`'siz koşuda ayna 5 s içinde
  cevap vermedi (fail-first ölçüldü), `r`'le veriyor. Düz `bracketed-paste`'te
  sınama `r`'siz de geçerdi, yani bekçi değil.
- **Ölçüm (gerçek pencere, oh-my-zsh + robbyrussell, `bracketed-paste-magic`
  bağlı):** üç satırlık ve 16 satırlık yapıştırma, yapıştırmadan 150 ms sonraki
  karede bile dock'ta; ızgarada standout'lu satır görünmedi. Bilinen sınır
  yazılmadı.
- **phase-4'ün tepe satırı artığı yeniden üretildi ve 032'nin değil**: artık
  bir glyph değil **blok işareti** (yeşil chevron). 16 satırlık komut koşup
  ekran kayınca ızgaranın ikinci satırına oturuyor ve içerik kaydıkça yerinde
  kalıyor. Sebep çıpa toplamada: prompt'un OSC 8 bağlantısı `preexec`'e kadar
  açık, yani çok satırlı komutun **bütün** satırları çıpayı taşıyor ve
  `blocks.anchors` "görünen ilk çıpalı satırı" komut satırı sayıyor — komutun
  ilk satırı ekranın üstüne kayınca işaret onun devamına düşüyor. Kural 010'dan
  ve bağlantının açıklığı 012'den; 032 öncesinde de çok satırlı yapıştırma
  ızgarada aynı satırları bırakıyordu (HEAD'de koşulmadı, mantıkla). `clear`
  satırları sildiği için siliyor, tema değişimi silmiyor. Çaresi (çıpa
  değişiminde üstteki satır aynı kimliği taşıyorsa işaret ve sayaç çizilmez)
  blok modeline dokunduğu için bu phase'in kapsamı dışında → phase-6'ya devir.
