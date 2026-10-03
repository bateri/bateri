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

- [x] Kodlayıcı modülü ve sınamaları
- [x] `Session::final_history` (kesim, alt ekran, tavan)
- [x] `SessionOptions::replay` ve oynatma yolu
- [x] İlk girdinin "çalıştır" biti
- [x] `restore_windows` ayar modeli
- [x] `CLAUDE.md` cümlesi
- [x] Test: round-trip, kesim, replay'li oturumun temiz defteri, ilk girdi, ayar
- [x] Doğrulama geçti (`make check` + `make linux` + `make test-race` — okuyucu/`Term` paylaşılan durumu)
- [x] Riskli phase: `/code-review` koştu; tek bulgu `## Waive`'de

## Uygulama Notları

- "Çalıştır" biti alan değil tip: `initial_input: Option<InitialInput>`
  (`InitialInput::run` / `::ready`); `None` veren çağıranlar değişmedi,
  `pane.rs` `Launch`'ın satırını `InitialInput::run`'la sarıyor.
- `RestoreWindows` `Changes`'e girmedi: `ConfirmClose` emsali — kapanışta ve
  açılışta güncel ayardan okunuyor, canlı uygulanacak bir yolu yok.
- Şablona `restore_windows` satırı girdi; `docs/AYARLAR.md`'nin şablon kopyası
  (`documented_template_is_the_template` bekçisi) bu phase'de güncellendi,
  belgenin anahtar tablosu ve Time Machine notu phase-4'te.
- Kesim `Input`'ta çıpanın **bitişik** koşusunun tepesi, imleçten yukarı:
  canlı prompt'un hemen üstüne bitişik bir Ctrl-L kopyası aynı koşuya girip
  kesiliyor (bilinen sınır, `anchor_top`'un doc'unda). `/code-review`: gerçek
  Ctrl-L'de (`2J`) eski kopya tam üstte kalıyor, yani bu normal durum;
  koşmamış bir giriş satırı olduğu için kaybı zararsız.

## Waive

- **`restore_windows`'un kabul edilmeyen değeri `"all"`a düşüyor**
  (`/code-review`, orta): `restore_windows = "Off"` gibi yanlış yazılmış bir
  değer açılışta varsayılana (`all`) düşer ve geçmiş diske yazılır; `osc52` ve
  `[remote] integration` bu kolda kapalıya düşüyor. Phase metni ve R4.1
  açıkça `parse_keeping` kuralını (kendi anahtarını değiştirmez) ve yalnız
  **kullanılamayan dosyada** `"layout"`'u istiyor; değiştirmek bir ürün
  kararı (`osc52` istisnasının üçüncü üyesi olmak) — kullanıcıya sorulacak;
  düzeltme phase-4'ün ayar satırıyla birlikte yapılabilir.
