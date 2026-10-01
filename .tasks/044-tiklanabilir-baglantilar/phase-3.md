# Phase 3 — Çözümleme, açma politikası, jest ön-rotası (`bt-shell-common`)

## Özet

Yol adayını diskteki hedefe çeviren saf `resolve`, ne yapılacağını söyleyen
politika tablosu ve ⌘-tıkın jest defterindeki ön-rotası.

_Requirements: R5, R5.1, R6_

## Değişiklikler

- **`crates/bt-shell-common/src/links.rs`** (yeni) — `resolve(aday, cwd,
  home, stat)`: `~` ev dizinine, göreli yol OSC 7 dizinine, sonek atılmış;
  `stat` enjekte edilir (sınamada sahte, üretimde `std::fs::metadata`),
  sonuç `Dir` / `File { executable }` / yok. `action(hedef, tür) ->
  LinkAction::{OpenUrl, OpenFile, Reveal, OpenDir, Confirm, Swallow}`:
  `plan.md` → R5.1'in beyaz listesi; dosyada "bilinen içerik tipi" sorusu
  argüman (UTType sorgusu AppKit'te, phase-4), yani tablo beyaz listenin
  tümleyenini saf olarak taşır. `bateri://` her yoldan `Swallow`. Makine
  adını okuyan küçük fonksiyon (`libc::gethostname`, `child`'ın emsali).
  Kuyruk, `dispatch2` ve AppKit **yok** (`make audit`).
- **`crates/bt-shell-common/src/gesture.rs`** — `Gesture::pressed_link(aralık)`:
  rotayı basışta kilitler (`Drag::Ignore`), bırakmada `Release::Link`;
  `sent` bitini kurmaz, yani kayıp bırakma yolu (`take_lost_releases`) sızmaz.
  Doc'u yol haritasının "`button_route`'un dördüncü kolu" taslağının neden
  burada olduğunu `discussion.md` → Muhakeme'ye bağlar.

## Kabul

- `links::tests`: göreli/`~`/mutlak çözüm, sonek atılması, yok → bağlantı
  değil; politika tablosunun her satırı (çalıştırılabilir → `Reveal`,
  bilinmeyen tip → `Reveal`, dizin → `OpenDir`, `vscode://` → `Confirm`,
  `bateri://tab/x` → `Swallow`).
- `gesture::tests`: fare kipinde ⌘ + bağlantı basışı rapor da seçim de
  üretmez, sürükleme `Ignore`, bırakma `Release::Link` ve rapor yok; Shift
  basılı olsa da aynı.
- `make check` + `make linux` yeşil.

## Checklist

- [x] `links.rs`: `resolve`, `action`, makine adı
- [x] `gesture.rs`: `pressed_link`, `Release::Link`
- [x] Test: çözüm, politika tablosu, jest ön-rotası
- [x] Doğrulama geçti (`make check` + `make linux`)

## Uygulama Notları

- **API'nin biçimi.** `links::local_path(hedef, &LinkKind) -> Option<PathBuf>`
  (yol adayı olduğu gibi; düz metin ya da OSC 8 `file://` URL'sinin yüzde
  çözülmüş yolu — yetki denetimi phase-1'in hit testinde, burada tekrar
  edilmiyor), `resolve(aday, cwd, home, stat) -> Option<Resolved { path,
  entry: Entry::{Dir, File { executable }} }>`, üretim `stat`'ı
  (`std::fs::metadata`, symlink izlenir), `action(hedef, &LinkKind,
  Option<&Resolved>, content) -> Option<LinkAction>` (`None` = bağlantı değil:
  var olmayan yol ya da `file://`) ve `hostname()` (`libc::gethostname`).
  phase-4'ün akışı: `local_path` `Some` ise arka planda `resolve(.., stat)`,
  tıkta `action`.
- **SAPMA — "bilinen içerik tipi" `bool` değil üç değerli `Content`**
  (`Document`/`Package`/`Other`, AppKit cevaplıyor). Gerekçe: `.app`, `.pkg`,
  `.workflow` **dizin** ve `OpenDir` `openURL` ile açılırsa paket
  **çalışır/kurulur** — beyaz listenin korumak istediği şey. Paket dizini
  `Reveal`; bunu ayırmak `NSWorkspace::isFilePackageAtPath` / UTType
  `com.apple.package` sorusu, yani phase-4'ün `Content` cevabı bunu da
  vermeli (phase-4 checklist'ine yazıldı).
- **SAPMA — sonek `resolve`'da yeniden atılmıyor.** phase-1 `:satır:sütun`'u
  hedeften ayırıp `LinkKind::Path { line, col }`'a koyuyor; ikinci kez atmak
  adı gerçekten `:12` ile biten bir dosyayı bozardı. Kabul'ün "sonek
  atılması" maddesi bu sözleşmeyle sınanıyor
  (`the_suffix_is_bt_cores_and_is_not_stripped_again`).
- **SAPMA — `pressed_link()` aralık almıyor, `Release::Link` yüksüz.** Defter
  `Copy` (view `Cell`'de take-modify-put yapıyor); `Vec<LinkSpan>` onu
  bozardı. Kilitli aralık view'ın tuttuğu doğrulanmış hover'ın kendisi —
  bırakmada onunla karşılaştırılıyor (Muhakeme: hit test yeniden koşmuyor).
  Shift'in girdisi yok, yani "Shift basılı olsa da aynı" yapısal; sınama
  iki koşuyu da aynı çağrıyla geçiyor.
- **Küçük kararlar.** `~user/…` genişletilmiyor (bağlantı değil); göreli yol
  yalnız mutlak bir OSC 7 dizinine çözülüyor; `./src`'nin `.`'sı
  `components` ile atılıyor, `..` kalıyor (symlink'te sözcüksel `..`
  yanlış). `file://` yolunda `?`/`#` sonrası yol değil. Şemasız OSC 8 URI'si
  ve listedeki olmayan düz metin URL (tarayıcıdan gelemez) `Confirm` —
  güvenli yön. `bateri:` her türde ilk satır.
- `view.rs`'teki `Release` eşlemesine `Release::Link` kolu eklendi (no-op,
  phase-4'ü işaret ediyor); başka platform kabuğu kodu değişmedi.
