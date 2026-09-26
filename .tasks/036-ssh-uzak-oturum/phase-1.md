# Phase 1 — Uzak oturumun modeli, başlık, `info` rolü ve bağlam satırı

## Özet

`bt-core` uzak oturumu tutuyor, OSC 7'yi yönlendiriyor, başlığa ve bağlam
satırına `⇄` koyuyor, üst saç çizgisini `info` ile boyuyor; üretimde henüz
kimse `set_remote` çağırmıyor, yani görünen davranış değişmiyor.

_Requirements: R1.1, R1.2, R1.3, R2, R3, R4.1, R4.2, R4.3_

## Değişiklikler

- **`crates/bt-core/src/shell.rs`**
  - `ShellLog`'a uzak durum: komut nesli (`Running`'e **geçişte** artar;
    ikinci `C` geçiş değil — saatin "ilk `C` kazanır" kuralıyla aynı yer),
    uzak host (`Option<String>`) ve uzak dizin yuvası. Host ve yuva `C`, `D`
    ve `A`'da siliniyor (Karar 2, 4).
  - `apply`'ın `Running` geçişi ve uzak durumun silinmesi çağırana
    bildiriliyor: `apply_scan_answering`'in `bool` dönüşü küçük bir sonuç
    tipine genişliyor (başlığın girdisi değişti / komut başladı); ikinci bir
    dönüş yolu açılmıyor. Başlık girdisi: farklı yerel dizin **ya da** uzak
    durumun silinmesi.
  - Tarayıcı: `parse_cwd` yabancı yetkiyi reddetmek yerine "yerel mi"
    bilgisiyle olay doğuruyor (`ScanEvent::Cwd`'nin şekli). Şema, yol ve
    yüzde çözme kuralları aynen; `LOCAL_AUTHORITIES`'in doc'u yeni anlamıyla
    (artık "kabul listesi" değil "yerel mi" sorusu).
  - `apply_scan_answering`'in `Cwd` kolu Karar 4'ün iki kuralı.
  - `title_of` uzak host argümanı alıyor: `⇄ ` + OSC başlığı, yoksa `⇄ ` +
    host (Karar 5). Öncelik doc'u güncellenir.
- **`crates/bt-core/src/session.rs`**
  - `Session::running_command() -> Option<u64>` ve
    `Session::set_remote(command: u64, host: Option<&str>)` — yaprak kilit
    (`shell`), `Term`'e dokunmadan; nesil tutmuyorsa ya da safha `Running`
    değilse no-op; değiştiyse kare ister (`set_theme` örüntüsü) ve
    değiştiğini döndürür ki çağıran başlığı tazelesin.
  - `Session::title` uzak durumu aynı kilit turunda okuyup `title_of`'a
    veriyor.
  - Okuyucu yolunun `apply_scan_answering` çağrısı: "komut başladı" →
    `Wake::command_started`, "başlık girdisi" → `Wake::title_changed`; ikisi
    de kilit **bırakıldıktan sonra** (bugünkü `title_changed` yeri).
  - `Session::dock` bağlam satırına uzak durumu veriyor (aynı yaprak kilit
    okumasında).
  - `TestWake`'e `command_started` sayacı.
- **`crates/bt-core/src/wake.rs`** — `fn command_started(&self)`: `Running`
  kenarı, yüksüz, okuyucu thread'de (kilit tutulurken gelebilir); doc'u üç
  yasağı ve "kenarda" kuralını `title_changed`'in dilinden bağlıyor. Varsayılan
  gövde yok.
- **`crates/bt-shell/src/window.rs`** (`ShellWake`) ve
  **`crates/bt-shell/src/child.rs`** (`SilentWake`) — `command_started`'ın
  gövdesi bu phase'de **boş**; yoklama phase-3'te. Boş gövdenin yorumu bunu
  söyler.
- **`crates/bt-core/src/color.rs`** — `Theme::info` alanı; `BATERI` `0x79b3b3`,
  `BATERI_LIGHT` `0x23787f` (Karar 6; yorumda "kendi temasının ANSI
  camgöbeği"); `info_linear()`. `color::tests`'e bekçi: iki gömülü temada
  `info` zeminde 3:1'i geçiyor (mevcut `contrast` yardımcısı).
- **`crates/bt-core/src/theme.rs`** — roller dizisine `info`; modül ve
  `parse` doc'larında "kalan durum rolleri" artık yalnız uyarı. Belge tema
  bloğunu ayrık tabanla okuyan sınama `info`'yu da kapsıyor.
- **`crates/bt-core/src/dock.rs`**
  - `Dock`'a üst saç çizgisinin rengi için ayrı alan (ör. `edge`): uzakta
    `info_linear`, değilse `separator`. `separator` ikinci çizgi için kalıyor.
  - `render_context` uzak biçimi (R4.1): `⇄` ve host `info`, iki boşluk, yol
    bugünkü iki kademede ve soldan kısalan; dal ve `|` yok. Bütçe önce
    `⇄ host`'a; host kısalmıyor, sığmazsa yalnız `⇄`. Yerel biçim bit bit
    aynı.
  - İşaret karakteri tek bir sabitte (`REMOTE_MARK`); başlık da onu kullanır
    (`bt-core` içinde tek kopya).
- **`crates/bt-gpu/src/frame.rs`**, **`crates/bt-gpu/src/link.rs`** —
  `open_dock` üst çizginin rengini ayrı alıyor; `dock_ground`'ın ilk ayracı
  onu, ikincisi `separator`'ı boyuyor. Mevcut `dock_ground` sınamaları iki
  rengi ayrı doğruluyor.
- **`crates/bt-atlas/`** (sınama) — `⇄` **Menlo'da, adıyla** (makineden
  bağımsız kapı; ölçülen font) **küçük sınıfta** kutuya düşmüyor (Karar 7).
  SF Mono kuruluysa aynı sorgu onun için de koşuyor (kurulu değilse atlanır
  ve sınama bunu söyler); SF Mono'nun sonucu kapı değil, gözle kontrolde. Kutuysa `REMOTE_MARK` `↔` olur ve
  sınama onu doğrular; ikisi de kutuysa phase durur ve raporlanır.
- **`docs/AYARLAR.md`** — Temalar tablosuna `info` ("uzak oturum: bağlam
  satırında host ve dock'un üst çizgisi"); "On rol" sayısı; "kalan iki durum
  rolü … bugünden yazılabilir" maddesi yalnız uyarıya iner; `info`'nun
  eksikte `bateri`'den geldiği maddeye eklenir.

## Kabul

- `ShellLog` sınamaları: `C` → nesil artar, ikinci `C` artırmaz; `set_remote`
  bayat nesilde ve `Running` dışında no-op; `D` ve `A` host'u ve uzak yuvayı
  siler ve "başlık girdisi değişti" döner.
- OSC 7: yerel yetki bugünkü gibi; yabancı yetki uzak yuvaya; uzak etkinken
  boş yetki de uzak yuvaya ve yerel dizin kıpırdamıyor; bozuk URI yine
  yoksayılıyor.
- `title_of`: dört kol (uzak × OSC var/yok) + bugünkü üç kol değişmeden.
- Bağlam satırı: uzak biçim, yolsuz biçim, dar genişlikte yalnız `⇄`, yerel
  biçimin hücreleri bugünküyle aynı.
- `Wake::command_started` gerçek bir oturumda `C` kenarında bir kez geliyor
  (`TestWake`).
- `info` 3:1 bekçisi ve tema bloğu sınaması yeşil; `⇄` atlas sınaması yeşil.

## Checklist

- [ ] `ShellLog`: komut nesli, uzak host, uzak yuva, silme noktaları
- [ ] Tarayıcı: `Cwd` olayı yetkisiyle; Karar 4 yönlendirmesi
- [ ] `title_of` ve `Session::title` uzak kolu
- [ ] `Session::running_command` / `set_remote`; `Wake::command_started` ve üç uygulayıcı
- [ ] `info` rolü: `Theme`, iki değer, `theme.rs`, 3:1 bekçisi, `docs/AYARLAR.md`
- [ ] `Dock`'un üst çizgi rengi; `bt-gpu`'nun `open_dock`/`dock_ground`'ı
- [ ] `render_context` uzak biçimi; `REMOTE_MARK`
- [ ] Test: `⇄` Menlo'da küçük sınıfta kutu değil (ya da `↔` yedeği); SF Mono kuruluysa koşullu sorgu
- [ ] Test: yukarıdaki Kabul maddeleri
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
