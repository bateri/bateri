---
name: ship
description: Değişiklikleri doğrulayıp commit'ler ve uzak depoya gönderir; biten iş setlerinin .tasks indeks satırını push'tan önce aynı commit'te 🟢 yapar. Kullanıcı "push et", "teslim et", "gönder", "commit'le ve yolla" dediğinde kullanılır.
allowed-tools: Read, Edit, Glob, Grep, Bash(make:*), Bash(cargo:*), Bash(git:*)
---

Kullanıcı çalışılanı uzak depoya göndermek istiyor.

**Zincir:** `/rfc` üretir → `/plan-review` sınar → `/implement` yürütür →
`/ship` teslim eder (bu skill).

Bu skill **projeye özgü hiçbir komutu gövdesinde taşımaz**; hepsi
`.claude/is-akisi/proje.md` içindedir (doğrulama komutları, branch akışı,
commit kuralları, türetilmiş dosya politikası). Başlamadan onu ve set düzeni
için `.claude/is-akisi/duzen.md`'yi oku.

**Kapsam:** teslim birimi **branch**'tir, iş seti değil — çalışma ağacındaki
her şey birlikte gider, sete göre seçmeli teslim yoktur.

## 1. Doğrula

`proje.md` → "Doğrulama" bölümündeki kapıyı koş. **Yeşil değilse teslim yok** —
neyin kırıldığını çıktıyla göster ve dur.

**Aynı oturumda yeşil koştuysa ve o koşudan beri yalnız derlenmeyen dosyalar
değiştiyse** (`.tasks/`, `docs/`, `CLAUDE.md`) kapıyı yeniden koşma; son
koşunun sonucunu kanıt olarak an. `crates/`, `assets/`, `Makefile`,
`Cargo.*` değiştiyse ya da rebase olduysa koş. Gerekçe: `/implement` kapıyı
phase commit'inden hemen önce koşuyor ve arada yalnız defter commit'i
kalıyor; aynı sonucu bir buçuk dakikaya ikinci kez almak doğrulama değil
tekrar (025'te kullanıcı sordu).

Koşullu komutları da burada uygula: `.metal` değiştiyse `make shader`,
`assets/terminfo` değiştiyse `make terminfo` koşmuş olmalı. `Cargo.lock`
değiştiyse bunun bilinçli bir bağımlılık kararı olduğunu (`discussion.md`
`## Karar` ya da phase notu) gör; görmüyorsan dur ve sor.

## 2. Bu teslimle ne gidiyor?

Dokunmadan önce keşfet:

- `git status`, `git log origin/main..HEAD --oneline`, `git diff --stat`
- Binen iş setlerini **iki sinyalle** bul, indekse tek başına güvenme:
  `.tasks/README.md`'deki 🔨 satırları **ve** commit'lerin dokunduğu `.tasks/`
  yolları (`git log origin/main..HEAD --name-only -- .tasks/`). İndeks bir kez
  bakımsız kalırsa ilk sinyal sessizce boş döner; ikincisi commit'lerin
  kendisinden gelir ve bakım gerektirmez.
- Bir set **yarım görünüyorsa** (`plan.md ## Durum` tablosunda `✅` olmayan
  phase var) bunu **açıkça uyar**: onun kısmi commit'leri de gidecek. Phase'ler
  bitmiş ama `kapı` satırı ✅ ya da gerekçeli `[~]` değilse set kapısı koşmamıştır
  (`proje.md` → Kalite kapısı) — uyar ve kullanıcıya sor.

**Özetle ve onay bekle.** Örnek: "Bu teslimle 2 iş gidiyor: 003-ek-fiil-indeksi
(3 phase + kapı), 005-ocr-onarim (1 phase, kapı `[~]`). Devam?"

## 3. Kirlilik denetimi

`git status`'ta `.DS_Store`, `target/`, `*.metallib`, `*.dSYM`, `*.dmg`,
`*.app`, `*.trace` ya da kişisel/geçici dosya (`~/.config/bateri` kopyası,
ekran kaydı) görürsen **uyar ve devam etme**. Commit'lemek yerine
`.gitignore` öner. `Cargo.lock` bilinçli olarak depodadır, kirlilik değildir.

## 4. İndeks ve commit

Adım 2'de bulunan, bütün phase'leri ✅ ve `kapı` satırı kapalı her set için
`.tasks/README.md` satırını **🟢** yap; not tek cümle kalır (`duzen.md` →
İndeks). Bu değişiklik push'tan **önce**, gidecek commit'e girer; push'tan
sonra ayrı bir damga commit'i atılmaz. Bekleyen ölçüm ya da ürün kararı 🟢'yi
ertelemez — onların yeri `docs/OLCUMLER.md` ve yol haritası.

Sonra commit: çalışma ağacı temizse (defter değişmediyse) atla — boş commit
üretme. Değilse `proje.md`'deki kurala göre mesaj yaz (Türkçe, emir kipinde,
tek satırlık özet). Push reddedilirse damga commit'i de onunla birlikte bekler;
ayrı düzeltme gerekmez.

`$ARGUMENTS` gerçek bir commit konusu gibi görünüyorsa (anlamlı bir cümle/öbek)
onu kullan. Kısa bir onay sözcüğüyse ("yap", "go", "tamam") "kullanıcı onayladı,
mesajı sen yaz" olarak yorumla ve değişikliklerden üret.

## 5. Push

**Önce niyeti ayır:** kullanıcı yalnız commit istediyse (`"şunu commit'le"`,
`"kaydet"`) burada **dur** — commit atıldı, push edilmedi, bunu söyle ve bitir.
Push dışa dönüktür ve geri alması pahalıdır; "gönder / push et / teslim et"
denmediyse yapılmaz.

`proje.md` → "Teslim" bölümündeki branch akışını izle. Bu depoda tek branch
vardır: `main` üzerinde commit, `git push origin main`.

Push reddedilirse (uzak ilerlemiş) **zorlama**: `git pull --rebase` ile
çakışmayı çöz, doğrulamayı (adım 1) **yeniden koş**, sonra gönder. Rebase
sonrası doğrulama atlanırsa birleşmiş kod hiç test edilmemiş olur.

## 6. Özet

Gönderilen commit'ler, değişen dosya sayısı, `Cargo.lock` oynadıysa hangi
crate ve ölçüm değiştiyse yeni değer.
