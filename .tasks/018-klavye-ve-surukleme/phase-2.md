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

- [x] Girdi kaydı tanımlandı (`keys::KeyPress`) ve `encode_key`'in imzası
      genişledi
- [x] Option kolu: `\eb` / `\ef` / `\e\x7f` (küçük harf)
- [x] `reaches_terminal` tek istisna tanıyor: Cmd+Delete → `\x15`
- [x] `command_keys_never_reach_the_terminal` tek istisnayla yeniden yazıldı
- [x] Dokuz mevcut sınama yeni imzayla yeşil (+ üç yeni kol sınaması)
- [~] Test: kazanç tablosunun tamamı elle geçti (Option+7 ve Option+b dahil)
      — **kullanıcı koşacak**: gerçek pencere ve tuş vuruşu gerekiyor, ajan
      kabuğunda klavye sentezi yok
- [~] Test: `Cmd+T` kabuğa harf yazmıyor — **kullanıcı koşacak**, aynı
      gerekçe. Hermetik yarısı çivili: `command_keys_never_reach_the_terminal`
      Cmd+`"t"`'yi altı değiştirici kombinasyonunda da reddediyor
- [x] `keys.rs`/`view.rs` doc'ları ve yol haritası güncellendi (+ `CLAUDE.md`,
      aşağıda)
- [x] Doğrulama geçti (`make hepsi` → exit 0); `make duman` kullanıcıda
      (gerçek pencere ister)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

- **Kayıt `chars`'ı da taşıyor** (`KeyPress { chars, ctrl, option, command }`),
  yalnız bayrakları değil: `plan.md`'nin Akış'ı `encode_key(girdi kaydı)`
  diyor ve ⌘⌫ ile ⌥⌫ aynı `characters`'tan yalnız bayrakla ayrıldığı için
  kollar ikisini **birlikte** soruyor. **Shift kayda girmedi** — hiçbir kol
  onu sormuyor ve `page_scroll` onu zaten ayrı alıyor; kullanılmayan bir
  bayrak kaydın sözleşmesini gevşetirdi.
- **⌫'in karakteri tek yerde** (`keys::BACKSPACE`, `PAGE_UP` emsali): izin
  listesi `view::reaches_terminal`'da, baytı `encode_key`'de ve iki yerde
  ayrı yazılan bir literal birinde kayardı.
- **İzin listesi yanındaki değiştiricileri sormuyor** — ölçüt yalnız
  karakter. Planda yazılı değildi; ters kararın somut bedeli **CapsLock**:
  bayrağı tesadüfen açık olan kullanıcıda ⌘⌫ sessizce yutulurdu. Aynı
  gerekçe Option'ın gezinme sınıfında da geçerli (Ctrl'lü Option+ok da kelime
  gezer) ve `page_scroll`'un "Shift dışındaki değiştiriciler sorulmuyor"
  kuralının kopyası. **Yan etkisi adıyla:** Ctrl+Option+ok bugüne kadar düz
  ok gönderiyordu, artık `\eb`/`\ef` gönderiyor — R1.7'nin sıfır regresyon
  listesinde değil ve zsh'in o tuşta bağlaması zaten yoktu.
- **Cmd kolu Option'ın önünde:** ⌘⌥⌫ satırı siler, kelimeyi değil. İzin
  listesi adı konmuş bir istisna, Option'ın sınıfı bir kural; ters sıra
  istisnayı kuralın altına sokardı.
- **`chars` artık Cmd kolundan önce okunuyor.** Kol sırası (a) Cmd → (b)
  Shift+PgUp/PgDn → (c) Control → (d) yığın **değişmedi**; değişen yalnız
  `characters`'ın nerede çözüldüğü, çünkü izin listesinin ölçütü artık tuşun
  kimliği.
- **R4.2 `!command` guard'ında uygulanıyor.** Yığın kolu `!ctrl && !command`:
  izin listesinden geçen ⌘⌫ de `interpretKeyEvents:`e girmiyor. Girseydi
  yığın onu `deleteToBeginningOfLine:`e çevirir, `doCommandBySelector:`
  sessizce yutar ve `\x15` kolu hiç koşmazdı.
- **Option kolunun yığına bağımlılığı yazıldı.** Option'lı ok/⌫ `encode_key`'e
  ancak `doCommandBySelector:` (`moveWordLeft:`, `deleteWordBackward:`)
  bayrak kurmadığı için varıyor — faz 1'in sözleşmesi. O metoda bir gün gövde
  yazan kişi bu kolu sessizce öldürür; gerekçe kolun kendi yorumunda.
- **`CLAUDE.md` bu fazın `## Değişiklikler`'inde yoktu ama düzeldi:** giriş
  özeti "Cmd'li olay yutulur" diyordu ve kodla çelişir hâle geldi — depo
  kuralı ("buradaki bir cümle kodla çelişirse ikisinden biri aynı commit'te
  düzelir") phase dosyasının listesini geçiyor. Aynı paragrafa Option'ın iki
  sınıfı da yazıldı.
- **Yol haritası "tek satır"a inmedi, *018'in devraldığı kadarı* çıktı.**
  Kalanlar kasıtlı: Home/End'in **şekli** (yeni `pub enum`, ölçülmüş baytlar)
  `plan.md` → Kapsam Dışı'nın "şekli `docs/YOL-HARITASI.md`'nin borç satırında
  bağlandı" cümlesiyle oraya emanet, Ctrl+Shift+Tab ile Ctrl+numpad Enter de
  bu sette kapanmıyor. Çıkan: ölü tuşlar, Option'ın Meta dizileri, Cmd'nin
  izin listesi ve "set henüz açılmadı" defteri.

### Set kapısının önerdiği waive (2026-09-20) — **karar orkestratörün**

**Yığını atlayan iki yeni yazıcı, bekleyen bir bileşimin üstünden PTY'ye bayt
akıtıyor.** phase-1'in `/code-review`'unda kabul edilen 3. waive'in (Control
kolu bekleyen bileşimi yıkmıyor) **aynı sınıfı**, ama **başka tuşlar**, yani
kabul edilmiş kalemin kapsamında değil:

- **⌘⌫ (bu faz).** `keyDown:`'ın yığın kolu `!ctrl && !command` ile korunuyor
  (R4.2'nin gereği), yani izin listesinden geçen ⌘⌫ `interpretKeyEvents:`e hiç
  girmiyor ve `marked_text` el değmeden kalıyor. **Fazdan önce zararsızdı** —
  Cmd'li olay yutuluyordu, PTY'ye hiçbir şey gitmiyordu; şimdi `\x15` gidiyor
  ve bileşim hâlâ bekliyor. Belirti: `Option+ü` → ⌘⌫ → `a` = `ã`.
- **Finder damlası (phase-3).** `performDragOperation:` bir tuş olayı değil,
  yığına hiç uğramıyor ve `Session::paste` baytları doğrudan akıtıyor. Aynı
  belirti: bekleyen ölü tuştan sonra damla, sonra bir harf.

**Düzeltme önerilmiyor, ölçüm öneriliyor.** Gerekçe kabul edilmiş waive'lerin
aynısı: karşı hâl **ölçülmemiş** — bileşimi bu kollarda `unmarkText`'le
yıkmak, kullanıcının bekleyen aksanını sessizce düşürür ve preedit
**çizilmediği** için ekranda hiçbir şey değişmez; bugünkü hâlde en azından
aksan sonraki harfe biniyor, yani kayıp görünür. Dokunduğu şey **hissedilir
davranış** (tuşun ne yazdığı, damlanın ne ürettiği), o yüzden kapı onu
düzeltmedi.

Kalem phase-1'in ölçüm tablosuna **iki satır** olarak ekleniyor ve
kullanıcının elle turuna giriyor: `Option+ü` sonra ⌘⌫, ve `Option+ü` sonra
Finder damlası. Kapatma kararı o ölçümden sonra, bu sette değil.
