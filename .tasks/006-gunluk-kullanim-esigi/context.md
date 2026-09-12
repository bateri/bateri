# Günlük kullanım eşiği — Bağlam

## Mevcut Durum

`bateri` bugün metni doğru çizen bir pencere, ama günlük kullanıma kapalı.
Dört eksik, dördü de kodda okunuyor:

**Pano yok.** `crates/` altında `NSPasteboard`, `pasteboard`, `clipboard`,
`PBWrite`/`PBRead` hiç geçmiyor. Üstelik `view.rs:61-63` Command'lı her tuşu
**yutuyor**: "menü gelene kadar doğru davranış hiçbir şey yapmamak" diye
yazılmış bir bekçi var. Yani Cmd-V bugün shell'e "v" yazmıyor — hiçbir şey
yapmıyor. Cmd-C de öyle. Kopyala/yapıştır sıfırdan gelecek, mevcut bir yolu
açmak değil.

**Seçim yok.** `Selection`, `selectedRange`, `mouseDown:`, `mouseDragged:`
hiç yok. Seçim modelinin yaşayacağı yer de belli değil — `bt-core`'da mı
(grid'i gören), `bt-gpu`'da mı (pikseli gören), `bt-shell`'de mi (olayı gören)?
Bu, setin ilk mimari kararı.

**Kaydırma yok.** `scrollWheel:` işleyicisi yok; `scroll` geçen dosyalar
yalnızca scrollback deposuna değiniyor (`bt-core/src/session.rs`,
`bt-shell/src/app.rs`). Grid'de geriye gitmenin bir yolu yok — ne tekerlek,
ne Shift+PgUp, ne kaydırma çubuğu.

**Bundle yok.** `make kur` "henüz yok" deyip kırmızı düşüyor (`Makefile:76`):
`.app paketi bundle setiyle gelir`. Bundle'sız süreç öne çıkma hakkı
taşımıyor, Dock ikonu almıyor, varsayılan terminal olamıyor. Sonuçları
005'te ölçüldü: kapı `kare` sayısını görünmez pencerede ölçüyor.

## Motivasyon

`docs/YOL-HARITASI.md` bu seti **günlük kullanım eşiği** sayıyor: bateri'yi
kendi terminalim olarak açabildiğim gün. O tarihten sonra hatalar sınamadan
değil **kullanımdan** gelmeye başlar — ve kullanımın bulduğu hatalar başka
türlü bulunamaz.

Sıralamanın gerekçesi de orada: 007 (ayar/tema/font), 008 (emoji), 009
(sekme) **eşikten sonra** çünkü neye ihtiyaç olduğu kullanırken daha iyi
görülür. Referans davranış `docs/ARASTIRMA.md`'dedir; özellikle Metalterm'in
OSC 52 (`clipboard.osc52`) ve bracketed paste desteği (`ARASTIRMA.md:42-43`),
ve kabuk entegrasyon notları.

## Kanıt

Dört olgu, dördü de `grep` çıktısı — tahmin değil:

- Pano: `NSPasteboard|pasteboard|clipboard` → `crates/` altında **sıfır** eşleşme.
- Fare: `scrollWheel|mouseDown|mouseDragged|selectedRange` → **sıfır** eşleşme.
- Bundle: `make kur` → `henüz yok: .app paketi bundle setiyle gelir`, exit 1.
- Cmd tuşları: `view.rs:61-63` yutuyor; yorum "menü gelene kadar" diyor, yani
  bu bekçi menüyle birlikte kalkmak üzere yazılmış — ama menü bu setin
  kapsamında değil.

Devredilen iki borç da bu sete bağlı:

- **002'nin Apache-2.0 attribution'ı** (`alacritty_terminal` Apache-2.0):
  "Lisans metni ve attribution paneli **bundle**" — `.tasks/002-vt-motoru/teslim.md:54-56`.
  Bundle olmadan kapanamaz; yeri hazır.
- **`IDLE_FRAME_LIMIT` yeniden ölçümü**: bugünkü `8`, görünmez pencerede
  ölçüldü. Bundle görünür pencere getirince sınır yeniden ölçülmeli
  (`docs/YOL-HARITASI.md:31`, `bt-shell`'de sabitin doc'unda).

## Mevcut Mimari

```
NSEvent (AppKit, ana thread)
  └─ BateriView::key_down (view.rs:47)
       ├─ Command basılı → YUT (view.rs:61-63)   ← pano buraya takılacak
       ├─ Ctrl / düz metin → encode_key (keys.rs, AppKit'siz, saf)
       └─ session.write → PTY (bt-core)

Fare olayları: HIÇBİR ŞEY — scrollWheel:/mouseDown: işleyicisi yok.

Kapanış: Session::shutdown (bt-core, sınırlı bekleme — 005 phase-2b)
Bundle: YOK — `cargo run`, öne çıkma hakkı yok
```

Katman notu (proje.md → tuzaklar): seçim modeli ve OSC 52 ayrıştırma
`bt-core`'a aittir; `bt-gpu` "ne çizeceğini" alır, "ne anlama geldiğini"
bilmez. Panoya yazan/okuyan taraf `bt-shell`'dir (AppKit). Bu üç katmanın
nerede buluşacağı discussion'ın işi.
