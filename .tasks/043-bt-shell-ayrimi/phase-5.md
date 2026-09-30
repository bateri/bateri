# Phase 5 — `bt-shell` → `bt-shell-macos`, denetim ve sözleşme

## Özet

AppKit crate'i adını alır, `make denetim` yeni katman kuralını ve ortak
crate'in platform sınırını denetler, sözleşme belgeleri güncellenir; set
kapısı (`discussion.md` → Karar 2, 5).

_Requirements: R4.1, R4.2, R4.3, R4.4_

## Değişiklikler

- **`crates/bt-shell` → `crates/bt-shell-macos`** (`git mv`), paket adı
  `bt-shell-macos`; `lib.rs` başlığı İngilizce yeniden yazılır (AppKit
  kabuğu, ortak crate'e işaretçi, `pacer` burada). Dosyaların içeriği ve
  yorum dili değişmez (Karar 3).
- **Kök `Cargo.toml`** — `members` ve `[workspace.dependencies]`.
- **`crates/bateri`** — `Cargo.toml` ve `main.rs`'in `bt_shell::` yolları;
  `bundle_assets.rs`'in `bt-shell` anmaları.
- **`Makefile`** — `denetim`: `cargo tree -p bt-shell-common -e normal
  --depth 1`'de `objc2-app-kit|objc2-quartz-core|objc2-foundation|objc2-user-notifications|block2|bt-shell-macos`
  yok; `crates/bt-shell-common/src`'de `objc2|dispatch2|block2` yalnız
  `watch`'ın macOS gövdesinde (yorum satırı hariç); başlık yorumuna kural.
  `bt-shell` anan yorumlar (41, 227, 562).
- **Belgeler** — `CLAUDE.md` (`bt-shell` → `bt-shell-macos` adı: diyagram,
  tablo, Dil istisnası, `crates/bt-shell/` yolları; ortak crate'in satırı
  phase-2/3'te geldi), `.claude/is-akisi/proje.md` (Doğrulama'nın
  denetim satırı), `docs/OLCUMLER.md` ve `.claude/is-akisi/olcum.md`'nin
  `app.rs` yolları, `docs/YOL-HARITASI.md` (043 satırının kapanışı, sonraki
  satırlarda `bt-shell` anmaları gerektiği kadar), `tools/linux/Dockerfile`
  başlığı.

## Kabul

- `make denetim` temiz ve kural sınandı: ortak crate'e geçici bir
  `objc2_app_kit` satırı kırmızı veriyor (geri alındı).
- macOS'ta sınama adları phase-4'ünkiyle aynı (crate öneki hariç).
- `make kur` yeşil (`crates/bateri` değişti); `make duman` yeşil, jetonlar
  aynı anahtarlarla.
- `make linux` yeşil.
- Kapanış mesajının gözle kontrol satırı: paketli uygulamada ayar kaydı
  canlı uygulanıyor, ⌘W koşan işi soruyor, dock var, açılışta `Last login`
  yok.
- `make hepsi` yeşil.

## Checklist

- [x] Dizin ve paket adı, `bateri`
- [x] `make denetim` kuralları ve sınanması
- [x] Belgeler güncel
- [x] Test: sınama adları phase-4'le aynı
- [x] Doğrulama geçti (`make hepsi` + `make linux` + `make kur` + `make duman`)

## Uygulama Notları

- **Denetim deseni `bt-shell-(macos|linux)`:** Karar 2 yalnız `bt-shell-macos`'u
  sayıyordu; katman kuralı iki platform kabuğunu da yasakladığı için desen
  ikisini birden arıyor. Sınandı: geçici `objc2-app-kit` bağımlılığı ve
  `use objc2_app_kit` satırı iki kuralı da kırmızıya çevirdi (geri alındı).
- **Yerinde bırakılan `bt-shell` anmaları:** `bt-core`/`bt-gpu`/`bt-atlas`
  yorumlarında `bt-shell` kabuk katmanının adı olarak geçiyor (Türkçe yorumlar,
  bu setin kapsamı değil — Karar 3); `.tasks/` tarihçesi ve yol haritasının
  kapanmış satırları da. Değişen: manifest yorumları, `bateri`, `Makefile`,
  `README.md` (tabloya ortak crate satırı), sarmalayıcı betiğin iki yorumu,
  `CLAUDE.md`'nin bütün anmaları ve `bt-shell-linux`'un yeri.
- `Cargo.lock` yalnız yeniden adlandırmanın doğal farkı (paket adı ve
  `bateri`'nin kenarı); sınama adları (1212) phase-4'le aynı.
- **Set kapısı `/code-review` (9 bulgu), düzeltilen üç:** ad değişiminden kalan
  `bt-shell` anmaları (`bt-shell-common`'ın `lib.rs` katman satırı ve sınırı,
  `child.rs`, `bt-shell-macos`'un yorumları ve `expect` metni); denetimin
  bağımlılık deseni `objc2` çekirdeğini de arıyor (sınandı, kırmızı); macOS
  `shell_command` sınamasındaki ilgisiz `zsh_wrapper_dir` iddiası kalktı
  (release profilinde yanlış birimi düşürüyordu; sarmalayıcıyı
  `the_zsh_wrapper_ships_with_the_crate` sınıyor). `/audit`: bulgu yok.

## WAIVE

Kalan altı bulgu, hepsi macOS'ta davranışsız ya da setin kaydedilmiş kararı:

1. `jobs::Procfs::name` `/proc/<pid>/comm` okuyor; doğrudan shebang'li mosh
   betiği (Fedora) `mosh` adıyla görünüp uzak oturum ssh sanılabilir. Linux'ta
   bugün tüketici yok — winit MVP'nin uzak oturum işi.
2. `Procfs` her yoklamada bütün `/proc`'u tarıyor ve `Undecided` komutta
   yoklama çıktı kenarı başına tekrar ediyor; doc "yalnız kapanışta ve komut
   başında" diyor. Linux UI thread'i MVP'de doğuyor; ölçü ve çare orada.
3. Linux'ta yerel düşüşü `en_US.UTF-8` ve varlığı sınanmıyor (`C.UTF-8`
   yalnız sistemde yanlış yerel). Discussion Karar 1'in açık kararı ("düşüş
   aynı"); değişikliği ürün kararı, MVP'nin paketleme/ortam işiyle.
4. inotify thread'i `notify`'ı mutex altında çağırıyor; `notify` içinden
   `Watch` düşürmek kilitlenir. Kural `watch` sözleşme doc'unda (phase-4
   Uygulama Notları), macOS çağıranı `exec_async`.
5. `test-support` dev-dependency'si tüketicisiz — R2.3'ün istediği bağ
   (phase-2 notu); ilk tüketici `bt-shell-linux`.
6. `bt-shell-common` `bt-gpu`'ya yalnız `FontNotice` için bağlı. Katman
   kuralı buna izin veriyor (`→ bt-gpu`); `FontNotice`'i aşağı taşımak bu
   setin kapsamı dışında (yeni davranış yok ilkesi), yol haritasına aday.

