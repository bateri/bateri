# Phase 3b — Tam ekran uygulamada tekerlek: fare tekerlek raporu ve ok tuşları

## Özet

Tekerlek, fare raporlaması isteyen uygulamaya (vim/nvim `mouse=a`, htop)
**fare tekerlek dizisi** olarak, istemeyen tam ekran uygulamaya (`less`,
`man`) **ok tuşu** olarak gider; klavye oklarının kodlaması DECCKM'e
(`\e[?1h`) uyar.

_Requirements: R3.3, R3.4, R3.5_

---

## Neden bu phase var

phase-3 R3.3'ü ("alternate screen'de tekerlek yoksayılır") uyguladı ve
`/code-review` bedelini buldu: `man ls` ve `less` tekerlekle
kaydırılamıyor. Kullanıcı 2026-09-14'te iki karar verdi (`discussion.md` →
Karar 4 eki):

1. "less man gibi yerlerde scroll edememek çok kötü bir olay" → tekerlek
   alternate screen'de ok tuşlarına çevrilir (xterm'in "alternate scroll"
   kipi, DECSET 1007; alacritty'de `TermMode::ALTERNATE_SCROLL` **varsayılan
   açık**). phase-3'ün `## Uygulama Notları` → "WAIVE önerisi — DECSET 1007"
   maddesinde sınanmış bir taslak var; oradan başla.
2. Fare raporlaması isteyen uygulamalar (macOS vim'i `.vimrc` yoksa
   `defaults.vim` ile `mouse=a` açıyor; nvim'de varsayılan açık; htop) ok
   tuşu değil fare olayı bekler → fare raporlamasının **yalnız tekerlek
   kolu** bu phase'e girer. Tıklama, sürükleme ve hareket raporlaması girmez:
   onlar bugünkü gibi seçim yapar.

Sıra: phase-4'ten sonra, phase-5'ten (boşta kare sınırının yeniden ölçümü)
**önce** — ölçüm kodun son hâline alınmalı (R5.1).

---

## 1. Tekerleğin karar tablosu — `bt-core`'da

`crates/bt-core/src/session.rs` → `scroll_by` ve çevresi. Kip `Term`'de
yaşıyor, karar orada verilir (R3.3'ün katman gerekçesi aynen geçerli).
Sıra alacritty'nin kendi davranışıyla aynı (`alacritty/src/input/mod.rs` →
`scroll_terminal`, `mouse_report`, `normal_mouse_report`, `sgr_mouse_report`;
depoda yok — orkestratör 2026-09-14'te `master`'dan okudu, aşağıdaki
ayrıntılar oradan):

| koşul | tekerlek ne yapar |
|---|---|
| herhangi bir fare raporlama kipi açık (`TermMode::MOUSE_MODE`: 1000/1002/1003), ekran fark etmez | işaretçinin altındaki hücre için satır başına bir **tekerlek raporu** (§3) |
| değilse: `ALT_SCREEN` **ve** `ALTERNATE_SCROLL` açık **ve** Shift basılı değil | satır başına bir **ok tuşu** (§2): geriye (artı) → yukarı, ileriye (eksi) → aşağı |
| değilse: `ALT_SCREEN` açık (`\e[?1007l` ya da Shift basılı) | yoksay |
| değilse (birincil ekran) | `scroll_display` — **davranış değişmez** |

Shift+PgUp/PgDn (`scroll_page`) bu tablodan geçmez: klavyedir, fare raporu
ya da ok üretmez; alternate screen'de `None` → "tuşu uygulamaya geçir"
sözleşmesi aynen kalır.

Baytlar `write_owned`'dan mı yoksa doğrudan PTY'ye mi gidecek, karar senin;
gerekçesini yaz: `write_owned` girdide pencereyi dibe döndürüyor (phase-3
sapması). Birincil ekranda fare kipi açıkken pencere geçmişe kaydırılmışsa
alacritty görünen hücreyi grid mutlağına çeviriyor ve işaretçi **geçmiş**
satırındaysa (mutlak satır `< 0`) raporu **göndermiyor** — aynısını yap.

**Kare:** rapor ya da ok göndermek kare **istemez** — uygulama ekranını
yeniden çizince okuyucu thread'in `Wakeup`'ı kareyi getirir. Boşta sıfır kare
korunur.

**Dönüş tipi:** bugün alternate screen `None` döndürüyor ve `view` `None`'da
`scroll_carry`'yi sıfırlıyor. Baytlar gönderildiğinde artık sıfırlanmamalı —
yoksa trackpad'le yavaş kaydırmada her olayın küsuratı düşer ve uygulama
sarsak kayar. Ayrımı tipte taşı (ör. `Option<i32>` yerine kollu bir
`pub enum`; **alacritty tipi pub API'ye çıkmaz**).

**İşaretçi hücresi:** tekerlek raporu hücre koordinatı ister. `bt-shell`
`scrollWheel:`'de olayın konumunu mevcut `point_to_cell` ile hücreye çevirir
(yeniden türetme; ızgara dışı kırpması zaten orada) ve `bt-core`'a geçirir.
`SelectionPoint`'in `half` alanı rapora girmez — ya yalnız `col`/`row`
geçir ya da neden tümünü geçirdiğini yaz.

## 2. Ok kodlaması DECCKM'e uyar — tek kaynak

`crates/bt-shell/src/keys.rs` bugün okları koşulsuz `\e[A`…`\e[D` diye
kodluyor. Ama `TERM=xterm-256color`'ın terminfo'su `smkx=\E[?1h\E=` ve
`kcuu1=\EOA` diyor: terminfo okuyan uygulama (less, ncurses) açılışta DECCKM'i
açar ve `\EOA` bekler. Tekerleğin gönderdiği ok da aynı kurala uymak zorunda,
yoksa özellik tam da hedeflediği uygulamada boşa düşebilir.

- Kodlama **tek yerde**: tekerlek ve klavye aynı yardımcıdan geçer; ok baytı
  iki dosyada iki kez yazılmaz.
- DECCKM sorusu `bt-core`'da cevaplanır; `bt-shell` kip tutmaz ve alacritty
  tipini görmez.
- `APP_CURSOR` açık → `\eOA`/`\eOB`/`\eOC`/`\eOD`; kapalı → `\e[A`…`\e[D`.
  alacritty'nin klavye bağları da böyle.
- **Tekerleğin okları:** alacritty burada DECCKM'e **bakmıyor**, her zaman
  `\eOA`/`\eOB` gönderiyor; xterm DECCKM'e göre gönderiyor. İkisi de çalışır
  (uygulamalar iki biçimi de tanıyor). Tek kaynak ilkesi DECCKM'e duyarlı
  yardımcıyı paylaşmayı öneriyor; hangisini seçtiğini gerekçesiyle yaz.
- `keys.rs` Home/End kodluyorsa aynı kural onlara da uygulanır
  (`khome=\EOH`, `kend=\EOF`); kodlamıyorsa ekleme, kapsam dışı.
- Değiştiricili oklar (`\e[1;2A` vb.) kapsam dışı.

Klavye yolunda kip sorgusu tuş başına bir `Term` kilidi demek; phase-3 girdi
başına zaten bir kilit alıyor (`write_owned`). İkinci bir kilit ekleme —
birleştirilebiliyorsa birleştir, gerekçesini yaz.

## 3. Tekerlek raporu — yalnız tekerlek kolu

Düğme kodu: geriye/yukarı **64**, ileriye/aşağı **65**; tekerleğin bırakma
olayı yok, yalnız basma gönderilir. Koordinatlar 1 tabanlı (`col + 1`,
`row + 1`). Kodlama kipe göre:

| kip | bayt dizisi |
|---|---|
| SGR (`TermMode::SGR_MOUSE`, 1006) | `\e[<{kod};{col+1};{row+1}M` |
| UTF-8 (`TermMode::UTF8_MOUSE`, 1005) | `\e[M` + `32+kod` + UTF-8(`32+col+1`) + UTF-8(`32+row+1`) |
| düz (X10/normal) | `\e[M` + `32+kod` + `32+col+1` + `32+row+1` (her biri tek bayt) |

Sınır alacritty'de: `col` ya da `row` düz kipte `>= 223`, UTF-8 kipinde
`>= 2015` ise rapor **gönderilmez**. UTF-8 kipinde `32+1+pos` değeri `>= 128`
(yani `pos >= 95`) olunca iki bayt: `0xC0 + v/64`, `0x80 + (v & 63)`; altında
tek bayt. SGR'ın sınırı yok. Sınırları sınamaya bağla.

Kapsam dışı (notlara yaz): değiştirici bitleri (alacritty ekliyor: Shift +4,
Alt +8, Ctrl +16 — bizde `0`),
tıklama/sürükleme/hareket raporları, SGR-pixel (1016), yatay tekerlek
(66/67). Tıklama raporlanmadığı için `mouse=a` açık vim'de fareyle tıklamak
imleci taşımaz, seçim yapar — bugünkü davranış.

Kodlama `bt-core`'da platformsuz ve saf fonksiyon olarak sınanabilir olsun.

**`git log` bu phase'in konusu değil:** git `LESS` tanımsızsa `FRX` veriyor
ve `-X` less'in alternate screen'e geçişini kapatıyor; `git log` birincil
ekranda koşuyor ve tekerlek terminal geçmişini kaydırıyor — Terminal.app ve
alacritty'de de aynı. Göz kontrolüne bu yüzden girmedi.

---

## Uygulama Notları

## Yayın Etkisi

---

## Checklist

- [ ] phase-3'ün DECSET 1007 taslağı okundu, ondan başlandı
- [ ] Karar tablosu `bt-core`'da: fare kipi → rapor (geçmiş satırında rapor yok); alternate screen + 1007 + Shift yok → ok; `\e[?1007l`/Shift → yoksay; birincil ekran değişmedi
- [ ] Baytlar gönderildiğinde `scroll_carry` korunuyor (ayrım tipte, pub API'de alacritty tipi yok)
- [ ] İşaretçi hücresi `point_to_cell`'den geliyor
- [ ] Ok kodlaması DECCKM'e uyuyor; tekerlek ve klavye aynı kaynaktan
- [ ] Tekerlek raporu: SGR, UTF-8 ve düz kodlama; düz kipte sığmayan koordinatta rapor yok
- [ ] Test: alternate screen'de tekerlek → PTY'ye `\e[A`/`\e[B` (sayı ve yön doğru)
- [ ] Test: DECCKM açıkken klavye oku `\eOA`, kapalıyken `\e[A` gönderiyor; tekerleğin ok biçimi seçilen kurala uyuyor
- [ ] Test: `\e[?1000h\e[?1006h` altında tekerlek → `\e[<64;{c};{r}M` satır başına; aşağı yönde 65
- [ ] Test: `\e[?1000h` (SGR yok) altında düz kodlama doğru; `\e[?1005h` altında `pos >= 95` iki bayt; sınırı aşan koordinatta hiçbir şey gitmiyor
- [ ] Test: fare kipi alternate scroll'dan önce geliyor; `\e[?1007l` ve Shift altında hiçbir şey gitmiyor
- [ ] Test: rapor/ok göndermek kare istemiyor (kirli bayrağı dikilmiyor)
- [ ] `[elle]` göz kontrolü: `man ls` ve `less` tekerlek + trackpad'le kayıyor; `less`'te klavye okları çalışıyor; `nvim` (ya da `.vimrc`'siz `vim`) ve `htop` tekerlekle kayıyor; birincil ekranda geçmişe kaydırma bozulmadı
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**, kilit yolu değişirse `make test-yaris`)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
