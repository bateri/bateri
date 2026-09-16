# Shell entegrasyonu ve komut durumu — Bağlam

## Mevcut Durum

Oturum kullanıcının kabuğunu **login kabuk** olarak açıyor: `SessionOptions`
`command: None` ile geliyor ve alacritty `$SHELL`'e (yoksa passwd kaydına)
düşüyor. Kabuğun ortamına yalnız üç şey giriyor ve üçü de `bt-shell`'in
politikası: `TERM=xterm-256color`, `COLORTERM=truecolor` (ikisi ezilemez) ve
`child`'ın çözdüğü `LANG` ile başlangıç dizini. `tty::setup_env()` **hiç**
çağrılmıyor; kendi sürecimizin ortamına dokunulmuyor.

Terminal bugün akıştan yalnız **karakter** görüyor. Prompt'un nerede
başladığı, hangi metni kullanıcının yazdığı, komutun ne zaman koştuğu ve
hangi kodla bittiği bilgisi ekranda yok — çünkü bu bilgi bayt akışında
**yok**. Komut blokları, çıkış kodu rozeti, komutlar arası atlama ve Input
Dock'un tamamı bu körlüğün arkasında duruyor.

OSC yolu şöyle işliyor: `Adapter` `EventListener`'ı uyguluyor ve alacritty'nin
ürettiği olayları karşılıyor (`ColorRequest`, `ClipboardStore`,
`TextAreaSizeRequest`, `ChildExit`, `Wakeup`). Bilinmeyen diziler sessizce
düşüyor.

`assets/` altında bugün yalnız `bundle/` var; `assets/shell/` **yok**.
`make denetim` onu şimdiden gözlüyor: dizin varsa ve içindeki bir betik
kullanıcının rc dosyasına yazıyorsa denetim kırmızı düşüyor.

## Motivasyon

Yol haritasının ucundaki üç iş — komut blokları, Input Dock ve yazma/silme
animasyonları — tek bir ön koşula bağlı: **kabuğun ne yaptığını terminale
söylemesi.** Bu set o kanalı açıyor.

Sıra kullanıcı kararıyla öne alındı: 014'e (Input Dock) hızlı varmak için
materyal yüzey, emoji/geniş glyph ve sekme/bölme ertelendi. Referans
davranışın envanteri `docs/ARASTIRMA.md` → "Shell entegrasyonu" ve "Görünüm"
altında; burada tekrarlanmıyor.

Setin ikinci işi bir **sözleşme kurmak**: entegrasyon her kabukta aynı şeyi
veremez. zsh satır editörünün canlı durumunu dışarı verebiliyor (ZLE
kancaları), bash'in readline'ında dengi yok, fish'inki başka. Bunu `bool`
olarak modellemek, sonradan gelen her kabuk için UI'a özel dallar serpmek
demek. Seviye modeli bu yüzden bu sette, ilk kod satırından önce kuruluyor.

## Kanıt

**OSC 133 bize hiç ulaşmıyor — ve bu alacritty'nin değil `vte`'nin kararı.**
`vte-0.15.0/src/ansi.rs`'in `osc_dispatch`'i tanıdığı numaraları `Handler`
metotlarına çeviriyor, tanımadığını `unhandled()` ile düşürüyor. `Handler`
trait'inde "bilinmeyen OSC" kancası **yok**, yani `Term`'ü saran bir tip bile
bu diziyi göremez. 133 dosyada hiç geçmiyor (`grep` boş).

**Bayt akışı da bize uğramıyor.** `alacritty_terminal::event_loop::EventLoop`
PTY'yi kendi thread'inde okuyor ve doğrudan ayrıştırıcıya veriyor
(`event_loop.rs:122` `self.pty.reader().read(...)` → `:154`
`state.parser.advance(&mut **terminal, &buf[..n])`). Bizim kodumuz araya
girmiyor.

**Ama bir kapı var ve dar değil.** `EventLoop<T: tty::EventedPty, U>` PTY
tipinde **jenerik**; gereken sözleşme küçük: `EventedReadWrite`'ın üç poller
metodu + `reader()`/`writer()` (ilişkili tipler `io::Read`/`io::Write`),
`EventedPty`'nin `next_child_event()`'i, `event::OnResize` ve `Send +
'static`. Yani gerçek `Pty`'yi saran, `reader()`'ı kendi okuyucusuyla
değiştiren bir tip `EventLoop`'a olduğu gibi verilebilir.

`Session::spawn` `Pty`'yi yalnız iki satırda kullanıyor — `tty::new` (886) ve
`EventLoop::new`'a taşıma (895) — yani sarmalamanın başka bir çağrı yerini
bozma riski bugün yok. (Kayıtlı kapanış çaresi `pty.file().try_clone()` henüz
kod değil; sarmalayıcı o günü düşünüp içeriye erişim bırakmalı.)

## Mevcut Mimari

```
kabuk (çocuk)
   │  bayt akışı
   ▼
tty::Pty ──► EventLoop (okuyucu thread)
                  │ parser.advance(Term)
                  ▼
              Term ──► Adapter (EventListener)
                  │        └─ ColorRequest / ClipboardStore / ChildExit …
                  │           bilinmeyen dizi: sessizce düşer
                  ▼
            Session::frame()  ──►  Cell akışı + Cursor  ──► bt-gpu
```

Entegrasyonun açacağı yol aynı akışın **iki ucuna** dokunuyor: çocuğun
ortamına bir sarmalayıcı (`bt-shell` → `child`), okuyucu thread'inde
işaretlerin görülmesi (`bt-core`). Aradaki her şey değişmeden kalıyor.
