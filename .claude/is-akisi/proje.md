# Proje profili — bateri

İş akışı skill'lerinin **projeye özgü** kısmı burada durur: skill gövdeleri
"doğrula", "kapıdan geçir" der, hangi komut olduğunu burası söyler. Başka bir
projeye taşırken bu dosyayı (ve `/audit`, `/measure` merceklerini) yeniden yaz.

Proje sözleşmesinin tamamı `CLAUDE.md`'dedir ve **burada tekrarlanmaz**; her
ajan onu zaten yükler, iki kopya hem bağlam hem drift demektir.

> Girdisi henüz olmayan hedef: `terminfo` (shell/TERM seti). Koşunca "henüz
> yok" deyip kırmızı düşer. Tetiklenirse doğrulama "yeşil" değil "koşamadı"dır:
> `[~]` işaretlenir, phase bitmiş sayılmaz. Hedef gerçek olunca bu not silinir.

## İçindekiler

- Doğrulama (definition of done)
- Kalite kapısı
- Yayın etkisi
- Teslim
- Bu depoya özgü tuzaklar

## Doğrulama (definition of done)

Bir phase, doğrulama yeşil olmadan bitmiş sayılmaz.

| durum | komut |
|---|---|
| her phase | `make hepsi` — sürüm + `fmt` + `denetim` + `clippy -D warnings` + `test` |
| hızlı iç döngü | `cargo test -p {crate}` |
| `.metal` ya da `build.rs` değiştiyse | `make shader` — cargo'nun bayatlık takibini atlayan kanarya; derleme reçetesi yalnız `build.rs`'te |
| `assets/terminfo/*` değiştiyse | `make terminfo` — *henüz yok, bkz. üstteki not* |
| `assets/bundle/*`, `crates/bateri` ya da `kur` hedefi değiştiyse | `make kur` — **ürünü** denetler, düşerse çıkış 2; neyi denetlediği `Makefile`'ın `kur` yorumunda. İmza yok (006 Karar 6) |
| PTY okuyucu, render thread ya da paylaşılan duruma dokunulduysa | `make test-yaris` — iki zamanlama profili, ikisi de geçmeli. TSan nightly ister ve araç zinciri pin'li değil: "TSan koşmadı" waive değil, bilinen sınırdır |
| pencereyi açan davranış değiştiyse (giriş, çizim, sekme) | `make duman` — `kare`, `hucre`, `glif`, `kural` > 0 ve **`icerik`** ≤ `IDLE_FRAME_LIMIT` olmalı (üst sınırın operandı `kare` değil: animasyon `kare`'yi meşru olarak şişirir); shell sabit olduğu için `hucre=8 glif=6 kural=15` beklenir. Dördü de CPU sayacıdır: GPU'nun gerçekten boyadığını `make hepsi`'deki offscreen sınamalar, pencerenin görünürlüğünü hiçbiri kanıtlamaz. Jetonların anlamı `Makefile`'ın `duman` yorumunda ve `Report::token_line`'da. Başsız ortamda "ATLANDI" → `[~]`. `IDLE_FRAME_LIMIT` ölçülmüş bir sözleşmedir: değişikliği kod phase'lerinden **ayrı** commit'le iner, yoksa regresyonu maskeler |

**Türetilmiş dosya yoktur** (`default.metallib` `target/` altında kalır).
Buradaki karşılığı "`Cargo.lock` değişti mi"dir — değiştiyse ya kayıtlı bir
bağımlılık kararıdır ya da kusurdur; `make denetim` uyarır.

**Ölçüm bir kapı değildir** ve ölçülmemiş sayı yazılmaz (`CLAUDE.md`). Kare,
gecikme ya da bellek iddiası taşıyan phase `## Yayın Etkisi`'ne **"ölçüm
bekliyor: {ne}"** yazar ve devam eder; `/measure` oradan kapatır.

## Kalite kapısı

İki katman: ucuz olanı her phase'de, pahalı olanı sette bir kez.

**Her phase — doğrulama.** Yukarıdaki tablo. `make hepsi` `make denetim`'i
içerir; proje kurallarının mekanik yarısı (katman yönü, `bt-core`'da panik
yolu, rc dosyasına yazma, bağımlılık uyarısı) her phase'de ajansız koşar.

**Riskli phase — ayrıca `/code-review`.** Phase şunlardan birini tetiklediyse
kendi diff'i phase sonunda incelenir: `make test-yaris` gerekti (paylaşılan
durum), `make shader` gerekti (`#[repr(C)]` ↔ `.metal` düzeni), `Cargo.lock`
değişti. Bu üçünde hata sessizdir ve sonraki phase'ler onun üstüne kurulur;
geri kalan her şey set sonunu bekler. Liste doğrulama tablosundan türer, ayrı
tutulmaz.

**Set sonunda — bir kez** (son phase'den sonra, `teslim.md` derlenmeden önce):

1. `/code-review` — setin commit aralığı (`duzen.md` → Set aralığı).
2. `/audit` — `make denetim`'in kapsamadığı mercekler, yalnız ilgili dosya
   değiştiyse.
3. Bulgu düzeltildiyse `make hepsi` yeniden ve **tek** düzeltme commit'i.

`/simplify` kapının parçası değildir; kullanıcı isterse koşar. Kapının neden
phase başından set sonuna taşındığı `.claude/README.md` → Hafifletme.

### Kapıyı kim koşturur

**Ajan koşturur, `Skill` aracıyla ve ön planda.** Kapıyı arka planda başlatıp
yoklamak yasaktır (`implement/references/otonom-serit.md` → Ajan kuralları).
Skill çağrısı gerçekten hata verirse `code-reviewer` subagent'ı; o da olmazsa
**dur ve kullanıcıdan iste**. Waive *bulgu* içindir: kapı hiç koşmadıysa bu
waive değil, atlanmış kapıdır.

"Yok" demeden önce aramanın o şeyi bulabilecek türden olduğunu göster:
yerleşik skill'ler `.claude/skills/` altında durmaz.

### İz

| işaret | anlamı |
|---|---|
| `- [x]` | yapıldı |
| `- [~]` | **waive / atlandı** — yanına gerekçe |
| `- [ ]` | yapılmadı |

Phase'in izi checklist'indedir (doğrulama, riskli ise `/code-review`, yayın
etkisi). Set kapısının izi `plan.md → ## Durum` tablosunun `kapı` satırıdır.
Kutu silinmez: koşmayan kapının kutusu `[~]` ve gerekçesiyle durur, silinen
kutu atlandığını hiçbir yerde göstermez.

## Yayın etkisi

Phase dosyalarındaki `## Yayın Etkisi` bloğu şunları arar (hiçbiri yoksa "yok"):

- **shader** — `.metal` değiştiyse `make shader` koştu mu; uniform/vertex
  yapıları Rust `#[repr(C)]` karşılığıyla **alan alan** aynı mı
- **terminfo / `TERM`** — yetenek ya da ad değiştiyse `assets/terminfo`
  güncellendi mi, kurulum adımı (`[komut]`) teslim.md'ye düştü mü; uzak
  makinede geri düşüş (`xterm-256color`) korunmalı
- **ayar şeması** — anahtar eklendi/adı değiştiyse varsayılan değer,
  `docs/AYARLAR.md`, eski anahtarın akıbeti; **bilinmeyen anahtar asla silinmez**
- **tema / materyal biçimi** — paketli temaların hepsi ve kullanıcı tema
  dizini için geriye dönük okuma
- **shell entegrasyonu** — üç kabuk da (zsh, bash, fish) gözden geçirildi mi;
  kullanıcı rc dosyasına **dokunulmaz**
- **app bundle** — `Info.plist`, entitlements, `NS*UsageDescription`, imza
- ölçüm bekleyen iddia → **"ölçüm bekliyor: {ne}"**, sayı uydurma
- `CLAUDE.md` / crate'in `lib.rs` başlık yorumu güncellenmeli mi
- **yeni bağımlılık** — mimari karardır, kendiliğinden yapılmaz: dur ve sor

## Teslim

Tek branch: `main`. Dev branch, migration, container, panel yok.

- Commit iletisi **Türkçe, emir kipinde, tek satırlık özet**
  ("Glyph atlasını tek dokuya topla ve tahliyeyi ölç"). Set commit'inde
  gövdenin ilk satırı `{NNN-slug} phase-{N}` (set kapısında `{NNN-slug} kapı`):
  hash dosyaya yazılmaz, phase'in commit'i `git log --grep` ile bu satırdan
  bulunur.
- **Phase = tek commit:** kod, phase checklist'i, `plan.md ## Durum` ✅ ve
  (ilk phase'de) indeksin 🔨'ü birlikte girer. Defter için ayrı commit atılmaz.
- Push `/ship` kararıdır, `/implement` push etmez. `make hepsi` yeşil olmadan
  push yok.
- Depoya girmeyenler: `target/`, `*.metallib`, `*.dSYM`, `*.dmg`, `*.app`,
  `*.icns`, `*.iconset/`, `.DS_Store`. İkonun kaynağı
  (`assets/bundle/bateri.png`) ve `Cargo.lock` **girer**.

## Bu depoya özgü tuzaklar

Katman yönü, `bt-core`'un platformsuzluğu, boşta sıfır kare, hücre boyutu,
panik yolu, ayar anahtarları, ölçüm sahipliği ve dil kuralı `CLAUDE.md`'dedir.
İş akışında ayrıca akılda tutulacak ve orada yazmayanlar:

- **Renderer'a terminal semantiği eklenmez.** OSC/CSI ayrıştırma, komut
  blokları ve seçim modeli `bt-core`'dadır; `bt-gpu` "ne çizeceğini" alır, "ne
  anlama geldiğini" bilmez. Renderer'da escape dizisi tanıyan dal yanlış yerdedir.
- **`TERM` adı bir sözleşmedir.** Özel terminfo SSH'daki uzak makinede yoktur
  (Metalterm #23). Ya `xterm-256color` ile uyumlu kal ya da uzak tarafta geri
  düşüşü tasarla; ikisi de yapılmadan `TERM` değiştirilmez.
- **Araç zinciri pin'li değil.** Homebrew rustc, `rustup` ve
  `rust-toolchain.toml` bilinçli olarak yok (001). `brew upgrade` sonrası yeni
  bir clippy lint'i dokunulmamış kodu kırmızıya çevirebilir: `make hepsi`
  sürümü başta basar, kırmızıda önce sürüme bak.
