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

- [ ] `link.rs` tarayıcısı ve sınamaları
- [ ] `LinkPoint`/`LinkHit`/`Session::link_at`, OSC 8 koşusu, `bateri://` elemesi
- [ ] Yerel yetki fonksiyonu (OSC 7 ile ortak), `SessionOptions::hostname`
- [ ] Test: sarma, geniş karakter/küme, OSC 8, bant, uzak oturum
- [ ] Doğrulama geçti (`make check` + `make linux`)
