# Phase 1 — Kasten temizleme bayrağı

## Özet

PTY tarayıcısı `CSI 2 J`'yi tanır ve "ekran kasten temizlendi" bayrağını
kurar; bayrak ekran doğal yoldan dolunca düşer.

_Requirements: R1.1, R1.2, R1.3, R1.4_

## Neden önce

Doldurmadan **sonra** inseydi, arada kalan commit'te Ctrl-L geri alınmış
görünürdü — ölçülmüş bir regresyon (`context.md` → Kanıt 3, 4). Bu phase tek
başına hiçbir davranış değiştirmiyor: bayrağın tüketicisi phase-2'de doğuyor.

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — `ScanState`'e CSI durumu. Bugün `ESC [`
  `Ground`'a düşüyor (`:1515`'in `_ =>` kolu) ve modül doc'u "CSI bizi
  ilgilendirmiyor" diyor; ikisi de değişiyor ve **gerekçesi yazılıyor**.
  - Tanınan tek dizi `CSI 2 J`. `3J` ve RIS **yok**: ikisi de
    `clear_history()` çağırıyor (alacritty `term/mod.rs:1805`,
    `grid/mod.rs:341`), yani `history_size() == 0` ve doldurma kendiliğinden
    kapanıyor. Üçüncü bir kol önerisini kapatan bu cümle doc'a girer.
  - **İptal kuralları vte'den birebir taşınır** (R1.3): `ESC` → `Escape`,
    `0x18`/`0x1A` → `Ground`, parametre uzunluğuna `MAX_OSC_NUMBER` emsali
    tavan. Taşınmazsa bozuk bir CSI durumu takar ve peşinden gelen
    `ESC ] 133;…` yutulur — bloklar, bastırma ve dock **sessizce** ölür.
  - `feed`'in "baytlara dokunmama" garantisi korunur (R1.4).
- **`crates/bt-core/src/shell.rs` ya da `session.rs`** — bayrağın **yaşadığı
  yer bu phase'in asıl tasarım kararı.** İki kol var ve biri seçilip gerekçesi
  yazılır:
  - *nesil sayacı + compare-and-set* — temizleme yalnız kurulduğu neslin
    üstüne yazar;
  - *bayrak `Term` kilidinin altında* — okuyucu thread baytları uygulamak için
    o kilidi zaten alıyor ve "temizleme bir terminal olayı" gerekçesi oraya
    işaret ediyor.
  Seçilmezse şu dizi sessizce bozar: tarayıcı kurar → `frame()` kurulu okur →
  `Term` kilidi (baytlar henüz uygulanmadı, `content_rows == rows`) → kilit
  bırakılır → bayrak **temizlenir** → `Term` `2J`'yi uygular.
- **Ömür:** `content_rows == rows` **ve** `!alt_screen`. İkinci koşul zorunlu:
  alternatif ekranda `content_rows` tanımı gereği `rows`
  (`session.rs:2020-2021`, bekçi `:6270`), yani onsuz `vim` bayrağı düşürür.

## Kabul

- `CSI 2 J` bayrağı kuruyor; `CSI J`, `CSI 1 J`, `CSI 3 J` **kurmuyor**.
- Bozuk/yarım CSI'dan sonra gelen `ESC ] 133;A` hâlâ görülüyor (bekçi).
- `ESC [ ? 1049 h` gibi özel CSI'lar bayrağı kurmuyor ve durumu takmıyor.
- Alternatif ekranda geçen kareler bayrağı düşürmüyor.
- Bayrağın hiç tüketicisi yok: davranış bugünküyle **birebir** aynı.
- `make hepsi` ve `make test-yaris` yeşil.

## Yayın Etkisi

- **`CLAUDE.md`:** `bt-core` satırındaki "tarayıcının üç kolu var" cümlesi
  dördüncüye çıkıyor — ama yeni kol OSC değil CSI, ve yükü yok (bayrak).
- shell entegrasyonu: **yok** — betik ve tel değişmiyor, sinyal terminalin
  kendi gözlemi. Üç kabuk için de aynı.
- **Ölçüm bekliyor:** CSI kolunun yoğun akıştaki (vim, `less`) tarama
  maliyeti; hızlı yol artık CSI başına birkaç bayt fazladan adımlıyor.
- terminfo / `TERM` / ayar şeması / tema / app bundle / yeni bağımlılık: yok.

## Checklist

- [ ] CSI durumu ve `2J` tanıma yazıldı, iptal kuralları taşındı
- [ ] Bayrağın yaşadığı yer seçildi ve gerekçesi doc'a yazıldı
- [ ] Ömre `!alt_screen` koşulu kondu
- [ ] Test: `2J` kurar / `1J`,`3J`,`J` kurmaz
- [ ] Test: bozuk CSI'dan sonra OSC 133 hâlâ görülüyor
- [ ] Test: alternatif ekran bayrağı düşürmüyor
- [ ] Test: yarış (`make test-yaris`, iki zamanlama profili)
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris`)
- [ ] Riskli phase: `/code-review` koştu (paylaşılan durum), bulgular giderildi
- [ ] Yayın etkisi yazıldı
