# Phase 4 — Dönüşüm: çok satır dock'ta

## Özet

`Multiline`'ı kaldır: satır sonlu görüntü ve `PREBUFFER` dock'ta çizilir,
bastırma ızgaradaki bütün giriş satırlarını kapsar, caret dock'ta kalır;
`Multiline`'ın arkasında erişilemez duran sessiz kırılmalar aynı commit'te
bekçiyle kapanır.

_Requirements: R3.1, R3.2, R3.3, R3.4, R3.5, R4.1, R4.2_

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — `DockStatus::Multiline` ve bütün kolları
  kalkar (`caret_home_raw`, tutma istisnası, eşleşmeler); `\n` kontrol
  karakteri sayılmaz, `Control` kalan kontrol karakterleri için aynen.
  `last_ink` süzgeci `\n`'i atlar. `apply_dock::End` (`e`): safha `Input`'ta
  kaldıkça son görüntü, bant ve bastırma `HANDOVER_HOLD` kadar tutulur; `u`
  gelirse yeni ayna, 133 `C` ya da süre dolunca bugünkü sıfırlama
  (`discussion.md` → Karar 11). `SuppressedInput` satır farkında girdileri
  taşır; `blank_mirror` "görüntünün hiç karakteri yok" ve `PREBUFFER`'ı
  hesaba katar (025'in "karaktersiz ayna imleci prompt'un satırından aşağı
  itemez" öncülü `PREBUFFER` varken yanlış; orada çıpa sınaması
  sorulmaz).
- **Bilinen sınır, adıyla yazılacak:** `PS2` satırında ızgara kullanıcının
  `for> ` mürekkebini taşıyor, ayna taşımıyor; `BUFFER` boşken içerik kapısı
  "bayat" der. Bugün olduğu gibi zamansal kapı (`line-init`'in `u`'su ⏎'e
  cevap) kurtarıyor; yönü güvenli (satır iki yerde görünür).
- **`crates/bt-core/src/session.rs`** — bastırmanın `to`'su imlecin altındaki
  satırları (`BUFFER`'ın imleçten sonraki satırları, sarmalarıyla) kapsar;
  `PREBUFFER` doluysa `floor` çıpanın satırı. Doluluk bastırılan satırları
  saymaz (tek yüklem, dört tüketici korunur). `can_be_typed` değişmez.
- **`crates/bt-core/src/dock.rs`** — `PREBUFFER` satırları düzenlenebilir
  satırların üstünde, aynı girintide; seçilebilir, kopyalanır; isabet
  `PREBUFFER`'a düşerse caret taşınmaz. 031'in düzenleme kapısı `PREBUFFER`'a
  değen seçimde komut göndermez (seçim kalkar, tuş bugünkü yoldan).
- **`CLAUDE.md`** — `Multiline`'ın bütün anılışları, "dock'u çok satırlı
  girişe göre büyütmek bilerek yapılmadı" paragrafı, "dock'un giriş satırı
  bir tane", `DOCK_ROWS * cell_h` ve `split_into_grid` cümlesi (PTY payı /
  çizilen bant ayrımı), saç çizgisi paragrafları (024'ün pencereleme cümlesi
  phase-3'te sarmaya çevrildi), kaymanın yön kuralına bandın istisnası, `bt-gpu` satırındaki "kaç satır
  olduğu `DOCK_ROWS`" — kural + tek cümle gerekçe + işaretçi.
- **`docs/YOL-HARITASI.md`** — borç kalemi (şimdiden sete bağlı) kapanış
  notuyla tek satır.

## Kabul

- Çevrilen bekçiler: `a_newline_anywhere_in_the_display_marks_the_mirror_multiline`
  → `Live`; `a_multiline_mirror_is_never_held` → tutma kuralı;
  `a_multiline_mirror_draws_nothing_and_keeps_the_caret_in_the_grid` →
  çok satır çiziliyor; `a_bracketed_multiline_paste_leaves_the_line_and_the_caret_in_the_grid`
  → satır ve caret dock'ta (ayna tazeyken); Control sınamasındaki "ikisi
  birden → Multiline" → `Control`.
- Yeni bekçiler: `last_ink` satır sonuyla biten yapıştırmada; `blank_mirror`
  (025'in senaryosu: sondaki satır sonunda duran caret); imlecin altındaki
  satırların bastırılması; canlı zsh'le (`Session::spawn` + sarmalayıcı)
  `for i in 1 2; do` ⏎ `echo $i` — `PREBUFFER` dock'ta, ızgarada bastırılmış,
  ⏎'de bant pompalamıyor.
- `make hepsi`, `make test-yaris`, `make duman` yeşil.
- Gözle kontrol (üç yüzey): çok satırlı yapıştırma, geçmişten `for` döngüsü,
  heredoc — dock büyür, ızgara yukarı süzülür (dolu ızgarada tepe kırpılır,
  boşken doldurma bandı kısalır), Enter'da komut ızgarada yerinde belirir,
  bant geri çekilir; vim'e girip çıkınca dock doğru boyda.

## Checklist

- [ ] `Multiline` kalkıyor; `\n` → `Live`
- [ ] Bastırma bütün satırlarda; `PREBUFFER` tabanı
- [ ] `e` tutması (`HANDOVER_HOLD`)
- [ ] `PREBUFFER` çizimi, salt okunur seçim
- [ ] `last_ink`, `blank_mirror`
- [ ] Bekçiler çevrildi / eklendi; canlı zsh sınaması
- [ ] `CLAUDE.md`, `docs/YOL-HARITASI.md`
- [ ] phase-1'den devir: imleç bir `\n`'in arkasındayken ızgara başlangıcı ve caret kuralı (phase-1 → Uygulama Notları) — bu phase'in dock tarafına etkisini uygula ya da gerekçesiyle kapat
- [ ] Orkestratör (phase-3'ün waive 2'sinden, kullanıcı tarafı): tavanı aşan girişte dikey pencerenin dışındaki satırlara fare de ulaşabilsin — işaretçi dock'un üstündeyken tekerlek/trackpad dock'un dikey penceresini kaydırsın (ızgarayı değil); bırakınca caret'i izleme yeniden başlasın (yazınca/caret hareket edince pencere caret'e döner). Sürükleyerek seçim pencerenin kenarına değince pencere kaysın.
- [ ] Orkestratör (phase-3'ün gözlemi): punto büyütmeden (resize) sonra ızgarada eski bir satır kalıyordu — HEAD'de (032 öncesi, 9a0dd82) de oluyor mu ölç; 032'nin getirdiğiyse düzelt, değilse Uygulama Notları'na bilinen sınır olarak yaz.
- [ ] Doğrulama geçti (`make hepsi`, `make test-yaris`, `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
