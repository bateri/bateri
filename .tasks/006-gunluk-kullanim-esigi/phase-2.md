# Phase 2 — Pano: Cmd-C/V, bracketed paste

## Özet

Kopyala ve yapıştır çalışır; yapıştırma uygulamaya yapıştırma olduğunu söyler.

_Requirements: R2, R2.1, R2.2, R2.3, R6.1, R6.2_

---

## 1. `keyDown:`'da iki tuşluk dal (Karar 2 (a))

`crates/bt-shell/src/view.rs` — `view.rs:61-63` bekçisi bugün Command'lı her
tuşu yutuyor. Cmd-C ve Cmd-V o bekçiden önce ele alınır; diğer Command tuşları
yutulmaya devam eder. Menü günü (00X) bu dal **silinir** — kalıcı çözüm menü
seçicileridir, bu dal geçici köprüdür.

## 2. `NSPasteboard` köprüsü

`bt-shell`'de: Cmd-C → `core.selection_text()` (phase-1'in tek metin yolu) →
panoya yaz. Cmd-V → panodan oku → `paste()`'e ver. Panoya dokunan yalnız
`bt-shell` (AppKit); `bt-core` bayt görür, pano görmez.

## 3. `paste()` + 2004 sorgusu

`crates/bt-core/src/session.rs` — yapıştırma `session.write` yolundan
(`session.rs:811`) ama ham değil, yeni bir `paste()` ile sarılarak. DECSET
2004 **tutulmaz**; kiplik alacritty `Term`'in içinde, kilit altında sorgulanır.
Set ise `\e[200~…\e[201~` sarılır; değilse ham yazılır.

Güvenlik notu: ham yapıştırma vim/REPL'de satırları çalıştırır. Sarmalayan
`paste()`'tir; `session.write`'a doğrudan yapıştırma baytı verilmez.

---

## Uygulama Notları

## Yayın Etkisi

- Cmd-C/V artık panoya dokunur; diğer Command tuşları yutulmaya devam eder.
- Yeni bağımlılık yok. Ayar şeması yok — okuma yönü (OSC 52) kapsam dışı.
- Ölçüm bekleyen iddia yok.

---

## Checklist

- [ ] `keyDown:`'da Cmd-C/V dalı; diğer Command tuşları yutuluyor
- [ ] `NSPasteboard` köprüsü `bt-shell`'de; `selection_text()` tek kaynak
- [ ] `paste()` + 2004 sorgulu sarma; `session.write`'a ham yapıştırma yok
- [ ] Test: 2004 setken sarma, değilken ham yazma
- [ ] Test: Cmd-C seçili metni panoya yazıyor (NSPasteboard sahteyle ya da başsız atlamalı)
- [ ] `[elle]` göz kontrolü: kopyala-yapıştır turu (terminal içi + dış uygulama)
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
