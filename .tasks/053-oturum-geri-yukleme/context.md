# Oturum geri yükleme — Bağlam

## Mevcut Durum

Açılışta `AppDelegate::did_finish_launching` (`bt-shell-macos/src/app.rs`)
tek bir `open_window(None, Opening::Window)` çağırıyor: ev dizininde, sıfır
punto farkıyla, ayardan çözülen temada tek pane'li tek pencere. Kapanışta
`applicationWillTerminate:` → `AppDelegate::shutdown` her pane'in
`Session`'ına `SIGHUP` gönderip tek son tarihe kadar bekliyor (`CLAUDE.md` →
Kapanış sınırlı bekler). Arada hiçbir şey diske yazılmıyor: pencere, sekme,
bölme ağacı, odak, zoom, pane'in dizini ve geçmişi süreçle birlikte ölüyor.
AppKit'in kendi pencere geri yüklemesi (`NSWindowRestoration`) kurulu değil.

Geri kurulacak her parçanın bugün bir sahibi ve okuyanı var — yeni bir
kaynak icat edilmiyor, var olanlar okunuyor:

| parça | bugünkü sahibi |
|---|---|
| sekme grubu ve sırası | `NSWindowTabGroup` (026), `TerminalWindow::show_as_tab_of` |
| bölme ağacı (eksen + oran) | `bt_shell_common::split::Tree` — `pub`, doğrudan kurulabilir (039) |
| odaktaki pane, zoom | `WindowIvars::focused`, `SplitView::zoomed` (039) |
| pane'in yerel dizini | `Session::working_directory` (OSC 7, yerel yetki) |
| punto farkı | `TerminalPane::zoom` → `Zoom` (`bt-shell-common::zoom`) |
| uzak hedef | `Session::remote_line` — ⌘T'nin ilk girdisi (`initial_line`, 037 Karar 6) |
| pane kimliği | `TabId` (038), `pane.rs` → `new_tab_id()`; `BATERI_TAB_URL`, `bateri focus` (050) |
| geçmiş + ekran | alacritty `Term`'ün ızgarası, `bt-core`'un `Session`'ı arkasında kapsüllü |
| tema | ayar dosyası (`[appearance] theme`); Theme ▸ oraya yazıyor, pane'e özgü tema yok |

İki yapı taşı ölçüldü ve elde:

- **Oynatma yolu var.** Okuyucu döngü `bt-core`'un (`reader.rs`, 035) ve
  ayrıştırıcı `Term`'ü yalnız `handler::ClusterHandler` üstünden görüyor;
  aynı ikili (`ansi::Processor` + `ClusterHandler`) `Session::spawn`'da
  `EventLoop`'tan önce kurulup kayıtlı baytları `Term`'e uygulayabilir —
  `Scanner`'dan (OSC 133/8133/7) geçmeden, emoji kümelerini bozmadan.
- **Alternatif ekrandaki birincil ızgara okunabilir.** `CLAUDE.md` onu
  "erişilemez" diyor (`Term::inactive_grid` özel) ama `Term::swap_alt` `pub`
  (alacritty 0.26.0, `term/mod.rs`); kapanışta bir kez çevirip birincili
  okumak mümkün, geri çevirmek alt ekranı sıfırlıyor ve kapanışta önemsiz.

Bağımlılık grafı ölçüldü (`cargo tree --target aarch64-apple-darwin`):
`serde` ve `serde_json` grafta **yok**, `alacritty_terminal`'ın `serde`
özelliği kapalı (`default-features = false`), `toml_edit` var ama yalnız
`bt-core`'un doğrudan bağımlılığı. bateri'nin kendi durum dosyası
(`remote-hosts`) satır biçiminde, geçici ad + `rename` ile yazılıyor
(`bt-shell-common/src/ssh_wrap.rs`).

## Motivasyon

Sparkle 2'nin güncellemesi uygulamayı `terminate:` ile kapatıp yeni sürümü
açıyor; bugün her güncelleme kullanıcının bütün düzenini siliyor. Aynı kayıp
⌘Q'da, oturum kapatmada ve yeniden başlatmada da var. Referans ürünün
`restore_windows` ayarı var ve "pencere geri yükleme" ürün özelliği
(`docs/ARASTIRMA.md` → Ayar anahtarlarının tamamı, Ürün özellikleri).

Bu set **birinci seviye**: düzen ve geçmişin metni geri gelir, kabuklar
yenidir, koşan işler ölür (bilinçli). Sparkle'ın kapanışı da `terminate:`'ten
geçtiği için koşan işte `confirm_close` sorusu açılır ve "Cancel" güncellemeyi
de erteler — bu sette değişmiyor.

## Sonraki set ile ilişki (Set B — canlı devir)

İkinci seviye ayrı bir set olarak açılacak (`docs/YOL-HARITASI.md` →
"güncellemede canlı devir"): yalnız güncelleme kapanışında `SIGHUP`
gönderilmeyecek; `bateri`'nin bir alt komutu olan tutucu süreç PTY master
fd'lerini `SCM_RIGHTS` ile alıp aradaki çıktıyı tamponlayacak, yeni sürüm
fd'leri + tam VT durumunu (ızgara, geçmiş, imleç, modlar, alt ekran, kayıtlı
imleç, charset, tab durakları; kabuk entegrasyonu defteri, dock aynası, uzak
oturum durumu) + tamponu alıp oynatacak. Nihai kullanıcı ölçütü: ssh ile bağlı
uzak sunucuda koşan bir Claude Code oturumu güncellemeden sonra kopmadan,
ekranı yerinde geri gelir.

Bu set o işin **temeli ve geri düşüşü** olarak tasarlanıyor, yani üç şeyi
Set B'ye bırakmak zorunda:

1. **Sürümlü biçim.** Düzen dosyası ilk satırında bir sürüm taşır ve okuyucu
   tanımadığı sürümü yok sayar (temiz açılış). Set B kendi verisini ekleyip
   sürümü artırır; "Set B'nin verisi kullanılamazsa bu setin alanlarına —
   yeni kabuk + oynatılan geçmiş — düş" kuralı ve bekçisi Set B'nindir.
2. **Kararlı pane kimliği.** Geri yüklenen pane kayıtlı `TabId`'yi geri alır;
   Set B'nin fd'yi doğru pane'e bağlaması ve dış kancaların
   (`bateri focus bateri://tab/<UUID>`, 050) güncellemeden sonra aynı pane'i
   bulması buna dayanıyor.
3. **Geri yüklemenin tek giriş yolu.** Bugün yok: pencere `open_window`'dan
   tek pane'le, bölmeler `add_pane`'den doğuyor ve `add_pane` kabuğu hemen
   başlatıyor. Bu set bütün pane'leri kayıtlı ağaçla önce yerleştirip
   kabukları **sonra** başlatan tek yolu kuruyor (`discussion.md` → Muhakeme);
   Set B o noktada yeni kabuk yerine devralınan fd'yi verir.

Set B'nin tam VT durumu (modlar, kayıtlı imleç, charset) bu setin geçmiş
biçimiyle taşınmaz; o setin kendi biçim/bağımlılık kararıdır.
