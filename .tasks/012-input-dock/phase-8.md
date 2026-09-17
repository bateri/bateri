# Phase 8 — Caret tek yerde ve açılış temiz

## Özet

Pencere açıldığı andan itibaren caret **tam bir yerde** olsun: komut
koşmuyorsa dock'ta, koşuyorsa ızgarada. Ve açılışta ızgara tertemiz olsun —
`login(1)`'in banner'ı da gitsin.

_Requirements: R2.5 eki, R3.1 eki_

## Bağlam

Set yürürken gerçek pencerede iki kusur çıktı (kullanıcı, ekran görüntüleriyle):

1. **Caret sıçrıyor.** Dock canlı değilken caret ızgarada, canlı olunca dock'a
   atlıyor. İki pencere var ve ikisi de görünür: pencere açılırken (zsh'in rc
   süresi) ve **her komuttan sonra** — `precmd` içinde `git rev-parse` fork'u
   da olduğu için `Finished` penceresi kısa değil.
2. **Izgarada açılış artığı.** `login(1)`'in `Last login: …` banner'ı ilk
   satırda duruyor.

İkisi ayrı mekanizma, tek şikâyet: "sadece dock aktifleşecek ve ızgara
tertemiz olacak."

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — caret'in sahibi **tek bir yüklemde**:
  `caret_home(shell, status) -> CaretHome`. İki tüketicisi var (`dock::render`
  çizmek için, `Session::frame` gizlemek için) ve tek kaynak olmak zorunda —
  ayrı ayrı yazılsalardı bir karede iki caret ya da hiç caret doğardı.
  - `Running` → `Grid`: komut çalışıyor, satırın sahibi o.
  - `Unavailable` → `Grid`: gösteremediğimiz bir satır var, ızgarada kalmalı
    (R1.2'nin aynı gerekçesi).
  - Kalan her hâl → `Dock`. **Kabuğun hiç konuşmamış olması (`state == None`)
    dahil**: açılış tam da o hâl ve caret'in oraya ait olması gereken an.
- **`crates/bt-core/src/dock.rs`** — dock, caret'in sahibiyken onu **her
  hâlde** çiziyor: ayna `Live` ise hesaplanan sütunda, değilse `TEXT_COL`'da
  (boş satırın başı).
- **`crates/bt-core/src/session.rs`** — ızgara, caret'in sahibi değilken
  imleci çizmiyor. `SessionOptions.dock` ile oturum "bu pencerenin dock'u var"
  bilgisini alıyor; alternatif ekranda dock kalktığı için orada ızgara yine
  sahip (`alt_screen` zaten kilidin altında).
- **`crates/bt-shell/src/child.rs`** — `login(1)` **her zaman `-q`** ile:
  banner yok. alacritty'nin macOS yolunun paritesi (`home` ve `shell` emsali:
  politika bizde, çözüm parite hâlinde), ve çözülemeyen bir kullanıcı/kabuk
  `command: None`'a düşüyor — banner geri gelir, oturum çalışır.

## Kararlar

- **`SessionOptions.dock` ikinci bir kaynak değil, ikinci bir tüketici.**
  Doğum kararı `bt-shell`'de tek bir ifadede (`birth`) veriliyor ve oradan iki
  yere gidiyor: `Layout.dock_rows` (çizim payı) ve `SessionOptions.dock`
  (caret'in sahipliği). İki alan, tek karar, bitişik iki satır.
- **Güvenlik yönü değişti ve bu bilinçli.** Danışman kapıyı "kabuk en az bir
  kez konuştu mu"ya bağlamayı önerdi; o, bozuk bir entegrasyonda ızgara
  imlecini ayakta tutuyor ama **ilk açılış karesini düzeltmeden bırakıyor** —
  yani şikâyetin kendisini. Seçilen değişmez daha güçlü bir güvence veriyor:
  **hiçbir hâlde caretsiz kalınmıyor**, çünkü ızgara bırakınca dock alıyor.
  - **Bilinen sınır:** entegrasyon kurulu ama betik sessizce ölürse (zsh
    `add-zle-hook-widget`'ı bulamazsa) caret dock'ta durur ve yazdıkça
    kıpırdamaz — yazı ızgarada belirir. Yanıltıcı ama **görünür**: aynı
    pencerede dock boş, prompt eski hâlinde ve blok şeritleri yok. Sessiz
    değil, o yüzden kabul edildi.

## Kabul

- Pencere açıldığı anda ızgarada **hiç** imleç yok; caret dock'ta.
- `Last login:` satırı yok; ızgara boş.
- Komut koşarken caret ızgarada (`sleep 5` sırasında, `vim` içinde).
- Komut bitince caret dock'a dönüyor — arada ızgarada bir kare bile
  görünmüyor.
- Alternatif ekranda (vim, htop, `less`) caret ızgarada ve dock yok.
- Ayna gösteremiyorsa (`Unavailable`) caret ızgarada — satır orada.
- **phase-7'den devreden gözle testi:** vim, htop, `less` gir/çık; `git log`'da
  dock yerinde kalıyor.

## Yayın Etkisi

- **`make kur` zorunlu** (`crates/bateri` yolu değişmiyor ama kabuk doğurma
  politikası `bt-shell`'de; paket koşusu bu yolu kullanıyor).
- **`make duman` zorunlu**: kabuk doğurma yolu değişti. Süreli koşu kendi
  komutunu veriyor (`smoke_shell`/`load_shell`), yani `-q` yolu ona
  **uğramıyor** ve `hucre=8 glif=6 kural=15` sözleşmesi dokunulmadan kalıyor.
- **`CLAUDE.md`:** caret'in tek sahibi kuralı ve `login -q`.
- Göç: kullanıcı `Last login:` satırını kaybediyor. Geri isteyen için bugün
  anahtar **yok** (ayar şeması bu setin işi değil); istenirse sonraki set.
- shader, terminfo, tema, app bundle: yok. Yeni bağımlılık: yok.

## Checklist

- [ ] `caret_home` tek yüklem; `dock::render` ve `Session::frame` onu tüketiyor
- [ ] Dock caret'i `Live` değilken de çiziyor (`TEXT_COL`)
- [ ] Izgara, caret'in sahibi değilken imleci çizmiyor
- [ ] `SessionOptions.dock` doğum kararından geliyor (tek ifade, iki tüketici)
- [ ] `login -q`: banner yok; çözülemeyen kullanıcı/kabuk `None`'a düşüyor
- [ ] Test: `Running`'de caret ızgarada, dock'ta yok
- [ ] Test: `Finished`/açılışta caret dock'ta, ızgarada yok
- [ ] Test: `Unavailable`'da caret ızgarada
- [ ] Test: `login` komutu `-q` taşıyor; çözülemeyen hâl `None`
- [ ] Gerçek pencerede gözle: açılış, komut, vim/htop/less (phase-7'den devir)
- [ ] Doğrulama geçti (`make hepsi` + `make kur` + `make duman`)
- [ ] Yayın etkisi yazıldı
