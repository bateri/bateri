# Phase 10 — İki anahtar tek anahtara: prompt sizinse dock da yok

## Özet

`prompt` anahtarını **emekliye ayır** ve taşıdığı seçimi `integration`'ın üçüncü
değerine taşı. "Prompt kullanıcının" demek zaten "sarmalayıcı kurulu ama giriş
satırı ızgarada" demek; ikisini ayrı anahtar yapmak ekranda **iki prompt**
üretiyordu.

_Requirements: R4.3 (yeniden yazılıyor)_

## Bağlam

Kullanıcı `prompt = "shell"`'i gerçek pencerede denedi ve iki kare gönderdi:

1. Boş ekranda kendi prompt'u (`→ ~`) ızgarada, imleç onun yanında — **ve
   altında dock duruyor**, kendi `>` işareti ve bağlam satırıyla.
2. `echo` yazınca kullanıcının prompt'u **büsbütün kayboldu**, imleç dock'a
   atladı.

Kullanıcının cümlesi: *"shell modunda ekran boşken prompt kısmında duruyor imleç
ama dock var, ve bir şey yazdığında dock'a imleç geçiyor. bu çok saçma."*

İki ayrı kusur var ve **ikincisi birincinin sebebi değil**:

- **Mekanik:** bastırma satır geneli (`from..=to` bütün sütunları atlıyor) ve
  çıpa kullanıcının `PS1`'inin **önüne** ekleniyor (`bateri.zsh:333`), yani
  `PS1` bastırılan satırda kalıyor. Tazelik kapısı da ızgara satırının
  tamamının son mürekkebini aynanınkiyle karşılaştırıyor, oysa ayna prompt'u
  hiç taşımıyor: boş promptta "bayat" (ızgarada `~`, aynada hiçbir şey), ilk
  tuşta "taze". Yanıp sönme buradan.
- **Tasarım:** mekanik tamir edilse bile **iki prompt** kalıyor. Dock'un alt
  satırı zaten yol ve dal gösteriyor, yani p10k/starship'in yaptığı işin
  çoğunu; üstüne kullanıcının prompt'u ızgarada duruyor ve caret'in mantıklı
  bir evi yok.

Yani düzeltilecek olan kod değil **karar**: Karar 7a ayrı anahtar seçmişti ve
gerekçesi doğruydu ("prompt'unu geri isteyenin tek çıkışının entegrasyonu
büsbütün kapatmak olması orantısız"), ama seçtiği mekanizma hedefi tutturmadı.

## Kararlar

- **Karar 7a geri alınıyor.** Amacı korunuyor — prompt'unu geri isteyen
  kullanıcı bloklarını kaybetmeyecek — mekanizması değişiyor: ayrı anahtar
  değil, `integration`'ın üçüncü değeri.
- **Üç değerli tek anahtar.** `auto` (varsayılan) = sarmalayıcı + dock +
  terminalin prompt'u; **`blocks`** = sarmalayıcı + bloklar ve komut
  işaretleri, dock **yok**, prompt kullanıcının; `off` = hiçbiri.
- **Orta kademe uydurulmuyor, zaten var.** bash (`--rcfile`) ve fish
  (`vendor_conf.d`) betikleri doğduğunda o kabuklarda işaretler olacak ama
  dock olmayacak — dock ZLE'nin aynasına bağlı. Yani "entegrasyon var, dock
  yok" hâli yapının kendisi; bu karar yalnız zsh kullanıcısına **seçme** hakkı
  veriyor.
- **Adı `"blocks"` (kullanıcı seçti).** `"marks"` ve `"minimal"` elendi:
  ilki alt katmanın adı ve kullanıcı "mark" kelimesini ekranda hiç görmüyor,
  ikincisi ne aldığını söylemiyor. `"blocks"` kullanıcının **gördüğü** şeyi
  adlandırıyor ve Metalterm'in de kendi kelimesi.
- **`prompt` silinmiyor, okunmuyor.** Ayar dosyasında kalan anahtar korunur
  (deponun kuralı: *bilinmeyen anahtar korunur, anahtar silinmez*). Değeri
  artık hiçbir şeyi değiştirmiyor ve bu **tanıya** düşüyor: emekli anahtar
  sessizce yok sayılmaz, alt başlıkta söylenir.
- **Mekanik kusurlar bu phase'de kapanmıyor.** Tazelik kapısının prompt
  hücrelerini sayması ve bastırmanın satır geneli olması `docs/YOL-HARITASI.md`
  → "Sete bağlanmamış borçlar"da duruyor. Orta kademede dock hiç doğmadığı
  için ikisi de **erişilemez** hâle geliyor; gerçek çareleri kendi setinde.

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — `ShellIntegration` üçüncü varyantı
  alır; `Prompt` enum'u, `prompt()` ayrıştırıcısı ve `Settings.prompt` alanı
  kalkar. Şablondan `prompt = "terminal"` satırı çıkar, `integration`'ın
  yorumu üç değeri anlatır. Emekli anahtar için tanı: `shell.prompt` görülürse
  "artık okunmuyor, `integration`'a bakın" der.
- **`crates/bt-shell/src/app.rs`** — `shell_integration_env` `Prompt`
  parametresini bırakır; `BATERI_PROMPT` orta kademede gönderilir.
  `dock_rows_at_birth` **orta kademede sıfır** olur: dock'u olmayan pencerede
  haberci zaten kurulmuyor, yani alternatif ekran yolu da yapısal olarak
  kapalı kalır.
- **`assets/shell/zsh/bateri.zsh`** — `BATERI_PROMPT` kolunun gerekçesi
  değişir (artık "kullanıcı prompt'u istedi" değil, "bu oturumda dock yok").
  Çıpa ve `B` eki **her iki kademede de** basılır: bloklar orta kademenin
  varlık sebebi.
- **`docs/AYARLAR.md`** — `prompt` bölümü kalkar, `integration` üç değeri
  anlatır; kurtarma bölümü artık orta kademeyi gösterir.
- **`CLAUDE.md`** — `settings.toml` envanterinden `shell.prompt` çıkar; "prompt
  terminalin" paragrafı geri dönüşün yeni adını söyler.

## Kabul

- Orta kademede pencere **dock'suz doğuyor** (`dock_rows_at_birth == 0`):
  ızgarada imleç, kullanıcının `PS1`'i olduğu gibi, caret hiç dock'a gitmiyor.
- Aynı pencerede **bloklar ve komut işaretleri çalışıyor** (OSC 133 geliyor,
  şerit çiziliyor) — orta kademenin bütün gerekçesi bu.
- `auto`'da bugünkü davranış **birebir** aynı.
- `off`'ta hiçbir şey yok.
- Ayar dosyasında kalan `prompt` anahtarı korunuyor, davranışı değiştirmiyor ve
  alt başlıkta emekli olduğu söyleniyor.
- Kullanıcının iki karesi tekrar edilince: ekranda **tek** prompt var.

## Yayın Etkisi

- **Ayar şeması: bir anahtar emekli.** `prompt` okunmuyor ama **silinmiyor**;
  eski `settings.toml` aynen açılıyor. `integration` üçüncü değeri kabul
  ediyor, iki eski değer dokunulmadan çalışıyor — yani geriye dönük okuma
  sorunu yok, yalnız ileriye dönük bir davranış değişikliği var ve o da
  kullanıcının **açıkça seçtiği** kademede.
- **`make kur` zorunlu** (`assets/shell/*` değişiyor).
- **`make duman`**: reçete `/bin/sh` koşuyor ve zaten dock almıyor, yani jeton
  sözleşmesi (`hucre=8 glif=6 kural=15`) dokunulmadan kalıyor. Yine de kabuk
  doğurma yolu değiştiği için koşulur.
- **Göç:** `prompt = "shell"` yazmış kullanıcı (bugün yalnız bu depo sahibi)
  satırı `integration`'a taşımalı; eski satır zarar vermiyor ama bir şey de
  yapmıyor. Sürüm notunda adıyla.
- **`docs/AYARLAR.md` ve `CLAUDE.md`** aynı commit'te.
- shader, terminfo, tema, yeni bağımlılık: yok. Ölçüm bekleyen iddia: yok.

## Checklist

- [x] Orta kademenin adı kullanıcıyla seçildi
- [x] `ShellIntegration` üç değerli; `Prompt` ve tüketicileri kalktı
- [x] Test: emekli `prompt` anahtarı korunuyor ve tanı bırakıyor
- [x] Test: üç kademenin `shell_integration_env` çıktısı (bugünkü `Prompt`
      sınamalarının yerine)
- [x] Orta kademede `dock_rows_at_birth == 0`
- [x] Test: orta kademede dock payı ayrılmıyor — `blocks_keeps_the_wrapper_and_drops_the_dock`
      (pay sıfırsa oturum `dock: false` doğuyor, yani `caret_in_dock` yapısal
      olarak `false` ve dock hiç çizilmiyor)
- [x] Betiğin çıpası ve `B` eki her iki kademede de basılıyor
- [x] `docs/AYARLAR.md` + `CLAUDE.md` aynı commit'te
- [ ] Gerçek pencerede gözle: üç kademe, kullanıcının iki karesi tekrar edildi
- [ ] Doğrulama geçti (`make hepsi` + `make kur` + `make duman`)
- [x] Yayın etkisi yazıldı
