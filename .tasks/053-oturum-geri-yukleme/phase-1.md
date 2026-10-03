# Phase 1 — Geçmişin anlık görüntüsü, oynatması ve ayar modeli (`bt-core`)

## Özet

`bt-core` kapanışta birincil ızgarayı VT baytlarına çevirir, yeni oturum o
baytları kabuk doğmadan `Term`'e oynatır; ilk girdi çalıştırılmadan
verilebilir ve `restore_windows` ayar modelinde yer alır.

_Requirements: R1.1, R1.2, R1.3, R1.4, R1.5, R4.1_

## Değişiklikler

- **`crates/bt-core/src/` — yeni modül (ör. `snapshot.rs`)** — saf kodlayıcı:
  satır satır hücreleri yürür, yalnız **değişen** SGR özniteliğini yazar
  (ön/arka plan: adlı/256/truecolor ayrımı korunur, kalın, sönük, eğik, ters,
  gizli, alt çizgi biçimi + `58` rengi, üstü çizili), hücrenin sıfır
  genişlikli karakterlerini taban karakterin arkasından, geniş karakterin
  spacer hücresini atlayarak; satır `WRAPLINE` taşıyorsa satır sonu yazmaz,
  taşımıyorsa sondaki varsayılan boşlukları kırpıp `\r\n`. OSC 8 hiç
  yazılmaz. Sınır: alacritty tipi modülün `pub` API'sine çıkmaz (`CLAUDE.md`
  → Bağımlılık mimari karardır).
- **`crates/bt-core/src/session.rs`** —
  - `Session::final_history(&self) -> Vec<u8>` (ad sözleşme değil, doc'u
    sözleşme: **yalnız kapanışta**): `Term` kilidi altında; alternatif
    ekrandaysa bir kez `Term::swap_alt` ile birincile geçer ve geri
    **çevirmez** (ikinci çevirme alt ekranı sıfırlar; Set B'nin canlı yolu bu
    yöntemi kullanamaz — doc'ta adıyla). Kesim noktası R1.2: kabuk `Input`
    safhasındaysa son bloğun çıpa satırı (`ShellLog` + hücrelerin OSC 8
    çıpası, düşürülmeden **önce** okunur; bastırmanın ızgara aralığıyla aynı
    kaynak — `suppressed_input` / `anchor_row_at_or_above`), `"blocks"`
    kademesinde de aynı çıpa (prompt kullanıcının, yeni kabuk onu yeniden
    basar); çıpası olmayan kabukta (`integration = "off"`) imlecin satırı
    dahil edilmez, öncesi aynen; kabuk
    `Input`'ta değilse (komut koşuyor) imlecin satırına kadar her şey; sondaki boş satırlar atılır, akış satır sonuyla biter.
    Tavan kullanıcının `scrollback`'i (ızgara zaten o kadar).
  - `SessionOptions::replay: Option<Vec<u8>>` — `spawn`'da `Term::new` ile
    `EventLoop::new` arasında, okuyucu döngünün ayrıştırıcı tipiyle ve
    `handler::ClusterHandler` üstünden uygulanır (`reader.rs`'in `advance`
    yolu emsal), `TappedPty`/`Scanner`'a uğramaz. Oynatmanın ızgaraya
    bıraktığı hasar ilk karede çizilir; `screen_clears` ve doldurma
    bandının ömrü bugünkü gibi temiz başlar — bant oynatılan satırları geçmiş
    olarak gösterir (istenen).
  - `SessionOptions::initial_input` yanında "çalıştır" biti (ör.
    `initial_input_run: bool` ya da küçük bir tip): `false`'ta teslimde `\r`
    eklenmez. Bugünkü iki teslim kolu (ilk kimlikli `A`, doğumda) aynı biti
    okur; mevcut çağıranlar `true` verir ve davranış değişmez.
- **`crates/bt-core/src/settings.rs`** — `RestoreWindows { All, Layout, Off }`
  (`NAMES`, `name()`, `ConfirmClose` emsali), `[terminal] restore_windows`,
  varsayılan `All`, `for_unusable_file` → `Layout`; `SettingsEdit` kolu ve
  `Settings::changes`. Kabul edilmeyen değer kendi anahtarını değiştirmez
  (`parse_keeping`).
- **`CLAUDE.md`** — "`Term::inactive_grid` özel / birincil geçmiş orada
  erişilemez" cümlesi kapanış istisnasıyla düzelir (tek cümle + işaretçi).

## Kabul

- Round-trip sınamaları: renkli/biçimli satır, alt çizgi rengi, geniş
  karakter, emoji kümesi (`👍🏽`, `🇹🇷`), sarılan uzun satırın dar ve geniş
  genişliğe oynatılması, alternatif ekrandayken birincilin okunması, `Input`
  safhasında son prompt satırının kesilmesi, sondaki boş satırlar, OSC 8'li
  hücrenin bağlantısız dönmesi.
- `replay`'li oturumda `shell_state()` / ayna / uzak durum boş; `replay`'siz
  oturum bit bit bugünkü gibi.
- İlk girdi `run = false`'ta `\r`'siz teslim, `true`'da bugünkü sınamalar yeşil.
- Ayar: üç değerin ayrıştırılması, varsayılan, kullanılamayan dosya kolu,
  bilinmeyen anahtarı koruyan yazma round-trip'i.

## Checklist

- [ ] Kodlayıcı modülü ve sınamaları
- [ ] `Session::final_history` (kesim, alt ekran, tavan)
- [ ] `SessionOptions::replay` ve oynatma yolu
- [ ] İlk girdinin "çalıştır" biti
- [ ] `restore_windows` ayar modeli
- [ ] `CLAUDE.md` cümlesi
- [ ] Test: round-trip, kesim, replay'li oturumun temiz defteri, ilk girdi, ayar
- [ ] Doğrulama geçti (`make check` + `make linux` + `make test-race` — okuyucu/`Term` paylaşılan durumu)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
