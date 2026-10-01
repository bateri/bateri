# Uzak dosyayı indirme ve önizleme — Bağlam

## Mevcut Durum

ssh/mosh oturumunda (036) bateri uzak tarafı yalnız **yerelden** biliyor:
hedef (`RemoteTarget`, `jobs::remote`'un argv'si), uzak OSC 7 dizini
(`DockContext::remote_cwd`; uzak kabuk OSC 7 basmıyorsa boş) ve başlık.

**Bağlantılar (044) uzakta kapalı.** `Session::link_at`'in kapısı
(`link_allowed`) uzak oturumda her düz metin yolu ve her `file://`'yi
`None`'a çeviriyor; URL ve OSC 8 (`file://` dışı) geçiyor. Gerekçesi
`.tasks/044-tiklanabilir-baglantilar/discussion.md` → "Uzak oturum": yol
yerel diskte doğrulanamıyor. Aday üretimi (`link::path_candidates`,
`LinkHit::candidates`/`choose`) ve yerel doğrulama kuyruğu
(`hyperlink::verify_paths` → `links::resolve_first`) platformsuz ve
enjekte edilebilir `stat` alıyor; göreli taban bugün yalnız **yerel** dizin
(`Session::working_directory`).

**Jest:** ⌘-basış bağlantıya kilitleniyor (`Gesture::pressed_link`),
sürükleme yutuluyor (`Drag::Ignore`), açma bırakmada (`Release::Link`).
Sürükleme eşiği ya da basış konumu tutulmuyor. View yalnız sürükleme
**hedefi** (`NSDraggingDestination`, Finder damlası → 037 yüklemesi); hiçbir
yerde `NSDraggingSource`/`NSFilePromiseProvider` yok ve `objc2-app-kit`'te
o başlıkların özellik bayrakları kapalı.

**Aktarım altyapısı (037) tek yönlü.** `upload::ssh_argv` kullanıcının ssh
seçeneklerini (`-p`, `-i`, `-J`, `-o`, `-F` …) `BatchMode=yes`,
`ControlMaster=no` ile yeniden kullanılabilir bir argv'ye çeviriyor;
`probe`/`transfer`/`TarWatcher`/`Shared` (iptal, disk dolu, ilerleme) ve
`Uploads` kuyruğu (sıra, durum satırı, liste, durdurma sorusu, başlık öneki)
`bt-shell-common`'da, sayfa/popover/bildirim/Dock simgesi
`bt-shell-macos::uploader`'da. Yön metinlerde gömülü (`↑`, "Upload…",
"Stop uploading?", `titled`'ın `↑`'su); `bt-core`'un `Transfer`'ı ve dock
düğmeleri (`Cancel`, `Cancel all`, `Show files (N)`) yönden bağımsız.

**Ayarlar:** `[remote]` bugün yalnız `hosts`. Ayar penceresinde dört
kategori (General, Appearance, Cursor, Motion), satır türleri
popup/switch/slider/field/stepper; düğme ya da klasör seçici satırı yok.

## Motivasyon

Kullanıcı isteği (2026-10-01): ssh'tayken `ls` çıktısındaki bir dosyaya
bakmak ya da onu almak bugün terminali terk etmeyi (ayrı `scp`, SFTP
istemcisi) gerektiriyor. İstenen üç jest: **⌘-tık önizler** (yereldeki
⌘-tık "aç" anlamının uzak karşılığı), **⌘-sürükle Finder'a indirir**,
**sağ tık menüsü indirir** — upload'daki kuyruk, ilerleme ve iptal
deneyimiyle, ters yönde.

Emsal: iTerm2 aynı işi yalnız sağ tıkla ve uzakta kendi betiği kuruluysa
yapıyor ("Download with scp from {host}"; uzakta ⌘-tık çalışmıyor). SFTP
istemcileri (Transmit, Cyberduck) uzak dosyayı Finder'a file promise ile
sürüklüyor. Terminal metninden Finder'a sürükleyip indiren bir terminal
bulunamadı.

Tasarım tuvali (ekranlar ve notlar, konuşmada kararlaştırıldı):
https://claude.ai/artifact/HiF2Rr8wJ4S1msMd8oCATE
