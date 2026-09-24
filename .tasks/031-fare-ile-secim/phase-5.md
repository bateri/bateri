# Phase 5 — Dock düzenleme: tıkla-caret, seçimi silme ve üstüne yazma

## Özet

Sarmalayıcı bir widget kuruyor, terminal ona tek komutla (`d;S;E;L`) aralık
silip caret taşıtıyor; seçim varken ⌫/⌦/yazma/yapıştırma/⌘X/←/→/⇧←/⇧→
Karar 8'in tablosuyla davranıyor.

_Requirements: R4.1, R4.2, R4.3, R4.4, R4.5_

## Değişiklikler

- **`assets/shell/zsh/bateri.zsh`** — dock'lu kademede (`__bateri_dock`)
  `__bateri_dock_edit` widget'ı: yükü `read -k` ile BEL'e kadar okur
  (zaman aşımı adlı ve gerekçeli bir değişken — BEL'siz dizi ancak bozuk bir
  telde), `d;S;E;L`'yi ayrıştırır, `${#BUFFER} != L` ise ya da sayılar aralık
  dışıysa hiçbir şey yapmaz, değilse `[S,E)`'yi siler ve `CURSOR=S`. `emulate
  -L zsh`. Bağlama `main`, `emacs`, `viins`'e `\e[8133~` ile **her
  `line-init`'te** (yerleşik, fork yok) ve hemen ardından yetenek
  `\e]8133;w\a`. Tel başlığı (ESC ] 8133 listesi) ilk kez terminalden kabuğa
  giden yönü ve dizinin biçimini anlatır. Kullanıcının rc dosyasına dokunulmaz.
- **`crates/bt-core/src/shell.rs`** — `8133;w` → `DockState`'in yanında bu
  prompt'un yeteneği; `8133;e` (`line-finish`) siler. Çözücü sınaması.
- **`crates/bt-core/src/session.rs`** — düzenleme kapısı (`can_edit_dock`):
  `suppressed_input` + `insert_keymap` + `answers` güncel nesil + yetenek; komut
  `send_input`'tan geçer (nesil ilerler, iki seçim temizlenir). `dock_delete_selection`,
  `dock_move_caret(index)`, `dock_replace_selection` yardımcıları: yazma ve
  yapıştırmada önce `d`, sonra olağan `write`/`paste` (kararı `paste`'in).
  ⇧←/⇧→ yalnız terminalde seçimi büyütür/başlatır (caret'in indeksi
  aynadan). Kapı kapalıyken hiçbir komut yok; seçim yalnız kopyalanabilir.
- **`crates/bt-shell/src/view.rs`** — `keyDown:`'da dock seçimi varken tuş
  tablosu (Karar 8) — `reaches_terminal`'ın izin listesi değişmez;
  `insertText:` ve yapıştırma seçimin yerine yazar; sürüklemesiz tıklamanın
  bırakmasında caret taşınır (`d;N;N;L`); öneriye tıklamak `BUFFER`'ın sonuna.
  `cut:` eylemi ve `validateMenuItem:` (Cut yalnız dock seçimi ve kapı
  açıkken; Copy/Select All bugünkü gibi).
- **`crates/bt-shell/src/menu.rs`** — Edit ▸ Cut (⌘X).
- **`CLAUDE.md`** — `keyDown:` arbitrajına dock seçiminin kolu, Edit menüsü
  (Cut), shell entegrasyonu maddesine widget + bağlama + yetenek (kullanıcı rc
  dosyasına yine yazılmıyor), dock'un kabuğa tek yönlü olmayan ilk teli.

## Kabul

- Sınama (`bt-core`): kapının dört koşulunun her biri tek başına kapatır
  (`vicmd`, bayat ayna, yetenek yok, safha `Running`); açıkken ⌫ tam olarak
  `\e[8133~d;S;E;L\a` yazar; yazma `d` + metin, yapıştırma `d` + `paste`.
- Sınama (betik, pty): widget `emacs` ve `viins`'te aralığı siler ve caret'i
  taşır, `L` tutmazsa no-op; `bindkey -A mymap main` yapan kullanıcıda da
  bağlı; `line-init` sonrası `8133;w` basılır. (Emsal: `context.md` → Ölçüm.)
- `make kur` yeşil (betik kopyası `cmp`), `make hepsi` yeşil, `make duman`
  jetonları değişmez.
- Gözle (ızgara, dock, bant): dock'ta tıkla-caret, sürükle-sil, çift tıkla-yaz,
  ⌘X/⌘V, `vicmd`'de seçim kopyalanır ama tuşlar ZLE'nin.

## Checklist

- [x] Widget, bağlama, yetenek; tel başlığı
- [x] `8133;w` çözücüsü
- [x] Düzenleme kapısı ve üç yardımcı
- [x] `view.rs` tuş tablosu, `insertText:`, tıkla-caret, `cut:` + `validateMenuItem:`
- [x] Edit ▸ Cut
- [x] Test: kapı koşulları, komut baytları; betik pty sınaması
- [x] `CLAUDE.md`
- [x] Doğrulama geçti (`make hepsi`, `make kur`, `make duman`)

## Uygulama Notları

- Widget komuttan sonra aynayı **açıkça** basıyor (`__bateri_dock_redraw`):
  `line-pre-redraw` yalnız görüntü değişince koşuyor ve `L`'si tutmayan ya da
  caret'i yerinde bırakan komut cevapsız kalıp kapıyı bir sonraki tuşa kadar
  kapatırdı. Aynı gerekçeyle `Session::dock_click` caret zaten oradaysa
  hiçbir şey göndermiyor.
- Bağlama + `w` ayrı bir `line-init` kancasında (`__bateri_dock_arm`), aynadan
  önce kayıtlı; `zle -N` bir kez `__bateri_hooks`'ta.
- Yetenek `ShellLog::dock_editable` (aynanın yanında); `e`'ye ek olarak `A`
  da siliyor (kesilen satırda `line-finish` koşmazsa yetenek taşınmasın).
- API: `dock_replace_selection` yerine `Session::type_text` (`insertText:`'in
  yolu) ve `paste()`'in kendisi önce `dock_delete_selection` çağırıyor — Finder
  damlası da seçimin yerine geçiyor. Ek: `can_edit_dock`, `can_cut`,
  `dock_cut`, `dock_click`, `dock_key(DockKey)` (tuş tablosu `bt-core`'da,
  `bt-shell` yalnız `keys::dock_key` ile `NSEvent`'i çeviriyor).
- ⇧←/⇧→ de aynı dört koşullu kapıya bağlı (kabuğa bir şey göndermese de):
  seçim caret'ten başlıyor ve caret'in yeri ancak taze aynada doğru;
  `vicmd`'de tuş vi'nin. Seçimsiz ⇧←/⇧→ kapı açıkken hep tüketiliyor.
- Kapsam eki: seçim yokken Shift+tık artık **caret'ten** başlıyor (phase-4'te
  tıklanan noktadan boş başlıyordu) — metin alanı beklentisi, tek satır.
- Bırakmanın dock kolu `Release::Dock` (gesture); tıkla-caret'in ölçütü
  defterde değil `bt-core`'da: boş kalan `Simple` seçim (`DockSelection::click`).
- `make kur` `APP=` ile scratchpad'e koşturuldu (yeşil, `cmp` dahil);
  `target/release/bateri.app`'e dokunulmadı.
- Gözle (geçici paket, açık tema): tıkla-caret, çift tık + yazma, sürükle + ⌫,
  ⇧← ×3 + ⌘X + ⌘V, çok satırlı olmayan `Control` satırında Cut gri. `vicmd`
  sahnesi otomasyonla kurulamadı (enjekte Escape pencereye varmadı, TR
  düzeninde ⌃[ `^A` üretti); kapısı birim ve pty sınamalarında.
- Set kapısı: `/code-review` tek bulgu (düşük) — bekleyen ölü tuş bileşimi
  varken dock tuşu ⌫'yi yığından çalıyordu; kol artık `marked_text` boşken
  soruluyor. `/audit` temiz (bağımlılık merceği ilgisiz).
