# Phase 4 — bt-shell giriş ve kapanış

## Özet

`BateriView` klavyeyi PTY'ye akıtır; `ChildExit` uygulamayı bitirir;
`applicationWillTerminate:` shell'i düzgün kapatır; duman deadline `shutdown()`
+ `exit`; bekçi thread asılmayı keser. Terminal ilk kez kullanılabilir olur.

_Requirements: R5, R6, R7_

---

## 1. Feature

Workspace `objc2-app-kit` feature listesine `"NSEvent"` (`keyDown:` onun
arkasında).

## 2. Klavye eşlemesi (saf)

`crates/bt-shell/src/keys.rs`

```rust
/// AppKit'siz sınanır. `chars` = NSEvent.characters, `ctrl` = Control basılı.
pub fn kod_cevir(chars: &str, ctrl: bool) -> Option<Cow<'static, [u8]>> {
    let c = chars.chars().next()?;
    Some(match (c, ctrl) {
        ('\r', _) => b"\r".into(),
        ('\u{7f}', _) => b"\x7f".into(),                 // Backspace
        ('\t', _) => b"\t".into(),
        ('\u{1b}', _) => b"\x1b".into(),
        ('\u{f700}', _) => b"\x1b[A".into(),             // NSUpArrowFunctionKey
        ('\u{f701}', _) => b"\x1b[B".into(),
        ('\u{f702}', _) => b"\x1b[D".into(),
        ('\u{f703}', _) => b"\x1b[C".into(),
        (c, true) if c.is_ascii_alphabetic() => vec![(c.to_ascii_lowercase() as u8) & 0x1f].into(),
        (_, false) => chars.as_bytes().to_vec().into(),
        _ => return None,
    })
}
```

Sınama: Enter, Backspace, oklar, Ctrl-C (`0x03`), düz metin, Türkçe karakter
(UTF-8 çok baytlı). IME, ölü tuşlar, Option-as-Meta, kitty **yok** — kapsam dışı.

## 3. BateriView

`crates/bt-shell/src/view.rs`

```rust
define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriView"]
    #[ivars = ViewIvars]          // session: Arc<Session>
    pub(crate) struct BateriView;
    unsafe impl NSObjectProtocol for BateriView {}
    impl BateriView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool { true }
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let chars = event.characters().map(|s| s.to_string()).unwrap_or_default();
            let ctrl = event.modifierFlags().contains(NSEventModifierFlags::Control);
            if let Some(b) = kod_cevir(&chars, ctrl) { self.ivars().session.write(&b); }
        }
    }
);
```

`app.rs`: `NSView::initWithFrame` → `BateriView::new(mtm, rect, session)`;
`window.makeFirstResponder(Some(&view))`. Layer takma sırası aynı.

## 4. Kapanış

- `applicationWillTerminate:` → `session.shutdown()` (Session `Arc` içinde;
  `shutdown(&mut self)` → `Mutex<Session>` ya da `shutdown(&self)` iç
  `Mutex<Option<JoinHandle>>` — uygulamada seç, `Drop` ile tutarlı).
- `ShellWake::child_exit` → `exec_async(|| NSApp.terminate(None))`.
- `run_deadline`: hüküm → `session.shutdown()` → `process::exit(kod)`.
- `bateri` main: `BT_RUN_SECONDS` varsa bekçi thread
  `sleep(run_seconds * 3); libc::_exit(70)` — `libc` bt-core'dan geçişli;
  `bateri`'ye açık bağımlılık **eklenmez**: `std::process::abort` yerine
  `unsafe { libc::_exit }` gerekiyorsa `bt-shell` `pub fn bekci(secs)` sağlar
  (alacritty zaten `libc` çekiyor; `CLAUDE.md`'ye tek satır).
- Son pencere kapanınca `applicationShouldTerminateAfterLastWindowClosed`
  → `terminate:` → `applicationWillTerminate:` — tek yol.

## 5. Belgeler (R7)

`CLAUDE.md` "İskelet 001 ile kuruldu … VT motoru 002'de gelir" →
"002 ile `bt-core` alacritty kapsüllü çekirdeği taşır; glyph 003'te".
`docs/ARASTIRMA.md` dokunulmaz.

---

## Uygulama Notları

## Yayın Etkisi

- Belgeler (bölüm 5). Feature `NSEvent`. Yeni bağımlılık: yok.
- Ölçüm bekleyen iddia: yok.

---

## Checklist

- [ ] **Kapanışta uçuştaki kare** (002 phase-3 `/code-review` devri): tamamlanma bloğunu Metal `Block_copy` ile tutuyor ve **kendi thread'inde** serbest bırakıyor. `DisplayLink` uçuşta kare varken düşerse bloğun elindeki son `Waker` de orada düşer; `MainThreadBound::drop` ana kuyruğa **senkron** iş atar (`exec_sync`) ve ana thread o sırada `Session::shutdown()`'ın `join`'inde bekliyorsa ikisi birbirini kilitler. `Session` tarafı tümüyle kapatıldı (`Waker` artık `DirtyFlag` tutuyor, `Arc`/`Weak` değil), kalan tek şey `MainThreadBound<Retained<CAMetalDisplayLink>>`'in `Drop`'u — ana thread dışında `exec_sync` ile ana kuyruğa iş atıp **bekler**; kalan yarı kapanış sırasının kendisiyle çözülür (önce link'i durdur/invalidate et, sonra oturumu kapat) — `applicationWillTerminate:` yolunu kurarken bu sıra yazılmalı
- [ ] `keys.rs` + sınamalar; `BateriView` `keyDown:`; `makeFirstResponder`
- [ ] `applicationWillTerminate:` → `shutdown()`; `child_exit` → terminate; deadline → `shutdown` + exit; bekçi
- [ ] Test: `cargo run -q -p bateri` → prompt gelir, `ls --color` yazınca renkli arka planlı hücreler belirir (göz; glyph yok)
- [ ] Test: `exit` yazınca pencere kapanır ve süreç 0 ile biter; kırmızı düğme aynı; `ps` ile yetim shell yok
- [ ] Test: `make duman` → `kare=N hucre=8 pipeline=ok`; `BT_RUN_SECONDS=1` + `sleep 30` komutu ile bekçi devreye girmeden shell kapanıyor (çıkış 0, 3 s'den kısa)
- [ ] Test: SIGHUP'ı yutan komut (`trap '' HUP; sleep 100`) ile bekçi `_exit(70)` (`make` "Error 70")
- [ ] Belgeler aynı commit'te
- [ ] Doğrulama geçti (`make hepsi`; koşullu: `make duman`, `make test-yaris`)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi (mercek 7: `shutdown` ana thread'de bloklar mı, bekçi; mercek 10: `kod_cevir` Türkçe ad yerel)
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
