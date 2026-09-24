# Phase 1 — Düzen fonksiyonu ve aynanın `PREBUFFER`'ı (görünmez)

## Özet

Satır farkında tek düzen fonksiyonunu kur, bastırmanın sınırlarını ona taşı
ve aynaya `PREBUFFER`'ı ekle; ekranda hiçbir şey değişmez (`Multiline`
duruyor).

_Requirements: R1.1, R3.3 (tel ve çözme yarısı)_

## Değişiklikler

- **`crates/bt-core/src/dock.rs`** — `layout`: görüntünün parçalarını
  (`PREBUFFER`, `PREDISPLAY`, `BUFFER`, `POSTDISPLAY`) satır sonlarında bölen
  ve verilen genişlikte saran tek yürüyüş; girdileri genişlik, ilk satırın
  başı, devam satırlarının başı; çıktısı görsel satırlar (her biri: hangi
  karakter aralığı, hangi sütundan) ve caret'in (satır, sütun)'u. Sütun
  sayısının tek yetkilisi yine `column_width`'in tablosu; `\n` genişlik
  almaz, satır kırar. Geniş karakter satır sonunda yarılanmaz, alt satıra
  geçer. `columns()` yürüyüşü bu phase'de dokunulmaz (dock çizimi hâlâ tek
  satır).
- **`crates/bt-core/src/session.rs`** (`frame()`) — `suppress_to` ve
  `suppress_floor` sütun bölmesi yerine `layout`'un ızgara parametrizasyonundan
  okur: ilk satırın başı imlecin ızgaradaki sütunundan gözlenir (`cursor_col` −
  imleçten önceki parçanın sütunu), devam 0. Tek satırda sonuç bugünkü
  formülle **aynı** (tam dolan satırın `saturating_sub(1)` kuralı dahil) —
  eşdeğerlik bekçisi. `PREBUFFER` ızgara hesabına girmez. Yorumdaki "hatası
  yönlü" paragrafı güncellenir.
- **`crates/bt-core/src/shell.rs`** — `decode_line` yedinci, **isteğe bağlı**
  gövdeyi (`b64(PREBUFFER)`) `KEYMAP`'in **arkasında** çözer; eksikse boş
  (eski betik). `DockState`'e alan; `PREBUFFER` bu phase'de `Multiline` kontrolüne
  **girmez** — her zaman `\n`'le bitiyor ve girseydi `for`'un ikinci satırı
  (bugün `Live`, dock'ta) ızgaraya düşerdi. Anlamını phase-4 verir.
- **`assets/shell/zsh/bateri.zsh`** — `__bateri_dock_redraw` yedinci gövdeyi
  basar; taşma kapısının toplamına `${#PREBUFFER}` girer; tel başlığı
  güncellenir. Yalnız dock'lu kademe (aynanın kendisi zaten öyle); rc
  dosyasına yazılmaz.

## Kabul

- `layout` sınamaları: satır sonu, sarma, satır sonunda geniş karakter, boş
  son satır (`echo a\n`), caret satır sonundan hemen sonra.
- Tek satırlık bütün mevcut bastırma bekçileri değişmeden yeşil; eşdeğerlik
  bekçisi eski formülün sonucunu sabit girdilerde karşılaştırır.
- Yedi gövdeli ve altı gövdeli ayna ikisi de çözülür; bekçi: yedinci gövde
  `\n` taşıyor, `BUFFER` tek satır → `Live`.
- `make hepsi`, `make kur` (betik) ve `make test-yaris` (okuyucu thread'in
  çözdüğü paylaşılan durum) yeşil.

## Checklist

- [x] `dock::layout` ve sınamaları
- [x] `suppress_to`/`suppress_floor` `layout`'tan; eşdeğerlik bekçisi
- [x] Yedinci gövde: betik + çözücü + bütçe
- [x] Test: altı gövdeli (eski betik) ayna hâlâ çözülüyor
- [x] Doğrulama geçti (`make hepsi`, `make test-yaris`)
- [~] `make kur` — otonom şeritte yasak (`target/release/bateri.app` kullanıcının açık örneği olabilir); yerine geçici paket kuruldu, beş betik dosyası `cmp` ile aynı, gerçek pencerede tek satır / `for` / iki satırlı yapıştırma HEAD'le aynı davrandı
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- **Metin `frame()`'e `Blocks`'un içinden geçiyor** (`input`, `input_caret`):
  `SuppressedInput` `Copy` ve metin taşımıyor, yürüyüş ise `Term` kilidinin
  altında (genişlik ve imlecin sütunu) koşmak zorunda. Kopya
  `suppressed_input()` ile aynı yaprak kilit turunda
  (`ShellLog::display_into`), yalnız bastırılan satır varken; çağıranın
  tamponu olduğu için kare başına ayırma yok ve `bt-gpu` değişmedi.
- **Izgara parametrizasyonu ayrı bir yardımcı** (`dock::grid_span`): ilk
  satırın başı `(cursor_col − imlecin mantıksal satırında imleçten önceki
  sütun) mod genişlik`. İmleç bir `\n`'in arkasındaysa başlangıç
  gözlenemiyor ve `0` varsayılıyor (üst uç eksik bastırır); bugün bu kola
  satır gelmiyor (`Multiline`), phase-4 kararını versin.
- **Caret kuralı:** caret sıradaki karakterin gideceği yerde, sonda bir
  sütunluk karakterin gideceği yerde — tam dolan satırın ardındaki caret alt
  satırın başında ve o satır sayılıyor. Eşdeğerlik bu kurala dayanıyor;
  phase-3 dock'ta aynı kuralı kullanırsa tam genişlikte yazılan satır dock'u
  bir satır büyütür (zsh'in ızgarasıyla aynı).
- **Eski formülden tek ayrılık geniş karakter:** imleçten sonraki kuyrukta
  satır sonuna sığmayan geniş glyph bölmede eksik sayılıyordu; yürüyüş doğru
  sayıyor (`grid_span_counts_the_row_a_wide_char_is_pushed_to`).
- **Bozuk yedinci gövde yükü bozuyor** (`Malformed`), yokluğu bozmuyor —
  öteki metin gövdeleriyle aynı kural.
