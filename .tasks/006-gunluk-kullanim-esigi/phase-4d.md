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

## Yayın Etkisi

---

## Checklist

- [ ] Her bulgu önce kırmızı sınamayla doğrulandı (kırmızı düşmeyen `[~]` + gerekçe)
- [ ] Boş olmayan kullanıcı girdisi seçimi temizliyor; aynı kilit; görünür seçim kalkınca kare
- [ ] Yapıştırmanın seçimi temizleyip temizlemediği alacritty kaynağından doğrulandı ve uygulandı
- [ ] Yanıtlar, tekerlek raporu/okları ve Cmd-C seçimi temizlemiyor (doc)
- [ ] Seçim ters videoyu çeviriyor (XOR); `DIM` kuralı korunuyor; imleç durumu incelendi
- [ ] Shift+Tab → `\e[Z`, fn+Backspace → `\e[3~`
- [ ] Test: seçim + yazma → seçim yok, `selection_text()` `None`, kare istendi
- [ ] Test: seçim + tekerlek raporu/yanıt → seçim duruyor
- [ ] Test: seçili ters videolu hücre seçilmemiş ters videolu hücreden farklı renkte (normal renkler)
- [ ] Test: `encode_key` Shift+Tab ve `U+F728` dizileri
- [ ] `[elle]` göz kontrolü: bir kelime seç, bir harf yaz → vurgu kalkıyor; `printf '\e[7mters yazı\e[0m\n'` çıktısını seç → vurgu görünüyor; zsh'ta `ls ` + Tab Tab ile açılan menüde Shift+Tab geri gidiyor; satır ortasında fn+Backspace imlecin sağındaki harfi siliyor
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**; kilit yolu değişirse `make test-yaris`)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
