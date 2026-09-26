# ssh'ta uzak oturum hissi — Bağlam

## Mevcut Durum

bateri ssh'ı hiç tanımıyor. Kullanıcı `ssh prod-web-1` yazınca terminal için
olan şey "yerel kabukta bir komut koşuyor"dan ibaret:

- **Safha `Running`'e geçiyor ve orada kalıyor.** `C` işareti
  `__bateri_preexec`'te basılıyor (`assets/shell/zsh/bateri.zsh`), `D` ssh
  bitince `precmd`'de. Aradaki her şey uzak makinenin baytları; bizim
  sarmalayıcımız orada yok, yani ne blok, ne işaret, ne dock aynası var
  (`Session::shell_state`'in doc'u bunu "SSH ile başka bir makineye geçti"
  diye zaten sayıyor).
- **Dock'un bağlam satırı yalan söylüyor.** `DockContext` (`bt-core/src/
  shell.rs`) prompt başına gelen yerel OSC 7 dizinini ve `8133;b` dalını
  tutuyor ve komut koşarken de ekranda kalıyor (tasarım gereği — `CLAUDE.md`
  → dock'un bağlam satırı). ssh sürdükçe bu, **yerel** dizini ve dalı
  gösteriyor; kullanıcı uzakta olduğu hâlde alt satır "`~/proj | main`"
  diyor.
- **Uzak OSC 7 yoksayılıyor.** `parse_cwd` yetkisi `""` ya da `localhost`
  olmayan her URI'yi reddediyor (`LOCAL_AUTHORITIES`, doc'unda gerekçe:
  `gethostname` bağımlılığı kurmamak). Uzak kabuk `file://prod-web-1/var/www`
  bassa bile olay doğmuyor.
- **Başlık yalnız OSC 0/2'den ya da yerel dizinden.** `shell::title_of`'un
  öncelik sırası 026 Karar 7'de; ssh'ın varlığı başlığa hiç girmiyor. Uzak
  kabuk başlık basmıyorsa sekme yerel klasörün adını taşıyor.
- **Ön plandaki işin adı zaten okunuyor** — ama yalnız kapanış anında:
  `bt-shell/src/jobs.rs` (028) kabuğun `e_tpgid`'inden ön plan grubunu,
  grubun yapraklarından adları çıkarıyor. Argümanlar okunmuyor.
- **Dock'un giriş satırı ssh boyunca boş duruyor.** Safha `Running`, caret
  ızgarada (`shell::caret_home`), dock'ta yalnız prompt işareti ve boş bir
  satır kalıyor. Bandın boyu `Cursor::input_rows`'tan (`≥ 1`) ve
  `bt_gpu::band_px(input_rows)`'tan geliyor; sıfır giriş satırı bugün
  temsil edilmiyor (`band_target`'ın `saturating_sub`'ı, `dock::render_with`
  ile `Session::dock`'taki `.max(1)`'ler).

## Motivasyon

**Kullanıcı isteği (2026-09-26):** "bateri ile ssh'a bağlanıldığında
kullanıcı uzak makinede olduğunu hissetsin." Görünüm ve kapsam kullanıcıyla
sohbette seçildi (taslaktaki "E" yönü): uzak tarafa hiçbir şey kurmadan ssh'ı
algılamak, bağlam satırında `⇄ host  /uzak/yol` göstermek, başlıkta ve
sekmede `⇄` taşımak, dock'un üst saç çizgisini renklendirmek ve ssh boyunca
dock'u tek satırlık bir durum çubuğuna indirmek.

Bugünkü davranışın en keskin kısmı yanlış bilgi: bağlam satırı ssh sürerken
yerel yol ve dalı gösteriyor. Bu depo "sessizce yanlış"ı kusur sayıyor
(`dock::render_context`'in dal kuralı aynı gerekçeden).

Referans ürün Metalterm'de ssh için bir gösterge envanterde geçmiyor
(`docs/ARASTIRMA.md`); tek ilgili kayıt `TERM` adının ssh'ta ncurses'ı kırması
(#23) ve bu set `TERM`'e dokunmuyor. Tam uzak entegrasyon (kitty'nin
`kitten ssh`'ı, Ghostty'nin ssh-integration'ı: betik ve terminfo taşıma,
uzakta blok ve dock) bu setin kapsamı dışında ve yol haritasına satır olarak
yazıldı.

### Kanıt

- **Algılamanın zamanlaması bir yarış.** `133;C` `preexec`'te basılıyor ve
  zsh komutu bu kanca döndükten **sonra** çatallıyor. `C`'nin haberini alan
  bir yoklama kabuğu hâlâ ön planda, ya da çatallanmış ama henüz `exec`
  etmemiş (adı hâlâ `zsh`) bir çocuğu görebilir. Tasarım bunu hesaba katmak
  zorunda (`discussion.md` → Karar 2).
- **`⇄` (U+21C4) Menlo'da var ve hücrenin içinde.** CoreText sorgusu
  (2026-09-26, bu makine, Menlo-Regular 13 pt × 0.8 × 2): ilerlemesi `A`'nınkiyle
  aynı, mürekkebi ilerlemenin içinde. **SF Mono ölçülmedi** — bu makinede kurulu
  değil (sorgu Helvetica'ya düştü) ve cascade `⇄`'ü Hiragino'dan hücrenin
  1.66 katı ilerlemeyle veriyor, yani SF Mono'da glyph yoksa mürekkep kapısı onu
  kutuya çevirir. Karar ve yedeği `discussion.md` → Karar 7.
- **Sıfır giriş satırlı bant aritmetikte bir tam satır değil.** `dock_height`
  tek satırlık dock'u boşluksuz sayıyor, iki satırlığı boşluklu
  (`frame.rs`); yani `band_px(0) − dock_px(DOCK_ROWS)` bir hücre boyu **artı**
  satır arası boşluk. Bandın fazlası kesirli olmak zorunda.
