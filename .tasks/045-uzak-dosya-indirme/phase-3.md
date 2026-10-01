# Phase 3 — Uzak doğrulama, sağ tık indirmesi

## Özet

⌘-hover uzak adı yardımcı ssh oturumuna sorup vurgular; sağ tık menüsü
indirir, gerekirse onay sayfası açar.

_Requirements: R1.1, R1.2, R1.3, R3, R4_

## Değişiklikler

- **`crates/bt-shell-macos/src/remote_probe.rs`** (yeni; ad kodda seçilir) —
  pane başına yardımcı oturum: `ssh_argv` + `remote_files`'ın döngü betiği,
  arka plan thread'i, satır tabanlı istek/cevap, ana kuyruğa dönüş
  (`PaneLookup` emsali); tembel açılış, uzak neslin değişmesi ya da boşta
  kalma süresi (tasarım sabiti) kapatır; açılamazsa hata metni.
  Cevap önbelleği (uzak dizin, ad) → tür/boyut/mtime; nesil değişince boşalır.
- **`crates/bt-shell-macos/src/hyperlink.rs`** — uzak hit için `verify_paths`
  yerine yardımcı oturum (`resolve_first`'ün `stat`'ı uzak cevaptan, taban
  `remote_cwd`); `remote_cwd` boşsa göreli aday düşer, ⌘ altındaysa hedef
  etiketi R1.2'nin metnini gösterir; oturum açılamadıysa etiket nedeni. Sağ
  tık menüsü uzak yolda R3'ün öğeleri: Download to Downloads / Download To…
  (`NSOpenPanel`, klasör seçimi) / Copy Path / Copy as scp Path; Open
  Preview phase 4'e kadar gri.
- **`crates/bt-shell-macos/src/uploader.rs`** — indirme onay sayfası (R4):
  klasörde sayım + boyut, çakışmada Keep both / Replace (ya da ayarın
  sabitlediği kol), yerelde yer yoksa düğme kapalı; tek dosya sayfasız
  kuyruğa.
- **`crates/bt-core/src/session.rs`** — gerekiyorsa uzak hit'in hedef
  etiketi için küçük bir yardımcı; kare yoluna yeni hesap girmez.

## Kabul

- Sınamalar: yardımcı oturumun istek/cevap döngüsü yerel `sh` ile (ssh
  yerine enjekte); önbellek nesil değişiminde boşalır; boş `remote_cwd`'de
  göreli aday reddi.
- `make check` yeşil.
- Gözle kontrol (gerçek ssh): `ls -lh`'de dosya adı ⌘ altında kısa gecikmeyle
  çizilir, `deploy`/`Sep` çizilmez; OSC 7'siz sunucuda göreli adda etiket;
  sağ tık › Download to Downloads dosyayı karantinalı indirir; klasörde ve
  çakışmada sayfa.

## Checklist

- [ ] Yardımcı oturum + önbellek
- [ ] hyperlink: uzak doğrulama, etiket metinleri, menü öğeleri
- [ ] `hyperlink.rs`'in iki `link_at` çağrısındaki `!hit.remote` süzgecini kaldır (phase-1'in geçici düşürmesi)
- [ ] Onay sayfası (klasör, çakışma, yer)
- [ ] `TerminalPane`'e indirme giriş noktası (phase-2'den): `Job::download(…, Lane::Queue, conflict)` + `Transfers::enqueue` + `start_transfers`; sayfanın Keep both / Replace cevabı `Conflict` olarak işe girer; `download_notify` ayarı bildirime bağlanır (bugün her biten kuyruk arkadayken bildiriyor)
- [ ] Test: Kabul listesi
- [ ] Doğrulama geçti (`make check`)
