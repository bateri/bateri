# Phase 1 — Tarayıcı ve hit test (`bt-core`)

## Özet

Bir noktanın altındaki bağlantıyı (aralık, hedef, tür, damga) veren saf
tarayıcı ve `Session::link_at`; kare yoluna ve görüntüye dokunulmaz.

_Requirements: R1, R1.1, R1.2, R2, R3_

## Değişiklikler

- **`crates/bt-core/src/link.rs`** (yeni) — `scan(&str) -> Vec<Found>`:
  şema önekli URL'ler (`http://`, `https://`, `ftp://`, `mailto:`,
  `file://`) ve yol adayları (boşluk/tırnak sınırlı belirteç; `/`, `~/`,
  `./`, `../` ile başlayan, `/` taşıyan ya da uzantılı çıplak ad), sondaki
  noktalamanın kırpılması, parantez/köşeli parantez dengesi, `:satır`,
  `:satır:sütun`, `(satır,sütun)` sonekinin hedeften ayrılması (tür ve
  sonek ayrı alanlar). Karar yalnız sözdizimi: varlık burada sorulmaz
  (dosya G/Ç yok). Modül başlığı neden regex olmadığını tek cümleyle
  `discussion.md` → Karar 1'e bağlar.
- **`crates/bt-core/src/session.rs`** — `pub enum LinkPoint { Screen { row:
  i32, col: u16 }, Dock(..) }` (dock kolu phase-5'te dolar; bu phase'de
  `None`), `pub struct LinkHit` (hücre aralığı — ekran satırı + sütun, sarmada
  birden çok satır —, hedef dizgisi, `LinkKind::{Url, Path { line, col },
  Osc8}`, damga) ve `Session::link_at`. Tek `Term` kilidi turunda: satırı
  `drawn_lines` ile kapıla (negatif satır = bant, `cover_of`'un aritmetiği),
  önce hücrenin OSC 8'i (aynı `Hyperlink`'in bitişik koşusu, sarma boyunca),
  yoksa mantıksal satırın dizgisi (`search::wraps` + `WRAP_REACH`; küme ve
  geniş spacer atlanarak hücre ↔ karakter eşlemesi) → `link::scan` → noktayı
  kapsayan aday. `bateri://` önekli hyperlink yokmuş gibi (altındaki düz
  metin yine taranır). Damga aramanın `LedgerMark`'ı (OSC 8'de ayrıca
  `Hyperlink`).
- **Uzak ve yetki** — uzak oturumda (`DockContext::remote`) `Path` ve
  `file://` sonucu verilmez. `file://` yetkisinin "yerel mi" sorusu tek
  fonksiyona iner ve OSC 7 de onu çağırır (`shell.rs` → `LOCAL_AUTHORITIES`
  + isteğe bağlı makine adı); `SessionOptions`'a `hostname: Option<String>`
  girer, bu phase'de çağıranlar `None` geçer (davranış aynı; adı phase-4
  okur). `LOCAL_AUTHORITIES`'in doc'undaki "bilinen sınır" paragrafı çarenin
  geldiğini söyleyecek biçimde güncellenir.

## Kabul

- `link::tests`: noktalama (`https://x.dev).`), parantez
  (`…/wiki/X_(Y)`), `mailto:`, `src/main.rs:12:5`, `(12,5)`, `~/x`, Türkçe
  karakterli yol, boşlukla bitişik iki bağlantı.
- `session` sınamaları gerçek PTY ile: sarılmış satıra bölünen URL tek
  `LinkHit`; geniş karakter ve `🇹🇷` sonrası sütunlar doğru; OSC 8 metni
  hedefinden farklı (`ESC]8;;https://a.dev ESC\ tıkla ESC]8;; ESC\`) →
  hedef `https://a.dev`; `bateri://block/N` asla hit vermiyor; bant satırı
  (negatif) hit veriyor; uzak oturumda yol `None`, URL var.
- `make check` + `make linux` yeşil.

## Checklist

- [x] `link.rs` tarayıcısı ve sınamaları
- [x] `LinkPoint`/`LinkHit`/`Session::link_at`, OSC 8 koşusu, `bateri://` elemesi
- [x] Yerel yetki fonksiyonu (OSC 7 ile ortak), `SessionOptions::hostname`
- [x] Test: sarma, geniş karakter/küme, OSC 8, bant, uzak oturum
- [x] Doğrulama geçti (`make check` + `make linux`)

## Uygulama Notları

- **Sınırın biçimi.** `LinkHit { spans: Vec<LinkSpan>, target, kind, stamp }`;
  `LinkSpan { row: i32, first, last }` `LinkPoint::Screen` ile aynı uzayda
  (ekran satırı, negatif = bant), yani phase-2 hücreyi doğrudan karşılaştırır.
  `LinkStamp` opak (`LedgerMark` + OSC 8'de `(id, uri)` dizgileri; alacritty
  tipi `pub` API'ye çıkmıyor) ve `PartialEq` taşıyor — phase-2'nin
  karşılaştırması bu eşitlik. `LinkPoint::Dock` kolunun yükü `SelectionPoint`
  (`dock_select`'in noktası); bugün `None`.
- **Tarayıcının küçük kararları** (`link.rs`): belirteç sınırı boşluk +
  `"'`` ` ``<>` (kaçırılmış boşluk izlenmiyor — `~/a\ b` iki belirteç);
  şema belirtecin başında ya da harf/rakam olmayan bir karakterden sonra
  (`url=https://…`); yolun çevreleyen parantez çifti atılıyor
  (`(src/x.rs)`); `(satır)` soneki de `(satır,sütun)`'un yanında tanınıyor;
  uzantılı çıplak adın uzantısı bir harf taşımalı (`1.5` aday değil) ve
  `://` taşıyan belirteç yol değil. `file_authority`/`is_file_url` de orada.
- **Set sonrası (kullanıcı kararı, 2026-10-01):** uzantı şartı kalktı —
  her çıplak kelime aday (`ls`'in `src` klasörü, `Makefile`), karar
  varlıkta; iTerm2'nin semantic history kuralı. Belirti kullanıcıda
  görüldü: `ls` çıktısında dosyalar tıklanıyor, klasörler tıklanmıyordu.
- **OSC 8 koşusunda spacer** koşuyu bölmüyor ama yalnız bağlantılı iki hücre
  arasında ya da bağlantılı geniş karakterin sağ yarısı olarak sayılıyor.
  Bitişik iki id'siz bağlantının alacritty'nin ürettiği id'leri ayrı, yani
  aynı URI'li iki ayrı OSC 8 iki koşu (Karar 2 ile uyumlu).
- **`file://`'nin kapısı** düz metin URL'de de OSC 8'de de aynı: uzak
  oturumda hiçbiri; yerelde yetki `shell::is_local_authority`'den (boş,
  `localhost`, verilmişse makine adı; büyük/küçük harf duyarsız, boş ad
  "ad yok"). OSC 7 aynı fonksiyonu `Scanner::hostname`'den çağırıyor.
  Bu phase'de bütün çağıranlar `hostname: None` geçiyor (davranış aynı;
  `pane.rs`'teki satır phase-4'ü işaret ediyor).
- `search::wraps` ve `WRAP_REACH` `pub(crate)` oldu (kopya değil).
- `make test-race` gerekmedi: paylaşılan değişken durum yok — `hostname`
  açılışta yazılan sabit bir alan, `link_at` yalnız okuyor (uzak bit
  yaprak kilitte `Term`'den önce, damga mevcut atomiklerden).

