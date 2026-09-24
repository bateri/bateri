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

- [ ] `dock::layout` ve sınamaları
- [ ] `suppress_to`/`suppress_floor` `layout`'tan; eşdeğerlik bekçisi
- [ ] Yedinci gövde: betik + çözücü + bütçe
- [ ] Test: altı gövdeli (eski betik) ayna hâlâ çözülüyor
- [ ] Doğrulama geçti (`make hepsi`, `make kur`, `make test-yaris`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
