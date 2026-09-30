# `bt-shell` ayrımı

## Hedef

`bt-shell`'in AppKit görmeyen on bir modülü `bt-shell-common`'a ayrılıyor,
geri kalanı `bt-shell-macos` oluyor. Süreç tablosu, kabuğun doğuşu ve dosya
izleme Linux gövdelerini de kazanıyor ve `make linux` ortak crate'i koşuyor.
macOS'ta kullanıcı **hiçbir fark görmüyor**. Katman kuralı:
`bateri → bt-shell-{macos,linux} → bt-shell-common → bt-gpu → {bt-atlas, bt-core}`.

## Gereksinimler

- **R1** — Çeviri: taşınan on bir modülün yorumları, doc'ları, `assert!`
  gerekçeleri ve tanı metinleri İngilizce; yorumsuz fark yalnız dizgi.
- **R2** — Ortak crate
  - **R2.1** — `bt-shell-common` workspace üyesi; `settings`, `split`,
    `zoom`, `notices`, `gesture`, `quote`, `keys`, `upload`, `jobs`, `child`,
    `watch` orada, `bt-shell` onlara ondan bağlanıyor.
  - **R2.2** — Ortak crate AppKit, Quartz, Foundation ve bildirim merkezini
    görmüyor; macOS'a özgü tek bağımlılığı `dispatch2`, `cfg(macos)`
    altında. `NSLocale` okuması macOS crate'inde.
  - **R2.3** — Sınama yardımcıları `test-support` özelliğinin arkasında;
    macOS'ta sınama adları (öneksiz) ayrımdan öncekiyle aynı.
  - **R2.4** — Taşınan öğelere bakan belge işaretçileri taşıyan commit'te
    güncel.
- **R3** — Linux gövdeleri
  - **R3.1** — `jobs`: `ProcessTable`'ın `/proc` gövdesi; gerçek PTY
    sınaması Linux'ta da geçiyor.
  - **R3.2** — `child`: `shell_command` komutu ve `ShellParent`'ı birlikte
    veriyor (macOS `login -qflp` + `Login`, Linux `$SHELL -l` + `Direct`);
    macOS kolunu sabitleyen `cfg(macos)` sınama.
  - **R3.3** — `watch`: kuyruksuz `install(paths, notify)`, bildirim iki
    platformda arka plandan, macOS çağıranı ana kuyruğa taşıyor; Linux
    gövdesi inotify; yedi sınama iki platformda geçiyor.
  - **R3.4** — `make linux` `bt-shell-common`'ı da koşuyor ve yeşil.
- **R4** — Adlandırma ve sözleşme
  - **R4.1** — `crates/bt-shell` → `crates/bt-shell-macos` (paket
    `bt-shell-macos`); `bateri` ona bağlı.
  - **R4.2** — `make denetim` yeni katman kuralını ve ortak crate'in platform
    sınırını (discussion.md → Karar 2) denetliyor.
  - **R4.3** — `CLAUDE.md` (katman diyagramı ve tablosu, Dil, Komutlar),
    `proje.md`, `Makefile` yorumları, `tools/linux/Dockerfile` başlığı ve yol
    haritası güncel.
  - **R4.4** — `make kur` ve `make duman` yeşil; paketli uygulamada davranış
    aynı.

## Yaklaşım

1. Taşınacak on bir modülün yerinde çevirisi (phase-1).
2. `bt-shell-common`'ın doğuşu ve taşıma; macOS gövdeleri `cfg`'li,
   `NSLocale` okuması `bt-shell`'e, `test-support`, belge işaretçileri
   (phase-2).
3. `jobs`'un `/proc` gövdesi, `child::shell_command` ve Linux kolu, `make
   linux` += `bt-shell-common` (phase-3).
4. `watch`'ın tek bildirim sözleşmesi ve inotify gövdesi (phase-4).
5. `bt-shell` → `bt-shell-macos`, denetim, sözleşme belgeleri, set kapısı
   (phase-5).

Gerekçeler `discussion.md` → Karar 1–5. Dil kısıtı 040 `plan.md` → Dil
kısıtı; kapsamı discussion.md → Karar 3.

## Kapsam Dışı

- `app.rs`/`lib.rs`'in AppKit'siz yarısı (süreli koşu tipleri, bekçi, duman
  raporu ve hükmü, girdi/hareket kararları, entegrasyon ortamı, ızgara
  bölmesi) — winit MVP setinin ilk taşıması (discussion.md → Karar 4).
- Platformsuz tuş sözlüğü (`keys` AppKit'in fonksiyon tuşu kod noktalarıyla
  taşınıyor), `bt-shell-linux`, Linux `Pacer`'ı — winit MVP.
- Linux release derlemesinde betik dizininin yeri — paketleme seti (bilinen
  sınır: orada entegrasyon kurulmaz).
- `bt-shell-macos`'ta yerinde kalan AppKit kodunun yorum çevirisi
  (discussion.md → Karar 3).
- Yeni dış crate. Yeni davranış.

## Akış

```
phase-1  yerinde çeviri (11 modül, kod aynı)
phase-2  bt-shell-common doğar ── 11 modül taşınır (macOS gövdeleri cfg'li) ── bt-shell ona bağlı
phase-3  jobs /proc + child::shell_command ── make linux += bt-shell-common
phase-4  watch: tek sözleşme (arka plan → çağıran ana kuyruğa) + inotify
phase-5  bt-shell → bt-shell-macos ── denetim ── belgeler ── make kur / duman ── set kapısı
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | |
| phase-4 | |
| phase-5 | |
| kapı | |
