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

**Bekçi `bateri`'de değil `bt-shell`'de, ve `run()`'da değil `kapat()`'ta.**
Plan `bateri`'nin `main`'ine bir bekçi thread'i koyuyordu ve asıl kaygısı
`bateri`'ye açık bağımlılık eklememekti. `bt-shell::run` zaten `run_seconds`'ı
biliyor, `runDeadline:` zamanlayıcısı da orada: ikisini ayırmak duman modunu
üçüncü bir crate'e daha yaymak olurdu. `pub fn bekci` da gerekmedi, private
kaldı. Kurulum yeri `/code-review` bulgusuyla `run()`'dan `kapat()`'a taşındı:
bütçe süreç başından ölçülseydi açılış (Metal device, metallib, ilk pencere)
kapanış bütçesinden düşer ve soğuk bir makinede sağlıklı bir koşu `_exit(70)`
ile kırmızı düşerdi.

**`BateriView` oturumu geç alıyor.** Plan `BateriView::new(mtm, rect, session)`
yazıyordu ama sıra buna izin vermiyor: grid ölçüsü `contentView`'ın
bounds'undan türüyor, yani view `Session::spawn`'dan önce doğmak zorunda.
`ViewIvars.session` bu yüzden `OnceCell` — `ShellWake.waker` ile aynı örüntü ve
aynı gerekçe. Boşluk `applicationDidFinishLaunching`'in içinde, run loop
dönmeden kapanıyor.

**`Session::shutdown(&self)` kararı zaten verilmişti.** Plan "`Mutex<Session>`
ya da iç `Mutex<Option<JoinHandle>>` — uygulamada seç" diyordu; phase-1
ikincisini seçmiş ve `Drop` de onu çağırıyor, yani bu phase'de yapılacak bir
şey kalmadı.

**Kapanış sırası `DisplayLink::stop()`'u doğurdu.** Devredilen risk (phase-3
`/code-review`) şuydu: `MainThreadBound<Retained<CAMetalDisplayLink>>`'in
`Drop`'u ana thread dışında ana kuyruğa **senkron** iş atıp bekler, ve o gövdeyi
Metal'in tamamlanma bloğu da tutuyor. Çözüm `DisplayLink`'i kapanış boyunca
**yaşatmak**: `kapat()` onu düşürmüyor, yalnız durduruyor. Sıra zorunlu — önce
ritim keser, sonra bloklayan `shutdown()`.

**`Kapi` tipi ve tek yönlü mandal.** `/code-review` `stop()`'un "geri dönüşü
yok" iddiasının koda dayanmadığını gösterdi: pencere delegate'i kapanışta
sökülmüyor, yani `stop()`'tan sonra düşen bir `windowDidChangeOcclusionState:`
görünürlük bayrağını geri açardı. Görünürlük kapısı `Ardisik` gibi ayrı ve
sınanabilir bir tipe (`Kapi`) çıktı; mandal **okuma** tarafında sorgulanıyor,
yoksa `ayarla`/`durdur` arasında bir TOCTOU kalırdı. `durdurulan_kapi_...`
sınaması mutasyonla doğrulandı (kontrolü kaldırınca düşüyor).

**Duman raporu her çıkış yolundan basılıyor.** `/code-review`'un en ağır
bulgusu: `child_exit` → `terminate:` yolu `runDeadline:`'ı atlıyordu, yani
`BT_RUN_SECONDS` betiğin uykusundan uzun olursa (`sleep 10` vs. `=11`) koşu
hiçbir jeton basmadan `exit 0` verir ve `make duman` **hiçbir şey ölçmemiş bir
koşuyu yeşil gösterirdi**. Rapor `rapor_ve_cik()`'e taşındı; `runDeadline:` ve
`applicationWillTerminate:` (duman yolunda) ikisi de onu çağırıyor. Doğrulandı:
`BT_RUN_SECONDS=11` → `kare=1 hucre=8 pipeline=ok`, düzeltmeden önce sessiz 0.

**Numpad Enter `Ctrl-C` gönderiyordu.** macOS'ta `NSEnterCharacter` = U+0003,
yani Ctrl-C'nin baytıyla aynı. Ctrl basılı değilken bu bir satır sonudur; kolu
olmadan düz metin dalından `0x03` olarak geçiyordu ve sayısal tuş takımının
Enter'ı her komutu çalıştırmak yerine keserdi. `/code-review` bulgusu, sınamayla
bağlandı (`numpad_enter_satir_sonu_verir_kesme_degil`).

**Command'lı tuşlar yutuluyor.** Ana menü yok (00X) ve menüsüz bir uygulamada
`performKeyEquivalent:` hiçbir şey yakalamıyor: Cmd-V shell'e `v`, Cmd-W `w`
yazıyordu. `keyDown:` artık Command basılıysa erken dönüyor. Bunun sonucu:
**Cmd-Q çalışmıyor** — uygulama kırmızı düğmeyle ya da shell'den `exit` ile
kapanır. Menü kapsam dışı.

**Bilinen sınır — shell'in son çıktısı.** alacritty sırayı `ChildExit` →
`Wakeup` diye kuruyor, yani `child_exit`'e geldiğimizde son bayt henüz
çizilmemiş olabilir ve `terminate:` araya bir vsync girmeden koşuyor. Garanti
etmek ya sihirli bir gecikme ya da display link'e "hasar tükendi, şimdi çık"
semantiği eklemek olurdu; ikincisi renderer'a terminal bilgisi sokar.
`bateri -e cmd` yolu geldiğinde `drain_on_exit` ile birlikte tasarlanacak.

**Bilinen sınır — etkileşimli kapanış asılabilir.** `Session::shutdown()`
`SIGHUP`'tan sonra çocuğu bekliyor; sinyali yutan bir çocuk (`trap '' HUP`)
ana thread'i süresiz bekletir. Duman koşusunda bekçi keser, etkileşimli
kullanımda **kesen yok**. Kalıcı çözüm `bt-core`'da sınırlı bekleme
(`SIGHUP` → süre → `SIGKILL`) ve bu bir tasarım kararı: bugünkü `shutdown`
"çocuk gerçekten öldü" garantisi veriyor, sınırlı bekleme onu gevşetir.
`CLAUDE.md`'ye borç olarak yazıldı.

**Bağımlılık kaydı.** İki kenar eklendi ve **yeni crate yok**: `dispatch2`
(`bt-shell`'e — `child_exit` okuyucu thread'de gelir, `terminate:` ana thread'in
işidir) ve `libc` (workspace + `bt-shell` — bekçinin `write(2, …)` + `_exit(70)`
çifti; `eprintln!` olamazdı, Rust'ın stderr kilidini tam da kesmeye çalıştığı
asılı ana thread tutuyor olabilir). İkisi de grafta zaten vardı; `Cargo.lock`'ta
yalnız `bt-shell`'in bağımlılık listesine iki satır girdi, sürüm oynamadı.

**`keys.rs` sadeleşmesi.** İlk taslaktaki `\r`/`\t`/`\x1b`/`\x7f` kolları
catch-all'ın ürettiği baytın aynısını üretiyordu (`/simplify` bunu tüm karakter
uzayında doğruladı); silindiler, sözleşmeyi `donus_ve_silme_tek_bayt` çiviliyor.

**Checklist'in `ls --color` testi yanlıştı.** Plan "renkli arka planlı hücreler
belirir" derken `ls`'in arka plan boyadığını varsayıyordu; boyamıyor —
`ls -G` ön plan basıyor (`ESC[1m ESC[35m … ESC[39;49m`) ve bu renderer yalnız
varsayılan olmayan **arka planı** çiziyor. Glyph 003'te olduğu için `ls`
çıktısı tanım gereği sıfır hücre üretir, yani madde geçseydi de yanlış şeyi
kanıtlardı. Göz kontrolü `\033[4Xm` basan bir betikle tekrarlandı ve geçti.

---

## Yayın Etkisi

- Belgeler: `CLAUDE.md` iskelet paragrafı (klavye + kapanış), katman tablosunun
  `bt-shell` satırı (`dispatch2`, `libc`), yeni bir borç maddesi (etkileşimli
  kapanışın asılma sınırı). Feature `NSEvent`.
- **Bağımlılık:** yeni crate yok; `dispatch2` ve `libc` grafta var olan
  kenarlar olarak eklendi (gerekçe yukarıda, manifest yorumlarında ve
  `CLAUDE.md` katman tablosunda).
- `make duman` sözleşmesi **değişmedi** — jetonlar aynı; değişen, jetonların
  artık her çıkış yolundan basılması.
- Ölçüm bekleyen iddia: yok.

---

## Kalite kapısı

### `/simplify`

Dört mercek paralel koştu. Uygulananlar: `keys.rs`'in dört fallthrough kolu
silindi; görünürlük kapısı `Kapi` tipine çıktı (`Ardisik` ile aynı gerekçe:
sınanabilirlik); `Ivars.view` `OnceCell`'i kalktı, `baglat` view'ı parametre
alıyor; kapanışın "tek kapı" dokümanı gerçeğe uyduruldu (kapı `kapat()`,
`applicationWillTerminate:` onun çağıranı); `FONKSIYON` yorumundaki yanlış
sabit adı düzeltildi.

Reddedilenler ve gerekçeleri: **`Session::write(Cow)`** — tuş başına bir
allocation kurtarırdı ama `bt-core`'un imzasını çağıranın tipine göre
şekillendirir ve kazanç insan yazma hızında ölçülmedi; **Ctrl tablosu ve
`NSString::to_str(pool)`** — aynı sınıftan, okunabilirlik ölçülmemiş bir
allocation'dan değerli; **`shutdown`'ın bloklamasını kaldırmak** — davranış
değişikliği, yukarıda bilinen sınır olarak kayıtlı; **bekçiyi `bateri`'ye
taşımak** — deadline'la aynı yerde durması tutarlı; **`geometriyi_esitle`'yi
`contentRectForFrameRect`'ten türetmek** — ikinci bir geometri kaynağı doğurur;
**`MainThreadBound` yerine asenkron `Drop` sarmalayıcı** — `unsafe` pointer
taşıma ekler ve bugün ulaşılamaz bir yolu kapatır (aşağıya not).

### `/code-review`

On iki bulgu, dördü gerçek hata. Giderildi: duman koşusunun sahte yeşili
(rapor her çıkış yolundan), numpad Enter'ın `Ctrl-C`'si, Command'lı tuşların
PTY'ye yazılması, bekçi bütçesinin süreç başından ölçülmesi, `Kapi`'nin
TOCTOU'su, bekçinin `eprintln!` ile stderr kilidine girmesi, `stop()`'un
`invalidate` idempotentliğini varsayması, çok karakterli `characters`'ın ctrl
dalında kırpılması, `libc` yorumunun "`Cargo.lock` oynamaz" iddiası, `FONKSIYON`
aralığının "PUA'nın tamamı" iddiası, `child_exit` yorumunun son kareyi
gerekçelendirmesi (bilinen sınır olarak yeniden yazıldı).

Waive edilen: **etkileşimli kapanışın asılabilmesi** — kök neden `bt-core`'da
ve düzeltmesi `shutdown`'ın kapanış semantiğini değiştiren bir tasarım kararı;
yukarıda bilinen sınır, `CLAUDE.md`'de borç.

### `/audit`

İlgisiz mercekler: 4 (ayar/tema — `settings.rs` yok), 5 (shell entegrasyonu —
`assets/shell/` el değmedi), 9 (hücre/shader düzeni — `.metal` ve `Cell`
değişmedi).

Koşanlar: 1 katman yönü **temiz** (`bt-core` grafında platform kütüphanesi yok,
kaynakta kaçak yok, `bt-gpu → bt-shell` kenarı yok); 2 bağımlılık — iki kenar,
yukarıda kayıtlı; 3 panik yolu **temiz** (`bt-core`'a eklenen satırlar yalnız
yorum); 6 ölçüm sahipliği **temiz**; 7 thread ve blokaj **temiz** (kilit sırası
term→size korunuyor, `reader` kilidi `join` boyunca tutulmuyor, `child_exit`
ana kuyruğa asenkron sıçrıyor, bekçi kilit almıyor); 8 boşta sıfır kare
**temiz** + bir dikiş: ana kuyruğa çoktan atılmış "aç" bloğu kapıyı yeniden
okumuyordu, tek satırla kapatıldı; 10 belge borcu — beş bayat cümle düzeltildi
(`run()` dokümanı kapanış için yanlış yeri gösteriyordu, `ViewIvars` gerekçesi
"pencere key olamaz" diyordu ama `makeKeyAndOrderFront` daha önce koşuyor,
`CLAUDE.md`'nin `libc` satırı `write`'ı gizliyordu, `FONKSIYON` yorumu kendi
kollarıyla çelişiyordu, `kapat`'ın "idempotent" cümlesi bekçiyi kapsıyormuş
gibi okunuyordu).

**Sete devredilen yapısal not (003 ya da ayrı set):** `WakerInner.link`'teki
`MainThreadBound`'un `Drop`'u ana thread dışında senkron bloklar; bugünkü
panzehir "`DisplayLink` düşürülmez" kuralı ve o kural crate sınırını aşan bir
el sıkışma. Sekme/bölme geldiğinde kapatılan bir sekmenin link'i **düşmek
zorunda** kalacak, yani kural o iş geldiğinde kaçınılmaz olarak ihlal edilir.
Kalıcı çözüm `bt-gpu`'nun içinde: ya `dirty`'ye yapılan indirgemenin aynısı
(`link` paylaşılan gövdeye hiç girmesin) ya da `Drop`'u ana kuyruğa `exec_sync`
değil `exec_async` iş atan yerel bir sarmalayıcı.

---

## Checklist

- [x] **Kapanışta uçuştaki kare** (002 phase-3 `/code-review` devri): tamamlanma bloğunu Metal `Block_copy` ile tutuyor ve **kendi thread'inde** serbest bırakıyor. `DisplayLink` uçuşta kare varken düşerse bloğun elindeki son `Waker` de orada düşer; `MainThreadBound::drop` ana kuyruğa **senkron** iş atar (`exec_sync`) ve ana thread o sırada `Session::shutdown()`'ın `join`'inde bekliyorsa ikisi birbirini kilitler. Çözüldü: `kapat()` link'i **düşürmüyor**, `DisplayLink::stop()` ile durduruyor ve sıra "önce link, sonra oturum"; son referans hep ana thread'de kalıyor. Yapısal kalıntı (sekme geldiğinde link düşmek zorunda) yukarıda sete devredildi
- [x] `keys.rs` + sınamalar; `BateriView` `keyDown:`; `makeFirstResponder`
- [x] `applicationWillTerminate:` → `shutdown()`; `child_exit` → terminate; deadline → `shutdown` + exit; bekçi
- [x] Test: `cargo run -q -p bateri` → pencerede imleç bloğu var, körlemesine yazılan `sh /tmp/r` (üç `\033[4Xm` bloğu basan betik) renkli hücreleri getirdi — **kullanıcı koştu, geçti**. Maddenin `ls --color` hâli **yanlış testti**: `ls` ön plan rengi basıyor (`ESC[35m`), arka plan değil, ve bu renderer yalnız varsayılan olmayan **arka planı** çiziyor — glyph 003'te olduğu için `ls` çıktısı tanım gereği sıfır hücre üretir. Test doğru komutla tekrarlandı
- [x] Test: `exit` yazınca pencere kapanır ve süreç 0 ile biter; kırmızı düğme aynı; `ps` ile yetim shell yok — **kullanıcı koştu, geçti**. Klavyenin uçtan uca çalıştığının kanıtı da bu: dört harf + Enter `keyDown:` → `kod_cevir` → `Session::write` → PTY → `ChildExit` → `terminate:` zincirinin tamamını geçti. Otomatik yarısı ayrıca `BT_RUN_SECONDS=11` ile doğrulanmıştı (`kare=1 hucre=8 pipeline=ok`, exit 0, `ps` temiz)
- [x] Test: `make duman` → `kare=1 hucre=8 pipeline=ok` (çıkış 0); `BT_RUN_SECONDS=1` → 1,5 s'de bitti, bekçi (3 s) devreye girmedi
- [x] Test: SIGHUP'ı yutan komut (`trap '' HUP; sleep 100`) ile bekçi `_exit(70)` — `BT_RUN_SECONDS=2`, kapanış bütçesi 6 s, çıkış 70 ve **jeton basılmadı**; `smoke_shell` geçici olarak değiştirilip geri alındı
- [x] Belgeler aynı commit'te
- [x] Doğrulama geçti (`make hepsi`, `make shader`, `make test-yaris`, `make duman`)
- [x] `/simplify` çalıştırıldı, bulgular uygulandı
- [x] `/code-review` çalıştırıldı, bulgular giderildi
- [x] `/audit` çalıştırıldı, bulgular giderildi (mercek 7: `shutdown` ana thread'de bloklar mı, bekçi; mercek 10: `kod_cevir` Türkçe ad yerel)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: `b458d7f`
