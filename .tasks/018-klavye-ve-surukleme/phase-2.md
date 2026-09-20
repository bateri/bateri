# Phase 2 — Değiştirici kodlaması: Option'ın gezinme tuşları ve Cmd'nin tek istisnası

## Özet

Option+ok ile Option+Delete Meta dizisi gönderir, Cmd'nin kapalı izin listesi
tek tuşla açılır; ikisi de aynı girdi kaydının tüketicisi.

_Requirements: R3, R4, R6 (bu fazın dokunduğu doc'lar)_

## Değişiklikler

- **`crates/bt-shell/src/keys.rs`** — `encode_key`'in girdisi genişler:
  bugünkü `(chars, ctrl)` yerine değiştirici bayraklarını taşıyan bir kayıt.
  **Kaydı tanımlayan faz onu tüketen fazdır**; faz 1'de tanımlanıp boş
  bırakılmadı. Yeni kol Option'lı gezinme/silme tuşlarını Meta dizisine
  çevirir: `\eb`, `\ef`, `\e\x7f`. Diziler **küçük harf** — `\eA`
  `accept-and-hold`, `\eB` `backward-word`, yani büyük harfli hâl başka bir
  widget'a gider (ölçüldü). Dokuz sınama yeni imzaya göre **bu fazda bir kez**
  yazılır.
  `shell_quote` bu fazda **yok** (faz 3).
- **`crates/bt-shell/src/view.rs`** — `reaches_terminal` tuşun **kimliğini**
  öğrenir (imzası genişler) ve tek istisna tanır: Cmd+Delete → `\x15`.
  Command'lı başka her tuş yine yutulur — liste **kapalı**, yoksa bir gün
  Cmd-T kabuğa `t` yazar. Cmd'li olayın `interpretKeyEvents:`e hiç girmemesi
  (faz 1'in sözleşmesi) burada **zorunlu** hâle gelir: girseydi ⌘⌫ orada
  `deleteToBeginningOfLine:` olur ve bu liste onu hiç görmezdi.
  `command_keys_never_reach_the_terminal` sınaması tek istisnayla yeniden
  yazılır — "her Command kombinasyonu yutulur" iddiası artık doğru değil.
- **`crates/bt-shell/src/keys.rs` + `view.rs` doc'ları** — kapsam-dışı listesi
  **bölünüyor, eksilmiyor**: Option+Backspace kapsam **içi**; Ctrl+Backspace
  ile Option/Ctrl'lü ileri silme **dışarıda**; "Option-as-Meta" ibaresi
  yeniden yazılır (gezinme tuşları Meta, **harf değil**); "değiştiricili oklar
  (`\e[1;5A`)" hâlâ dışarıda ve Option+ok'un `\eb` vermesiyle çelişmiyor.
- **`docs/YOL-HARITASI.md`** — "Klavye kalanları" maddesi tek satıra iner
  (set açıldı; `duzen.md`'nin kuralı). Home/End satırı **kalır** — bu sette
  kapanmıyor ve yutulmaya devam ediyor.

## Kabul

**Kazanç (elle, gerçek pencere, varsayılan zsh):**

| tuş | beklenen |
|---|---|
| `Option+←` | kelime geri (`\eb` → `backward-word`) |
| `Option+→` | kelime ileri (`\ef` → `forward-word`) |
| `Option+Delete` | kelime siler (`\e\x7f` → `backward-kill-word`) |
| `Cmd+Delete` | satırın tamamı gider (`\x15` → `kill-whole-line`) |
| `Option+7` | `{` — **değişmemeli** (R3.2) |
| `Option+b` | `∫` — **değişmemeli** |
| `Cmd+T`, `Cmd+Shift+A` | hiçbir şey; kabuğa harf **yazılmamalı** |

**Sınama (hermetik):** yeni kolların dizileri `keys.rs`'te; `reaches_terminal`'ın
tek istisnası `view.rs`'te. Dokuz mevcut sınama yeni imzayla yeşil.

**Kapı:** `make hepsi` yeşil. `make duman` **kullanıcı koşar**.

## Yayın Etkisi

- **shader / terminfo / tema / shell entegrasyonu / app bundle** — yok.
- **ayar şeması** — yok. Option kipleri (`[keyboard] left_option`) bilerek
  **kapsam dışı**: bu tuşlar hiçbir düzende harf üretmediği için ayara
  bağlanacak bir çatışma yok (`discussion.md` → Karar).
- **yeni bağımlılık** — yok.
- **ölçüm bekliyor** — yok. `\eb`/`\ef`/`\e\x7f`/`\x15` ve Home/End'in sıfır
  bağlaması `zsh -f -c 'bindkey -e; bindkey -L'` ile ölçüldü (zsh 5.9,
  2026-09-20).
- **Geri alma birimi bu fazdan sonra commit değil `set`:** girdi kaydının
  imzası burada değişiyor ve faz 1'in arbitrajının üstüne oturuyor, yani faz
  1'i tek başına geri almak derlemeyi kırar. `teslim.md`'ye böyle geçer.
- **Sapma kayda geçer:** Cmd+Delete macOS'ta "satır **başına kadar** sil"
  demek; zsh'te `backward-kill-line` varsayılanda bağlı değil (ölçüldü), bağlı
  olan `^U` = `kill-whole-line`. Kullanıcının beklentisi ("satırı silmiyor")
  satırın gitmesi olduğu için sapma bilinçli.
- `keys.rs`/`view.rs` doc'ları ve `docs/YOL-HARITASI.md` güncellenir.

## Checklist

- [ ] Girdi kaydı tanımlandı ve `encode_key`'in imzası genişledi
- [ ] Option kolu: `\eb` / `\ef` / `\e\x7f` (küçük harf)
- [ ] `reaches_terminal` tek istisna tanıyor: Cmd+Delete → `\x15`
- [ ] `command_keys_never_reach_the_terminal` tek istisnayla yeniden yazıldı
- [ ] Dokuz mevcut sınama yeni imzayla yeşil
- [ ] Test: kazanç tablosunun tamamı elle geçti (Option+7 ve Option+b dahil)
- [ ] Test: `Cmd+T` kabuğa harf yazmıyor
- [ ] `keys.rs`/`view.rs` doc'ları ve yol haritası güncellendi
- [ ] Doğrulama geçti (`make hepsi`; `make duman` kullanıcıda)
- [ ] Yayın etkisi yazıldı
