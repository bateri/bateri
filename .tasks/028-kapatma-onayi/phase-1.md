# Phase 1 — Koşan işin tespiti

## Özet

Bir pencerenin kabuğunun dışında ön planda koşan işi ve adlarını veren modül;
UI yok, hiçbir kapanış yolu henüz onu çağırmıyor.

_Requirements: R1.1, R1.2, R1.3, R1.4, R1.5, R1.6_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `Session::child_pid() -> u32`:
  `tty::new`'in döndürdüğü `Pty`'nin `child().id()`'si, `TappedPty`'ye
  sarılmadan önce alınıp `Session`'da saklanır. Alacritty tipi sızmaz,
  `Cargo.toml` değişmez. Doc: pid PTY'nin çocuğunun, kabuğun değil (login
  yolu, `context.md`'deki ölçüm) ve biçildikten sonra bayatlayabilir —
  tüketen `reader_alive` ile birlikte sorar.
- **`crates/bt-shell/src/jobs.rs`** (yeni) — iki yarı:
  - **Saf karar**: girdisi kabuğun kimliği (`ShellParent::Login` |
    `Direct`), çocuğun pid'i ve bir süreç tablosu arayüzü (pid → ebeveyn,
    grup, ad; pid → çocuklar; grup → üyeler; kabuğun `e_tpgid`'i). Çıktısı
    `Foreground::Idle` | `Running(Vec<String>)` (boş vektör = adsız). Kural
    `discussion.md` → Karar 1: kabuğu bul, ön plan grubu kabuğun grubu mu,
    değilse adlar grubun yapraklarından (tekrarsız, pid sırası), yoksa
    liderin adı. Başarısızlık kolları R1.5.
  - **Okuyucu**: arayüzün `libc` gövdesi — `proc_listchildpids`,
    `proc_pidinfo` (`PROC_PIDTBSDINFO` yalnız kabuğa: `e_tpgid`, `pbi_pgid`;
    `PROC_PIDT_SHORTBSDINFO` lider ve üyelere: root'a ait `login`'de ve
    `sudo` grubunda da çalışıyor), `proc_listpgrppids`, ad için `proc_name`
    (kısa bilginin `comm`'u 16 karakterle kesik; `proc_name` başarısızsa
    ona düşer). `unsafe` blokların `// SAFETY:` gerekçesi; tampon boyları
    çağrının kendi dönüşünden.
  - Modül başlığı: neden OSC 133 değil, neden kabuğun `e_tpgid`'i (login
    root'a ait, `TBSDINFO` onda 0 — Muhakeme), bilinen sınırlar (Karar 7).
- **`crates/bt-shell/src/window.rs`** — `WindowIvars`'a kabuğun kimliği
  (`ShellParent`), `start_session`'da komuttan: `child::login_command()` ya
  da `None` (alacritty'nin macOS yolu da `login`) → `Login`; süreli koşunun
  betikleri → `Direct`. `TerminalWindow::foreground() -> Foreground`:
  oturum yoksa ya da `reader_alive` yanlışsa `Idle`, değilse `jobs`'a sorar.
  Henüz çağıranı yok — `#[allow(dead_code)]` yerine `pub(crate)` ve bir
  sınama tüketicisi yeterliyse o; değilse phase-2'ye kadar gerekçeli
  `allow`.
- **`crates/bt-shell/src/lib.rs`** — `mod jobs;`.

## Kabul

- Saf kararın sahte tablolu sınamaları: login yolunda boşta (ön plan =
  kabuk), koşan iş (ön plan = `vim`, ebeveyni kabuk), sarmalayıcı grubu
  (lider `bash`, yaprak `claude` → ad `claude`; bağlamdaki üçüncü satır),
  tekrarsız çok yaprak (`cat | grep`), çocuksuz `login` → boşta, okunamayan
  tablo → adsız koşuyor, doğrudan yolda boşta ve koşan iş.
- Gerçek PTY sınaması (`child.rs`'in `Session::spawn` örüntüsü, doğrudan
  yol, etkileşimli `/bin/zsh -f -i`): boşta `Idle`; kısa bir uyku (`sleep 5`) yazılınca
  `Running(["sleep"])`; sınama sonunda oturum `shutdown` ile kapanır, geride
  süreç kalmaz. Bekleme sınırlı ve sınamanın kendi son tarihiyle —
  mevcut PTY sınamalarının örüntüsü. Kontrol terminali olmayan ortamda
  `e_tpgid` 0 gelirse sınama "koşamadı" diye atlanmaz, düşer: o durumda
  okuyucu yanlış.
- `make hepsi` yeşil.

## Checklist

- [ ] `Session::child_pid`
- [ ] `jobs.rs`: saf karar + `libc` okuyucusu
- [ ] `ShellParent` pencerede, doğumda
- [ ] Test: sahte tablolu karar sınamaları (yukarıdaki yedi kol)
- [ ] Test: gerçek PTY'de boşta / `sleep`
- [ ] Doğrulama geçti (`make hepsi`)
