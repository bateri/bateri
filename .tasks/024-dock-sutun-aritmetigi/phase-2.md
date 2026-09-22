# Phase 2 — Tazelik kapısının iki tarafı aynı birimi okur

## Özet

`DockState::last_ink` sıfır genişlikli kod noktalarını atlıyor, böylece
birleştirici taşıyan bir satır (`❤️`, ZWJ) bastırmayı bırakmıyor.

_Requirements: R4, R4.1, R4.2_

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — `last_ink` bugün son boşluk olmayan
  `char`'ı alıyor. Ölçüt **genişliğe** genişliyor: sıfır genişlikli kod
  noktaları atlanıyor, çünkü ızgara hücresine hiç girmiyorlar (alacritty
  birleştiriciyi `CellExtra`'da tutuyor ve `cell.c` taban karakteri
  taşıyor). İki taraf aynı birimi okumazsa kapı **kalıcı olarak** "bayat"
  der: `❤️` yazan kullanıcının satırı her tuşta ızgaraya düşer.
  Ölçüt yine `unicode-width` (genişlik 0) — phase-1'in bağımlılığının ikinci
  tüketicisi ve aynı kaynaktan beslenmesi şart, yoksa kapı ile çizim
  ayrışır.
  Mevcut daraltma **korunuyor**: `is_whitespace()` değil `' '`/`'\t'`, çünkü
  kapının ızgara yarısı da öyle (`Session::last_ink_in_row`) ve NBSP taşıyan
  bir tampon iki tarafı ayrıştırırdı — gerekçesi o fonksiyonun doc'unda.

## Kabul

- `❤️` (U+2764 + U+FE0F) yazılan satır **bastırılıyor**: caret dock'ta,
  ızgara imleci gizli. Ölçülen kusurun tam tersi (`context.md` → Kanıt).
- Bastırmanın var olan bekçileri oynamıyor — özellikle bayat aynanın kendi
  sınaması (`a_stale_mirror_leaves_the_input_line_in_the_grid`): kapı
  **gevşemiyor**, yalnız birimi düzeliyor.
- `make hepsi` yeşil.

## Uygulama Notları

- **Süzgeç `Some(0)`'a bakıyor, `width() == 0`'a değil.** Kontrol
  karakterleri `None` dönüyor ve bu süzgece **girmiyorlar**: onların akıbeti
  zaten adıyla yazılı bir bilinen sınır (ZLE `^A` çiziyor, ayna `\x01`
  diyor) ve buranın konusu değil. `unwrap_or(0)` yazmak o sınırı sessizce
  değiştirirdi — phase-1'de aynı ayrımın tersi yönde bir regresyona yol
  açtığı için ikinci kez dikkat edildi.
- **İkinci bekçi zaten vardı.** Checklist "bayat ayna hâlâ bastırmayı
  bırakıyor" diyordu ve `a_stale_mirror_leaves_the_input_line_in_the_grid`
  tam onu soruyor; yeni bir sınama yazmak aynı iddianın ikinci kopyası
  olurdu. Kapının gevşemediğinin kanıtı o sınamanın yeşil kalması.
- **Kapı dört bulgu verdi ve ikisi gerçek kusurdu.** (1) **Caret'in
  altındaki karakter kaybolabiliyordu:** pencere sabit bir sütun ayırıyordu
  (`caret_col + 1`), oysa caret geniş bir glyph'in üstünde durduğunda o glyph
  iki sütun ister ve sağ kenar kuralı onu hiç çizmezdi — caret boş bir
  hücrenin üstünde kalırdı. 024 öncesinde caret'in altındaki karakter her
  zaman çiziliyordu, yani regresyondu. Karar 2 caret'ten **sonraki**
  karakteri kapsıyordu; bu onun altındakiydi. Bekçisi
  `the_window_reserves_the_whole_char_under_the_caret`.
  (2) **Bastırma aralığının hata yönü tersine dönmüştü:** yazılı sözleşme
  "yalnız eksik bastırır" diyordu ama birleştiriciler **fazla** bastırıyordu
  (`chars_after_cursor` onları sayıyor, sütunlar saymıyor) ve o güvensiz yön
  — NFD bir dosya adı satırı tam `cols`'un altına getirdiğinde altındaki
  tamamlama listesinin ilk satırı gizleniyordu. Çare `DockState`'e sütun
  ikizleri (`display_cols`, `cursor_col`) ve `SuppressedInput`'un
  `chars_*` alanlarının `cols_*` olması.
  (3) `CLAUDE.md` kodla çelişiyordu ("sütun sayısının tek yetkilisi ızgara"
  ve dock'un pencerelemesi karakter biriminde) — dosyanın kendi kuralı aynı
  commit'i istiyor, o yüzden phase-3'ten buraya alındı; yetkinin doğru adı
  **`unicode-width`'in tablosu** ve iki yüzey de onu okuyor, çünkü dock
  ızgaraya soramıyor (çizdiği şey ZLE'nin `BUFFER`'ı).
  (4) Üç bayat yorum ve **kaybolan bir kapsama**: 023'ün silinen bekçisi
  `render_context`'i CJK'lı bir `cwd` ile geçen tek sınamaydı ve yerine gelen
  dördü bağlam satırına hiç dokunmuyordu. Yeni bekçi
  `the_context_line_keeps_character_columns` o boşluğu dolduruyor ve bilinen
  sınırı (küçük sınıfta sütun kayması) çiviliyor.
- **`DockState`'in elle yazılmış `clone_from`/`reset`'i yeni alanları
  taşımıyordu** ve belirtisi bir sınamanın düşmesi oldu
  (`a_wrapped_input_line_is_suppressed_below_the_cursor_row_too`). Kopyalanan
  alan listesinin sessiz drift'i: `#[derive(Clone)]` olmadığı için derleyici
  uyarmıyor. Yeni alan ekleyen her değişiklik o iki listeyi de görmek
  zorunda.
- **Genişlik fonksiyonu tek yere indi.** İlk yazım `shell.rs`'te aynı kuralı
  ikinci kez ifade ediyordu (`width_of` kapanışı); `dock::column_width`
  `pub(crate)` oldu ve iki tüketici de onu çağırıyor — tam da bu setin
  kaçındığı "iki yetkili" kokusu, kendi kodumda.
- **`make test-yaris` tetiklendi ve beklenmiyordu.** phase dosyası yalnız
  `make hepsi` diyordu, ama `last_ink` OSC çözme yolunda hesaplanıyor ve o
  yol **PTY okuyucu thread'inde** koşuyor (`proje.md`'nin doğrulama tablosu
  o durumu adıyla sayıyor). Yedi yarış sınaması da yeşil; değişiklik saf bir
  süzgeç eklemesi, yeni paylaşılan durum yok. Aynı sebeple phase **riskli**
  sayıldı ve `/code-review` koştu.

## Checklist

- [x] `last_ink` sıfır genişliklileri atlıyor; `' '`/`'\t'` daraltması duruyor
- [x] Test: VS16'lı satır bastırılıyor → `a_combining_mark_does_not_make_the_mirror_look_stale`
- [x] Test: bayat ayna hâlâ bastırmayı bırakıyor — mevcut bekçi
      (`a_stale_mirror_leaves_the_input_line_in_the_grid`) yeşil kaldı
- [x] Doğrulama geçti (`make hepsi` yeşil; `make test-yaris` 7/7 — okuyucu
      thread'ine dokunuldu)
- [x] Riskli phase: `/code-review` koştu, dört bulgunun dördü giderildi
- [x] Test: caret'in altındaki karakter ayrılıyor → `the_window_reserves_the_whole_char_under_the_caret`
- [x] Test: bağlam satırı karakter biriminde → `the_context_line_keeps_character_columns`
