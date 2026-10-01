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

- [x] Yardımcı oturum + önbellek
- [x] hyperlink: uzak doğrulama, etiket metinleri, menü öğeleri
- [x] `hyperlink.rs`'in iki `link_at` çağrısındaki `!hit.remote` süzgecini kaldır (phase-1'in geçici düşürmesi)
- [x] Onay sayfası (klasör, çakışma, yer)
- [x] `TerminalPane`'e indirme giriş noktası (phase-2'den): `Job::download(…, Lane::Queue, conflict)` + `Transfers::enqueue` + `start_transfers`; sayfanın Keep both / Replace cevabı `Conflict` olarak işe girer; `download_notify` ayarı bildirime bağlanır (bugün her biten kuyruk arkadayken bildiriyor)
- [x] Test: Kabul listesi
- [x] Doğrulama geçti (`make check`, `make linux`)

## Uygulama Notları

- Yardımcı oturum `bt-shell-common/src/remote_helper.rs`'te (phase dosyasının `bt-shell-macos/src/remote_probe.rs`'i değil): süreç, işçi thread'i ve önbellek platformsuz — `upload`'un süreçleri gibi — ve Kabul'ün yerel `sh` sınaması `download`'ın emsaliyle orada koşuyor; `remote_probe` adı 036'nın `pane::RemoteProbe`'uyla karışırdı. Ana kuyruğa dönüş macOS'ta (`hyperlink.rs`, `uploader.rs`, pane kimliğiyle).
- Önbellek yalnız **var olan** cevabı tutuyor (uzak nesil başına); yokluk view'ın `missing`'inde, ⌘ kalkınca unutuluyor — yoksa sonradan `touch` edilen ad o ssh oturumu boyunca bağlantı olmazdı. İndirmenin sorusu (`Ask::Count`) önbelleğe bakmaz, her seferinde taze.
- Tasarım sabitleri (ölçülmedi): `OPEN_TIMEOUT` 15 sn, `STAT_TIMEOUT` 10 sn, `COUNT_TIMEOUT` 120 sn, `IDLE` 120 sn ve açılamayan oturum için `RETRY_AFTER` 10 sn — aynı nesilde o süre içinde ikinci ssh denenmiyor, ilk hata aynen dönüyor (ulaşılamayan sunucuda her kelimeye bir bağlantı açılmasın). Güvensiz ad (`\`, kontrol karakteri) adaydan düşer, oturuma mal olmaz.
- Etiket notu (R1.2 metni, R1.3 hatası) `LinkState::note` ile hover'dan ayrı izleniyor; hover'ı temizleyen her kol notu da kaldırıyor. Cevap, sorulduğu uzak nesille dönüyor; nesil değiştiyse düşüyor.
- İndirme: `Ask::Count` + yerel hedefin hazırlığı (klasör yoksa yaratılır, `download::free_space` = `statvfs`, ad çakışması) yardımcının thread'inde; karar saf `remote_files::download_sheet`'te (`Ok(conflict)` sayfasız, `Err(sayfa)`). Sayfa upload'un `upload_alert`/`set_asking` yuvasını paylaşıyor, yani iki sayfa üst üste açılmıyor; onaydan sonra upload'un `upload_confirmed` yolu (kuyruk şeridi).
- "Download to Downloads" başlığı `download_dir` değişse de aynı (R3'ün metni). `download_notify` arkadaki **her** bitişin bildirimini kapatıyor (iki yön de; `docs/AYARLAR.md`'nin metni "transfer").
- Menü `autoenablesItems` kapalı kuruluyor; Open Preview (`openLinkFromMenu:`) dosyada gri görünüyor. Uzak bağlantıya ⌘-tık bugün bir şey yapmıyor (basış bağlantının, `links::action` uzak hit'te `None`) — phase-4.
- Yardımcının ssh'ı pane kapanırken (`begin_close`) ve uzak oturum bitince (`remote_or_title_changed`) kapatılıyor; boşta kalınca da kendisi.
- `objc2-app-kit`'e `NSOpenPanel`/`NSSavePanel`/`NSPanel` başlık bayrakları (Download To…); `Cargo.lock` değişmedi.
