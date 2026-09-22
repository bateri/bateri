---
name: ship
description: Değişiklikleri doğrulayıp commit'ler ve uzak depoya gönderir; 🟢'yi /implement koyar, /ship yalnız eksikse tamamlar. Kullanıcı "push et", "teslim et", "gönder", "commit'le ve yolla" dediğinde kullanılır.
allowed-tools: Read, Edit, Glob, Grep, Bash(git:*)
---

Kullanıcı çalışılanı uzak depoya göndermek istiyor.

**Zincir:** `/rfc` üretir → `/plan-review` sınar → `/implement` yürütür →
`/ship` teslim eder (bu skill).

Bu skill **projeye özgü hiçbir komutu gövdesinde taşımaz**: komutlar, dosya
sınıfları, dal ve commit dili `.claude/is-akisi/proje.md`'de; set düzeni ve
teslim kuralları `.claude/is-akisi/duzen.md`'de. Başlamadan ikisini oku.

**Kapsam:** teslim birimi **branch**'tir, iş seti değil — çalışma ağacındaki
her şey birlikte gider, sete göre seçmeli teslim yoktur.

## 1. Doğrula

`proje.md` → Doğrulama'daki kapı komutunu koş. **Yeşil değilse teslim yok** —
neyin kırıldığını çıktıyla göster ve dur.

**Aynı oturumda yeşil koştuysa ve o koşudan beri yalnız derlenmeyen dosyalar
değiştiyse** (`proje.md` → Dosya sınıfları) kapıyı yeniden koşma; son koşunun
sonucunu kanıt olarak an. Başka bir şey değiştiyse ya da rebase olduysa koş.
Gerekçe: `/implement` kapıyı son phase'in commit'inden hemen önce koşuyor ve
o commit'ten sonra kod değişmiyor; aynı sonucu ikinci kez almak doğrulama
değil tekrar.

Tetiklenen koşullu komutlar (`proje.md` → Doğrulama) koşmuş olmalı. Kilit
dosyası değiştiyse bunun bilinçli bir bağımlılık kararı olduğunu
(`discussion.md` `## Karar` ya da phase notu) gör; görmüyorsan dur ve sor.

## 2. Bu teslimle ne gidiyor?

Dokunmadan önce keşfet:

- `git status`, `git log @{u}..HEAD --oneline`, `git diff --stat`
- Binen iş setlerini **iki sinyalle** bul, indekse tek başına güvenme:
  `.tasks/README.md`'deki 🔨 satırları **ve** commit'lerin dokunduğu `.tasks/`
  yolları (`git log @{u}..HEAD --name-only -- .tasks/`). İndeks bir kez
  bakımsız kalırsa ilk sinyal sessizce boş döner; ikincisi commit'lerin
  kendisinden gelir ve bakım gerektirmez.
- Bir set **yarım görünüyorsa** (`plan.md ## Durum` tablosunda `✅` olmayan
  phase var) bunu **açıkça uyar**: onun kısmi commit'leri de gidecek. Phase'ler
  bitmiş ama `kapı` satırı ✅ ya da gerekçeli `[~]` değilse set kapısı koşmamıştır
  (`duzen.md` → Kalite kapısı) — uyar ve kullanıcıya sor.

**Özetle ve onay bekle.** Örnek: "Bu teslimle 2 iş gidiyor: 003-ornek-is
(3 phase + kapı), 005-diger-is (1 phase, kapı `[~]`). Devam?"

## 3. Kirlilik denetimi

`git status`'ta depoya girmeyen bir dosya ya da kişisel/geçici dosya
(`proje.md` → Dosya sınıfları) görürsen **uyar ve devam etme**. Commit'lemek
yerine `.gitignore` öner. Kilit dosyası bilinçli olarak depodadır, kirlilik
değildir.

## 4. İndeks ve commit

🟢'yi `/implement` son phase'in commit'inde koyuyor (`duzen.md` → İndeks);
burada yalnız **kontrol** edilir. Adım 2'de bulunan, bütün phase'leri ✅ ve
`kapı` satırı kapalı bir set hâlâ 🔨 ise (eski akışla kapanmış set, elle
düzeltilmiş kapı) satırı 🟢 yap; bu tek istisna gidecek commit'e girer.
Bekleyen ölçüm ya da ürün kararı 🟢'yi ertelemez — onların yeri ölçüm
defteri ve sıra belgesi (`proje.md` → Belgeler).

Sonra commit: çalışma ağacı temizse atla — boş commit üretme. Olağan akışta
ağaç temizdir ve `/ship` hiç commit atmaz, yalnız gönderir. Değilse
`duzen.md` → Teslim'e ve `proje.md` → Teslim'deki dile göre tek satırlık
özet yaz.

`$ARGUMENTS` gerçek bir commit konusu gibi görünüyorsa (anlamlı bir cümle/öbek)
onu kullan. Kısa bir onay sözcüğüyse ("yap", "go", "tamam") "kullanıcı onayladı,
mesajı sen yaz" olarak yorumla ve değişikliklerden üret.

## 5. Push

**Önce niyeti ayır:** kullanıcı yalnız commit istediyse (`"şunu commit'le"`,
`"kaydet"`) burada **dur** — commit atıldı, push edilmedi, bunu söyle ve bitir.
Push dışa dönüktür ve geri alması pahalıdır; "gönder / push et / teslim et"
denmediyse yapılmaz.

`proje.md` → Teslim'deki dal ve push komutunu kullan.

Push reddedilirse (uzak ilerlemiş) **zorlama**: `git pull --rebase` ile
çakışmayı çöz, doğrulamayı (adım 1) **yeniden koş**, sonra gönder. Rebase
sonrası doğrulama atlanırsa birleşmiş kod hiç test edilmemiş olur.

## 6. Özet

Gönderilen commit'ler, değişen dosya sayısı, kilit dosyası oynadıysa hangi
bağımlılık ve ölçüm değiştiyse yeni değer.
