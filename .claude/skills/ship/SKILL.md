---
name: ship
description: Değişiklikleri kalite kapısından geçirip commit'ler ve uzak depoya gönderir; iş setlerinin teslim.md checklist'ini ve .tasks indeksini damgalar. Kullanıcı "push et", "teslim et", "gönder", "commit'le ve yolla" dediğinde kullanılır.
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
her şey birlikte gider, sete göre seçmeli teslim yoktur. teslim.md'nin
`[komut]`/`[elle]` adımları otomatik koşulmaz; adım 2'de görünür kılınır,
adım 7'de birlikte yürütmek teklif edilir.

## 1. Doğrula (önce, her zaman)

`proje.md` → "Doğrulama" bölümündeki kapıyı koş. **Yeşil değilse teslim yok** —
neyin kırıldığını çıktıyla göster ve dur.

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
- Binen her set için `teslim.md` varsa oku ve **bekleyen manuel adımları**
  topla — B bölümünün `### Yayın Checklist` başlığı altındaki işaretsiz
  `[komut]`/`[elle]` maddeleri.
  teslim.md'si olmayan set "standart, manuel adım yok" sayılır.
- Bir set **yarım görünüyorsa** (`plan.md ## Durum` tablosunda `✅` olmayan
  phase var) bunu **açıkça uyar**: onun kısmi commit'leri de gidecek.

**Özetle ve onay bekle.** Örnek: "Bu teslimle 2 iş gidiyor: 003-ek-fiil-indeksi
(2 `[elle]` adımı bekliyor — ölçüm `docs/OLCUMLER.md`'ye işlenecek),
005-ocr-onarim (no-op). Devam?"

## 3. Kirlilik denetimi

`git status`'ta `.DS_Store`, `target/`, `*.metallib`, `*.dSYM`, `*.dmg`,
`*.app`, `*.trace` ya da kişisel/geçici dosya (`~/.config/bateri` kopyası,
ekran kaydı) görürsen **uyar ve devam etme**. Commit'lemek yerine
`.gitignore` öner. `Cargo.lock` bilinçli olarak depodadır, kirlilik değildir.

## 4. Commit

Çalışma ağacı zaten temizse (`/implement` her şeyi commit'lemiş olabilir) bu
adımı **atla** — boş commit üretme.

Değilse: dosyaları hazırla ve `proje.md`'deki commit kuralına göre mesaj yaz
(Türkçe, emir kipinde, tek satırlık özet; gövdede ne değişti).

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

## 7. Set defteri — adım 2'de bulunan HER set için

- **teslim.md'yi damgala:** az önce koşulan `[oto]` maddelerini `[x]` yap.
  `[komut]`/`[elle]` maddeleri işaretsiz kalır — koşulmadılar.
- **`.tasks/README.md`'yi güncelle:** setin bekleyen manuel adımı yoksa
  satırını **🟢 bitti** yap ve notu güncelle. Bekleyen adım varsa notu
  "🔨 teslim bekliyor: {kalan adımlar}" yap — gerçekten bitmeden 🟢 verme.
- **Kalan dilimi bitirmeyi teklif et:** "İstersen kalan adımları birlikte
  yürütelim — `[komut]` adımlarını tek tek onaylatarak ben koşarım, `[elle]`
  adımlarında ne yapılacağını tarif ederim, sen tamamlayınca doğrulamayı
  koştururum." Kabul edilirse teslim.md sırasını izle, tamamlanan her maddeyi
  `[x]` yap ve checklist bitince README satırını 🟢'ye çevir.
