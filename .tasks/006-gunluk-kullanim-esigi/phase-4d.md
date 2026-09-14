# Phase 4d — Seçim ve klavye rötuşu: girdide seçim, ters videoda seçim, Shift+Tab, ileri silme

## Özet

Yazınca seçim kalkar, ters videolu metnin üstündeki seçim görünür, Shift+Tab
`\e[Z` ve fn+Backspace `\e[3~` gönderir.

_Requirements: R1.4, R1.5, R7_

---

## Neden bu phase var

phase-4c'nin `/code-review`'u 006'nın yayımlanmamış bütün commit'lerine baktı
ve önceki phase'lerin kodunda **doğrulanmamış** bulgular bıraktı
(`phase-4c.md` → Uygulama Notları → "/code-review (10 bulgu)"). Kullanıcı
2026-09-15'te üçünü phase-5 ölçümünden önce düzeltmeyi seçti
(`discussion.md` → "Kapsam eki (2026-09-15)"):

- **(2)** girdi seçimi temizlemiyor: içerik kayınca vurgu kayar, Cmd-C eski
  metni kopyalar;
- **(3)** Shift+Tab ham `0x19` gidiyor, fn+Backspace yutuluyor;
- **(1)** seçim `INVERSE` ile OR'lanıyor: ters videolu hücrede seçim
  görünmüyor.

**Önce doğrula.** Her bulgu için önce onu gösteren sınamayı yaz ve bugünkü
kodda **kırmızı** düştüğünü gör. Kırmızı düşmeyen (bulgu yanlış çıkan) madde
düşer: kutusu `[~]` + gerekçe, kod değişmez.

Sıra: phase-4c'den sonra, phase-5 ölçümünden **önce** (R5.1).

---

## 1. Girdi seçimi temizler — `bt-core`

`crates/bt-core/src/session.rs` → kullanıcı girdisinin tek gönderim noktası
(phase-3'te `write_owned`, phase-3b'de `send_input` oldu; boş olmayan girdide
pencereyi dibe döndüren yer).

- Boş olmayan girdi seçimi **temizler**, dibe dönüşle **aynı kilit altında**
  (ikinci bir `Term` kilidi ekleme — phase-3'ün kararı).
- Görünür seçim kalktıysa kare istenir (`visible_range` kapısı; phase-1'in
  kare kuralı: kare yalnız çizilen aralık değişince).
- Referans: alacritty `ActionContext::on_terminal_input_start` seçimi
  temizleyip dibe dönüyor. **Yapıştırmanın** (`paste`) da bu yoldan geçip
  geçmediğini alacritty kaynağından doğrula (`alacritty/src/event.rs`) ve
  aynısını yap; bizde `paste` `send_input`'tan geçiyorsa bedava gelir.
- Uygulamaya giden **yanıtlar** (`Adapter::reply`), tekerlek raporu ve
  tekerlek okları (phase-3b) seçimi **temizlemez** — kullanıcı girdisi
  değiller. Hangi yolun temizleyip hangisinin temizlemediğini doc'a yaz.
- Cmd-C seçimi temizlemez (girdi değil).

## 2. Ters videoda seçim görünür — `bt-core`

`crates/bt-core/src/session.rs` → `frame()`, bugün
`let inverse = flags.contains(Flags::INVERSE) || selected;`.

- Seçim ters videoyu **çevirir** (XOR): seçili ters videolu hücre normal
  renklerle çizilir. alacritty'nin varsayılan seçim renkleri de hücrenin
  çözülmüş iki rengini takas ediyor.
- `DIM` + ters video kuralı (yorumdaki "sönüklük `cell.fg`'den doğan renge
  gider") XOR'dan sonra da doğru kalmalı; sınamaya bağla.
- Blok imleç: imleç tersine çevirmesi aynı `inverse` değişkeninden mi
  geçiyor bak. Aynı kuralı paylaşıyorsa ve ters videolu hücrede imleç de
  görünmüyorsa aynı düzeltme onu da kapsar (sınamasıyla); paylaşmıyorsa
  dokunma, nota yaz. `contains_cell`'in blok imleç hücresini atlaması
  (phase-1) değişmez.
- Boş (varsayılan arka planlı) ters videolu hücrenin seçilince çizilmeyen
  hücreye dönmesi doğru davranıştır; atlama koşulunun bunu bozmadığını gör.

## 3. Shift+Tab ve ileri silme — tek kaynak

`crates/bt-shell/src/keys.rs` (ve phase-3b'nin `bt-core` `input.rs`'i, ok
kodlaması orada toplandıysa aynı yere).

- `xterm-256color` terminfo'su (bu makinede `infocmp` ile okundu):
  `kcbt=\E[Z`, `kdch1=\E[3~`.
- Shift+Tab: AppKit `characters`'ı `U+0019` (`NSBackTabCharacter`) veriyor —
  bugün düz metin dalından ham `0x19` gidiyor. `\e[Z` gönder.
- fn+Backspace: `NSDeleteFunctionKey` (`U+F728`) — bugün fonksiyon tuşu
  kolunda yutuluyor. `\e[3~` gönder.
- Ctrl'lü/Option'lı hâller kapsam dışı. Home/End (`U+F729`/`U+F72B`) bugün
  **bilerek** yutuluyor (sınaması var) — dokunma; kapanışta borç olarak
  yazılacak, nota geç.

---

## Uygulama Notları

- **Önce doğrulama: üç bulgunun üçü de doğrulandı**, hiçbiri düşmedi.
  Sınamalar düzeltmeden önce yazıldı ve HEAD kodunda koşuldu:
  - (1) ters video — `selected_inverse_cell_is_drawn_in_normal_colors`
    kırmızı: seçili 1. sütun seçilmemiş 0. sütunla birebir aynı renkte,
    `left: (Some(kırmızı), yeşil) right: (Some(yeşil), kırmızı)`.
  - (2) girdide seçim — `input_clears_the_selection` kırmızı:
    `yazma seçimi temizlemedi — left: Some("araba") right: None`.
  - (3) tuşlar — `back_tab_and_forward_delete_emit_xterm_sequences`
    kırmızı, iki yarısı ayrı ayrı: Shift+Tab `left: [25] right: [27, 91, 90]`
    (ham `0x19`); `U+F728` `"\u{f728}" (ctrl=false) bayt vermedi: None`
    (sıra geçici olarak çevrilip koşuldu, sonra geri alındı).
  - `wheel_and_replies_keep_the_selection` bir **bekçi**, kırmızı-önce değil:
    tekerlek raporu ve yanıt yarısı HEAD'de de yeşil (hiçbir yol
    temizlemiyordu); HEAD'de sınamanın sonundaki karşıt satır (`write` →
    `None`) düştü.
- **(2) Girdi seçimi temizler — `send_input`.** Temizlik dibe dönüşle aynı
  `Term` kilidinde, ikinci kilit yok (phase-3 kararı). Sıra: önce seçim,
  sonra dibe dönüş — "çizili miydi" sorusu kullanıcının baktığı pencereye
  soruluyor; pencere kayarsa kare zaten isteniyor. Kare **tek** istek:
  temizlik çizili aralığı kaldırdıysa `request_frame`, değilse
  `wake_if_moved` (ikisi art arda çağrılsaydı vuruş başına iki uyandırma
  olurdu; biçim `/simplify`'dan, aşağıda). Temizliğin gövdesi `clear_selection`
  ile ortak yeni yardımcıda (`clear_selection_locked`, `scroll_locked`'ın
  karşılığı): kare kuralı (`visible_range`) tek yerde kaldı.
  - `write`, `paste` (iki dalı da `write_owned` → `send_input`) ve
    `write_arrow` bedava geliyor.
  - **Boş girdi seçime dokunmuyor**, pencereye dokunmadığı gibi (phase-3).
    alacritty'nin `paste("")`'i `on_terminal_input_start`'ı yine de
    çağırıyor; bizde boş yapıştırma hiçbir şey yapmıyor ve bu tutarlılık
    tercih edildi (boş panoda Cmd-V'nin seçimi silmesi kullanıcıya bir şey
    kazandırmıyor).
  - Temizlemeyenler (doc: `write_owned`, `scroll_wheel`, `selection_text`):
    `Adapter::reply` (uygulamanın sorusuna yanıt; `Term` kilidini okuyucu
    tutuyor), `scroll_wheel`'in rapor ve ok gönderimi, Cmd-C
    (`selection_text` okuma), Shift+PgUp'ın kaydırması (`scroll_page`
    `send_input`'tan geçmiyor; alternate screen'de reddedilince tuş
    `write`'a düşüyor ve orada temizliyor — alacritty'de de uygulamaya giden
    tuş temizliyor).
- **alacritty kaynağı (master, 2026-09-15, `curl` ile ham dosyalar okundu):**
  - `alacritty/src/event.rs` `on_terminal_input_start` (satır 1359):
    `on_typing_start` → `clear_selection` → `display_offset != 0` ise
    `Scroll::Bottom`.
  - Aynı dosya `paste` (1369): bracketed dal da ham dal da
    `on_terminal_input_start`'ı çağırıyor → **yapıştırma seçimi temizler**;
    bizde `paste` `send_input`'tan geçtiği için bedava geldi.
  - `clear_selection` (760): seçimi `take` ediyor, `dirty`'yi yalnız boş
    olmayan seçimde dikiyor. Bizim kapı bir adım dar: yalnız **ekranda
    çizili** aralık (`visible_range`), phase-1'in kare kuralı.
  - `input/keyboard.rs` (99): bayt yazan her tuş `on_terminal_input_start`,
    yalnız saf değiştirici tuşlar hariç (`is_modifier_key`). Bizde saf
    değiştiricinin `characters`'ı boş → `encode_key` `None` → hiçbir şey
    yazılmıyor; aynı sonuç yapı gereği.
  - `config/bindings.rs` (455): Shift+Tab → `Action::Esc("\x1b[Z")`;
    `input/mod.rs` (172) `Action::Esc` → `ctx.paste(s, false)`, yani o tuş da
    seçimi temizliyor. Delete `build_sequence`'ta `("3", '~')` (keyboard.rs
    549).
  - `input/mod.rs` `scroll_terminal` (760–827) ve `sgr_mouse_report` (607):
    tekerlek okları ve raporları `write_to_pty`'ye **doğrudan** yazıyor —
    temizlemiyor, dibe dönmüyor. Bizim ayrım bununla aynı.
  - `display/content.rs` (211–264): hücre önce `INVERSE` için takaslanıyor,
    sonra seçiliyse `compute_cell_rgb` varsayılan seçim renkleriyle
    (`config/color.rs` 122: `CellBackground`/`CellForeground`) bir kez daha
    takaslıyor → net XOR. alacritty'nin ek kuralı ("fg == bg ise ters
    renklerle göster") alınmadı, kapsam dışı.
  - Kitaplık (`alacritty_terminal-0.26.0` `term/mod.rs` 1657, 1773, 1786,
    1811): seçimi yalnız silme/kaydırma dizileri kesişince düşürüyor, düz
    yazmada değil — bulgunun kitaplıkta zaten çözülü olmadığının kanıtı.
- **(1) XOR.** `inverse = INVERSE ^ selected`. phase-1'in yorumu (`||`, "ters
  videolu hücre seçilince düzleşmemeli") kullanıcı kararıyla (R1.5) tersine
  döndü; yorum gerekçesiyle yeniden yazıldı.
  - `DIM` kuralı değişmeden doğru: iki dal da aynı `inverse`'e bakıyor.
    Sınamada seçili sönük ters video `(yeşil, sönük kırmızı)`, seçilmemişi
    `(sönük kırmızı, yeşil)`.
  - **İmleç:** blok imlecin tersine çevirmesi `inverse`'ten **geçmiyor**
    (`fore` en sonda koşulsuz `BG_RGB`, blok opak). `contains_cell` imleç
    hücresini yalnız seçimin **ucundaysa** dışlıyor (`alacritty_terminal`
    `selection.rs` 66–78; ilk taslak "zaten dışlıyor" diyordu, `/code-review`
    düzeltti). Seçimin ortasındaki imleç hücresinde `selected` doğru ve XOR
    `inverse`'i çeviriyor; değişen tek şey opak bloğun altında kalan arka
    plan (ters videolu hücrede normal renk, düz hücrede eskisi gibi ön plan
    rengi). Harf ve blok aynı. İçi boş ya da yanıp sönen imleç gelirse bu
    hücre yeniden düşünülmeli. Dokunulmadı.
  - Varsayılan renkli ters video boşluk seçilince `bg: None, ch: None` →
    atlama koşulu onu eliyor; sınamada `sink`'e varmadığı görüldü.
- **(3) Tuşlar `bt-shell` `keys.rs`'te**, `bt-core` `input.rs`'te değil: iki
  dizi de kipten bağımsız (PgUp'ın `\e[5~`'i gibi); `input.rs` yalnız
  DECCKM'e bağlı ok ve tekerlek raporunu tutuyor. Terminfo bu makinede
  yeniden okundu: `infocmp xterm-256color` → `kcbt=\E[Z`, `kdch1=\E[3~`.
  - **Ctrl-Y çakışması:** U+0019 Ctrl-Y'nin de `characters`'ı. Kol
    `('\u{19}', false)`: Control'lü hâl düz metin dalından `0x19` gidiyor
    (numpad Enter/Ctrl-C kolunun aynı biçimi). Sınamada iki yol da
    (`"\u{19}"`+ctrl, `"y"`+ctrl) `0x19`.
  - fn+Backspace kolu (`'\u{f728}'`) fonksiyon tuşu yutma kolundan önce;
    Ctrl'lü/Option'lı hâller kapsam dışı (Ctrl'lü fn+Backspace düz `\e[3~`
    alıyor, oklar gibi).
  - **Borç (kapanışta yol haritasına):** Home/End (`U+F729`/`U+F72B`,
    terminfo `khome=\EOH`, `kend=\EOF`) hâlâ bilerek yutuluyor,
    `keys_without_sequences_are_swallowed` sınaması duruyor.
  - AppKit'in Shift+Tab'da gerçekten U+0019 verdiği burada sınanamadı
    (sentetik `NSEvent` kurulmadı); kanıtı `[elle]` göz kontrolü.
- **Mutasyonlar (hepsi kızardı, dosyalar geri yüklendi, `cmp` ile doğrulandı):**
  - M1 `^` → `||`: `selected_inverse_cell_is_drawn_in_normal_colors`.
  - M2 temizlik yok (`cleared = false`): `input_clears_the_selection`
    (`yazma seçimi temizlemedi`).
  - M3 temizlik var, kare yok: aynı sınama (`kalkan vurgu kare istemedi`).
  - M4 kare her seçimde (`selection.is_some()`): `input_clears_the_selection`
    (`boş seçimin temizliği kare istedi`) + phase-1'in iki sınaması
    (`selection_redraws_only_when_the_drawn_range_changes`,
    `selection_scrolled_into_history_is_not_drawn`).
  - M5 `scroll_wheel` seçimi temizliyor: `wheel_and_replies_keep_the_selection`.
  - M6 `('\u{19}', _)`: keys sınaması (Ctrl-Y `\e[Z` oldu).
- **Sınama notları.**
  - `input_clears_the_selection` `stty -echo` ile: yankı kendi `Wakeup`'ını
    doğururdu ve "kare istendi" iddiası temizlik olmadan da geçerdi.
  - Yanıt sınaması `Adapter::send_event(Event::PtyWrite(..))` ile, PTY turu
    kurulmadan: yanıt yolu `Term`'e yapı gereği dokunmuyor, olay girişinin
    kendisi çağrılıyor.
  - İlk taslak `[(&str, fn(&Session)); 3]` dizisi clippy `type_complexity`'ye
    takıldı; iç içe `fn clears` oldu.
- **`/simplify` (4 mercek, Skill; ajanlar reuse/sadeleştirme `sonnet`,
  verimlilik/seviye `opus`).**
  - Verimlilik: temiz. Seçim yokken ek iş bir `Option::take`; `to_range`
    `Simple` seçimde sabit maliyet; `frame()`'de hücre başına maliyet yok.
  - Uygulandı:
    - `send_input`'ta `moved.is_some_and(|n| n != 0)` `wake_if_moved`'ın
      kuralını kopyalıyordu (reuse + sadeleştirme + seviye, üçü aynı satır):
      `if cleared { request_frame } else { wake_if_moved(moved) }`. Tek istek
      sözü korunuyor. Mutasyon yeniden koşuldu (temizlik kolu kare
      istemiyor → `yazma: kalkan vurgu kare istemedi`).
    - Sınamada seçim uçları iki yerde yazılıydı: `select_word` yardımcısı.
    - Ters video sınamasının yorumu `frame()`'deki gerekçeyi tekrar
      ediyordu: işaretçiye indi.
  - **Atlandı (seviye):** `encode_key` Shift+Tab'ı Ctrl-Y'den yalnız Control
    bayrağıyla ayırıyor; Ctrl+Shift+Tab büyük olasılıkla U+0019 + Control
    gelir ve `0x19` (yank) gider. Numpad Enter kolunda da aynı yapı (Ctrl+
    numpad Enter → `0x03`). Merceğin önerisi `charactersIgnoringModifiers`'ı
    `encode_key`'e geçirmek — imzayı, `view.rs`'i ve önceki phase'in kolunu
    değiştirir; kılavuz Ctrl'lü/Option'lı hâlleri kapsam dışı sayıyor.
    Gerileme değil: Ctrl+Shift+Tab bugün de `0x19` gidiyordu. Gerçek
    klavyede sınanmadı.
- **`/code-review` (Skill, arka planda; aralık `git diff HEAD`) — 9 bulgu,
  doğruluk hatası yok.**
  - Giderildi:
    - (2) `scroll_wheel` doc'unun "less'te tekerlekle kaydırıp Cmd-C" gerekçesi
      girdideki temizliğin gerekçesiyle çelişiyordu (tekerlek oku = klavye
      oku baytları; ekranı baştan çizen uygulamada vurgu bayatlar). Doc artık
      dayanağı alacritty ile aynı davranmak diye yazıyor ve bedeli sayıyor.
    - (3) İmleç notu yanlış öncüle dayanıyordu (yukarıda düzeltildi).
    - (5) "Tek istek" iddiasının sınaması yoktu: `input_clears_the_selection`'a
      geçmişe bakan pencerede seçim + yazma → uyandırma sayısı `+1`. Mutasyon
      (iki çağrı art arda): `left: 5 right: 4`, kızardı.
    - (7) Sondaki "kare yok" iddiası çocuğun yaşamasına bağlıydı (`sleep 5`
      biterse `ChildExit` `Wakeup`'ı): `sleep 60`.
    - (9) `clear_selection` üretimde çağrılmıyor (yalnız sınamalar; phase-1'den
      `pub`). Doc'lar onu canlı bir yol gibi anıyordu: `clear_selection_locked`
      doc'u üretimdeki tek yolun `send_input` olduğunu söylüyor,
      `write_owned` kare kuralını yardımcıya bağlıyor.
  - Not düşüldü, kod değişmedi:
    - (6) Yanıt yarısı yapı gereği kızaramaz (`Adapter`'ın `Term`'i yok).
      Checklist'in "yanıt" kolu bu yüzden bir **belge bekçisi**; gerçek bekçi
      tekerlek yarısı (M5). Sınamada kaldı: maliyeti bir olay çağrısı.
  - Atlandı:
    - (4) Uygulamanın seçili hücreleri yeniden yazması (`watch`, `top`,
      zsh'ın asenkron istemi) seçimi temizlemiyor. Doğru; ama alacritty de
      temizlemiyor (kitaplık yalnız silme/kaydırmada) ve bulgu (2)'nin
      kapsamı kullanıcı girdisi. Hasara duyarlı seçim geçersizleştirme ayrı
      bir iş; yol haritası borcu adayı.
    - (8) Kipten bağımsız fonksiyon tuşu dizilerini tek tabloya toplamak:
      önceki phase'in kollarını (PgUp/PgDn; `PAGE_UP` `page_scroll`'da da
      okunuyor) yeniden yazar. Home/End borcu gelince değerlendirilecek.
  - **WAIVE önerisi (1):** Ctrl+Shift+Tab (`U+0019` + Control, gerçek klavyede
    doğrulanmadı) `0x19` yani readline/zle `yank` gönderir. `/simplify`'ın
    seviye merceğiyle aynı bulgu. Gerekçe: kılavuz Ctrl'lü hâlleri kapsam
    dışı sayıyor; davranış bu phase'ten önce de aynıydı (Shift+Tab'ın kendisi
    de `0x19`'du); kalıcı çare `charactersIgnoringModifiers`'ı `encode_key`'e
    geçirmek ve numpad Enter kolunu da aynı yoldan ayırmak — `view.rs` ile
    numpad Enter'ın mevcut kolunu değiştiren ayrı bir iş. Kabul edilmezse phase-4d'ye
    geri döner.
- **`/audit` (Skill).** Eleme: değişen dosyalar `bt-core` `session.rs`,
  `bt-shell` `keys.rs`, bu dosya.
  - Mekanik, inline: 1 katman temiz (`cargo tree` üçü boş; `bt-atlas`'ta
    yalnız izinli `objc2-core-*`, `objc2` çekirdeği yok; `bt-core`
    kaynağında platform adı yok). 2 bağımlılık temiz (`Cargo.toml`/
    `Cargo.lock` diff'i boş). 3 panik yolu temiz (eklenen üretim satırında
    `unwrap`/`expect`/`panic!` yok; grep'in üç eşleşmesi `#[test]`
    özniteliği). 6 ölçüm sahipliği temiz (sayı yok; nottaki sayılar sınama
    çıktısı ve kaynak satır numarası).
  - İlgisiz: 4 ayar şeması, 5 shell üçlüsü, 9 hücre/shader (`Cell` ve
    `.metal` değişmedi).
  - Yargı, üç ajan (`opus`, paralel):
    - 7 thread/blokaj: temiz. `clear_selection`'ın geçici kilidi `let`'in
      sonunda düşüyor; `send_input`'ta uyandırma kilit bloğundan sonra;
      `size` kilidi alınmıyor; yanıt sınaması `Term` kilidine girmiyor.
    - 8 boşta kare: kod temiz; bir **sınama boşluğu** giderildi —
      "geçmişte kalan seçim + girdi → kare yok" iddiası sınanmıyordu
      (`selection_scrolled_into_history_is_not_drawn` önce boş seçim
      kuruyor). `input_clears_the_selection`'a eklendi; mutasyon (kapı
      pencereye bakmasın: `to_range(..).is_some()`) kızardı: `görünmeyen
      seçimin temizliği kare istedi`.
    - 10 belge/üslup, giderildi: `frame()`'deki phase-1 yorumu "blok imlecin
      durduğu hücrede seçim tersine çevrilmez, çift tersleme" diyordu — hem
      istisna yalnız uçta, hem imleç rengi `inverse`'ten geçmiyor; yeniden
      yazıldı. `keys.rs`'te `single` yorumu yeni iki kolu saymıyordu;
      `FORWARD_DELETE` sabiti tek yerde okunuyordu (okların literal
      düzenine döndü); Home/End "dizileri yok" → "henüz yazılmadı, borç".
      Nottaki "seçimi bırakıyor" (iki anlamlı) → "seçime dokunmuyor". İki
      doc paragrafının kırık satırları toplandı. Kimlikler İngilizce,
      yorumlar ve `assert!` gerekçeleri Türkçe, `#[allow]` yok.
- **Doğrulama (kapı sonrası son hâl).**
  - `make hepsi` → 0 (kapıdan önce de 0; ilk koşu clippy `type_complexity`
    ile düşmüştü, yukarıda).
  - `make test-yaris` → 0 (kapıdan önce ve sonra). Gerekçe: `send_input`'un
    `Term` kilidi bölümü değişti ve `frame()` aynı `term.selection`'ı okuyor.
  - `make duman` → 0, kapıdan önce `kare=1 hucre=8 glif=6 kural=15
    yuva=13/2048 yuk=smoke istek=3 kapanis=clean profil=debug ornek=off
    pipeline=ok`, kapıdan sonra aynı satır `kare=2` ile. `glif=6`, phase-4c'nin
    gürültüsü görülmedi, A/B gerekmedi. Koşu öncesi ve sonrası `pgrep`'te
    `bateri` süreci yoktu.
  - `make shader`, `make kur`, `make terminfo` gerekmedi (`.metal`, bundle,
    terminfo el değmedi).


- **Orkestratör kararları (2026-09-15).**
  - sadakat: makas yok — `git show --stat 3907585` (`bt-core` `session.rs`,
    `bt-shell` `keys.rs`, `phase-4d.md`, `plan.md`) checklist'le örtüşüyor;
    `4817155` yalnız hash damgası.
  - Üç bulgu da HEAD'de kırmızı düşerek **doğrulandı**; hiçbiri düşmedi.
  - Sapmalar (ortak `clear_selection_locked`; `contains_cell` yorumunun
    düzeltilmesi; boş yapıştırmanın seçime dokunmaması; tuş dizilerinin
    `keys.rs`'te kalması) **kabul**: R1.4, R1.5, R7 oynamadı. Boş yapıştırma
    alacritty'den ayrılıyor ama bizde boş girdi pencereye de dokunmuyor —
    tutarlı.
  - WAIVE (1) (Ctrl+Shift+Tab `0x19`) **kabul**, kapanışta Home/End ile
    birlikte klavye borcu olarak yol haritasına.
  - WAIVE (4) (uygulama seçili hücreyi yeniden yazınca seçim düşmüyor)
    **kabul**: alacritty de düşürmüyor; borç değil, not.

## Yayın Etkisi

- **Davranış değişikliği (kullanıcıya görünen):**
  - Seçim varken yazmak, yapıştırmak ya da ok tuşuna basmak seçimi
    kaldırıyor; Cmd-C artık o seçimi kopyalamıyor (pano el değmeden kalır).
    Tekerlek (rapor/ok), Shift+PgUp kaydırması ve Cmd-C seçime dokunmuyor.
  - Ters videolu metnin seçimi görünür: seçili hücre normal renklerinde.
    Ters video olmayan metnin vurgusu değişmedi.
  - Shift+Tab `\e[Z`, fn+Backspace `\e[3~` gönderiyor. Ctrl-Y aynen `0x19`.
    Home/End hâlâ yutuluyor (borç).
- **Kare:** girdi başına en çok **bir** kare isteği (temizlik ve dibe dönüş
  tek istekte). Ekranda çizili seçim yoksa temizlik kare istemiyor; boşta
  sıfır kare kuralına yeni yol eklenmedi. Ölçüm bekleyen iddia yok; phase-5
  ölçümü bu son koda alınacak (R5.1).
- **Kilit:** `send_input`'ta kilit sayısı değişmedi (vuruş başına bir `Term`
  kilidi); kilit altındaki iş bir `Option::take` ve seçim varsa bir
  `to_range`. `make test-yaris` koştu.
- **`TERM`/terminfo değişmedi.** Diziler `xterm-256color`'ın kendi
  `kcbt`/`kdch1`'i.
- **Bağımlılık:** yok. `Cargo.toml`/`Cargo.lock` değişmedi.
- Ayar şeması, tema, shell entegrasyonu (`assets/shell/`), `.metal`, app
  bundle (`Info.plist`, imza): el değmedi. `make shader`, `make kur`
  gerekmedi.
- **Belge:** `session.rs` modül başlığı ve `write`/`write_owned`/
  `send_input`/`paste`/`write_arrow`/`scroll_wheel`/`selection_text`
  doc'ları, `frame()`'in seçim yorumu (XOR ve phase-1'den kalan
  `contains_cell` imleç cümlesi), `keys.rs` doc'ları. `CLAUDE.md`
  seçimi ve tuş kodlamasını saymıyor, değişmedi. `docs/MIMARI.md` yok.
- **Kapanışa devir (yol haritası borcu):** Home/End dizileri. Adaylar
  (orkestratör kararı): Ctrl+Shift+Tab ve Ctrl+numpad Enter'ın
  `charactersIgnoringModifiers` ile ayrılması (WAIVE önerisi (1));
  uygulamanın seçili hücreleri yeniden yazmasında seçimin düşmesi
  (`/code-review` (4)).
- `[elle]` göz kontrolü bekliyor (checklist).

---

## Checklist

- [x] Her bulgu önce kırmızı sınamayla doğrulandı (kırmızı düşmeyen `[~]` + gerekçe)
- [x] Boş olmayan kullanıcı girdisi seçimi temizliyor; aynı kilit; görünür seçim kalkınca kare
- [x] Yapıştırmanın seçimi temizleyip temizlemediği alacritty kaynağından doğrulandı ve uygulandı
- [x] Yanıtlar, tekerlek raporu/okları ve Cmd-C seçimi temizlemiyor (doc)
- [x] Seçim ters videoyu çeviriyor (XOR); `DIM` kuralı korunuyor; imleç durumu incelendi
- [x] Shift+Tab → `\e[Z`, fn+Backspace → `\e[3~`
- [x] Test: seçim + yazma → seçim yok, `selection_text()` `None`, kare istendi
- [x] Test: seçim + tekerlek raporu/yanıt → seçim duruyor (yanıt yarısı yapı gereği kızaramaz — Uygulama Notları → `/code-review` (6))
- [x] Test: seçili ters videolu hücre seçilmemiş ters videolu hücreden farklı renkte (normal renkler)
- [x] Test: `encode_key` Shift+Tab ve `U+F728` dizileri
- [ ] `[elle]` göz kontrolü: bir kelime seç, bir harf yaz → vurgu kalkıyor; `printf '\e[7mters yazı\e[0m\n'` çıktısını seç → vurgu görünüyor; zsh'ta `ls ` + Tab Tab ile açılan menüde Shift+Tab geri gidiyor; satır ortasında fn+Backspace imlecin sağındaki harfi siliyor
- [x] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**; kilit yolu değişirse `make test-yaris`)
- [x] `/simplify` çalıştırıldı, bulgular uygulandı
- [x] `/code-review` çalıştırıldı, bulgular giderildi (WAIVE önerisi: (1) Ctrl+Shift+Tab; atlanan (4), (8) gerekçeli)
- [x] `/audit` çalıştırıldı, bulgular giderildi
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: 3907585
