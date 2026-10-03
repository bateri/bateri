# Odak sorgusu — Bağlam

## Mevcut Durum

- **Pane'in dış adı var, durumu yok.** Her pane `TERM_SESSION_ID` ve
  `BATERI_TAB_URL=bateri://tab/<UUID>` taşıyor (038; pane başına, 039 Karar
  10) ve `application:openURLs:` o pane'i öne getiriyor. URL yalnız odaklıyor,
  bilgi geri vermiyor: güvenlik değişmezi, çünkü şemayı her uygulama açabilir
  (`.tasks/038-terminal-kimligi/discussion.md` → Karar 5–7).
- **Odağı biliyoruz, söyleyecek kanal yok.** "Odaktaki pane" pencerenin first
  responder'ının pane'i (`TerminalWindow::focused_pane`, arama alanı dahil);
  pencerenin key biti `windowDidBecomeKey:`/`windowDidResignKey:`'ten bütün
  pane'lere iniyor (`window.rs`, `TerminalPane::apply_focus`) ve resign
  uygulamanın deaktivasyonunu da kapsıyor.
- **bateri'nin binary'si zaten alt komut taşıyor.** `main.rs` AppKit'ten önce
  `askpass`, `ssh-argv`, `ssh-fell-back`'i deniyor; tanımadığı her argv düz
  GUI açılışına düşüyor. `ssh-argv` çalışan örneği **adıyla** buluyor
  (`--instance`, `BATERI_SSH_INSTANCE` → `ssh_route::instance_dirs`); adı
  bilmeden canlı örnekleri sayan mantık bugün yalnız `Masters::sweep`'in
  içinde, ölü sahipleri bulmak için (`instance_entry` + `owner` + `alive`).
- **Örnek dizini açılışta doğuyor ve sahibi `Masters`.** `app.rs`'in
  `masters()`'ı açılışta süpürme thread'ini başlatıyor, o da
  `Masters::bases()` ile her kökte (`~/Library/Caches/bateri/s`,
  `/tmp/bateri-$UID`) `prepare_instance` yapıyor; süreli koşuda `masters`
  hiç kurulmuyor. Uzun yaşayan bir dinleyici yok: askpass soketi deneme
  başına açılıp kapanıyor. Ölü örneğin dizinini `remove_instance` yalnız
  **kendi bildiği adları** silerek kaldırıyor.
- **Son girdinin tek hunisi var.** `TerminalPane::note_interaction`
  (`stats.rs`) view'ın tuş/basış/tekerlek/fare hareketi çağrılarının ve
  pencerenin key oluşunun indiği yer; bugün yalnız yük göstergesinin
  zamanlayıcısını besliyor.
- **Uzak oturumda kimlik zaten öbür uçta.** 049 `BATERI_TAB_URL`'yi
  `LC_BATERI_TAB_URL` olarak ssh'ın öbür ucuna geçiriyor; evlat onu uzak
  sunucudan okuyor (`TabLink.forwarded`).

## Motivasyon

evlat (`../evlat-app/evlat`, ekranın kenarında AI oturumlarının durum şeridi)
bir Claude Code oturumu bitince ya da soru sorunca halkayı turuncuya boyuyor.
Kullanıcı o an o oturumun pane'ine bakıyorsa sinyal gürültü: zaten
ekrandadır. evlat oturumun uygulamasını ve pane'in UUID'sini biliyor
(`TabLink.swift`), bateri'nin önde olduğunu izinsiz öğrenebiliyor
(`NSWorkspace.frontmostApplication`), sistem genelindeki boşta süreyi de
(`CGEventSourceSecondsSinceLastEventType`). Bilemediği tek şey **bateri'nin
içinde hangi pane'in odakta olduğu ve o pane'e en son ne zaman dokunulduğu**;
bunu dışarıdan izinsiz bilmenin yolu yok (pencere başlığı Ekran Kaydı ya da
Erişilebilirlik izni ister, evlat izin istemiyor).

Kanal seçimi kullanıcıyla konuşuldu (2026-10-03): genel bir durum API'si değil,
**yalnız soranın elindeki pane için** evet/hayır + kaba bir boşta süresi;
push yok, evlat olay anında soruyor.

**evlat tarafının bilmesi gereken** (o deponun işi, burada yalnız sözleşme):
binary'yi sabit yoldan değil çalışan kopyadan alır
(`NSRunningApplication(processIdentifier:).executableURL`, `--pid` ile
birlikte); bugün kurulu sürüm (0.3.0) `focus`'u tanımıyor ve tanımadığı
alt komutta **GUI açıyor**, yani evlat çağırmadan önce çalışan kopyanın
sürümünü 050'yi taşıyan sürümle kapılamalı.

